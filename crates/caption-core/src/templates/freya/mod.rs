//! Native port of the tscaps `freya` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\freya\{style.scss,template.json,filters.svg}`.
//! Uppercase Anton at 5.5cqh, negative line spacing (-0.28em), block
//! bottom-anchored at 0.84H and tilted -3° about its own centre. The
//! segment wears `glitch-outline` (a 0.1em #0a0a0a dilation ring)
//! throughout; `tscaps-glitch-flicker` overlays `#glitch` in three
//! short entry bursts, each frame of which sees a fresh turbulence
//! seed (the 60 Hz `tscaps-tick-60` → `(now_ms * 0.06).floor()`).
//! `#glitch` quantises horizontal-only noise, displaces both the source
//! and its alpha, and stacks the chromatic-split ghosts behind the
//! displaced source. Clause segments of 22 chars (min 0), up to two
//! balanced rows, `remove_punctuation` on (the `emoji` decoration is
//! unported — v1 renders no emoji). `rendering.padding: 1em` inflates
//! the filter box; the style controls bake their `template.json`
//! defaults (outline thickness 0.1, split-x 0.05em, displacement 9,
//! frequency 0.75).
//!
//! Filter ports (ids prefixed `freya-`): `tscaps:outline` →
//! dilate/flood/composite ring; `tscaps:chromatic-split` → two
//! feOffset/feFlood/feComposite ghosts at (±x, y=0) off the displaced
//! alpha.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, stack};
use crate::CaptionDocument;

// ── spec constants (from freya's style.scss + template.json) ─────────────
const FONT: &str = "Anton"; // fontStack anton
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.055; // font-size: 5.5cqh
const LETTER_SPACING_EM: f32 = 0.02; // letter-spacing: 0.02em
const LINE_HEIGHT: f32 = 1.2; // line-height: 1.2 (mixin default)
const WORD_SPACING_EM: f32 = 0.12; // @include word-layout(0.12em)
const LINE_SPACING_EM: f32 = -0.28; // typography.lineSpacing: -0.28
const ROTATION_DEG: f32 = -3.0; // rotation.angleDeg: -3
const ANCHOR_Y_FRAC: f32 = 0.84; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalAlign: center
const MAX_CHARS: usize = 22; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // ...minChars (descriptor default; absent in json)
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#ffffff"; // primary-color default
const HIGHLIGHT: &str = "#ffffff"; // highlight-color default (same ink)
const OUTLINE_THICKNESS_EM: f32 = 0.1; // outline-thickness control default
const OUTLINE_COLOR: &str = "#0a0a0a"; // outline-color default
const SPLIT_X_EM: f32 = 0.05; // split-x control default
const SPLIT_COLOR_A: &str = "#ff0044"; // split-color-a default
const SPLIT_COLOR_B: &str = "#00f4ff"; // split-color-b default
const DISPLACEMENT_AMOUNT: f32 = 9.0; // displacement-amount control default
const DISPLACEMENT_FREQUENCY: f32 = 0.75; // displacement-frequency default
const PADDING_EM: f32 = 1.0; // rendering.padding: 1em
// `tscaps-glitch-flicker` (step-end, relative to the segment's own
// duration): bursts [0,5%) [6,10%) [11,14%), outline-only elsewhere.
const FLICKER_BURSTS: [(f32, f32); 3] = [(0.0, 0.05), (0.06, 0.10), (0.11, 0.14)];

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
    force_uppercase: true, // freya always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// freya's `effects` array: `emoji` (unported) + `remove_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::RemovePunctuation],
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

// ── glitch filters ────────────────────────────────────────────────────────

/// The `glitch-outline` filter body: the `tscaps:outline` recipe at the
/// control-default thickness, merged under SourceGraphic.
fn outline_filter_defs(fs: f32) -> String {
    let radius = OUTLINE_THICKNESS_EM * fs;
    format!(
        "<filter id=\"freya-glitch-outline\" x=\"-15%\" y=\"-20%\" width=\"130%\" height=\"140%\" \
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

/// The `glitch` filter body: anisotropic turbulence (quantised R,
/// constant G → horizontal-only bands) displacing the source *and* its
/// alpha; `chromatic-split` ghosts at (±split-x, 0) off the displaced
/// alpha stack behind the displaced source. `seed` is the 60 Hz tick
/// (fresh integer per output frame in tscaps). Region per filters.svg
/// (no explicit colour-interpolation — the recipe's linearRGB default
/// stays).
fn glitch_filter_defs(fs: f32, tick: i32) -> String {
    let sx = SPLIT_X_EM * fs;
    let nsx = -sx;
    let scale = DISPLACEMENT_AMOUNT;
    format!(
        "<filter id=\"freya-glitch\" x=\"-20%\" y=\"-15%\" width=\"140%\" height=\"130%\">\
         <feTurbulence type=\"fractalNoise\" baseFrequency=\"0.02 {DISPLACEMENT_FREQUENCY}\" \
         numOctaves=\"2\" seed=\"{tick}\" result=\"rawNoise\"/>\
         <feComponentTransfer in=\"rawNoise\" result=\"noise\">\
         <feFuncR type=\"discrete\" tableValues=\"0 0.35 0.65 1\"/>\
         <feFuncG type=\"linear\" slope=\"0\" intercept=\"0.5\"/>\
         </feComponentTransfer>\
         <feDisplacementMap in=\"SourceGraphic\" in2=\"noise\" scale=\"{scale}\" \
         xChannelSelector=\"R\" yChannelSelector=\"G\" result=\"displacedSrc\"/>\
         <feDisplacementMap in=\"SourceAlpha\" in2=\"noise\" scale=\"{scale}\" \
         xChannelSelector=\"R\" yChannelSelector=\"G\" result=\"displacedAlpha\"/>\
         <feOffset in=\"displacedAlpha\" dx=\"{sx}\" dy=\"0\" result=\"split-a-shape\"/>\
         <feFlood flood-color=\"{SPLIT_COLOR_A}\" result=\"split-a-ink\"/>\
         <feComposite in=\"split-a-ink\" in2=\"split-a-shape\" operator=\"in\" result=\"split-a\"/>\
         <feOffset in=\"displacedAlpha\" dx=\"{nsx}\" dy=\"0\" result=\"split-b-shape\"/>\
         <feFlood flood-color=\"{SPLIT_COLOR_B}\" result=\"split-b-ink\"/>\
         <feComposite in=\"split-b-ink\" in2=\"split-b-shape\" operator=\"in\" result=\"split-b\"/>\
         <feMerge>\
         <feMergeNode in=\"split-a\"/><feMergeNode in=\"split-b\"/>\
         <feMergeNode in=\"displacedSrc\"/>\
         </feMerge>\
         </filter>"
    )
}

/// `tscaps-glitch-flicker` under `step-end`: the outline filter is
/// chained on every keyframe, so the burst is just the glitch layer
/// appearing over it for three short windows of the segment's life.
fn glitch_burst_on(p: f32) -> bool {
    FLICKER_BURSTS
        .iter()
        .any(|&(from, to)| p >= from && p < to)
}

/// `tscaps-tick-60` resolved for this frame: the same 60 Hz integer the
/// studio's render loop feeds into `feTurbulence seed`.
fn tick_60(now_ms: f32) -> i32 {
    (now_ms * 0.06).floor() as i32
}

/// `rendering.padding: 1em` as the transparent rectangle drawn first
/// inside the filtered group (naya's border-box convention).
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

    // Bottom-aligned block: the block's bottom edge sits at 0.84H (the
    // negative line spacing shrinks the block from what the per-line
    // math would otherwise give).
    let line_gap = ov.line_spacing_em * fs;
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * line_gap;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let block_w: f32 = lines
        .iter()
        .map(|&(start, end)| widths[start..end].iter().map(|w| w + 2.0 * margin).sum::<f32>())
        .fold(0.0, f32::max);
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Segment flicker: the burst rides the segment's own lifetime.
    let duration_s = ((segment.end_ms - segment.start_ms) / 1000.0).max(1e-3);
    let p = ((now_ms - segment.start_ms) / 1000.0 / duration_s).clamp(0.0, 1.0);
    let burst = glitch_burst_on(p);
    let tick = tick_60(now_ms);

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
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            // `.word-being-narrated { color: highlight }` — the default
            // highlight is the same white as the primary, but the split
            // still matters when a variant overrides one of them.
            let fill = if super::state::word_narration(now_ms, word.start_ms, word.end_ms)
                .is_being()
            {
                HIGHLIGHT
            } else {
                ov.primary.as_str()
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

    // Burst group (only while the flicker window is open) nested *inside*
    // the outline group, matching the keyframe chain
    // `url(#glitch-outline) url(#glitch)`.
    let inner = if burst {
        format!("<g filter=\"url(#freya-glitch)\">{words_svg}</g>")
    } else {
        words_svg
    };

    // The outline + rotation ride the whole segment (CSS applies filter
    // in the element's local space, then the transform maps it — so the
    // rotation wraps the filtered group).
    let cx = center_x;
    let cy = block_top + block_h / 2.0;
    let mut segment_svg = format!(
        "<g transform=\"rotate({ROTATION_DEG} {cx} {cy})\">\
         <g filter=\"url(#freya-glitch-outline)\">"
    );
    segment_svg.push_str(&padding_rect(
        center_x - block_w / 2.0,
        block_top,
        block_w,
        block_h,
        fs,
    ));
    segment_svg.push_str(&inner);
    segment_svg.push_str("</g></g>");

    // User rotation composes outside the template's own tilt, about the
    // same block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        segment_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = segment_svg,
        );
    }

    let defs = format!(
        "{}{}",
        outline_filter_defs(fs),
        glitch_filter_defs(fs, tick)
    );
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
    fn flicker_bursts_are_three_short_entry_windows() {
        assert!(glitch_burst_on(0.0), "first frame bursts");
        assert!(glitch_burst_on(0.04), "inside first pulse");
        assert!(!glitch_burst_on(0.05), "gap between pulses");
        assert!(glitch_burst_on(0.06), "second pulse opens");
        assert!(!glitch_burst_on(0.10), "second pulse closes");
        assert!(glitch_burst_on(0.11), "third pulse opens");
        assert!(!glitch_burst_on(0.14), "rest of the segment is outline-only");
        assert!(!glitch_burst_on(1.0), "settled after the entry");
        assert_eq!(FLICKER_BURSTS[0], (0.0, 0.05));
    }

    #[test]
    fn outline_ring_dilates_by_the_control_default_and_rides_every_keyframe() {
        let fs = 60.5; // 5.5cqh at 1100px
        let defs = outline_filter_defs(fs);
        assert!(defs.contains("id=\"freya-glitch-outline\""), "{defs}");
        assert!(
            defs.contains("x=\"-15%\" y=\"-20%\" width=\"130%\" height=\"140%\""),
            "{defs}"
        );
        assert!(
            defs.contains(&format!(
                "operator=\"dilate\" radius=\"{}\"",
                OUTLINE_THICKNESS_EM * fs
            )),
            "{defs}"
        );
        assert!(defs.contains(&format!("flood-color=\"{OUTLINE_COLOR}\"")), "{defs}");
        let outline_at = defs.find("in=\"outline\"").expect("ring merged");
        let source_at = defs.find("in=\"SourceGraphic\"").expect("source on top");
        assert!(outline_at < source_at, "ring under the source");
        assert_eq!(OUTLINE_THICKNESS_EM, 0.1, "control default, not the 0.08 fallback");
    }

    #[test]
    fn glitch_filter_quantises_and_displaces_then_stacks_the_split_behind() {
        let fs = 60.5;
        let defs = glitch_filter_defs(fs, 42);
        assert!(defs.contains("id=\"freya-glitch\""), "{defs}");
        assert!(
            defs.contains("baseFrequency=\"0.02 0.75\""),
            "{defs}"
        );
        assert!(defs.contains("seed=\"42\""), "{defs}");
        assert!(
            defs.contains("tableValues=\"0 0.35 0.65 1\""),
            "{defs}"
        );
        assert!(
            defs.contains("slope=\"0\" intercept=\"0.5\""),
            "{defs}"
        );
        assert!(
            defs.contains("scale=\"9\" xChannelSelector=\"R\" yChannelSelector=\"G\" \
             result=\"displacedSrc\""),
            "{defs}"
        );
        assert!(defs.contains("result=\"displacedAlpha\""), "{defs}");
        let sx = SPLIT_X_EM * fs;
        assert!(
            defs.contains(&format!("dx=\"{sx}\" dy=\"0\" result=\"split-a-shape\"")),
            "{defs}"
        );
        assert!(
            defs.contains(&format!("dx=\"{n}\" dy=\"0\" result=\"split-b-shape\"", n = -sx)),
            "{defs}"
        );
        let a = defs.find("in=\"split-a\"").expect("ghost a merged");
        let b = defs.find("in=\"split-b\"").expect("ghost b merged");
        let s = defs.find("in=\"displacedSrc\"").expect("source merged on top");
        assert!(a < b && b < s, "ghosts behind the displaced source");
        // The 60 Hz seed advances with the clock.
        assert_eq!(tick_60(0.0), 0);
        assert_eq!(tick_60(1000.0), 60);
    }

    #[test]
    fn freya_segment_padding_inflates_the_filter_box() {
        let rect = padding_rect(100.0, 200.0, 300.0, 80.0, 55.0);
        assert!(rect.contains("x=\"45\""), "{rect}");
        assert!(rect.contains("y=\"145\""), "{rect}");
        assert!(rect.contains("width=\"410\""), "{rect}");
        assert!(rect.contains("height=\"190\""), "{rect}");
        assert!(rect.contains("fill-opacity=\"0\""), "{rect}");
        assert_eq!(PADDING_EM, 1.0);
        assert_eq!(ROTATION_DEG, -3.0);
        assert_eq!(LINE_SPACING_EM, -0.28);
    }

    #[test]
    fn freya_clause_segments_split_at_twenty_two_chars() {
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
    }
}
