//! Fase 2 — burn-in caption ke video: probe (ffprobe) → frame caption per
//! frame (caption-core) → pipe rawvideo RGBA → ffmpeg overlay → mp4.
//!
//! Logika caption 100% caption-core; ffmpeg hanya compositor/muxer.
//! Raster engine `cpu` berjalan PARALEL (rayon, semua thread logis); `skia`
//! serial dengan renderer GPU yang bertahan antar frame.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail, ensure};
use rayon::prelude::*;

use crate::doc;
use crate::fonts::CliFontSource;
use caption_core::CaptionDocument;
use fframes::{FFramesContext, FFramesMode, TimeBase, VideoSize};

#[derive(Debug, Clone)]
pub struct VideoProbe {
    pub width: u32,
    pub height: u32,
    /// pembilang/denyut frame rate (mis. 30000/1001)
    pub fps_num: u32,
    pub fps_den: u32,
    pub duration_sec: f64,
    pub has_audio: bool,
}

impl VideoProbe {
    pub fn fps_f64(&self) -> f64 {
        self.fps_num as f64 / self.fps_den as f64
    }

    pub fn fps_str(&self) -> String {
        format!("{}/{}", self.fps_num, self.fps_den)
    }
}

/// Cari binari ffmpeg/ffprobe: env `RSCAPS_FFMPEG_DIR` → PATH → fallback lokal.
pub fn tool_bin(name: &str, extra_dir: Option<&Path>) -> Result<PathBuf> {
    let exe = format!("{name}.exe");
    if let Some(dir) = extra_dir {
        let p = dir.join(&exe);
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Ok(dir) = std::env::var("RSCAPS_FFMPEG_DIR") {
        let p = PathBuf::from(dir).join(&exe);
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(&exe);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    let local = PathBuf::from(
        "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin",
    )
    .join(&exe);
    if local.is_file() {
        return Ok(local);
    }
    bail!(
        "{name} tidak ditemukan — set PATH, env RSCAPS_FFMPEG_DIR, atau flag --ffmpeg-dir"
    )
}

pub fn probe_video(ffprobe: &Path, video: &Path) -> Result<VideoProbe> {
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=width,height,r_frame_rate,codec_type:format=duration",
            "-of",
            "json",
        ])
        .arg(video)
        .output()
        .with_context(|| format!("jalankan {}", ffprobe.display()))?;
    ensure!(
        out.status.success(),
        "ffprobe gagal: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;

    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = (30u32, 1u32);
    let mut has_audio = false;
    let empty = Vec::new();
    for s in json["streams"].as_array().unwrap_or(&empty) {
        if s["codec_type"] == "video" && width == 0 {
            width = s["width"].as_u64().unwrap_or(0) as u32;
            height = s["height"].as_u64().unwrap_or(0) as u32;
            if let Some(rate) = s["r_frame_rate"].as_str() {
                if let Some((n, d)) = rate.split_once('/') {
                    fps = (
                        n.parse().unwrap_or(30),
                        d.parse().unwrap_or(1).max(1),
                    );
                }
            }
        }
        if s["codec_type"] == "audio" {
            has_audio = true;
        }
    }
    let duration_sec: f64 = json["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);
    ensure!(width > 0 && height > 0, "video tidak punya stream video");
    ensure!(duration_sec > 0.0, "durasi video tidak valid");

    Ok(VideoProbe {
        width,
        height,
        fps_num: fps.0,
        fps_den: fps.1,
        duration_sec,
        has_audio,
    })
}

/// Triple encoder GPU yang didukung `--hw-encoder` + cek ketersediaan di build
/// ffmpeg ini (dicari di daftar `-encoders`).
pub fn hw_encoder_available(ffmpeg: &Path, encoder: &str) -> bool {
    Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(encoder))
        .unwrap_or(false)
}

/// Argumen ffmpeg untuk encoder hardware (nama flag kualitas beda per vendor;
/// nvenc = cq, qsv = global_quality, vaapi = qp, videotoolbox = q:v, amf = qp_*).
pub fn hw_encode_args(encoder: &str, crf: u32, preset: &str) -> Vec<String> {
    let crf = crf.to_string();
    let mut a = vec!["-c:v".to_string(), encoder.to_string()];
    match encoder {
        "h264_qsv" => a.extend(["-global_quality".to_string(), crf, "-preset".to_string(), preset.to_string()]),
        "h264_vaapi" => a.extend(["-qp".to_string(), crf]),
        "h264_videotoolbox" => a.extend(["-q:v".to_string(), crf]),
        "h264_amf" => a.extend(["-qp_i".to_string(), crf.clone(), "-qp_p".to_string(), crf]),
        // h264_nvenc + default: cq + preset.
        _ => a.extend(["-cq".to_string(), crf, "-preset".to_string(), preset.to_string()]),
    }
    a
}

pub struct RenderVideoOpts<'a> {
    pub engine: &'a str,
    pub hw_encode: bool,
    /// Nama encoder hardware: h264_nvenc | h264_qsv | h264_vaapi |
    /// h264_videotoolbox | h264_amf.
    pub hw_encoder: &'a str,
    pub hwaccel: Option<&'a str>,
    pub video: &'a Path,
    pub srt: &'a Path,
    pub template: &'a str,
    pub out: &'a Path,
    pub params: Option<&'a Path>,
    pub fonts: Option<&'a Path>,
    pub ffmpeg_dir: Option<&'a Path>,
    pub crf: u32,
    pub preset: &'a str,
}

#[allow(clippy::too_many_arguments)]
pub fn render_video(
    opts: &RenderVideoOpts<'_>,
    mut on_progress: impl FnMut(u32, u32),
) -> Result<()> {
    // Engine "fframes": pipeline penuh fframes::render (CompositeVideo +
    // frame video sumber + audio + encode). Berbeda jalur dari PNG-pipe.
    if opts.engine == "fframes" {
        on_progress(0, 1);
        crate::fframes_engine::render_fframes(&crate::fframes_engine::FframesRenderOpts {
            video: opts.video,
            srt: opts.srt,
            template: opts.template,
            out: opts.out,
            params: opts.params,
            fonts: opts.fonts,
            ffmpeg_dir: opts.ffmpeg_dir,
            crf: opts.crf,
            preset: opts.preset,
            hw_encode: opts.hw_encode,
            hw_encoder: opts.hw_encoder,
            hwaccel: opts.hwaccel,
        })?;
        on_progress(1, 1);
        return Ok(());
    }

    let ffprobe = tool_bin("ffprobe", opts.ffmpeg_dir)?;
    let ffmpeg = tool_bin("ffmpeg", opts.ffmpeg_dir)?;
    let probe = probe_video(&ffprobe, opts.video)?;
    let fps_f64 = probe.fps_f64();
    let frame_count = (probe.duration_sec * fps_f64).ceil().max(1.0) as u32;

    // Dokumen menutupi seluruh durasi video.
    let doc: CaptionDocument = doc::build_document(
        opts.srt,
        opts.template,
        opts.params,
        Some(frame_count),
    )?;

    let mut font_source = CliFontSource::new();
    font_source.load_system();
    for dir in doc::candidate_font_dirs(opts.fonts) {
        if dir.is_dir() {
            font_source.load_dir(&dir);
        }
    }

    let ctx = FFramesContext {
        time_base: TimeBase {
            fps: frame_count as usize,
            sample_rate: 48000,
        },
        current_video_size: VideoSize {
            width: probe.width as usize,
            height: probe.height as usize,
        },
        duration_in_frames: frame_count as usize,
        mode: FFramesMode::Renderer,
        scenes: None,
        media_source: None,
        font_source: Some(&font_source),
        abort_signal: None,
    };

    if let Some(parent) = opts.out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("buat folder {}", parent.display()))?;
        }
    }

    // Engine skia: backend Vulkan/CPU + font-DB fframes dibuat SEKALI di sini
    // (bukan per frame) — lihat SkiaRasterizer.
    let skia_raster = if opts.engine == "skia" {
        Some(SkiaRasterizer::new(
            probe.width,
            probe.height,
            font_source.database(),
        )?)
    } else {
        None
    };


    // ── ffmpeg: video + PNG pipe → overlay → H.264 ─────────────────────────
    let mut head: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(ha) = opts.hwaccel {
        head.push("-hwaccel".to_string());
        head.push(ha.to_string());
    }
    head.push("-i".to_string());
    // Encoder hardware bila diminta & tersedia; selain itu libx264.
    let use_hw = opts.hw_encode && hw_encoder_available(&ffmpeg, opts.hw_encoder);
    if opts.hw_encode && !use_hw {
        eprintln!(
            "[render] WARN: --hw-encode diminta tapi {} tidak tersedia → fallback libx264",
            opts.hw_encoder
        );
    }
    let mut encode_args: Vec<String> = if use_hw {
        hw_encode_args(opts.hw_encoder, opts.crf, opts.preset)
    } else {
        vec![
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            opts.crf.to_string(),
            "-preset".into(),
            opts.preset.to_string(),
        ]
    };
    encode_args.extend(["-pix_fmt".into(), "yuv420p".into(), "-movflags".into(), "+faststart".into()]);
    let mut child = Command::new(&ffmpeg)
        .args(&head)
        .arg(opts.video)
        // Backend pipe = rawvideo RGBA (bukan PNG per-frame): rasterizer
        // mengirim byte piksel mentah — tanpa zlib deflate per frame.
        .args(["-framerate", &probe.fps_str()])
        .args([
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-video_size",
            &format!("{}x{}", probe.width, probe.height),
            "-i",
            "pipe:0",
        ])
        .args([
            "-filter_complex",
            "[0:v][1:v]overlay=format=auto:shortest=1[v]",
        ])
        .args(["-map", "[v]", "-map", "0:a?", "-c:a", "copy"])
        .args(&encode_args)
        .arg(opts.out)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("jalankan {}", ffmpeg.display()))?;

    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let mut stderr_child = child.stderr.take().context("ffmpeg stderr")?;
    let stderr_collector = {
        let buf = stderr_buf.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut tmp = [0u8; 4096];
            loop {
                match stderr_child.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut b = buf.lock().unwrap();
                        if b.len() < 64 * 1024 {
                            b.extend_from_slice(&tmp[..n]);
                        }
                    }
                }
            }
        })
    };

    let mut stdin = child.stdin.take().context("ffmpeg stdin")?;
    // Frame tanpa caption: satu buffer RGBA(nanti) transparan penuh dipakai
    // ulang — rawvideo input butuh piksel berukuran canvas.
    let zeros_rgba =
        std::sync::Arc::new(vec![0u8; probe.width as usize * probe.height as usize * 4]);
    let mut write_err: Option<std::io::Error> = None;

    if opts.engine == "skia" {
        // ── Skia: raster PARALEL GPU — satu renderer per thread rayon ────────
        // SkiaVulkanCtx disengaja `unsafe impl Sync` oleh fframes:
        // create_skia_context() round-robin antrean (`next_queue.fetch_add` +
        // queue-lock per antrean), jadi tiap thread pegang GPU context sendiri
        // tanpa tabrakan antrean. CPU memakai thread untuk parse SVG + copy.
        let raster = skia_raster
            .as_ref()
            .context("renderer skia tidak terbuat (bug internal engine skia)")?;
        // ── Skia PARALEL: K worker thread, renderer dibuat DI thread-nya ─────
        // (Skia surface/context tak Send → renderer tak boleh berpindah thread).
        // Backend shared (SkiaVulkanCtx Sync: antrean round-robin + lock per
        // antrean). Frame f kerjakan worker f % K; tulis stdin tetap berurutan.
        const SKIA_THREADS: usize = 12;
        const WINDOW: u32 = 256; // RAM: 256×8.3MB ≈ 2.2GB
        let mut base = 0u32;
        while base < frame_count {
            let w_end = (base + WINDOW).min(frame_count);
            let win: Vec<Option<Vec<u8>>> =
                std::thread::scope(|scope| -> Result<Vec<Option<Vec<u8>>>> {
                let mut lane_joins = Vec::new();
                for lane_i in 0..SKIA_THREADS {
                    let raster_lane = raster;
                    let doc_lane = &doc;
                    let ctx_lane = &ctx;
                    let join = scope.spawn(move || {
                        let mut renderer = raster_lane.make_renderer();
                        let mut lane_out: Vec<Option<Vec<u8>>> = Vec::new();
                        let mut f = base + lane_i as u32;
                        while f < w_end {
                            let now_ms = (f as f64) * 1000.0 / fps_f64;
                            let svg = caption_core::templates::render(
                                &doc_lane.template,
                                doc_lane,
                                now_ms as f32,
                                ctx_lane,
                            )
                            .ok_or_else(|| {
                                anyhow::anyhow!("template tak dikenal: {}", doc_lane.template)
                            })?;
                            let s = svg.to_string();
                            let bytes = if s.is_empty() {
                                None
                            } else {
                                Some(renderer.render_frame_rgba(&s, probe.width, probe.height)?)
                            };
                            lane_out.push(bytes);
                            f += SKIA_THREADS as u32;
                        }
                        Ok::<_, anyhow::Error>(lane_out)
                    });
                    lane_joins.push(join);
                }
                // Gabung lane → window berurutan (frame f = lane f%K, posisi f/K).
                let mut window: Vec<Option<Vec<u8>>> = vec![None; (w_end - base) as usize];
                for (lane_i, join) in lane_joins.into_iter().enumerate() {
                    for (p, bytes) in join
                        .join()
                        .expect("skia lane panic")
                        .map_err(anyhow::Error::from)?
                        .into_iter()
                        .enumerate()
                    {
                        window[p * SKIA_THREADS + lane_i] = bytes;
                    }
                }
                Ok(window)
            })?;
            for payload in &win {
                let bytes: &[u8] = match payload {
                    Some(b) => b,
                    None => zeros_rgba.as_slice(),
                };
                if let Err(err) = stdin.write_all(bytes) {
                    write_err = Some(err);
                    break;
                }
            }
            if write_err.is_some() {
                break;
            }
            on_progress(w_end, frame_count);
            base = w_end;
        }
    } else {
        // ── CPU (dan engine tak dikenal): raster PARALEL rayon ────────────────
        // Semua thread logis dipakai; cache + pool DIBUAT SEKALI per thread rayon
        // lalu dipakai ulang antar frame (dulu: dibuat-luang tiap frame — jadi
        // cache subtitle gol statis tidak tersimpan).
        const BATCH_FRAMES: u32 = 32;
        let mut batch_start = 0u32;
        while batch_start < frame_count {
            let batch_end = (batch_start + BATCH_FRAMES).min(frame_count);
            let batch = (batch_start..batch_end)
                .into_par_iter()
                .map_init(
                    || CpuWorkerState {
                        doc: &doc,
                        ctx: &ctx,
                        fdb: font_source.database(),
                        fps_f64,
                        width: probe.width,
                        height: probe.height,
                        cache: svgr::SvgrCache::new(4),
                        pool: svgr::PixmapPool::new(),
                    },
                    |w, f| w.render_frame(f),
                )
                .collect::<Result<Vec<Option<Vec<u8>>>, anyhow::Error>>();
            let batch = match batch {
                Ok(v) => v,
                Err(err) => {
                    // abort batch — tutup pipe vis-à-vis ffmpeg lalu lempar.
                    drop(stdin);
                    let _ = child.wait();
                    return Err(err);
                }
            };
            for item in batch {
                let payload: &[u8] = match &item {
                    Some(bytes) => bytes,
                    None => zeros_rgba.as_slice(),
                };
                if let Err(err) = stdin.write_all(payload) {
                    write_err = Some(err);
                    break;
                }
            }
            if write_err.is_some() {
                break;
            }
            on_progress(batch_end, frame_count);
            batch_start = batch_end;
        }
    }
    drop(stdin);
    let status = child.wait().context("tunggu ffmpeg")?;
    stderr_collector.join().ok();
    let stderr_text = String::from_utf8_lossy(&stderr_buf.lock().unwrap()).to_string();

    if !status.success() || write_err.is_some() {
        let reason = match write_err {
            Some(err) => format!("pipe gagal di frame: {err}"),
            None => "ffmpeg exit non-zero".to_string(),
        };
        bail!("{reason}\n--- ffmpeg stderr ---\n{stderr_text}");
    }
    on_progress(frame_count, frame_count);
    Ok(())
}

// ── worker raster CPU (dipanggil per-thread rayon) ─────────────────────────

struct CpuWorkerState<'a> {
    doc: &'a CaptionDocument,
    ctx: &'a FFramesContext<'a, 'a>,
    fdb: &'a svgr::usvgr::fontdb::Database,
    fps_f64: f64,
    width: u32,
    height: u32,
    cache: svgr::SvgrCache,
    pool: svgr::PixmapPool,
}

impl CpuWorkerState<'_> {
    fn render_frame(&mut self, f: u32) -> Result<Option<Vec<u8>>> {
        let now_ms = (f as f64) * 1000.0 / self.fps_f64;
        let svg = match caption_core::templates::render(
            &self.doc.template,
            self.doc,
            now_ms as f32,
            self.ctx,
        ) {
            Some(svg) => svg,
            None => bail!("template tak dikenal: {}", self.doc.template),
        };
        let s = svg.to_string();
        if s.is_empty() {
            return Ok(None);
        }
        Ok(Some(cpu_frame_rgba(
            &s,
            self.width,
            self.height,
            self.fdb,
            &mut self.cache,
            &self.pool,
        )?))
    }
}

/// SVG → RGBA premultiplied mentah (untuk pipe rawvideo) — tanpa PNG encode.
fn cpu_frame_rgba(
    svg: &str,
    width: u32,
    height: u32,
    fontdb: &svgr::usvgr::fontdb::Database,
    cache: &mut svgr::SvgrCache,
    pool: &svgr::PixmapPool,
) -> Result<Vec<u8>> {
    let opt = svgr::usvgr::Options::default();
    let tree = svgr::usvgr::Tree::from_str(svg, &opt, fontdb)
        .map_err(|err| anyhow::anyhow!("parse svg: {err}"))?;
    let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("gagal alokasi pixmap {width}×{height}"))?;
    let ctx = svgr::Context::new_from_pixmap(&pixmap);
    svgr::render(
        &tree,
        svgr::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
        cache,
        pool,
        &ctx,
    );
    Ok(pixmap.data().to_vec())
}

/// Salin font dari Database milik svgr ke Database milik fframes (usvgr
/// versi berbeda tidak menyatu di cargo — binari font-nya sama).
#[cfg(feature = "gpu-skia")]
fn to_fframes_fontdb(src: &svgr::usvgr::fontdb::Database) -> fframes::usvgr::fontdb::Database {
    use std::collections::HashSet;
    let mut db = fframes::usvgr::fontdb::Database::new();
    let mut seen_ptr: HashSet<usize> = HashSet::new();
    let mut seen_path: HashSet<std::path::PathBuf> = HashSet::new();
    let mut n_bin = 0usize;
    let mut n_file = 0usize;
    for face in src.faces() {
        match &face.source {
            svgr::usvgr::fontdb::Source::Binary(bin) => {
                let r: &dyn AsRef<[u8]> = &**bin;
                let bytes: &[u8] = r.as_ref();
                if seen_ptr.insert(bytes.as_ptr() as usize) {
                    db.load_font_data(bytes.to_vec());
                    n_bin += 1;
                }
            }
            other => {
                // Varian File/SharedFile (atau bentuk lama lain): ekstrak path
                // dari Debug output — jalur ada di dalam tanda kutip pertama.
                let dbg = format!("{other:?}");
                if let Some(start) = dbg.find('"') {
                    if let Some(len) = dbg[start + 1..].find('"') {
                        let path: std::path::PathBuf = dbg[start + 1..start + 1 + len].into();
                        if path.exists() && seen_path.insert(path.clone()) {
                            if let Ok(data) = std::fs::read(&path) {
                                db.load_font_data(data);
                                n_file += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "[engine=skia] font dibawa: bin={n_bin} file={n_file} faces={}",
        db.faces().count()
    );
    db
}
/// Rasterizer Skia untuk satu run render: **backend (Vulkan/CPU) + font-DB
/// fframes dibuat SEKALI** lalu dipakai ulang tiap frame.
///
/// Sebelumnya `skia_pixmap` membangun `SkiaVulkanCtx` (load Vulkan entry +
/// instance + device) dan mengonversi 188 file font **per frame** — itulah
/// sebab engine skia 3× lebih lambat dari cpu. `SkiaFrameRenderer` baru dibuat
/// per frame (murah: struct init); surface GPU-nya di-cache oleh library.
#[cfg(feature = "gpu-skia")]
pub struct SkiaRasterizer {
    backend: SkiaBackendOwned,
    fontdb: fframes::usvgr::fontdb::Database,
}

#[cfg(feature = "gpu-skia")]
enum SkiaBackendOwned {
    Vulkan(fframes_skia_renderer::vulkan::SkiaVulkanCtx),
    Cpu(fframes_skia_renderer::SkiaCpuCtx),
}

#[cfg(feature = "gpu-skia")]
impl SkiaRasterizer {
    pub fn new(
        width: u32,
        height: u32,
        svgr_db: &svgr::usvgr::fontdb::Database,
    ) -> Result<Self> {
        let backend = match fframes_skia_renderer::vulkan::SkiaVulkanCtx::new(
            width as usize,
            height as usize,
        ) {
            Ok(ctx) => {
                eprintln!("[engine=skia] backend: Vulkan (GPU) {width}x{height}");
                SkiaBackendOwned::Vulkan(ctx)
            }
            Err(err) => {
                eprintln!("[engine=skia] vulkan tidak tersedia ({err}) -> fallback Skia-CPU");
                SkiaBackendOwned::Cpu(fframes_skia_renderer::SkiaCpuCtx::new(
                    width as usize,
                    height as usize,
                ))
            }
        };
        Ok(Self {
            backend,
            fontdb: to_fframes_fontdb(svgr_db),
        })
    }

    /// Renderer Skia yang BERTAHAN antar frame: surface GPU + render-cache
    /// dibuat lazy sekali lalu dipakai ulang (dulu: dibuat-buang tiap frame).
    pub fn make_renderer(&self) -> SkiaRendererOwned<'_> {
        SkiaRendererOwned {
            fontdb: &self.fontdb,
            inner: match &self.backend {
                SkiaBackendOwned::Vulkan(ctx) => {
                    SkiaRendererInner::Vulkan(fframes_skia_renderer::SkiaFrameRenderer::new(ctx))
                }
                SkiaBackendOwned::Cpu(ctx) => {
                    SkiaRendererInner::Cpu(fframes_skia_renderer::SkiaFrameRenderer::new(ctx))
                }
            },
        }
    }
}

/// Renderer Skia hasil `SkiaRasterizer::make_renderer` — hidup selama borrow
/// rasterizer (backend + font-DB disimpan di rasterizer pemiliknya).
#[cfg(feature = "gpu-skia")]
pub struct SkiaRendererOwned<'a> {
    fontdb: &'a fframes::usvgr::fontdb::Database,
    inner: SkiaRendererInner<'a>,
}

#[cfg(feature = "gpu-skia")]
enum SkiaRendererInner<'a> {
    Vulkan(fframes_skia_renderer::SkiaFrameRenderer<'a, fframes_skia_renderer::vulkan::SkiaVulkanCtx>),
    Cpu(fframes_skia_renderer::SkiaFrameRenderer<'a, fframes_skia_renderer::SkiaCpuCtx>),
}

#[cfg(feature = "gpu-skia")]
impl SkiaRendererOwned<'_> {
    /// Render satu frame → RGBA premultiplied mentah (pipe rawvideo).
    pub fn render_frame_rgba(&mut self, svg: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        use fframes::FrameRenderer;
        let opt = fframes::usvgr::Options::default();
        let tree = fframes::usvgr::Tree::from_str(svg, &opt, self.fontdb)
            .map_err(|err| anyhow::anyhow!("parse svg: {err}"))?;
        let clear = fframes::Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
        let frame = match &mut self.inner {
            SkiaRendererInner::Vulkan(r) => r.render_tree(&tree, clear, width, height),
            SkiaRendererInner::Cpu(r) => r.render_tree(&tree, clear, width, height),
        }
        .map_err(|err| anyhow::anyhow!("render skia: {err}"))?;
        // RgbaFrame = straight RGBA; pipe rawvideo memakai premultiplied agar
        // setara byte-dengan-jalur PNG lama (tiny-skia Pixmap premultiplied).
        let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
        for src in frame.pixels.chunks_exact(4) {
            let px = svgr::tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3])
                .premultiply();
            bytes.extend_from_slice(&[px.red(), px.green(), px.blue(), px.alpha()]);
        }
        Ok(bytes)
    }

    /// Render satu frame → bytes PNG (dipakai perintah `frames`).
    pub fn render_frame_png(&mut self, svg: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        let rgba = self.render_frame_rgba(svg, width, height)?;
        let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| anyhow::anyhow!("alokasi pixmap skia {width}x{height}"))?;
        pixmap.data_mut().copy_from_slice(&rgba);
        Ok(pixmap.encode_png().context("encode png skia")?)
    }
}

#[cfg(not(feature = "gpu-skia"))]
pub struct SkiaRasterizer;

#[cfg(not(feature = "gpu-skia"))]
pub struct SkiaRendererOwned<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}

#[cfg(not(feature = "gpu-skia"))]
impl SkiaRasterizer {
    pub fn new(
        _width: u32,
        _height: u32,
        _svgr_db: &svgr::usvgr::fontdb::Database,
    ) -> Result<Self> {
        bail!("engine skia butuh build ulang: cargo build -p rscaps-cli --features gpu-skia")
    }

    pub fn make_renderer(&self) -> SkiaRendererOwned<'_> {
        unreachable!("SkiaRasterizer hanya bisa dibuat saat feature gpu-skia aktif")
    }
}

#[cfg(not(feature = "gpu-skia"))]
impl SkiaRendererOwned<'_> {
    pub fn render_frame_rgba(&mut self, _svg: &str, _width: u32, _height: u32) -> Result<Vec<u8>> {
        unreachable!("SkiaRendererOwned hanya bisa dibuat saat feature gpu-skia aktif")
    }

    pub fn render_frame_png(&mut self, _svg: &str, _width: u32, _height: u32) -> Result<Vec<u8>> {
        unreachable!("SkiaRendererOwned hanya bisa dibuat saat feature gpu-skia aktif")
    }
}

