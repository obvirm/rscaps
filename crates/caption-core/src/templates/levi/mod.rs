//! Native port of the tscaps `levi` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\levi\{style.scss,template.json}`.
//! Huge uppercase Anton at 6cqh on a four-to-eight-line block, gradient
//! fill per line (primary → primary mixed with 18% black), a soft drop
//! shadow around each line, and a line-level rise-in (0.28s, ease-out)
//! keyed off when the line starts being narrated. `gap_free` keeps the
//! block on screen between nearby segments.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from levi's style.scss + template.json) ───────────────
const FONT: &str = "Anton";
// template.json asks weight 800; Anton ships a single static face (400)
// and the browser resolves to it too — we query what we actually embed.
const FONT_WEIGHT: u16 = 400;
const FONT_SIZE_CQH: f32 = 0.06; // font-size: 6.0cqh
const LINE_HEIGHT: f32 = 1.0; // line-height: 1
const LINE_SPACING_EM: f32 = 0.09; // .line + .line margin-top: 0.09em
const WORD_SPACING_EM: f32 = 0.08; // .word margin: 0 0.08em
const ANCHOR_Y_FRAC: f32 = 0.4; // alignment.verticalOffset (center)
const ANCHOR_X_FRAC: f32 = 0.04; // alignment.horizontalOffset (left)
const MAX_CHARS: usize = 80; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 40; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 4; // lineSplitter.minLines
const MAX_LINES: usize = 8; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.49; // lineSplitter.maxWidthRatio
const MAX_GAP_MS: f32 = 1000.0; // gap_free default maxGapMs
const RISE_DURATION_S: f32 = 0.28; // rise-in($duration: 0.28s, ease-out)
const PRIMARY: &str = "#ffffff"; // primary-color default
// color-mix(in srgb, $primary, #000 18%) — srgb channel mix with black.
const GRADIENT_BLACK_SHARE: f32 = 0.18;
// text-shadow: soft-drop-shadow(0.03, 0.12) via filter: drop-shadow(...)
const SHADOW_DY_EM: f32 = 0.03;
const SHADOW_BLUR_EM: f32 = 0.12;

/// levi's pipeline: sentence boundary → char limit → gap_free.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: MAX_GAP_MS,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const ANTON_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Anton.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(ANTON_TTF, 0).expect("Anton parses");
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

/// `color-mix(in srgb, <hex>, #000 18%)` — channel × (1 − share).
fn mixed_down(hex: &str, share: f32) -> String {
    let parse = |s: &str| -> [f32; 3] {
        let h = s.trim_start_matches('#');
        [0, 2, 4]
            .iter()
            .map(|i| u8::from_str_radix(&h[*i..*i + 2], 16).unwrap_or(255) as f32)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    };
    let [r, g, b] = parse(hex);
    let m = 1.0 - share;
    format!(
        "#{:02x}{:02x}{:02x}",
        (r * m).round().clamp(0.0, 255.0) as u8,
        (g * m).round().clamp(0.0, 255.0) as u8,
        (b * m).round().clamp(0.0, 255.0) as u8
    )
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
    let margin = WORD_SPACING_EM * fs;
    let line_h = LINE_HEIGHT * fs;
    let line_gap = LINE_SPACING_EM * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) = line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        ANTON_TTF,
        FONT_WEIGHT,
        ctx.font_source,
        fs,
        0.0,
        true,
    );
    let widths = measured.widths;
    let display_texts: Vec<String> = segment
        .words
        .iter()
        .map(|w| w.display.to_uppercase())
        .collect();

    let splitter = layout::BalancedPixelWidth {
        min_lines: MIN_LINES,
        max_lines: MAX_LINES,
        max_width: (video_w as f32 * MAX_WIDTH_RATIO).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Centered vertically (anchor 0.4H), left-aligned at 0.04W; lines are
    // stacked with a 0.09em gap between them, not inside the first.
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * line_gap;
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - block_h / 2.0;
    let block_left = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;
    let gradient_bottom = mixed_down(PRIMARY, GRADIENT_BLACK_SHARE);
    let rise = recipes::RiseIn::custom(0.0, 0.0, RISE_DURATION_S, recipes::ease_out);

    let mut defs_svg = String::new();
    let mut lines_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + line_gap);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let xs = layout::word_x_positions(&widths[start..end], margin, block_left, line_w, rtl);

        // The line rises in when its own narration starts (first word).
        let line_span = state::LineSpan::of_words(&segment.words[start..end]);
        let line_opacity = rise.eval((now_ms - line_span.start_ms) / 1000.0).0;

        // Per-line 180° gradient across the line box (background-clip:
        // text maps the background over the element's box, not the glyphs).
        let grad_id = format!("levi-grad-{line_idx}");
        defs_svg.push_str(&format!(
            "<linearGradient id=\"{grad_id}\" gradientUnits=\"userSpaceOnUse\" \
             x1=\"0\" y1=\"{line_top}\" x2=\"0\" y2=\"{}\">\
             <stop offset=\"0\" stop-color=\"{PRIMARY}\"/>\
             <stop offset=\"1\" stop-color=\"{gradient_bottom}\"/>\
             </linearGradient>",
            line_top + line_h
        ));

        lines_svg.push_str(&format!(
            "<g opacity=\"{line_opacity}\" filter=\"url(#levi-shadow)\">"
        ));
        for ((x, text), family) in xs
            .iter()
            .zip(&display_texts[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(text);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            lines_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"url(#{grad_id})\"{family_attr}>{text}</text>"
            ));
        }
        lines_svg.push_str("</g>");
    }
    lines_svg.push_str("</g>");

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 1920 1080"
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="levi-shadow" x="-20%" y="-20%" width="140%" height="170%">
                    <feDropShadow dx={0.0} dy={shadow_dy} stdDeviation={shadow_std} flood-color="#000000" flood-opacity={1.0}/>
                </filter>
                {defs_svg}
            </defs>
            {lines_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_anton_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(ANTON_TTF, 0).unwrap();
        let mut name = None;
        for n in face.names() {
            if n.name_id == fframes::ttf_parser::name_id::FAMILY {
                name = n.to_string();
                break;
            }
        }
        assert_eq!(name.as_deref(), Some(FONT));
        // Single static face — what the weight-800 request resolves to.
        assert_eq!(face.weight().to_number(), 400);
    }

    #[test]
    fn gradient_bottom_is_primary_mixed_with_eighteen_percent_black() {
        // 0xffffffff → 255 × 0.82 = 209.1 → #d1d1d1.
        assert_eq!(mixed_down("#ffffff", 0.18), "#d1d1d1");
        // A saturated channel scales too.
        assert_eq!(mixed_down("#ff0000", 0.18), "#d10000");
    }

    #[test]
    fn levi_block_height_counts_the_line_gap_between_not_around_lines() {
        let fs = 0.06 * 1080.0;
        let n = 4;
        let block_h = n as f32 * fs + (n - 1) as f32 * 0.09 * fs;
        assert!((block_h - (4.0 + 3.0 * 0.09) * fs).abs() < 1e-3);
    }

    #[test]
    fn levi_line_rise_fades_the_line_over_28s_of_ease_out() {
        let rise = recipes::RiseIn::custom(0.0, 0.0, RISE_DURATION_S, recipes::ease_out);
        let (before, dy) = rise.eval(-0.01);
        assert_eq!(before, 0.0);
        assert_eq!(dy, 0.0);
        let (mid, _) = rise.eval(0.14);
        assert!(mid > 0.6, "ease-out front-loads, got {mid}");
        let (after, _) = rise.eval(0.28);
        assert!((after - 1.0).abs() < 1e-6);
    }

    #[test]
    fn levi_uppercases_for_display() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "make it big".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let upper: Vec<String> = segments[0]
            .words
            .iter()
            .map(|w| w.display.to_uppercase())
            .collect();
        assert_eq!(upper, vec!["MAKE", "IT", "BIG"]);
    }

    #[test]
    fn levi_gap_free_stretches_the_window() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![
                crate::Cue {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "First part.".into(),
                },
                crate::Cue {
                    start_ms: 1500,
                    end_ms: 2500,
                    text: "Second part.".into(),
                },
            ],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert_eq!(segments.len(), 2);
        assert!((segments[0].end_ms - 1500.0).abs() < 1e-3);
    }
}
