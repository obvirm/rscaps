//! Native port of the tscaps `enzo` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\enzo\{style.scss,template.json}`.
//! Heavy Inter Black at 5.2cqh with tight tracking (-0.045em) on a tight
//! 0.92em leading; words fade in individually (0.16s) and a soft grey
//! shade (#333333) lifts *away* from the glyphs (dy −0.4em, blur 0.8em)
//! so the baseline stays crisp. Clause-boundary segments with `gap_free`
//! and `smart_punctuation`, block bottom-anchored at 0.72H.

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{effects, layout, pipeline, recipes};
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
// text-shadow: 0 -0.4em 0.8em var(--tscaps-shadow-color, #333333)
const SHADOW_DY_EM: f32 = -0.4;
const SHADOW_BLUR_EM: f32 = 0.8;
const SHADOW_COLOR: &str = "#333333"; // --tscaps-shadow-color default
// -webkit-text-stroke: var(--tscaps-outline-thickness, 0em) $outline-color
const OUTLINE_THICKNESS_EM: f32 = 0.0;
const OUTLINE_COLOR: &str = "#000000"; // --tscaps-outline-color default

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

/// Raw word width: advance + one letter-spacing per UTF-16 unit (the
/// `DomProbeCanvasTextMeasurer` replica; margins live in the placement).
fn measure_word(face: &dyn fframes::FontFace, fs: f32, ls_px: f32, text: &str) -> f32 {
    let upem = font_metrics().2 as usize;
    let fallback = text.encode_utf16().count() as f32 * fs * 0.5;
    let advance = match face.font_variant(upem) {
        Some(variant) => {
            let units = layout::measure_units(face, variant, upem, text);
            units as f32 * fs / upem as f32
        }
        None => fallback,
    };
    advance + text.encode_utf16().count() as f32 * ls_px
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
    let ls_px = LETTER_SPACING_EM * fs;
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
            Some(face) => measure_word(face.as_ref(), fs, ls_px, &word.display),
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

    // Bottom-aligned block: the block's bottom edge sits at 0.72H; lines
    // center horizontally. lineSpacing is 0, so lines stack at line_h.
    let block_h = lines.len() as f32 * line_h;
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - block_h;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Shadow geometry (px from em ratios): the shade floats *above* the
    // glyphs — dy is negative on purpose.
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;
    let stroke_px = OUTLINE_THICKNESS_EM * fs;

    // Each word fades in on its own narration clock.
    let fade = recipes::FadeIn::with(0.0, FADE_DURATION_S);

    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" filter=\"url(#enzo-shadow)\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, false);
        for (x, word) in xs.iter().zip(&segment.words[start..end]) {
            let opacity = fade.eval((now_ms - word.start_ms) / 1000.0);
            let text = layout::escape_xml(&word.display);
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{PRIMARY}\" \
                 stroke=\"{OUTLINE_COLOR}\" stroke-width=\"{stroke_px}\" \
                 paint-order=\"stroke\" opacity=\"{opacity}\">{text}</text>"
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
                <filter id="enzo-shadow" x="-100%" y="-300%" width="300%" height="500%">
                    <feDropShadow dx={0.0} dy={shadow_dy} stdDeviation={shadow_std} flood-color={SHADOW_COLOR} flood-opacity={1.0}/>
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
