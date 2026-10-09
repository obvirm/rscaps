//! Native port of the tscaps `otto` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\otto\{style.scss,template.json}`.
//! Inter SemiBold at 2.7cqh on one line per segment, each line sitting on
//! its own black pill (`--tscaps-bg-*` 0.46em / 0.23em / radius 0.15em,
//! drawn behind the text at `z-index: -1`); the word being narrated turns
//! gold (`--tscaps-highlight-color`). No entrance — the segment simply
//! exists while its window is on screen, `gap_free` holding it between
//! nearby segments. Sentence-boundary segments, 30/10 chars, block
//! bottom-anchored at 0.85H.
//!
//! Unported: the `--tscaps-rotation` knob the typography mixin exposes
//! (its default is `0deg`, so the default render is identical), and the
//! block-level pill (`pill-mode: segment`) — `lineSplitter.maxLines` is 1,
//! so both pills cover exactly the same box and the per-line one is drawn.
//!
//! Family note: the embedded Inter 600 carries the name-table family
//! "Inter" (name ID 16), which is both what fontdb resolves and what the
//! SVG emits.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, pipeline, stack, state};
use crate::CaptionDocument;

// ── spec constants (from otto's style.scss + template.json) ───────────────
const FONT: &str = "Inter"; // fontStack inter (typographic family, ID 16)
const FONT_WEIGHT: u16 = 600; // typography.fontWeight: 600
const FONT_SIZE_CQH: f32 = 0.027; // font-size: 2.7cqh
const LETTER_SPACING_EM: f32 = 0.0; // letter-spacing default 0em
const LINE_HEIGHT: f32 = 1.2; // line-height: 1.2 (mixin default)
const WORD_SPACING_EM: f32 = 0.15; // .word margin: 0 0.15em
// .line + .line margin-top: var(--tscaps-line-spacing, 0.54em) — inert
// while maxLines is 1, kept so the geometry would match if it ever isn't.
const LINE_SPACING_EM: f32 = 0.54;
const ANCHOR_Y_FRAC: f32 = 0.85; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 30; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 10; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // lineSplitter.maxWidthRatio default
const PRIMARY: &str = "#ffffff"; // primary-color default
const HIGHLIGHT: &str = "#ffd700"; // --tscaps-highlight-color default
// .line::before pill (bg-color / bg-radius / bg-padding-x / bg-padding-y).
const PILL_BG: &str = "#000000";
const PILL_RADIUS_EM: f32 = 0.15;
const PILL_PAD_X_EM: f32 = 0.46;
const PILL_PAD_Y_EM: f32 = 0.23;

/// otto's `effects` array: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const INTER_600_TTF: &[u8] = include_bytes!("../../../assets/inter-latin-600-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Inter 600.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(INTER_600_TTF, 0).expect("Inter parses");
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

/// `.word-being-narrated { color: highlight }` — the gold follows the
/// word's own narration window, not the line's.
fn word_fill(now_ms: f32, word: &pipeline::TimedWord) -> &'static str {
    if state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
        HIGHLIGHT
    } else {
        PRIMARY
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
    let margin = WORD_SPACING_EM * fs;
    let line_h = LINE_HEIGHT * fs;
    let pad_x = PILL_PAD_X_EM * fs;
    let pad_y = PILL_PAD_Y_EM * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        INTER_600_TTF,
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

    // Bottom-aligned block: the block's bottom edge sits at 0.85H; lines
    // stack with the line-spacing gap between them (never in the last).
    let block_h = lines.len() as f32 * line_h
        + lines.len().saturating_sub(1) as f32 * LINE_SPACING_EM * fs;
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - block_h;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;
    let pill_r = PILL_RADIUS_EM * fs;

    let mut body = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + LINE_SPACING_EM * fs);
        let ws = &widths[start..end];
        let run: f32 = ws.iter().map(|w| w + 2.0 * margin).sum();

        // The pill: line box grown by the negative insets, painted first
        // so it sits behind the glyphs (`z-index: -1`).
        let box_w = run + 2.0 * pad_x;
        let box_left = center_x - box_w / 2.0;
        let box_h = line_h + 2.0 * pad_y;
        let box_top = line_top - pad_y;
        body.push_str(&format!(
            "<rect x=\"{box_left}\" y=\"{box_top}\" width=\"{box_w}\" height=\"{box_h}\" \
             rx=\"{pill_r}\" ry=\"{pill_r}\" fill=\"{PILL_BG}\"/>"
        ));

        let baseline = line_top + half_leading + ascent;
        let line_left = center_x - run / 2.0;
        let xs = layout::word_x_positions(ws, margin, line_left, run, rtl);
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&word.display);
            let fill = word_fill(now_ms, word);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            body.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}>{text}</text>"
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
        let face = fframes::ttf_parser::Face::parse(INTER_600_TTF, 0).unwrap();
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
    fn otto_splitter_is_sentence_plus_thirty_chars_on_one_line() {
        assert_eq!(MAX_LINES, 1, "otto always renders a single line");
        assert_eq!(MAX_WIDTH_RATIO, 0.72, "maxWidthRatio is not in json");
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait. Hold on a second and look at this one now.".into(),
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

    #[test]
    fn pill_wraps_the_line_box_and_the_gold_follows_the_word() {
        // Negative insets grow the line box on every side.
        assert_eq!(PILL_PAD_X_EM, 0.46);
        assert_eq!(PILL_PAD_Y_EM, 0.23);
        assert_eq!(PILL_RADIUS_EM, 0.15);
        assert_eq!(PILL_BG, "#000000");
        // A word narrating at 500ms is gold, its neighbours stay white.
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "one two".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let words = &segments[0].words;
        assert_eq!(word_fill(words[0].start_ms, &words[0]), HIGHLIGHT);
        assert_eq!(word_fill(words[0].end_ms, &words[0]), PRIMARY);
        assert_eq!(word_fill(words[0].start_ms, &words[1]), PRIMARY);
    }
}
