//! Native port of the tscaps `zara` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\zara\{style.scss,template.json}`.
//! Anton, uppercased, at 4.69cqh: cyan/magenta ghosts cast on a diagonal
//! (blurred once each, then hard on top) over four hard black offset
//! shadows and a hairline black stroke, the narrated word lighting up
//! `#fff200`. The segment arrives through `glitch-in` — displaced, kicked
//! to the opposite offset, then landing (`steps(2)`, 0.22s).

use fframes::{FFramesContext, Svgr};

use super::{layout, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from zara's style.scss + template.json) ───────────────
const FONT: &str = "Anton"; // fontStack anton
const FONT_WEIGHT: u16 = 400; // typography.fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.0469; // font-size: 4.69cqh
const LETTER_SPACING_EM: f32 = 0.025; // letter-spacing: 0.025em
const LINE_HEIGHT: f32 = 1.15; // line-height: 1.15
const WORD_SPACING_EM: f32 = 0.1; // .word margin: 0 0.1em
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top: 0
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center, mixin default)
const MAX_CHARS: usize = 32; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // ...minChars (descriptor default; absent in json)
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#ffffff"; // primary-color default
const HIGHLIGHT: &str = "#fff200"; // highlight-color default
const STROKE_EM: f32 = 0.01; // -webkit-text-stroke: 0.01em black
const STROKE_COLOR: &str = "#000000"; // ...the `black` in that rule
// The ghost split: one dial drives both axes so the diagonal stretches
// uniformly. Each colour is painted blurred underneath and hard on top.
const SPLIT_X_EM: f32 = 0.05; // --tscaps-split-x default
const GLOW_RADIUS_EM: f32 = 0.12; // --tscaps-glow-radius default
const SPLIT_COLOR_A: &str = "#ff00aa"; // split-color-a default
const SPLIT_COLOR_B: &str = "#00e5ff"; // split-color-b default
const HARD_XY_EM: f32 = 0.03; // the four black offsets: ±0.03em
const GLITCH_X_EM: f32 = 0.03; // glitch-in($x: 0.03em)
const GLITCH_Y_EM: f32 = -0.02; // glitch-in($y: -0.02em)

/// zara's `segmentSplitters`: boundary (sentence) + 32 chars; no
/// `effects` array, so windows stay the raw word span.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const ANTON_400_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Anton.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(ANTON_400_TTF, 0).expect("Anton parses");
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

    let video_w = ctx.current_video_size.width;
    let video_h = ctx.current_video_size.height;
    let fs = FONT_SIZE_CQH * video_h as f32;
    let ls_px = LETTER_SPACING_EM * fs;
    let margin = WORD_SPACING_EM * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        ANTON_400_TTF,
        FONT_WEIGHT,
        ctx.font_source,
        fs,
        ls_px,
        true,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: MIN_LINES,
        max_lines: MAX_LINES,
        max_width: (video_w as f32 * MAX_WIDTH_RATIO).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Top-aligned block: the block's top edge sits at 0.75H; lines center
    // horizontally, and the whole segment rides the glitch entrance.
    let block_top = ANCHOR_Y_FRAC * video_h as f32;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // `glitch-in` on `.segment`: `--on-segment-starts` delay, steps(2).
    let glitch = recipes::GlitchIn::with(GLITCH_X_EM, GLITCH_Y_EM);
    let (segment_opacity, tx_em, ty_em) = glitch.eval((now_ms - segment.start_ms) / 1000.0);
    let tx = tx_em * fs;
    let ty = ty_em * fs;

    // Eight shadow links: the two ghosts each land blurred and hard, then
    // four hard black offsets behind them. The first link is the innermost
    // shadow (the CSS list paints its first entry on top).
    let stroke_w = STROKE_EM * fs;
    let mut shadow_svg = String::new();
    let layers: [(f32, f32, f32, &str); 8] = [
        (SPLIT_X_EM, SPLIT_X_EM, GLOW_RADIUS_EM, SPLIT_COLOR_A),
        (-SPLIT_X_EM, -SPLIT_X_EM, GLOW_RADIUS_EM, SPLIT_COLOR_B),
        (SPLIT_X_EM, SPLIT_X_EM, 0.0, SPLIT_COLOR_A),
        (-SPLIT_X_EM, -SPLIT_X_EM, 0.0, SPLIT_COLOR_B),
        (-HARD_XY_EM, -HARD_XY_EM, 0.0, STROKE_COLOR),
        (HARD_XY_EM, -HARD_XY_EM, 0.0, STROKE_COLOR),
        (-HARD_XY_EM, HARD_XY_EM, 0.0, STROKE_COLOR),
        (HARD_XY_EM, HARD_XY_EM, 0.0, STROKE_COLOR),
    ];
    for (i, &(dx_em, dy_em, blur_em, color)) in layers.iter().enumerate() {
        let attr = if i == 0 {
            String::new()
        } else {
            format!(" in=\"s{i}\"")
        };
        shadow_svg.push_str(&format!(
            "<feDropShadow{attr} dx=\"{}\" dy=\"{}\" stdDeviation=\"{}\" \
             flood-color=\"{color}\" flood-opacity=\"1.0\" result=\"s{}\"/>",
            dx_em * fs,
            dy_em * fs,
            blur_em * fs / 2.0,
            i + 1
        ));
    }

    let mut words_svg = format!(
        "<g transform=\"translate({tx} {ty})\" opacity=\"{segment_opacity}\" \
         font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" stroke=\"{STROKE_COLOR}\" stroke-width=\"{stroke_w}\" \
         paint-order=\"stroke\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + LINE_SPACING_EM * fs);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let fill = if state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
                HIGHLIGHT
            } else {
                PRIMARY
            };
            let text = layout::escape_xml(&uppercase(&word.display));
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr} \
                 filter=\"url(#zara-ghost)\">{text}</text>"
            ));
        }
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
                <filter id="zara-ghost" x="-40%" y="-70%" width="180%" height="280%">
                    {shadow_svg}
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
    fn embedded_anton_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(ANTON_400_TTF, 0).unwrap();
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
    fn glitch_snaps_through_two_opposed_offsets_then_lands() {
        let glitch = recipes::GlitchIn::with(GLITCH_X_EM, GLITCH_Y_EM);
        // Before the segment starts (fill both) it holds the displaced
        // first stop: opacity 0 at (+x, +y).
        let (op0, tx0, ty0) = glitch.eval(-0.1);
        assert_eq!(op0, 0.0);
        assert_eq!(tx0, GLITCH_X_EM);
        assert_eq!(ty0, GLITCH_Y_EM);
        // Half way: hard cut to the opposite offset, fully lit.
        let (op1, tx1, ty1) = glitch.eval(0.11);
        assert_eq!(op1, 1.0);
        assert_eq!(tx1, -GLITCH_X_EM);
        assert_eq!(ty1, -GLITCH_Y_EM);
        // Resting state after 0.22s.
        let (op2, tx2, ty2) = glitch.eval(0.5);
        assert_eq!((op2, tx2, ty2), (1.0, 0.0, 0.0));
    }

    #[test]
    fn uppercase_is_measured_and_emitted() {
        assert_eq!(uppercase("already"), "ALREADY");
        assert_eq!(uppercase("MiXeD 42"), "MIXED 42");
        // The stroke and split geometry the ghost stack is built from.
        assert_eq!(STROKE_EM, 0.01);
        assert_eq!(SPLIT_X_EM, 0.05);
        assert_eq!(GLOW_RADIUS_EM, 0.12);
        assert_eq!(HARD_XY_EM, 0.03);
    }
}
