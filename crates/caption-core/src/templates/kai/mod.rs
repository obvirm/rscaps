//! Native port of the tscaps `kai` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\kai\{style.scss,template.json,filters.svg}`.
//! Uppercase Bricolage Grotesque 800 at 5cqh: resting words sit dim
//! (#e8e8ef @ 55%); once a word starts narrating it turns white, lights
//! up with the `chroma-active` filter (two colour ghosts split
//! ±0.09/0.085em under a 0.025em black edge ring) and jitters
//! (`jitter` recipe: x 0.02em / y 0.012em, steps(2), over the word's own
//! narration window) — and it *stays* lit: `.word-already-narrated`
//! shares the same rule, so spoken words keep the white fill, the split
//! and the edge for the rest of the segment. Clause segments of 14
//! chars (min 0, descriptor default), one balanced row, block
//! top-anchored at 0.68H, `gap_free` holding the window open.
//!
//! The SVG emits `font-family="Bricolage Grotesque"` (the studio
//! `@font-face` name) while measuring resolves `FONTDB_FAMILY` — the
//! embedded face's name-table family — the lyra/mira split.
//!
//! Filter ports (standard SVG primitives, ids prefixed `kai-`):
//! `tscaps:chromatic-split` → two feOffset/feFlood/feComposite ghost
//! stacks (`-a` at (+x,+y), `-b` at (−x,−y)); `tscaps:outline` →
//! dilate/flood/composite ring. Unported: the `--tscaps-split-x/y`
//! *control* values win over the filter.svg `var()` fallbacks (0.045 /
//! 0.03), following the studio control-defaults convention.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from kai's style.scss + template.json) ───────────────
const FONT: &str = "Bricolage Grotesque"; // SVG font-family (studio @font-face)
const FONT_WEIGHT: u16 = 800; // fontWeight: 800
const FONT_SIZE_CQH: f32 = 0.05; // font-size: 5.0cqh
const LETTER_SPACING_EM: f32 = 0.01; // letter-spacing: 0.01em
const LINE_HEIGHT: f32 = 1.2; // line-height: null → font-native (1.2 stands in)
const WORD_SPACING_EM: f32 = 0.14; // @include word-layout(0.14em)
const LINE_SPACING_EM: f32 = 0.08; // .line + .line margin-top: 0.08em
const ANCHOR_Y_FRAC: f32 = 0.68; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalAlign default (center)
const MAX_CHARS: usize = 14; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // ...minChars (descriptor default; absent in json)
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#e8e8ef"; // primary-color default
const HIGHLIGHT: &str = "#ffffff"; // highlight-color default
const WORD_NOT_YET_OPACITY: f32 = 0.55; // .word { opacity: 0.55 }
// filters.svg `chroma-active`: split-x/y (control defaults; the filter
// fallbacks read 0.045 / 0.03) + the two ghost inks + the edge ring.
const SPLIT_X_EM: f32 = 0.09; // --tscaps-split-x default
const SPLIT_Y_EM: f32 = 0.085; // --tscaps-split-y default
const SPLIT_COLOR_A: &str = "#ff2e63"; // split-color-a default
const SPLIT_COLOR_B: &str = "#1fffd0"; // split-color-b default
const EDGE_THICKNESS_EM: f32 = 0.025; // outline-thickness default
const EDGE_COLOR: &str = "#000000"; // outline-color default
// jitter($element: 'word', $x: 0.02em, $y: 0.012em,
// $delay: var(--on-word-being-narrated-starts),
// $duration: var(--word-being-narrated-duration), $easing: steps(2, end))
const JITTER_X_EM: f32 = 0.02;
const JITTER_Y_EM: f32 = 0.012;

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
    force_uppercase: true, // kai always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// kai's `effects` array: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const BRICOLAGE_800_TTF: &[u8] =
    include_bytes!("../../../assets/bricolage-grotesque-latin-800-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Bricolage Grotesque 800.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face =
            fframes::ttf_parser::Face::parse(BRICOLAGE_800_TTF, 0).expect("Bricolage parses");
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

// ── chroma-active filter ──────────────────────────────────────────────────

/// The `chroma-active` filter body from filters.svg: the
/// `tscaps:chromatic-split` recipe (ghost B at (−x,−y), ghost A at
/// (+x,+y)) and the `tscaps:outline` recipe as the edge ring, merged
/// under SourceGraphic. Ghosts are pushed at px offsets resolved from em.
fn filter_defs(fs: f32) -> String {
    let sx = SPLIT_X_EM * fs;
    let sy = SPLIT_Y_EM * fs;
    let edge = EDGE_THICKNESS_EM * fs;
    // Ghost B mirrors the shift: the recipe writes `-{x}em`/`-{y}em`.
    let nsx = -sx;
    let nsy = -sy;
    format!(
        "<filter id=\"kai-chroma-active\" x=\"-30%\" y=\"-40%\" width=\"160%\" height=\"180%\" \
         color-interpolation-filters=\"sRGB\">\
         <feOffset in=\"SourceAlpha\" dx=\"{nsx}\" dy=\"{nsy}\" result=\"split-b-shape\"/>\
         <feFlood flood-color=\"{SPLIT_COLOR_B}\" result=\"split-b-ink\"/>\
         <feComposite in=\"split-b-ink\" in2=\"split-b-shape\" operator=\"in\" result=\"split-b\"/>\
         <feOffset in=\"SourceAlpha\" dx=\"{sx}\" dy=\"{sy}\" result=\"split-a-shape\"/>\
         <feFlood flood-color=\"{SPLIT_COLOR_A}\" result=\"split-a-ink\"/>\
         <feComposite in=\"split-a-ink\" in2=\"split-a-shape\" operator=\"in\" result=\"split-a\"/>\
         <feMorphology in=\"SourceAlpha\" operator=\"dilate\" radius=\"{edge}\" \
         result=\"edge-shape\"/>\
         <feFlood flood-color=\"{EDGE_COLOR}\" result=\"edge-ink\"/>\
         <feComposite in=\"edge-ink\" in2=\"edge-shape\" operator=\"in\" result=\"edge\"/>\
         <feMerge>\
         <feMergeNode in=\"split-b\"/>\
         <feMergeNode in=\"split-a\"/>\
         <feMergeNode in=\"edge\"/>\
         <feMergeNode in=\"SourceGraphic\"/>\
         </feMerge>\
         </filter>"
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
        BRICOLAGE_800_TTF,
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

    // Top-aligned block: the block's top edge sits at 0.68H; lines center
    // horizontally.
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let font_family_attr = stack::svg_font_family(&ov.font_family);
    let mut words_svg = format!(
        "<g font-family=\"{font_family_attr}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">",
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + ov.line_spacing_em * fs);
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
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{}\"", stack::svg_font_family(&family))
            };
            let narration = state::word_narration(now_ms, word.start_ms, word.end_ms);
            match narration {
                state::Narration::NotYet => {
                    words_svg.push_str(&format!(
                        "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\" \
                         opacity=\"{WORD_NOT_YET_OPACITY}\"{family_attr}>{text}</text>",
                        primary = ov.primary.as_str(),
                    ));
                }
                state::Narration::Being => {
                    // The jitter rides the word's own narration window
                    // (`--word-being-narrated-duration`).
                    let mut jitter = recipes::Jitter::with(JITTER_X_EM, JITTER_Y_EM);
                    jitter.duration_s = word.end_ms - word.start_ms;
                    let (jx_em, jy_em) = jitter.eval((now_ms - word.start_ms) / 1000.0);
                    let jx = jx_em * fs;
                    let jy = jy_em * fs;
                    words_svg.push_str(&format!(
                        "<text x=\"{x}\" y=\"{baseline}\" fill=\"{HIGHLIGHT}\" \
                         filter=\"url(#kai-chroma-active)\" transform=\"translate({jx},{jy})\"{family_attr}>\
                         {text}</text>"
                    ));
                }
                state::Narration::Already => {
                    // Same lit rule as Being — the jitter has finished and
                    // rests at (0, 0); the filter and the white stay.
                    words_svg.push_str(&format!(
                        "<text x=\"{x}\" y=\"{baseline}\" fill=\"{HIGHLIGHT}\" \
                         filter=\"url(#kai-chroma-active)\"{family_attr}>{text}</text>"
                    ));
                }
            }
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the block about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let block_h = lines.len() as f32 * line_h
            + lines.len().saturating_sub(1) as f32 * ov.line_spacing_em * fs;
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
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
            {words_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_bricolage_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(BRICOLAGE_800_TTF, 0).unwrap();
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1).
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
        // The TTF name-table family is the Bricolage packaging quirk;
        // the SVG emits `FONT` for the studio @font-face.
        assert_eq!(name.as_deref(), Some("Bricolage Grotesque 96pt ExtraBold"));
        assert_eq!(FONT, "Bricolage Grotesque");
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn chroma_active_splits_both_ways_and_rings_the_glyph_last() {
        let fs = 50.0;
        let defs = filter_defs(fs);
        assert!(defs.contains("id=\"kai-chroma-active\""), "{defs}");
        assert!(
            defs.contains("x=\"-30%\" y=\"-40%\" width=\"160%\" height=\"180%\""),
            "{defs}"
        );
        // Ghost B: (−x, −y); ghost A: (+x, +y) — control defaults in px.
        assert!(
            defs.contains(&format!("dx=\"{}\" dy=\"{}\" result=\"split-b-shape\"", -SPLIT_X_EM * fs, -SPLIT_Y_EM * fs)),
            "{defs}"
        );
        assert!(
            defs.contains(&format!("dx=\"{ax}\" dy=\"{ay}\" result=\"split-a-shape\"", ax = SPLIT_X_EM * fs, ay = SPLIT_Y_EM * fs)),
            "{defs}"
        );
        assert!(defs.contains(&format!("flood-color=\"{SPLIT_COLOR_A}\"")), "{defs}");
        assert!(defs.contains(&format!("flood-color=\"{SPLIT_COLOR_B}\"")), "{defs}");
        // The edge ring dilates the raw glyph and lands above both ghosts.
        assert!(
            defs.contains(&format!(
                "operator=\"dilate\" radius=\"{}\" result=\"edge-shape\"",
                EDGE_THICKNESS_EM * fs
            )),
            "{defs}"
        );
        let b = defs.find("in=\"split-b\"").expect("ghost b merged");
        let a = defs.find("in=\"split-a\"").expect("ghost a merged");
        let e = defs.find("in=\"edge\"").expect("edge merged");
        let s = defs.find("in=\"SourceGraphic\"").expect("source on top");
        assert!(b < a && a < e && e < s, "stack order b, a, edge, source");
    }

    #[test]
    fn jitter_snaps_through_its_stop_table_over_the_word_window() {
        let mut jitter = recipes::Jitter::with(JITTER_X_EM, JITTER_Y_EM);
        jitter.duration_s = 0.3;
        // Rest at the endpoints…
        assert_eq!(jitter.eval(0.0), (0.0, 0.0));
        assert_eq!(jitter.eval(0.3), (0.0, 0.0));
        // …and a hard snap to (−x, +y) inside the 20–40% step (steps2
        // holds the step's own value until the half-way cut).
        let (jx, jy) = jitter.eval(0.07); // p ≈ 0.23 → interval [0.2,0.4)
        assert!((jx - -JITTER_X_EM).abs() < 1e-6, "jx {jx}");
        assert!((jy - JITTER_Y_EM).abs() < 1e-6, "jy {jy}");
        assert_eq!(JITTER_X_EM, 0.02);
        assert_eq!(JITTER_Y_EM, 0.012);
    }

    #[test]
    fn kai_clause_segments_split_at_fourteen_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, hold on now and look at this one here.".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
        assert_eq!(MAX_LINES, 1, "kai is always one line");
        assert_eq!(WORD_NOT_YET_OPACITY, 0.55);
    }
}
