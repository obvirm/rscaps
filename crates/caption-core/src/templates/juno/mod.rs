//! Native port of the tscaps `juno` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\juno\{style.scss,template.json}`.
//! Italic uppercase Bungee at 3.91cqh with a 0.02em black stroke, a
//! six-layer hard black shadow (depth 1.2) and a yellow `#ffdd00`
//! spoken word. The whole segment `pop-in`s (0.1, 0.35s) from when it
//! starts. Clause segments of 25 chars (no minimum), up to two rows of
//! 0.56W, block top-anchored at 0.75H.

use fframes::{FFramesContext, Svgr};

use super::{layout, overrides, pipeline, recipes, stack, state, style};
use crate::CaptionDocument;

// ── spec constants (from juno's style.scss + template.json) ───────────────
const FONT: &str = "Bungee"; // fontStack bungee
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.0391; // font-size: 3.91cqh
const LINE_HEIGHT: f32 = 1.05; // line-height: 1.05
const WORD_SPACING_EM: f32 = 0.12; // .word margin: 0 0.12em
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalAlign default (center)
const MAX_CHARS: usize = 25; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // boundary_score_limit_by_chars.minChars default
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.56; // lineSplitter.maxWidthRatio
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top default
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
const HIGHLIGHT: &str = "#ffdd00"; // --tscaps-highlight-color default
const SHADOW_COLOR: &str = "#000000"; // --tscaps-shadow-color default
const STROKE_EM: f32 = 0.02; // --tscaps-stroke-width default (control 0.02)
const SHADOW_DEPTH: f32 = 1.2; // --tscaps-shadow-depth default (control 1.2)
// text-shadow: six zero-blur casts at ±0.02em…0.12em × depth, CSS order
// (first listed paints on top; each SVG link sits behind the previous).
const SHADOW_OFFSETS_EM: [f32; 6] = [-0.02, 0.04, 0.06, 0.08, 0.1, 0.12];
const POP_AMOUNT: f32 = 0.1; // pop-in($amount: 0.1)

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: 0.0, // juno has no letter-spacing
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: LINE_SPACING_EM,
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: HIGHLIGHT,
    force_uppercase: true, // juno always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// juno declares no `effects` array — windows stay the raw word span.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const BUNGEE_TTF: &[u8] = include_bytes!("../../../assets/bungee-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Bungee.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(BUNGEE_TTF, 0).expect("Bungee parses");
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
    let st = style::Style::of(&doc.params, "juno");
    let highlight = st.color("highlight-color", HIGHLIGHT);
    let stroke_em = st.f32("stroke-width", STROKE_EM);
    let shadow_depth = st.f32("shadow-depth", SHADOW_DEPTH);
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) = line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        BUNGEE_TTF,
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
    // horizontally, and the whole segment box scales about its own centre
    // (CSS `transform-origin: 50% 50%` of the full-width `.segment`).
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;
    let block_h = lines.len() as f32 * line_h + (lines.len().saturating_sub(1)) as f32 * ov.line_spacing_em * fs;
    let block_cy = block_top + block_h / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let mut pop = recipes::PopIn::with(POP_AMOUNT);
    pop.duration_s = ov.duration_or(pop.duration_s);
    let scale = if ov.motion_static {
        1.0
    } else {
        pop.eval((now_ms - segment.start_ms) / 1000.0)
    };
    let stroke_px = stroke_em * fs;
    let shadow_px: Vec<f32> = SHADOW_OFFSETS_EM
        .iter()
        .map(|o| o * shadow_depth * fs)
        .collect();

    let mut shadow_svg = String::new();
    for (i, &d) in shadow_px.iter().enumerate() {
        let attr = if i == 0 {
            String::new()
        } else {
            format!(" in=\"s{i}\"")
        };
        shadow_svg.push_str(&format!(
            "<feDropShadow{attr} dx=\"{d}\" dy=\"{d}\" stdDeviation=\"0\" \
             flood-color=\"{SHADOW_COLOR}\" flood-opacity=\"1.0\" result=\"s{}\"/>",
            i + 1
        ));
    }

    let mut words_svg = format!(
        "<g transform=\"translate({center_x} {block_cy}) scale({scale}) \
         translate({} {})\" font-family=\"{font}\" font-weight=\"{weight}\" \
         font-style=\"italic\" font-size=\"{fs}\" letter-spacing=\"{ls_px}\">",
        -center_x,
        -block_cy,
        font = ov.font_family,
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
            let fill = if state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
                highlight.as_str()
            } else {
                ov.primary.as_str()
            };
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\" stroke=\"{SHADOW_COLOR}\" \
                 stroke-width=\"{stroke_px}\" paint-order=\"stroke\" \
                 filter=\"url(#juno-shadow)\"{family_attr}>{text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the popped block about its own centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_cy;
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
                <filter id="juno-shadow" x="-50%" y="-50%" width="220%" height="220%">
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
    fn embedded_bungee_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(BUNGEE_TTF, 0).unwrap();
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
    fn juno_splitter_is_clause_plus_twenty_five_chars_without_a_minimum() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, hold on a second and look at this one right now please."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert_eq!(MIN_CHARS, 0);
        assert!(segments.len() >= 3, "{segments:?}");
        // Clause boundary closes at the comma, not before it.
        assert_eq!(segments[0].words[0].text, "Wait,");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
    }

    #[test]
    fn pop_in_overshoots_the_segment_then_settles() {
        let pop = recipes::PopIn::with(POP_AMOUNT);
        assert!((pop.eval(-1.0) - 1.0).abs() < 1e-6);
        // Peak at 40% of the mixin's 0.35s.
        assert!((pop.eval(0.35 * 0.4) - (1.0 + POP_AMOUNT)).abs() < 1e-5);
        assert!((pop.eval(0.35) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn only_the_spoken_word_takes_the_highlight() {
        assert_eq!(state::word_narration(150.0, 100.0, 200.0), state::Narration::Being);
        let being = state::word_narration(150.0, 100.0, 200.0).is_being();
        let not_yet = state::word_narration(50.0, 100.0, 200.0).is_being();
        let already = state::word_narration(250.0, 100.0, 200.0).is_being();
        assert!(being && !not_yet && !already);
        assert_eq!(HIGHLIGHT, "#ffdd00");
        assert_eq!(SHADOW_OFFSETS_EM.len(), 6);
        assert_eq!(uppercase("loud words"), "LOUD WORDS");
    }
}
