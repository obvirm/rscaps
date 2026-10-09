//! Native port of the tscaps `nova` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\nova\{style.scss,template.json}`.
//! Poppins 700 at 3.2cqh, two lines pinned to 0.74H: the first renders
//! 1.5× larger in `#ffea00`, the second at the base size in white tucked
//! up under it by -0.25em. Each line rises into place on its own
//! narration start through `tscaps-nova-rise-1` / `-rise-2` (0.25s /
//! 0.30s, `cubic-bezier(0, 0.85, 0.4, 1)`): opacity snaps at 15%, while
//! the lift and a 0.25em blur settle over the full duration.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from nova's style.scss + template.json) ───────────────
const FONT: &str = "Poppins"; // fontStack poppins
const FONT_WEIGHT: u16 = 700; // typography.fontWeight: 700
const FONT_SIZE_CQH: f32 = 0.032; // font-size: 3.2cqh
const LETTER_SPACING_EM: f32 = -0.02; // letter-spacing: -0.02em
const LINE_HEIGHT: f32 = 1.05; // line-height: 1.05
const WORD_SPACING_EM: f32 = 0.12; // .word margin: 0 0.12em
const LINE_SPACING_EM: f32 = -0.25; // .line + .line margin-top: -0.25em
const FIRST_LINE_SCALE: f32 = 1.5; // .line:first-child font-size × 1.5
const ANCHOR_Y_FRAC: f32 = 0.74; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 30; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 6; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 2; // lineSplitter.minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.75; // lineSplitter.maxWidthRatio
const PRIMARY: &str = "#ffffff"; // primary-color default (`.line + .line`)
const HIGHLIGHT: &str = "#ffea00"; // highlight-color default (`.line:first-child`)
// .word text-shadow: soft-drop-shadow(0.04, 0.25) — `0 0.04em 0.25em #000`.
const SHADOW_DY_EM: f32 = 0.04;
const SHADOW_BLUR_EM: f32 = 0.25;
const RISE_DISTANCE_EM: f32 = 0.75; // --tscaps-rise-distance default
const RISE_DURATION_S: f32 = 0.25; // --tscaps-rise-duration default
const RISE2_DURATION_S: f32 = 0.30; // rise-duration + 0.05s
const RISE2_DISTANCE_EM: f32 = 1.125; // rise-distance × 1.5
const RISE_BLUR_EM: f32 = 0.25; // keyframes filter: blur(0.25em)
const OPACITY_HOLD: f32 = 0.15; // keyframes 15% stop (opacity 1)

/// nova's `effects`: `gap_free` then `smart_punctuation`, both enabled.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::SmartPunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const POPPINS_700_TTF: &[u8] = include_bytes!("../../../assets/poppins-latin-700-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Poppins 700.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(POPPINS_700_TTF, 0).expect("Poppins parses");
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

// ── keyframe ports ────────────────────────────────────────────────────────

/// Shared multi-stop entry clamp: negative elapsed holds the first stop
/// (fill `both`), past the duration holds the last.
fn progress(elapsed_s: f32, duration_s: f32) -> f32 {
    if elapsed_s <= 0.0 {
        0.0
    } else if duration_s <= 0.0 || elapsed_s >= duration_s {
        1.0
    } else {
        elapsed_s / duration_s
    }
}

/// `tscaps-nova-rise-1` / `-rise-2`: opacity 0 → 1 over the 0-15% stop,
/// the translateY and the blur running 0% → 100% (the 15% stop declares
/// neither, so both interpolate straight across the full duration).
///
/// Returns `(opacity, translateY_em, blur_em)`; `em` resolves against the
/// line's own font-size.
fn nova_rise(
    elapsed_s: f32,
    duration_s: f32,
    distance_em: f32,
    blur_em: f32,
    ease: &impl Fn(f32) -> f32,
) -> (f32, f32, f32) {
    let p = progress(elapsed_s, duration_s);
    let opacity = if p < OPACITY_HOLD {
        ease(p / OPACITY_HOLD)
    } else {
        1.0
    };
    let settle = ease(p);
    (opacity, distance_em * (1.0 - settle), blur_em * (1.0 - settle))
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

    let video_w = ctx.current_video_size.width;
    let video_h = ctx.current_video_size.height;
    let fs = FONT_SIZE_CQH * video_h as f32;
    let ls_px = LETTER_SPACING_EM * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    // Split widths are measured at the segment size (a word is measured
    // before any line exists); the lines then re-measure at their own
    // font-size for placement.
    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        POPPINS_700_TTF,
        FONT_WEIGHT,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: MIN_LINES,
        max_lines: MAX_LINES,
        max_width: (video_w as f32 * MAX_WIDTH_RATIO).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * fs * WORD_SPACING_EM).collect();
    let lines = splitter.split(&split_widths);

    // Line boxes: line 0 at 1.5× the segment size, the rest at the base
    // size. The -0.25em gap resolves against each later line's own (base)
    // font-size, and the block is pinned by its bottom edge.
    let mut boxes: Vec<(f32, f32, f32)> = Vec::with_capacity(lines.len()); // (top, height, fs)
    let mut cursor = 0.0f32;
    for (i, _) in lines.iter().enumerate() {
        let line_fs = if i == 0 { fs * FIRST_LINE_SCALE } else { fs };
        if i > 0 {
            cursor += LINE_SPACING_EM * line_fs;
        }
        let height = LINE_HEIGHT * line_fs;
        boxes.push((cursor, height, line_fs));
        cursor += height;
    }
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - cursor;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ease = recipes::cubic_bezier(0.0, 0.85, 0.4, 1.0);

    // Both rises run off the line's own narration start, resolved against
    // that line's font-size; the results drive the line's transform,
    // opacity and the animated rise blur below.
    let mut rises: Vec<(f32, f32, f32)> = Vec::with_capacity(lines.len()); // (opacity, dy, blur_std)
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_fs = boxes[line_idx].2;
        let span = state::LineSpan::of_words(&segment.words[start..end]);
        let (duration, distance) = if line_idx == 0 {
            (RISE_DURATION_S, RISE_DISTANCE_EM)
        } else {
            (RISE2_DURATION_S, RISE2_DISTANCE_EM)
        };
        let (opacity, dy_em, blur_em) =
            nova_rise((now_ms - span.start_ms) / 1000.0, duration, distance, RISE_BLUR_EM, &ease);
        rises.push((opacity, dy_em * line_fs, blur_em * line_fs));
    }

    // Per-line filters: the word's soft drop shadow (em against that line's
    // font-size) and the rise blur, whose stdDeviation tracks the animated
    // blur — the filter drops out once the line has settled.
    let mut defs = String::new();
    for (i, &(_, _, line_fs)) in boxes.iter().enumerate() {
        let shadow_dy = SHADOW_DY_EM * line_fs;
        let shadow_std = SHADOW_BLUR_EM * line_fs / 2.0;
        let blur_std = rises[i].2;
        if blur_std > 0.01 {
            defs.push_str(&format!(
                "<filter id=\"nova-blur-{i}\" x=\"-50%\" y=\"-100%\" width=\"200%\" height=\"300%\">\
                 <feGaussianBlur stdDeviation=\"{blur_std}\"/></filter>"
            ));
        }
        defs.push_str(&format!(
            "<filter id=\"nova-shadow-{i}\" x=\"-30%\" y=\"-60%\" width=\"160%\" height=\"260%\">\
             <feDropShadow dx=\"0\" dy=\"{shadow_dy}\" stdDeviation=\"{shadow_std}\" \
             flood-color=\"#000000\" flood-opacity=\"1.0\"/></filter>"
        ));
    }

    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let (top, line_h, line_fs) = boxes[line_idx];
        let line_top = block_top + top;
        let ascent = asc_ratio * line_fs;
        let descent = desc_ratio * line_fs;
        let baseline = line_top + (line_h - (ascent + descent)) / 2.0 + ascent;

        let margin = WORD_SPACING_EM * line_fs;
        let line_words = &segment.words[start..end];
        let line_widths = stack::measure_words(
            line_words,
            &script_stack,
            POPPINS_700_TTF,
            FONT_WEIGHT,
            ctx.font_source,
            line_fs,
            ls_px,
            false,
        )
        .widths;
        let line_split: Vec<f32> = line_widths.iter().map(|w| w + 2.0 * margin).collect();
        let line_w: f32 = line_split.iter().sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&line_widths, margin, line_left, line_w, rtl);

        let (opacity, dy, blur_std) = rises[line_idx];

        // First line takes the highlight, later lines the primary colour.
        let fill = if line_idx == 0 { HIGHLIGHT } else { PRIMARY };

        if blur_std > 0.01 {
            words_svg.push_str(&format!(
                "<g font-size=\"{line_fs}\" opacity=\"{opacity}\" transform=\"translate(0,{dy})\" \
                 filter=\"url(#nova-blur-{line_idx})\">"
            ));
        } else {
            words_svg.push_str(&format!(
                "<g font-size=\"{line_fs}\" opacity=\"{opacity}\" transform=\"translate(0,{dy})\">"
            ));
        }
        for ((x, word), family) in xs
            .iter()
            .zip(line_words)
            .zip(&measured.families[start..end])
        {
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            let text = layout::escape_xml(&word.display);
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr} \
                 filter=\"url(#nova-shadow-{line_idx})\">{text}</text>"
            ));
        }
        words_svg.push_str("</g>");
    }
    words_svg.push_str("</g>");

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 1920 1080"
            width={video_w}
            height={video_h}
        >
            <defs>
                {defs}
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
    fn embedded_poppins_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(POPPINS_700_TTF, 0).unwrap();
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
    fn rise_snaps_the_opacity_at_fifteen_percent_but_settles_slowly() {
        let ease = recipes::cubic_bezier(0.0, 0.85, 0.4, 1.0);
        // Start (fill both): hidden, full distance down, full blur.
        let (op0, dy0, blur0) = nova_rise(-0.2, 0.25, RISE_DISTANCE_EM, RISE_BLUR_EM, &ease);
        assert_eq!(op0, 0.0);
        assert_eq!(dy0, RISE_DISTANCE_EM);
        assert_eq!(blur0, RISE_BLUR_EM);
        // Opacity is lit well before the motion finishes.
        let (op1, dy1, _) = nova_rise(0.25 * 0.15, 0.25, RISE_DISTANCE_EM, RISE_BLUR_EM, &ease);
        assert!((op1 - 1.0).abs() < 1e-6, "opacity {op1}");
        assert!(dy1 > 0.0, "still lifting, dy {dy1}");
        // Resting state at the end of the duration.
        let (op2, dy2, blur2) = nova_rise(1.0, 0.25, RISE_DISTANCE_EM, RISE_BLUR_EM, &ease);
        assert_eq!((op2, dy2, blur2), (1.0, 0.0, 0.0));
        // The second line starts 1.5× further down than the first.
        let (_, far, _) = nova_rise(-0.1, RISE2_DURATION_S, RISE2_DISTANCE_EM, RISE_BLUR_EM, &ease);
        assert_eq!(far, RISE2_DISTANCE_EM);
        assert_eq!(RISE2_DISTANCE_EM, RISE_DISTANCE_EM * FIRST_LINE_SCALE);
    }

    #[test]
    fn nova_forces_two_lines_and_clause_splits() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "wait, hold on and look at this one here now.".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        // Clause mode: the comma closes the first segment.
        assert!(segments[0]
            .words
            .iter()
            .any(|w| w.text.ends_with(',')));
        // minLines 2 is a hard floor for any segment with two words.
        let splitter = layout::BalancedPixelWidth {
            min_lines: MIN_LINES,
            max_lines: MAX_LINES,
            max_width: 1920.0 * MAX_WIDTH_RATIO,
        };
        assert_eq!(splitter.split(&[400.0, 400.0]).len(), 2);
    }
}
