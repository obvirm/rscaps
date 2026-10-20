//! Native port of the tscaps `mira` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\mira\{style.scss,template.json}`.
//! Montserrat SemiBold at 3.9cqh that *grows for short captions*
//! (`dynamic-font-size` 0.15 under a twelve-character threshold); the whole
//! segment settles in (0.3s from 80% opacity, entering 0.08em from above),
//! every word wears a 0.16em black stroke under the fill
//! (`paint-order: stroke fill`) plus a soft drop shadow
//! (`0 0.08em 0.4em`). Clause-boundary segments with `gap_free`,
//! `remove_punctuation`, `carry_quotes`, `smart_punctuation` and
//! `smart_lowercase`; block bottom-anchored at 0.65H, one line.
//!
//! Unported: the `.quote` colour/italic rule and the
//! `.emphasis/.accent/.entity` weight rule key off word *tags*
//! (`semanticTags`), which the native `Cue` text does not carry.
//!
//! Family note: the embedded Montserrat 600 carries the name-table family
//! "Montserrat Thin" (fontsource packaging) — that is the string fontdb
//! resolves (`FONTDB_FAMILY`, used to measure), while the SVG emits `FONT`
//! so the studio's @font-face ("Montserrat") matches in the browser.

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from mira's style.scss + template.json) ───────────────
const FONT: &str = "Montserrat"; // fontStack montserrat (browser @font-face)
const FONT_WEIGHT: u16 = 600; // fontWeight: 600
// font-size: var(--tscaps-font-size, 3.0cqh) — the app always emits that
// var from template.json `typography.fontSize`, so 3.9cqh is what renders
// and the scss 3.0cqh is only the unbound fallback.
const FONT_SIZE_CQH: f32 = 0.039; // typography.fontSize: 3.9
const LETTER_SPACING_EM: f32 = 0.0; // typography.letterSpacing: 0
const LINE_HEIGHT: f32 = 1.2; // line-height: 1.2 (mixin default)
const WORD_SPACING_EM: f32 = 0.13; // .word margin: 0 0.13em
const ANCHOR_Y_FRAC: f32 = 0.65; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 18; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 7; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // lineSplitter.maxWidthRatio
// settle-in($element: 'segment', $distance: -0.08em, $from-opacity: 0.8,
// $delay: var(--on-segment-starts), $duration: 0.3s)
const SETTLE_DISTANCE_EM: f32 = -0.08;
const SETTLE_FROM_OPACITY: f32 = 0.8;
const SETTLE_DURATION_S: f32 = 0.3;
const DYN_THRESHOLD: f32 = 12.0; // dynamic-font-size toggle `valueOn` ('12')
const DYN_STRENGTH: f32 = 0.15; // dynamic-font-size(0.15)
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
// -webkit-text-stroke: 0.16em #000; paint-order: stroke fill;
// --tscaps-outline-thickness / --tscaps-outline-color defaults.
const OUTLINE_THICKNESS_EM: f32 = 0.16;
const OUTLINE_COLOR: &str = "#000000";
// text-shadow: soft-drop-shadow(0.08, 0.4)
const SHADOW_DY_EM: f32 = 0.08;
const SHADOW_BLUR_EM: f32 = 0.4;
const SHADOW_COLOR: &str = "#000000"; // --tscaps-shadow-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // no LINE_SPACING_EM in mira's spec
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: false,
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// mira's `effects` array: `gap_free`, `remove_punctuation`,
/// `carry_quotes`, `smart_punctuation`, `smart_lowercase`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[
        effects::Effect::GapFree,
        effects::Effect::RemovePunctuation,
        effects::Effect::CarryQuotes,
        effects::Effect::SmartPunctuation,
        effects::Effect::SmartLowercase,
    ],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const MONTSERRAT_600_TTF: &[u8] = include_bytes!("../../../assets/montserrat-latin-600-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Montserrat 600.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face =
            fframes::ttf_parser::Face::parse(MONTSERRAT_600_TTF, 0).expect("Montserrat parses");
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
/// grow, twelve characters and up render at the plain 3.9cqh.
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
    let st = style::Style::of(&doc.params, "mira");
    // Type, metrics and shadows all scale with the segment's length.
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
        MONTSERRAT_600_TTF,
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

    // Bottom-aligned block: the block's bottom edge sits at 0.65H; the
    // single line centers horizontally.
    let block_h = lines.len() as f32 * line_h;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // The segment settles in as a whole: `--on-segment-starts`.
    let settle = recipes::SettleIn {
        distance_em: ov.distance_or(SETTLE_DISTANCE_EM),
        from_opacity: SETTLE_FROM_OPACITY,
        duration_s: ov.duration_or(SETTLE_DURATION_S),
        ease: recipes::ease_settle,
    };
    let (opacity, ty_em) = if ov.motion_static {
        (1.0, 0.0)
    } else {
        settle.eval((now_ms - segment.start_ms) / 1000.0)
    };
    let ty = ty_em * fs;

    // Shadow geometry (px from em ratios).
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;
    let stroke_px = st.f32("outline-thickness", OUTLINE_THICKNESS_EM) * fs;
    let outline_color = st.color("outline-color", OUTLINE_COLOR);

    let font_family_attr = stack::svg_font_family(&ov.font_family);
    let mut words_svg = format!(
        "<g transform=\"translate(0,{ty})\" opacity=\"{opacity}\">\
         <g font-family=\"{font_family_attr}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" stroke=\"{outline_color}\" stroke-width=\"{stroke_px}\" \
         paint-order=\"stroke\" filter=\"url(#mira-shadow)\">",
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for (word_off, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{}\"", stack::svg_font_family(&family))
            };
            // Tag `.quote` tscaps: kata ber-kutip → warna quote (+ italic).
            let is_quote = style::word_has_quote(&word.display);
            let fill = if is_quote {
                st.color("quote-color", "#e6d647")
            } else {
                ov.primary.clone()
            };
            let italic_attr = if is_quote && st.bool("quote-italic", true) {
                " font-style=\"italic\""
            } else {
                ""
            };
            // Element inspector: override per-kata (attrs + transform).
            let wo_ctx = element_css::WordCtx {
                base_fs: fs,
                ls_px,
                video_w: video_w as f32,
                video_h: video_h as f32,
                center_x: *x + widths[start + word_off] / 2.0,
                center_y: baseline - fs * 0.35,
                word_start_s: word.start_ms / 1000.0,
                now_s: now_ms / 1000.0,
                font_family_svg: &stack::svg_font_family,
            };
            let wo = element_css::word_svg_attrs(&doc.params, word.index, &wo_ctx);
            let wo_t = element_css::word_svg_transform(&doc.params, word.index, &wo_ctx);
            let wo_transform = wo_t
                .map(|t| format!(" transform=\"{t}\""))
                .unwrap_or_default();
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}{italic_attr}\
                 {wo}{wo_transform}>{text}</text>",
            ));
        }
    }
    words_svg.push_str("</g></g>");

    // User rotation wraps the caption around the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    let words_svg = layout::dedupe_text_attrs(&words_svg);
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="mira-shadow" x="-30%" y="-60%" width="160%" height="260%">
                    <feDropShadow dx={0.0} dy={shadow_dy} stdDeviation={shadow_std} flood-color={SHADOW_COLOR} flood-opacity={1.0}/>
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
    fn embedded_montserrat_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(MONTSERRAT_600_TTF, 0).unwrap();
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1) — this is the
        // string `resolve_font` matches, and the TTF ships ID 16 as
        // "Montserrat Thin".
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
        assert_eq!(name.as_deref(), Some("Montserrat Thin"));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn short_segments_grow_and_long_ones_stay_put() {
        // Twelve characters is the threshold: at or above, scale is 1.
        assert_eq!(font_size_scale(12), 1.0);
        assert_eq!(font_size_scale(30), 1.0);
        // Below it every missing character adds 15%: 5 chars → 2.05.
        assert!((font_size_scale(5) - 2.05).abs() < 1e-6);
        assert!((font_size_scale(2) - 2.5).abs() < 1e-6);
    }

    #[test]
    fn char_count_is_the_raw_text_joined_by_spaces() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Hi ok.".into(),
                sheet: None,
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert_eq!(segments.len(), 1);
        // "Hi ok." = 6 UTF-16 units → 1.9× the 3.9cqh base.
        assert_eq!(segment_char_count(&segments[0].words), 6);
        assert!((font_size_scale(6) - 1.9).abs() < 1e-6);
    }

    #[test]
    fn settle_in_fades_in_by_the_midpoint_and_lands_at_rest() {
        let settle = recipes::SettleIn {
            distance_em: SETTLE_DISTANCE_EM,
            from_opacity: SETTLE_FROM_OPACITY,
            duration_s: SETTLE_DURATION_S,
            ease: recipes::ease_settle,
        };
        // Before the segment starts: the first keyframe holds.
        let (before, dy) = settle.eval(-0.01);
        assert_eq!(before, 0.8);
        assert!((dy + 0.08).abs() < 1e-6);
        // Halfway (0.15s) the text is fully readable, still travelling.
        let (mid, dy_mid) = settle.eval(0.15);
        assert!((mid - 1.0).abs() < 1e-6);
        assert!(dy_mid < -0.005, "still moving, got {dy_mid}");
        // At the end it rests at full opacity with no offset.
        let (done, dy_done) = settle.eval(0.3);
        assert!((done - 1.0).abs() < 1e-6);
        assert_eq!(dy_done, 0.0);
    }

    #[test]
    fn mira_clause_segments_split_strip_and_lowercase() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, WHAT I saw.".into(),
                sheet: None,
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.display.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        // Clause boundary closes the group at the comma; then
        // remove_punctuation drops the trailing run and smart_lowercase
        // lowers everything but the pronoun `I`.
        assert_eq!(texts, vec!["wait", "what I saw"]);
    }
}
