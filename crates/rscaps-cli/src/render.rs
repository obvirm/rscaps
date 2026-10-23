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
    // Paritas dengan studio web: HANYA font dari fontManifest.json (satu
    // sumber dengan fontManifest.ts — urutan file identik = fallback glif
    // usvgr identik di kedua jalur). Fontdb penuh Windows (ratusan file)
    // membuat text layout usvgr ~10x lebih lambat (178ms vs ~14ms per frame
    // @1080p). Escape hatch utk teks eksotis: env RSCAPS_LOAD_SYSTEM_FONTS=1.
    font_source.load_manifest(Path::new(crate::fonts::MANIFEST_RELPATH));
    if std::env::var("RSCAPS_LOAD_SYSTEM_FONTS").as_deref() == Ok("1") {
        font_source.load_system();
    }
    if let Some(dir) = opts.fonts {
        if dir.is_dir() {
            font_source.load_dir(dir);
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
    // (bukan per frame) - lihat SkiaRasterizer. Hanya saat feature gpu-skia;
    // di luar feature, engine skia bail di pemilihan lane raster.
    #[cfg(feature = "gpu-skia")]
    let skia_raster = if opts.engine == "skia" {
        Some(SkiaRasterizer::new(
            probe.width,
            probe.height,
            font_source.database(),
        )?)
    } else {
        None
    };


    // ── ffmpeg: video + rawvideo pipe → overlay → encoder ──────────────────
    let mut head: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(ha) = opts.hwaccel {
        head.push("-hwaccel".to_string());
        head.push(ha.to_string());
    }
    // Env override jumlah thread decode per proses slice (opsional; default:
    // apa adanya ffmpeg). Raster kini murah (region filter ketat + dedupe
    // layer) sehingga thread decode ffmpeg tak lagi menindas lane.
    if let Ok(v) = std::env::var("RSCAPS_FFMPEG_THREADS") {
        if let Ok(n) = v.parse::<u32>() {
            if n > 0 {
                head.push("-threads".to_string());
                head.push(n.to_string());
            }
        }
    }
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

    // ── K-SLICE (engine skia & cpu): K proses ffmpeg paralel (decode+blend+
    // encode per potongan frame), tiap potongan digabung `concat -c copy` ──
    // Dulu 1 proses ffmpeg = filter chain single-thread jadi dinding: write
    // 40.3s menunggu ffmpeg mengonsumsi pipe. K proses = konsumsi pipe ×K +
    // NVENC session ×K + decode ×K. Raster (12 lane, backend sesuai engine)
    // dan cache per-SVG dibagi rata ke tiap slice; double-buffer tetap jalan
    // per slice (raster window N+1 selagi window N ditulis).
    if opts.engine == "skia" || opts.engine == "cpu" {
        let slices = std::env::var("RSCAPS_SLICES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(3);
        let lanes_per_slice = (std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(12) / slices)
            .max(1);
        const WINDOW: u32 = 128; // RAM: SLICES×WINDOW×8.3MB ≈ 3.2GB
        let lane_raster = if opts.engine == "skia" {
            #[cfg(feature = "gpu-skia")]
            {
                LaneRaster::Skia(
                    skia_raster
                        .as_ref()
                        .context("renderer skia tidak terbuat (bug internal engine skia)")?,
                )
            }
            #[cfg(not(feature = "gpu-skia"))]
            {
                bail!("engine skia butuh build ulang: cargo build -p rscaps-cli --features gpu-skia")
            }
        } else {
            LaneRaster::Cpu {
                fdb: font_source.database(),
            }
        };
        let raster_cache: Arc<RasterCache> = Arc::new(RasterCache::new(128));
        let frames_done = std::sync::atomic::AtomicU32::new(0);

        // ── Band crop: sample frame → union bbox alpha → pita vertikal ─────
        // Raster + pipe hanya pita [y0, y0+bh): blur SVG (σ≈28×3 layer)
        // mahal per piksel — pita ~35% tinggi frame memangkas biaya raster
        // MISS dan byte pipe ~2.7×. Margin 128px untuk bleed blur. Fallback
        // full-frame bila caption menyebar (>85% tinggi).
        let band = compute_caption_band(
            &lane_raster,
            &doc,
            &ctx,
            frame_count,
            fps_f64,
            probe.width,
            probe.height,
        );
        let (band_y, band_h) = match band {
            Some((y, h)) => (y, h),
            None => (0, probe.height),
        };
        if band.is_some() {
            eprintln!(
                "[render] band crop aktif: y={band_y} h={band_h} ({}% tinggi frame)",
                band_h * 100 / probe.height
            );
        }
        let zeros_rgba = std::sync::Arc::new(vec![
            0u8;
            probe.width as usize * band_h as usize * 4
        ]);

        let frames_per_slice = frame_count.div_ceil(slices as u32);
        let slice_results: Vec<Result<PathBuf, anyhow::Error>> = std::thread::scope(|scope| {
            let mut joins = Vec::new();
            for slice_i in 0..slices {
                let s0 = (slice_i as u32 * frames_per_slice).min(frame_count);
                let s1 = ((slice_i as u32 + 1) * frames_per_slice).min(frame_count);
                if s0 >= s1 {
                    continue;
                }
                let ffmpeg = &ffmpeg;
                let head = &head;
                let encode_args = &encode_args;
                let video = opts.video;
                let probe_ref = &probe;
                let doc_ref = &doc;
                let ctx_ref = &ctx;
                let raster_lane = lane_raster;
                let cache = Arc::clone(&raster_cache);
                let zeros = Arc::clone(&zeros_rgba);
                let progress_ref = &frames_done;
                let seg_path = opts
                    .out
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(format!("rscaps_slice_{slice_i}.mp4"));
                let p_out_path = seg_path.clone();
                let join = scope.spawn(move || -> Result<PathBuf, anyhow::Error> {
                    run_skia_slice(SliceParams {
                        ffmpeg,
                        head,
                        encode_args,
                        video,
                        probe: probe_ref,
                        doc: doc_ref,
                        ctx: ctx_ref,
                        raster: raster_lane,
                        cache,
                        zeros,
                        range: s0..s1,
                        lanes: lanes_per_slice,
                        window: WINDOW,
                        fps_f64,
                        out: seg_path,
                        progress_ref,
                        band_y,
                        band_h,
                    })?;
                    Ok(p_out_path)
                });
                joins.push(join);
            }
            let mut results = Vec::new();
            for join in joins {
                results.push(join.join().expect("skia slice panic"));
            }
            results
        });
        let mut seg_paths: Vec<PathBuf> = Vec::new();
        for r in slice_results {
            seg_paths.push(r?);
        }

        // Gabung potongan → output final (pola sama dengan fframes chunked).
        crate::fframes_engine::concat_segments(&ffmpeg, &seg_paths, opts.out)?;
        for seg in &seg_paths {
            let _ = std::fs::remove_file(seg);
        }
        on_progress(frame_count, frame_count);
    }
    Ok(())
}

// ── worker raster CPU (dipanggil per-thread rayon) ─────────────────────────

/// SVG → RGBA premultiplied mentah (untuk pipe rawvideo) — tanpa PNG encode.
fn cpu_frame_rgba(
    svg: &str,
    width: u32,
    height: u32,
    fontdb: &svgr::usvgr::fontdb::Database,
    cache: &mut svgr::SvgrCache,
    pool: &svgr::PixmapPool,
) -> Result<Vec<u8>> {
    // Gambar data: URL (mask iris, upload user) — decode-once lalu ikut ke
    // Options.image_data; kaca selene dilewati (ditangani host fframes).
    crate::images::ensure_svg_images(svg);
    let images = crate::images::snapshot();
    let opt = svgr::usvgr::Options {
        image_data: Some(&images),
        ..Default::default()
    };
    let t0 = std::time::Instant::now();
    let tree = svgr::usvgr::Tree::from_str(svg, &opt, fontdb)
        .map_err(|err| anyhow::anyhow!("parse svg: {err}"))?;
    let parse_us = t0.elapsed().as_micros();
    let t1 = std::time::Instant::now();
    let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("gagal alokasi pixmap {width}×{height}"))?;
    let ctx = svgr::Context::new_from_pixmap(&pixmap);
    let alloc_us = t1.elapsed().as_micros();
    let t2 = std::time::Instant::now();
    svgr::render(
        &tree,
        svgr::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
        cache,
        pool,
        &ctx,
    );
    let render_us = t2.elapsed().as_micros();
    if let Ok(th) = std::env::var("RSCAPS_DEBUG_SLOW") {
        let th: u128 = th.parse().unwrap_or(50_000);
        if render_us > th {
            eprintln!(
                "SLOW render={render_us}us size={width}x{height} svg_len={} texts={}",
                svg.len(),
                svg.matches("<text").count()
            );
        }
    }
    let out = pixmap.data().to_vec();
    let copy_us = t2.elapsed().as_micros() - render_us;
    PHASE_PARSE.fetch_add(parse_us as u64, std::sync::atomic::Ordering::Relaxed);
    PHASE_ALLOC.fetch_add(alloc_us as u64, std::sync::atomic::Ordering::Relaxed);
    PHASE_RENDER.fetch_add(render_us as u64, std::sync::atomic::Ordering::Relaxed);
    PHASE_COPY.fetch_add(copy_us as u64, std::sync::atomic::Ordering::Relaxed);
    Ok(out)
}

// ── slice: satu proses ffmpeg + raster lane + double-buffer + cache ────────

/// Backend raster per-lane: skia (GPU Vulkan, butuh feature gpu-skia) atau
/// usvgr/tiny-skia CPU. Keduanya lewat arsitektur slice yang sama: band
/// crop + cache per-SVG + double-buffer + K proses ffmpeg paralel.
#[derive(Clone, Copy)]
enum LaneRaster<'a> {
    #[cfg(feature = "gpu-skia")]
    Skia(&'a SkiaRasterizer),
    Cpu {
        fdb: &'a svgr::usvgr::fontdb::Database,
    },
}

/// State raster per-lane thread (renderer/cache/pool tak boleh berpindah
/// thread — dibuat DI dalam lane-nya).
enum LaneState<'a> {
    #[cfg(feature = "gpu-skia")]
    Skia(SkiaRendererOwned<'a>),
    Cpu {
        fdb: &'a svgr::usvgr::fontdb::Database,
        cache: svgr::SvgrCache,
        pool: svgr::PixmapPool,
    },
}

impl LaneState<'_> {
    fn render_rgba(&mut self, svg: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        match self {
            #[cfg(feature = "gpu-skia")]
            LaneState::Skia(r) => r.render_frame_rgba(svg, width, height),
            LaneState::Cpu { fdb, cache, pool } => {
                cpu_frame_rgba(svg, width, height, fdb, cache, pool)
            }
        }
    }
}

/// Hitung band vertikal caption: sample `SAMPLES` frame merata, raster
/// full-frame, union baris yang punya piksel alpha>0, + margin bleed blur.
/// None = full-frame (caption menyebar / sampling gagal).
fn compute_caption_band(
    raster: &LaneRaster<'_>,
    doc: &CaptionDocument,
    ctx: &FFramesContext<'_, '_>,
    frame_count: u32,
    fps_f64: f64,
    width: u32,
    height: u32,
) -> Option<(u32, u32)> {
    const SAMPLES: u32 = 24;
    const MARGIN: u32 = 128;
    let mut state = match raster {
        #[cfg(feature = "gpu-skia")]
        LaneRaster::Skia(r) => LaneState::Skia(r.make_renderer()),
        LaneRaster::Cpu { fdb } => LaneState::Cpu {
            fdb,
            cache: svgr::SvgrCache::none(),
            pool: svgr::PixmapPool::new(),
        },
    };
    let mut y_min = height;
    let mut y_max = 0u32;
    let mut found_any = false;
    for i in 0..SAMPLES {
        let f = (i * frame_count / SAMPLES.max(1)).min(frame_count.saturating_sub(1));
        let now_ms = (f as f64) * 1000.0 / fps_f64;
        let svg = caption_core::templates::render(&doc.template, doc, now_ms as f32, ctx)?;
        let s = svg.to_string();
        if s.is_empty() {
            continue;
        }
        // Raster full-frame lalu pindai baris alpha.
        let data = state.render_rgba(&s, width, height).ok()?;
        let row_bytes = width as usize * 4;
        for (row, chunk) in data.chunks_exact(row_bytes).enumerate() {
            let has_paint = chunk.chunks_exact(4).any(|px| px[3] != 0);
            if has_paint {
                found_any = true;
                y_min = y_min.min(row as u32);
                y_max = y_max.max(row as u32 + 1);
            }
        }
    }
    if !found_any {
        return None;
    }
    let y0 = y_min.saturating_sub(MARGIN);
    let y1 = (y_max + MARGIN).min(height);
    let bh = y1 - y0;
    if bh >= height * 85 / 100 {
        return None; // caption menyebar — band tak berguna
    }
    Some((y0, bh))
}

/// Bungkus ulang SVG dengan viewBox tergeser ke band: konten koordinat penuh
/// tetap utuh, root baru memetakan y∈[y0, y0+bh) → [0, bh).
fn wrap_svg_band(svg: &str, width: u32, y0: u32, bh: u32) -> Option<String> {
    let open_end = svg.find('>')? + 1;
    let close = svg.rfind("</svg>")?;
    if close < open_end {
        return None;
    }
    let inner = &svg[open_end..close];
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{bh}\" viewBox=\"0 {y0} {width} {bh}\">{inner}</svg>"
    ))
}

struct SliceParams<'a> {
    ffmpeg: &'a Path,
    head: &'a [String],
    encode_args: &'a [String],
    video: &'a Path,
    probe: &'a VideoProbe,
    doc: &'a CaptionDocument,
    ctx: &'a FFramesContext<'a, 'a>,
    raster: LaneRaster<'a>,
    cache: Arc<RasterCache>,
    zeros: Arc<Vec<u8>>,
    /// Rentang frame global slice ini (inklusif-eksklusif).
    range: std::ops::Range<u32>,
    lanes: usize,
    window: u32,
    fps_f64: f64,
    out: PathBuf,
    progress_ref: &'a std::sync::atomic::AtomicU32,
    /// Band crop: baris atas + tinggi pita (band_h == tinggi frame = full).
    band_y: u32,
    band_h: u32,
}

fn run_skia_slice(p: SliceParams<'_>) -> Result<()> {
    let s0 = p.range.start;
    let s1 = p.range.end;
    let start_sec = s0 as f64 / p.fps_f64;
    let count = s1 - s0;

    // ffmpeg per-slice: seek ke awal potongan, batasi jumlah frame.
    let mut cmd = Command::new(p.ffmpeg);
    for a in p.head {
        if a == "-i" {
            break;
        }
        cmd.arg(a);
    }
    cmd.args(["-ss", &format!("{start_sec}")]);
    cmd.arg("-i").arg(p.video);
    cmd.args(["-framerate", &p.probe.fps_str()]);
    cmd.args([
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-video_size",
        &format!("{}x{}", p.probe.width, p.band_h),
        "-i",
        "pipe:0",
    ]);
    // Overlay pita caption di baris band_y (pita penuh lebar).
    if p.band_h == p.probe.height {
        cmd.args([
            "-filter_complex",
            "[0:v][1:v]overlay=format=auto:shortest=1[v]",
        ]);
    } else {
        cmd.args([
            "-filter_complex",
            &format!("[0:v][1:v]overlay=format=auto:shortest=1:x=0:y={}[v]", p.band_y),
        ]);
    }
    cmd.args(["-map", "[v]", "-map", "0:a?", "-c:a", "copy"]);
    cmd.args(p.encode_args);
    cmd.args(["-frames:v", &count.to_string()]);
    cmd.arg(&p.out);
    cmd.stdin(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("jalankan {} (slice {s0}..{s1})", p.ffmpeg.display()))?;
    // Kumpulkan stderr slice untuk pesan error yang jelas.
    let stderr_handle = child.stderr.take().context("ffmpeg stderr slice")?;
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_collector = {
        let buf = Arc::clone(&stderr_buf);
        std::thread::spawn(move || {
            use std::io::Read;
            let mut h = stderr_handle;
            let mut tmp = [0u8; 4096];
            loop {
                match h.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut b = buf.lock().unwrap();
                        if b.len() < 16 * 1024 {
                            b.extend_from_slice(&tmp[..n]);
                        }
                    }
                }
            }
        })
    };
    let mut stdin = child.stdin.take().context("ffmpeg stdin slice")?;

    let raster_total = std::sync::atomic::AtomicU64::new(0);
    let mut write_err: Option<std::io::Error> = None;
    let mut written: u32 = 0;
    let mut pending: Option<Vec<Option<Arc<Vec<u8>>>>> = None;
    let mut pending_end: u32 = s0;
    let mut base = s0;
    while base < s1 {
        let w_end = (base + p.window).min(s1);
        let r0 = std::time::Instant::now();
        let win: Vec<Option<Arc<Vec<u8>>>> =
            std::thread::scope(|scope| -> Result<Vec<Option<Arc<Vec<u8>>>>> {
                let mut lane_joins = Vec::new();
                for lane_i in 0..p.lanes {
                    let doc_lane = p.doc;
                    let ctx_lane = p.ctx;
                    let cache_lane = Arc::clone(&p.cache);
                    let join = scope.spawn(move || {
                        let mut lane_state = match p.raster {
                            #[cfg(feature = "gpu-skia")]
                            LaneRaster::Skia(r) => LaneState::Skia(r.make_renderer()),
                            LaneRaster::Cpu { fdb } => LaneState::Cpu {
                                fdb,
                                cache: svgr::SvgrCache::none(),
                                pool: svgr::PixmapPool::new(),
                            },
                        };
                        let mut lane_out: Vec<Option<Arc<Vec<u8>>>> = Vec::new();
                        let mut f = base + lane_i as u32;
                        while f < w_end {
                            let now_ms = (f as f64) * 1000.0 / p.fps_f64;
                            let t_build = std::time::Instant::now();
                            let svg = caption_core::templates::render(
                                &doc_lane.template,
                                doc_lane,
                                now_ms as f32,
                                ctx_lane,
                            )
                            .ok_or_else(|| {
                                anyhow::anyhow!("template tak dikenal: {}", doc_lane.template)
                            })?;
                            let raw = svg.to_string();
                            let s = if raw.is_empty() {
                                String::new()
                            } else if p.band_h == p.probe.height {
                                raw
                            } else {
                                wrap_svg_band(&raw, p.probe.width, p.band_y, p.band_h)
                                    .ok_or_else(|| {
                                        anyhow::anyhow!("wrap band svg gagal (frame {f})")
                                    })?
                            };
                            SVG_BUILD_US.fetch_add(
                                t_build.elapsed().as_micros() as u64,
                                std::sync::atomic::Ordering::Relaxed,
                            );
                            let bytes = if s.is_empty() {
                                None
                            } else if let Some(hit) = cache_lane.get(&s) {
                                Some(hit)
                            } else {
                                let t_r = std::time::Instant::now();
                                let data = lane_state.render_rgba(&s, p.probe.width, p.band_h)?;
                                RASTER_US.fetch_add(
                                    t_r.elapsed().as_micros() as u64,
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                                let arc = Arc::new(data);
                                cache_lane.insert(&s, Arc::clone(&arc));
                                Some(arc)
                            };
                            lane_out.push(bytes);
                            f += p.lanes as u32;
                        }
                        Ok::<_, anyhow::Error>(lane_out)
                    });
                    lane_joins.push(join);
                }
                // Selagi lane me-raster window ini, tulis window SEBELUMNYA.
                if let Some(prev) = pending.take() {
                    for payload in &prev {
                        let bytes: &[u8] = match payload {
                            Some(b) if !b.is_empty() => b.as_slice(),
                            _ => p.zeros.as_slice(),
                        };
                        if let Err(err) = stdin.write_all(bytes) {
                            write_err = Some(err);
                            break;
                        }
                    }
                    let new_total = pending_end - s0;
                    let newly = new_total - written;
                    written = new_total;
                    p.progress_ref
                        .fetch_add(newly, std::sync::atomic::Ordering::Relaxed);
                    raster_total.fetch_add(
                        r0.elapsed().as_millis() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                // Gabung lane → window berurutan (frame f = lane f%K, posisi f/K).
                let mut window: Vec<Option<Arc<Vec<u8>>>> = vec![None; (w_end - base) as usize];
                for (lane_i, join) in lane_joins.into_iter().enumerate() {
                    for (pp, bytes) in join
                        .join()
                        .expect("raster lane panic")
                        .map_err(anyhow::Error::from)?
                        .into_iter()
                        .enumerate()
                    {
                        window[pp * p.lanes + lane_i] = bytes;
                    }
                }
                Ok(window)
            })?;
        pending = Some(win);
        pending_end = w_end;
        if write_err.is_some() {
            break;
        }
        base = w_end;
    }
    // Tulis window terakhir.
    if write_err.is_none() {
        if let Some(last) = pending.take() {
            for payload in &last {
                let bytes: &[u8] = match payload {
                    Some(b) if !b.is_empty() => b.as_slice(),
                    _ => p.zeros.as_slice(),
                };
                if let Err(err) = stdin.write_all(bytes) {
                    write_err = Some(err);
                    break;
                }
            }
            let newly = (s1 - s0) - written;
            p.progress_ref
                .fetch_add(newly, std::sync::atomic::Ordering::Relaxed);
        }
    }
    drop(stdin);
    let status = child.wait().context("tunggu ffmpeg slice")?;
    stderr_collector.join().ok();
    let rt = raster_total.load(std::sync::atomic::Ordering::Relaxed);
    let hits = HITS.swap(0, std::sync::atomic::Ordering::Relaxed);
    let misses = MISSES.swap(0, std::sync::atomic::Ordering::Relaxed);
    let build_ms = SVG_BUILD_US.swap(0, std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0;
    let raster_ms = RASTER_US.swap(0, std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0;
    eprintln!(
        "[render] profil slice {s0}..{s1}: raster={rt}ms hits={hits} misses={misses} svg_build={build_ms:.0}ms raster_only={raster_ms:.0}ms fase: parse={}ms alloc={}ms render={}ms copy={}ms",
        PHASE_PARSE.swap(0, std::sync::atomic::Ordering::Relaxed) / 1000,
        PHASE_ALLOC.swap(0, std::sync::atomic::Ordering::Relaxed) / 1000,
        PHASE_RENDER.swap(0, std::sync::atomic::Ordering::Relaxed) / 1000,
        PHASE_COPY.swap(0, std::sync::atomic::Ordering::Relaxed) / 1000,
    );
    if !status.success() || write_err.is_some() {
        let reason = match write_err {
            Some(err) => format!("pipe gagal: {err}"),
            None => "ffmpeg exit non-zero".to_string(),
        };
        let stderr_text = String::from_utf8_lossy(&stderr_buf.lock().unwrap()).to_string();
        bail!("{reason}\n--- ffmpeg stderr slice {s0}..{s1} ---\n{stderr_text}");
    }
    Ok(())
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
        // Konversi bulk ke buffer pre-alokasi — dulu per-piksel
        // extend_from_slice (2 juta iterasi @1080p ≈ 15-25ms/frame).
        let n = width as usize * height as usize * 4;
        let mut bytes = vec![0u8; n];
        debug_assert_eq!(frame.pixels.len(), n);
        for (dst, src) in bytes.chunks_exact_mut(4).zip(frame.pixels.chunks_exact(4)) {
            let px = svgr::tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3])
                .premultiply();
            dst[0] = px.red();
            dst[1] = px.green();
            dst[2] = px.blue();
            dst[3] = px.alpha();
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
    pub fn render_frame_png(&mut self, _svg: &str, _width: u32, _height: u32) -> Result<Vec<u8>> {
        unreachable!("SkiaRendererOwned hanya bisa dibuat saat feature gpu-skia aktif")
    }
}


/// Cache raster per-SVG: frame statis dalam satu cue menghasilkan SVG identik,
/// jadi hasil raster (RGBA premultiplied siap-pipe) boleh dipakai ulang.
/// Key = hash string SVG; LRU sederhana berkapasitas `cap` entri (8.3MB/frame
/// @1080p — 64 entri ≈ 530MB RAM). Hit = skip raster + readback GPU.
struct RasterCache {
    inner: std::sync::Mutex<RasterCacheState>,
    cap: usize,
}

struct RasterCacheState {
    map: std::collections::HashMap<u64, Arc<Vec<u8>>>,
    order: std::collections::VecDeque<u64>,
}

impl RasterCache {
    fn new(cap: usize) -> Self {
        Self {
            inner: std::sync::Mutex::new(RasterCacheState {
                map: std::collections::HashMap::new(),
                order: std::collections::VecDeque::new(),
            }),
            cap,
        }
    }

    fn hash(svg: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        svg.hash(&mut h);
        h.finish()
    }

    fn get(&self, svg: &str) -> Option<Arc<Vec<u8>>> {
        let key = Self::hash(svg);
        let mut g = self.inner.lock().unwrap();
        let hit = g.map.get(&key).cloned()?;
        g.order.retain(|k| *k != key);
        g.order.push_back(key);
        HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(hit)
    }

    fn insert(&self, svg: &str, val: Arc<Vec<u8>>) {
        MISSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let key = Self::hash(svg);
        let mut g = self.inner.lock().unwrap();
        if g.map.insert(key, val).is_some() {
            g.order.retain(|k| *k != key);
        }
        g.order.push_back(key);
        while g.order.len() > self.cap {
            if let Some(old) = g.order.pop_front() {
                g.map.remove(&old);
            }
        }
    }
}

/// Diagnostik cache raster (agregat lintas lane/slice).
static HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static MISSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SVG_BUILD_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static RASTER_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PHASE_PARSE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PHASE_ALLOC: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PHASE_RENDER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PHASE_COPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
