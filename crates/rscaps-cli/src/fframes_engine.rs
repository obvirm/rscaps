//! Engine "fframes" — burn-in via pipeline fframes penuh (opsi B).
//!
//! `CompositeVideo` = wrapper `CaptionVideo` + frame video sumber sebagai
//! `<image>` di SVG. fframes mengatur: raster, audio timeline, encode.
//!
//! Dimensi kanvas mengikuti **ukuran video nyata** via const generics
//! (`CompositeVideo<W, H>`) — portrait (1080×1920) dan landscape (1920×1080)
//! keduanya benar. Frame video sumber di-pre-decode (batas
//! `MAX_PREDECODED_FRAMES`); audio diekstrak ke WAV lalu di-mux fframes.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use fframes::media::PreloadedAudioData;
use fframes::usvgr::PreloadedImageData;
use fframes::{
    AudioData, AudioMap, AudioTimestamp, AudioTrack, Color, Duration, EncoderOptions,
    FFramesContext, Frame, MediaProvider, RawFontData, RenderOptions, Svgr, Video, render,
};
use caption_core::{CaptionDocument, CaptionVideo};

use crate::doc;
use crate::render::{probe_video, tool_bin};

/// Ukuran potongan (chunk): hanya chunk ini yang di-pre-decode ke RAM.
/// Video lebih panjang dirender per potongan (fframes `frame_range`) lalu
/// digabung dengan ffmpeg concat — jadi tak ada cap total. Override lewat env
/// `RSCAPS_FFRAMES_CHUNK` (untuk uji).
const CHUNK_FRAMES: usize = 600;

fn chunk_frames() -> usize {
    std::env::var("RSCAPS_FFRAMES_CHUNK")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(CHUNK_FRAMES)
}

// ── decode frame video sumber → PreloadedImageData ─────────────────────────

/// Decode frame video ke RGBA via ffmpeg rawvideo pipe, di-resample ke
/// `target_fps` supaya index frame cocok dengan `Video::FPS` (CaptionVideo=30).
/// `width`/`height` = ukuran sumber (dari probe pemanggil). `hwaccel`
/// diteruskan ke ffmpeg (decode GPU lalu download ke CPU — tanpa
/// `-hwaccel_output_format`, jadi output rawvideo tetap kompatibel).
/// `start_sec` = seek (0 = awal); `max_frames` = batas jumlah frame
/// (0 = sampai EOF). Dipakai per-potongan saat chunked render.
fn decode_frames_rgba(
    ffmpeg: &Path,
    video: &Path,
    width: u32,
    height: u32,
    target_fps: u32,
    hwaccel: Option<&str>,
    start_sec: f64,
    max_frames: usize,
) -> Result<Vec<Arc<PreloadedImageData>>> {
    let frame_bytes = (width as usize) * (height as usize) * 4;
    let fps_filter = format!("fps={target_fps}");
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
    if let Some(ha) = hwaccel {
        cmd.args(["-hwaccel", ha]);
    }
    if start_sec > 0.0 {
        // Seek sebelum input (frame-accurate di ffmpeg modern).
        cmd.args(["-ss", &format!("{start_sec}")]);
    }
    cmd.arg("-i").arg(video);
    if max_frames > 0 {
        cmd.args(["-frames:v", &max_frames.to_string()]);
    }
    let mut child = cmd
        .args(["-vf", &fps_filter])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("jalankan {}", ffmpeg.display()))?;

    let mut stdout = child.stdout.take().context("ffmpeg stdout")?;
    let mut frames = Vec::new();
    let mut buf = vec![0u8; frame_bytes];
    let mut idx = 0usize;

    loop {
        let mut read_total = 0usize;
        while read_total < frame_bytes {
            match stdout.read(&mut buf[read_total..]) {
                Ok(0) => break,
                Ok(n) => read_total += n,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err).context("baca rawvideo"),
            }
        }
        if read_total < frame_bytes {
            break;
        }
        let id = format!("frame_{idx}");
        frames.push(Arc::new(PreloadedImageData::new(id, width, height, &buf)));
        idx += 1;
    }

    let status = child.wait().context("tunggu ffmpeg")?;
    ensure!(status.success(), "ffmpeg decode exit non-zero");
    ensure!(
        !frames.is_empty(),
        "tidak ada frame ter-decode dari video sumber"
    );
    Ok(frames)
}

// ── CompositeVideo<const W, const H>: kanvas = ukuran video nyata ─────────

/// Video wrapper: caption (CaptionVideo) + frame video sumber sebagai `<image>`.
/// Dimensi kanvas = W×H (dipilih runtime via dispatch, lihat `render_fframes`).
pub struct CompositeVideo<const W: usize, const H: usize> {
    caption: CaptionVideo,
    frames: Vec<Arc<PreloadedImageData>>,
    /// Index frame global dari `frames[0]` (offset chunk; 0 = seluruh video).
    frame_offset: usize,
    audio_name: String,
    duration_frames: usize,
    /// Prefix SVG caption (tag pembuka `<svg …>`) — di-cache setelah frame
    /// pertama supaya tidak scan ulang tiap frame.
    svg_prefix: std::sync::OnceLock<String>,
}

impl<const W: usize, const H: usize> Video for CompositeVideo<W, H> {
    const FPS: usize = CaptionVideo::FPS;
    const WIDTH: usize = W;
    const HEIGHT: usize = H;
    const BACKGROUND_COLOR: Color = Color::TRANSPARENT;

    fn duration(&self) -> Duration<'_> {
        Duration::Frames(self.duration_frames)
    }

    fn audio(&self) -> AudioMap<'_> {
        if self.audio_name.is_empty() {
            AudioMap::none()
        } else {
            AudioMap::from([AudioTrack::new(
                self.audio_name.as_str(),
                AudioTimestamp::Second(0.)..AudioTimestamp::Eof,
            )])
        }
    }

    fn render_frame<'a>(&'a self, frame: Frame, ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        let idx = frame.index;
        let caption = self.caption.render_frame(frame, ctx);

        // frame.index = index global (renderer menambah frame_offset saat
        // render range) → ubah ke index lokal chunk.
        let Some(local) = idx.checked_sub(self.frame_offset) else {
            return caption;
        };
        let Some(img) = self.frames.get(local) else {
            return caption;
        };

        let caption_str = caption.to_string();

        let w = W;
        let h = H;
        // "meet" = letterbox (aspect ratio terjaga). Frame sumber ukuran asli;
        // kanvas W×H sesuai video nyata (portrait/landscape benar).
        let image_elem = format!(
            r#"<image href="{}" x="0" y="0" width="{w}" height="{h}" preserveAspectRatio="xMidYMid meet"/>"#,
            img.id
        );

        // Cache prefix (tag pembuka <svg …>) setelah frame pertama.
        let prefix = self.svg_prefix.get_or_init(|| {
            find_svg_open_prefix(&caption_str)
                .unwrap_or_else(|| caption_str.clone())
        });

        // Kalau prefix = seluruh caption (tidak ada <svg> valid), fallback caption.
        if prefix.len() >= caption_str.len() {
            return caption;
        }

        let suffix = &caption_str[prefix.len()..];
        let mut out = String::with_capacity(prefix.len() + image_elem.len() + suffix.len());
        out.push_str(prefix);
        out.push('\n');
        out.push_str(&image_elem);
        out.push_str(suffix);
        Svgr::from(out)
    }
}

/// Return prefix SVG = dari awal sampai dan termasuk `<svg …>` pembuka
/// (lewati `<?xml?>`, komentar, whitespace). `None` kalau tidak ketemu.
fn find_svg_open_prefix(svg: &str) -> Option<String> {
    let bytes = svg.as_bytes();
    let mut i = 0usize;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i..].starts_with(b"<?") {
            let rest = &svg[i..];
            let end = rest.find("?>")?;
            i += end + 2;
        } else if bytes[i..].starts_with(b"<!--") {
            let rest = &svg[i..];
            let end = rest.find("-->")?;
            i += end + 3;
        } else {
            break;
        }
    }
    if !svg[i..].starts_with("<svg") {
        return None;
    }
    let close = svg[i..].find('>')?;
    Some(svg[..i + close + 1].to_string())
}

// ── MediaProvider: frame video + fonts + audio ─────────────────────────────

struct FframesMedia {
    images: HashMap<String, Arc<PreloadedImageData>>,
    fonts: Vec<RawFontData>,
    audio: HashMap<String, AudioData<'static>>,
}

impl std::fmt::Debug for FframesMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FframesMedia")
            .field("images", &self.images.len())
            .field("fonts", &self.fonts.len())
            .field("audio", &self.audio.len())
            .finish()
    }
}

impl MediaProvider<'_> for FframesMedia {
    fn resolve_audio(&self, name: &str) -> Option<&AudioData<'_>> {
        self.audio.get(name)
    }

    fn resolve_image(&self, _name: &str) -> Option<&fframes::media::ImageData<'_>> {
        None
    }

    fn resolve_subtitles(&self, _name: &str) -> Option<&fframes::media::Subtitles<'_>> {
        None
    }

    fn resolve_video(&self, _name: &str) -> Option<&fframes::media::VideoMedia> {
        None
    }

    fn populate_font_source(&self, font_source: &mut dyn fframes::FontSource) {
        for font in &self.fonts {
            font_source.add_font(font.file_name.clone(), font.data.clone());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn populate_image_source(
        &self,
        image_data: &mut HashMap<String, Arc<PreloadedImageData>>,
    ) {
        for (key, img) in &self.images {
            image_data.insert(key.clone(), img.clone());
        }
    }
}

// ── entry point ────────────────────────────────────────────────────────────

pub struct FframesRenderOpts<'a> {
    pub video: &'a Path,
    pub srt: &'a Path,
    pub template: &'a str,
    pub out: &'a Path,
    pub params: Option<&'a Path>,
    pub fonts: Option<&'a Path>,
    pub ffmpeg_dir: Option<&'a Path>,
    pub crf: u32,
    pub preset: &'a str,
    pub hw_encode: bool,
    pub hwaccel: Option<&'a str>,
}

struct TmpDirGuard(std::path::PathBuf);
impl Drop for TmpDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Burn-in caption ke video via pipeline fframes penuh.
/// Kanvas mengikuti ukuran video nyata (const generics + dispatch).
pub fn render_fframes(opts: &FframesRenderOpts<'_>) -> Result<()> {
    let ffprobe = tool_bin("ffprobe", opts.ffmpeg_dir)?;
    let ffmpeg = tool_bin("ffmpeg", opts.ffmpeg_dir)?;
    let probe = probe_video(&ffprobe, opts.video)?;

    // Bulatkan ke genap (encoder yuv420 reject dimensi ganjil).
    let src_w = ((probe.width as usize) + 1) & !1;
    let src_h = ((probe.height as usize) + 1) & !1;
    ensure!(src_w >= 2 && src_h >= 2, "ukuran video tidak valid: {src_w}x{src_h}");

    let fps_f64 = probe.fps_f64();
    let video_frames = (probe.duration_sec * fps_f64).ceil().max(1.0) as usize;

    let doc: CaptionDocument = doc::build_document(
        opts.srt,
        opts.template,
        opts.params,
        Some(video_frames as u32),
    )?;
    let caption_frames = doc.duration_frames as usize;
    let duration_frames = caption_frames.max(video_frames);

    // Semua decode di-resample ke fps CaptionVideo (30) supaya index cocok.
    let target_fps = CaptionVideo::FPS as u32;

    // Font.
    let mut fonts: Vec<RawFontData> = Vec::new();
    for dir in doc::candidate_font_dirs(opts.fonts) {
        if dir.is_dir() {
            collect_font_files(&dir, &mut fonts)?;
        }
    }
    eprintln!("[fframes] {} file font dimuat", fonts.len());

    // Audio + tmp dir (auto-cleanup via Drop).
    let tmp_dir = std::env::temp_dir().join(format!("rscaps-fframes-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).context("buat tmp dir")?;
    let _tmp_guard = TmpDirGuard(tmp_dir.clone());

    let mut audio_map: HashMap<String, AudioData<'static>> = HashMap::new();
    let mut audio_name = String::new();
    if probe.has_audio {
        let wav_path = tmp_dir.join("audio.wav");
        extract_audio_wav(&ffmpeg, opts.video, &wav_path, opts.hwaccel)?;
        match PreloadedAudioData::decode_raw_file_stereo(None, &wav_path) {
            Ok(audio_data) => {
                audio_name = "audio.wav".to_string();
                audio_map.insert(audio_name.clone(), AudioData::Preloaded(audio_data));
                eprintln!("[fframes] audio diekstrak → {}", wav_path.display());
            }
            Err(err) => {
                eprintln!("[fframes] WARN: gagal decode audio ({err:?}) - render tanpa audio");
            }
        }
    }

    // Encoder: NVENC bila diminta & tersedia, selain itu libx264.
    let crf_str = opts.crf.to_string();
    let use_nvenc = opts.hw_encode && nvenc_available(&ffmpeg);
    if opts.hw_encode && !use_nvenc {
        eprintln!("[fframes] WARN: --hw-encode diminta tapi h264_nvenc tidak ditemukan → fallback libx264");
    }
    let (encoder_name, codec_params): (&str, &[(&str, &str)]) = if use_nvenc {
        ("h264_nvenc", &[("cq", &crf_str), ("preset", opts.preset)])
    } else {
        ("libx264", &[("crf", &crf_str), ("preset", opts.preset)])
    };
    let video_encoder = EncoderOptions {
        preferred_encoder: Some(encoder_name),
        codec_params: Some(codec_params),
        ..Default::default()
    };

    if let Some(parent) = opts.out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("buat folder {}", parent.display()))?;
        }
    }

    let chunk = chunk_frames();

    if duration_frames <= chunk {
        // Satu kali render — seluruh video di-pre-decode (bounded `chunk`).
        eprintln!(
            "[fframes] decode video sumber ({src_w}x{src_h}, resample ke {target_fps}fps, {duration_frames} frame)..."
        );
        let frames = decode_frames_rgba(
            &ffmpeg, opts.video, probe.width, probe.height, target_fps, opts.hwaccel, 0.0, 0,
        )?;
        let media = FframesMedia {
            images: images_of(&frames),
            fonts: fonts.clone(),
            audio: audio_map.clone(),
        };
        let render_opts = RenderOptions {
            media: Some(&media),
            video_encoder_options: video_encoder,
            audio_encoder_options: EncoderOptions::default(),
            load_system_fonts: true,
            default_font: "Arial",
            ..Default::default()
        };
        eprintln!(
            "[fframes] render {duration_frames} frame ({src_w}x{src_h}) → {}",
            opts.out.display()
        );
        return dispatch_render(
            src_w, src_h, frames, 0, audio_name, duration_frames, &render_opts, opts.out,
        );
    }

    // Video panjang → potongan ≤ `chunk` frame, digabung dengan ffmpeg concat.
    // Hanya potongan aktif yang ada di RAM, jadi tak ada cap total.
    eprintln!("[fframes] video {duration_frames} frame > {chunk} → render per potongan");
    let mut segments: Vec<PathBuf> = Vec::new();
    let mut start = 0usize;
    let mut seg_idx = 0usize;
    while start < duration_frames {
        let end = (start + chunk).min(duration_frames);
        let count = end - start;
        let start_sec = start as f64 / target_fps as f64;
        eprintln!("[fframes] potongan {seg_idx}: frame {start}..{end} ({count})");

        let frames = decode_frames_rgba(
            &ffmpeg,
            opts.video,
            probe.width,
            probe.height,
            target_fps,
            opts.hwaccel,
            start_sec,
            count,
        )?;
        let media = FframesMedia {
            images: images_of(&frames),
            fonts: fonts.clone(),
            audio: audio_map.clone(),
        };
        let seg_path = tmp_dir.join(format!("seg_{seg_idx}.mp4"));
        let render_opts = RenderOptions {
            media: Some(&media),
            video_encoder_options: video_encoder.clone(),
            audio_encoder_options: EncoderOptions::default(),
            load_system_fonts: true,
            default_font: "Arial",
            frame_range: Some(start..end),
            ..Default::default()
        };
        dispatch_render(
            src_w,
            src_h,
            frames,
            start,
            audio_name.clone(),
            duration_frames,
            &render_opts,
            &seg_path,
        )?;
        segments.push(seg_path);
        start = end;
        seg_idx += 1;
    }

    concat_segments(&ffmpeg, &segments, opts.out)?;
    eprintln!(
        "[fframes] selesai → {} ({} potongan)",
        opts.out.display(),
        segments.len()
    );
    Ok(())
}

/// Dispatch ke `CompositeVideo<W, H>` untuk ukuran umum; selain itu pakai
/// pasangan terdekat yang se-aspek (untuk aspect ratio aneh).
fn dispatch_render(
    w: usize,
    h: usize,
    frames: Vec<Arc<PreloadedImageData>>,
    frame_offset: usize,
    audio_name: String,
    duration_frames: usize,
    render_opts: &RenderOptions<'_, '_>,
    out: &Path,
) -> Result<()> {
    // Pasangan umum: landscape, portrait, square, 4:3, 3:2.
    macro_rules! try_size {
        ($cw:expr, $ch:expr) => {
            if w == $cw && h == $ch {
                return render_sized::<$cw, $ch>(
                    frames,
                    frame_offset,
                    audio_name,
                    duration_frames,
                    render_opts,
                    out,
                );
            }
        };
    }
    // Landscape 16:9.
    try_size!(1920, 1080);
    try_size!(1280, 720);
    try_size!(3840, 2160);
    try_size!(854, 480);
    try_size!(640, 360);
    try_size!(320, 180);
    // Landscape 4:3.
    try_size!(1024, 768);
    try_size!(800, 600);
    try_size!(640, 480);
    try_size!(320, 240);
    // Landscape 3:2.
    try_size!(1440, 960);
    try_size!(1080, 720);
    try_size!(720, 480);
    // Portrait 9:16 (Shorts/Reels/TikTok).
    try_size!(1080, 1920);
    try_size!(720, 1280);
    try_size!(2160, 3840);
    try_size!(480, 854);
    try_size!(360, 640);
    try_size!(180, 320);
    // Portrait 3:4.
    try_size!(768, 1024);
    try_size!(600, 800);
    try_size!(480, 640);
    try_size!(240, 320);
    // Square.
    try_size!(1080, 1080);
    try_size!(720, 720);
    try_size!(512, 512);

    // Ukuran tidak umum: pakai pasangan terdekat yang SE-ASPEK tanpa
    // scale_resolution (scale_resolution men-scale seragam → merusak aspect).
    // "meet" di SVG menjaga aspect ratio source di dalam kanvas terdekat.
    let aspect = w as f64 / h as f64;
    let (bw, bh) = closest_aspect_size(aspect);
    eprintln!(
        "[fframes] ukuran {w}x{h} tidak umum → kanvas {bw}x{bh} (aspect dijaga 'meet')"
    );
    render_sized_generic(
        bw,
        bh,
        frames,
        frame_offset,
        audio_name,
        duration_frames,
        render_opts,
        out,
    )
}

/// Render dengan ukuran kanvas spesifik (const generic).
fn render_sized<const W: usize, const H: usize>(
    frames: Vec<Arc<PreloadedImageData>>,
    frame_offset: usize,
    audio_name: String,
    duration_frames: usize,
    render_opts: &RenderOptions<'_, '_>,
    out: &Path,
) -> Result<()> {
    let video = CompositeVideo::<W, H> {
        caption: CaptionVideo,
        frames,
        frame_offset,
        audio_name,
        duration_frames,
        svg_prefix: std::sync::OnceLock::new(),
    };
    run_backend(&video, render_opts, out)
}

/// Render dengan ukuran runtime (dispatch ke const generic terdekat).
fn render_sized_generic(
    w: usize,
    h: usize,
    frames: Vec<Arc<PreloadedImageData>>,
    frame_offset: usize,
    audio_name: String,
    duration_frames: usize,
    render_opts: &RenderOptions<'_, '_>,
    out: &Path,
) -> Result<()> {
    // Delegate ke pasangan umum terdekat; aspect sudah dijaga "meet".
    macro_rules! try_size {
        ($cw:expr, $ch:expr) => {
            if w == $cw && h == $ch {
                return render_sized::<$cw, $ch>(
                    frames,
                    frame_offset,
                    audio_name,
                    duration_frames,
                    render_opts,
                    out,
                );
            }
        };
    }
    try_size!(1920, 1080);
    try_size!(1280, 720);
    try_size!(1080, 1920);
    try_size!(720, 1280);
    try_size!(1080, 1080);
    try_size!(1024, 768);
    try_size!(800, 600);
    try_size!(640, 480);
    try_size!(320, 240);
    try_size!(1440, 960);
    try_size!(1080, 720);
    try_size!(720, 480);
    try_size!(768, 1024);
    try_size!(600, 800);
    try_size!(480, 640);
    try_size!(240, 320);
    // Fallback: 1920×1080.
    eprintln!("[fframes] WARN: fallback kanvas 1920x1080");
    render_sized::<1920, 1080>(
        frames,
        frame_offset,
        audio_name,
        duration_frames,
        render_opts,
        out,
    )
}

/// Cari pasangan umum yang paling se-aspek dengan `aspect`.
/// Karena pencarian pakai `<` ketat, ia selalu mengembalikan anggota PERTAMA
/// per keluarga aspek (mis. 16:9 → 1920×1080), yang semuanya ada di daftar
/// dispatch `render_sized_generic` — jadi fallback 1920×1080 tak tercapai.
fn closest_aspect_size(aspect: f64) -> (usize, usize) {
    const COMMON: &[(usize, usize)] = &[
        // Landscape 16:9.
        (1920, 1080), (1280, 720), (3840, 2160), (854, 480), (640, 360),
        // Landscape 4:3.
        (1024, 768), (800, 600), (640, 480), (320, 240),
        // Landscape 3:2.
        (1440, 960), (1080, 720), (720, 480),
        // Portrait 9:16.
        (1080, 1920), (720, 1280), (2160, 3840), (480, 854), (360, 640),
        // Portrait 3:4.
        (768, 1024), (600, 800), (480, 640), (240, 320),
        // Square.
        (1080, 1080), (720, 720), (512, 512),
    ];
    let mut best = COMMON[0];
    let mut best_diff = f64::MAX;
    for &(cw, ch) in COMMON {
        let ca = cw as f64 / ch as f64;
        let diff = (ca - aspect).abs();
        if diff < best_diff {
            best_diff = diff;
            best = (cw, ch);
        }
    }
    best
}

/// Jalankan backend render (Skia bila gpu-skia, selain itu CPU).
fn run_backend<const W: usize, const H: usize>(
    video: &CompositeVideo<W, H>,
    render_opts: &RenderOptions<'_, '_>,
    out: &Path,
) -> Result<()> {
    #[cfg(feature = "gpu-skia")]
    {
        use fframes_skia_renderer::{
            SkiaCpuCtx, SkiaFFramesRenderer, SkiaPipelineConfig, vulkan::SkiaVulkanCtx,
        };
        let pipeline = SkiaPipelineConfig::default();
        let result = match SkiaVulkanCtx::new(W, H) {
            Ok(gpu_ctx) => {
                eprintln!("[fframes] backend: Skia Vulkan (GPU) {W}x{H}");
                let backend = SkiaFFramesRenderer::new(pipeline, &gpu_ctx);
                render(out, video, backend, render_opts)
            }
            Err(err) => {
                eprintln!("[fframes] Vulkan tidak tersedia ({err}) → fallback Skia-CPU");
                let cpu_ctx = SkiaCpuCtx::new(W, H);
                let backend = SkiaFFramesRenderer::new(pipeline, &cpu_ctx);
                render(out, video, backend, render_opts)
            }
        };
        result.map_err(|err| anyhow::anyhow!("fframes render (skia): {err}"))?;
    }
    #[cfg(not(feature = "gpu-skia"))]
    {
        eprintln!("[fframes] backend: CPU (tiny-skia) {W}x{H}");
        let backend = fframes::cpu::CpuRenderingBackend::default();
        render(out, video, backend, render_opts)
            .map_err(|err| anyhow::anyhow!("fframes render (cpu): {err}"))?;
    }
    Ok(())
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Kumpulkan frame ter-decode jadi map `id → image` untuk MediaProvider.
fn images_of(frames: &[Arc<PreloadedImageData>]) -> HashMap<String, Arc<PreloadedImageData>> {
    frames.iter().map(|f| (f.id.clone(), f.clone())).collect()
}

/// Gabungkan segmen mp4 hasil chunked render → satu file (`-c copy`; semua
/// segmen punya codec/parameter sama karena encoder yang sama).
fn concat_segments(ffmpeg: &Path, segments: &[PathBuf], out: &Path) -> Result<()> {
    ensure!(!segments.is_empty(), "tidak ada segmen untuk digabung");
    if segments.len() == 1 {
        std::fs::copy(&segments[0], out)
            .with_context(|| format!("copy {} → {}", segments[0].display(), out.display()))?;
        return Ok(());
    }
    let list = segments[0]
        .parent()
        .context("segmen tanpa parent dir")?
        .join("concat.txt");
    let mut body = String::new();
    for seg in segments {
        // Path concat demuxer pakai '/' (hindari escape backslash Windows).
        body.push_str(&format!("file '{}'\n", seg.to_string_lossy().replace('\\', "/")));
    }
    std::fs::write(&list, body).with_context(|| format!("tulis {}", list.display()))?;

    let status = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .args(["-c", "copy"])
        .arg(out)
        .status()
        .with_context(|| format!("jalankan {}", ffmpeg.display()))?;
    ensure!(status.success(), "ffmpeg concat exit non-zero");
    Ok(())
}

/// Cek apakah `h264_nvenc` tersedia di ffmpeg build ini.
fn nvenc_available(ffmpeg: &Path) -> bool {
    Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout).contains("h264_nvenc")
        })
        .unwrap_or(false)
}

fn collect_font_files(dir: &Path, out: &mut Vec<RawFontData>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("baca {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
            continue;
        }
        let bytes =
            std::fs::read(&path).with_context(|| format!("baca font {}", path.display()))?;
        let name = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "font.ttf".to_string());
        out.push(RawFontData {
            file_name: name,
            data: Arc::new(bytes),
        });
    }
    Ok(())
}

fn extract_audio_wav(
    ffmpeg: &Path,
    video: &Path,
    out: &Path,
    hwaccel: Option<&str>,
) -> Result<()> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
    if let Some(ha) = hwaccel {
        cmd.args(["-hwaccel", ha]);
    }
    let status = cmd
        .arg("-i")
        .arg(video)
        .args(["-vn", "-ac", "2", "-ar", "48000", "-c:a", "pcm_s16le"])
        .arg(out)
        .status()
        .with_context(|| format!("jalankan {}", ffmpeg.display()))?;
    ensure!(status.success(), "ffmpeg extract audio exit non-zero");
    Ok(())
}
