//! Engine "fframes" — burn-in via pipeline fframes penuh (opsi B).
//!
//! `CompositeVideo` = wrapper `CaptionVideo` + frame video sumber sebagai
//! `<image>` di SVG. fframes mengatur: seek, decode, raster, audio timeline,
//! encode. CLI hanya membangun MediaProvider + Video lalu memanggil
//! `fframes::render`.
//!
//! Batasan v1: frame video sumber di-pre-decode ke memori (cocok untuk clip
//! pendek / test; long video = butuh chunked rendering, menyusul).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use fframes::media::{PreloadedAudioData, VideoMedia};
use fframes::usvgr::PreloadedImageData;
use fframes::{
    AudioData, AudioMap, AudioTimestamp, AudioTrack, Color, Duration, EncoderOptions,
    FFramesContext, Frame, MediaProvider, RawFontData, RenderOptions, Svgr, Video, render,
};
use caption_core::{CaptionDocument, CaptionVideo};

use crate::doc;
use crate::render::{probe_video, tool_bin};

// ── decode frame video sumber → PreloadedImageData ─────────────────────────

/// Decode semua frame video ke RGBA via ffmpeg rawvideo pipe.
/// Mengembalikan (width, height, frames) di mana tiap frame = PreloadedImageData.
fn decode_frames_rgba(
    ffmpeg: &Path,
    video: &Path,
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
) -> Result<Vec<Arc<PreloadedImageData>>> {
    let frame_bytes = (width as usize) * (height as usize) * 4;
    let mut child = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .arg("-i")
        .arg(video)
        .args(["-vf", &format!("fps={fps_num}/{fps_den}")])
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
            break; // EOF - frame terakhir tidak lengkap / habis
        }
        let id = format!("frame_{idx}");
        frames.push(Arc::new(PreloadedImageData::new(
            id,
            width,
            height,
            &buf,
        )));
        idx += 1;
    }

    let status = child.wait().context("tunggu ffmpeg")?;
    ensure!(status.success(), "ffmpeg decode exit non-zero");
    ensure!(!frames.is_empty(), "tidak ada frame ter-decode dari video sumber");
    Ok(frames)
}

// ── CompositeVideo: CaptionVideo + <image> background ──────────────────────

/// Video wrapper: render caption (via CaptionVideo) + tempel frame video
/// sumber sebagai `<image>` di belakangnya. Dipakai `fframes::render`.
pub struct CompositeVideo {
    caption: CaptionVideo,
    /// Frame video sumber per index (sudah di-scale ke canvas).
    frames: Vec<Arc<PreloadedImageData>>,
    /// Nama file audio di media provider ("" = tanpa audio).
    audio_name: String,
    /// Durasi total dalam frame (max caption doc, video sumber).
    duration_frames: usize,
}

impl Video for CompositeVideo {
    const FPS: usize = CaptionVideo::FPS;
    const WIDTH: usize = CaptionVideo::WIDTH;
    const HEIGHT: usize = CaptionVideo::HEIGHT;
    // Background transparan - video frame digambar via <image>, caption di atas.
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
        // Caption SVG dari CaptionVideo (sama persis dengan preview/CLI lama).
        let caption = self.caption.render_frame(frame, ctx);
        let caption_str = caption.to_string();

        // Frame video sumber (kalau ada).
        let Some(img) = self.frames.get(idx) else {
            // Lewat durasi video sumber: hanya caption.
            return caption;
        };

        // Suntik <image> tepat setelah tag pembuka <svg ...> caption.
        // href = id PreloadedImageData yang terdaftar di image_data HashMap.
        let w = Self::WIDTH;
        let h = Self::HEIGHT;
        let image_elem = format!(
            r#"<image href="{}" x="0" y="0" width="{w}" height="{h}" preserveAspectRatio="xMidYMid slice"/>"#,
            img.id
        );

        if let Some(pos) = caption_str.find('>') {
            let mut out = String::with_capacity(caption_str.len() + image_elem.len() + 1);
            out.push_str(&caption_str[..=pos]);
            out.push('\n');
            out.push_str(&image_elem);
            out.push_str(&caption_str[pos + 1..]);
            Svgr::from(out)
        } else {
            // SVG aneh - fallback caption saja.
            caption
        }
    }
}

// ── MediaProvider dengan frame video + fonts + audio ───────────────────────

/// Media provider statis: frame video (sebagai image), font, audio.
/// `populate_image_source` mengisi HashMap yang dipakai usvg resolve `<image href>`.
struct FframesMedia {
    images: HashMap<String, Arc<PreloadedImageData>>,
    fonts: Vec<RawFontData>,
    audio: HashMap<String, AudioData<'static>>,
    videos: HashMap<String, VideoMedia>,
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

    fn resolve_video(&self, name: &str) -> Option<&VideoMedia> {
        self.videos.get(name)
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
}

/// Burn-in caption ke video via pipeline fframes penuh:
/// CaptionVideo + frame video sumber → fframes::render (raster + encode + audio).
pub fn render_fframes(opts: &FframesRenderOpts<'_>) -> Result<()> {
    let ffprobe = tool_bin("ffprobe", opts.ffmpeg_dir)?;
    let ffmpeg = tool_bin("ffmpeg", opts.ffmpeg_dir)?;
    let probe = probe_video(&ffprobe, opts.video)?;
    let fps_num = probe.fps_num.max(1);
    let fps_den = probe.fps_den.max(1);
    let fps_f64 = probe.fps_f64();
    let video_frames = (probe.duration_sec * fps_f64).ceil().max(1.0) as usize;

    // Dokumen caption menutupi seluruh durasi video.
    let doc: CaptionDocument = doc::build_document(
        opts.srt,
        opts.template,
        opts.params,
        Some(video_frames as u32),
    )?;
    let caption_frames = doc.duration_frames as usize;
    let duration_frames = caption_frames.max(video_frames);

    // ── decode frame video sumber ke RGBA ──────────────────────────────────
    eprintln!(
        "[fframes] decode {} frame video sumber ({}x{})...",
        video_frames, probe.width, probe.height
    );
    let frames = decode_frames_rgba(
        &ffmpeg,
        opts.video,
        probe.width,
        probe.height,
        fps_num,
        fps_den,
    )?;

    // ── kumpulkan font → RawFontData ───────────────────────────────────────
    let mut fonts: Vec<RawFontData> = Vec::new();
    for dir in doc::candidate_font_dirs(opts.fonts) {
        if !dir.is_dir() {
            continue;
        }
        collect_font_files(&dir, &mut fonts)?;
    }
    eprintln!("[fframes] {} file font dimuat", fonts.len());

    // ── audio: ekstrak ke wav sementara bila video punya audio ────────────
    let mut audio_map: HashMap<String, AudioData<'static>> = HashMap::new();
    let mut audio_name = String::new();
    let tmp_dir = std::env::temp_dir().join(format!(
        "rscaps-fframes-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp_dir).context("buat tmp dir")?;
    if probe.has_audio {
        let wav_path = tmp_dir.join("audio.wav");
        extract_audio_wav(&ffmpeg, opts.video, &wav_path)?;
        match PreloadedAudioData::decode_raw_file_stereo(None, &wav_path) {
            Ok(audio_data) => {
                audio_name = "audio.wav".to_string();
                audio_map.insert(
                    audio_name.clone(),
                    AudioData::Preloaded(audio_data),
                );
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

    let mut videos: HashMap<String, VideoMedia> = HashMap::new();
    let video_key = opts
        .video
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "source".to_string());
    videos.insert(
        video_key.clone(),
        VideoMedia {
            path: opts.video.to_path_buf(),
            metadata: None,
        },
    );

    let media = FframesMedia {
        images,
        fonts,
        audio: audio_map,
        videos,
    };

    // ── CompositeVideo ─────────────────────────────────────────────────────
    let video = CompositeVideo {
        caption: CaptionVideo,
        frames,
        audio_name,
        duration_frames,
    };

    // ── RenderOptions + encoder ────────────────────────────────────────────
    let crf_str = opts.crf.to_string();
    let codec_params: &[(&str, &str)] = if opts.hw_encode {
        // NVENC: cq menggantikan crf.
        &[("cq", &crf_str), ("preset", opts.preset)]
    } else {
        &[("crf", &crf_str), ("preset", opts.preset)]
    };

    let video_encoder = EncoderOptions {
        preferred_encoder: Some(if opts.hw_encode {
            "h264_nvenc"
        } else {
            "libx264"
        }),
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
        "[fframes] render {} frame → {}",
        duration_frames,
        opts.out.display()
    );

    // Backend: Skia GPU bila gpu-skia, selain itu CPU bawaan fframes.
    #[cfg(feature = "gpu-skia")]
    {
        use fframes_skia_renderer::{SkiaCpuCtx, SkiaFFramesRenderer, SkiaPipelineConfig};
        let ctx = SkiaCpuCtx::new(CompositeVideo::WIDTH, CompositeVideo::HEIGHT);
        let backend = SkiaFFramesRenderer::new(SkiaPipelineConfig::default(), &ctx);
        render(opts.out, &video, backend, &render_opts)
            .map_err(|err| anyhow::anyhow!("fframes render (skia): {err}"))?;
    }
    #[cfg(not(feature = "gpu-skia"))]
    {
        let backend = fframes::cpu::CpuRenderingBackend::default();
        render(opts.out, &video, backend, &render_opts)
            .map_err(|err| anyhow::anyhow!("fframes render (cpu): {err}"))?;
    }

    // Bersihkan tmp.
    let _ = std::fs::remove_dir_all(&tmp_dir);

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
        let bytes = std::fs::read(&path)
            .with_context(|| format!("baca font {}", path.display()))?;
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

/// Ekstrak audio video sumber ke WAV stereo 48kHz.
fn extract_audio_wav(ffmpeg: &Path, video: &Path, out: &Path) -> Result<()> {
    let status = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .arg("-i")
        .arg(video)
        .args(["-vn", "-ac", "2", "-ar", "48000", "-c:a", "pcm_s16le"])
        .arg(out)
        .status()
        .with_context(|| format!("jalankan {}", ffmpeg.display()))?;
    ensure!(status.success(), "ffmpeg extract audio exit non-zero");
    Ok(())
}
