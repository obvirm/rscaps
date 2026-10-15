//! Native port of the tscaps `naya` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\naya\{style.scss,template.json,filters.svg}`.
//! Uppercase Anton at 4.69cqh wearing a 0.14em black dilation ring —
//! `hype-outline` sits on the whole segment, not the word, so
//! neighbouring words never clip each other's outline — rising into
//! place through `rise-in` (0.13em, 0.25s, `ease`) while every word
//! pops (`pop-in`, 10%, 0.25s) on its own narration start and lights
//! `#ffff00` while narrated. Sentence segments of 35 chars (min 8), up
//! to two balanced rows of 0.72W, block top-anchored at 0.75H, with
//! `rendering.padding: 1em` inflating the filter's box. Unported: the
//! `emoji` decoration effect — its sole `effects` entry, skipped because
//! no color emoji font is bundled; the style controls bake their
//! `template.json` defaults in place of variants.

use fframes::{FFramesContext, Svgr};

use super::{layout, overrides, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from naya's style.scss + template.json) ───────────────
const FONT: &str = "Anton"; // fontStack anton
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.0469; // font-size: 4.69cqh
const LETTER_SPACING_EM: f32 = 0.015; // letter-spacing: 0.015em
const LINE_HEIGHT: f32 = 1.15; // line-height: 1.15
const WORD_SPACING_EM: f32 = 0.14; // @include word-layout(0.14em)
const LINE_SPACING_EM: f32 = 0.1; // typography.lineSpacing: 0.1
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalAlign default (center)
const MAX_CHARS: usize = 35; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 8; // ...minChars
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#ffffff"; // primary-color default
const HIGHLIGHT: &str = "#ffff00"; // highlight-color default
const OUTLINE_COLOR: &str = "#000000"; // outline-color default
const OUTLINE_THICKNESS_EM: f32 = 0.14; // outline-thickness default (em)
const PADDING_EM: f32 = 1.0; // rendering.padding: 1em (filter-box inflation)
// rise-in($element: 'segment', $distance: 0.13em, $from-opacity: 0,
// $duration: 0.25s, $easing: ease, $delay: var(--on-segment-starts))
const RISE_DISTANCE_EM: f32 = 0.13;
const RISE_DURATION_S: f32 = 0.25;
// pop-in($element: 'word', $amount: 0.1, $duration: 0.25s,
// $easing: ease, $delay: var(--on-word-being-narrated-starts))
const POP_AMOUNT: f32 = 0.1;
const POP_DURATION_S: f32 = 0.25;

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
    highlight: HIGHLIGHT,
    force_uppercase: true, // naya always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// naya's `segmentSplitters`: boundary (sentence preset) then 35/8 chars.
/// Its `effects` array is `emoji`-only — unported, so windows stay the
/// raw word span.
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

// ── outline filter ────────────────────────────────────────────────────────

/// The `hype-outline` filter body: the `tscaps:outline` recipe expanded
/// at font size `fs` (dilate `SourceAlpha` by the em thickness, flood the
/// dilation, composite it back in) merged under `SourceGraphic`. The
/// region is the recipe's host declaration — the visible thickness *is*
/// the radius, so the box has to leave room for it.
fn filter_defs(fs: f32) -> String {
    let radius = OUTLINE_THICKNESS_EM * fs;
    format!(
        "<filter id=\"naya-hype-outline\" x=\"-10%\" y=\"-20%\" width=\"120%\" height=\"140%\" \
         color-interpolation-filters=\"sRGB\">\
         <feMorphology in=\"SourceAlpha\" operator=\"dilate\" radius=\"{radius}\" \
         result=\"outline-shape\"/>\
         <feFlood flood-color=\"{OUTLINE_COLOR}\" result=\"outline-ink\"/>\
         <feComposite in=\"outline-ink\" in2=\"outline-shape\" operator=\"in\" \
         result=\"outline\"/>\
         <feMerge><feMergeNode in=\"outline\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>\
         </filter>"
    )
}

/// `rendering.padding: 1em` as the transparent rectangle drawn first
/// inside the filtered group. In tscaps the filter region percentages
/// resolve against the segment's border box — content plus padding, with
/// the negative margin keeping layout put — and a transparent rect of
/// that geometry gives the group the same bbox here.
fn padding_rect(block_left: f32, block_top: f32, block_w: f32, block_h: f32, fs: f32) -> String {
    let pad = PADDING_EM * fs;
    format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" \
         fill=\"#000000\" fill-opacity=\"0\"/>",
        x = block_left - pad,
        y = block_top - pad,
        w = block_w + 2.0 * pad,
        h = block_h + 2.0 * pad,
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

    let video_w = crate::ctx_video_size(ctx).width;
    let video_h = crate::ctx_video_size(ctx).height;
    // User overrides (params) win over the template's spec constants.
    let ov = overrides::resolve(&doc.params, DEFAULTS);
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
        ANTON_400_TTF,
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

    // Top-aligned block: the block's top edge sits at 0.75H; lines center
    // horizontally.
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;
    let line_gap = ov.line_spacing_em * fs;
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * line_gap;
    let block_w: f32 = lines
        .iter()
        .map(|&(start, end)| widths[start..end].iter().map(|w| w + 2.0 * margin).sum::<f32>())
        .fold(0.0, f32::max);
    let block_left = center_x - block_w / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Segment rise: `--on-segment-starts` is the segment's own start.
    let rise = recipes::RiseIn::custom(
        ov.distance_or(RISE_DISTANCE_EM),
        0.0,
        ov.duration_or(RISE_DURATION_S),
        recipes::ease,
    );
    let (rise_opacity, rise_dy_em) = if ov.motion_static {
        (1.0, 0.0)
    } else {
        rise.eval((now_ms - segment.start_ms) / 1000.0)
    };
    let rise_dy = rise_dy_em * fs;

    // Word pop: struck on the word's narration start (fill `both`, so it
    // rests at scale 1 before and after the window).
    let pop = recipes::PopIn {
        amount: POP_AMOUNT,
        duration_s: ov.duration_or(POP_DURATION_S),
        ease: recipes::ease,
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
            let scale = if ov.motion_static {
                1.0
            } else {
                pop.eval((now_ms - word.start_ms) / 1000.0)
            };
            let fill = if state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
                HIGHLIGHT
            } else {
                ov.primary.as_str()
            };
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            // Scale about the word's own box centre (sol's convention).
            let cx = *x + widths[start + j] / 2.0;
            let cy = baseline + (descent - ascent) / 2.0;
            words_svg.push_str(&format!(
                "<g transform=\"translate({cx},{cy}) scale({scale}) translate({nx},{ny})\">\
                 <text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}>{text}</text></g>",
                nx = -cx,
                ny = -cy,
            ));
        }
    }
    words_svg.push_str("</g>");

    // Segment shell: the outline filter, the rise, and the padding rect
    // that gives the filter its tscaps border box.
    let mut segment_svg = format!(
        "<g filter=\"url(#naya-hype-outline)\" opacity=\"{rise_opacity}\" \
         transform=\"translate(0,{rise_dy})\">"
    );
    segment_svg.push_str(&padding_rect(block_left, block_top, block_w, block_h, fs));
    segment_svg.push_str(&words_svg);
    segment_svg.push_str("</g>");

    // User rotation wraps the caption around the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        segment_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = segment_svg,
        );
    }

    let defs = filter_defs(fs);
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                {defs}
            </defs>
            {segment_svg}
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
    fn hype_outline_dilates_the_glyph_ring_and_merges_it_under_the_source() {
        let fs = 48.0;
        let defs = filter_defs(fs);
        // The recipe, expanded: dilate → flood → composite, then the ring
        // under SourceGraphic inside the host's region.
        assert!(defs.contains("id=\"naya-hype-outline\""), "{defs}");
        assert!(defs.contains("x=\"-10%\" y=\"-20%\" width=\"120%\" height=\"140%\""), "{defs}");
        assert!(
            defs.contains(&format!(
                "operator=\"dilate\" radius=\"{}\"",
                OUTLINE_THICKNESS_EM * fs
            )),
            "{defs}"
        );
        assert!(defs.contains("result=\"outline-shape\""), "{defs}");
        assert!(
            defs.contains(&format!("flood-color=\"{OUTLINE_COLOR}\"")),
            "{defs}"
        );
        let outline_at = defs.find("in=\"outline\"").expect("ring merged");
        let source_at = defs
            .find("in=\"SourceGraphic\"")
            .expect("source merged on top");
        assert!(outline_at < source_at, "ring paints under the source");
        // The thickness is the radius — no double-dip.
        assert_eq!(OUTLINE_THICKNESS_EM, 0.14);
        assert_eq!(PADDING_EM, 1.0);
    }

    #[test]
    fn segment_padding_inflates_the_filter_box_without_moving_it() {
        // Block at (100, 200) sized 300×80 with a 1em (48px) pad: the
        // rect grows in every direction and stays transparent.
        let rect = padding_rect(100.0, 200.0, 300.0, 80.0, 48.0);
        assert!(rect.contains("x=\"52\""), "{rect}");
        assert!(rect.contains("y=\"152\""), "{rect}");
        assert!(rect.contains("width=\"396\""), "{rect}");
        assert!(rect.contains("height=\"176\""), "{rect}");
        assert!(rect.contains("fill-opacity=\"0\""), "{rect}");
    }

    #[test]
    fn naya_splitter_is_sentence_plus_thirty_five_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "This caption is definitely long enough to need several segments here."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
        assert_eq!(MAX_LINES, 2);
    }

    #[test]
    fn segment_rises_and_the_word_pops_on_its_own_clock() {
        let rise = recipes::RiseIn::custom(RISE_DISTANCE_EM, 0.0, RISE_DURATION_S, recipes::ease);
        // Before the segment starts (fill both): hidden, full distance.
        let (op0, dy0) = rise.eval(-0.1);
        assert_eq!(op0, 0.0);
        assert!((dy0 - RISE_DISTANCE_EM).abs() < 1e-6);
        // Landed after the quarter second.
        let (op1, dy1) = rise.eval(RISE_DURATION_S);
        assert!((op1 - 1.0).abs() < 1e-6);
        assert!(dy1.abs() < 1e-6);

        let pop = recipes::PopIn {
            amount: POP_AMOUNT,
            duration_s: POP_DURATION_S,
            ease: recipes::ease,
        };
        // Resting before the word's narration, overshooting at 40%.
        assert!((pop.eval(-1.0) - 1.0).abs() < 1e-6);
        assert!((pop.eval(POP_DURATION_S * 0.4) - 1.1).abs() < 1e-5);
        assert!((pop.eval(POP_DURATION_S) - 1.0).abs() < 1e-6);
        assert_eq!(uppercase("hype moment"), "HYPE MOMENT");
    }
}
