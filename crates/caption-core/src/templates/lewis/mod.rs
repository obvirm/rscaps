//! Native port of the tscaps `lewis` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\lewis\{style.scss,template.json}`.
//! One word at a time: Gabarito 900 at 4.92cqh with a 0.1em black
//! outline (`paint-order: stroke`) and `lit-from-behind` — the halo and
//! shade pair laid down twice. The whole segment `scale-in`s from 0.909
//! over 0.1s on an overshooting curve, about the full-width segment box.
//! `limit_by_words: 1` means every segment is a single word on a single
//! row, top-anchored at 0.64H.

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from lewis's style.scss + template.json) ──────────────
const FONT: &str = "Gabarito"; // fontStack gabarito ('Gabarito Variable' in scss)
const FONT_WEIGHT: u16 = 900; // fontWeight: 900
const FONT_SIZE_CQH: f32 = 0.0492; // font-size: 4.92cqh
const LETTER_SPACING_EM: f32 = 0.0; // letterSpacing: 0
const LINE_HEIGHT: f32 = 1.2; // line-height: 1.2
const WORD_SPACING_EM: f32 = 0.13; // @include word-layout(0.13em)
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top: 0
const ANCHOR_Y_FRAC: f32 = 0.64; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
// `segmentSplitters: [{ type: "limit_by_words", maxWords: 1 }]` → one word
// per segment. Expressed as an uncapped boundary pass: the char limit
// runs with max 0, so every cut takes exactly one word.
const MAX_CHARS: usize = 0; // limit_by_words.maxWords: 1
const MIN_CHARS: usize = 0;
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
const OUTLINE_THICKNESS_EM: f32 = 0.1; // --tscaps-outline-thickness default
const OUTLINE_COLOR: &str = "#000000"; // --tscaps-outline-color default
const SHADOW_COLOR: &str = "#000000"; // --tscaps-shadow-color default
// @mixin lit-from-behind($halo): halo, shade, halo, shade — CSS order,
// each later link painting behind the previous.
const HALO_BLUR_EM: f32 = 0.127; // 0 0 0.127em $halo
const SHADE_DY_EM: f32 = 0.111; // 0 0.111em 0.159em $shadow
const SHADE_BLUR_EM: f32 = 0.159;
// scale-in($element: 'segment', $from: 0.909, $duration: 0.1s,
//          $easing: cubic-bezier(0.1, 0.01, 0, 1.14)) — $from-opacity
// rests at 1, so the entrance is a pure size change.
const FROM_SCALE: f32 = 0.909;
const SCALE_DURATION_S: f32 = 0.1;

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
    force_uppercase: false, // lewis keeps the spoken case
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// lewis's `effects` array: `gap_free`, `remove_punctuation` (`emoji`
/// skipped — decorations need data the word stream does not carry).
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::None,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[
        effects::Effect::GapFree,
        effects::Effect::RemovePunctuation,
    ],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const GABARITO_TTF: &[u8] = include_bytes!("../../../assets/gabarito-latin-900-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Gabarito Black.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(GABARITO_TTF, 0).expect("Gabarito parses");
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

/// The entrance's own curve: slow start, a tenth of overshoot, settled.
fn ease_lewis(x: f32) -> f32 {
    (recipes::cubic_bezier(0.1, 0.01, 0.0, 1.14))(x)
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
    let st = style::Style::of(&doc.params, "lewis");
    let outline_color = st.color("outline-color", OUTLINE_COLOR);
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
        GABARITO_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let lines = splitter.split(&split_widths);

    // Top-aligned block: the block's top edge sits at 0.64H; lines center
    // horizontally. The entrance scales the full-width segment box about
    // its own centre.
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * ov.line_spacing_em * fs;
    let block_cy = block_top + block_h / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let entrance = recipes::ScaleIn {
        from_scale: FROM_SCALE,
        from_opacity: 1.0,
        peak: None,
        peak_at: 0.6,
        duration_s: ov.duration_or(SCALE_DURATION_S),
        ease: ease_lewis,
    };
    let (opacity, scale) = if ov.motion_static {
        (1.0, 1.0)
    } else {
        entrance.eval((now_ms - segment.start_ms) / 1000.0)
    };

    let stroke_px = st.f32("outline-thickness", OUTLINE_THICKNESS_EM) * fs;
    let halo_std = HALO_BLUR_EM * fs / 2.0;
    let shade_dy = SHADE_DY_EM * fs;
    let shade_std = SHADE_BLUR_EM * fs / 2.0;

    let mut words_svg = format!(
        "<g transform=\"translate({center_x} {block_cy}) scale({scale}) \
         translate({} {})\" opacity=\"{opacity}\" font-family=\"{font}\" \
         font-weight=\"{weight}\" font-size=\"{fs}\">",
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
        for (word_off, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            let text = layout::escape_xml(&word.display);
            // Element inspector: override per-kata (attrs + transform).
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
            let wo_t = element_css::word_svg_transform(&doc.params, word.index, &wo_ctx);
            let wo_transform = wo_t
                .map(|t| format!(" transform=\"{t}\""))
                .unwrap_or_default();
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr} \
                 stroke=\"{outline_color}\" stroke-width=\"{stroke_px}\" \
                 paint-order=\"stroke\" filter=\"url(#lewis-lit)\"{wo}{wo_transform}>{text}</text>",
                primary = ov.primary.as_str(),
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the scaled word about the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_cy;
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
                <filter id="lewis-lit" x="-50%" y="-60%" width="200%" height="260%">
                    <feDropShadow dx={0.0} dy={0.0} stdDeviation={halo_std} flood-color={SHADOW_COLOR} flood-opacity={1.0} result="h1"/>
                    <feDropShadow in="h1" dx={0.0} dy={shade_dy} stdDeviation={shade_std} flood-color={SHADOW_COLOR} flood-opacity={1.0} result="s1"/>
                    <feDropShadow in="s1" dx={0.0} dy={0.0} stdDeviation={halo_std} flood-color={SHADOW_COLOR} flood-opacity={1.0} result="h2"/>
                    <feDropShadow in="h2" dx={0.0} dy={shade_dy} stdDeviation={shade_std} flood-color={SHADOW_COLOR} flood-opacity={1.0}/>
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
    fn embedded_gabarito_black_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(GABARITO_TTF, 0).unwrap();
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
        assert_eq!(FONT_WEIGHT, 900);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn lewis_shows_one_word_at_a_time() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "no commas, no stops — just words".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 4, "{segments:?}");
        for s in &segments {
            assert_eq!(s.words.len(), 1, "expected one word per segment");
        }
        // `remove_punctuation` strips the trailing dash from display form
        // while `text` keeps it.
        assert!(segments
            .iter()
            .any(|s| s.words[0].text == "words" && s.words[0].display == "words"));
    }

    #[test]
    fn scale_in_runs_from_0_909_and_settles_at_1() {
        let entrance = recipes::ScaleIn {
            from_scale: FROM_SCALE,
            from_opacity: 1.0,
            peak: None,
            peak_at: 0.6,
            duration_s: SCALE_DURATION_S,
            ease: ease_lewis,
        };
        let (opacity, start) = entrance.eval(-1.0);
        assert!((start - FROM_SCALE).abs() < 1e-6);
        assert_eq!(opacity, 1.0);
        let (_, end) = entrance.eval(SCALE_DURATION_S);
        assert!((end - 1.0).abs() < 1e-6);
        // The curve carries the pop: it passes the resting size before
        // coming back.
        let peak = (0..400)
            .map(|i| entrance.eval(SCALE_DURATION_S * i as f32 / 400.0).1)
            .fold(0.0f32, f32::max);
        assert!(peak > 1.0, "no overshoot, peak {peak}");
    }

    #[test]
    fn lit_from_behind_paints_the_pair_twice() {
        assert_eq!(HALO_BLUR_EM, 0.127);
        assert_eq!(SHADE_DY_EM, 0.111);
        assert_eq!(SHADE_BLUR_EM, 0.159);
        assert_eq!(OUTLINE_THICKNESS_EM, 0.1);
        assert_eq!(LINE_HEIGHT, 1.2);
        assert_eq!(MAX_LINES, 1);
    }
}
