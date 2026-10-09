//! Native port of the tscaps `kel` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\kel\{style.scss,template.json}`.
//! JetBrains Mono at 2.6cqh on a per-word dark strip: every word sits in
//! its own padded plate (`#1e1e1e`, 0.3em × 0.35em) that the negative
//! word-spacing welds into one continuous pill with rounded line ends,
//! and the spoken word lights `#569cd6` on `#252526`. There is no
//! entrance at all — colour state is the only motion. Sentence segments
//! of 48 chars (min 12), up to two rows of 0.78W, block bottom-anchored
//! at 0.78H.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, pipeline, stack, state};
use crate::CaptionDocument;

// ── spec constants (from kel's style.scss + template.json) ────────────────
const FONT: &str = "JetBrains Mono"; // fontStack jetbrains-mono ('JetBrains Mono Variable')
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.026; // font-size: 2.6cqh
const LINE_HEIGHT: f32 = 1.4; // line-height: 1.4
const WORD_SPACING_EM: f32 = -0.02; // .word margin: 0 var(--tscaps-word-spacing, -0.02em)
const LINE_SPACING_EM: f32 = 0.25; // .line + .line margin-top: 0.25em
const ANCHOR_Y_FRAC: f32 = 0.78; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 48; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 12; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.78; // lineSplitter.maxWidthRatio
const PRIMARY: &str = "#d4d4d4"; // --tscaps-primary-color default
const UPCOMING: &str = "#808080"; // --tscaps-upcoming-color default
const HIGHLIGHT: &str = "#569cd6"; // --tscaps-highlight-color default
const BG: &str = "#1e1e1e"; // --tscaps-bg-color default
const HIGHLIGHT_BG: &str = "#252526"; // --tscaps-highlight-bg-color default
const PAD_X_EM: f32 = 0.35; // --tscaps-word-padding-x default
const PAD_Y_EM: f32 = 0.3; // --tscaps-word-padding-y default
const RADIUS_EM: f32 = 0.18; // --tscaps-bg-radius default

/// kel's `effects` array: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const JETBRAINS_TTF: &[u8] = include_bytes!("../../../assets/jetbrains-mono-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// JetBrains Mono.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(JETBRAINS_TTF, 0)
            .expect("JetBrains Mono parses");
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

// ── narration state (`.word-not-narrated-yet` / `-being-` / `-already-`) ──

/// Glyph colour for the word's narration state.
fn fill_at(now_ms: f32, word_start: f32, word_end: f32) -> &'static str {
    match state::word_narration(now_ms, word_start, word_end) {
        state::Narration::NotYet => UPCOMING,
        state::Narration::Being => HIGHLIGHT,
        state::Narration::Already => PRIMARY,
    }
}

/// Plate colour: only the spoken word swaps its background.
fn bg_at(now_ms: f32, word_start: f32, word_end: f32) -> &'static str {
    if state::word_narration(now_ms, word_start, word_end).is_being() {
        HIGHLIGHT_BG
    } else {
        BG
    }
}

// ── plate geometry ────────────────────────────────────────────────────────

/// One painted word plate as a path: the corners the strip's *ends* need
/// are rounded, the interior ones stay square so the plates read as one
/// continuous pill.
fn plate_path(x: f32, y: f32, w: f32, h: f32, tl: f32, tr: f32, br: f32, bl: f32) -> String {
    let clamp = |r: f32| r.clamp(0.0, w / 2.0).min(h / 2.0);
    let (tl, tr, br, bl) = (clamp(tl), clamp(tr), clamp(br), clamp(bl));
    let mut d = format!("M {} {y}", x + tl);
    d.push_str(&format!(" L {} {y}", x + w - tr));
    if tr > 0.0 {
        d.push_str(&format!(" A {tr} {tr} 0 0 1 {} {}", x + w, y + tr));
    }
    d.push_str(&format!(" L {} {}", x + w, y + h - br));
    if br > 0.0 {
        d.push_str(&format!(" A {br} {br} 0 0 1 {} {}", x + w - br, y + h));
    }
    d.push_str(&format!(" L {} {}", x + bl, y + h));
    if bl > 0.0 {
        d.push_str(&format!(" A {bl} {bl} 0 0 1 {x} {}", y + h - bl));
    }
    d.push_str(&format!(" L {x} {}", y + tl));
    if tl > 0.0 {
        d.push_str(&format!(" A {tl} {tl} 0 0 1 {} {y}", x + tl));
    }
    d.push_str(" Z");
    d
}

/// Which corners the word at `index` of a `count`-word line rounds: the
/// painted strip's ends, never the first/last word *spoken*.
fn rounded_corners(index: usize, count: usize) -> (f32, f32, f32, f32) {
    let left = (index == 0) as usize as f32 * RADIUS_EM;
    let right = (index + 1 == count) as usize as f32 * RADIUS_EM;
    (left, right, right, left)
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
    let margin = WORD_SPACING_EM * fs; // negative: plates touch/overlap
    let line_h = LINE_HEIGHT * fs;
    let pad_x = PAD_X_EM * fs;
    let pad_y = PAD_Y_EM * fs;
    let box_h = line_h + 2.0 * pad_y;
    let gap = LINE_SPACING_EM * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    // kel has no letter-spacing, so `ls_px` stays 0.
    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        JETBRAINS_TTF,
        FONT_WEIGHT,
        ctx.font_source,
        fs,
        0.0,
        false,
    );
    let widths = measured.widths;

    // Split on the *plate* widths: a line's budget counts the padding the
    // plates add on top of the glyphs.
    let box_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * pad_x).collect();
    let split_widths: Vec<f32> = box_widths.iter().map(|w| w + 2.0 * margin).collect();
    let splitter = layout::BalancedPixelWidth {
        min_lines: MIN_LINES,
        max_lines: MAX_LINES,
        max_width: (video_w as f32 * MAX_WIDTH_RATIO).round(),
    };
    let lines = splitter.split(&split_widths);

    // Bottom-anchored block: the stack's bottom edge sits at 0.78H, each
    // line box carries the plate padding, lines center horizontally.
    let block_h = lines.len() as f32 * box_h + lines.len().saturating_sub(1) as f32 * gap;
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - block_h;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let mut plates_svg = String::new();
    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let box_top = block_top + line_idx as f32 * (box_h + gap);
        let line_top = box_top + pad_y;
        let baseline = line_top + half_leading + ascent;
        let count = end - start;
        let line_w: f32 = box_widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&box_widths[start..end], margin, line_left, line_w, rtl);
        for (j, x) in xs.iter().enumerate() {
            let (tl, tr, br, bl) = rounded_corners(j, count);
            let d = plate_path(*x, box_top, box_widths[start + j], box_h, tl, tr, br, bl);
            let word = &segment.words[start + j];
            plates_svg.push_str(&format!(
                "<path d=\"{d}\" fill=\"{}\"/>",
                bg_at(now_ms, word.start_ms, word.end_ms)
            ));
        }
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&word.display);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            words_svg.push_str(&format!(
                "<text x=\"{}\" y=\"{baseline}\" fill=\"{}\"{family_attr}>{text}</text>",
                x + pad_x,
                fill_at(now_ms, word.start_ms, word.end_ms)
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
            {plates_svg}
            {words_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_jetbrains_mono_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(JETBRAINS_TTF, 0).unwrap();
        let mut name = None;
        for n in face.names() {
            if n.name_id == fframes::ttf_parser::name_id::FAMILY {
                name = n.to_string();
                break;
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
    fn kel_splitter_is_sentence_plus_forty_eight_chars() {
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
        assert!(segments.len() >= 2, "{segments:?}");
        assert_eq!(MIN_CHARS, 12);
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
    }

    #[test]
    fn only_the_ends_of_the_strip_round() {
        // Two-word line: left box rounds its left pair, right box its
        // right pair, and the touching pair stays square.
        assert_eq!(rounded_corners(0, 2), (RADIUS_EM, 0.0, 0.0, RADIUS_EM));
        assert_eq!(rounded_corners(1, 2), (0.0, RADIUS_EM, RADIUS_EM, 0.0));
        // A single-word line is the whole strip: all four corners.
        assert_eq!(rounded_corners(0, 1), (RADIUS_EM, RADIUS_EM, RADIUS_EM, RADIUS_EM));
        // Interior word of a long line rounds nothing.
        assert_eq!(rounded_corners(1, 3), (0.0, 0.0, 0.0, 0.0));
        // The path spans the plate box exactly: top-left corner in, right
        // side straight down (square), bottom-left corner back out.
        let d = plate_path(8.0, 16.0, 64.0, 32.0, 8.0, 0.0, 0.0, 8.0);
        assert!(d.starts_with("M 16 16"), "{d}");
        assert!(d.contains("L 72 16 L 72 48 L 16 48"), "{d}");
        assert!(d.contains("A 8 8 0 0 1 8 40"), "{d}");
        assert!(d.ends_with('Z'), "{d}");
    }

    #[test]
    fn plates_and_glyphs_follow_the_narration_state() {
        // 100..200 is the narration window.
        assert_eq!(fill_at(50.0, 100.0, 200.0), UPCOMING);
        assert_eq!(fill_at(150.0, 100.0, 200.0), HIGHLIGHT);
        assert_eq!(fill_at(250.0, 100.0, 200.0), PRIMARY);
        assert_eq!(bg_at(50.0, 100.0, 200.0), BG);
        assert_eq!(bg_at(150.0, 100.0, 200.0), HIGHLIGHT_BG);
        assert_eq!(bg_at(250.0, 100.0, 200.0), BG);
    }
}
