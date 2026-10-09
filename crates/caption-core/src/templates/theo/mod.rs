//! Native port of the tscaps `theo` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\theo\{style.scss,template.json}`.
//! Heavy Inter (900) at 3cqh, one line per segment hanging from 0.78H, the
//! line sitting on a translucent black plate (`-0.25em / -0.55em` insets,
//! opacity 0.55, radius 0.2em) and every word carrying a soft
//! `text-shadow: 0 0.05em 0.2em #000`. Word colour follows the narration
//! state: upcoming words are `#d8d8d8` at 60% opacity, the word being
//! spoken is gold `#ffd700`, finished words are white.
//!
//! Unported: the `highlight-color` *variants* (Gold/Cyan/Green/Pink/Violet
//! live in the variant UI, not the default render) and
//! `--tscaps-text-decoration`.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, pipeline, stack, state};
use crate::CaptionDocument;

// ── spec constants (from theo's style.scss + template.json) ──────────────
const FONT: &str = "Inter"; // fontStack inter (typographic family, ID 16)
const FONT_WEIGHT: u16 = 900; // typography.fontWeight: 900
const FONT_SIZE_CQH: f32 = 0.03; // font-size: 3.0cqh
const LETTER_SPACING_EM: f32 = 0.0; // letter-spacing default 0em
const LINE_HEIGHT: f32 = 1.2; // line-height: 1.2 (mixin default)
const WORD_SPACING_EM: f32 = 0.15; // .word margin: 0 var(--tscaps-word-spacing, 0.15em)
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top default
const ANCHOR_Y_FRAC: f32 = 0.78; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 24; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 8; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.79; // lineSplitter.maxWidthRatio
const PRIMARY: &str = "#ffffff"; // primary-color default (past word)
const HIGHLIGHT: &str = "#ffd700"; // highlight-color default (active word)
const UPCOMING: &str = "#d8d8d8"; // upcoming-color default
const UPCOMING_OPACITY: f32 = 0.6; // upcoming-opacity default
// .line::before plate: bg-color / bg-opacity / bg-radius / bg-padding-x/y.
const PLATE_BG: &str = "#000000";
const PLATE_OPACITY: f32 = 0.55;
const PLATE_RADIUS_EM: f32 = 0.2;
const PLATE_PAD_X_EM: f32 = 0.55;
const PLATE_PAD_Y_EM: f32 = 0.25;
// .word text-shadow: soft-drop-shadow(0.05, 0.2) → `0 0.05em 0.2em #000`;
// text-shadow's third length is a radius, so the Gaussian σ is half.
const SHADOW_DY_EM: f32 = 0.05;
const SHADOW_BLUR_EM: f32 = 0.2;
const SHADOW_SIGMA_EM: f32 = SHADOW_BLUR_EM / 2.0;

/// theo's `effects` array: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const INTER_900_TTF: &[u8] = include_bytes!("../../../assets/inter-latin-900-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Inter 900.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(INTER_900_TTF, 0).expect("Inter parses");
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

/// `(fill, opacity)` of a word from its narration state — gold while
/// spoken, white once spoken, translucent `#d8d8d8` before.
fn word_paint(now_ms: f32, word: &pipeline::TimedWord) -> (&'static str, f32) {
    match state::word_narration(now_ms, word.start_ms, word.end_ms) {
        state::Narration::NotYet => (UPCOMING, UPCOMING_OPACITY),
        state::Narration::Being => (HIGHLIGHT, 1.0),
        state::Narration::Already => (PRIMARY, 1.0),
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
    let ls_px = LETTER_SPACING_EM * fs;
    let margin = WORD_SPACING_EM * fs; // word-layout: full spacing per side
    let line_h = LINE_HEIGHT * fs;
    let pad_x = PLATE_PAD_X_EM * fs;
    let pad_y = PLATE_PAD_Y_EM * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        INTER_900_TTF,
        FONT_WEIGHT,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: MIN_LINES,
        max_lines: MAX_LINES,
        max_width: (video_w as f32 * MAX_WIDTH_RATIO).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Bottom-aligned block: the block's bottom edge sits at 0.78H.
    let block_h = lines.len() as f32 * line_h
        + lines.len().saturating_sub(1) as f32 * LINE_SPACING_EM * fs;
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - block_h;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;
    let plate_r = PLATE_RADIUS_EM * fs;
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_sigma = SHADOW_SIGMA_EM * fs;

    let mut body = format!(
        "<defs><filter id=\"theo-shadow\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\">\
         <feDropShadow dx=\"0\" dy=\"{shadow_dy}\" stdDeviation=\"{shadow_sigma}\" \
         flood-color=\"#000000\" flood-opacity=\"1\"/></filter></defs>\
         <g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + LINE_SPACING_EM * fs);
        let ws = &widths[start..end];
        let content_w: f32 = ws.iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - content_w / 2.0;

        // The plate: the line box grown by the negative insets, painted
        // first so it sits behind the glyphs (`z-index: -1`).
        let plate_w = content_w + 2.0 * pad_x;
        let plate_left = line_left - pad_x;
        let plate_h = line_h + 2.0 * pad_y;
        let plate_top = line_top - pad_y;
        body.push_str(&format!(
            "<rect x=\"{plate_left}\" y=\"{plate_top}\" width=\"{plate_w}\" height=\"{plate_h}\" \
             rx=\"{plate_r}\" ry=\"{plate_r}\" fill=\"{PLATE_BG}\" fill-opacity=\"{PLATE_OPACITY}\"/>"
        ));

        let baseline = line_top + half_leading + ascent;
        let xs = layout::word_x_positions(ws, margin, line_left, content_w, rtl);
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&word.display);
            let (fill, opacity) = word_paint(now_ms, word);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            body.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\" opacity=\"{opacity}\"{family_attr} \
                 filter=\"url(#theo-shadow)\">{text}</text>"
            ));
        }
    }
    body.push_str("</g>");

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 1920 1080"
            width={video_w}
            height={video_h}
        >
            {body}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_inter_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(INTER_900_TTF, 0).unwrap();
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
        assert_eq!(name.as_deref(), Some(FONT));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn shadow_is_a_text_shadow_so_sigma_is_half_the_blur() {
        // The scss mixin's own caveat: text-shadow reads its third length
        // as a radius; the drop-shadow we emit needs the Gaussian σ.
        assert_eq!(SHADOW_DY_EM, 0.05);
        assert_eq!(SHADOW_BLUR_EM, 0.2);
        assert_eq!(SHADOW_SIGMA_EM, 0.1);
        // …and the plate hugs the line box on every side.
        assert_eq!(PLATE_PAD_X_EM, 0.55);
        assert_eq!(PLATE_PAD_Y_EM, 0.25);
        assert_eq!(PLATE_RADIUS_EM, 0.2);
        assert_eq!(PLATE_OPACITY, 0.55);
        assert_eq!(MAX_WIDTH_RATIO, 0.79);
    }

    #[test]
    fn word_paint_follows_the_narration_state() {
        let word = pipeline::TimedWord::new("go", 100.0, 200.0);
        assert_eq!(word_paint(50.0, &word), (UPCOMING, 0.6));
        assert_eq!(word_paint(150.0, &word), (HIGHLIGHT, 1.0));
        assert_eq!(word_paint(250.0, &word), (PRIMARY, 1.0));
    }

    #[test]
    fn theo_sentence_segments_split_at_twenty_four_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait. Hold on and look at this one here now.".into(),
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
        assert_eq!(texts[0], "Wait.");
        assert!(segments.len() >= 2, "{texts:?}");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
    }
}
