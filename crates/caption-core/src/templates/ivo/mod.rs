//! Native port of the tscaps `ivo` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\ivo\{style.scss,template.json}`.
//! Anton uppercase on two plate rows: the first line sits on a black
//! bar (white text), the last on a lime tape (`#adff2f`, black text)
//! that lands crooked — `wobble-in(swing 9°, rest -2°)` keeps it
//! tilted, and the rows overlap by 0.09em so the tape rides over the
//! bar. The segment scales in from 0.94 with a 1.03 overshoot over
//! the default 0.32s. Sentence segments (35/8, `gap_free`), always
//! two balanced rows of 0.67W, block top-anchored at 0.72H.

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{effects, layout, pipeline, recipes};
use crate::CaptionDocument;

// ── spec constants (from ivo's style.scss + template.json) ────────────────
const FONT: &str = "Anton"; // fontStack anton
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.0508; // font-size: 5.08cqh
const LINE_HEIGHT: f32 = 1.0; // line-height: 1
const WORD_SPACING_EM: f32 = 0.12; // .word margin: 0 0.12em
const ANCHOR_Y_FRAC: f32 = 0.72; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 35; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 8; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 2; // lineSplitter.minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.67; // lineSplitter.maxWidthRatio
// typography.lineSpacing: -0.09 (the json value the app binds to
// `--tscaps-line-spacing`; the scss var fallback reads -0.15em).
const LINE_SPACING_EM: f32 = -0.09;
const BG_PAD_X_EM: f32 = 0.23; // --tscaps-bg-padding-x default
const BG_PAD_Y_EM: f32 = 0.09; // --tscaps-bg-padding-y default
const BG_RADIUS_EM: f32 = 0.09; // --tscaps-bg-radius default
const PRIMARY: &str = "#ffffff"; // --tscaps-primary-color default
const PRIMARY_BG: &str = "#000000"; // --tscaps-primary-bg default
const ACCENT: &str = "#000000"; // --tscaps-accent-color default
const ACCENT_BG: &str = "#adff2f"; // --tscaps-accent-bg default
const LINE_ROTATE_DEG: f32 = -2.0; // .line:last-child rotate(-2deg)
const WOBBLE_SWING_DEG: f32 = 9.0; // wobble-in($swing: 9deg, $rest: -2deg)
const SCALE_FROM: f32 = 0.94; // scale-in($from: 0.94)
const SCALE_PEAK: f32 = 1.03; // scale-in($peak: 1.03)

/// ivo's `effects` array: `gap_free`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const ANTON_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the
/// embedded Anton.
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

/// `text-transform: uppercase` — the uppercased form is what gets
/// measured *and* emitted.
fn uppercase(text: &str) -> String {
    text.to_uppercase()
}

/// `scale-in($element: 'segment', $from: 0.94, $peak: 1.03,
/// $from-opacity: 0)` — duration and easing are the mixin defaults.
fn scale_in() -> recipes::ScaleIn {
    recipes::ScaleIn::with(SCALE_FROM, 0.0).peak(SCALE_PEAK)
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
    let pad_x = BG_PAD_X_EM * fs;
    let pad_y = BG_PAD_Y_EM * fs;
    let box_h = line_h + 2.0 * pad_y;
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
        let upper = uppercase(&word.display);
        let width_px = match &face {
            Some(face) => measure_word(face.as_ref(), fs, 0.0, &upper),
            None => upper.encode_utf16().count() as f32 * fs * 0.5,
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

    // Top-aligned block: the block's top edge sits at 0.72H. The rows
    // overlap by the (negative) line-spacing so the tape rides over
    // the bar; paint order is document order, tape last.
    let block_top = ANCHOR_Y_FRAC * video_h as f32;
    let center_x = ANCHOR_X_FRAC * video_w as f32;
    let total_h = lines.len() as f32 * box_h
        + (lines.len() as f32 - 1.0) * LINE_SPACING_EM * fs;
    let block_cy = block_top + total_h / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let elapsed_s = (now_ms - segment.start_ms) / 1000.0;
    let (opacity, scale) = scale_in().eval(elapsed_s);
    let wobble = recipes::WobbleIn::with(WOBBLE_SWING_DEG, LINE_ROTATE_DEG);

    let mut body = format!(
        "<g transform=\"translate({center_x} {block_cy}) scale({scale}) \
         translate({} {})\" opacity=\"{opacity}\">",
        -center_x,
        -block_cy
    );
    body.push_str(&format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\">"
    ));
    let mut y = block_top;
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let is_last = line_idx + 1 == lines.len();
        let ws = &widths[start..end];
        let run: f32 = ws.iter().map(|w| w + 2.0 * margin).sum();
        let box_w = run + 2.0 * pad_x;
        let box_left = center_x - box_w / 2.0;
        // A lone line is both :first-child and :last-child — the
        // tape wins (later rule in the scss).
        let (bg, fg) = if is_last {
            (ACCENT_BG, ACCENT)
        } else {
            (PRIMARY_BG, PRIMARY)
        };
        let r = BG_RADIUS_EM * fs;
        if is_last {
            let angle = wobble.eval(elapsed_s);
            let cy = y + box_h / 2.0;
            body.push_str(&format!("<g transform=\"rotate({angle} {center_x} {cy})\">"));
        }
        body.push_str(&format!(
            "<rect x=\"{box_left}\" y=\"{y}\" width=\"{box_w}\" height=\"{box_h}\" \
             rx=\"{r}\" ry=\"{r}\" fill=\"{bg}\"/>"
        ));
        let line_left = box_left + pad_x;
        let xs = layout::word_x_positions(ws, margin, line_left, run, false);
        let baseline = y + pad_y + half_leading + ascent;
        for (x, word) in xs.iter().zip(&segment.words[start..end]) {
            let text = layout::escape_xml(&uppercase(&word.display));
            body.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fg}\">{text}</text>"
            ));
        }
        if is_last {
            body.push_str("</g>");
        }
        y += box_h + LINE_SPACING_EM * fs;
    }
    body.push_str("</g></g>");

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
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1) — this is the
        // string `resolve_font` matches.
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
    fn ivo_splitter_is_sentence_plus_thirty_five_chars() {
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
        // Sentence boundary closes at "Wait."; the long second sentence
        // then splits on the 35-char limit.
        assert_eq!(texts[0], "Wait.");
        assert!(segments.len() >= 2, "{texts:?}");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars} ({s:?})");
        }
    }

    #[test]
    fn the_accent_tape_wobbles_in_and_stays_crooked() {
        let wobble = recipes::WobbleIn::with(WOBBLE_SWING_DEG, LINE_ROTATE_DEG);
        // 0% is rest + swing: -2° + 9° = 7°.
        assert_eq!(wobble.eval(0.0), 7.0);
        // It damps onto the resting tilt over the mixin's 0.6s.
        assert!((wobble.eval(0.6) - LINE_ROTATE_DEG).abs() < 1e-4);
        assert!((wobble.eval(1.2) - LINE_ROTATE_DEG).abs() < 1e-4);
    }

    #[test]
    fn plates_scale_in_with_an_overshoot_and_stay_uppercase() {
        let anim = scale_in();
        let (o0, s0) = anim.eval(0.0);
        assert_eq!(o0, 0.0);
        assert!((s0 - SCALE_FROM).abs() < 1e-6, "scale {s0}");
        // The peak stop at 60% of the default 0.32s: passes 1.03
        // before settling back to 1.
        let (o1, s1) = anim.eval(0.32);
        assert_eq!(o1, 1.0);
        assert_eq!(s1, 1.0);
        let sampled_peak: f32 = (0..=32)
            .map(|i| anim.eval(i as f32 * 0.01).1)
            .fold(f32::MIN, f32::max);
        assert!(
            sampled_peak >= SCALE_PEAK - 1e-3,
            "no overshoot: {sampled_peak}"
        );
        // text-transform: uppercase on measure and emit alike.
        assert_eq!(uppercase("Hello There"), "HELLO THERE");
        assert_eq!(LINE_SPACING_EM, -0.09);
    }
}
