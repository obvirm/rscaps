//! Native port of the tscaps `lena` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\lena\{style.scss,template.json}`.
//! iMessage-style bubbles: Inter at 2.5cqh inside a blue `#2997ff` pill
//! per line (0.3em × 0.47em padding, 0.6em radius squared to 0.2em at the
//! bottom-left, soft `0 0.03em 0.07em` shade), a clawed tail on the last
//! bubble, and the whole bubble `rise-in`s (0.27em, 0.32s, overshooting
//! `cubic-bezier(0.34, 1.4, 0.5, 1)`) when its line starts narrating.
//! Sentence segments of 60 chars (min 30), three rows max of 0.5W, block
//! bottom-left anchored at 0.85H / 0.06W.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, state, style};
use crate::CaptionDocument;

// ── spec constants (from lena's style.scss + template.json) ───────────────
const FONT: &str = "Inter"; // fontStack inter ('Inter Variable' in scss)
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.025; // font-size: 2.5cqh
const LETTER_SPACING_EM: f32 = 0.0; // letterSpacing: 0
const LINE_HEIGHT: f32 = 1.3; // line-height: 1.3
const WORD_SPACING_EM: f32 = 0.13; // @include word-layout(0.13em)
const LINE_SPACING_EM: f32 = 0.2; // .segment gap: 0.2em
const ANCHOR_Y_FRAC: f32 = 0.85; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.06; // alignment.horizontalOffset (left)
const MAX_CHARS: usize = 60; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 30; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 3; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.5; // lineSplitter.maxWidthRatio
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default (.line)
const BG: &str = "#2997ff"; // --tscaps-bg-color default
const PAD_X_EM: f32 = 0.47; // --tscaps-bg-padding-x default
const PAD_Y_EM: f32 = 0.3; // --tscaps-bg-padding-y default
const RADIUS_EM: f32 = 0.6; // --tscaps-bg-radius default
const RADIUS_BL_EM: f32 = 0.2; // border-bottom-left-radius: 0.2em
// box-shadow: 0 0.03em 0.07em rgba(0, 0, 0, 0.16)
const SHADE_DY_EM: f32 = 0.03;
const SHADE_BLUR_EM: f32 = 0.07;
const SHADE_OPACITY: f32 = 0.16;
// rise-in($element: 'line', $distance: 0.27em, $from-opacity: 0,
//          $duration: 0.32s, $easing: cubic-bezier(0.34, 1.4, 0.5, 1))
const RISE_EM: f32 = 0.27;
const RISE_DURATION_S: f32 = 0.32;
// Tail: ::after at left -0.23em, bottom 0, 0.53em × 0.47em, clipped to
// polygon(100% 0%, 0% 0%, 25% 85%, 75% 100%, 87% 30%).
const TAIL_X_EM: f32 = -0.23;
const TAIL_W_EM: f32 = 0.53;
const TAIL_H_EM: f32 = 0.47;
const TAIL_POINTS: [(f32, f32); 5] = [
    (1.0, 0.0),
    (0.0, 0.0),
    (0.25, 0.85),
    (0.75, 1.0),
    (0.87, 0.3),
];

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
    force_uppercase: false, // lena keeps the spoken case
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// lena's `effects` array: `gap_free`, `emoji` (skipped — decorations
/// need data the word stream does not carry), `smart_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::SmartPunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const INTER_TTF: &[u8] = include_bytes!("../../../assets/inter-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Inter.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(INTER_TTF, 0).expect("Inter parses");
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

/// `rise-in` with lena's own overshooting curve (the mixin's default
/// `cubic-bezier(0.22, 1, 0.36, 1)` does not apply here).
fn ease_lena(x: f32) -> f32 {
    (recipes::cubic_bezier(0.34, 1.4, 0.5, 1.0))(x)
}

// ── bubble geometry ───────────────────────────────────────────────────────

/// One bubble as a path with per-corner radii in px
/// `(tl, tr, br, bl)` — bottom-left squared off.
fn bubble_path(x: f32, y: f32, w: f32, h: f32, tl: f32, tr: f32, br: f32, bl: f32) -> String {
    let clamp = |r: f32| r.clamp(0.0, w / 2.0).min(h / 2.0);
    let (tl, tr, br, bl) = (clamp(tl), clamp(tr), clamp(br), clamp(bl));
    let mut d = format!("M {} {y}", x + tl);
    d.push_str(&format!(" L {} {y}", x + w - tr));
    if tr > 0.0 {
        d.push_str(&format!(" A {tr} {tr} 0 0 1 {} {}", x + w, y + tr));
    }
    d.push_str(&format!(" L {} {}", x + w, y + h - br));
    if br > 0.0 {
        d.push_str(&format!(" A {br} {br} 0 0 1 {} {}", x + w - br, y + h));
    }
    d.push_str(&format!(" L {} {}", x + bl, y + h));
    if bl > 0.0 {
        d.push_str(&format!(" A {bl} {bl} 0 0 1 {x} {}", y + h - bl));
    }
    d.push_str(&format!(" L {x} {}", y + tl));
    if tl > 0.0 {
        d.push_str(&format!(" A {tl} {tl} 0 0 1 {} {y}", x + tl));
    }
    d.push_str(" Z");
    d
}

/// The tail polygon in absolute points, hanging off the bubble's
/// bottom-left corner (`left: -0.23em; bottom: 0`).
fn tail_points(box_left: f32, box_bottom: f32, fs: f32) -> String {
    let x = box_left + TAIL_X_EM * fs;
    let y = box_bottom - TAIL_H_EM * fs;
    let w = TAIL_W_EM * fs;
    let h = TAIL_H_EM * fs;
    TAIL_POINTS
        .iter()
        .map(|(px, py)| format!("{} {}", x + px * w, y + py * h))
        .collect::<Vec<_>>()
        .join(" ")
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
    let st = style::Style::of(&doc.params, "lena");
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let pad_x = st.f32("bg-padding-x", PAD_X_EM) * fs;
    let pad_y = st.f32("bg-padding-y", PAD_Y_EM) * fs;
    let box_h = line_h + 2.0 * pad_y;
    let gap = ov.line_spacing_em * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        INTER_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    let run_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let lines = splitter.split(&run_widths);

    // Bottom-left anchored stack: every bubble hangs off the frame's left
    // edge (0.06W), the stack's bottom edge sits at 0.85H.
    let block_h = lines.len() as f32 * box_h + lines.len().saturating_sub(1) as f32 * gap;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let block_left = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let radius = st.f32("bg-radius", RADIUS_EM) * fs;
    let radius_bl = RADIUS_BL_EM * fs;
    let bg = st.color("bg-color", BG);
    let show_tail = st.bool("show-tail", true);
    let shade_dy = SHADE_DY_EM * fs;
    let shade_std = SHADE_BLUR_EM * fs / 2.0;
    let rise = recipes::RiseIn::custom(
        ov.distance_or(RISE_EM),
        0.0,
        ov.duration_or(RISE_DURATION_S),
        ease_lena,
    );

    let mut lines_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    let last_line = lines.len().saturating_sub(1);
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let box_top = block_top + line_idx as f32 * (box_h + gap);
        let box_bottom = box_top + box_h;
        let line_top = box_top + pad_y;
        let baseline = line_top + half_leading + ascent;
        let run: f32 = run_widths[start..end].iter().sum();
        let content_left = block_left + pad_x;
        let xs = layout::word_x_positions(&widths[start..end], margin, content_left, run, rtl);

        // `rise-in` fires when the line's own narration starts.
        let span = state::LineSpan::of_words(&segment.words[start..end]);
        let (opacity, dy_em) = if ov.motion_static {
            (1.0, 0.0)
        } else {
            rise.eval((now_ms - span.start_ms) / 1000.0)
        };
        lines_svg.push_str(&format!(
            "<g opacity=\"{opacity}\" transform=\"translate(0 {})\">",
            dy_em * fs
        ));

        // The bubble carries the shade (box-shadow follows the rounded
        // box, so the filter rides the path, never the text).
        let d = bubble_path(
            block_left,
            box_top,
            run + 2.0 * pad_x,
            box_h,
            radius,
            radius,
            radius,
            radius_bl,
        );
        lines_svg
            .push_str(&format!("<path d=\"{d}\" fill=\"{bg}\" filter=\"url(#lena-shade)\"/>"));
        if line_idx == last_line && show_tail {
            lines_svg.push_str(&format!(
                "<polygon points=\"{}\" fill=\"{bg}\"/>",
                tail_points(block_left, box_bottom, fs)
            ));
        }
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&word.display);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            lines_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr}>{text}</text>",
                primary = ov.primary.as_str(),
            ));
        }
        lines_svg.push_str("</g>");
    }
    lines_svg.push_str("</g>");

    // User rotation wraps the bubble stack about its anchor.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = block_left;
        let cy = block_top + block_h / 2.0;
        lines_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = lines_svg,
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
                <filter id="lena-shade" x="-30%" y="-30%" width="160%" height="170%">
                    <feDropShadow dx={0.0} dy={shade_dy} stdDeviation={shade_std} flood-color="#000000" flood-opacity={SHADE_OPACITY}/>
                </filter>
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
    fn embedded_inter_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(INTER_TTF, 0).unwrap();
        let mut name = None;
        for n in face.names() {
            if n.name_id == fframes::ttf_parser::name_id::FAMILY {
                name = n.to_string();
                break;
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
    fn lena_splitter_is_sentence_plus_sixty_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "First clause is here, second clause follows right after this one, \
                       and the third clause carries on for a while longer."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "{segments:?}");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
    }

    #[test]
    fn bubble_squares_its_bottom_left_corner() {
        let d = bubble_path(0.0, 0.0, 64.0, 32.0, 8.0, 8.0, 8.0, 4.0);
        // Top-left rounds, bottom-left uses the tighter 0.2em radius.
        assert!(d.starts_with("M 8 0"), "{d}");
        assert!(d.contains("A 4 4 0 0 1 0 28"), "{d}");
        assert!(d.ends_with('Z'), "{d}");
        // The tail hangs left of the bubble and sits on its bottom edge.
        let pts: Vec<f32> = tail_points(0.0, 32.0, 40.0)
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(pts.len(), 10);
        let expected = [12.0, 13.2, -9.2, 13.2, -3.9, 29.18, 6.7, 32.0, 9.244, 18.84];
        for (got, want) in pts.iter().zip(expected) {
            assert!((got - want).abs() < 1e-3, "tail point {got} vs {want}");
        }
    }

    #[test]
    fn rise_in_overshoots_on_lenas_curve() {
        let rise = recipes::RiseIn::custom(RISE_EM, 0.0, RISE_DURATION_S, ease_lena);
        // Hidden and dropped 0.27em before it starts.
        let (opacity, dy) = rise.eval(-1.0);
        assert_eq!(opacity, 0.0);
        assert!((dy - RISE_EM).abs() < 1e-6);
        // It rises past 1 (translateY < 0) on the way in — the curve
        // overshoots, not a keyframe.
        let (_, mid) = rise.eval(0.32 * 0.5);
        assert!(mid < 0.0, "overshot to {mid}");
        let (opacity, dy) = rise.eval(0.32);
        assert!((opacity - 1.0).abs() < 1e-6 && dy.abs() < 1e-6);
    }
}
