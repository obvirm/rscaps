//! Native port of the tscaps `rhea` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\rhea\{style.scss,template.json}`.
//! Montserrat: the spoken line wears the heavy 700 while the lines
//! around it rest at 300 (the "other lines weight" control), so every
//! line re-measures and re-centres when the speaker reaches it —
//! words never change width on their own. Lines balance across at most
//! two rows of 0.4W (split measured at the heaviest weight, per the
//! spec comment); the segment scales in from 0.9 over 0.1s with
//! `cubic-bezier(0.215, 0.61, 0.355, 1)`, words carry a soft drop
//! shadow (`0 0.03em 0.12em`), block bottom-anchored at 0.72H.
//!
//! Family note: both embedded Montserrat TTFs carry the name-table
//! family "Montserrat Thin" (fontsource packaging) — that is the
//! string fontdb resolves (`FONTDB_FAMILY`, used to measure), while
//! the SVG emits `FONT` so the studio's @font-face ("Montserrat")
//! matches in the browser.

use fframes::{FFramesContext, Svgr};

use super::{layout, overrides, pipeline, recipes, stack, state, style};
use crate::CaptionDocument;

// ── spec constants (from rhea's style.scss + template.json) ───────────────
const FONT: &str = "Montserrat"; // fontStack montserrat (browser @font-face)
const FONT_WEIGHT: u16 = 700; // fontWeight: 700 — the spoken line
const RESTING_WEIGHT: u16 = 300; // --tscaps-resting-line-weight default
const FONT_SIZE_CQH: f32 = 0.036; // font-size: 3.6cqh
const LETTER_SPACING_EM: f32 = -0.03; // letter-spacing: -0.03em
const LINE_HEIGHT: f32 = 1.1; // line-height: 1.1
const WORD_SPACING_EM: f32 = 0.09; // .word margin: 0 0.09em
const ANCHOR_Y_FRAC: f32 = 0.72; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 36; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 12; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.4; // lineSplitter.maxWidthRatio
const SCALE_DURATION_S: f32 = 0.1; // scale-in($duration: 0.1s)
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
// text-shadow: 0 0.03em 0.12em var(--tscaps-shadow-color, #000000)
const SHADOW_DY_EM: f32 = 0.03;
const SHADOW_BLUR_EM: f32 = 0.12;
const SHADOW_COLOR: &str = "#000000"; // --tscaps-shadow-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // no LINE_SPACING_EM in rhea's spec
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

/// rhea's `effects` array: none declared in template.json.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const MONTSERRAT_700_TTF: &[u8] = include_bytes!("../../../assets/montserrat-latin-700-normal.ttf");
const MONTSERRAT_300_TTF: &[u8] = include_bytes!("../../../assets/montserrat-latin-300-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the
/// embedded Montserrat 700 (the line-splitting weight).
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(MONTSERRAT_700_TTF, 0).expect("Montserrat parses");
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

/// The spoken line carries the segment's 700; every other line rests
/// at the control's weight (`.line-not-narrated-yet` /
/// `.line-already-narrated`).
fn line_weight(narration: state::Narration, resting: u16) -> u16 {
    match narration {
        state::Narration::Being => FONT_WEIGHT,
        _ => resting,
    }
}

/// `cubic-bezier(0.215, 0.61, 0.355, 1)` — rhea's scale-in easing.
fn ease_rhea(x: f32) -> f32 {
    recipes::cubic_bezier(0.215, 0.61, 0.355, 1.0)(x)
}

/// `scale-in($element: 'segment', $from: 0.9, $from-opacity: 0,
/// $duration: 0.1s, $easing: cubic-bezier(0.215, 0.61, 0.355, 1))`.
fn scale_in() -> recipes::ScaleIn {
    recipes::ScaleIn {
        from_scale: 0.9,
        from_opacity: 0.0,
        peak: None,
        peak_at: 0.6,
        duration_s: SCALE_DURATION_S,
        ease: ease_rhea,
    }
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
    let st = style::Style::of(&doc.params, "rhea");
    // The spoken line carries the 700; every other line rests at this
    // control's weight (the `resting-line-weight` default is 300).
    let resting_weight = st.f32("resting-line-weight", RESTING_WEIGHT as f32) as u16;
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    // The line splitter measures every line at the width it can reach —
    // the segment's heaviest weight (the spec comment's contract).
    let measured_heavy = stack::measure_words(
        &segment.words,
        &script_stack,
        MONTSERRAT_700_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths_heavy: Vec<f32> = measured_heavy.widths;
    // The light widths are what the resting lines actually paint.
    let measured_light = stack::measure_words(
        &segment.words,
        &script_stack,
        MONTSERRAT_300_TTF,
        resting_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths_light: Vec<f32> = measured_light.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths_heavy.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Bottom-aligned block: the block's bottom edge sits at 0.72H;
    // lineSpacing defaults to 0. Lines center horizontally.
    let block_h = lines.len() as f32 * line_h;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let center_x = ov.anchor_x * video_w as f32;
    let block_cy = block_top + block_h / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;

    let elapsed_s = (now_ms - segment.start_ms) / 1000.0;
    let mut anim = scale_in();
    anim.duration_s = ov.duration_or(SCALE_DURATION_S);
    let (opacity, scale) = if ov.motion_static {
        (1.0, 1.0)
    } else {
        anim.eval(elapsed_s)
    };

    let mut body = format!(
        "<g transform=\"translate({center_x} {block_cy}) scale({scale}) \
         translate({} {})\" opacity=\"{opacity}\">",
        -center_x,
        -block_cy
    );
    let font_family_attr = stack::svg_font_family(&ov.font_family);
    body.push_str(&format!(
        "<g font-family=\"{font_family_attr}\" font-size=\"{fs}\" letter-spacing=\"{ls_px}\" \
         fill=\"{fill}\" filter=\"url(#rhea-shadow)\">",
        fill = ov.primary,
    ));
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let words = &segment.words[start..end];
        let weight = line_weight(state::LineSpan::of_words(words).narration_at(now_ms), resting_weight);
        // The spoken line wears the (possibly overridden) heavy weight.
        let emit_weight = if weight == FONT_WEIGHT {
            ov.font_weight
        } else {
            weight
        };
        let ws: &[f32] = if weight == FONT_WEIGHT {
            &widths_heavy[start..end]
        } else {
            &widths_light[start..end]
        };
        let run: f32 = ws.iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - run / 2.0;
        let xs = layout::word_x_positions(ws, margin, line_left, run, rtl);
        let families: &[&str] = if weight == FONT_WEIGHT {
            &measured_heavy.families[start..end]
        } else {
            &measured_light.families[start..end]
        };
        for ((x, word), family) in xs.iter().zip(words).zip(families) {
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{}\"", stack::svg_font_family(&family))
            };
            body.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" font-weight=\"{emit_weight}\"{family_attr}>{text}</text>"
            ));
        }
    }
    body.push_str("</g></g>");

    // User rotation wraps the caption around the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        body = format!(
            "<g transform=\"rotate({angle},{center_x},{block_cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = body,
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
                <filter id="rhea-shadow" x="-50%" y="-50%" width="200%" height="220%">
                    <feDropShadow dx={0.0} dy={shadow_dy} stdDeviation={shadow_std} flood-color={SHADOW_COLOR} flood-opacity={1.0}/>
                </filter>
            </defs>
            {body}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_montserrat_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(MONTSERRAT_700_TTF, 0).unwrap();
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1). This file ships
        // no ID 16, and its ID 1 is the fontsource packaging artifact
        // "Montserrat Thin" — the string resolve_font actually matches.
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
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn rhea_splitter_is_sentence_plus_thirty_six_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, this is fine. But then the whole idea shifts again, doesn't it now?"
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
        // Sentence boundary closes at "fine."; the long second sentence
        // then splits on the 36-char limit.
        assert_eq!(texts[0], "Wait, this is fine.");
        assert!(segments.len() >= 2, "{texts:?}");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars} ({s:?})");
        }
    }

    #[test]
    fn the_spoken_line_wears_700_and_the_rest_300() {
        assert_eq!(line_weight(state::Narration::Being, RESTING_WEIGHT), 700);
        assert_eq!(line_weight(state::Narration::NotYet, RESTING_WEIGHT), 300);
        assert_eq!(line_weight(state::Narration::Already, RESTING_WEIGHT), 300);
        assert_eq!(FONT_WEIGHT, 700);
        assert_eq!(RESTING_WEIGHT, 300);
    }

    #[test]
    fn segment_scales_in_over_a_tenth_of_a_second() {
        let anim = scale_in();
        let (o0, s0) = anim.eval(0.0);
        assert_eq!(o0, 0.0);
        assert!((s0 - 0.9).abs() < 1e-6, "scale {s0}");
        let (o1, s1) = anim.eval(SCALE_DURATION_S);
        assert_eq!(o1, 1.0);
        assert_eq!(s1, 1.0);
        let (_, smid) = anim.eval(SCALE_DURATION_S / 2.0);
        assert!(smid > 0.9 && smid < 1.0, "mid scale {smid}");
        assert!(ease_rhea(0.0).abs() < 1e-4);
        assert!((ease_rhea(1.0) - 1.0).abs() < 1e-4);
    }
}
