//! Native port of the tscaps `tala` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\tala\{style.scss,template.json}`.
//! Uppercase Anton at 4cqh where *the line is the plate*: every line is a
//! shrink-to-fit dark tape (`fit-content`, padding 0.28em / 0.11em, radius
//! 0.17em, `#1a1a1a`) centred on screen, the half-word-gap trimmed at both
//! ends so the padding is the only thing between the letters and the edge.
//! The block hangs from its top edge at 0.73H, one line per segment, with
//! `gap_free` + `remove_punctuation` over clause-boundary segments (18/8
//! chars).
//!
//! Unported: `.line:has(:is(.emphasis, .accent, .entity))` swaps the tape
//! to gold — it keys off word *tags* (`semanticTags`), which the native
//! `Cue` text does not carry; likewise `--tscaps-text-decoration`.

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{effects, layout, pipeline};
use crate::CaptionDocument;

// ── spec constants (from tala's style.scss + template.json) ───────────────
const FONT: &str = "Anton"; // fontStack anton / segment-typography $font-family
const FONT_WEIGHT: u16 = 400; // typography.fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.04; // font-size: 4.0cqh
const LETTER_SPACING_EM: f32 = 0.03; // letter-spacing: 0.03em
const LINE_HEIGHT: f32 = 1.25; // line-height: 1.25
const WORD_SPACING_EM: f32 = 0.35; // --tscaps-word-spacing default
const LINE_SPACING_EM: f32 = 0.14; // .line + .line margin-top (json)
const ANCHOR_Y_FRAC: f32 = 0.73; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalOffset default (center)
const MAX_CHARS: usize = 18; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 8; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // lineSplitter.maxWidthRatio default
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
// .line background chrome: bg-color / bg-radius / bg-padding-x/y.
const TAPE_BG: &str = "#1a1a1a";
const TAPE_RADIUS_EM: f32 = 0.17;
const TAPE_PAD_X_EM: f32 = 0.28;
const TAPE_PAD_Y_EM: f32 = 0.11;

/// tala's `effects` array: `gap_free`, `remove_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::RemovePunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const ANTON_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Anton.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(ANTON_TTF, 0).expect("Anton parses");
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

/// Line width after `.word:first-child/last-child` drop the half-gap at
/// both ends: every inner gap counts, the two edge margins do not.
fn trimmed_content_width(widths: &[f32], margin: f32) -> f32 {
    let gross: f32 = widths.iter().map(|w| w + 2.0 * margin).sum();
    if widths.is_empty() {
        0.0
    } else {
        gross - 2.0 * margin
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
    let margin = WORD_SPACING_EM * fs / 2.0; // margin: 0 (spacing / 2)
    let line_h = LINE_HEIGHT * fs;
    let pad_x = TAPE_PAD_X_EM * fs;
    let pad_y = TAPE_PAD_Y_EM * fs;
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
    // text-transform: uppercase — measured and emitted as displayed.
    let display_texts: Vec<String> = segment
        .words
        .iter()
        .map(|w| w.display.to_uppercase())
        .collect();
    let mut widths = Vec::with_capacity(segment.words.len());
    for text in &display_texts {
        let width_px = match &face {
            Some(face) => measure_word(face.as_ref(), fs, ls_px, text),
            None => text.encode_utf16().count() as f32 * fs * 0.5,
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

    // Top-aligned block: the block's top edge sits at 0.73H. Each line is
    // its own tape, shrink-to-fit and centred, stacked with the 0.14em
    // gap.
    let block_top = ANCHOR_Y_FRAC * video_h as f32;
    let center_x = ANCHOR_X_FRAC * video_w as f32;
    let box_h = line_h + 2.0 * pad_y;
    let tape_r = TAPE_RADIUS_EM * fs;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let mut body = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let box_top = block_top + line_idx as f32 * (box_h + LINE_SPACING_EM * fs);
        let ws = &widths[start..end];
        let content_w = trimmed_content_width(ws, margin);
        let box_w = content_w + 2.0 * pad_x;
        let box_left = center_x - box_w / 2.0;
        let content_left = box_left + pad_x;

        // The tape is the line: painted first, glyphs on top of it.
        body.push_str(&format!(
            "<rect x=\"{box_left}\" y=\"{box_top}\" width=\"{box_w}\" height=\"{box_h}\" \
             rx=\"{tape_r}\" ry=\"{tape_r}\" fill=\"{TAPE_BG}\"/>"
        ));

        // First word starts flush with the padding (its left margin is
        // trimmed); the rest keep the 2·margin gaps.
        let xs = layout::word_x_positions(ws, margin, content_left - margin, content_w, false);
        let baseline = box_top + pad_y + half_leading + ascent;
        for (x, text) in xs.iter().zip(&display_texts[start..end]) {
            let text = layout::escape_xml(text);
            body.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{PRIMARY}\">{text}</text>"
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
    fn embedded_anton_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(ANTON_TTF, 0).unwrap();
        let mut name = None;
        for n in face.names() {
            if n.name_id == 1 {
                name = n.to_string();
                break;
            }
        }
        assert_eq!(name.as_deref(), Some("Anton"));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn tape_is_the_line_with_its_half_gaps_trimmed() {
        // Words 10 / 20 with a 4-unit half-gap: the edge margins vanish,
        // the inner gap stays 2·margin wide (10 + 8 + 20).
        assert_eq!(trimmed_content_width(&[10.0, 20.0], 4.0), 38.0);
        // A single word has no gaps at all.
        assert_eq!(trimmed_content_width(&[10.0], 4.0), 10.0);
        assert_eq!(trimmed_content_width(&[], 4.0), 0.0);
        // The tape grows by the padding on every side.
        assert_eq!(TAPE_PAD_X_EM, 0.28);
        assert_eq!(TAPE_PAD_Y_EM, 0.11);
        assert_eq!(TAPE_RADIUS_EM, 0.17);
        assert_eq!(TAPE_BG, "#1a1a1a");
        // Word spacing 0.35em lands as a 0.175em margin per side.
        assert_eq!(WORD_SPACING_EM / 2.0, 0.175);
    }

    #[test]
    fn tala_clause_segments_split_and_strip_trailing_punctuation() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, hold on now.".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.display.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        // Clause boundary closes at the comma; remove_punctuation then
        // drops trailing punctuation from every word's display form —
        // including the comma that made the split.
        assert_eq!(texts, vec!["Wait", "hold on now"]);
        // The raw text the splitters saw is untouched.
        assert_eq!(segments[0].words.last().unwrap().text, "Wait,");
        assert_eq!(segments[1].words.last().unwrap().text, "now.");
    }
}
