//! Jalur native desktop (fitur "native"). LIHAT AGENTS.md — arsitektur
//! cross-platform.
//!
//! MVP Fase 3: window Slint + editor UI + video preview berbasis decode
//! thread (FFmpegDecoder -> RGBA -> slint::Image) + caption overlay via
//! fframes::Previewer + CpuFrameRenderer (svgr/tiny-skia — engine SAMA
//! dengan export dan dengan raster overlay WASM).
//!
//! Alur preview:
//!   main thread (Slint event loop)
//!     ├─ Timer 16ms  ─ drain channel event dari decode thread
//!     │                ├─ Frame  -> preview-image + clock + caption render
//!     │                └─ Opened -> metadata, route editor, sinkron timeline
//!     └─ callbacks   ─ open(rfd)/play/pause/seek/back -> kirim Cmd
//!   decode thread (1 thread per proses, owns FFmpegDecoder)
//!     └─ recv_timeout(33ms saat play) -> decode_up_to(offset) ->
//!        convert resize -> Ev::Frame
//!
//! Belum ada MVP ini: transcribe, persist proyek, export, audio (cpal),
//! undo/redo, volume — callback terkait dibiarkan no-op.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use caption_core::CaptionVideo;
use fframes::{
    AudioData, CpuFrameRenderer, FontSource, MediaProvider, Previewer, RenderOptions,
};
use fframes_media::{FFmpegDecoder, FrameConvertOptions, ResizeVideoFrame};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};

slint::include_modules!();

use crate::ui_common::{active_cue_at, fmt_dur_ms, section_rows, sync_params_global, template_id};

/// Frame rate clock preview + satuan offset decode (== CaptionVideo::FPS;
/// FFmpegDecoder custom_time_base = 1/TARGET_FPS sehingga offset = index frame).
const TARGET_FPS: usize = 30;
/// Lebar maks frame video preview (downscale via SwsScaler di decoder).
/// 480 = preseden preview-proxy tscaps (480p H.264); 960 terlalu berat
/// untuk decode+copy per frame di main thread.
const PREVIEW_MAX_W: u32 = 480;

// ═════════════════════════════════════════════════════════════════════════
// Decode thread
// ═════════════════════════════════════════════════════════════════════════

enum Cmd {
    Open(PathBuf),
    Play,
    Pause,
    /// Detik (bukan frame) — UI bebas pakai satuan apa pun.
    Seek(f64),
}

enum Ev {
    Opened {
        w: u32,
        h: u32,
        duration_s: f64,
        fps: f32,
        path_stem: String,
    },
    Ended,
    Error(String),
}

/// Frame video preview terbaru (latest-slot anti-antrean, paritas tscaps
/// FrameBuffer: frame lama DITIMPA, bukan ditumpuk — UI selalu ambil
/// yang terbaru, stut decode tak pernah menumpuk jadi lag bertumpuk).
struct LatestFrame {
    inner: std::sync::Mutex<Option<(f64, u32, u32, Vec<u8>)>>,
}

impl LatestFrame {
    const fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(None),
        }
    }
    fn set(&self, t: f64, w: u32, h: u32, rgba: Vec<u8>) {
        *self.inner.lock().unwrap() = Some((t, w, h, rgba));
    }
    fn take(&self) -> Option<(f64, u32, u32, Vec<u8>)> {
        self.inner.lock().unwrap().take()
    }
}

/// FFmpegDecoder memegang pointer mentah libav (jadi !Send otomatis).
/// fframes sendiri memindahkan decoder antar thread lewat wrapper serupa
/// (VideoDecodersWorker: "thread-safe due to RwLock" + unsafe impl Send).
/// Kontrak di sini: SATU decoder dimiliki SATU thread ini sepanjang hidup
/// (tak pernah dipinjam lintas thread) — desain yang sama aman.
struct SendDecoder(FFmpegDecoder);

unsafe impl Send for SendDecoder {}

/// Decode satu frame pada detik `t`, di-downscale ke <= PREVIEW_MAX_W lebar.
/// Returns (width, height, rgba_straight).
fn decode_at(dec: &mut SendDecoder, t: f64) -> Option<(u32, u32, Vec<u8>)> {
    let offset = (t * TARGET_FPS as f64).round() as i64;
    // decode_up_to: forward linear = murah; lompat jauh = seek keyframe.
    let has = unsafe { dec.0.decode_up_to(offset) }.ok()?;
    if !has {
        return None; // lewat akhir stream — frame terakhir sudah dikirim sebelumnya
    }
    let fb = dec.0.get_raw_frame();
    let sw = fb.get_stream_width();
    let sh = fb.get_stream_height();
    if sw == 0 || sh == 0 {
        return None;
    }
    let scale = (PREVIEW_MAX_W as f64 / sw as f64).min(1.0);
    let rw = ((sw as f64 * scale).round() as u32).max(1);
    let rh = ((sh as f64 * scale).round() as u32).max(1);
    let opts = FrameConvertOptions {
        resize: ResizeVideoFrame {
            width: rw,
            height: rh,
        },
    };
    // convert_*/get_stream_* ada di FFmpegFrameBuf (bukan FFmpegDecoder).
    // Hasilnya Arc<PreloadedImageData> — clone Cow -> Vec untuk kirim lewat
    // channel (Owned: copy; Borrowed: copy slice).
    let img = unsafe { fb.convert_last_decoded_frame_into_svg_image(Some(&opts)) }.ok()?;
    Some((img.width, img.height, img.data.clone().into_owned()))
}

fn video_worker(rx: mpsc::Receiver<Cmd>, tx: mpsc::Sender<Ev>, frame_slot: &LatestFrame) {
    let mut dec: Option<SendDecoder> = None;
    let mut duration_s = 0.0f64;
    let mut playing = false;
    // Clock playback: t = t0 + (now - at). Pause men-frozen t; Play reset at.
    let mut t0 = 0.0f64;
    let mut at = Instant::now();
    // Offset frame terakhir yang sukses dikirim (gate anti-resend per tick).
    let mut last_sent: Option<i64> = None;

    // Decode frame pada detik t lalu tulis ke latest-slot (skip jika offset
    // sama dengan terakhir).
    let mut push = |dec: &mut Option<SendDecoder>,
                    last_sent: &mut Option<i64>,
                    t: f64,
                    slot: &LatestFrame| {
        let Some(d) = dec.as_mut() else { return };
        let offset = (t * TARGET_FPS as f64).round() as i64;
        if *last_sent == Some(offset) {
            return;
        }
        if let Some((w, h, rgba)) = decode_at(d, t) {
            *last_sent = Some(offset);
            slot.set(t, w, h, rgba);
        }
    };

    loop {
        // 33ms tick saat play (~30fps); idle lebih hemat saat pause.
        let tick = if playing {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(100)
        };
        let first = match rx.recv_timeout(tick) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };

        // Drain SEMUA perintah yang menumpuk (paritas tscaps: seek drag
        // 60Hz tidak boleh mendeocode tiap tick — hanya posisi TERAKHIR
        // yang berarti). Play/Pause diterapkan berurutan; Seek terakhir
        // menang.
        let mut cmds: Vec<Cmd> = first.into_iter().collect();
        while let Ok(c) = rx.try_recv() {
            cmds.push(c);
        }
        let mut seek_target: Option<f64> = None;
        for c in cmds {
            match c {
                Cmd::Open(path) => {
                    match unsafe { FFmpegDecoder::new(&path, TARGET_FPS, 4) } {
                        Ok(d) => {
                            let fb = d.get_raw_frame();
                            // NAMA MENIPU: fungsi ini mengembalikan detik (durasi
                            // stream × time_base), BUKAN jumlah frame.
                            let dur = unsafe { fb.get_stream_duration_in_frames() } as f64;
                            duration_s = if dur.is_finite() && dur > 0.0 { dur } else { 0.0 };
                            let (w, h) = (fb.get_stream_width(), fb.get_stream_height());
                            let fps = fb.get_stream_fps();
                            let stem = path
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            dec = Some(SendDecoder(d));
                            playing = false;
                            t0 = 0.0;
                            at = Instant::now();
                            last_sent = None;
                            let _ = tx.send(Ev::Opened {
                                w,
                                h,
                                duration_s,
                                fps,
                                path_stem: stem,
                            });
                            push(&mut dec, &mut last_sent, 0.0, frame_slot);
                        }
                        Err(e) => {
                            let _ = tx.send(Ev::Error(format!("Buka video gagal: {e:?}")));
                        }
                    }
                }
                Cmd::Play => {
                    if dec.is_some() && duration_s > 0.0 && !playing {
                        at = Instant::now();
                        playing = true;
                    }
                }
                Cmd::Pause => {
                    if playing {
                        t0 += at.elapsed().as_secs_f64();
                        playing = false;
                    }
                }
                Cmd::Seek(t) => {
                    seek_target = Some(t); // coalescing: hanya terakhir
                }
            }
        }
        if let Some(t) = seek_target {
            t0 = t.clamp(0.0, duration_s);
            at = Instant::now();
            last_sent = None; // paksa kirim frame di posisi baru
            push(&mut dec, &mut last_sent, t0, frame_slot);
        }

        if playing {
            let t = t0 + at.elapsed().as_secs_f64();
            if duration_s > 0.0 && t >= duration_s {
                t0 = duration_s;
                playing = false;
                push(&mut dec, &mut last_sent, duration_s, frame_slot);
                let _ = tx.send(Ev::Ended);
            } else {
                push(&mut dec, &mut last_sent, t, frame_slot);
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Caption overlay (fframes::Previewer + CpuFrameRenderer — engine export)
// ═════════════════════════════════════════════════════════════════════════

/// Font template caption-core (caption-core/assets/*.ttf) sebagai
/// MediaProvider — populate_font_source dipanggil Previewer saat konstruksi,
/// jadi family template (Montserrat dsb) resolve di native (fontdb baca
/// name-table dari biner TTF — sama seperti WASM overlay).
struct TemplateFonts {
    fonts: Vec<(String, Vec<u8>)>,
}

impl fmt::Debug for TemplateFonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TemplateFonts({} files)", self.fonts.len())
    }
}

impl TemplateFonts {
    fn load() -> Self {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../caption-core/assets");
        let mut fonts = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("ttf") {
                    if let Ok(bytes) = std::fs::read(&p) {
                        let name =
                            p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        fonts.push((name, bytes));
                    }
                }
            }
        }
        Self { fonts }
    }
}

impl<'a> MediaProvider<'a> for TemplateFonts {
    fn resolve_audio(&'a self, _name: &str) -> Option<&'a AudioData<'a>> {
        None
    }
    fn resolve_image(&'a self, _name: &str) -> Option<&'a fframes::media::ImageData<'a>> {
        None
    }
    fn resolve_subtitles(&'a self, _name: &str) -> Option<&'a fframes::media::Subtitles<'a>> {
        None
    }
    fn resolve_video(&'a self, _name: &str) -> Option<&'a fframes::media::VideoMedia> {
        None
    }
    fn populate_font_source(&'a self, font_source: &mut dyn FontSource) {
        for (name, data) in &self.fonts {
            // Vec<u8>: AsRef<[u8]> + Send + Sync -> unsize coercion ke trait
            // object FontSource. Copy sekali saat startup (Previewer baru).
            let blob: Arc<dyn AsRef<[u8]> + Sync + Send> = Arc::new(data.clone());
            font_source.add_font(name.clone(), blob);
        }
    }
    fn populate_image_source(
        &'a self,
        _image_data: &mut HashMap<String, Arc<fframes::usvgr::PreloadedImageData>>,
    ) {
    }
}

/// Previewer caption + renderer CPU. Timeline caption di-resolve saat
/// `Previewer::new` (dari CaptionDocument.duration_frames) — setelah durasi
/// video disinkronkan ke dokumen, `recreate` memaksa build ulang.
struct CaptionState {
    fonts: &'static TemplateFonts,
    previewer: Previewer<'static, 'static, CaptionVideo>,
    renderer: CpuFrameRenderer,
    recreate: bool,
}

fn make_caption(fonts: &'static TemplateFonts) -> CaptionState {
    static VIDEO: CaptionVideo = CaptionVideo;
    // Di-leak sekali per pemanggilan: RenderOptions memegang &media dengan
    // lifetime 'static. Ukuran kecil; proses app memang hidup selamanya.
    let options: &'static RenderOptions<'static, 'static> = Box::leak(Box::new(RenderOptions {
        media: Some(fonts),
        load_system_fonts: true, // Arial utk default params
        ..RenderOptions::default()
    }));
    let mut previewer = Previewer::new(&VIDEO, options).expect("Previewer::new caption");
    previewer.set_scale(0.5); // raster caption 960×540 — cukup utk preview
    CaptionState {
        fonts,
        previewer,
        renderer: CpuFrameRenderer::default(),
        recreate: false,
    }
}

/// Render caption frame -> overlay-image. Gagal (frame di luar timeline /
/// error raster) dibiarkan — overlay lama dipertahankan.
fn render_caption(st: &mut CaptionState, ui: &App, frame: usize) {
    if st.recreate {
        *st = make_caption(st.fonts);
    }
    let Ok(rgba) = st.previewer.render(frame, &mut st.renderer) else {
        // TEMP debug
        eprintln!("[caption] render FAILED at frame {frame}");
        return;
    };
    let (w, h) = (rgba.width, rgba.height);
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let dst = buf.make_mut_bytes();
    if dst.len() == rgba.pixels.len() {
        dst.copy_from_slice(&rgba.pixels);
        ui.set_overlay_image(Image::from_rgba8(buf));
        ui.set_overlay_live(true);
    } else {
        // TEMP debug
        eprintln!(
            "[caption] pixel len mismatch: got {} expect {} ({w}x{h})",
            rgba.pixels.len(),
            dst.len()
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Gallery template (glue shared -> tipe Slint)
// ═════════════════════════════════════════════════════════════════════════

fn section_items(section: &str, query: &str) -> Vec<TemplateItem> {
    section_rows(section, query)
        .into_iter()
        .map(|(name, idx, row, col)| TemplateItem {
            name: name.into(),
            idx,
            row,
            col,
        })
        .collect()
}

fn set_models(ui: &App, query: &str) {
    ui.set_template_viral(Rc::new(slint::VecModel::from(section_items("viral", query))).into());
    ui.set_template_modern(Rc::new(slint::VecModel::from(section_items("modern", query))).into());
    ui.set_template_classic(Rc::new(slint::VecModel::from(section_items("classic", query))).into());
    ui.set_template_key(Rc::new(slint::VecModel::from(section_items("key-moments", query))).into());
    ui.set_template_lab(Rc::new(slint::VecModel::from(section_items("lab", query))).into());
}

fn pick_video_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(
            "Video",
            &["mp4", "mov", "webm", "mkv", "avi", "m4v", "mts", "wmv"],
        )
        .pick_file()
}

// ═════════════════════════════════════════════════════════════════════════
// Entry
// ═════════════════════════════════════════════════════════════════════════

/// Entry point aplikasi native. Dipanggil dari `src/bin/studio_native.rs`.
pub fn run() -> Result<(), slint::PlatformError> {
    let ui = App::new()?;

    ui.set_route("home".into());
    ui.set_app_is_dark(true);
    // Native tidak punya layer di belakang kanvas (WASM punya DOM <video> +
    // body HTML) — isi window-background dengan bg0 opak (paritas tema
    // gelap; MVP native dark-only).
    ui.set_window_background(slint::Brush::SolidColor(slint::Color::from_rgb_u8(
        0x0c, 0x0c, 0x0e,
    )));
    set_models(&ui, "");
    sync_params_global(&ui);
    crate::ui_common::sync_style_controls(&ui);
    ui.set_status("Native MVP — buka video untuk mulai.".into());

    // ── shared state ───────────────────────────────────────────────────
    let fonts: &'static TemplateFonts = Box::leak(Box::new(TemplateFonts::load()));
    let caption = Rc::new(RefCell::new(make_caption(fonts)));
    let duration_s = Rc::new(Cell::new(0.0f64));
    let playing = Rc::new(Cell::new(false));
    // Frame terakhir yang caption-nya sudah di-render (gate render caption).
    let cap_frame = Rc::new(Cell::new(-1i32));
    // true = Opened dari New Project (tampilkan dialog); false = Import
    // (langsung editor). Di-set sebelum kirim Cmd::Open.
    let open_via_new_project = Rc::new(Cell::new(false));

    // ── decode thread ──────────────────────────────────────────────────
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (ev_tx, ev_rx) = mpsc::channel::<Ev>();
    // Arc (bukan Rc): slot dibagi antara decode thread dan UI thread.
    let frame_slot = Arc::new(LatestFrame::new());
    {
        let slot = frame_slot.clone();
        thread::spawn(move || video_worker(cmd_rx, ev_tx, &slot));
    }

    // ══ DEBUG (temporary): portrait video + loki + cues — verifikasi koordinat ══
    {
        let tx = cmd_tx.clone();
        let cap = caption.clone();
        let cap_frame_c = cap_frame.clone();
        let test_video = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../whisper-spike/test/portrait-test.mp4");
        if test_video.exists() {
            if let Ok(mut doc) = caption_core::document().write() {
                doc.cues = vec![
                    caption_core::Cue { start_ms: 0, end_ms: 3000, text: "Hello world".into() },
                ];
                doc.duration_frames = 180;
                doc.template = "loki".into();
            }
            cap.borrow_mut().recreate = true;
            cap_frame_c.set(-1);
            let _ = tx.send(Cmd::Open(test_video));
            open_via_new_project.set(false);
        }
    }
    // ══ END DEBUG ══

    // ── callbacks: buka video ──────────────────────────────────────────
    // New Project (home): pilih file -> probe -> NewProjectDialog muncul
    // (paritas WASM; transkrip native belum ada — Start = langsung editor
    // dengan status jujur). Import/Attach (editor): langsung ganti video.
    // np-cancel: kembali ke home tanpa masuk editor.
    {
        let tx = cmd_tx.clone();
        let weak = ui.as_weak();
        let via_np = open_via_new_project.clone();
        ui.on_new_project(move || {
            if let Some(p) = pick_video_file() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status("Membuka video…".into());
                    ui.set_status_is_error(false);
                }
                via_np.set(true);
                let _ = tx.send(Cmd::Open(p));
            }
        });
    }
    {
        let tx = cmd_tx.clone();
        let weak = ui.as_weak();
        let via_np = open_via_new_project.clone();
        ui.on_import_video(move || {
            if let Some(p) = pick_video_file() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status("Membuka video…".into());
                    ui.set_status_is_error(false);
                }
                via_np.set(false);
                let _ = tx.send(Cmd::Open(p));
            }
        });
    }
    {
        let tx = cmd_tx.clone();
        let weak = ui.as_weak();
        let via_np = open_via_new_project.clone();
        ui.on_attach_video(move || {
            if let Some(p) = pick_video_file() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status("Membuka video…".into());
                    ui.set_status_is_error(false);
                }
                via_np.set(false);
                let _ = tx.send(Cmd::Open(p));
            }
        });
    }
    {
        let weak = ui.as_weak();
        let playing_c = playing.clone();
        ui.on_np_cancel(move || {
            if let Some(ui) = weak.upgrade() {
                playing_c.set(false);
                ui.set_playing(false);
                ui.set_has_video(false);
                ui.set_show_new_project(false);
                ui.set_route("home".into());
                ui.set_status("Proyek dibatalkan.".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let playing_c = playing.clone();
        let cap = caption.clone();
        ui.on_np_start(move || {
            let Some(ui) = weak.upgrade() else { return };
            // Terapkan template terpilih (paritas WASM np_start).
            let tpl = ui.get_np_template_selected();
            if let Ok(mut doc) = caption_core::document().write() {
                doc.template = template_id(tpl);
            }
            cap.borrow_mut().recreate = true;
            playing_c.set(false);
            ui.set_playing(false);
            ui.set_has_video(true);
            ui.set_show_new_project(false);
            ui.set_route("editor".into());
            ui.set_status(
                "Video siap. Transkrip belum tersedia di native — tambah caption manual."
                    .into(),
            );
            ui.set_status_is_error(false);
        });
    }
    // np-use-subtitle: impor .srt/.vtt via rfd (paritas jalur file-input
    // WASM) -> parse caption-core -> masuk editor dgn cues siap.
    {
        let weak = ui.as_weak();
        let playing_c = playing.clone();
        let cap = caption.clone();
        let cap_frame_c = cap_frame.clone();
        ui.on_np_use_subtitle(move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Subtitle", &["srt", "vtt"])
                .pick_file()
            else {
                return; // dibatalkan — dialog tetap terbuka
            };
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    ui.set_status(format!("subtitle read failed: {e}").into());
                    ui.set_status_is_error(true);
                    return;
                }
            };
            let fmt = if name.to_ascii_lowercase().ends_with(".vtt") {
                caption_core::srt::Format::Vtt
            } else {
                caption_core::srt::Format::Srt
            };
            let parsed = match caption_core::srt::parse(&text, fmt) {
                Ok(p) => p,
                Err(e) => {
                    ui.set_status(format!("subtitle parse failed: {e}").into());
                    ui.set_status_is_error(true);
                    return;
                }
            };
            let n = parsed.cues.len();
            if let Ok(mut doc) = caption_core::document().write() {
                doc.cues = parsed.cues;
                // Jangan pendekkan timeline video — perpanjang hanya jika
                // cue melewati durasi video yang sudah terbuka.
                if parsed.duration_frames > doc.duration_frames {
                    doc.duration_frames = parsed.duration_frames;
                }
            }
            cap.borrow_mut().recreate = true;
            cap_frame_c.set(-1);
            playing_c.set(false);
            ui.set_playing(false);
            ui.set_has_video(true);
            ui.set_show_new_project(false);
            ui.set_route("editor".into());
            let tpl = ui.get_np_template_selected();
            if let Ok(mut doc) = caption_core::document().write() {
                doc.template = template_id(tpl);
            }
            ui.set_status(format!("Impor {n} cue dari {name}.").into());
            crate::ui_common::sync_cue_list(&ui);
            ui.set_status_is_error(false);
        });
    }

    // ── play / pause ───────────────────────────────────────────────────
    {
        let tx = cmd_tx.clone();
        let weak = ui.as_weak();
        let playing_c = playing.clone();
        ui.on_toggle_play(move || {
            let Some(ui) = weak.upgrade() else { return };
            let p = !ui.get_playing();
            ui.set_playing(p);
            playing_c.set(p);
            let _ = tx.send(if p { Cmd::Play } else { Cmd::Pause });
        });
    }

    // ── scrub (f = fraction 0..1, paritas jalur WASM) ──────────────────
    {
        let tx = cmd_tx.clone();
        let weak = ui.as_weak();
        let dur = duration_s.clone();
        let cap = caption.clone();
        let cap_frame_c = cap_frame.clone();
        ui.on_scrub_to(move |f: f32| {
            let d = dur.get();
            if !(d.is_finite() && d > 0.0) {
                return;
            }
            let t = (f as f64 * d).clamp(0.0, d);
            let _ = tx.send(Cmd::Seek(t));
            let Some(ui) = weak.upgrade() else { return };
            let frame = (t * TARGET_FPS as f64) as i32;
            ui.set_time_text(fmt_dur_ms(t).into());
            ui.set_frame(frame);
            ui.set_overlay_text(active_cue_at((t * 1000.0) as u32).into());
            // Caption di-render langsung (feedback instan); frame video
            // menyusul dari decode thread lewat pump.
            cap_frame_c.set(frame);
            render_caption(&mut cap.borrow_mut(), &ui, frame.max(0) as usize);
        });
    }

    // ── back -> home ───────────────────────────────────────────────────
    {
        let tx = cmd_tx.clone();
        let weak = ui.as_weak();
        let playing_c = playing.clone();
        ui.on_back(move || {
            let _ = tx.send(Cmd::Pause);
            if let Some(ui) = weak.upgrade() {
                playing_c.set(false);
                ui.set_playing(false);
                ui.set_has_video(false);
                ui.set_video_aspect(16.0 / 9.0);
                ui.set_route("home".into());
            }
        });
    }

    // ── template: pilih + search ───────────────────────────────────────
    {
        let weak = ui.as_weak();
        let cap = caption.clone();
        let cap_frame_c = cap_frame.clone();
        ui.on_template_chosen(move |i| {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_template_selected(i);
            if let Ok(mut doc) = caption_core::document().write() {
                doc.template = template_id(i);
            }
            crate::ui_common::sync_style_controls(&ui);
            // Durasi tak berubah (tak perlu recreate) — render ulang frame
            // sekarang agar caption langsung ganti.
            cap_frame_c.set(-1);
            let f = ui.get_frame().max(0);
            cap_frame_c.set(f);
            render_caption(&mut cap.borrow_mut(), &ui, f as usize);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_template_search(move |q| {
            if let Some(ui) = weak.upgrade() {
                set_models(&ui, q.as_str());
            }
        });
    }

    // ── DocParams global: panel callbacks → caption-core ────────────────
    // Shared wiring (ui_common): patch → refresh. Native tanpa undo history.
    {
        let cap = caption.clone();
        let cap_frame_c = cap_frame.clone();
        let refresh = move |ui: &App| {
            cap.borrow_mut().recreate = true;
            cap_frame_c.set(-1);
            let f = ui.get_frame().max(0);
            render_caption(&mut cap.borrow_mut(), ui, f.max(0) as usize);
        };
        let cap2 = caption.clone();
        let cap_frame_c2 = cap_frame.clone();
        let refresh2 = move |ui: &App| {
            cap2.borrow_mut().recreate = true;
            cap_frame_c2.set(-1);
            let f = ui.get_frame().max(0);
            render_caption(&mut cap2.borrow_mut(), ui, f.max(0) as usize);
        };
        crate::ui_common::wire_doc_params(&ui, |_| {}, refresh);
        crate::ui_common::wire_doc_cues(&ui, |_| {}, refresh2);
        crate::ui_common::sync_cue_list(&ui);
    }

    // Timer frame-pump/ui.slint memanggil dua callback ini — native tidak
    // memakainya (pump sendiri di bawah); pasang no-op eksplisit.
    ui.on_pump_frame(|| {});
    ui.on_poll_width(|| {});

    // ── pump: drain event + frame-slot -> UI (jalan terus, 16ms) ───────
    {
        let weak = ui.as_weak();
        let dur = duration_s.clone();
        let playing_c = playing.clone();
        let cap = caption.clone();
        let cap_frame_c = cap_frame.clone();
        let slot = frame_slot.clone();
        let cap_last_render = Rc::new(Cell::new(Instant::now()));
        let via_np = open_via_new_project.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(ui) = weak.upgrade() else { return };
            while let Ok(ev) = ev_rx.try_recv() {
                match ev {
                    Ev::Opened {
                        w,
                        h,
                        duration_s: d,
                        fps: _,
                        path_stem,
                    } => {
                        dur.set(d);
                        playing_c.set(false);
                        ui.set_playing(false);
                        ui.set_has_video(false); // dialog dulu; Start yang set
                        ui.set_video_aspect(w as f32 / h as f32);
                        ui.set_duration_text(fmt_dur_ms(d).into());
                        ui.set_scrub_max((d * TARGET_FPS as f64) as i32);
                        ui.set_time_text(fmt_dur_ms(0.0).into());
                        ui.set_frame(0);
                        ui.set_stage_chip("frame 0 · 0.00s".into());
                        // Sinkron timeline caption dengan durasi video,
                        // lalu paksa Previewer build ulang (timeline di-
                        // resolve saat konstruksi).
                        if let Ok(mut doc) = caption_core::document().write() {
                            doc.duration_frames = (d * TARGET_FPS as f64).round() as u32;
                        }
                        cap.borrow_mut().recreate = true;
                        cap_frame_c.set(-1);
                        // Konteks: New Project -> dialog; Import/Attach -> editor.
                        if via_np.get() {
                            ui.set_np_name(path_stem.as_str().into());
                            ui.set_np_video_name(path_stem.as_str().into());
                            ui.set_np_video_meta(
                                format!("{w}×{h} · {}", fmt_dur_ms(d)).into(),
                            );
                            ui.set_show_new_project(true);
                            ui.set_status("Video terbuka — lengkapi proyek.".into());
                        } else {
                            ui.set_has_video(true);
                            ui.set_project_name(path_stem.as_str().into());
                            ui.set_route("editor".into());
                            ui.set_status(
                                format!("Video terbuka · {w}×{h} · {d:.1}s").into(),
                            );
                        }
                        // Segarkan panel yang membaca dokumen (cue list,
                        // style controls template aktif) setelah buka.
                        crate::ui_common::sync_style_controls(&ui);
                        crate::ui_common::sync_cue_list(&ui);
                        ui.set_status_is_error(false);
                    }
                    Ev::Ended => {
                        playing_c.set(false);
                        ui.set_playing(false);
                    }
                    Ev::Error(e) => {
                        ui.set_status(e.as_str().into());
                        ui.set_status_is_error(true);
                    }
                }
            }
            // Latest-slot: ambil frame TERAKHIR saja (frame lama sudah
            // ditimpa decode thread — tanpa antrean, tanpa lag menumpuk).
            if let Some((t, w, h, rgba)) = slot.take() {
                if rgba.len() == w as usize * h as usize * 4 {
                    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
                    buf.make_mut_bytes().copy_from_slice(&rgba);
                    ui.set_preview_image(Image::from_rgba8(buf));
                }
                let f = (t * TARGET_FPS as f64).round() as i32;
                ui.set_frame(f);
                ui.set_time_text(fmt_dur_ms(t).into());
                ui.set_stage_chip(format!("frame {f} · {t:.2}s").into());
                ui.set_overlay_text(active_cue_at((t * 1000.0) as u32).into());
                // Caption throttle saat playing (≥50ms / ~15fps): raster
                // caption di main thread cukup berat; pause/seek tetap
                // instan karena jalur scrub di bawah langsung render.
                if cap_frame_c.get() != f {
                    let now = Instant::now();
                    let last = cap_last_render.get();
                    let is_playing = playing_c.get();
                    if !is_playing || now.duration_since(last).as_millis() >= 50 {
                        cap_last_render.set(now);
                        cap_frame_c.set(f);
                        render_caption(&mut cap.borrow_mut(), &ui, f.max(0) as usize);
                    }
                }
            }
        });
        // timer harus hidup selama run() — jangan di-drop.
        std::mem::forget(timer);
    }

    ui.run()
}
