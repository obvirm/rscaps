//! rscaps CLI — engine caption (caption-core) tanpa browser.
//!
//! Fase 1: `templates` (katalog), `convert` (SRT/VTT), `frames` (render frame
//! SVG/PNG headless). Fase berikutnya: `render` (burn-in video) dan `serve`
//! (API localhost).
//!
//! Semua logika caption lewat caption-core (satu titik) — CLI ini hanya
//! orkestrasi I/O: baca subtitle, panggil `templates::render`, tulis file.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

mod doc;
mod fonts;
mod render;
mod server;

use fonts::CliFontSource;
use render::{RenderVideoOpts, render_video, tool_bin};
use caption_core::CaptionDocument;
use fframes::{FFramesContext, FFramesMode, TimeBase, VideoSize};

#[derive(Parser)]
#[command(
    name = "rscaps",
    version,
    about = "rscaps — headless caption engine (caption-core)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Daftar template (dari katalog caption-core)
    Templates,
    /// Konversi file subtitle antar format (.srt / .vtt)
    Convert {
        /// File input (.srt atau .vtt)
        input: PathBuf,
        /// Format output: srt | vtt
        #[arg(long)]
        to: String,
        /// File output (default: ganti ekstensi input)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Render frame caption (SVG atau PNG) — tanpa video, transparan
    Frames {
        /// File subtitle (.srt / .vtt)
        #[arg(long)]
        srt: PathBuf,
        /// Id template (lihat `rscaps templates`)
        #[arg(long)]
        template: String,
        /// Folder output
        #[arg(long, default_value = "out")]
        out: PathBuf,
        /// Ukuran canvas / video: <lebar>x<tinggi> (mis. 1080x1920)
        #[arg(long, default_value = "1920x1080")]
        size: String,
        /// Frame per detik
        #[arg(long, default_value_t = 30)]
        fps: u32,
        /// Format output: svg | png
        #[arg(long, default_value = "svg")]
        format: String,
        /// Patch params (JSON object) — contoh: '{"font_size":64,"v_offset":0.8}'
        #[arg(long)]
        params: Option<PathBuf>,
        /// Frame awal (inklusif)
        #[arg(long)]
        from: Option<u32>,
        /// Frame akhir (eksklusif)
        #[arg(long)]
        to: Option<u32>,
        /// Folder font tambahan selain font sistem (default: studio/tplfonts/ttf kalau ada)
        #[arg(long)]
        fonts: Option<PathBuf>,
    },
    /// Burn-in caption ke video → mp4 (frame caption-core + overlay ffmpeg)
    Render {
        /// Video input
        #[arg(long)]
        video: PathBuf,
        /// File subtitle (.srt / .vtt)
        #[arg(long)]
        srt: PathBuf,
        /// Id template (lihat `rscaps templates`)
        #[arg(long)]
        template: String,
        /// File output (.mp4)
        #[arg(long)]
        out: PathBuf,
        /// Patch params (JSON object)
        #[arg(long)]
        params: Option<PathBuf>,
        /// Folder font tambahan
        #[arg(long)]
        fonts: Option<PathBuf>,
        /// Folder berisi ffmpeg/ffprobe (atau env RSCAPS_FFMPEG_DIR / PATH)
        #[arg(long)]
        ffmpeg_dir: Option<PathBuf>,
        /// x264 CRF (lebih kecil = lebih bagus, biasanya 17–23)
        #[arg(long, default_value_t = 18)]
        crf: u32,
        /// x264 preset
        #[arg(long, default_value = "medium")]
        preset: String,
    },
    /// API server localhost (job render + progress)
    Serve {
        /// Alamat bind
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
        /// Port
        #[arg(long, default_value_t = 8787)]
        port: u16,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Templates => cmd_templates(),
        Cmd::Convert { input, to, out } => cmd_convert(&input, &to, out.as_deref()),
        Cmd::Frames {
            srt,
            template,
            out,
            size,
            fps,
            format,
            params,
            from,
            to,
            fonts,
        } => cmd_frames(CmdFramesArgs {
            srt: &srt,
            template: &template,
            out: &out,
            size: &size,
            fps,
            format: &format,
            params: params.as_deref(),
            from,
            to,
            fonts: fonts.as_deref(),
        }),
        Cmd::Render {
            video,
            srt,
            template,
            out,
            params,
            fonts,
            ffmpeg_dir,
            crf,
            preset,
        } => cmd_render(CmdRenderArgs {
            video: &video,
            srt: &srt,
            template: &template,
            out: &out,
            params: params.as_deref(),
            fonts: fonts.as_deref(),
            ffmpeg_dir: ffmpeg_dir.as_deref(),
            crf,
            preset: &preset,
        }),
        Cmd::Serve { bind, port } => cmd_serve(&bind, port),
    }
}

// ── templates ──────────────────────────────────────────────────────────────

fn cmd_templates() -> Result<()> {
    let raw = caption_core::template_catalog_json();
    let list: Vec<serde_json::Value> = serde_json::from_str(&raw)?;
    for t in &list {
        let id = t["id"].as_str().unwrap_or("?");
        let label = t["label"].as_str().unwrap_or("");
        let category = t["category"].as_str().unwrap_or("");
        println!("{id:<12} {label:<18} {category}");
    }
    println!("({} template)", list.len());
    Ok(())
}

// ── convert ────────────────────────────────────────────────────────────────

fn cmd_convert(input: &Path, to: &str, out: Option<&Path>) -> Result<()> {
    let from = doc::ext_format(input)?;
    let to = to.to_lowercase();
    if to != "srt" && to != "vtt" {
        bail!("format output harus srt atau vtt (didapat: {to})");
    }
    let text = std::fs::read_to_string(input)
        .with_context(|| format!("baca {}", input.display()))?;
    let cues_json = caption_core::parse_subtitle_json(&text, &from)
        .map_err(|err| anyhow::anyhow!("parse subtitle: {err}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&cues_json)?;
    let out_path = match out {
        Some(p) => p.to_path_buf(),
        None => input.with_extension(&to),
    };
    let formatted = caption_core::format_subtitle_json(&parsed["cues"].to_string(), to.as_str())
        .map_err(|err| anyhow::anyhow!("format subtitle: {err}"))?;
    std::fs::write(&out_path, formatted)
        .with_context(|| format!("tulis {}", out_path.display()))?;
    println!(
        "{} → {} ({} cue)",
        input.display(),
        out_path.display(),
        parsed["cues"].as_array().map_or(0, |c| c.len())
    );
    Ok(())
}


// ── frames ─────────────────────────────────────────────────────────────────

struct CmdFramesArgs<'a> {
    srt: &'a Path,
    template: &'a str,
    out: &'a Path,
    size: &'a str,
    fps: u32,
    format: &'a str,
    params: Option<&'a Path>,
    from: Option<u32>,
    to: Option<u32>,
    fonts: Option<&'a Path>,
}

fn cmd_frames(args: CmdFramesArgs<'_>) -> Result<()> {
    let t0 = Instant::now();
    let format = args.format.to_lowercase();
    if format != "svg" && format != "png" {
        bail!("format harus svg atau png (didapat: {})", args.format);
    }
    let (width, height) = doc::parse_size(args.size)?;
    let fps = args.fps.max(1);

    // ── Dokumen: lewat API caption-core (satu sumber kebenaran) ────────────
    let doc: CaptionDocument = doc::build_document(args.srt, args.template, args.params, None)?;
    let duration_frames = doc.duration_frames;

    // ── Range frame ────────────────────────────────────────────────────────
    let from_f = args.from.unwrap_or(0).min(duration_frames);
    let to_f = args.to.unwrap_or(duration_frames).min(duration_frames);
    if from_f >= to_f {
        bail!("range frame kosong ({from_f}..{to_f} dari {duration_frames})");
    }

    std::fs::create_dir_all(args.out)
        .with_context(|| format!("buat folder {}", args.out.display()))?;

    // ── Font: sekali untuk UKURAN kata (measure_words) + raster text→path ─
    let mut font_source = CliFontSource::new();
    font_source.load_system();
    for dir in doc::candidate_font_dirs(args.fonts) {
        if dir.is_dir() {
            font_source.load_dir(&dir);
            eprintln!("fonts: {}", dir.display());
        }
    }
    let svg_opt = svgr::usvgr::Options::default();
    // ── Konteks render (engine membaca ukuran dari sini) ───────────────────
    let ctx = FFramesContext {
        time_base: TimeBase {
            fps: fps as usize,
            sample_rate: 48000,
        },
        current_video_size: VideoSize {
            width: width as usize,
            height: height as usize,
        },
        duration_in_frames: duration_frames as usize,
        mode: FFramesMode::Renderer,
        scenes: None,
        media_source: None,
        font_source: Some(&font_source),
        abort_signal: None,
    };
    let mut cache = svgr::SvgrCache::new(8);
    let pool = svgr::PixmapPool::new();

    // ── Render ─────────────────────────────────────────────────────────────
    let mut written = 0usize;
    let mut empty = 0usize;
    for f in from_f..to_f {
        let now_ms = (f as f32) * 1000.0 / (fps as f32);
        let svg = match caption_core::templates::render(&doc.template, &doc, now_ms, &ctx) {
            Some(svg) => svg,
            None => {
                // Template tak dikenal sudah dicek di atas; None = tak render.
                empty += 1;
                continue;
            }
        };
        let s = svg.to_string();
        if s.is_empty() {
            empty += 1;
            continue; // tidak ada yang tampil di frame ini
        }
        let name = format!("frame_{f:06}");
        match format.as_str() {
            "svg" => {
                let path = args.out.join(format!("{name}.svg"));
                std::fs::write(&path, &s)
                    .with_context(|| format!("tulis {}", path.display()))?;
            }
            "png" => {
                let pixmap = render_png(&s, width, height, &svg_opt, font_source.database(), &mut cache, &pool)
                    .with_context(|| format!("raster {name}"))?;
                let bytes = pixmap
                    .encode_png()
                    .with_context(|| format!("encode {name}.png"))?;
                std::fs::write(args.out.join(format!("{name}.png")), bytes)?;
            }
            _ => unreachable!(),
        }
        written += 1;
        if written % 60 == 0 {
            eprint!("\r{written} frame…");
        }
    }
    eprintln!();
    println!(
        "selesai: {written} frame ({} kosong) → {} · {:?} · {}×{} {fps}fps {format}",
        empty,
        args.out.display(),
        t0.elapsed(),
        width,
        height
    );
    Ok(())
}

fn render_png(
    svg: &str,
    width: u32,
    height: u32,
    opt: &svgr::usvgr::Options,
    fontdb: &svgr::usvgr::fontdb::Database,
    cache: &mut svgr::SvgrCache,
    pool: &svgr::PixmapPool,
) -> Result<svgr::tiny_skia::Pixmap> {
    let tree = svgr::usvgr::Tree::from_str(svg, opt, fontdb)
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
    Ok(pixmap)
}

// ── render (burn-in) ───────────────────────────────────────────────────────

struct CmdRenderArgs<'a> {
    video: &'a Path,
    srt: &'a Path,
    template: &'a str,
    out: &'a Path,
    params: Option<&'a Path>,
    fonts: Option<&'a Path>,
    ffmpeg_dir: Option<&'a Path>,
    crf: u32,
    preset: &'a str,
}

fn cmd_render(args: CmdRenderArgs<'_>) -> Result<()> {
    let t0 = Instant::now();
    // Validasi dulu biar error cepat sebelum spawn ffmpeg.
    let _ = tool_bin("ffprobe", args.ffmpeg_dir)?;
    println!(
        "render {} + {} ({}) → {}",
        args.video.display(),
        args.srt.display(),
        args.template,
        args.out.display()
    );
    let mut last_pct = 0u32;
    render_video(
        &RenderVideoOpts {
            video: args.video,
            srt: args.srt,
            template: args.template,
            out: args.out,
            params: args.params,
            fonts: args.fonts,
            ffmpeg_dir: args.ffmpeg_dir,
            crf: args.crf,
            preset: args.preset,
        },
        |done, total| {
            let pct = if total > 0 { done * 100 / total } else { 0 };
            if pct != last_pct {
                last_pct = pct;
                eprint!("\r{pct}% ({done}/{total})");
            }
        },
    )?;
    eprintln!();
    println!("selesai → {} · {:?}", args.out.display(), t0.elapsed());
    Ok(())
}

// ── serve ──────────────────────────────────────────────────────────────────

fn cmd_serve(bind: &str, port: u16) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("buat tokio runtime")?;
    rt.block_on(server::serve(bind, port))
}
