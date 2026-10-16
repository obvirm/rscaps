//! Fase 2 — burn-in caption ke video: probe (ffprobe) → frame caption per
//! frame (caption-core) → pipe PNG → ffmpeg overlay → mp4.
//!
//! Logika caption 100% caption-core; ffmpeg hanya compositor/muxer.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail, ensure};

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

pub struct RenderVideoOpts<'a> {
    pub engine: &'a str,
    pub hw_encode: bool,
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
    let encode_args: Vec<String> = if opts.hw_encode {
        vec![
            "-c:v".into(),
            "h264_nvenc".into(),
            "-cq".into(),
            opts.crf.to_string(),
            "-preset".into(),
            opts.preset.to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-movflags".into(),
            "+faststart".into(),
        ]
    } else {
        vec![
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            opts.crf.to_string(),
            "-preset".into(),
            opts.preset.to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-movflags".into(),
            "+faststart".into(),
        ]
    };
    let mut child = Command::new(&ffmpeg)
        .args(&head)
        .arg(opts.video)
        .args(["-framerate", &probe.fps_str()])
        .args(["-f", "image2pipe", "-c:v", "png", "-i", "pipe:0"])
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
    let mut write_err: Option<std::io::Error> = None;
    for f in 0..frame_count {
        let now_ms = (f as f64) * 1000.0 / fps_f64;
        let png = match caption_core::templates::render(&doc.template, &doc, now_ms as f32, &ctx) {
            Some(svg) => {
                let s = svg.to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(raster_png(&s, probe.width, probe.height, font_source.database(), opts.engine)?)
                }
            }
            None => bail!("template tak dikenal: {}", doc.template),
        };
        match png {
            Some(bytes) => {
                if let Err(err) = stdin.write_all(&bytes) {
                    write_err = Some(err);
                    break;
                }
            }
            None => {
                // Frame tanpa caption: PNG 1×1 transparan tidak cukup untuk
                // overlay (ukuran beda) — raster transparan penuh.
                let bytes = transparent_png(probe.width, probe.height)?;
                if let Err(err) = stdin.write_all(&bytes) {
                    write_err = Some(err);
                    break;
                }
            }
        }
        if f % 15 == 0 || f + 1 == frame_count {
            on_progress(f + 1, frame_count);
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

// ── raster helpers (PNG untuk image2pipe) ──────────────────────────────────

fn raster_png(
    svg: &str,
    width: u32,
    height: u32,
    fontdb: &svgr::usvgr::fontdb::Database,
    engine: &str,
) -> Result<Vec<u8>> {
    match engine {
        "skia" => return skia_png(svg, width, height, fontdb),
        // fframes engine ditangani di render_video (pipeline penuh), bukan per-frame.
        "fframes" => bail!("engine fframes = pipeline penuh, tidak lewat raster_png per-frame"),
        _ => {}
    }
    let opt = svgr::usvgr::Options::default();
    let tree = svgr::usvgr::Tree::from_str(svg, &opt, fontdb)
        .map_err(|err| anyhow::anyhow!("parse svg: {err}"))?;
    let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("gagal alokasi pixmap {width}×{height}"))?;
    let mut cache = svgr::SvgrCache::new(4);
    let pool = svgr::PixmapPool::new();
    let ctx = svgr::Context::new_from_pixmap(&pixmap);
    svgr::render(
        &tree,
        svgr::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
        &mut cache,
        &pool,
        &ctx,
    );
    Ok(pixmap
        .encode_png()
        .context("encode png")?)
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
/// Raster SVG lewat backend Skia (fframes_skia_renderer): Vulkan GPU dengan
/// fallback Skia-CPU. Hanya ada saat build `--features gpu-skia`; tanpa
/// fitur, `skia_png` tetap ada tapi menuntut build ulang (lihat di bawah).
#[cfg(feature = "gpu-skia")]
pub fn skia_pixmap(
    svg: &str,
    width: u32,
    height: u32,
    fontdb: &svgr::usvgr::fontdb::Database,
) -> Result<svgr::tiny_skia::Pixmap> {
    use fframes::FrameRenderer;
    let xdb = to_fframes_fontdb(fontdb);
    let opt = fframes::usvgr::Options::default();
    let tree = fframes::usvgr::Tree::from_str(svg, &opt, &xdb)
        .map_err(|err| anyhow::anyhow!("parse svg: {err}"))?;
    let clear = fframes::Color { r: 0, g: 0, b: 0, a: 0 };
    let frame = match fframes_skia_renderer::vulkan::SkiaVulkanCtx::new(width as usize, height as usize)
    {
        Ok(ctx) => {
            let mut renderer = fframes_skia_renderer::SkiaFrameRenderer::new(&ctx);
            renderer.render_tree(&tree, clear, width, height)
        }
        Err(err) => {
            eprintln!("[engine=skia] vulkan tidak tersedia ({err}) -> fallback Skia-CPU");
            let ctx = fframes_skia_renderer::SkiaCpuCtx::new(width as usize, height as usize);
            let mut renderer = fframes_skia_renderer::SkiaFrameRenderer::new(&ctx);
            renderer.render_tree(&tree, clear, width, height)
        }
    }
    .map_err(|err| anyhow::anyhow!("render skia: {err}"))?;
    // RgbaFrame = straight RGBA; tiny-skia Pixmap = premultiplied.
    let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("alokasi pixmap skia {width}x{height}"))?;
    let out = pixmap.pixels_mut();
    for (px, src) in out.iter_mut().zip(frame.pixels.chunks_exact(4)) {
        *px = svgr::tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    Ok(pixmap)
}

#[cfg(feature = "gpu-skia")]
pub fn skia_png(
    svg: &str,
    width: u32,
    height: u32,
    fontdb: &svgr::usvgr::fontdb::Database,
) -> Result<Vec<u8>> {
    let pixmap = skia_pixmap(svg, width, height, fontdb)?;
    Ok(pixmap.encode_png().context("encode png skia")?)
}

#[cfg(not(feature = "gpu-skia"))]
pub fn skia_pixmap(
    _svg: &str,
    _width: u32,
    _height: u32,
    _fontdb: &svgr::usvgr::fontdb::Database,
) -> Result<svgr::tiny_skia::Pixmap> {
    bail!("engine skia butuh build ulang: cargo build -p rscaps-cli --features gpu-skia")
}

#[cfg(not(feature = "gpu-skia"))]
pub fn skia_png(
    svg: &str,
    width: u32,
    height: u32,
    fontdb: &svgr::usvgr::fontdb::Database,
) -> Result<Vec<u8>> {
    let pixmap = skia_pixmap(svg, width, height, fontdb)?;
    Ok(pixmap.encode_png().context("encode png skia")?)
}
fn transparent_png(width: u32, height: u32) -> Result<Vec<u8>> {
    let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("gagal alokasi pixmap transparan"))?;
    pixmap.fill(svgr::tiny_skia::Color::TRANSPARENT);
    Ok(pixmap.encode_png().context("encode png transparan")?)
}

