//! Native port of the tscaps `remi` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\remi\{style.scss,template.json}`.
//! Handwritten Caveat Bold at 6cqh: every word arrives low, small and
//! tilted and springs into place as it is narrated (`snap-in`, 0.36s),
//! the spoken word lights gold (#ffd966), and a two-layer shadow — a hard
//! black offset plus a soft haze — sits behind the line. `gap_free` keeps
//! the caption up between nearby segments.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from remi's style.scss + template.json) ───────────────
const FONT: &str = "Caveat"; // fontStack caveat ('Caveat Variable' in scss)
const FONT_WEIGHT: u16 = 700; // fontWeight: 700
const FONT_SIZE_CQH: f32 = 0.06; // font-size: 6.0cqh
const LINE_HEIGHT: f32 = 1.05; // line-height: 1.05
const WORD_SPACING_EM: f32 = 0.12; // .word margin: 0 0.12em
const ANCHOR_Y_FRAC: f32 = 0.68; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 30; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 8; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const SNAP_RISE_EM: f32 = 0.31; // snap-in($rise: 0.31em)
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
const HIGHLIGHT: &str = "#ffd966"; // --tscaps-highlight-color default
// text-shadow: 0.03em 0.04em 0 $shadow, 0 0.06em 0.19em rgba(0,0,0,.45)
const SHADOW1_DX_EM: f32 = 0.03;
const SHADOW1_DY_EM: f32 = 0.04;
const SHADOW2_DY_EM: f32 = 0.06;
const SHADOW2_BLUR_EM: f32 = 0.19;
const SHADOW2_ALPHA: f32 = 0.45;
const SHADOW_COLOR: &str = "#000000"; // --tscaps-shadow-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: 0.0, // no LETTER_SPACING_EM in remi's spec
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // no LINE_SPACING_EM in remi's spec
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

/// remi's `effects` array: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const CAVEAT_TTF: &[u8] = include_bytes!("../../../assets/caveat-latin-700-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Caveat.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(CAVEAT_TTF, 0).expect("Caveat parses");
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
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        CAVEAT_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        0.0,
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

    // Top-aligned block: the block's top edge sits at 0.68H; lines center
    // horizontally.
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Every word snaps in on its own narration clock.
    let mut snap = recipes::SnapIn::with(SNAP_RISE_EM);
    snap.rise_em = ov.distance_or(snap.rise_em);
    snap.duration_s = ov.duration_or(snap.duration_s);

    // Shadow geometry (px from em ratios).
    let shadow1_dx = SHADOW1_DX_EM * fs;
    let shadow1_dy = SHADOW1_DY_EM * fs;
    let shadow2_dy = SHADOW2_DY_EM * fs;
    let shadow2_std = SHADOW2_BLUR_EM * fs / 2.0;

    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         filter=\"url(#remi-shadow)\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
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
            let w = widths[start + j];
            let (opacity, ty_em, rot, sc) = if ov.motion_static {
                (1.0, 0.0, 0.0, 1.0)
            } else {
                snap.eval((now_ms - word.start_ms) / 1000.0)
            };
            let ty = ty_em * fs;
            // The word's own box: content top of this line, half the line
            // box tall — transform origin is its centre.
            let cx = x + w / 2.0;
            let cy = line_top + line_h / 2.0;
            let transform = if rot.abs() < 1e-4 && (sc - 1.0).abs() < 1e-4 {
                if ty.abs() < 1e-4 {
                    String::new()
                } else {
                    format!(" transform=\"translate(0,{ty})\"")
                }
            } else {
                format!(
                    " transform=\"translate(0,{ty}) rotate({rot},{cx},{cy}) \
                     translate({cx},{cy}) scale({sc}) translate({},{})\"",
                    -cx, -cy
                )
            };
            let fill = if state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
                HIGHLIGHT
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
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr} \
                 opacity=\"{opacity}\"{transform}>{text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the caption around the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + (lines.len() as f32 * line_h) / 2.0;
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
                <filter id="remi-shadow" x="-30%" y="-40%" width="160%" height="200%">
                    <feDropShadow dx={shadow1_dx} dy={shadow1_dy} stdDeviation={0.0} flood-color={SHADOW_COLOR} flood-opacity={1.0} result="s1"/>
                    <feDropShadow in="s1" dx={0.0} dy={shadow2_dy} stdDeviation={shadow2_std} flood-color={SHADOW_COLOR} flood-opacity={SHADOW2_ALPHA}/>
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
    fn embedded_caveat_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(CAVEAT_TTF, 0).unwrap();
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
    fn words_snap_in_low_small_and_tilted() {
        let snap = recipes::SnapIn::with(SNAP_RISE_EM);
        assert_eq!(snap.duration_s, 0.36);
        assert_eq!(snap.rotation_deg, 7.0);
        assert_eq!(snap.scale, 0.7);
        // At the narration start the word sits below, smaller and
        // counter-rotated, fully invisible.
        let (opacity, ty, rot, sc) = snap.eval(0.0);
        assert_eq!(opacity, 0.0);
        assert!((ty - 0.31).abs() < 1e-6, "rise {ty}");
        assert!((rot + 7.0).abs() < 1e-6, "rotation {rot}");
        assert_eq!(sc, 0.7);
        // Rest: on screen, upright, full size.
        let (opacity, ty, rot, sc) = snap.eval(1.0);
        assert_eq!((opacity, ty, rot, sc), (1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn remi_splitter_is_sentence_plus_thirty_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "One very long sentence that definitely needs a second segment here."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
        let joined = segments
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("|");
        assert!(!joined.contains("needs a|second"), "{joined}");
    }
}
