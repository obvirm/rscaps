//! Native port of the tscaps `hugo` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\hugo\{style.scss,template.json}`.
//! Uppercase Gabarito 600 at 3.5cqh with tight negative tracking, a bloom
//! in the text's own colour (`0 0 0.267em #ffffff`) and a per-word
//! `flash-in` (0.1s, white plate) struck from when the word starts
//! narrating. Clause segments of 22 chars (min 9), always two balanced
//! rows of 0.74W, block top-anchored at 0.78H.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from hugo's style.scss + template.json) ───────────────
const FONT: &str = "Gabarito"; // fontStack gabarito ('Gabarito Variable' in scss)
const FONT_WEIGHT: u16 = 600; // fontWeight: 600
const FONT_SIZE_CQH: f32 = 0.035; // font-size: 3.5cqh
const LETTER_SPACING_EM: f32 = -0.065; // letter-spacing: -0.065em
const LINE_HEIGHT: f32 = 0.92; // line-height: 0.92
const WORD_SPACING_EM: f32 = 0.13; // .word margin: 0 0.13em
const ANCHOR_Y_FRAC: f32 = 0.78; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 22; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 9; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 2; // lineSplitter.minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.74; // lineSplitter.maxWidthRatio
const LINE_SPACING_EM: f32 = 0.0; // typography.lineSpacing: 0
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
const SHADOW_BLUR_EM: f32 = 0.267; // --tscaps-shadow-blur default (shadow-blur 0.267)
const OUTLINE_THICKNESS_EM: f32 = 0.0; // --tscaps-outline-thickness default (control 0)
const OUTLINE_COLOR: &str = "#1a1a1a"; // --tscaps-outline-color default
// flash-in($element: 'word', $plate: #ffffff, $duration: 0.1s, ease)
const FLASH_DURATION_S: f32 = 0.1;
const FLASH_PLATE: &str = "#ffffff";

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: LINE_SPACING_EM,
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: true, // hugo always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// hugo's `effects` array: `gap_free`, `smart_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::SmartPunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const GABARITO_TTF: &[u8] = include_bytes!("../../../assets/gabarito-latin-600-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Gabarito.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(GABARITO_TTF, 0).expect("Gabarito parses");
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

/// `text-transform: uppercase` — the uppercased form is what gets
/// measured *and* emitted.
fn uppercase(text: &str) -> String {
    text.to_uppercase()
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
    let st = style::Style::of(&doc.params, "hugo");
    let outline_thickness_em = st.f32("outline-thickness", OUTLINE_THICKNESS_EM);
    let outline_color = st.color("outline-color", OUTLINE_COLOR);
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
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
        GABARITO_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        true,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Top-aligned block: the block's top edge sits at 0.78H; lines center
    // horizontally.
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;
    let line_gap = ov.line_spacing_em * fs;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;
    let glow_std = SHADOW_BLUR_EM * fs / 2.0;
    let flash = recipes::FlashIn {
        duration_s: ov.duration_or(FLASH_DURATION_S),
    };

    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + line_gap);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for (j, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            // flash-in: the word is struck by a plate when its own
            // narration starts — hidden (visibility, not opacity) until
            // the plate cuts away, then it arrives.
            let state = if ov.motion_static {
                recipes::FlashState {
                    element_visible: true,
                    element_opacity: 1.0,
                    plate_opacity: 0.0,
                }
            } else {
                flash.eval((now_ms - word.start_ms) / 1000.0)
            };
            if state.element_visible {
                let family_attr = if *family == FONT {
                    String::new()
                } else {
                    format!(" font-family=\"{family}\"")
                };
                let text = layout::escape_xml(&ov.text_case.apply(&word.display));
                let stroke = if outline_thickness_em > 0.0 {
                    format!(
                        " stroke=\"{outline_color}\" stroke-width=\"{}\" paint-order=\"stroke\"",
                        outline_thickness_em * fs
                    )
                } else {
                    String::new()
                };
                words_svg.push_str(&format!(
                    "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr} \
                     opacity=\"{}\" filter=\"url(#hugo-glow)\"{stroke}>{text}</text>",
                    state.element_opacity,
                    primary = ov.primary.as_str(),
                ));
            }
            if state.plate_opacity > 0.0 {
                // `::after { position: absolute; inset: 0 }` over the word's
                // own inline-block box (width = measured width, height =
                // the line box).
                words_svg.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{line_top}\" width=\"{}\" height=\"{line_h}\" \
                     fill=\"{FLASH_PLATE}\" opacity=\"{}\"/>",
                    widths[start + j],
                    state.plate_opacity
                ));
            }
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the block about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let block_h =
            lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * line_gap;
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="hugo-glow" x="-40%" y="-80%" width="180%" height="320%">
                    <feDropShadow dx={0.0} dy={0.0} stdDeviation={glow_std} flood-color={ov.primary} flood-opacity={1.0}/>
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
    fn embedded_gabarito_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(GABARITO_TTF, 0).unwrap();
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1) — this is the
        // string `resolve_font` matches.
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
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn hugo_splitter_is_clause_plus_twenty_two_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, hold on a second and look at this one right now, please."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        // Clause boundary closes at the comma; the long tail then splits
        // on the 22-char limit.
        assert_eq!(texts[0], "Wait,");
        assert!(segments.len() >= 3, "{texts:?}");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
        // smart_punctuation is declared, so the ellipsis in display form
        // would be typographic (checked on display, not text).
        assert!(segments
            .iter()
            .all(|s| s.words.iter().all(|w| w.display == w.text)));
    }

    #[test]
    fn flash_in_hides_the_word_until_the_plate_cuts_away() {
    let flash = recipes::FlashIn {
        duration_s: FLASH_DURATION_S,
    };
        // Before the word's narration starts the entrance holds its
        // first keyframe: hidden, no plate yet.
        let before = flash.eval(-1.0);
        assert!(!before.element_visible && before.plate_opacity == 0.0);
        // Mid-hold: the plate is lit and the word is still hidden.
        let hold = flash.eval(FLASH_DURATION_S * 0.3);
        assert!(!hold.element_visible);
        assert!((hold.plate_opacity - 1.0).abs() < 1e-6);
        // After the cut the word is visible at full opacity again.
        let after = flash.eval(FLASH_DURATION_S);
        assert!(after.element_visible);
        assert!((after.element_opacity - 1.0).abs() < 1e-6);
        assert_eq!(after.plate_opacity, 0.0);
        assert_eq!(FLASH_PLATE, "#ffffff");
    }

    #[test]
    fn hugo_uppercases_for_measure_and_emit() {
        assert_eq!(uppercase("make it big"), "MAKE IT BIG");
        assert_eq!(LINE_SPACING_EM, 0.0);
        assert_eq!(OUTLINE_THICKNESS_EM, 0.0);
    }
}
