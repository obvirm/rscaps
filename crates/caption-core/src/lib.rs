use fframes::{
    AudioMap, BreakLinesOpts, Color, Duration, FFramesContext, FontQuery, Frame, Svgr, TextAlign,
    Transform, Video,
};
use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};

pub mod srt;
pub mod templates;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
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
        }
    }
}

static DOC: OnceLock<RwLock<CaptionDocument>> = OnceLock::new();

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
/// `[{"index":0,"text":"…","start_ms":0.0,"end_ms":210.0}, …]`.
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
            })
        })
        .collect();
    serde_json::Value::Array(items).to_string()
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

        // Native templates own their whole document window (gap-free tails
        // cross cue boundaries), so they run before the per-cue lookup.
        if !doc.template.is_empty() {
            if let Some(svg) = templates::render(&doc.template, &doc, now_ms, ctx) {
                return svg;
            }
        }

        let Some(cue) = doc
            .cues
            .iter()
            .find(|cue| now_ms >= cue.start_ms as f32 && now_ms < cue.end_ms as f32)
        else {
            return Svgr::empty();
        };

        let params = &doc.params;
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
    }
}
