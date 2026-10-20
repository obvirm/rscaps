//! Native port of the tscaps `enzo` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\enzo\{style.scss,template.json}`.
//! Heavy Inter Black at 5.2cqh with tight tracking (-0.045em) on a tight
//! 0.92em leading; words fade in individually (0.16s) and a soft grey
//! shade (#333333) lifts *away* from the glyphs (dy −0.4em, blur 0.8em)
//! so the baseline stays crisp. Clause-boundary segments with `gap_free`
//! and `smart_punctuation`, block bottom-anchored at 0.72H.

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from enzo's style.scss + template.json) ───────────────
const FONT: &str = "Inter"; // fontStack inter ('Inter Variable' in scss)
const FONT_WEIGHT: u16 = 900; // fontWeight: 900
const FONT_SIZE_CQH: f32 = 0.052; // font-size: 5.2cqh
const LETTER_SPACING_EM: f32 = -0.045; // letter-spacing: -0.045em
const LINE_HEIGHT: f32 = 0.92; // line-height: 0.92em
const WORD_SPACING_EM: f32 = 0.12; // .word margin: 0 0.12em
const ANCHOR_Y_FRAC: f32 = 0.72; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 25; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 6; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 3; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.65; // lineSplitter.maxWidthRatio
const FADE_DURATION_S: f32 = 0.16; // fade-in($duration: 0.16s)
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
const TAG_COLOR: &str = "#ffffff"; // --tscaps-tag-color default
// text-shadow: 0 -0.4em 0.8em var(--tscaps-shadow-color, #333333)
const SHADOW_DY_EM: f32 = -0.4;
const SHADOW_BLUR_EM: f32 = 0.8;
const SHADOW_COLOR: &str = "#333333"; // --tscaps-shadow-color default
// -webkit-text-stroke: var(--tscaps-outline-thickness, 0em) $outline-color
const OUTLINE_THICKNESS_EM: f32 = 0.0;
const OUTLINE_COLOR: &str = "#000000"; // --tscaps-outline-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // enzo has no line-spacing (lines stack at line_h)
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: false, // enzo keeps the spoken case
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// enzo's `effects` array: `gap_free`, `smart_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::SmartPunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const INTER_TTF: &[u8] = include_bytes!("../../../assets/inter-latin-900-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Inter Black.
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
    let st = style::Style::of(&doc.params, "enzo");
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
        INTER_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Bottom-aligned block: the block's bottom edge sits at 0.72H; lines
    // center horizontally. lineSpacing is 0, so lines stack at line_h.
    let block_h = lines.len() as f32 * line_h;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Shadow geometry (px from em ratios): the shade floats *above* the
    // glyphs - dy is negative on purpose.
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;
    let stroke_px = st.f32("outline-thickness", OUTLINE_THICKNESS_EM) * fs;
    let tag_color = st.color("tag-color", TAG_COLOR);
    // Tag shadow (scss `.emphasis,.accent,.entity`): glow ketat warna tag +
    // dua lapis shade warna shadow-color. Ukuran font-scale/font-family
    // tag-nya Tier 2 (lihat catatan di atas).
    let tag_glow_std = 0.087 * fs / 2.0;
    let tag_haze_std = 0.368 * fs / 2.0;
    let tag_shade_dy = -0.4 * fs;
    let tag_shade_std = 0.8 * fs / 2.0;

    // Each word fades in on its own narration clock.
    let fade = recipes::FadeIn::with(0.0, ov.duration_or(FADE_DURATION_S));

    let group_attrs = format!(
        "font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" letter-spacing=\"{ls_px}\"",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    let mut base_svg = String::new();
    let mut tag_svg = String::new();
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
            // Element inspector: kata ber-override menahan fade-in
            // per-katanya sendiri — override yang mengatur.
            let has_ov = element_css::has_override(&doc.params, word.index);
            let opacity = if ov.motion_static || has_ov {
                1.0
            } else {
                fade.eval((now_ms - word.start_ms) / 1000.0)
            };
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            let text = layout::escape_xml(&word.display);
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
            let tagged =
                word.has_tag("emphasis") || word.has_tag("accent") || word.has_tag("entity");
            let word_svg = format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr} \
                 stroke=\"{outline_color}\" stroke-width=\"{stroke_px}\" \
                 paint-order=\"stroke\" opacity=\"{opacity}\"{wo}{wo_transform}>{text}</text>",
                primary = if tagged { tag_color.as_str() } else { ov.primary.as_str() },
            );
            if tagged {
                tag_svg.push_str(&word_svg);
            } else {
                base_svg.push_str(&word_svg);
            }
        }
    }
    let mut words_svg = format!(
        "<g {attrs} filter=\"url(#enzo-shadow)\">{base_svg}</g>\
         <g {attrs} filter=\"url(#enzo-tag-shadow)\">{tag_svg}</g>",
        attrs = group_attrs,
    );

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
                <filter id="enzo-shadow" x="-100%" y="-300%" width="300%" height="500%">
                    <feDropShadow dx={0.0} dy={shadow_dy} stdDeviation={shadow_std} flood-color={SHADOW_COLOR} flood-opacity={1.0}/>
                </filter>
                <filter id="enzo-tag-shadow" x="-100%" y="-300%" width="300%" height="500%">
                    <feDropShadow dx={0.0} dy={0.0} stdDeviation={tag_glow_std} flood-color={tag_color.as_str()} flood-opacity={1.0} result="t1"/>
                    <feDropShadow in="t1" dx={0.0} dy={0.0} stdDeviation={tag_haze_std} flood-color={SHADOW_COLOR} flood-opacity={1.0} result="t2"/>
                    <feDropShadow in="t2" dx={0.0} dy={tag_shade_dy} stdDeviation={tag_shade_std} flood-color={SHADOW_COLOR} flood-opacity={1.0}/>
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
    fn embedded_inter_black_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(INTER_TTF, 0).unwrap();
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1) — this is the
        // string `resolve_font` matches, and the TTF ships ID 1 as
        // "Inter Black".
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
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn enzo_clause_boundary_with_tight_char_limit() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, hold on a second and look at this one.".into(),
                sheet: None,
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        // Clause split at the comma; the long tail then splits on the
        // 25-char limit.
        assert_eq!(texts[0], "Wait,");
        assert!(segments.len() >= 3, "{texts:?}");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars} ({s:?})");
        }
    }

    #[test]
    fn shade_lifts_upward_and_fades_in_sixteen_hundredths() {
        assert!(SHADOW_DY_EM < 0.0, "the shade is thrown upward");
        let fade = recipes::FadeIn::with(0.0, FADE_DURATION_S);
        assert_eq!(fade.eval(-0.1), 0.0);
        assert!(fade.eval(0.08) > 0.5);
        assert!((fade.eval(0.16) - 1.0).abs() < 1e-6);
    }
}
