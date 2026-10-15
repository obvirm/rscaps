//! Resolved user overrides — the ONE place `CaptionParams` meets a
//! template's hardcoded defaults.
//!
//! Every template hands its spec constants ([`TemplateDefaults`]) plus the
//! document's [`crate::CaptionParams`] here; the returned [`Overrides`]
//! carries the effective values (user override when set, template default
//! otherwise). Panels edit params via `patch_params_json`; renderers consult
//! Overrides — no template ever reads `doc.params` fields ad hoc, so the
//! override semantics live in exactly one file.
//!
//! Tri-state rule: `Option` params — `None` = inherit template, `Some` =
//! force. The legacy non-Option fields (`font_family`, `font_size`,
//! `font_weight`, `color`, `y_frac`) predate this layer; they are treated as
//! "user changed" only when they differ from their v0 defaults, so switching
//! a template on an untouched document keeps the template's identity.

use crate::CaptionParams;

/// A template's spec constants for the knobs the user can override.
/// Every field mirrors the template's own `const`s — pass those directly so
/// the defaults can never drift from what the template renders.
#[derive(Debug, Clone, Copy)]
pub struct TemplateDefaults {
    /// `FONT_SIZE_CQH` — font size as fraction of frame height.
    pub font_size_cqh: f32,
    /// `LETTER_SPACING_EM`.
    pub letter_spacing_em: f32,
    /// `WORD_SPACING_EM`.
    pub word_spacing_em: f32,
    /// `LINE_SPACING_EM` — extra gap on top of line height.
    pub line_spacing_em: f32,
    /// `ANCHOR_X_FRAC` — block anchor horizontal fraction (0 left … 1 right).
    pub anchor_x: f32,
    /// `ANCHOR_Y_FRAC` — block anchor vertical fraction (0 top … 1 bottom).
    pub anchor_y: f32,
    /// The template's primary (dominant) font family name.
    pub font: &'static str,
    /// The template's primary font weight.
    pub font_weight: u16,
    /// The template's primary text color.
    pub primary: &'static str,
    /// The template's highlight (active-word) color.
    pub highlight: &'static str,
    /// Whether the template forces uppercase regardless of user text_case.
    pub force_uppercase: bool,
    /// `MIN_LINES` — balanced line splitter minimum.
    pub min_lines: usize,
    /// `MAX_LINES` — balanced line splitter maximum.
    pub max_lines: usize,
    /// `MAX_WIDTH_RATIO` — max line width as fraction of frame width.
    pub max_width_ratio: f32,
}

/// Effective values for one render: user override when set, template
/// default otherwise. Ready to plug into the template's layout math.
#[derive(Debug, Clone)]
pub struct Overrides {
    pub letter_spacing_em: f32,
    pub word_spacing_em: f32,
    pub line_spacing_em: f32,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub text_case: TextCase,
    pub text_align: Align,
    /// Block anchor horizontal (0..1 from left, screen-side).
    pub anchor_x: f32,
    /// Block anchor vertical (0..1 from top).
    pub anchor_y: f32,
    /// Segment rotation in degrees.
    pub angle_deg: f32,
    /// Font family to emit (user override or the template's primary).
    pub font_family: String,
    pub font_weight: u16,
    /// Primary text color (style control > user color > template primary).
    pub primary: String,
    /// Highlight (active-word) color (style control > template default).
    pub highlight: String,
    /// Balanced line splitter: effective min lines.
    pub min_lines: usize,
    /// Balanced line splitter: effective max lines (user override wins).
    pub max_lines: usize,
    /// Max line width as fraction of frame width (user override wins).
    pub max_width_ratio: f32,
    /// Motion: `true` = entrance dimatikan ("static"), `false` = pakai
    /// entrance template.
    pub motion_static: bool,
    /// Entrance duration override (detik); `None` = durasi template.
    pub motion_duration_s: Option<f32>,
    /// Entrance travel/distance override (em); `None` = jarak template.
    pub motion_distance_em: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCase {
    None,
    Uppercase,
    Lowercase,
}

impl TextCase {
    pub fn apply(&self, text: &str) -> String {
        match self {
            TextCase::None => text.to_string(),
            TextCase::Uppercase => text.to_uppercase(),
            TextCase::Lowercase => text.to_lowercase(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    /// SVG `text-anchor` for a positioned word/line.
    pub fn text_anchor(&self) -> &'static str {
        match self {
            Align::Left => "start",
            Align::Center => "middle",
            Align::Right => "end",
        }
    }
}

/// Resolve user params against a template's defaults. Called once per
/// frame at the top of every template render.
pub fn resolve(params: &CaptionParams, defaults: TemplateDefaults) -> Overrides {
    let text_case = match params.text_case.as_deref() {
        Some("uppercase") => TextCase::Uppercase,
        Some("lowercase") => TextCase::Lowercase,
        Some("none") => TextCase::None,
        _ => {
            if defaults.force_uppercase {
                TextCase::Uppercase
            } else {
                TextCase::None
            }
        }
    };
    let text_align = match params.text_align.as_deref() {
        Some("left") => Align::Left,
        Some("right") => Align::Right,
        _ => Align::Center,
    };
    let anchor_x = match params.h_align.as_deref() {
        Some("left") => params.h_offset.unwrap_or(0.0).clamp(0.0, 1.0),
        Some("right") => 1.0 - params.h_offset.unwrap_or(0.0).clamp(0.0, 1.0),
        Some("center") => 0.5,
        // No align override: a bare h_offset is an absolute x fraction.
        _ => params.h_offset.unwrap_or(defaults.anchor_x).clamp(0.0, 1.0),
    };
    let anchor_y = match params.v_align.as_deref() {
        Some("top") => params.v_offset.unwrap_or(0.0).clamp(0.0, 1.0),
        Some("bottom") => 1.0 - params.v_offset.unwrap_or(0.0).clamp(0.0, 1.0),
        Some("center") => 0.5,
        _ => params
            .v_offset
            .or_else(|| {
                // Legacy knob: y_frac was the only vertical control; treat a
                // non-default y_frac as an explicit anchor so old projects
                // keep working.
                if (params.y_frac - crate::default_y_frac()).abs() > f32::EPSILON {
                    Some(params.y_frac)
                } else {
                    None
                }
            })
            .unwrap_or(defaults.anchor_y)
            .clamp(0.0, 1.0),
    };
    let font_family = if params.font_family == crate::default_font_family() {
        defaults.font.to_string()
    } else {
        params.font_family.clone()
    };
    let font_weight = if params.font_weight == crate::default_font_weight() {
        defaults.font_weight
    } else {
        params.font_weight
    };
    // Style controls win over the legacy color knob over the template.
    let style_color = |id: &str| -> Option<String> {
        params
            .style_values
            .get(id)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let primary = style_color("primary-color").unwrap_or_else(|| {
        if params.color == crate::default_color() {
            defaults.primary.to_string()
        } else {
            params.color.clone()
        }
    });
    let mut highlight =
        style_color("highlight-color").unwrap_or_else(|| defaults.highlight.to_string());
    if highlight.is_empty() {
        // Template tanpa highlight terpisah: warna highlight = warna utama.
        highlight = primary.clone();
    }
    Overrides {
        letter_spacing_em: params.letter_spacing_em.unwrap_or(defaults.letter_spacing_em),
        word_spacing_em: params.word_spacing_em.unwrap_or(defaults.word_spacing_em),
        line_spacing_em: params.line_spacing_em.unwrap_or(defaults.line_spacing_em),
        italic: params.italic.unwrap_or(false),
        underline: params.underline.unwrap_or(false),
        strikethrough: params.strikethrough.unwrap_or(false),
        text_case,
        text_align,
        anchor_x,
        anchor_y,
        angle_deg: params.angle_deg.unwrap_or(0.0).clamp(-180.0, 180.0),
        font_family,
        font_weight,
        primary,
        highlight,
        min_lines: defaults.min_lines,
        max_lines: params
            .max_lines
            .map(|n| n as usize)
            .unwrap_or(defaults.max_lines)
            .max(defaults.min_lines),
        max_width_ratio: params
            .max_width_frac
            .unwrap_or(defaults.max_width_ratio)
            .clamp(0.1, 1.0),
        motion_static: matches!(params.motion_preset.as_deref(), Some("static")),
        motion_duration_s: params.motion_duration_ms.map(|ms| ms as f32 / 1000.0),
        motion_distance_em: params.motion_distance_em,
    }
}

impl Overrides {
    /// Durasi entrance: user override (ms → detik) atau default template.
    pub fn duration_or(&self, default_s: f32) -> f32 {
        self.motion_duration_s.unwrap_or(default_s)
    }

    /// Jarak/travel entrance (em): user override atau default template.
    pub fn distance_or(&self, default_em: f32) -> f32 {
        self.motion_distance_em.unwrap_or(default_em)
    }
}

/// Font size resolution shared by every template:
/// - user changed `font_size` away from the v0 default → absolute px;
/// - otherwise → template `font_size_cqh × video_h`.
pub fn resolve_font_size(params: &CaptionParams, defaults: TemplateDefaults, video_h: f32) -> f32 {
    if (params.font_size - crate::default_font_size()).abs() > f32::EPSILON {
        params.font_size.max(1.0)
    } else {
        (defaults.font_size_cqh * video_h as f32).max(1.0)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const LOKI: TemplateDefaults = TemplateDefaults {
        font_size_cqh: 0.045,
        letter_spacing_em: 0.02,
        word_spacing_em: 0.16,
        line_spacing_em: 0.0,
        anchor_x: 0.5,
        anchor_y: 0.75,
        font: "Komika Axis",
        font_weight: 400,
        primary: "#ffffff",
        highlight: "#ffea00",
        force_uppercase: true,
        min_lines: 1,
        max_lines: 2,
        max_width_ratio: 0.72,
    };

    #[test]
    fn untouched_params_keep_template_identity() {
        let ov = resolve(&CaptionParams::default(), LOKI);
        assert_eq!(ov.letter_spacing_em, 0.02);
        assert_eq!(ov.word_spacing_em, 0.16);
        assert_eq!(ov.anchor_y, 0.75);
        assert_eq!(ov.font_family, "Komika Axis");
        assert_eq!(ov.primary, "#ffffff");
        assert_eq!(ov.text_case, TextCase::Uppercase);
        // v0 default color/font/weight fall back to the template.
        assert_eq!(ov.font_weight, 400);
    }

    #[test]
    fn user_overrides_win() {
        let params = CaptionParams {
            letter_spacing_em: Some(0.1),
            v_offset: Some(0.2),
            text_case: Some("none".into()),
            ..Default::default()
        };
        let ov = resolve(&params, LOKI);
        assert_eq!(ov.letter_spacing_em, 0.1);
        assert_eq!(ov.anchor_y, 0.2);
        assert_eq!(ov.text_case, TextCase::None);
    }

    #[test]
    fn v_align_bottom_flips_offset_from_bottom() {
        let params = CaptionParams {
            v_align: Some("bottom".into()),
            v_offset: Some(0.1),
            ..Default::default()
        };
        let ov = resolve(&params, LOKI);
        assert!((ov.anchor_y - 0.9).abs() < 1e-6);
    }

    #[test]
    fn font_size_absolute_beats_template_cqh() {
        let params = CaptionParams {
            font_size: 80.0,
            ..Default::default()
        };
        assert_eq!(resolve_font_size(&params, LOKI, 1080.0), 80.0);
        // Untouched → template 4.5cqh @1080 = 48.6.
        let untouched = resolve_font_size(&CaptionParams::default(), LOKI, 1080.0);
        assert!((untouched - 48.6).abs() < 1e-3);
    }

    #[test]
    fn angle_clamps_to_half_turn() {
        let params = CaptionParams {
            angle_deg: Some(720.0),
            ..Default::default()
        };
        assert_eq!(resolve(&params, LOKI).angle_deg, 180.0);
    }
}

