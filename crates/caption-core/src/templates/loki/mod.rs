//! Native port of the tscaps `loki` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\loki\{style.scss,template.json,filters.svg}`.
//! Uppercase Komika Axis at 4.5cqh wearing `beast-outline`: a 0.125em
//! black dilation ring, a #ffea00 glow blurred to σ 0 (radius 0 — the
//! halo stays hidden under the wider opaque outline until a variant
//! spreads it), and a black drop-shadow cast by the *outlined*
//! silhouette, stacked shadow → glow → outline → fill. The segment
//! pops in through `scale-in` (from 0.96, 0.28s,
//! `cubic-bezier(0.34, 1.4, 0.64, 1)`) while narrated words light
//! `#ffea00`. Clause segments of 22 chars (min 0), up to two balanced
//! rows, block centre-anchored at 0.75H, `remove_punctuation` +
//! `gap_free` on, `rendering.padding: 2em` inflating the filter box.
//!
//! Filter ports (ids prefixed `loki-`): `tscaps:outline` →
//! dilate/flood/composite ring; `tscaps:drop-shadow` →
//! offset/blur/flood/composite off `outline-shape`. The drop-shadow
//! blur *is* the Gaussian σ (the recipe's "from filter:/drop-shadow()
//! keep" semantics).

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from loki's style.scss + template.json) ──────────────
const FONT: &str = "Komika Axis"; // fontStack komika-axis
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.045; // font-size: 4.5cqh
const LETTER_SPACING_EM: f32 = 0.02; // letter-spacing: 0.02em
const LINE_HEIGHT: f32 = 1.2; // line-height: null → font-native (1.2 stands in)
const WORD_SPACING_EM: f32 = 0.16; // @include word-layout(0.16em)
const LINE_SPACING_EM: f32 = 0.0; // typography.lineSpacing (absent → 0)
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (center)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalAlign default (center)
const MAX_CHARS: usize = 22; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // ...minChars (descriptor default; absent in json)
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#ffffff"; // primary-color default
const HIGHLIGHT: &str = "#ffea00"; // highlight-color default
const OUTLINE_THICKNESS_EM: f32 = 0.125; // outline-thickness control default
const OUTLINE_COLOR: &str = "#000000"; // outline-color default (scss #000)
const GLOW_RADIUS_EM: f32 = 0.0; // glow-radius control default
const GLOW_COLOR: &str = "#ffea00"; // glow-color default
const SHADOW_DISTANCE_EM: f32 = 0.04; // filter-shadow-distance default
const SHADOW_BLUR_EM: f32 = 0.04; // filter-shadow-blur default (Gaussian σ)
const SHADOW_COLOR: &str = "#000000"; // shadow-color default
const PADDING_EM: f32 = 2.0; // rendering.padding: 2em
// scale-in($element: 'segment', $from: 0.96, $from-opacity: 1,
// $duration: 0.28s, $easing: cubic-bezier(0.34, 1.4, 0.64, 1),
// $delay: var(--on-segment-starts))
const SCALE_FROM: f32 = 0.96;
const SCALE_FROM_OPACITY: f32 = 1.0;
const SCALE_DURATION_S: f32 = 0.28;

/// loki's `effects` array: `remove_punctuation` + `gap_free`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[
        effects::Effect::RemovePunctuation,
        effects::Effect::GapFree,
    ],
};

/// The loki `scale-in` overshoot: `cubic-bezier(0.34, 1.4, 0.64, 1)`.
fn ease_loki(x: f32) -> f32 {
    recipes::cubic_bezier(0.34, 1.4, 0.64, 1.0)(x)
}

// ── fonts ─────────────────────────────────────────────────────────────────

const KOMIKA_AXIS_400_TTF: &[u8] = include_bytes!("../../../assets/komika-axis-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Komika Axis.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face =
            fframes::ttf_parser::Face::parse(KOMIKA_AXIS_400_TTF, 0).expect("Komika Axis parses");
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

// ── beast-outline filter ──────────────────────────────────────────────────

/// The `beast-outline` filter body from filters.svg, recipes expanded:
/// the `tscaps:outline` ring from the raw glyph (feeding both the merge
/// *and* the shadow's silhouette), the glow blurred off SourceAlpha, the
/// drop-shadow offset/blur/flood/composite off `outline-shape`, all
/// stacked shadow → glow → outline → SourceGraphic. Em values resolve to
/// px at font size `fs`.
fn filter_defs(fs: f32) -> String {
    let radius = OUTLINE_THICKNESS_EM * fs;
    let glow_sigma = GLOW_RADIUS_EM * fs;
    let shadow_dy = SHADOW_DISTANCE_EM * fs;
    let shadow_sigma = SHADOW_BLUR_EM * fs;
    format!(
        "<filter id=\"loki-beast-outline\" x=\"-15%\" y=\"-15%\" width=\"130%\" height=\"130%\" \
         color-interpolation-filters=\"sRGB\">\
         <feMorphology in=\"SourceAlpha\" operator=\"dilate\" radius=\"{radius}\" \
         result=\"outline-shape\"/>\
         <feFlood flood-color=\"{OUTLINE_COLOR}\" result=\"outline-ink\"/>\
         <feComposite in=\"outline-ink\" in2=\"outline-shape\" operator=\"in\" \
         result=\"outline\"/>\
         <feGaussianBlur in=\"SourceAlpha\" stdDeviation=\"{glow_sigma}\" result=\"glowSpread\"/>\
         <feFlood flood-color=\"{GLOW_COLOR}\" result=\"glowFlood\"/>\
         <feComposite in=\"glowFlood\" in2=\"glowSpread\" operator=\"in\" result=\"glow\"/>\
         <feOffset in=\"outline-shape\" dx=\"0\" dy=\"{shadow_dy}\" result=\"shadow-offset\"/>\
         <feGaussianBlur in=\"shadow-offset\" stdDeviation=\"{shadow_sigma}\" \
         result=\"shadow-spread\"/>\
         <feFlood flood-color=\"{SHADOW_COLOR}\" result=\"shadow-ink\"/>\
         <feComposite in=\"shadow-ink\" in2=\"shadow-spread\" operator=\"in\" result=\"shadow\"/>\
         <feMerge>\
         <feMergeNode in=\"shadow\"/><feMergeNode in=\"glow\"/>\
         <feMergeNode in=\"outline\"/><feMergeNode in=\"SourceGraphic\"/>\
         </feMerge>\
         </filter>"
    )
}

/// `rendering.padding: 2em` as the transparent rectangle drawn first
/// inside the filtered group — the tscaps border box the filter region
/// percentages resolve against.
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

    let video_w = ctx.current_video_size.width;
    let video_h = ctx.current_video_size.height;
    let fs = FONT_SIZE_CQH * video_h as f32;
    let ls_px = LETTER_SPACING_EM * fs;
    let margin = WORD_SPACING_EM * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) = line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        KOMIKA_AXIS_400_TTF,
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

    // Centre-anchored block: the block's *centre* sits at 0.75H; lines
    // center horizontally.
    let line_gap = LINE_SPACING_EM * fs;
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * line_gap;
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - block_h / 2.0;
    let block_w: f32 = lines
        .iter()
        .map(|&(start, end)| widths[start..end].iter().map(|w| w + 2.0 * margin).sum::<f32>())
        .fold(0.0, f32::max);
    let center_x = ANCHOR_X_FRAC * video_w as f32;
    let block_left = center_x - block_w / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Segment scale-in: struck on the segment's own start (fill `both`,
    // so it rests at scale 1 / opacity 1 before and after).
    let scale_in = recipes::ScaleIn {
        from_scale: SCALE_FROM,
        from_opacity: SCALE_FROM_OPACITY,
        peak: None,
        peak_at: 0.6,
        duration_s: SCALE_DURATION_S,
        ease: ease_loki,
    };
    let (scale_opacity, scale) = scale_in.eval((now_ms - segment.start_ms) / 1000.0);

    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + line_gap);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&uppercase(&word.display));
            let fill = if state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
                HIGHLIGHT
            } else {
                PRIMARY
            };
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}>{text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    // Segment shell: the beast filter (CSS applies filter in the element's
    // local space, then the transform maps it — so the scale wraps the
    // filtered group), the scale-in, and the padding rect.
    let cx = center_x;
    let cy = block_top + block_h / 2.0;
    let mut segment_svg = format!(
        "<g transform=\"translate({cx},{cy}) scale({scale}) translate({nx},{cy2})\" \
         opacity=\"{scale_opacity}\"><g filter=\"url(#loki-beast-outline)\">",
        nx = -cx,
        cy2 = -cy,
    );
    segment_svg.push_str(&padding_rect(block_left, block_top, block_w, block_h, fs));
    segment_svg.push_str(&words_svg);
    segment_svg.push_str("</g></g>");

    let defs = filter_defs(fs);
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
            {segment_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_komika_axis_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(KOMIKA_AXIS_400_TTF, 0).unwrap();
        let mut name = None;
        // fontdb only accepts Unicode name records — this face's
        // Macintosh Roman ones decode to `None` and must be skipped.
        for n in face.names() {
            if n.name_id == 16 && n.to_string().is_some() {
                name = n.to_string();
                break;
            }
        }
        if name.is_none() {
            for n in face.names() {
                if n.name_id == 1 && n.to_string().is_some() {
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
    fn beast_outline_stacks_shadow_glow_ring_then_source() {
        let fs = 54.0; // 4.5cqh at 1200px
        let defs = filter_defs(fs);
        assert!(defs.contains("id=\"loki-beast-outline\""), "{defs}");
        assert!(
            defs.contains("x=\"-15%\" y=\"-15%\" width=\"130%\" height=\"130%\""),
            "{defs}"
        );
        // Ring: dilate by the control-default thickness.
        assert!(
            defs.contains(&format!(
                "operator=\"dilate\" radius=\"{}\"",
                OUTLINE_THICKNESS_EM * fs
            )),
            "{defs}"
        );
        // Glow: blurred off the raw glyph, flooded highlight.
        assert!(
            defs.contains(&format!(
                "in=\"SourceAlpha\" stdDeviation=\"{}\" result=\"glowSpread\"",
                GLOW_RADIUS_EM * fs
            )),
            "{defs}"
        );
        assert!(defs.contains(&format!("flood-color=\"{GLOW_COLOR}\"")), "{defs}");
        // Shadow: fed the *outlined* silhouette, offset then σ-blurred.
        assert!(
            defs.contains(&format!(
                "in=\"outline-shape\" dx=\"0\" dy=\"{}\" result=\"shadow-offset\"",
                SHADOW_DISTANCE_EM * fs
            )),
            "{defs}"
        );
        assert!(
            defs.contains(&format!(
                "in=\"shadow-offset\" stdDeviation=\"{}\" result=\"shadow-spread\"",
                SHADOW_BLUR_EM * fs
            )),
            "{defs}"
        );
        assert!(defs.contains(&format!("flood-color=\"{SHADOW_COLOR}\"")), "{defs}");
        let shadow = defs.find("in=\"shadow\"").expect("shadow merged");
        let glow = defs.find("in=\"glow\"").expect("glow merged");
        let outline = defs.find("in=\"outline\"").expect("outline merged");
        let source = defs.find("in=\"SourceGraphic\"").expect("source on top");
        assert!(shadow < glow && glow < outline && outline < source, "stack order");
    }

    #[test]
    fn segment_pops_from_096_with_the_loki_overshoot() {
        let scale_in = recipes::ScaleIn {
            from_scale: SCALE_FROM,
            from_opacity: SCALE_FROM_OPACITY,
            peak: None,
            peak_at: 0.6,
            duration_s: SCALE_DURATION_S,
            ease: ease_loki,
        };
        // Before the segment starts (fill both): resting at the from values.
        let (op0, sc0) = scale_in.eval(-0.1);
        assert_eq!(op0, SCALE_FROM_OPACITY);
        assert!((sc0 - SCALE_FROM).abs() < 1e-6);
        // Mid-flight the custom bezier has travelled part-way; its y
        // overshoot (1.4) lifts the scale a hair past 1 before it settles.
        let (_, sc_mid) = scale_in.eval(SCALE_DURATION_S * 0.5);
        assert!(sc_mid > SCALE_FROM && sc_mid < 1.01, "sc_mid {sc_mid}");
        // Landed after 0.28s.
        let (op1, sc1) = scale_in.eval(SCALE_DURATION_S);
        assert!((op1 - 1.0).abs() < 1e-6);
        assert!((sc1 - 1.0).abs() < 1e-6);
        // The ease is the overshoot bezier — still monotonic to 1 at p=1.
        assert!((ease_loki(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_loki(0.5) > 0.5, "overshoots mid-flight");
        assert_eq!(PADDING_EM, 2.0);
    }

    #[test]
    fn segment_padding_inflates_the_filter_box_twice_as_far() {
        let rect = padding_rect(100.0, 200.0, 300.0, 80.0, 54.0);
        assert!(rect.contains("x=\"-8\""), "{rect}"); // 100 − 2·54
        assert!(rect.contains("y=\"92\""), "{rect}"); // 200 − 108
        assert!(rect.contains("width=\"516\""), "{rect}"); // 300 + 216
        assert!(rect.contains("height=\"296\""), "{rect}"); // 80 + 216
        assert!(rect.contains("fill-opacity=\"0\""), "{rect}");
    }

    #[test]
    fn loki_clause_segments_split_at_twenty_two_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, hold on now and look at this one here, please."
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
        assert_eq!(uppercase("beast mode"), "BEAST MODE");
    }
}
