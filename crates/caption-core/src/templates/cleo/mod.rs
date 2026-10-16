//! Native port of the tscaps `cleo` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\cleo\{style.scss,template.json}`.
//! Lora Medium at 4.3cqh that *grows for short captions*
//! (`dynamic-font-size` 0.06 under a 12-character threshold); the whole
//! segment settles in (0.35s, from 80% opacity, 0.06em of travel) and a
//! four-layer black shadow — one tight, three soft — sits behind the
//! line. Clause-boundary segments with `gap_free`, `carry_quotes`,
//! `smart_punctuation` and `smart_lowercase`; block bottom-anchored at
//! 0.65H.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from cleo's style.scss + template.json) ───────────────
const FONT: &str = "Lora"; // fontStack lora ('Lora Variable' in scss)
const FONT_WEIGHT: u16 = 500; // fontWeight: 500
const FONT_SIZE_CQH: f32 = 0.043; // font-size: 4.3cqh
const LETTER_SPACING_EM: f32 = 0.0; // letterSpacing: 0
const LINE_HEIGHT: f32 = 1.2; // line-height: 1.2 (mixin default)
const WORD_SPACING_EM: f32 = 0.13; // .word margin: 0 0.13em
const ANCHOR_Y_FRAC: f32 = 0.65; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 18; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 7; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.73; // lineSplitter.maxWidthRatio
const SETTLE_DISTANCE_EM: f32 = 0.06; // settle-in($distance: 0.06em)
const DYN_THRESHOLD: f32 = 12.0; // dynamic-font-size valueOn ('12')
const DYN_STRENGTH: f32 = 0.06; // dynamic-font-size(0.06)
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
// text-shadow: 0 0 0.15em $shadow, then three soft-drop-shadow(0.06, 1.2)
const TIGHT_BLUR_EM: f32 = 0.15;
const SOFT_DY_EM: f32 = 0.06;
const SOFT_BLUR_EM: f32 = 1.2;
const SHADOW_COLOR: &str = "#000000"; // --tscaps-shadow-color default
// -webkit-text-stroke: var(--tscaps-outline-thickness, 0em) $outline-color
const OUTLINE_THICKNESS_EM: f32 = 0.0;
const OUTLINE_COLOR: &str = "#000000"; // --tscaps-outline-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // cleo has no line-spacing (lines stack at line_h)
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: false, // cleo keeps the spoken case
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// cleo's `effects` array: `gap_free`, `carry_quotes`,
/// `smart_punctuation`, `smart_lowercase` — declaration order is the
/// application order.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[
        effects::Effect::GapFree,
        effects::Effect::CarryQuotes,
        effects::Effect::SmartPunctuation,
        effects::Effect::SmartLowercase,
    ],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const LORA_TTF: &[u8] = include_bytes!("../../../assets/lora-latin-500-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Lora.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(LORA_TTF, 0).expect("Lora parses");
        let upem = face.units_per_em() as f32;
        (
            face.ascender() as f32 / upem,
            face.descender().abs() as f32 / upem,
            upem,
        )
    })
}

/// Baseline metrics for whichever family dominates the segment.
fn line_metrics(dominant: &str) -> (f32, f32, f32) {
    if dominant == FONT {
        font_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}

// ── dynamic font size ─────────────────────────────────────────────────────

/// `--segment-char-count`: the segment's raw texts joined by single
/// spaces, measured in UTF-16 units (JS `Segment.getText().length` —
/// effects rewrite `display`, never `text`).
fn segment_char_count(words: &[pipeline::TimedWord]) -> usize {
    let utf16: usize = words.iter().map(|w| w.text.encode_utf16().count()).sum();
    utf16 + words.len().saturating_sub(1)
}

/// `max(1, 1 + (threshold - char_count) * strength)` — short captions
/// grow, twelve characters and up render at the plain 4.3cqh.
fn font_size_scale(char_count: usize) -> f32 {
    (1.0 + (DYN_THRESHOLD - char_count as f32) * DYN_STRENGTH).max(1.0)
}

// ── render ────────────────────────────────────────────────────────────────

pub fn render<'a>(
    doc: &CaptionDocument,
    now_ms: f32,
    ctx: &FFramesContext<'a, '_>,
) -> Svgr<'a> {
    let segments = pipeline::collect_segments(doc, &SPLIT);
    let Some(segment) = segments
        .iter()
        .find(|s| now_ms >= s.start_ms && now_ms < s.end_ms)
    else {
        return Svgr::empty();
    };

    let video_w = crate::ctx_video_size(ctx).width;
    let video_h = crate::ctx_video_size(ctx).height;
    // User overrides (params) win over the template's spec constants.
    let ov = overrides::resolve(&doc.params, DEFAULTS);
    // Style controls data-driven (panel Style → template.json defaults).
    let st = style::Style::of(&doc.params, "cleo");
    // The whole segment — type, metrics, shadows — scales with the count.
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32)
        * font_size_scale(segment_char_count(&segment.words));
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        LORA_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Bottom-aligned block: the block's bottom edge sits at 0.65H; lines
    // center horizontally. lineSpacing is 0, so lines stack at line_h.
    let block_h = lines.len() as f32 * line_h;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Shadow geometry (px from em ratios): one tight layer, three soft
    // ones (soft-drop-shadow(0.06, 1.2) repeated).
    let tight_std = TIGHT_BLUR_EM * fs / 2.0;
    let soft_dy = SOFT_DY_EM * fs;
    let soft_std = SOFT_BLUR_EM * fs / 2.0;
    let stroke_px = st.f32("outline-thickness", OUTLINE_THICKNESS_EM) * fs;
    let outline_color = st.color("outline-color", OUTLINE_COLOR);

    // The segment settles in as a whole: `--on-segment-starts`.
    let mut settle = recipes::SettleIn::with(SETTLE_DISTANCE_EM);
    settle.distance_em = ov.distance_or(settle.distance_em);
    settle.duration_s = ov.duration_or(settle.duration_s);
    let (opacity, ty_em) = if ov.motion_static {
        (1.0, 0.0)
    } else {
        settle.eval((now_ms - segment.start_ms) / 1000.0)
    };
    let ty = ty_em * fs;

    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" transform=\"translate(0,{ty})\" opacity=\"{opacity}\" \
         filter=\"url(#cleo-shadow)\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            let text = layout::escape_xml(&word.display);
            // Tag `.quote` tscaps: kata ber-tanda-kutip seluruhnya warna quote.
            let fill = if style::word_has_quote(&word.display) {
                st.color("quote-color", "#e6d647")
            } else {
                ov.primary.clone()
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr} \
                 stroke=\"{outline_color}\" stroke-width=\"{stroke_px}\" \
                 paint-order=\"stroke\">{text}</text>",
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the block about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    // tscaps cleo memakai text-shadow multi-layer (CSS): tiap layer
    // INDEPENDEN di atas SourceAlpha lalu digabung — bukan feDropShadow
    // berantai (yang membuat bayangan menumpuk → awan gelap). Merge dari
    // layer bawah ke atas, SourceGraphic paling atas.
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="cleo-shadow" x="-150%" y="-250%" width="400%" height="600%">
                    <feOffset in="SourceAlpha" dx={0.0} dy={0.0} result="t-o"/>
                    <feGaussianBlur in="t-o" stdDeviation={tight_std} result="t-b"/>
                    <feFlood flood-color={SHADOW_COLOR} flood-opacity={1.0} result="t-f"/>
                    <feComposite in="t-f" in2="t-b" operator="in" result="t"/>
                    <feOffset in="SourceAlpha" dx={0.0} dy={soft_dy} result="l1-o"/>
                    <feGaussianBlur in="l1-o" stdDeviation={soft_std} result="l1-b"/>
                    <feFlood flood-color={SHADOW_COLOR} flood-opacity={1.0} result="l1-f"/>
                    <feComposite in="l1-f" in2="l1-b" operator="in" result="l1"/>
                    <feOffset in="SourceAlpha" dx={0.0} dy={soft_dy} result="l2-o"/>
                    <feGaussianBlur in="l2-o" stdDeviation={soft_std} result="l2-b"/>
                    <feFlood flood-color={SHADOW_COLOR} flood-opacity={1.0} result="l2-f"/>
                    <feComposite in="l2-f" in2="l2-b" operator="in" result="l2"/>
                    <feOffset in="SourceAlpha" dx={0.0} dy={soft_dy} result="l3-o"/>
                    <feGaussianBlur in="l3-o" stdDeviation={soft_std} result="l3-b"/>
                    <feFlood flood-color={SHADOW_COLOR} flood-opacity={1.0} result="l3-f"/>
                    <feComposite in="l3-f" in2="l3-b" operator="in" result="l3"/>
                    <feMerge>
                        <feMergeNode in="l3"/>
                        <feMergeNode in="l2"/>
                        <feMergeNode in="l1"/>
                        <feMergeNode in="t"/>
                        <feMergeNode in="SourceGraphic"/>
                    </feMerge>
                </filter>
            </defs>
            {words_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_lora_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(LORA_TTF, 0).unwrap();
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1) — this is the
        // string `resolve_font` matches, and the TTF ships ID 1 as
        // "Lora Medium".
        let mut name = None;
        for n in face.names() {
            if n.name_id == 16 {
                name = n.to_string();
                break;
            }
        }
        if name.is_none() {
            for n in face.names() {
                if n.name_id == 1 {
                    name = n.to_string();
                    break;
                }
            }
        }
        assert_eq!(name.as_deref(), Some(FONT));
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn short_captions_grow_and_long_ones_stay_put() {
        // Twelve characters is the threshold: at or above, scale is 1.
        assert_eq!(font_size_scale(12), 1.0);
        assert_eq!(font_size_scale(30), 1.0);
        // Below it every missing character adds 6%: 5 chars → 1.42.
        assert!((font_size_scale(5) - 1.42).abs() < 1e-6);
        assert!((font_size_scale(2) - 1.6).abs() < 1e-6);
    }

    #[test]
    fn char_count_is_the_raw_text_joined_by_spaces() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Hi ok.".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert_eq!(segments.len(), 1);
        // "Hi ok." = 6 UTF-16 units.
        assert_eq!(segment_char_count(&segments[0].words), 6);
        // …so the segment renders 36% larger than the 4.3cqh base.
        assert!((font_size_scale(6) - 1.36).abs() < 1e-6);
    }

    #[test]
    fn smart_lowercase_lowers_but_keeps_the_pronoun_i() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Hello THERE I am fine.".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let displays: Vec<&str> = segments
            .iter()
            .flat_map(|s| s.words.iter().map(|w| w.display.as_str()))
            .collect();
        assert_eq!(displays, vec!["hello", "there", "I", "am", "fine."]);
    }
}
