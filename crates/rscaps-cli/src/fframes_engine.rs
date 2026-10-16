//! Engine "fframes" — burn-in via pipeline fframes penuh (opsi B).
//!
//! `CompositeVideo` = wrapper `CaptionVideo` + frame video sumber sebagai
//! `<image>` di SVG. fframes mengatur: raster, audio timeline, encode.
//! Frame video sumber di-pre-decode via ffmpeg (batas memori, lihat
//! `MAX_PREDECODED_FRAMES`); audio diekstrak ke wav lalu di-mux fframes.
//!
//! Output selalu ukuran canvas caption (1920×1080); frame sumber di-letterbox
//! (`preserveAspectRatio="meet"`, bukan slice) supaya asli aspect ratio terjaga.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use fframes::media::PreloadedAudioData;
use fframes::usvgr::PreloadedImageData;
use fframes::{
    AudioData, AudioMap, AudioTimestamp, AudioTrack, Color, Duration, EncoderOptions,
    FFramesContext, Frame, MediaProvider, RawFontData, RenderOptions, Svgr, Video, render,
};
use caption_core::{CaptionDocument, CaptionVideo};

use crate::doc;
use crate::render::{probe_video, tool_bin};

/// Batas frame yang di-pre-decode ke memori (anti-OOM).
/// 600 frame ≈ 20 detik @ 30fps. Video lebih panjang → tolak, arahkan ke
/// `--engine cpu|skia` (PNG-pipe, streaming, tanpa pre-decode).
const MAX_PREDECODED_FRAMES: usize = 600;

// ── decode frame video sumber → PreloadedImageData ─────────────────────────

/// Decode frame video ke RGBA via ffmpeg rawvideo pipe, di-resample ke
/// `target_fps` supaya index frame cocok dengan `Video::FPS` (CaptionVideo=30).
fn decode_frames_rgba(
    ffmpeg: &Path,
    video: &Path,
    target_fps: u32,
) -> Result<Vec<Arc<PreloadedImageData>>> {
    // Probe dulu untuk tahu ukuran asli.
    let ffprobe = tool_bin("ffprobe", None)?;
    let probe = probe_video(&ffprobe, video)?;
    let width = probe.width;
    let height = probe.height;
    let src_fps = probe.fps_f64();
    let src_frames = (probe.duration_sec * src_fps).ceil().max(1.0) as usize;

    ensure!(
        src_frames <= MAX_PREDECODED_FRAMES,
        "video sumber {src_frames} frame (> {MAX_PREDECODED_FRAMES} batas pre-decode) \
         ≈ {:.0}s @ {src_fps:.1}fps. Pakai `--engine cpu` atau `--engine skia` \
         untuk video panjang (PNG-pipe, streaming, tanpa pre-decode).",
        probe.duration_sec
    );

    let frame_bytes = (width as usize) * (height as usize) * 4;
    let fps_filter = format!("fps={target_fps}");
    let mut child = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .arg("-i")
        .arg(video)
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
            break; // EOF
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

// ── CompositeVideo: CaptionVideo + <image> background ──────────────────────

/// Video wrapper: render caption (via CaptionVideo) + tempel frame video
/// sumber sebagai `<image>` di belakangnya. Dipakai `fframes::render`.
pub struct CompositeVideo {
    caption: CaptionVideo,
    /// Frame video sumber per index (ukuran asli; di-letterbox via SVG).
    frames: Vec<Arc<PreloadedImageData>>,
    /// Nama file audio di media provider ("" = tanpa audio).
    audio_name: String,
    /// Durasi total dalam frame (max caption doc, video sumber).
    duration_frames: usize,
}

impl Video for CompositeVideo {
    // CaptionVideo = 30fps / 1920×1080. Frame sumber di-resample ke 30fps
    // saat decode supaya index cocok; aspect ratio dijaga via "meet".
    const FPS: usize = CaptionVideo::FPS;
    const WIDTH: usize = CaptionVideo::WIDTH;
    const HEIGHT: usize = CaptionVideo::HEIGHT;
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
        let caption_str = caption.to_string();

        let Some(img) = self.frames.get(idx) else {
            return caption; // Lewat durasi video sumber: hanya caption.
        };

        // Suntik <image> setelah tag pembuka <svg ...> caption.
        // Lewati <?xml?>, komentar, dan whitespace dulu supaya posisi sisip
        // benar meski SVG diawali deklarasi/komentar.
        let w = Self::WIDTH;
        let h = Self::HEIGHT;
        // "meet" = letterbox (aspect ratio terjaga, ada bar), bukan "slice"
        // yang memotong. Frame sumber ukuran asli; canvas 1920×1080.
        let image_elem = format!(
            r#"<image href="{}" x="0" y="0" width="{w}" height="{h}" preserveAspectRatio="xMidYMid meet"/>"#,
            img.id
        );

        if let Some(pos) = find_svg_open_end(&caption_str) {
            let mut out = String::with_capacity(caption_str.len() + image_elem.len() + 1);
            out.push_str(&caption_str[..=pos]);
            out.push('\n');
            out.push_str(&image_elem);
            out.push_str(&caption_str[pos + 1..]);
            Svgr::from(out)
        } else {
            caption
        }
    }
}

/// Cari posisi `>` penutup tag pembuka `<svg ...>`, lewati `<?xml?>`, komentar,
/// dan whitespace di awal. Return index karakter `>` (inklusif).
fn find_svg_open_end(svg: &str) -> Option<usize> {
    let bytes = svg.as_bytes();
    let mut i = 0usize;
    // Lewati whitespace + proses instruksi XML + komentar di awal.
    loop {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i..].starts_with(b"<?") {
            // Instruksi XML: cari "?>"
            let rest = &svg[i..];
            let end = rest.find("?>")?;
            i += end + 2;
        } else if bytes[i..].starts_with(b"<!--") {
            // Komentar: cari "-->"
            let rest = &svg[i..];
            let end = rest.find("-->")?;
            i += end + 3;
        } else {
            break;
        }
    }
    // Sekarang harus di "<svg". Cari ">" penutup tag pembuka.
    if !svg[i..].starts_with("<svg") {
        return None;
    }
    svg[i..].find('>').map(|p| i + p)
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

// ── entry point: render burn-in via fframes::render ────────────────────────

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
    /// hwaccel untuk decode ffmpeg (sama seperti jalur overlay).
    pub hwaccel: Option<&'a str>,
}

/// Guard: hapus tmp dir saat keluar (sukses maupun error).
struct TmpDirGuard(std::path::PathBuf);
impl Drop for TmpDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Burn-in caption ke video via pipeline fframes penuh.
pub fn render_fframes(opts: &FframesRenderOpts<'_>) -> Result<()> {
    let ffprobe = tool_bin("ffprobe", opts.ffmpeg_dir)?;
    let ffmpeg = tool_bin("ffmpeg", opts.ffmpeg_dir)?;
    let probe = probe_video(&ffprobe, opts.video)?;
    let fps_f64 = probe.fps_f64();
    let video_frames = (probe.duration_sec * fps_f64).ceil().max(1.0) as usize;

    // Tolak video panjang sebelum decode (anti-OOM).
    ensure!(
        video_frames <= MAX_PREDECODED_FRAMES,
        "video sumber {video_frames} frame (> {MAX_PREDECODED_FRAMES} batas pre-decode) \
         ≈ {:.0}s. Pakai `--engine cpu` atau `--engine skia` untuk video panjang.",
        probe.duration_sec
    );

    // Dokumen caption menutupi seluruh durasi video.
    let doc: CaptionDocument = doc::build_document(
        opts.srt,
        opts.template,
        opts.params,
        Some(video_frames as u32),
    )?;
    let caption_frames = doc.duration_frames as usize;
    let duration_frames = caption_frames.max(video_frames);

    // ── decode frame video sumber ke RGBA (resample ke 30fps) ─────────────
    let target_fps = CompositeVideo::FPS as u32;
    eprintln!(
        "[fframes] decode video sumber ({}x{}, resample ke {target_fps}fps, ≤{MAX_PREDECODED_FRAMES} frame)...",
        probe.width, probe.height
    );
    let frames = decode_frames_rgba(&ffmpeg, opts.video, target_fps)?;

    // ── font ───────────────────────────────────────────────────────────────
    let mut fonts: Vec<RawFontData> = Vec::new();
    for dir in doc::candidate_font_dirs(opts.fonts) {
        if dir.is_dir() {
            collect_font_files(&dir, &mut fonts)?;
        }
    }
    eprintln!("[fframes] {} file font dimuat", fonts.len());

    // ── audio + tmp dir (auto-cleanup via Drop) ────────────────────────────
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

    // ── MediaProvider ──────────────────────────────────────────────────────
    let mut images: HashMap<String, Arc<PreloadedImageData>> = HashMap::new();
    for frame in frames.iter() {
        images.insert(frame.id.clone(), frame.clone());
    }
    let media = FframesMedia {
        images,
        fonts,
        audio: audio_map,
    };

    // ── CompositeVideo ─────────────────────────────────────────────────────
    let video = CompositeVideo {
        caption: CaptionVideo,
        frames,
        audio_name,
        duration_frames,
    };

    // ── encoder ────────────────────────────────────────────────────────────
    let crf_str = opts.crf.to_string();
    let (encoder_name, codec_params): (&str, &[(&str, &str)]) = if opts.hw_encode {
        // NVENC: cq menggantikan crf.
        (
            "h264_nvenc",
            &[("cq", &crf_str), ("preset", opts.preset)],
        )
    } else {
        (
            "libx264",
            &[("crf", &crf_str), ("preset", opts.preset)],
        )
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

    let render_opts = RenderOptions {
        media: Some(&media),
        video_encoder_options: video_encoder,
        audio_encoder_options: EncoderOptions::default(),
        load_system_fonts: true,
        default_font: "Arial",
        ..Default::default()
    };

    eprintln!(
        "[fframes] render {duration_frames} frame → {}",
        opts.out.display()
    );

    // Backend: Skia (Vulkan bila tersedia, fallback CPU) bila gpu-skia;
    // selain itu CPU bawaan fframes.
    #[cfg(feature = "gpu-skia")]
    {
        use fframes_skia_renderer::{
            SkiaCpuCtx, SkiaFFramesRenderer, SkiaPipelineConfig, vulkan::SkiaVulkanCtx,
        };
        let pipeline = SkiaPipelineConfig::default();
        let result = match SkiaVulkanCtx::new(CompositeVideo::WIDTH, CompositeVideo::HEIGHT) {
            Ok(gpu_ctx) => {
                eprintln!("[fframes] backend: Skia Vulkan (GPU)");
                let backend = SkiaFFramesRenderer::new(pipeline, &gpu_ctx);
                render(opts.out, &video, backend, &render_opts)
            }
            Err(err) => {
                eprintln!("[fframes] Vulkan tidak tersedia ({err}) → fallback Skia-CPU");
                let cpu_ctx = SkiaCpuCtx::new(CompositeVideo::WIDTH, CompositeVideo::HEIGHT);
                let backend = SkiaFFramesRenderer::new(pipeline, &cpu_ctx);
                render(opts.out, &video, backend, &render_opts)
            }
        };
        result.map_err(|err| anyhow::anyhow!("fframes render (skia): {err}"))?;
    }
    #[cfg(not(feature = "gpu-skia"))]
    {
        eprintln!("[fframes] backend: CPU (tiny-skia)");
        let backend = fframes::cpu::CpuRenderingBackend::default();
        render(opts.out, &video, backend, &render_opts)
            .map_err(|err| anyhow::anyhow!("fframes render (cpu): {err}"))?;
    }

    eprintln!("[fframes] selesai → {}", opts.out.display());
    Ok(())
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Kumpulkan file .ttf/.otf/.ttc dari folder (non-rekursif) ke RawFontData.
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

/// Ekstrak audio video sumber ke WAV stereo 48kHz (opsional hwaccel decode).
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
