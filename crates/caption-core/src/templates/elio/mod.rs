//! Native port of the tscaps `elio` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\elio\{style.scss,template.json}`.
//! Big uppercase Bebas Neue on a centered block that always shows at least
//! two lines (`balanced-pixel-width` minLines 2), each word rising in from
//! below with a fade exactly when its narration starts (`rise-in`, 0.32s),
//! soft drop shadow, primary and highlight both `#ffffff` in the default
//! style.

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, recipes, stack, state, style};
use crate::CaptionDocument;

// ── spec constants (cqh/em values from elio's style.scss) ────────────────
const FONT: &str = "Bebas Neue";
const FONT_WEIGHT: u16 = 400;
const FONT_SIZE_CQH: f32 = 0.06; // font-size: 6.0cqh
const LETTER_SPACING_EM: f32 = 0.04; // letter-spacing: 0.04em
const LINE_HEIGHT: f32 = 1.0; // line-height: 1.0 (no inter-line gap)
const WORD_SPACING_EM: f32 = 0.13; // .word margin: 0 <spacing>
const ANCHOR_Y_FRAC: f32 = 0.5; // alignment.verticalOffset (center)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 80; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 4; // boundary_score_limit_by_chars.minChars
const MAX_GAP_MS: f32 = 1000.0; // gap_free default maxGapMs
const MIN_LINES: usize = 2; // lineSplitter.minLines
const MAX_LINES: usize = 6; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.77; // lineSplitter.maxWidthRatio
const SHADOW_DY_EM: f32 = 0.06; // soft-drop-shadow(0.06, 0.25)
const SHADOW_BLUR_EM: f32 = 0.25;
const RISE_DISTANCE_EM: f32 = 0.29; // rise-in $distance
const PRIMARY: &str = "#ffffff"; // primary-color default
const HIGHLIGHT: &str = "#ffffff"; // highlight-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // elio has no line-spacing (.line margin-top 0)
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: HIGHLIGHT,
    force_uppercase: true, // elio always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// elio's segment pipeline: sentence boundary → char limit → gap_free.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: MAX_GAP_MS,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────
// Latin face `fontStack: "bebas-neue"` plus the shared script stack.

const BEBAS_TTF: &[u8] = include_bytes!("../../../assets/bebas-neue-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Bebas Neue — drives the half-leading/baseline model and the exact
/// measurement scale (measure at size == upem → integer advances are font
/// units).
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(BEBAS_TTF, 0).expect("Bebas Neue parses");
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
    let st = style::Style::of(&doc.params, "elio");
    let highlight = st.color("highlight-color", HIGHLIGHT);
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs; // one side of `.word` margin
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        BEBAS_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        true,
    );
    let widths = measured.widths;

    // Line breaking: balanced-pixel-width with maxWidth = ratio × video.
    // The splitter measures lines with their word margins; positions and
    // the line total add them on top of the raw widths.
    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Block centered on (0.5W, 0.5H); `.line + .line` margin-top is 0.
    let block_h = lines.len() as f32 * line_h;
    let block_top = ov.anchor_y * video_h as f32 - block_h / 2.0;
    let center_x = ov.anchor_x * video_w as f32;

    // Half-leading model: line box = line-height, content area = ascent +
    // descent, baseline sits half the leading below the content-area top.
    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // `text-shadow: 0 0.06em 0.25em #000000` → feDropShadow (blur radius
    // 2σ).
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;

    let mut rise = recipes::RiseIn::with(RISE_DISTANCE_EM, 0.0);
    rise.distance_em = ov.distance_or(rise.distance_em);
    rise.duration_s = ov.duration_or(rise.duration_s);
    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" filter=\"url(#elio-shadow)\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for (word_off, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            // Element inspector: kata ber-override menahan anim entrance
            // template (rise-in) sendiri — override yang mengatur.
            let has_ov = element_css::has_override(&doc.params, word.index);
            let (opacity, dy_em) = if ov.motion_static || has_ov {
                (1.0, 0.0)
            } else {
                rise.eval((now_ms - word.start_ms) / 1000.0)
            };
            let y = baseline + dy_em * fs;
            let fill = if state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
                highlight.as_str()
            } else {
                ov.primary.as_str()
            };
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let wo_ctx = element_css::WordCtx {
                base_fs: fs,
                ls_px,
                video_w: video_w as f32,
                video_h: video_h as f32,
                center_x: *x + widths[start + word_off] / 2.0,
                center_y: baseline - fs * 0.35,
                word_start_s: word.start_ms / 1000.0,
                now_s: now_ms / 1000.0,
                font_family_svg: &stack::svg_font_family,
            };
            let wo = element_css::word_svg_attrs(&doc.params, word.index, &wo_ctx);
            let wo_transform = if has_ov {
                element_css::word_svg_transform(&doc.params, word.index, &wo_ctx)
                    .map(|t| format!(" transform=\"{t}\""))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{y}\" fill=\"{fill}\"{family_attr} opacity=\"{opacity}\"{wo}{wo_transform}>{text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the block about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    let words_svg = layout::dedupe_text_attrs(&words_svg);
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="elio-shadow" x="-30%" y="-30%" width="160%" height="160%">
                    <feDropShadow dx="0" dy={shadow_dy} stdDeviation={shadow_std} flood-color="#000000" flood-opacity={1.0}/>
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
    fn embedded_bebas_neue_has_expected_identity_and_metrics() {
        let face = fframes::ttf_parser::Face::parse(BEBAS_TTF, 0).unwrap();
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let mut typographic = None;
        let mut legacy = None;
        for name in face.names() {
            let Some(text) = name.to_string() else { continue };
            match name.name_id {
                fframes::ttf_parser::name_id::TYPOGRAPHIC_FAMILY => typographic = Some(text),
                fframes::ttf_parser::name_id::FAMILY => legacy = Some(text),
                _ => {}
            }
        }
        assert_eq!(typographic.or(legacy).unwrap(), FONT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn display_text_is_uppercased_but_timing_is_not() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "hello world".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert_eq!(segments.len(), 1);
        // Timings spread over the original (lowercase) text …
        assert_eq!(segments[0].words[0].text, "hello");
        // … while display runs uppercase.
        let upper: Vec<String> = segments[0]
            .words
            .iter()
            .map(|w| w.display.to_uppercase())
            .collect();
        assert_eq!(upper, vec!["HELLO", "WORLD"]);
    }

    #[test]
    fn elio_splitter_config_splits_long_sentences_at_80_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "This sentence is definitely longer than eighty characters so it must split right now."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
        for s in &segments {
            let chars: usize = s
                .words
                .iter()
                .map(|w| w.text.encode_utf16().count())
                .sum::<usize>()
                + s.words.len().saturating_sub(1);
            assert!(chars <= 80 + 40, "segment weight {chars} too big");
        }
    }

    #[test]
    fn line_splitter_uses_elio_bounds() {
        let splitter = layout::BalancedPixelWidth {
            min_lines: MIN_LINES,
            max_lines: MAX_LINES,
            max_width: (1920.0 * MAX_WIDTH_RATIO).round(),
        };
        assert_eq!(splitter.max_width, 1478.0);
        // Four short words: one line fits → minLines 2 forces a half.
        let lines = splitter.split(&[200.0, 200.0, 200.0, 200.0]);
        assert_eq!(lines, vec![(0, 2), (2, 4)]);
        // A single word can never be split into two lines.
        assert_eq!(splitter.split(&[500.0]), vec![(0, 1)]);
    }

    #[test]
    fn rise_in_is_active_during_narration() {
        // Word starts at 1s: hidden at 0.99s, animating at 1.16s, landed
        // at 1.33s.
        let rise = recipes::RiseIn::with(RISE_DISTANCE_EM, 0.0);
        let (before, _) = rise.eval(0.99 - 1.0);
        assert_eq!(before, 0.0);
        let (mid, _) = rise.eval(1.16 - 1.0);
        assert!(mid > 0.5);
        let (after, _) = rise.eval(1.33 - 1.0);
        assert_eq!(after, 1.0);
    }
}
