//! Native port of the tscaps `yuki` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\yuki\{style.scss,template.json}`.
//! Quiet Inter Bold at 2.7cqh: a hairline outline painted under the fill
//! (`-webkit-text-stroke` + `paint-order: stroke fill`), a soft drop
//! shadow, a plain segment fade-in (0.18s) and no per-word motion.

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{layout, pipeline, recipes};
use crate::CaptionDocument;

// ── spec constants (from yuki's style.scss + template.json) ───────────────
const FONT: &str = "Inter";
const FONT_WEIGHT: u16 = 700;
const FONT_SIZE_CQH: f32 = 0.027; // font-size: 2.7cqh
const LINE_HEIGHT: f32 = 1.3; // line-height: 1.3
const WORD_SPACING_EM: f32 = 0.13; // .word margin: 0 0.13em
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 42; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 12; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const FADE_DURATION_S: f32 = 0.18; // fade-in default $duration
const PRIMARY: &str = "#ffffff"; // primary-color default
const OUTLINE_COLOR: &str = "#000000"; // outline-color default
const OUTLINE_THICKNESS_EM: f32 = 0.1; // outline-thickness control default
// text-shadow: soft-drop-shadow(0.08, 0.25) → 0 0.08em 0.25em #000
const SHADOW_DY_EM: f32 = 0.08;
const SHADOW_BLUR_EM: f32 = 0.25;

/// yuki has no `effects` array — windows stay the raw word span.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const INTER_TTF: &[u8] = include_bytes!("../../../assets/inter-latin-700-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Inter Bold.
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

/// Raw word width: advance + one letter-spacing per UTF-16 unit (yuki has
/// no letter-spacing; the term stays for the shared shape).
fn measure_word(face: &dyn fframes::FontFace, fs: f32, text: &str) -> f32 {
    let upem = font_metrics().2 as usize;
    match face.font_variant(upem) {
        Some(variant) => {
            let units = layout::measure_units(face, variant, upem, text);
            units as f32 * fs / upem as f32
        }
        None => text.encode_utf16().count() as f32 * fs * 0.5,
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

    let video_w = ctx.current_video_size.width;
    let video_h = ctx.current_video_size.height;
    let fs = FONT_SIZE_CQH * video_h as f32;
    let margin = WORD_SPACING_EM * fs;
    let line_h = LINE_HEIGHT * fs;
    let (asc_ratio, desc_ratio, _) = font_metrics();

    let probe = FontQuery {
        family: FONT,
        size: 1,
        weight: FONT_WEIGHT,
        ..Default::default()
    };
    let face = ctx
        .font_source
        .and_then(|source| source.resolve_font(FONT, probe.weight, probe.style, probe.stretch));
    let mut widths = Vec::with_capacity(segment.words.len());
    for word in &segment.words {
        let width_px = match &face {
            Some(face) => measure_word(face.as_ref(), fs, &word.display),
            None => word.display.encode_utf16().count() as f32 * fs * 0.5,
        };
        widths.push(width_px);
    }

    let splitter = layout::BalancedPixelWidth {
        min_lines: MIN_LINES,
        max_lines: MAX_LINES,
        max_width: (video_w as f32 * MAX_WIDTH_RATIO).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    let block_top = ANCHOR_Y_FRAC * video_h as f32;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let fade = recipes::FadeIn::with(0.0, FADE_DURATION_S);
    let segment_opacity = fade.eval((now_ms - segment.start_ms) / 1000.0);

    let outline_px = OUTLINE_THICKNESS_EM * fs;
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;

    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         opacity=\"{segment_opacity}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, false);
        for (x, text) in xs.iter().zip(&segment.words[start..end]) {
            let text = layout::escape_xml(&text.display);
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{PRIMARY}\" stroke=\"{OUTLINE_COLOR}\" \
                 stroke-width=\"{outline_px}\" paint-order=\"stroke\" filter=\"url(#yuki-shadow)\">\
                 {text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 1920 1080"
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="yuki-shadow" x="-30%" y="-30%" width="160%" height="180%">
                    <feDropShadow dx={0.0} dy={shadow_dy} stdDeviation={shadow_std} flood-color="#000000" flood-opacity={1.0}/>
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
    fn embedded_inter_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(INTER_TTF, 0).unwrap();
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let mut name = None;
        for n in face.names() {
            if n.name_id == fframes::ttf_parser::name_id::FAMILY {
                name = n.to_string();
                break;
            }
        }
        assert_eq!(name.as_deref(), Some("Inter"));
    }

    #[test]
    fn yuki_splitter_is_sentence_plus_forty_two_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "A fairly long single sentence that will need splitting at forty two chars."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
    }

    #[test]
    fn yuki_fades_in_over_the_default_eighteen() {
        let fade = recipes::FadeIn::with(0.0, FADE_DURATION_S);
        assert_eq!(fade.eval(-1.0), 0.0);
        assert!(fade.eval(0.09) > 0.5);
        assert!((fade.eval(0.18) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn outline_is_tenth_of_an_em_by_default() {
        let fs = 0.027 * 1080.0;
        assert!((OUTLINE_THICKNESS_EM * fs - 2.916).abs() < 1e-3);
    }
}
