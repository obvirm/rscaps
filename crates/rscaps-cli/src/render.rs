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
    let mut child = Command::new(&ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(opts.video)
        .args(["-framerate", &probe.fps_str()])
        .args(["-f", "image2pipe", "-c:v", "png", "-i", "pipe:0"])
        .args([
            "-filter_complex",
            "[0:v][1:v]overlay=format=auto:shortest=1[v]",
        ])
        .args(["-map", "[v]", "-map", "0:a?", "-c:a", "copy"])
        .args([
            "-c:v",
            "libx264",
            "-crf",
            &opts.crf.to_string(),
            "-preset",
            opts.preset,
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
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
                    Some(raster_png(&s, probe.width, probe.height, font_source.database())?)
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
) -> Result<Vec<u8>> {
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

fn transparent_png(width: u32, height: u32) -> Result<Vec<u8>> {
    let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("gagal alokasi pixmap transparan"))?;
    pixmap.fill(svgr::tiny_skia::Color::TRANSPARENT);
    Ok(pixmap.encode_png().context("encode png transparan")?)
}

