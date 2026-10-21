//! Native port of the tscaps `sara` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\sara\{style.scss,template.json}`.
//! IM Fell English at 4.92cqh with tight tracking (-0.048em); words fade
//! in individually (0.16s), and a three-layer shadow — a white bloom, a
//! fixed grey haze, then a soft black shade thrown down-right — lifts the
//! line off the picture. Clause-boundary segments with `gap_free` and
//! `smart_punctuation`.

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, recipes, stack, style, tagstyle};
use crate::CaptionDocument;

// ── spec constants (from sara's style.scss + template.json) ───────────────
const FONT: &str = "IM FELL English"; // name-table family (fontdb is exact-case)
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.0492; // font-size: 4.92cqh
const LETTER_SPACING_EM: f32 = -0.048; // letter-spacing: -0.048em
const LINE_HEIGHT: f32 = 1.0; // line-height: 1
const WORD_SPACING_EM: f32 = 0.11; // .word margin: 0 0.11em
const ANCHOR_Y_FRAC: f32 = 0.62; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 34; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 9; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 3; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.6; // lineSplitter.maxWidthRatio
const FADE_DURATION_S: f32 = 0.16; // fade-in($duration: 0.16s)
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
// text-shadow: 0 0 0.286em $primary, 0 0 0.159em #6a6a6a,
//              0.084em 0.084em 0.238em shadow @ 50%
const BLOOM_BLUR_EM: f32 = 0.286;
const HAZE_BLUR_EM: f32 = 0.159;
const HAZE_COLOR: &str = "#6a6a6a";
const SHADE_DX_EM: f32 = 0.084;
const SHADE_DY_EM: f32 = 0.084;
const SHADE_BLUR_EM: f32 = 0.238;
const SHADE_ALPHA: f32 = 0.5;
const SHADOW_COLOR: &str = "#000000"; // --tscaps-shadow-color default
const TAG_COLOR: &str = "#ffffff"; // --tscaps-tag-color default
// -webkit-text-stroke: var(--tscaps-outline-thickness, 0em) $outline-color
const OUTLINE_THICKNESS_EM: f32 = 0.0;
const OUTLINE_COLOR: &str = "#000000"; // --tscaps-outline-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // no LINE_SPACING_EM in sara's spec
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: false,
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// sara's `effects` array: `gap_free`, `smart_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::SmartPunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const IM_FELL_TTF: &[u8] =
    include_bytes!("../../../assets/im-fell-english-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// IM Fell English.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face =
            fframes::ttf_parser::Face::parse(IM_FELL_TTF, 0).expect("IM Fell English parses");
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
    let st = style::Style::of(&doc.params, "sara");
    let outline_color = st.color("outline-color", OUTLINE_COLOR);
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    // Tag Tier-2 (tag-font-scale): kata ber-tag membesar — ukur dengan
    // advance dasar × scale (face dasar tetap).
    let tt = tagstyle::resolve(&st, "sara", FONT, ov.font_weight);
    let types: Vec<Option<stack::WordType>> = segment
        .words
        .iter()
        .map(|w| tagstyle::word_type("sara", &tt, w, FONT, ov.font_weight))
        .collect();

    let measured = stack::measure_words_typed(
        &segment.words,
        &script_stack,
        IM_FELL_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
        &types,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Top-aligned block: the block's top edge sits at 0.62H; lines center
    // horizontally. Kata ber-scale membesarkan box line-nya (line-box CSS).
    let word_scale = |i: usize| types.get(i).copied().flatten().map(|t| t.scale).unwrap_or(1.0);
    let line_geoms: Vec<(f32, f32)> = lines
        .iter()
        .map(|&(s, e)| {
            let scale = (s..e).map(word_scale).fold(1.0f32, f32::max);
            let line_h = LINE_HEIGHT * fs * scale;
            let ascent = asc_ratio * fs * scale;
            let descent = desc_ratio * fs * scale;
            (line_h, (line_h - (ascent + descent)) / 2.0 + ascent)
        })
        .collect();
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;

    // Shadow geometry (px from em ratios).
    let bloom_std = BLOOM_BLUR_EM * fs / 2.0;
    let haze_std = HAZE_BLUR_EM * fs / 2.0;
    let shade_dx = SHADE_DX_EM * fs;
    let shade_dy = SHADE_DY_EM * fs;
    let shade_std = SHADE_BLUR_EM * fs / 2.0;
    let tag_color = st.color("tag-color", TAG_COLOR);
    // Tag shadow blur dibagi scale (scss `calc(x / var(--tag-font-scale))`)
    // supaya glow konstan saat kata membesar. Tscaps memakai scale kata
    // masing-masing (text-shadow per kata); filter grup satu radius — pakai
    // scale maksimum kata ber-tag.
    let tag_scale_max = types
        .iter()
        .flatten()
        .map(|t| t.scale)
        .fold(1.0f32, f32::max);
    let tag_bloom_std = bloom_std / tag_scale_max;
    let tag_haze_std = haze_std / tag_scale_max;
    let stroke_px = st.f32("outline-thickness", OUTLINE_THICKNESS_EM) * fs;

    // Each word fades in on its own narration clock.
    let fade = recipes::FadeIn::with(0.0, ov.duration_or(FADE_DURATION_S));

    // Tag `.emphasis/.accent/.entity` tscaps: kata ber-tag dapat warna +
    // glow tag-color sendiri (menggantikan shadow dasar). Dua grup filter
    // terpisah - tanpa filter per-kata, tanpa nested glow.
    let group_attrs = format!(
        "font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" letter-spacing=\"{ls_px}\"",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    let mut base_svg = String::new();
    let mut tag_svg = String::new();
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top: f32 = line_geoms[..line_idx].iter().map(|(lh, _)| lh).sum();
        let baseline = line_top + line_geoms[line_idx].1;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for (i, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            let w_scale = word_scale(start + i);
            // Kata ber-override menahan fade sendiri — paritas tscaps
            // "animation custom menggantikan animasi template".
            let opacity = if ov.motion_static
                || element_css::has_override(&doc.params, word.index)
            {
                1.0
            } else {
                fade.eval((now_ms - word.start_ms) / 1000.0)
            };
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            // Tag Tier-2 — sebelum {wo}; element inspector menang via
            // dedupe_text_attrs. Kata script mengabaikan tipografi tag.
            let tag_attrs = match types[start + i] {
                Some(ref ty) if *family == FONT => {
                    tagstyle::svg_attrs(ty, fs, FONT, ov.font_weight)
                }
                _ => String::new(),
            };
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            // Element inspector: override per-kata (attrs + transform).
            let wo_ctx = element_css::WordCtx {
                base_fs: fs * w_scale,
                ls_px,
                video_w: video_w as f32,
                video_h: video_h as f32,
                center_x: *x + widths[start + i] / 2.0,
                center_y: baseline - fs * 0.35,
                word_start_s: word.start_ms / 1000.0,
                now_s: now_ms / 1000.0,
                font_family_svg: &stack::svg_font_family,
            };
            let wo = element_css::word_svg_attrs(&doc.params, word.index, &wo_ctx);
            let wo_transform = element_css::word_svg_transform(&doc.params, word.index, &wo_ctx)
                .map(|t| format!(" transform=\"{t}\""))
                .unwrap_or_default();
            let tagged =
                word.has_tag("emphasis") || word.has_tag("accent") || word.has_tag("entity");
            let word_svg = format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}{tag_attrs} \
                 stroke=\"{outline_color}\" stroke-width=\"{stroke_px}\" \
                 paint-order=\"stroke\" opacity=\"{opacity}\"{wo}{wo_transform}>{text}</text>",
                fill = if tagged { tag_color.as_str() } else { ov.primary.as_str() },
            );
            if tagged {
                tag_svg.push_str(&word_svg);
            } else {
                base_svg.push_str(&word_svg);
            }
        }
    }
    let mut words_svg = format!(
        "<g {attrs} filter=\"url(#sara-shadow)\">{base_svg}</g>\
         <g {attrs} filter=\"url(#sara-tag-shadow)\">{tag_svg}</g>",
        attrs = group_attrs,
    );

    // User rotation wraps the caption around the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + line_geoms.iter().map(|(lh, _)| lh).sum::<f32>() / 2.0;
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
                <filter id="sara-shadow" x="-30%" y="-60%" width="180%" height="260%">
                    <feDropShadow dx={0.0} dy={0.0} stdDeviation={bloom_std} flood-color={ov.primary.as_str()} flood-opacity={1.0} result="s1"/>
                    <feDropShadow in="s1" dx={0.0} dy={0.0} stdDeviation={haze_std} flood-color={HAZE_COLOR} flood-opacity={1.0} result="s2"/>
                    <feDropShadow in="s2" dx={shade_dx} dy={shade_dy} stdDeviation={shade_std} flood-color={SHADOW_COLOR} flood-opacity={SHADE_ALPHA}/>
                </filter>
                <filter id="sara-tag-shadow" x="-30%" y="-60%" width="180%" height="260%">
                    <feDropShadow dx={0.0} dy={0.0} stdDeviation={tag_bloom_std} flood-color={tag_color.as_str()} flood-opacity={1.0} result="s1"/>
                    <feDropShadow in="s1" dx={0.0} dy={0.0} stdDeviation={tag_haze_std} flood-color={HAZE_COLOR} flood-opacity={1.0} result="s2"/>
                    <feDropShadow in="s2" dx={shade_dx} dy={shade_dy} stdDeviation={shade_std} flood-color={SHADOW_COLOR} flood-opacity={SHADE_ALPHA}/>
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
    fn embedded_im_fell_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(IM_FELL_TTF, 0).unwrap();
        let mut name = None;
        for n in face.names() {
            if n.name_id == fframes::ttf_parser::name_id::FAMILY {
                name = n.to_string();
                break;
            }
        }
        assert_eq!(name.as_deref(), Some(FONT));
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn sara_clause_boundary_splits_at_commas() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, what now. Fine, then!".into(),
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
        assert_eq!(texts, vec!["Wait,", "what now.", "Fine,", "then!"]);
    }

    #[test]
    fn sara_smart_punctuation_typography() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "wait... \"ok\" then -- bye".into(),
                sheet: None,
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let displays: Vec<&str> = segments
            .iter()
            .flat_map(|s| s.words.iter().map(|w| w.display.as_str()))
            .collect();
        assert!(displays.contains(&"wait\u{2026}"), "{displays:?}");
        assert!(displays.contains(&"\u{201C}ok\u{201D}"), "{displays:?}");
        assert!(displays.contains(&"\u{2014}"), "{displays:?}");
        assert!(displays.contains(&"bye"), "{displays:?}");
    }

    #[test]
    fn words_fade_in_over_sixteen_hundredths() {
        let fade = recipes::FadeIn::with(0.0, FADE_DURATION_S);
        assert_eq!(fade.eval(-0.1), 0.0);
        assert!(fade.eval(0.08) > 0.5);
        assert!((fade.eval(0.16) - 1.0).abs() < 1e-6);
    }
}
