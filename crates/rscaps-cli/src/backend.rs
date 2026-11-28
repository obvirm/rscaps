//! Pemetaan backend runtime untuk CLI: ASR, caption render, video encode.
//!
//! Satu titik parsing/normalisasi supaya `frames`, `render`, dan (nanti)
//! Tauri memakai nama opsi yang sama. Backend yang belum diimplementasikan
//! ditolak eksplisit — tidak ada fallback diam.

use anyhow::{Result, bail};
use std::collections::HashMap;
use std::sync::Arc;

/// Output kontrak backend render. GPU boleh mempertahankan texture sampai
/// encoder GPU; readback ke `CpuRgba` hanya untuk encoder CPU/harness.
#[allow(dead_code)]
pub enum RenderedFrame {
    CpuRgba(Vec<u8>),
    GpuTexture(GpuTextureHandle),
}

#[allow(dead_code)]
pub struct GpuTextureHandle {
    pub id: u64,
    pub width: u32,
    pub height: u32,
}

/// Kontrak minimum backend caption. Implementasi boleh menambah method
/// khusus (mis. akses `wgpu::Texture`) tanpa mengubah trait ini.
#[allow(dead_code)]
pub trait RenderBackend {
    fn name(&self) -> &'static str;
    fn render(&mut self, svg: &str, width: u32, height: u32) -> Result<RenderedFrame>;
}

/// Adapter CPU deterministik: `usvgr` parse + `tiny-skia` raster, kontrak
/// image data sama dengan `svgr-wasm` (kaca selene/iris/pastor).
pub struct TinySkiaBackend<'a> {
    fontdb: &'a svgr::usvgr::fontdb::Database,
    images: &'a HashMap<String, Arc<svgr::usvgr::PreloadedImageData>>,
    cache: &'a mut svgr::SvgrCache,
    pool: &'a svgr::PixmapPool,
}

impl<'a> TinySkiaBackend<'a> {
    pub fn new(
        fontdb: &'a svgr::usvgr::fontdb::Database,
        images: &'a HashMap<String, Arc<svgr::usvgr::PreloadedImageData>>,
        cache: &'a mut svgr::SvgrCache,
        pool: &'a svgr::PixmapPool,
    ) -> Self {
        Self {
            fontdb,
            images,
            cache,
            pool,
        }
    }

    pub fn render_pixmap(
        &mut self,
        svg: &str,
        width: u32,
        height: u32,
    ) -> Result<svgr::tiny_skia::Pixmap> {
        let opt = svgr::usvgr::Options {
            image_data: Some(self.images),
            ..Default::default()
        };
        let tree = svgr::usvgr::Tree::from_str(svg, &opt, self.fontdb)
            .map_err(|err| anyhow::anyhow!("parse svg: {err}"))?;
        let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| anyhow::anyhow!("gagal alokasi pixmap {width}×{height}"))?;
        let ctx = svgr::Context::new_from_pixmap(&pixmap);
        svgr::render(
            &tree,
            svgr::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
            self.cache,
            self.pool,
            &ctx,
        );
        Ok(pixmap)
    }
}

impl RenderBackend for TinySkiaBackend<'_> {
    fn name(&self) -> &'static str {
        "tiny-skia"
    }

    fn render(&mut self, svg: &str, width: u32, height: u32) -> Result<RenderedFrame> {
        let pixmap = self.render_pixmap(svg, width, height)?;
        Ok(RenderedFrame::CpuRgba(pixmap.data().to_vec()))
    }
}

/// Adapter Skia (GPU Vulkan atau CPU eksplisit). Surface bertahan antar
/// frame; output tetap `CpuRgba` karena encoder downstream bisa x264.
pub struct SkiaBackend {
    kind: &'static str,
    inner: crate::render::SkiaRasterizer,
}

#[cfg(feature = "gpu-skia")]
impl SkiaBackend {
    pub fn new_gpu(
        width: u32,
        height: u32,
        fontdb: &svgr::usvgr::fontdb::Database,
    ) -> Result<Self> {
        Ok(Self {
            kind: "skia-gpu",
            inner: crate::render::SkiaRasterizer::new_gpu(width, height, fontdb)?,
        })
    }

    pub fn new_cpu(
        width: u32,
        height: u32,
        fontdb: &svgr::usvgr::fontdb::Database,
    ) -> Result<Self> {
        Ok(Self {
            kind: "skia-cpu",
            inner: crate::render::SkiaRasterizer::new_cpu(width, height, fontdb)?,
        })
    }

    pub fn make_renderer(&self) -> crate::render::SkiaRendererOwned<'_> {
        self.inner.make_renderer()
    }
}

#[cfg(not(feature = "gpu-skia"))]
impl SkiaBackend {
    pub fn new_gpu(_: u32, _: u32, _: &svgr::usvgr::fontdb::Database) -> Result<Self> {
        bail!("engine skia-gpu butuh build ulang: cargo build -p rscaps-cli --features gpu-skia")
    }

    pub fn new_cpu(_: u32, _: u32, _: &svgr::usvgr::fontdb::Database) -> Result<Self> {
        bail!("engine skia-cpu butuh build ulang: cargo build -p rscaps-cli --features gpu-skia")
    }

    pub fn make_renderer(&self) -> crate::render::SkiaRendererOwned<'_> {
        self.inner.make_renderer()
    }
}

impl RenderBackend for SkiaBackend {
    fn name(&self) -> &'static str {
        self.kind
    }

    fn render(&mut self, svg: &str, width: u32, height: u32) -> Result<RenderedFrame> {
        #[cfg(feature = "gpu-skia")]
        {
            let mut renderer = self.inner.make_renderer();
            let rgba = renderer.render_frame_rgba(svg, width, height)?;
            Ok(RenderedFrame::CpuRgba(rgba))
        }
        #[cfg(not(feature = "gpu-skia"))]
        {
            let _ = (svg, width, height);
            bail!("engine skia butuh build ulang: cargo build -p rscaps-cli --features gpu-skia")
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrBackend {
    Auto,
    Cpu,
    Cuda,
    Dml,
    Vulkan,
    Boost,
}

#[allow(dead_code)]
impl AsrBackend {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" => Ok(Self::Cpu),
            "cuda" => Ok(Self::Cuda),
            "dml" | "directml" => Ok(Self::Dml),
            "vulkan" | "dawn" | "webgpu" => Ok(Self::Vulkan),
            "boost" => Ok(Self::Boost),
            other => {
                bail!("asr backend tidak dikenal: {other} (pakai auto|cpu|cuda|dml|vulkan|boost)")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionBackend {
    Auto,
    TinySkia,
    SkiaGpu,
    SkiaCpu,
    Fframes,
    FframesPipeline,
    WgpuLyon,
}

impl CaptionBackend {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" | "tiny-skia" | "tiny_skia" | "usvgr" => Ok(Self::TinySkia),
            "skia" | "skia-gpu" | "skia_gpu" => Ok(Self::SkiaGpu),
            "skia-cpu" | "skia_cpu" => Ok(Self::SkiaCpu),
            "fframes" => Ok(Self::Fframes),
            "fframes-pipeline" => Ok(Self::FframesPipeline),
            "wgpu-lyon" | "wgpu_lyon" => {
                #[cfg(feature = "wgpu-lyon")]
                {
                    Ok(Self::WgpuLyon)
                }
                #[cfg(not(feature = "wgpu-lyon"))]
                {
                    bail!(
                        "caption backend wgpu-lyon butuh build ulang: cargo build -p rscaps-cli --features wgpu-lyon"
                    )
                }
            }
            "vello-cpu" | "vello_cpu" | "wgpu-vello" | "wgpu_vello" => {
                bail!(
                    "caption backend {raw} belum tersedia di build ini; pakai auto|cpu|skia-gpu|skia-cpu|fframes|wgpu-lyon"
                )
            }
            other => bail!(
                "caption backend tidak dikenal: {other} (pakai auto|cpu|tiny-skia|skia-gpu|skia-cpu|fframes|fframes-pipeline)"
            ),
        }
    }

    /// `auto` mengunci satu kandidat: Skia GPU bila fitur build tersedia,
    /// selain itu tiny-skia. Kegagalan inisialisasi kandidat terpilih adalah
    /// hard error (bukan fallback ke CPU).
    pub fn resolve(self) -> Result<Self> {
        match self {
            Self::Auto => {
                if cfg!(feature = "gpu-skia") {
                    Ok(Self::SkiaGpu)
                } else {
                    Ok(Self::TinySkia)
                }
            }
            other => Ok(other),
        }
    }

    /// Nama internal lama yang dipakai jalur render yang sudah ada.
    pub fn engine_arg(self) -> &'static str {
        match self {
            Self::TinySkia => "cpu",
            Self::SkiaGpu => "skia",
            Self::SkiaCpu => "skia-cpu",
            Self::Fframes => "fframes",
            Self::FframesPipeline => "fframes-pipeline",
            Self::WgpuLyon => "wgpu-lyon",
            Self::Auto => "auto",
        }
    }

    pub fn is_skia(self) -> bool {
        matches!(self, Self::SkiaGpu | Self::SkiaCpu)
    }

    pub fn is_wgpu(self) -> bool {
        matches!(self, Self::WgpuLyon)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeBackend {
    Auto,
    Cpu,
    Nvenc,
    Qsv,
    Amf,
    Vaapi,
    Videotoolbox,
}

impl EncodeBackend {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" | "x264" | "libx264" => Ok(Self::Cpu),
            "nvenc" | "h264_nvenc" => Ok(Self::Nvenc),
            "qsv" | "h264_qsv" => Ok(Self::Qsv),
            "amf" | "h264_amf" => Ok(Self::Amf),
            "vaapi" | "h264_vaapi" => Ok(Self::Vaapi),
            "videotoolbox" | "h264_videotoolbox" => Ok(Self::Videotoolbox),
            other => bail!(
                "encode backend tidak dikenal: {other} (pakai auto|cpu|nvenc|qsv|amf|vaapi|videotoolbox)"
            ),
        }
    }

    pub fn hw_encoder(self) -> Option<&'static str> {
        match self {
            Self::Nvenc => Some("h264_nvenc"),
            Self::Qsv => Some("h264_qsv"),
            Self::Amf => Some("h264_amf"),
            Self::Vaapi => Some("h264_vaapi"),
            Self::Videotoolbox => Some("h264_videotoolbox"),
            Self::Auto | Self::Cpu => None,
        }
    }
}

/// Kandidat `auto` encode, urut prioritas. Hanya kandidat yang benar-benar
/// terdaftar di build ffmpeg yang dipakai.
pub const AUTO_ENCODERS: &[&str] = &[
    "h264_nvenc",
    "h264_qsv",
    "h264_amf",
    "h264_vaapi",
    "h264_videotoolbox",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_aliases() {
        assert_eq!(
            CaptionBackend::parse("cpu").unwrap(),
            CaptionBackend::TinySkia
        );
        assert_eq!(
            CaptionBackend::parse("tiny-skia").unwrap(),
            CaptionBackend::TinySkia
        );
        assert_eq!(
            CaptionBackend::parse("skia").unwrap(),
            CaptionBackend::SkiaGpu
        );
        assert_eq!(
            CaptionBackend::parse("skia-cpu").unwrap(),
            CaptionBackend::SkiaCpu
        );
        #[cfg(feature = "wgpu-lyon")]
        assert_eq!(
            CaptionBackend::parse("wgpu-lyon").unwrap(),
            CaptionBackend::WgpuLyon
        );
        #[cfg(not(feature = "wgpu-lyon"))]
        assert!(CaptionBackend::parse("wgpu-lyon").is_err());
    }

    #[test]
    fn encode_aliases() {
        assert_eq!(
            EncodeBackend::parse("nvenc").unwrap().hw_encoder(),
            Some("h264_nvenc")
        );
        assert_eq!(EncodeBackend::parse("cpu").unwrap().hw_encoder(), None);
        assert!(EncodeBackend::parse("bad").is_err());
    }

    #[test]
    fn tiny_skia_backend_renders_rgba() {
        let db = svgr::usvgr::fontdb::Database::new();
        let images = HashMap::new();
        let mut cache = svgr::SvgrCache::none();
        let pool = svgr::PixmapPool::new();
        let mut backend = TinySkiaBackend::new(&db, &images, &mut cache, &pool);
        let frame = backend
            .render(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#ff0000"/></svg>"##,
                4,
                4,
            )
            .unwrap();
        match frame {
            RenderedFrame::CpuRgba(px) => {
                assert_eq!(px.len(), 4 * 4 * 4);
                assert!(px.chunks_exact(4).all(|p| p[3] == 255));
            }
            RenderedFrame::GpuTexture(_) => panic!("tiny-skia harus CPU"),
        }
    }
}
