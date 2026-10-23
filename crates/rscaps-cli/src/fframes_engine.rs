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
    FFramesContext, FFramesMode, Frame, MediaProvider, RawFontData, RenderOptions, Svgr,
    TimeBase, Video, VideoSize, render,
};
use caption_core::{CaptionDocument, CaptionVideo};

use crate::doc;
use crate::render::{probe_video, tool_bin};

/// Ukuran potongan (chunk): hanya chunk ini yang di-pre-decode ke RAM.
/// Video lebih panjang dirender per potongan (fframes `frame_range`) lalu
/// digabung dengan ffmpeg concat — jadi tak ada cap total. Override lewat env
/// `RSCAPS_FFRAMES_CHUNK` (untuk uji). 300 bukan 600: pipelining decode
/// menahan 2 potongan di RAM (2 × 300 × 8.3MB ≈ 5GB).
const CHUNK_FRAMES: usize = 300;

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
/// Kalau decode dengan hwaccel gagal (hwaccel tak ada di mesin / nilai
/// salah), otomatis retry sekali tanpa hwaccel (CPU) supaya render tak mati.
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
    match decode_frames_once(ffmpeg, video, width, height, target_fps, hwaccel, start_sec, max_frames)
    {
        Ok(frames) => Ok(frames),
        Err(err) if hwaccel.is_some() => {
            eprintln!(
                "[fframes] WARN: decode dengan --hwaccel {} gagal ({err:#}) → retry CPU",
                hwaccel.unwrap_or_default()
            );
            decode_frames_once(ffmpeg, video, width, height, target_fps, None, start_sec, max_frames)
        }
        Err(err) => Err(err),
    }
}

/// Satu kali decode (tanpa retry). Lihat `decode_frames_rgba`.
fn decode_frames_once(
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
    let mut stderr = child.stderr.take().context("ffmpeg stderr")?;
    let mut frames = Vec::new();
    let mut idx = 0usize;

    loop {
        // Baca langsung ke buffer milik frame (satu salinan). JANGAN pakai
        // `PreloadedImageData::new`: ia mem-blend per-piksel via f32 skalar
        // (~11ms/frame @1080p) — no-op murni untuk frame video h264 yang
        // selalu opaque (α=255 dari konversi yuv420 → rgba).
        let mut data = vec![0u8; frame_bytes];
        let mut read_total = 0usize;
        while read_total < frame_bytes {
            match stdout.read(&mut data[read_total..]) {
                Ok(0) => break,
                Ok(n) => read_total += n,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err).context("baca rawvideo"),
            }
        }
        if read_total < frame_bytes {
            break;
        }
        debug_assert!(data.chunks_exact(4).all(|p| p[3] == 255), "frame tak opaque");
        frames.push(Arc::new(PreloadedImageData {
            id: format!("frame_{idx}"),
            data: std::borrow::Cow::Owned(data),
            width,
            height,
        }));
        idx += 1;
    }

    let status = child.wait().context("tunggu ffmpeg")?;
    if !status.success() {
        // Sertakan stderr ffmpeg supaya penyebab (mis. hwaccel tak
        // didukung) terlihat — pipe-nya belum pernah dibaca.
        let mut err_txt = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stderr, &mut err_txt);
        let err_txt = String::from_utf8_lossy(&err_txt);
        anyhow::bail!("ffmpeg decode exit non-zero: {}", err_txt.trim());
    }
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
    /// pertama yang ber-root. `None` = frame pertama kosong (gap di awal) →
    /// tak di-cache, dicari ulang tiap frame.
    svg_prefix: std::sync::OnceLock<Option<String>>,
    /// Template memakai kaca (marker `rscaps-glass` terdeteksi probe) —
    /// href kaca ditulis ulang ke frame video terdaftar per frame (setara
    /// web: burnWorker men-suplai piksel frame hidup di bawah kunci itu).
    glass: bool,
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
        // Diagnostik (env-gated): RSCAPS_NO_CAPTION=1 melompati caption —
        // isolasi biaya parse+raster caption vs video-image di profil.
        if std::env::var("RSCAPS_NO_CAPTION").as_deref() == Ok("1") {
            let w = W;
            let h = H;
            if let Some(img) = self.frames.get(idx.saturating_sub(self.frame_offset)) {
                return Svgr::from(format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><image href="{}" x="0" y="0" width="{w}" height="{h}" preserveAspectRatio="xMidYMid meet"/></svg>"#,
                    img.id
                ));
            }
            return Svgr::from(format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"/>"#
            ));
        }
        let caption = self.caption.render_frame(frame, ctx);
        let caption_str = caption.to_string();
        let w = W;
        let h = H;

        // frame.index = index global (renderer menambah frame_offset saat
        // render range) → ubah ke index lokal chunk.
        let img = idx
            .checked_sub(self.frame_offset)
            .and_then(|local| self.frames.get(local));

        let Some(img) = img else {
            // Tail caption tanpa frame sumber. Caption kosong (gap SRT →
            // Svgr::empty = string "") akan membuat backend gagal parse
            // (NoRootNode) — kembalikan root minimal yang valid.
            if caption_str.contains("<svg") {
                return caption;
            }
            return Svgr::from(format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"/>"#
            ));
        };

        // Kaca selene: href data URL (dummy dari probe) → kunci frame video
        // terdaftar (`frame_N`). Elemen tetap sama (filter blur + saturate,
        // clip-path) — hanya sumber pikselnya yang mengikuti frame ini,
        // persis kontrak web (piksel frame hidup di bawah kunci kaca).
        let caption_str = if self.glass {
            rewrite_glass_href(&caption_str, &img.id)
        } else {
            caption_str
        };

        // "meet" = letterbox (aspect ratio terjaga). Frame sumber ukuran asli;
        // kanvas W×H sesuai video nyata (portrait/landscape benar).
        let image_elem = format!(
            r#"<image href="{}" x="0" y="0" width="{w}" height="{h}" preserveAspectRatio="xMidYMid meet"/>"#,
            img.id
        );

        // Caption di luar cue (gap SRT) = string kosong. Bangun root minimal
        // berisi background video saja — jangan kirim string kosong ke parser.
        if !caption_str.contains("<svg") {
            return Svgr::from(format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{image_elem}</svg>"#
            ));
        }

        // Cache prefix (tag pembuka <svg …>) setelah frame pertama yang valid.
        // OnceLock<Option>: None = frame pertama kosong (gap di awal) → tak
        // cache apa pun, cari ulang tiap frame sampai ada caption ber-root.
        let cached = self
            .svg_prefix
            .get_or_init(|| find_svg_open_prefix(&caption_str));

        // Guard: caption frame ini harus diawali prefix yang sama. Tag <svg>
        // yang berubah panjang antar frame akan memotong di tengah tag kalau
        // di-slice membabi-buta — re-find saja (langka).
        let prefix = cached.as_deref().filter(|p| caption_str.starts_with(p));

        let out = match prefix {
            Some(p) => splice_image(p, &caption_str, &image_elem),
            None => match find_svg_open_prefix(&caption_str) {
                Some(p) if p.len() < caption_str.len() => splice_image(&p, &caption_str, &image_elem),
                // Tak ada <svg> valid — fallback caption (masih ber-root).
                _ => return caption,
            },
        };
        dump_composite_svg(idx, &out);
        Svgr::from(out)
    }
}

/// Tulis ulang href kaca selene (`data:image/jpeg;rscaps-glass;base64,…`)
/// menjadi kunci frame video terdaftar (`frame_N`). Konten data URL kaca
/// tidak pernah di-render di CLI — usvgr me-resolve lewat kunci ini.
fn rewrite_glass_href(caption: &str, frame_key: &str) -> String {
    const MARKER: &str = "data:image/jpeg;rscaps-glass;base64,";
    if !caption.contains(MARKER) {
        return caption.to_string();
    }
    let mut out = String::with_capacity(caption.len());
    let mut rest = caption;
    while let Some(pos) = rest.find(MARKER) {
        // Awal atribut href=" tepat sebelum marker.
        let href_start = rest[..pos]
            .rfind("href=\"")
            .map(|p| p + "href=\"".len())
            .unwrap_or(pos);
        out.push_str(&rest[..href_start]);
        out.push_str(frame_key);
        // Lompati isi data URL sampai penutup atribut.
        let tail = &rest[pos..];
        let end = tail.find('"').unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Diagnostik parity (env-gated `RSCAPS_DUMP_SVG_DIR`): tulis SVG komposit
/// final (frame video + caption, sesuai yang di-raster backend) per frame —
/// dipakai harness `parity_export_all.mjs` untuk membandingkan komposit
/// CLI vs web tanpa noise encoder.
fn dump_composite_svg(idx: usize, svg: &str) {
    static DUMP_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    let dir = DUMP_DIR.get_or_init(|| {
        std::env::var("RSCAPS_DUMP_SVG_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| std::fs::create_dir_all(p).is_ok())
    });
    if let Some(dir) = dir {
        let _ = std::fs::write(dir.join(format!("svg_{idx:06}.svg")), svg);
    }
}

/// Sisipkan `<image …>` tepat setelah tag pembuka `<svg …>`.
fn splice_image(prefix: &str, caption_str: &str, image_elem: &str) -> String {
    let suffix = &caption_str[prefix.len()..];
    let mut out = String::with_capacity(prefix.len() + image_elem.len() + 1 + suffix.len());
    out.push_str(prefix);
    out.push('\n');
    out.push_str(image_elem);
    out.push_str(suffix);
    out
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
    /// Nama encoder hardware (dipakai bila `hw_encode`).
    pub hw_encoder: &'a str,
    /// hwaccel untuk decode ffmpeg (sama seperti jalur overlay).
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
    // Font: urutan manifest = urutan web (fontManifest.json satu sumber) —
    // fallback glif usvgr menentukan paritas non-latin (Arab/Ibrani/dll.).
    let mut fonts: Vec<RawFontData> = Vec::new();
    for file in crate::fonts::manifest_font_files(
        Path::new(crate::fonts::MANIFEST_RELPATH),
        opts.fonts,
    ) {
        let bytes =
            std::fs::read(&file).with_context(|| format!("baca font {}", file.display()))?;
        let name = file
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "font.ttf".to_string());
        fonts.push(RawFontData {
            file_name: name,
            data: Arc::new(bytes),
        });
    }
    eprintln!("[fframes] {} file font dimuat (fontManifest.json)", fonts.len());

    // Probe caption (sekali sebelum render): (a) kumpulkan gambar `data:`
    // statis dari SVG caption (mask stroke iris / upload user) untuk
    // registry MediaProvider; (b) deteksi template kaca (`rscaps-glass`) —
    // frame video lalu dipakai sebagai kaca (rewrite href ke `frame_N`),
    // setara web yang men-suplai piksel frame hidup (D4 parity).
    let (probe_images, glass) = probe_caption_images(src_w, src_h);
    if glass {
        eprintln!("[fframes] kaca terdeteksi — frame video dipakai sebagai kaca (href rewrite)");
    }

    // Audio + tmp dir (auto-cleanup via Drop).
    let tmp_dir = std::env::temp_dir().join(format!("rscaps-fframes-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).context("buat tmp dir")?;
    let _tmp_guard = TmpDirGuard(tmp_dir.clone());

    let mut audio_map: HashMap<String, AudioData<'static>> = HashMap::new();
    let mut audio_name = String::new();
    if probe.has_audio {
        let wav_path = tmp_dir.join("audio.wav");
        extract_audio_wav(&ffmpeg, opts.video, &wav_path)?;
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

    // Encoder: hardware bila diminta & tersedia, selain itu libx264.
    let crf_str = opts.crf.to_string();
    let use_hw = opts.hw_encode && crate::render::hw_encoder_available(&ffmpeg, opts.hw_encoder);
    if opts.hw_encode && !use_hw {
        eprintln!(
            "[fframes] WARN: --hw-encode diminta tapi {} tidak tersedia → fallback libx264",
            opts.hw_encoder
        );
    }
    let crf = crf_str.as_str();
    // Nama flag kualitas beda per vendor (nvenc=cq, qsv=global_quality, dst).
    let p_nvenc = [("cq", crf), ("preset", opts.preset)];
    let p_qsv = [("global_quality", crf), ("preset", opts.preset)];
    let p_vaapi = [("qp", crf)];
    let p_vt = [("q:v", crf)];
    let p_amf = [("qp_i", crf), ("qp_p", crf)];
    let p_x264 = [("crf", crf), ("preset", opts.preset)];
    let (encoder_name, codec_params): (&str, &[(&str, &str)]) = if use_hw {
        match opts.hw_encoder {
            "h264_qsv" => ("h264_qsv", &p_qsv),
            "h264_vaapi" => ("h264_vaapi", &p_vaapi),
            "h264_videotoolbox" => ("h264_videotoolbox", &p_vt),
            "h264_amf" => ("h264_amf", &p_amf),
            _ => ("h264_nvenc", &p_nvenc),
        }
    } else {
        ("libx264", &p_x264)
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
        let mut media_images = images_of(&frames);
        for (key, img) in &probe_images {
            media_images.insert(key.clone(), Arc::clone(img));
        }
        let media = FframesMedia {
            images: media_images,
            fonts: fonts.clone(),
            audio: audio_map.clone(),
        };
        let render_opts = RenderOptions {
            media: Some(&media),
            video_encoder_options: video_encoder,
            audio_encoder_options: EncoderOptions::default(),
            // Progress baris-per-10% di stderr; tanpa banner ffplay/path.
            logger: fframes::FFramesLoggerVariant::Lines,
            load_system_fonts: std::env::var("RSCAPS_LOAD_SYSTEM_FONTS").as_deref() == Ok("1"),
            default_font: "Arial",
            ..Default::default()
        };
        eprintln!(
            "[fframes] render {duration_frames} frame ({src_w}x{src_h}) → {}",
            opts.out.display()
        );
        return dispatch_render(
            src_w, src_h, frames, 0, audio_name, duration_frames, &render_opts, opts.out, glass,
        );
    }

    // Video panjang → potongan ≤ `chunk` frame, digabung dengan ffmpeg concat.
    // Hanya 2 potongan aktif di RAM (yang dirender + yang di-decode) — decode
    // potongan berikutnya berjalan di thread lain SELAGI potongan aktif
    // dirender, jadi decoder tak masuk critical path.
    eprintln!("[fframes] video {duration_frames} frame > {chunk} → render per potongan (pipelined)");
    let mut segments: Vec<PathBuf> = Vec::new();
    type Frames = Vec<std::sync::Arc<PreloadedImageData>>;
    let spawn_decode = |s: usize, c: usize| {
        let ffmpeg = ffmpeg.clone();
        let video = opts.video.to_path_buf();
        let hwaccel = opts.hwaccel.map(str::to_string);
        let start_sec = s as f64 / target_fps as f64;
        std::thread::spawn(move || {
            decode_frames_rgba(
                &ffmpeg,
                &video,
                probe.width,
                probe.height,
                target_fps,
                hwaccel.as_deref(),
                start_sec,
                c,
            )
        })
    };
    let mut next_start = 0usize;
    let mut seg_idx = 0usize;
    let mut pending: Option<(usize, usize, usize, std::thread::JoinHandle<anyhow::Result<Frames>>)> = None;
    if next_start < duration_frames {
        let end = (next_start + chunk).min(duration_frames);
        let count = end - next_start;
        pending = Some((next_start, end, count, spawn_decode(next_start, count)));
        next_start = end;
        seg_idx += 1;
    }
    while let Some((start, end, count, join)) = pending.take() {
        eprintln!("[fframes] potongan {}: frame {start}..{end} ({count})", seg_idx);
        let frames = match join.join() {
            Ok(r) => r?,
            Err(_) => anyhow::bail!("thread decode panic"),
        };
        // Spawn decode potongan berikutnya SEBELUM render (overlap).
        if next_start < duration_frames {
            let end2 = (next_start + chunk).min(duration_frames);
            let count2 = end2 - next_start;
            pending = Some((next_start, end2, count2, spawn_decode(next_start, count2)));
            next_start = end2;
            seg_idx += 1;
        }
        let mut media_images = images_of(&frames);
        for (key, img) in &probe_images {
            media_images.insert(key.clone(), Arc::clone(img));
        }
        let media = FframesMedia {
            images: media_images,
            fonts: fonts.clone(),
            // Potongan VIDEO-ONLY: audio di-mux SEKALI di akhir — satu
            // encode AAC = satu priming di awal file, tanpa drift
            // akumulatif ~23ms di tiap batas concat.
            audio: HashMap::new(),
        };
        let seg_path = tmp_dir.join(format!("seg_{start}.mp4"));
        let render_opts = RenderOptions {
            media: Some(&media),
            video_encoder_options: video_encoder.clone(),
            audio_encoder_options: EncoderOptions::default(),
            logger: fframes::FFramesLoggerVariant::Lines,
            load_system_fonts: std::env::var("RSCAPS_LOAD_SYSTEM_FONTS").as_deref() == Ok("1"),
            default_font: "Arial",
            frame_range: Some(start..end),
            ..Default::default()
        };
        let t_render = std::time::Instant::now();
        dispatch_render(
            src_w,
            src_h,
            frames,
            start,
            // Nama audio kosong → CompositeVideo::audio() = AudioMap::none().
            String::new(),
            duration_frames,
            &render_opts,
            &seg_path,
            glass,
        )?;
        eprintln!(
            "[fframes] profil potongan: render={render_ms}ms ({count} frame)",
            render_ms = t_render.elapsed().as_millis(),
        );
        segments.push(seg_path);
    }
    // Concat potongan video-only; bila sumber punya audio (dan decode
    // berhasil), mux audio SEKALI dari wav ekstraksi — video -c copy.
    let mux_from_wav = if audio_name.is_empty() {
        None
    } else {
        Some(tmp_dir.join("audio.wav"))
    };
    let concat_out = match &mux_from_wav {
        Some(_) => tmp_dir.join("concat.mp4"),
        None => opts.out.to_path_buf(),
    };
    concat_segments(&ffmpeg, &segments, &concat_out)?;
    if let Some(wav) = &mux_from_wav {
        eprintln!("[fframes] mux audio sekali (satu encode AAC) → {}", opts.out.display());
        mux_audio_once(&ffmpeg, &concat_out, wav, opts.out)?;
    }
    eprintln!(
        "[fframes] selesai → {} ({} potongan)",
        opts.out.display(),
        segments.len()
    );
    Ok(())
}

/// Mux audio SEKALI ke video hasil concat: satu encode AAC = satu priming
/// di awal file (perilaku normal), tanpa drift akumulatif di tiap batas
/// segmen concat. Video disalin apa adanya (`-c:v copy`).
fn mux_audio_once(ffmpeg: &Path, video: &Path, wav: &Path, out: &Path) -> Result<()> {
    let status = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(wav)
        .args([
            "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
            "-shortest",
        ])
        .arg(out)
        .status()
        .with_context(|| format!("jalankan {}", ffmpeg.display()))?;
    ensure!(status.success(), "ffmpeg mux audio exit non-zero");
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
    glass: bool,
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
                    glass,
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
        glass,
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
    glass: bool,
) -> Result<()> {
    let video = CompositeVideo::<W, H> {
        caption: CaptionVideo,
        frames,
        frame_offset,
        audio_name,
        duration_frames,
        svg_prefix: std::sync::OnceLock::new(),
        glass,
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
    glass: bool,
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
                    glass,
                );
            }
        };
    }
    try_size!(1920, 1080);
    try_size!(1280, 720);
    try_size!(3840, 2160);
    // 854×480 / 480×854: ratio unik (1.7792 / 0.5621) — sedikit beda dari
    // 16:9 / 9:16, jadi bisa menang di closest_aspect_size untuk source
    // dengan aspect di antaranya. Tanpa ini jatuh ke 1920×1080 (portrait
    // → landscape, orientasi salah).
    try_size!(854, 480);
    try_size!(640, 360);
    try_size!(1080, 1920);
    try_size!(720, 1280);
    try_size!(2160, 3840);
    try_size!(480, 854);
    try_size!(360, 640);
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
        glass,
    )
}

/// Cari pasangan umum yang paling se-aspek dengan `aspect`.
///
/// Tie-break `<` ketat = anggota PERTAMA per keluarga ratio menang, KECUALI
/// keluarga dengan ratio unik sedikit berbeda — `854×480` (1.7792) dan
/// `480×854` (0.5621) bisa menang untuk source dengan aspect di antara.
/// Kedua ukuran itu (plus seluruh `COMMON`) wajib ada di
/// `render_sized_generic`, kalau tidak fallback 1920×1080 (orientasi
/// salah untuk portrait).
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

/// Probe caption di luar pipeline render (dokumen global sudah disetel
/// `doc::build_document`): render beberapa frame sampai ada SVG ber-root,
/// lalu (a) daftarkan semua gambar `data:` statis yang ditemukan (mask
/// stroke iris bawaan/upload user — href konstan sepanjang dokumen, jadi
/// probe 1 frame berisi cue cukup); (b) deteksi marker kaca `rscaps-glass`
/// — bila ada, frame video 1×1 dummy dibiarkan terpasang supaya template
/// memancarkan elemen kaca dan href-nya ditulis ulang per frame.
///
/// Return (gambar statis, template memakai kaca).
fn probe_caption_images(w: usize, h: usize) -> (HashMap<String, Arc<PreloadedImageData>>, bool) {
    // JPEG 1×1: cukup memicu pancaran kaca; kontennya tak pernah di-render
    // (href ditulis ulang ke frame video). Bukan kaca → dibuang lagi.
    caption_core::set_frame_image(&glass_probe_jpeg());

    let ctx = FFramesContext {
        time_base: TimeBase { fps: 30, sample_rate: 48000 },
        current_video_size: VideoSize { width: w, height: h },
        duration_in_frames: caption_core::document()
            .read()
            .map(|d| d.duration_frames as usize)
            .unwrap_or(0),
        mode: FFramesMode::Renderer,
        scenes: None,
        media_source: None,
        font_source: None,
        abort_signal: None,
    };

    let video = CaptionVideo;
    let mut images: HashMap<String, Arc<PreloadedImageData>> = HashMap::new();
    let mut has_glass = false;
    // Sampel 3 titik (awal/tengah/akhir) — menjangkau cue pertama walau
    // dokumen diawali gap, dan cue terakhir walau cue pertama sudah lewat.
    let last = ctx.duration_in_frames.saturating_sub(1);
    for f in [0usize, ctx.duration_in_frames / 2, last] {
        let frame = Frame::new(f, f, 30);
        let svg = video.render_frame(frame, &ctx).value;
        if !svg.contains("<svg") {
            continue;
        }
        crate::images::ensure_svg_images(&svg);
        has_glass |= svg.contains("rscaps-glass");
    }
    images.extend(crate::images::snapshot());
    if !has_glass {
        // Higienitas: template non-kaca tak boleh mewarisi frame dummy.
        caption_core::clear_frame_image();
    }
    eprintln!(
        "[fframes] probe: {} gambar statis terdaftar, kaca={}",
        images.len(),
        if has_glass { "ya" } else { "tidak" }
    );
    (images, has_glass)
}

/// JPEG 1×1 pemantik kaca (lihat `probe_caption_images`).
fn glass_probe_jpeg() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([16, 16, 16, 255]));
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75)
        .encode_image(&img)
        .expect("jpeg probe 1x1");
    out
}

/// Gabungkan segmen mp4 hasil chunked render → satu file (`-c copy`; semua
/// segmen punya codec/parameter sama karena encoder yang sama).
pub(crate) fn concat_segments(ffmpeg: &Path, segments: &[PathBuf], out: &Path) -> Result<()> {
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
        // Entri list WAJIB absolut: concat demuxer me-resolve path relatif
        // terhadap LOKASI FILE LIST (bukan CWD) — entri relatif membuat
        // `--out out/x.mp4` mencari `out/out/...`.
        let abs = std::path::absolute(seg)
            .with_context(|| format!("absolute path untuk {}", seg.display()))?;
        // Path concat demuxer pakai '/' (hindari escape backslash Windows).
        body.push_str(&format!("file '{}'\n", abs.to_string_lossy().replace('\\', "/")));
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

fn extract_audio_wav(ffmpeg: &Path, video: &Path, out: &Path) -> Result<()> {
    // Tanpa -hwaccel: dengan -vn video decoder tak dibuka, jadi hwaccel
    // di sini percuma — dan nilai yang tak didukung malah mematikan extract.
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
