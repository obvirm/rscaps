//! Native port of the tscaps `tito` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\tito\{style.scss,template.json}`.
//! Arcade-sign Bungee at 3.5cqh with a two-layer cast shadow, a green
//! highlight that lights the spoken word (`glow` + `#00ffb3`), a segment
//! fade-in (0.25s) and a looping per-word `bob` staggered by the word's
//! place in the spoken line (0.10s apart).

use fframes::{FFramesContext, Svgr};

use super::{layout, pipeline, recipes, stack, state};
use crate::CaptionDocument;

// ── spec constants (from tito's style.scss + template.json) ───────────────
const FONT: &str = "Bungee";
const FONT_WEIGHT: u16 = 400;
const FONT_SIZE_CQH: f32 = 0.035; // font-size: 3.5cqh
const LETTER_SPACING_EM: f32 = 0.02; // letter-spacing: 0.02em
const LINE_HEIGHT: f32 = 1.55; // line-height: 1.55
const WORD_SPACING_EM: f32 = 0.14; // .word margin: 0 0.14em
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 30; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 7; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const FADE_DURATION_S: f32 = 0.25; // fade-in($duration: 0.25s)
const BOB_DISTANCE_EM: f32 = 0.19; // bob($distance: 0.19em)
const BOB_STAGGER_S: f32 = 0.10; // delay: + word-index × 0.10s
const PRIMARY: &str = "#ffffff"; // primary-color default
const HIGHLIGHT: &str = "#00ffb3"; // highlight-color default
const SHADOW_COLOR: &str = "#000000"; // shadow-color default
// text-shadow: 0 0.06em 0.06em 0 $shadow, 0 0.11em 0.28em rgba(0,0,0,.45)
const CAST1_D_EM: f32 = 0.06;
const CAST2_DY_EM: f32 = 0.11;
const CAST2_BLUR_EM: f32 = 0.28;
// glow(0.1) → text-shadow: 0 0 0.1em currentColor
const GLOW_BLUR_EM: f32 = 0.1;

/// tito has no `effects` array — windows stay the raw word span.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const BUNGEE_TTF: &[u8] = include_bytes!("../../../assets/bungee-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Bungee.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(BUNGEE_TTF, 0).expect("Bungee parses");
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

    let video_w = ctx.current_video_size.width;
    let video_h = ctx.current_video_size.height;
    let fs = FONT_SIZE_CQH * video_h as f32;
    let ls_px = LETTER_SPACING_EM * fs;
    let margin = WORD_SPACING_EM * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) = line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        BUNGEE_TTF,
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

    // Top-aligned block: the block's top edge sits at 0.75H; lines center
    // horizontally.
    let block_top = ANCHOR_Y_FRAC * video_h as f32;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Segment fade-in: `--on-segment-starts` = the segment's start.
    let fade = recipes::FadeIn::with(0.0, FADE_DURATION_S);
    let segment_opacity = fade.eval((now_ms - segment.start_ms) / 1000.0);

    // Shadow geometry (px from em ratios).
    let cast1_d = CAST1_D_EM * fs;
    let cast2_dy = CAST2_DY_EM * fs;
    let cast2_std = CAST2_BLUR_EM * fs / 2.0;
    let glow_std = GLOW_BLUR_EM * fs / 2.0;

    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" opacity=\"{segment_opacity}\">"
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
            // Bob stutters the words of the line apart: delay = segment
            // start + word index within this line × 0.10s.
            let bob = recipes::Bob::with(
                BOB_DISTANCE_EM,
                (segment.start_ms + j as f32 * BOB_STAGGER_S * 1000.0) / 1000.0,
            );
            let y = baseline + bob.eval(now_ms / 1000.0) * fs;
            let narrated = state::word_narration(now_ms, word.start_ms, word.end_ms).is_being();
            let (fill, filter) = if narrated {
                (HIGHLIGHT, "tito-cast-glow")
            } else {
                (PRIMARY, "tito-cast")
            };
            let text = layout::escape_xml(&word.display);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{y}\" fill=\"{fill}\" filter=\"url(#{filter})\"{family_attr}>{text}</text>"
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
                <filter id="tito-cast" x="-30%" y="-60%" width="160%" height="260%">
                    <feDropShadow dx={cast1_d} dy={cast1_d} stdDeviation={0.0} flood-color={SHADOW_COLOR} flood-opacity={1.0} result="s1"/>
                    <feDropShadow in="s1" dx={0.0} dy={cast2_dy} stdDeviation={cast2_std} flood-color={SHADOW_COLOR} flood-opacity={0.45}/>
                </filter>
                <filter id="tito-cast-glow" x="-30%" y="-60%" width="160%" height="260%">
                    <feDropShadow dx={0.0} dy={0.0} stdDeviation={glow_std} flood-color={HIGHLIGHT} flood-opacity={1.0} result="g"/>
                    <feDropShadow in="g" dx={cast1_d} dy={cast1_d} stdDeviation={0.0} flood-color={SHADOW_COLOR} flood-opacity={1.0} result="s1"/>
                    <feDropShadow in="s1" dx={0.0} dy={cast2_dy} stdDeviation={cast2_std} flood-color={SHADOW_COLOR} flood-opacity={0.45}/>
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
    fn embedded_bungee_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(BUNGEE_TTF, 0).unwrap();
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
    fn tito_splitter_is_sentence_plus_thirty_chars() {
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
        // Sentence boundary: no split before the period.
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

    #[test]
    fn fade_in_runs_for_a_quarter_second() {
        let fade = recipes::FadeIn::with(0.0, FADE_DURATION_S);
        assert_eq!(fade.eval(-0.1), 0.0);
        assert!(fade.eval(0.125) > 0.5);
        assert!((fade.eval(0.25) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bob_staggers_words_ten_apart_from_the_segment_start() {
        // Word 0 bobs from the segment start; word 2 only 0.20s later.
        let w0 = recipes::Bob::with(BOB_DISTANCE_EM, 1.0);
        let w2 = recipes::Bob::with(BOB_DISTANCE_EM, 1.2);
        // A quarter-cycle in (0.4s) the first word is well off its rest
        // position; word 2 has not started yet.
        let mid0 = w0.eval(1.4);
        assert!(mid0 < -0.05, "bobbing, got {mid0}");
        assert_eq!(w2.eval(1.1), 0.0);
        // Looping: same phase one duration later.
        assert!((w0.eval(1.4) - w0.eval(3.0)).abs() < 1e-6);
    }
}
