//! Native port of the tscaps `sol` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\sol\{style.scss,template.json}`.
//! Two lines pinned at 0.74H: a light Montserrat 300 line at 4cqh over a
//! 1.5× handwritten accent line in `#f9d700` tucked up by -0.3em, its
//! words carrying a four-layer glow (two `#ffd50070` blurs, a contact
//! shadow, an upward `#33333390` bounce). The segment rises through
//! `tscaps-sol-rise-in` (0.25s, opacity lit by 10%, `cubic-bezier(0,
//! 0.85, 0.4, 1)`); the accent line's words pop individually on their
//! narration start (`tscaps-sol-pop-in`, 0.2s, `cubic-bezier(0.16, 1,
//! 0.3, 1)`).

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from sol's style.scss + template.json) ────────────────
const FONT: &str = "Montserrat"; // fontStack montserrat (scss: 'Montserrat Variable')
const FONT_WEIGHT: u16 = 300; // typography.fontWeight: 300
// `.line + .line` font-family: var(--tscaps-tag-font-family, 'Kalam') —
// Kalam is neither embedded here nor registered in the studio's
// @font-face table, so the accent line borrows the handwriting face that
// is: Caveat 700 (studio/main.js).
const TAG_FONT: &str = "Caveat"; // tag-font-family control default: "kalam"
const TAG_FONT_WEIGHT: u16 = 700; // tag-font-weight control default
const FONT_SIZE_CQH: f32 = 0.04; // font-size: 4.0cqh
const LETTER_SPACING_EM: f32 = -0.04; // letter-spacing: -0.04em
const LINE_HEIGHT: f32 = 1.15; // line-height: 1.15
const WORD_SPACING_EM: f32 = 0.12; // word-layout(0.12em) → .word margin
// `.line + .line` margin-top: -0.3em — `em` resolves against that line's
// own font-size, which the same rule just set to 1.5em.
const LINE_SPACING_EM: f32 = -0.3; // typography.lineSpacing: -0.3
const TAG_LINE_SCALE: f32 = 1.5; // .line + .line font-size: 1.5em
const ANCHOR_Y_FRAC: f32 = 0.74; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 30; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 6; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 2; // lineSplitter.minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.75; // lineSplitter.maxWidthRatio
const PRIMARY: &str = "#ffffff"; // primary-color control default
const HIGHLIGHT: &str = "#f9d700"; // highlight-color control default
const GLOW_COLOR: &str = "#ffd500"; // glow-color control default (#ffd50070)
const GLOW_ALPHA: f32 = 0.439215_7; // 0x70 / 255
const GLOW_RADIUS_EM: f32 = 0.5; // glow-radius control default
// `.line + .line` text-shadow, layer by layer (em against the accent
// line's font-size; CSS blur r → σ = r / 2):
//   0 0 0.5em #ffd50070
//   0 0 calc(0.5em * 2.5) #ffd50070
//   0 0.04em 0.18em rgba(0, 0, 0, 0.5)
//   0 -0.4em 0.8em #33333390
const CONTACT_DY_EM: f32 = 0.04;
const CONTACT_BLUR_EM: f32 = 0.18;
const BOUNCE_DY_EM: f32 = -0.4;
const BOUNCE_BLUR_EM: f32 = 0.8;
const BOUNCE_COLOR: &str = "#333333"; // ...#33333390
const BOUNCE_ALPHA: f32 = 0.564705_9; // 0x90 / 255
// tscaps-sol-rise-in (`.segment`): translateY + blur over the full 0.25s,
// opacity 0 → 1 across the 0-10% stop.
const RISE_DISTANCE_EM: f32 = 1.2; // rise-distance control default
const RISE_DURATION_S: f32 = 0.25; // rise-duration control default
const RISE_BLUR_EM: f32 = 0.5; // keyframes filter: blur(0.5em)
const RISE_OPACITY_HOLD: f32 = 0.1; // keyframes 10% stop
// tscaps-sol-pop-in (`.line + .line .word`).
const POP_SCALE: f32 = 0.1; // word-pop-scale control default
const POP_DURATION_S: f32 = 0.2; // word-pop-duration control default
const POP_HOLD: f32 = 0.3; // keyframes 30% stop
const POP_START_SCALE: f32 = 0.65; // keyframes 0% scale
const POP_TY_EM: f32 = 0.1; // keyframes 0% translateY(0.1em)
const POP_BLUR_EM: f32 = 0.3; // keyframes 0% blur(0.3em)

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
    force_uppercase: false,
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// sol's `segmentSplitters`: clause + 30/6 chars; `gap_free` then
/// `smart_punctuation` (`...` → `…`, quotes → “ ”) in json order.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::SmartPunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const MONTSERRAT_300_TTF: &[u8] = include_bytes!("../../../assets/montserrat-latin-300-normal.ttf");
const CAVEAT_700_TTF: &[u8] = include_bytes!("../../../assets/caveat-latin-700-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem for an embedded
/// TTF. The ascent/descent pair also centres the accent words' pop scale.
fn face_metrics(ttf: &[u8]) -> (f32, f32, f32) {
    let face = fframes::ttf_parser::Face::parse(ttf, 0).expect("embedded face parses");
    let upem = face.units_per_em() as f32;
    (
        face.ascender() as f32 / upem,
        face.descender().abs() as f32 / upem,
        upem,
    )
}

/// `(ascent, descent, upem)` for the light segment face.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| face_metrics(MONTSERRAT_300_TTF))
}

/// `(ascent, descent, upem)` for the handwritten accent face.
fn tag_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| face_metrics(CAVEAT_700_TTF))
}

/// Baseline metrics for whichever family dominates the segment (main
/// register: Montserrat; accent: Caveat — both go through the stack for
/// script fallback).
fn line_metrics(dominant: &str) -> (f32, f32, f32) {
    if dominant == FONT {
        font_metrics()
    } else if dominant == TAG_FONT {
        tag_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}

// ── keyframe ports ────────────────────────────────────────────────────────

/// Shared entry clamp: negative elapsed holds the first stop (fill `both`),
/// past the duration holds the last.
fn progress(elapsed_s: f32, duration_s: f32) -> f32 {
    if elapsed_s <= 0.0 {
        0.0
    } else if duration_s <= 0.0 || elapsed_s >= duration_s {
        1.0
    } else {
        elapsed_s / duration_s
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `tscaps-sol-rise-in`: opacity 0 → 1 over the 0-10% stop while the
/// translateY (1.2em) and the blur (0.5em) settle across the full
/// duration (neither is declared at 10%, so both run 0% → 100%).
///
/// Returns `(opacity, translateY_em, blur_em)`; `em` resolves against the
/// segment's font-size.
fn sol_rise(
    elapsed_s: f32,
    duration_s: f32,
    distance_em: f32,
    ease: &impl Fn(f32) -> f32,
) -> (f32, f32, f32) {
    let p = progress(elapsed_s, duration_s);
    let opacity = if p < RISE_OPACITY_HOLD {
        ease(p / RISE_OPACITY_HOLD)
    } else {
        1.0
    };
    let settle = ease(p);
    (
        opacity,
        distance_em * (1.0 - settle),
        RISE_BLUR_EM * (1.0 - settle),
    )
}

/// `tscaps-sol-pop-in`: opacity and blur resolve by 30%, the scale runs
/// 0.65 → 1.1 → 1 across both segments, and the translateY (declared only
/// at 0% and 100%) eases straight across the full duration.
///
/// Returns `(opacity, scale, translateY_em, blur_em)`; `em` resolves
/// against the accent word's font-size.
fn sol_pop(elapsed_s: f32, duration_s: f32, ease: &impl Fn(f32) -> f32) -> (f32, f32, f32, f32) {
    let p = progress(elapsed_s, duration_s);
    let opacity = if p < POP_HOLD {
        ease(p / POP_HOLD)
    } else {
        1.0
    };
    let peak = 1.0 + POP_SCALE;
    let scale = if p < POP_HOLD {
        lerp(POP_START_SCALE, peak, ease(p / POP_HOLD))
    } else {
        lerp(peak, 1.0, ease((p - POP_HOLD) / (1.0 - POP_HOLD)))
    };
    let ty = POP_TY_EM * (1.0 - ease(p));
    let blur = if p < POP_HOLD {
        POP_BLUR_EM * (1.0 - ease(p / POP_HOLD))
    } else {
        0.0
    };
    (opacity, scale, ty, blur)
}

/// Line boxes as `(top, height, font_size)` inside the block: line 0 at
/// the segment size, later lines at 1.5× with the -0.3em gap (resolved
/// against the accent line's own font-size) stacked before them.
fn line_boxes(line_count: usize, fs: f32) -> Vec<(f32, f32, f32)> {
    let mut boxes = Vec::with_capacity(line_count);
    let mut cursor = 0.0f32;
    for i in 0..line_count {
        let line_fs = if i == 0 { fs } else { fs * TAG_LINE_SCALE };
        if i > 0 {
            cursor += LINE_SPACING_EM * line_fs;
        }
        let height = LINE_HEIGHT * line_fs;
        boxes.push((cursor, height, line_fs));
        cursor += height;
    }
    boxes
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
    let st = style::Style::of(&doc.params, "sol");
    let highlight = st.color("highlight-color", HIGHLIGHT);
    let glow_color = st.color("glow-color", GLOW_COLOR);
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;

    let main_stack = stack::Stack::for_latin(FONT);
    let tag_stack = stack::Stack::for_latin(TAG_FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &main_stack));
    let (tag_asc_ratio, tag_desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &tag_stack));
    let rtl = segment.direction.is_rtl();

    // The splitter measures at the segment's typography — the accent
    // line's size/face only exist once lines do.
    let measured = stack::measure_words(
        &segment.words,
        &main_stack,
        MONTSERRAT_300_TTF,
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

    let boxes = line_boxes(lines.len(), fs);
    let block_h = boxes.last().map(|(top, height, _)| top + height).unwrap_or(0.0);
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let center_x = ov.anchor_x * video_w as f32;

    // The segment rise: one transform + one blur over the whole block.
    let rise_ease = recipes::cubic_bezier(0.0, 0.85, 0.4, 1.0);
    let (rise_opacity, rise_ty_em, rise_blur_em) = if ov.motion_static {
        (1.0, 0.0, 0.0)
    } else {
        sol_rise(
            (now_ms - segment.start_ms) / 1000.0,
            ov.duration_or(RISE_DURATION_S),
            ov.distance_or(RISE_DISTANCE_EM),
            &rise_ease,
        )
    };
    let rise_dy = rise_ty_em * fs;
    let rise_std = rise_blur_em * fs;

    // The accent line's four-layer glow, in px against its font-size.
    let tag_fs = boxes
        .iter()
        .find(|(_, _, line_fs)| (*line_fs - fs).abs() > f32::EPSILON)
        .map(|(_, _, line_fs)| *line_fs)
        .unwrap_or(fs * TAG_LINE_SCALE);
    let glow1_std = GLOW_RADIUS_EM * tag_fs / 2.0;
    let glow2_std = GLOW_RADIUS_EM * 2.5 * tag_fs / 2.0;
    let contact_dy = CONTACT_DY_EM * tag_fs;
    let contact_std = CONTACT_BLUR_EM * tag_fs / 2.0;
    let bounce_dy = BOUNCE_DY_EM * tag_fs;
    let bounce_std = BOUNCE_BLUR_EM * tag_fs / 2.0;

    let mut defs = String::new();
    if rise_std > 0.01 {
        defs.push_str(&format!(
            "<filter id=\"sol-rise\" x=\"-50%\" y=\"-200%\" width=\"200%\" height=\"500%\">\
             <feGaussianBlur stdDeviation=\"{rise_std}\"/></filter>"
        ));
    }
    if lines.len() > 1 {
        defs.push_str(&format!(
            "<filter id=\"sol-glow\" x=\"-300%\" y=\"-300%\" width=\"700%\" height=\"700%\">\
             <feDropShadow dx=\"0\" dy=\"0\" stdDeviation=\"{glow1_std}\" \
             flood-color=\"{glow_color}\" flood-opacity=\"{GLOW_ALPHA}\" result=\"g1\"/>\
             <feDropShadow in=\"g1\" dx=\"0\" dy=\"0\" stdDeviation=\"{glow2_std}\" \
             flood-color=\"{glow_color}\" flood-opacity=\"{GLOW_ALPHA}\" result=\"g2\"/>\
             <feDropShadow in=\"g2\" dx=\"0\" dy=\"{contact_dy}\" stdDeviation=\"{contact_std}\" \
             flood-color=\"#000000\" flood-opacity=\"0.5\" result=\"g3\"/>\
             <feDropShadow in=\"g3\" dx=\"0\" dy=\"{bounce_dy}\" stdDeviation=\"{bounce_std}\" \
             flood-color=\"{BOUNCE_COLOR}\" flood-opacity=\"{BOUNCE_ALPHA}\"/></filter>"
        ));
    }

    let pop_ease = recipes::cubic_bezier(0.16, 1.0, 0.3, 1.0);
    let mut pop_defs = String::new();
    let mut words_svg = String::new();
    let family_override = ov.font_family.as_str();
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let (top, line_h, line_fs) = boxes[line_idx];
        let line_top = block_top + top;
        let is_tag = line_idx > 0;
        let (latin_ttf, latin_weight, asc_ratio, desc_ratio, family, weight, fill, line_stack) = if is_tag {
            (
                CAVEAT_700_TTF,
                TAG_FONT_WEIGHT,
                tag_asc_ratio,
                tag_desc_ratio,
                TAG_FONT,
                TAG_FONT_WEIGHT,
                highlight.as_str(),
                &tag_stack,
            )
        } else {
            (
                MONTSERRAT_300_TTF,
                ov.font_weight,
                asc_ratio,
                desc_ratio,
                FONT,
                ov.font_weight,
                ov.primary.as_str(),
                &main_stack,
            )
        };
        let ascent = asc_ratio * line_fs;
        let descent = desc_ratio * line_fs;
        let baseline = line_top + (line_h - (ascent + descent)) / 2.0 + ascent;

        let line_margin = WORD_SPACING_EM * line_fs;
        let line_words = &segment.words[start..end];
        let line_measured = stack::measure_words(
            line_words,
            line_stack,
            latin_ttf,
            latin_weight,
            ctx.font_source,
            line_fs,
            ls_px,
            false,
        );
        let line_widths = &line_measured.widths;
        let line_split: Vec<f32> = line_widths
            .iter()
            .map(|w| w + 2.0 * line_margin)
            .collect();
        let line_w: f32 = line_split.iter().sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(line_widths, line_margin, line_left, line_w, rtl);

        let emit_family = if is_tag { family } else { family_override };
        words_svg.push_str(&format!(
            "<g font-family=\"{emit_family}\" font-weight=\"{weight}\" font-size=\"{line_fs}\" \
             fill=\"{fill}\">"
        ));
        for (i, ((x, word), word_family)) in xs
            .iter()
            .zip(line_words)
            .zip(&line_measured.families)
            .enumerate()
        {
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let family_attr = if *word_family == family {
                String::new()
            } else {
                format!(" font-family=\"{word_family}\"")
            };
            // Element inspector: override per-kata (attrs + transform).
            let wo_ctx = element_css::WordCtx {
                base_fs: line_fs,
                ls_px,
                video_w: video_w as f32,
                video_h: video_h as f32,
                center_x: *x + line_widths[i] / 2.0,
                center_y: baseline - line_fs * 0.35,
                word_start_s: word.start_ms / 1000.0,
                now_s: now_ms / 1000.0,
                font_family_svg: &stack::svg_font_family,
            };
            let wo = element_css::word_svg_attrs(&doc.params, word.index, &wo_ctx);
            let wo_transform = element_css::word_svg_transform(&doc.params, word.index, &wo_ctx)
                .map(|t| format!(" transform=\"{t}\""))
                .unwrap_or_default();
            if !is_tag {
                words_svg.push_str(&format!(
                    "<text x=\"{x}\" y=\"{baseline}\"{family_attr}{wo}{wo_transform}>{text}</text>"
                ));
                continue;
            }
            if element_css::has_override(&doc.params, word.index) {
                // Kata ber-override: pop sendiri ditahan, transform dari
                // inspector (glow aksen tetap).
                words_svg.push_str(&format!(
                    "<text x=\"{x}\" y=\"{baseline}\"{family_attr} filter=\"url(#sol-glow)\"{wo}{wo_transform}>{text}</text>"
                ));
                continue;
            }
            // Only the accent line pops, anchored on the word's own start.
            let (opacity, scale, ty_em, blur_em) = if ov.motion_static {
                (1.0, 1.0, 0.0, 0.0)
            } else {
                sol_pop(
                    (now_ms - word.start_ms) / 1000.0,
                    ov.duration_or(POP_DURATION_S),
                    &pop_ease,
                )
            };
            let ty = ty_em * line_fs;
            let blur_std = blur_em * line_fs;
            let filter_attr = if blur_std > 0.01 {
                let id = format!("sol-pop-{i}");
                pop_defs.push_str(&format!(
                    "<filter id=\"{id}\" x=\"-300%\" y=\"-300%\" width=\"700%\" height=\"700%\">\
                     <feGaussianBlur stdDeviation=\"{blur_std}\"/></filter>"
                ));
                format!(" filter=\"url(#{id})\"")
            } else {
                String::new()
            };
            // Scale about the word's own box centre, translateY inside.
            let cx = *x + line_widths[i] / 2.0;
            let cy = baseline + (descent - ascent) / 2.0;
            let nx = -cx;
            let ny = -cy;
            words_svg.push_str(&format!(
                "<g opacity=\"{opacity}\" transform=\"translate({cx},{cy}) scale({scale}) \
                 translate(0,{ty}) translate({nx},{ny})\"{filter_attr}>\
                 <text x=\"{x}\" y=\"{baseline}\"{family_attr} filter=\"url(#sol-glow)\">{text}</text></g>"
            ));
        }
        words_svg.push_str("</g>");
    }

    // Segment shell: the rise transform/blur wraps both lines.
    let mut segment_svg = format!(
        "<g opacity=\"{rise_opacity}\" transform=\"translate(0,{rise_dy})\" \
         font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\"",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    if rise_std > 0.01 {
        segment_svg.push_str(" filter=\"url(#sol-rise)\"");
    }
    segment_svg.push('>');
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
    defs.push_str(&pop_defs);

    let segment_svg = layout::dedupe_text_attrs(&segment_svg);
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
    fn embedded_faces_have_expected_identity() {
        let montserrat = fframes::ttf_parser::Face::parse(MONTSERRAT_300_TTF, 0).unwrap();
        let mut name = None;
        for n in montserrat.names() {
            if n.name_id == 16 {
                name = n.to_string();
                break;
            }
        }
        if name.is_none() {
            for n in montserrat.names() {
                if n.name_id == 1 {
                    name = n.to_string();
                    break;
                }
            }
        }
        assert_eq!(name.as_deref(), Some("Montserrat Thin"));
        assert_eq!(montserrat.weight().to_number(), FONT_WEIGHT);

        // The accent face stands in for Kalam: handwriting, weight 700.
        let caveat = fframes::ttf_parser::Face::parse(CAVEAT_700_TTF, 0).unwrap();
        let mut tag_name = None;
        for n in caveat.names() {
            if n.name_id == 16 {
                tag_name = n.to_string();
                break;
            }
        }
        if tag_name.is_none() {
            for n in caveat.names() {
                if n.name_id == 1 {
                    tag_name = n.to_string();
                    break;
                }
            }
        }
        assert_eq!(tag_name.as_deref(), Some(TAG_FONT));
        assert_eq!(caveat.weight().to_number(), TAG_FONT_WEIGHT);

        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
        let (t_asc, t_desc, t_upem) = tag_metrics();
        assert!(t_asc > 0.3 && t_asc < 2.0, "tag ascent {t_asc}");
        assert!(t_desc > 0.01 && t_desc < 1.5, "tag descent {t_desc}");
        assert!(t_upem >= 16.0, "tag upem {t_upem}");
    }

    #[test]
    fn rise_lights_by_ten_percent_and_settles_over_the_full_quarter_second() {
        let ease = recipes::cubic_bezier(0.0, 0.85, 0.4, 1.0);
        // Before the segment starts (fill both): hidden, full distance,
        // full blur.
        let (op0, dy0, blur0) = sol_rise(-0.2, RISE_DURATION_S, RISE_DISTANCE_EM, &ease);
        assert_eq!(op0, 0.0);
        assert_eq!(dy0, RISE_DISTANCE_EM);
        assert_eq!(blur0, RISE_BLUR_EM);
        // At 10% the opacity has landed; the lift and the blur run on the
        // full-duration curve, so both are partway (this ease front-loads)
        // and nowhere near home yet.
        let (op1, dy1, blur1) = sol_rise(RISE_DURATION_S * 0.1, RISE_DURATION_S, RISE_DISTANCE_EM, &ease);
        assert!((op1 - 1.0).abs() < 1e-6, "opacity {op1}");
        assert!(dy1 > 0.0 && dy1 < RISE_DISTANCE_EM * 0.75, "dy {dy1}");
        assert!(blur1 > 0.0 && blur1 < RISE_BLUR_EM, "blur {blur1}");
        // Resting state at the end of the duration.
        let (op2, dy2, blur2) = sol_rise(1.0, RISE_DURATION_S, RISE_DISTANCE_EM, &ease);
        assert_eq!((op2, dy2, blur2), (1.0, 0.0, 0.0));
    }

    #[test]
    fn word_pop_snaps_blur_away_and_overshoots_the_rest_scale() {
        let ease = recipes::cubic_bezier(0.16, 1.0, 0.3, 1.0);
        // Start: shrunk, dropped, still blurred, invisible.
        let (op0, scale0, ty0, blur0) = sol_pop(-0.1, POP_DURATION_S, &ease);
        assert_eq!((op0, scale0, ty0, blur0), (0.0, POP_START_SCALE, POP_TY_EM, POP_BLUR_EM));
        // At the 30% stop: lit, past the rest size, blur gone, still low.
        let (op1, scale1, ty1, blur1) = sol_pop(POP_DURATION_S * POP_HOLD, POP_DURATION_S, &ease);
        assert!((op1 - 1.0).abs() < 1e-6, "opacity {op1}");
        assert!((scale1 - (1.0 + POP_SCALE)).abs() < 1e-4, "scale {scale1}");
        assert_eq!(blur1, 0.0);
        assert!(ty1 > 0.0, "ty still easing down: {ty1}");
        // Resting state after the duration.
        let (op2, scale2, ty2, blur2) = sol_pop(1.0, POP_DURATION_S, &ease);
        assert!((op2, scale2, ty2, blur2) == (1.0, 1.0, 0.0, 0.0));
    }

    #[test]
    fn accent_line_is_one_and_a_half_times_the_size_and_tucks_up() {
        // Two lines, the accent at 1.5× with the -0.3em gap (against its
        // own font-size): a 100px block is 115 + (-45) + 172.5 tall.
        let boxes = line_boxes(2, 100.0);
        assert_eq!(boxes[0], (0.0, 115.0, 100.0));
        assert!((boxes[1].0 - 70.0).abs() < 1e-4, "accent top {}", boxes[1].0);
        assert!((boxes[1].1 - 172.5).abs() < 1e-4, "accent height {}", boxes[1].1);
        assert_eq!(boxes[1].2, 150.0);
        let block_h = boxes[1].0 + boxes[1].1;
        assert!((block_h - 242.5).abs() < 1e-4, "block height {block_h}");

        // The splitter honours the 2-line floor on clause splits.
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "hold on, this one runs quite long for a single line."
                    .into(),
                sheet: None,
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments
            .iter()
            .any(|s| s.words.iter().any(|w| w.text.ends_with(','))));
        let splitter = layout::BalancedPixelWidth {
            min_lines: MIN_LINES,
            max_lines: MAX_LINES,
            max_width: 1920.0 * MAX_WIDTH_RATIO,
        };
        assert_eq!(splitter.split(&[400.0, 400.0]).len(), 2);
    }
}
