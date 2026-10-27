use fframes::{
    AudioMap, BreakLinesOpts, Color, Duration, FFramesContext, FontQuery, Frame, Svgr, TextAlign,
    Transform, Video,
};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock, RwLock};

pub mod srt;
pub mod templates;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
    /// Sheet id pemilik cue (None/empty = sheet default dokumen).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
}

/// Satu sheet (track gaya): template + params opsional yang menimpa
/// dokumen. Cue ditugaskan ke sheet lewat `Cue::sheet`; cue tanpa sheet
/// milik sheet default (doc.template/doc.params).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sheet {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Template native untuk sheet ini; kosong = renderer v0.
    #[serde(default)]
    pub template: String,
    /// Params khusus sheet; None = pakai doc.params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<CaptionParams>,
    /// Role semantik sheet (tscaps SheetRole): "peak"/"hook" — kata-kata
    /// cue yang terikat sheet ini ikut bertag demikian (kel peak-bg, dll).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionParams {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_y_frac")]
    pub y_frac: f32,
    #[serde(default = "default_width_frac")]
    pub width_frac: f32,
    #[serde(default = "default_pop_in_ms")]
    pub pop_in_ms: u32,
    #[serde(default = "default_travel_px")]
    pub travel_px: f32,
    /// Base text direction: `"auto"` (strong-char majority over the whole
    /// document), `"ltr"`, or `"rtl"`. `"rtl"` is the right knob for Arabic/
    /// Hebrew content — mixed lines then keep the declared RTL base and the
    /// word highlight follows spoken order (right→left, jumping across an
    /// embedded Latin run) instead of flipping the whole line to LTR.
    #[serde(default = "default_direction")]
    pub direction: String,

    // ── Typography overrides (None = template default) ─────────────────────
    /// Letter spacing in `em`. `None` keeps the template's constant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing_em: Option<f32>,
    /// Word spacing in `em`. `None` keeps the template's constant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_spacing_em: Option<f32>,
    /// Extra line gap in `em` (added on top of the line height).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing_em: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    /// `"none"` | `"uppercase"` | `"lowercase"`. `None` = template default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_case: Option<String>,
    /// Screen-side horizontal text alignment: `"left"` | `"center"` |
    /// `"right"`. `None` = template default (center).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_align: Option<String>,

    // ── Position overrides (None = template default) ───────────────────────
    /// Screen-side block anchor horizontal: `"left"` | `"center"` | `"right"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h_align: Option<String>,
    /// Horizontal offset 0..1 measured from the LEFT edge of the frame
    /// (screen-side, independent of reading direction).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h_offset: Option<f32>,
    /// Block anchor vertical: `"top"` | `"center"` | `"bottom"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_align: Option<String>,
    /// Vertical offset 0..1 measured from the TOP edge (replaces the
    /// template's `ANCHOR_Y_FRAC` when set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_offset: Option<f32>,
    /// Segment rotation in degrees, −180..180. `None` = no rotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f32>,

    // ── Motion overrides (None = template default entrance) ────────────────
    /// Entrance preset id: `""` = template default, `"static"` = no motion,
    /// else a preset id from the motion catalog (`pop-in`, `rise-in`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_duration_ms: Option<u32>,
    /// Rise/travel distance in `em` (presets that travel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_distance_em: Option<f32>,

    // ── Effects (tri-state: None = template default, Some = force) ─────────
    #[serde(default)]
    pub effects: EffectsParams,

    // ── Layout overrides (None = template default) ─────────────────────────
    /// Balanced line splitter: max lines per segment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
    /// Balanced line splitter: max line width as fraction of frame width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width_frac: Option<f32>,
    /// Segment char-limit override (boundary_score_limit_by_chars.maxChars).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<u32>,

    // ── Template style controls (data-driven per template.json) ───────────
    /// Per-control values keyed by control id (`"highlight-color"` → …).
    /// Merged (not replaced) by `patch_params_json`; consumed in
    /// `overrides::resolve` (`primary-color`, `highlight-color` first).
    #[serde(default)]
    pub style_values: serde_json::Map<String, serde_json::Value>,

    // ── Element inspector (per-word overrides — tscaps parity) ────────────
    /// Override per-kata, dialamatkan dengan index kata global (urutan
    /// dokumen, stream sebelum effects). Kosong = tanpa override.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub element_overrides: Vec<WordOverride>,
}

/// Satu override elemen kata (element inspector). Field terstruktur +
/// `css` (subset CSS yang ditafsir engine — BUKAN browser; lihat
/// `templates::element_css`). `css` menang atas field terstruktur.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WordOverride {
    /// Index kata global (urutan dokumen, stream sebelum effects).
    pub word: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Skala ukuran relatif base template (1.0 = tak berubah).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_scale: Option<f32>,
    /// Rotasi derajat di sekitar pusat kata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<f32>,
    /// Anchor absolut kata: fraksi lebar/tinggi frame untuk PUSAT kata
    /// (None = tetap di layout asli).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h_frac: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_frac: Option<f32>,
    /// Preset animasi per-kata: `"fade"|"rise"|"slide"|"scale"|"pop"|
    /// "snap"|"wobble"|"flash"|"none"` (None/unknown = template default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<String>,
    /// Durasi animasi preset (detik).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_duration_s: Option<f32>,
    /// Custom CSS subset (ditafsir `templates::element_css`; menang atas
    /// field terstruktur di atas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
}

/// Document-transform effects as user overrides. Every flag is tri-state:
/// `None` = inherit the template's declared effect, `Some(true)` = force
/// on, `Some(false)` = force off (tscaps `UpdateEffectsAction` parity).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EffectsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_free: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_punctuation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart_punctuation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carry_quotes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart_lowercase: Option<bool>,
    /// Emoji placement: `"segment-below"` | `"segment-above"` | `"word"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji_placement: Option<String>,
    /// Emoji size multiplier (tscaps default 1.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji_size: Option<f32>,
    /// Emoji gap in em (tscaps default 0.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji_gap: Option<f32>,
}

/// The REAL size of the registered source video (the project's own width ×
/// height), falling back to the controller's default 1920×1080 canvas when no
/// source is registered yet (unit tests, cold boot).
///
/// The fframes controller hardwires `ctx.current_video_size` to `TVideo`'s
/// compile-time consts, so templates must read dimensions through this
/// helper — it is the single place that consults `media_source`. Portrait and
/// odd-sized videos then lay out, wrap and anchor on their own pixel grid
/// (viewBox = width × height → uniform scale when mapped onto the video box).
pub fn ctx_video_size(ctx: &fframes::FFramesContext<'_, '_>) -> fframes::VideoSize {
    if let Some(provider) = ctx.media_source {
        for (video, _) in provider.get_all_video_data() {
            if let Some(meta) = &video.metadata {
                return fframes::VideoSize {
                    width: meta.width as usize,
                    height: meta.height as usize,
                };
            }
        }
    }
    ctx.current_video_size.clone()
}

// ── Canonical defaults ─────────────────────────────────────────────────────
// This is the ONE place default caption params live. Consumers (studio UIs,
// other apps) must obtain them via `default_document_json()` / the wasm
// bridge instead of hardcoding their own copy — a second set of defaults is
// a guarantee of drift.
pub(crate) fn default_font_family() -> String {
    "Arial".to_string()
}
pub(crate) fn default_font_size() -> f32 {
    110.0
}
pub(crate) fn default_font_weight() -> u16 {
    400
}
pub(crate) fn default_color() -> String {
    "#ffdd57".to_string()
}
pub(crate) fn default_y_frac() -> f32 {
    0.78
}
fn default_width_frac() -> f32 {
    0.78
}
fn default_pop_in_ms() -> u32 {
    250
}
fn default_travel_px() -> f32 {
    48.0
}
fn default_direction() -> String {
    "auto".to_string()
}

impl Default for CaptionParams {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            font_weight: default_font_weight(),
            color: default_color(),
            y_frac: default_y_frac(),
            width_frac: default_width_frac(),
            pop_in_ms: default_pop_in_ms(),
            travel_px: default_travel_px(),
            direction: default_direction(),
            letter_spacing_em: None,
            word_spacing_em: None,
            line_spacing_em: None,
            italic: None,
            underline: None,
            strikethrough: None,
            text_case: None,
            text_align: None,
            h_align: None,
            h_offset: None,
            v_align: None,
            v_offset: None,
            angle_deg: None,
            motion_preset: None,
            motion_duration_ms: None,
            motion_distance_em: None,
            effects: EffectsParams::default(),
            max_lines: None,
            max_width_frac: None,
            max_chars: None,
            style_values: serde_json::Map::new(),
            element_overrides: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionDocument {
    #[serde(default = "default_duration_frames")]
    pub duration_frames: u32,
    #[serde(default)]
    pub cues: Vec<Cue>,
    #[serde(default)]
    pub params: CaptionParams,
    /// Native template id; empty falls back to the v0 renderer.
    #[serde(default)]
    pub template: String,
    /// Sheet tambahan (track gaya). Sheet default = dokumen sendiri
    /// (doc.template + doc.params + cue tanpa `sheet`) - tak masuk list ini.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<Sheet>,
    /// Tag manual per kata (index global = alamat `element_overrides`) —
    /// digabung dengan tag otomatis (quote/number) dan role sheet saat
    /// render. JSON object: { "12": ["emphasis"], ... }.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub word_tags: std::collections::BTreeMap<u32, Vec<String>>,
    /// Rentang waktu (ms, `[start, end)`) saat efek text-behind-actor aktif
    /// (template pastor) — dihitung host dari hasil segmentasi orang.
    /// Segmen template masuk register behind hanya bila [start, end]-nya
    /// SELURUHNYA di dalam satu rentang (aturan tscaps: register tak boleh
    /// berbalik di tengah segmen).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub behind_ranges: Vec<(u32, u32)>,
    /// Base index kata per cue (paralel dengan `cues`) — diisi render_frame
    /// untuk doc sementara per-sheet agar index kata tetap global. Tidak
    /// diserialisasi; kosong = hitung kumulatif dari nol.
    #[serde(skip)]
    pub word_index_bases: Vec<u32>,
}

fn default_duration_frames() -> u32 {
    900
}

impl Default for CaptionDocument {
    fn default() -> Self {
        Self {
            duration_frames: default_duration_frames(),
            cues: Vec::new(),
            params: CaptionParams::default(),
            template: String::new(),
            sheets: Vec::new(),
            word_tags: std::collections::BTreeMap::new(),
            behind_ranges: Vec::new(),
            word_index_bases: Vec::new(),
        }
    }
}

static DOC: OnceLock<RwLock<CaptionDocument>> = OnceLock::new();

/// Frame video saat ini (JPEG bytes, penuh ukuran video) — disetel host
/// (JS studio/worker) sebelum `render_frame` saat template memakai layer
/// kaca (selene). None = template merender plate tanpa frame.
static FRAME_IMAGE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Masker orang (PNG alpha kecil, putih=person) untuk occlusion
/// text-behind-actor (pastor). Host menyetel per frame/per sampel masker
/// SEBELUM `render_frame`; None = tak ada occlusion (register quiet).
static PERSON_MASK: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Bounding-box area alpha>0 masker terakhir, dalam KOORDINAT PIKSEL MASKER
/// `(x0, y0, x1, y1, mask_w, mask_h)` (x1/y1 eksklusif). Di-cache saat
/// `set_person_mask_png` (decode sekali per sampel) — pemakaian: region
/// masker/clip occlusion pastor di-crop ke area orang supaya raster tak
/// menghitung seluruh frame tiap frame. None = decode gagal → fallback
/// full-frame (perilaku lama).
static PERSON_MASK_REGION: Mutex<Option<(f32, f32, f32, f32, f32, f32)>> =
    Mutex::new(None);

/// Decode PNG + pindai alpha → bbox piksel ber-alpha. None bila PNG tak
/// valid / seluruhnya transparan.
fn person_mask_bbox(png_bytes: &[u8]) -> Option<(f32, f32, f32, f32, f32, f32)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as f32, info.height as f32);
    let channels = info.color_type.samples() as usize;
    // Alpha selalu kanal terakhir (RGBA/GrayscaleAlpha); tanpa alpha =
    // masker opaque penuh → full frame.
    if !matches!(
        info.color_type,
        png::ColorType::Rgba | png::ColorType::GrayscaleAlpha
    ) {
        return Some((0.0, 0.0, w, h, w, h));
    }
    let mut x0 = w;
    let mut y0 = h;
    let mut x1 = 0.0f32;
    let mut y1 = 0.0f32;
    for y in 0..info.height as usize {
        let row = &buf[y * info.width as usize * channels..];
        for x in 0..info.width as usize {
            if row[x * channels + channels - 1] != 0 {
                let fx = x as f32;
                x0 = x0.min(fx);
                y0 = y0.min(y as f32);
                x1 = x1.max(fx + 1.0);
                y1 = y1.max(y as f32 + 1.0);
            }
        }
    }
    if x1 <= x0 {
        return None;
    }
    Some((x0, y0, x1, y1, w, h))
}

/// Setel masker orang (PNG, putih + alpha=confidence) SEBELUM `render_frame`.
/// PNG di-decode SEKALI di sini untuk bbox area orang (di-cache).
pub fn set_person_mask_png(bytes: &[u8]) {
    let region = person_mask_bbox(bytes);
    *PERSON_MASK.lock().unwrap_or_else(|p| p.into_inner()) = Some(bytes.to_vec());
    *PERSON_MASK_REGION.lock().unwrap_or_else(|p| p.into_inner()) = region;
}

/// Buang masker tersimpan (host memanggil saat frame di luar rentang aktif).
pub fn clear_person_mask() {
    *PERSON_MASK.lock().unwrap_or_else(|p| p.into_inner()) = None;
    *PERSON_MASK_REGION.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Salinan masker terakhir (untuk emit pastor); None bila belum disetel.
pub fn person_mask_png() -> Option<Vec<u8>> {
    PERSON_MASK
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Bbox masker terakhir `(x0, y0, x1, y1, mask_w, mask_h)` dalam piksel
/// masker; None bila belum disetel / decode gagal.
pub fn person_mask_region() -> Option<(f32, f32, f32, f32, f32, f32)> {
    *PERSON_MASK_REGION.lock().unwrap_or_else(|p| p.into_inner())
}

/// Setel frame video untuk layer kaca (JPEG penuh ukuran video). Dipanggil
/// host per frame SEBELUM `render_frame`.
pub fn set_frame_image(bytes: &[u8]) {
    *FRAME_IMAGE.lock().unwrap_or_else(|p| p.into_inner()) = Some(bytes.to_vec());
}

/// Buang frame yang tersimpan (host memanggil saat tak dipakai lagi).
pub fn clear_frame_image() {
    *FRAME_IMAGE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Salinan frame terakhir (untuk emit template); None bila belum disetel.
pub fn frame_image() -> Option<Vec<u8>> {
    FRAME_IMAGE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Base64 standar (alphabet + padding) — untuk `href="data:image/jpeg;base64,…"`
/// di layer kaca. Kecil dan tanpa dependensi; input beberapa ratus KB per
/// frame, ini bukan jalur kritis.
pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

// BreakLinesOpts needs &'a str (tied to the frame context lifetime), so the
// dynamic family/color strings are leaked once per document update instead of
// being borrowed from the document lock guard.
static LIVE_FONT_FAMILY: RwLock<&'static str> = RwLock::new("Arial");
static LIVE_TEXT_COLOR: RwLock<&'static str> = RwLock::new("#ffdd57");

fn leak_str(value: &str) -> &'static str {
    Box::leak(value.to_string().into_boxed_str())
}

fn live_family() -> &'static str {
    LIVE_FONT_FAMILY.read().map(|guard| *guard).unwrap_or("Arial")
}

fn live_color() -> &'static str {
    LIVE_TEXT_COLOR.read().map(|guard| *guard).unwrap_or("#ffdd57")
}


pub fn document() -> &'static RwLock<CaptionDocument> {
    DOC.get_or_init(|| RwLock::new(CaptionDocument::default()))
}

pub fn set_document_json(json: &str) -> Result<(), String> {
    let parsed: CaptionDocument =
        serde_json::from_str(json).map_err(|err| format!("invalid document: {err}"))?;
    if let Ok(mut guard) = LIVE_FONT_FAMILY.write() {
        *guard = leak_str(&parsed.params.font_family);
    }
    if let Ok(mut guard) = LIVE_TEXT_COLOR.write() {
        *guard = leak_str(&parsed.params.color);
    }
    let mut guard = document()
        .write()
        .map_err(|_| "document lock poisoned".to_string())?;
    *guard = parsed;
    Ok(())
}

pub fn replace_cues_json(json: &str) -> Result<(), String> {
    let cues: Vec<Cue> =
        serde_json::from_str(json).map_err(|err| format!("invalid cues: {err}"))?;
    let mut guard = document()
        .write()
        .map_err(|_| "document lock poisoned".to_string())?;
    guard.cues = cues;
    Ok(())
}

/// Daftar kata dokumen aktif (urutan stream, sebelum effects) — alamat
/// `element_overrides` untuk element inspector per-kata:
/// `[{"index":0,"text":".","start_ms":0.0,"end_ms":210.0}, .]`.
pub fn list_words_json() -> String {
    let guard = document().read().unwrap_or_else(|err| err.into_inner());
    let words = crate::templates::pipeline::doc_words(&guard);
    let items: Vec<serde_json::Value> = words
        .iter()
        .map(|w| {
            serde_json::json!({
                "index": w.index,
                "text": w.text,
                "start_ms": w.start_ms,
                "end_ms": w.end_ms,
                "tags": w.tags,
            })
        })
        .collect();
    serde_json::Value::Array(items).to_string()
}

/// Params JSON → CSS subset (panel Code settings).
pub fn params_to_css_json(params_json: &str) -> Result<String, String> {
    let p: CaptionParams =
        serde_json::from_str(params_json).map_err(|err| format!("invalid params JSON: {err}"))?;
    Ok(crate::templates::params_css::params_to_css(&p))
}

/// CSS subset → params JSON (merge ke base; field di luar CSS dipertahankan).
pub fn params_from_css_json(base_params_json: &str, css: &str) -> Result<String, String> {
    let mut p: CaptionParams = serde_json::from_str(base_params_json)
        .map_err(|err| format!("invalid params JSON: {err}"))?;
    crate::templates::params_css::apply_css_to_params(&mut p, css)?;
    serde_json::to_string(&p).map_err(|err| format!("serialize params: {err}"))
}

/// Merge a partial params JSON object onto the live document's params —
/// the patch API the studio panels use (typography/position/effects/…).
///
/// Only the keys present in `patch_json` are written; everything else keeps
/// its current value. Nested `effects` merges per-key as well. Also refreshes
/// the live font/color strings so the v0 renderer follows immediately.
pub fn patch_params_json(patch_json: &str) -> Result<(), String> {
    let patch: serde_json::Value =
        serde_json::from_str(patch_json).map_err(|err| format!("invalid params patch: {err}"))?;
    if !patch.is_object() {
        return Err("params patch must be a JSON object".to_string());
    }
    let mut guard = document()
        .write()
        .map_err(|_| "document lock poisoned".to_string())?;
    let mut current = serde_json::to_value(&guard.params)
        .map_err(|err| format!("serialize current params: {err}"))?;
    merge_json(&mut current, patch);
    let next: CaptionParams = serde_json::from_value(current)
        .map_err(|err| format!("invalid params after merge: {err}"))?;
    if let Ok(mut live) = LIVE_FONT_FAMILY.write() {
        *live = leak_str(&next.font_family);
    }
    if let Ok(mut live) = LIVE_TEXT_COLOR.write() {
        *live = leak_str(&next.color);
    }
    guard.params = next;
    Ok(())
}

/// Recursively merge `patch` into `base` (objects merge per-key; anything
/// else replaces).
fn merge_json(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                match base_map.get_mut(&key) {
                    Some(slot) => merge_json(slot, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (slot, patch) => *slot = patch,
    }
}

/// Parse a subtitle file (`.srt` / `.vtt`) → JSON
/// `{"duration_frames": u32, "cues": [Cue, …]}` for the editor to merge.
/// Pure: the live document is untouched, the caller decides.
pub fn parse_subtitle_json(text: &str, format: &str) -> Result<String, String> {
    let format = srt::Format::parse_str(format)?;
    let parsed = srt::parse(text, format)?;
    let value = serde_json::json!({
        "duration_frames": parsed.duration_frames,
        "cues": parsed.cues,
    });
    serde_json::to_string(&value).map_err(|err| format!("serialize cues: {err}"))
}

/// Cues JSON (`[Cue, …]`) → subtitle file text. `format` is `"srt"` or `"vtt"`.
pub fn format_subtitle_json(cues_json: &str, format: &str) -> Result<String, String> {
    let format = srt::Format::parse_str(format)?;
    let cues: Vec<Cue> =
        serde_json::from_str(cues_json).map_err(|err| format!("invalid cues: {err}"))?;
    Ok(srt::format(&cues, format))
}

/// Canonical empty document as JSON: default params, no cues, no template.
///
/// Consumers build new projects from this instead of hardcoding their own
/// defaults — `CaptionParams::default()` above is the single source.
pub fn default_document_json() -> String {
    serde_json::to_string(&CaptionDocument::default()).expect("default document serializes")
}

/// Re-export of [`templates::template_catalog_json`] — the one template
/// catalog (`[{"id", "label"}, …]`) consumers must use.
pub fn template_catalog_json() -> String {
    templates::template_catalog_json()
}

/// Gabungkan SVG sheet overlay ke sheet dasar: inner `overlay` (def +
/// body) disisipkan sebelum `</svg>` base, dengan semua `id="…"` dan
/// `url(#…)` diberi prefix `s{idx}-` supaya defs antar-sheet tak bentrok.
/// Semua renderer (native template + v0) memancarkan `<svg>` penuh dengan
/// viewBox kanvas yang sama, jadi penumpukan langsung valid.
fn merge_svgs(base: String, overlay: &str, idx: usize) -> String {
    let Some(open_end) = overlay.find('>') else {
        return base;
    };
    let Some(close) = overlay.rfind("</svg>") else {
        return base;
    };
    if close <= open_end {
        return base;
    }
    let inner = &overlay[open_end + 1..close];
    let prefix = format!("s{idx}-");
    let namespaced = inner
        .replace("id=\"", &format!("id=\"{prefix}"))
        .replace("url(#", &format!("url(#{prefix}"));
    match base.rfind("</svg>") {
        Some(pos) => format!("{}{}{}", &base[..pos], namespaced, &base[pos..]),
        None => base,
    }
}

#[derive(Debug, Default)]
pub struct CaptionVideo;

impl Video for CaptionVideo {
    const FPS: usize = 30;
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    const BACKGROUND_COLOR: Color = Color::TRANSPARENT;

    fn duration(&self) -> Duration<'_> {
        let frames = document()
            .read()
            .map(|doc| doc.duration_frames as usize)
            .unwrap_or_else(|_| default_duration_frames() as usize);
        Duration::Frames(frames)
    }

    fn audio(&self) -> AudioMap<'_> {
        AudioMap::none()
    }

    fn render_frame<'a>(&'a self, mut frame: Frame, ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        let Ok(doc) = document().read() else {
            return Svgr::empty();
        };

        let now_ms = frame.seconds() * 1000.0;

        // ── Rencana render per sheet ─────────────────────────────────────────
        // Sheet default (doc-level) selalu indeks 0; sheet eksplisit menyusul
        // sesuai urutan `doc.sheets`. Sheet tanpa cue dilewati. Cukup satu
        // sheet kandidat → jalur cepat tanpa clone/merge.
        let has_extra_cues = doc.sheets.iter().any(|s| {
            doc.cues
                .iter()
                .any(|c| c.sheet.as_deref() == Some(s.id.as_str()))
        });

        if !has_extra_cues {
            // Jalur cepat: hanya sheet default (perilaku pra-sheets).
            return render_one(
                &doc,
                &doc.template,
                &doc.params,
                &doc.cues,
                now_ms,
                &mut frame,
                ctx,
            );
        }

        // Kandidat: (sheet_idx, template, params, cues, bases, own_sheet) —
        // sheet yang seluruh cue-nya sudah jauh lewat (+1500ms) di-skip
        // supaya tak di-clone. Bases = index kata global per cue subset:
        // doc sementara tidak boleh menggeser alamat kata (tag manual &
        // element_overrides memakai index dokumen penuh).
        let mut bases_full: Vec<u32> = Vec::with_capacity(doc.cues.len());
        let mut running = 0u32;
        for cue in &doc.cues {
            bases_full.push(running);
            running += crate::templates::pipeline::tokenize(&cue.text).len() as u32;
        }
        let mut plans: Vec<(usize, &str, &CaptionParams, Vec<Cue>, Vec<u32>, Option<&Sheet>)> =
            Vec::new();
        let mut default_cues: Vec<Cue> = Vec::new();
        let mut default_bases: Vec<u32> = Vec::new();
        for (i, cue) in doc.cues.iter().enumerate() {
            if cue.sheet.as_deref().map_or(true, |s| s.is_empty()) {
                default_cues.push(cue.clone());
                default_bases.push(bases_full[i]);
            }
        }
        if !default_cues.is_empty() {
            let last_end = default_cues.iter().map(|c| c.end_ms).max().unwrap_or(0);
            if (now_ms as u32) <= last_end.saturating_add(1500) {
                plans.push((
                    0,
                    doc.template.as_str(),
                    &doc.params,
                    default_cues,
                    default_bases,
                    None,
                ));
            }
        }
        for (i, sheet) in doc.sheets.iter().enumerate() {
            let mut cues: Vec<Cue> = Vec::new();
            let mut bases: Vec<u32> = Vec::new();
            for (ci, cue) in doc.cues.iter().enumerate() {
                if cue.sheet.as_deref() == Some(sheet.id.as_str()) {
                    cues.push(cue.clone());
                    bases.push(bases_full[ci]);
                }
            }
            if cues.is_empty() {
                continue;
            }
            let last_end = cues.iter().map(|c| c.end_ms).max().unwrap_or(0);
            if (now_ms as u32) > last_end.saturating_add(1500) {
                continue;
            }
            let params = sheet.params.as_ref().unwrap_or(&doc.params);
            plans.push((i + 1, sheet.template.as_str(), params, cues, bases, Some(sheet)));
        }
        if plans.len() == 1 {
            let p = plans.remove(0);
            return render_one(&doc, p.1, p.2, &p.3, now_ms, &mut frame, ctx);
        }

        // Render tiap sheet (template native butuh &CaptionDocument penuh —
        // bangun doc sementara per sheet), lalu gabung string-nya. Sheet
        // eksplisit ikut supaya role-nya (peak/hook) ikut men-tag kata.
        let mut rendered: Vec<(usize, String)> = Vec::new();
        for (idx, template, params, cues, bases, own_sheet) in plans {
            let temp = CaptionDocument {
                duration_frames: doc.duration_frames,
                cues,
                params: params.clone(),
                template: template.to_string(),
                sheets: own_sheet.cloned().into_iter().collect(),
                word_tags: doc.word_tags.clone(),
                behind_ranges: doc.behind_ranges.clone(),
                word_index_bases: bases,
            };
            let svg = render_one(&temp, template, params, &temp.cues, now_ms, &mut frame, ctx);
            let value = svg.value;
            if value.is_empty() {
                continue;
            }
            rendered.push((idx, value));
        }
        let Some((_, base)) = rendered.first().cloned() else {
            return Svgr::empty();
        };
        let merged = rendered
            .iter()
            .skip(1)
            .fold(base, |acc, (idx, svg)| merge_svgs(acc, svg, *idx));
        Svgr::from(merged)
    }

}

/// Render satu sheet: template native (doc sementara) atau v0.
/// `template` kosong = renderer v0 (cue aktif pertama).
#[allow(clippy::too_many_arguments)]
fn render_one<'a>(
    doc: &CaptionDocument,
    template: &str,
    params: &CaptionParams,
    cues: &[Cue],
    now_ms: f32,
    frame: &mut Frame,
    ctx: &FFramesContext<'a, '_>,
) -> Svgr<'a> {
    if !template.is_empty() {
        if let Some(svg) = templates::render(template, doc, now_ms, ctx) {
            return svg;
        }
    }

    let Some(cue) = cues
        .iter()
        .find(|cue| now_ms >= cue.start_ms as f32 && now_ms < cue.end_ms as f32)
    else {
        return Svgr::empty();
    };

    render_v0(cue, params, now_ms, frame, ctx)
}

/// Renderer v0: satu cue aktif, params dokumen/sheet.
fn render_v0<'a>(
    cue: &Cue,
    params: &CaptionParams,
    now_ms: f32,
    frame: &mut Frame,
    ctx: &FFramesContext<'a, '_>,
) -> Svgr<'a> {
        let pop_in_ms = params.pop_in_ms.max(1) as f32;
        let local_ms = now_ms - cue.start_ms as f32;
        let progress = (local_ms / pop_in_ms).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        let opacity = eased;
        let dy = (1.0 - eased) * params.travel_px;

        // v0 honouring the new override fields (position/align/case).
        let align = match params.text_align.as_deref() {
            Some("left") => TextAlign::Left,
            Some("right") => TextAlign::Right,
            _ => TextAlign::Center,
        };
        let anchor_x = match params.h_align.as_deref() {
            Some("left") => params.h_offset.unwrap_or(0.0).clamp(0.0, 1.0),
            Some("right") => 1.0 - params.h_offset.unwrap_or(0.0).clamp(0.0, 1.0),
            Some("center") => 0.5,
            _ => params.h_offset.unwrap_or(0.5).clamp(0.0, 1.0),
        };
        let anchor_y = match params.v_align.as_deref() {
            Some("top") => params.v_offset.unwrap_or(0.0).clamp(0.0, 1.0),
            Some("bottom") => 1.0 - params.v_offset.unwrap_or(0.0).clamp(0.0, 1.0),
            Some("center") => 0.5,
            _ => params
                .v_offset
                .unwrap_or(params.y_frac)
                .clamp(0.0, 1.0),
        };
        let text = match params.text_case.as_deref() {
            Some("uppercase") => cue.text.to_uppercase(),
            Some("lowercase") => cue.text.to_lowercase(),
            _ => cue.text.clone(),
        };

        let width = ctx_video_size(ctx).width;
        let height = ctx_video_size(ctx).height;
        let body = frame
            .text_break_lines(
                ctx,
                text.as_str(),
                BreakLinesOpts {
                    font: FontQuery {
                        family: live_family(),
                        size: params.font_size.max(1.0) as usize,
                        weight: params.font_weight,
                        ..Default::default()
                    },
                    width: ((width as f32) * params.width_frac.clamp(0.1, 1.0)) as usize,
                    line_height: 1.1,
                    x: ((width as f32) * anchor_x) as i32,
                    y: ((height as f32) * anchor_y) as i32,
                    align,
                    fill: live_color(),
                    dominant_baseline: "middle",
                    text_anchor: match align {
                        TextAlign::Left => "start",
                        TextAlign::Right => "end",
                        TextAlign::Center => "middle",
                    },
                    opacity: 1.0,
                },
            )
            .unwrap_or_default();

        fframes::svgr!(
            <svg
                xmlns="http://www.w3.org/2000/svg"
                viewBox={format!("0 0 {} {}", width, height)}
                width={width}
                height={height}
            >
                <g opacity={opacity} transform={Transform::translate(0.0, dy)}>
                    {body}
                </g>
            </svg>
        )
    }

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_patch_merges_partial_json() {
        let original = CaptionParams::default();
        let mut doc = CaptionDocument {
            params: original.clone(),
            ..Default::default()
        };
        doc.params.letter_spacing_em = Some(0.05);
        *document().write().unwrap() = doc;

        patch_params_json(r#"{"font_size": 90, "effects": {"gap_free": true}}"#).unwrap();

        let guard = document().read().unwrap();
        assert_eq!(guard.params.font_size, 90.0);
        assert_eq!(guard.params.effects.gap_free, Some(true));
        // Unpatched fields survive the merge.
        assert_eq!(guard.params.letter_spacing_em, Some(0.05));
        assert_eq!(guard.params.color, original.color);
    }

    #[test]
    fn params_patch_rejects_non_object() {
        assert!(patch_params_json("[1,2,3]").is_err());
        assert!(patch_params_json("not json").is_err());
    }

    #[test]
    fn old_documents_without_new_fields_deserialize() {
        // A pre-extension document JSON (only the original 9 fields).
        let json = r##"{
            "duration_frames": 900,
            "cues": [{"start_ms": 0, "end_ms": 1000, "text": "hi"}],
            "params": {
                "font_family": "Arial", "font_size": 110.0, "font_weight": 400,
                "color": "#ffdd57", "y_frac": 0.78, "width_frac": 0.78,
                "pop_in_ms": 250, "travel_px": 48.0, "direction": "auto"
            },
            "template": ""
        }"##;
        let doc: CaptionDocument = serde_json::from_str(json).unwrap();
        assert!(doc.params.letter_spacing_em.is_none());
        assert!(doc.params.effects.gap_free.is_none());
        assert!(doc.params.max_lines.is_none());
        assert!(doc.sheets.is_empty());
        assert!(doc.cues[0].sheet.is_none());
    }

    #[test]
    fn merge_svgs_namespaces_overlay_ids() {
        let base = r#"<svg viewBox="0 0 10 10"><defs><filter id="a-shadow"/></defs><g filter="url(#a-shadow)"/></svg>"#.to_string();
        let overlay = r#"<svg viewBox="0 0 10 10"><defs><filter id="a-shadow"/></defs><g filter="url(#a-shadow)"/></svg>"#;
        let merged = merge_svgs(base, overlay, 1);
        // Overlay id diberi prefix supaya tak menimpa id base.
        assert!(merged.contains(r#"id="s1-a-shadow""#));
        assert!(merged.contains(r#"url(#s1-a-shadow)"#));
        // Id base tetap.
        assert!(merged.contains(r#"id="a-shadow""#));
        // Overlay berada di dalam root svg base (satu dokumen).
        assert_eq!(merged.matches("</svg>").count(), 1);
    }

    #[test]
    fn merge_svgs_handles_missing_close() {
        assert_eq!(merge_svgs("<svg></svg>".to_string(), "bukan-svg", 0), "<svg></svg>");
        assert_eq!(merge_svgs(String::new(), "<svg><g/></svg>", 0), String::new());
    }
}
