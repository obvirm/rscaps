//! Native port of the tscaps `anya` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\anya\{style.scss,template.json}`.
//! Playfair Display 700 in the browser's synthetic oblique at 2.97cqh,
//! cream `#fff8e7` glyphs over one soft black cast shadow, and a
//! `highlight-pulse` that swells every word toward `#f5c982` across its
//! own narration window (hold 20%-80%, `ease-in-out`, fill `both`).

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{layout, pipeline, recipes};
use crate::CaptionDocument;

// ── spec constants (from anya's style.scss + template.json) ───────────────
const FONT: &str = "Playfair Display"; // fontStack playfair-display
const FONT_WEIGHT: u16 = 700; // typography.fontWeight: 700
const FONT_SIZE_CQH: f32 = 0.0297; // font-size: 2.97cqh
const LETTER_SPACING_EM: f32 = 0.02; // letter-spacing: 0.02em
const LINE_HEIGHT: f32 = 1.35; // line-height: 1.35
const WORD_SPACING_EM: f32 = 0.13; // .word margin: 0 0.13em
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top: 0
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center, mixin default)
const MAX_CHARS: usize = 36; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // ...minChars (descriptor default; absent in json)
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#fff8e7"; // primary-color default / pulse $rest-color
const HIGHLIGHT: &str = "#f5c982"; // highlight-color default / pulse $highlight-color
// text-shadow: 0 0.11em 0.47em var(--tscaps-shadow-color, rgba(0,0,0,0.6)) —
// the styleControl's bound default is what the app emits over that fallback.
const SHADOW_COLOR: &str = "#000000"; // shadow-color styleControl default
const SHADOW_DY_EM: f32 = 0.11;
const SHADOW_BLUR_EM: f32 = 0.47;

/// anya's `segmentSplitters`: boundary (sentence) + 36 chars; no
/// `effects` array, so windows stay the raw word span.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const PLAYFAIR_700_TTF: &[u8] =
    include_bytes!("../../../assets/playfair-display-latin-700-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Playfair Display 700.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(PLAYFAIR_700_TTF, 0)
            .expect("Playfair Display parses");
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
/// Playfair ships one upright face here, so the template's
/// `font-style: italic` is the browser's synthetic oblique — it skews the
/// glyphs without changing advances, and the measurement stays upright.
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

/// `highlight-pulse` fill: the pulse's mix factor blended between the
/// resting and highlight colours (0.0 = rest, 1.0 = highlight).
fn mix_hex(rest: &str, highlight: &str, t: f32) -> String {
    let parse = |s: &str| -> [f32; 3] {
        let h = s.trim_start_matches('#');
        [0, 2, 4]
            .iter()
            .map(|i| u8::from_str_radix(&h[*i..*i + 2], 16).unwrap_or(0) as f32)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    };
    let a = parse(rest);
    let b = parse(highlight);
    let t = t.clamp(0.0, 1.0);
    let ch = |i: usize| -> u8 {
        let v = a[i] + (b[i] - a[i]) * t;
        v.round().clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(1), ch(2))
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

    // Top-aligned block: the block's top edge sits at 0.75H; lines center
    // horizontally.
    let block_top = ANCHOR_Y_FRAC * video_h as f32;
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;
    let pulse = recipes::HighlightPulse::with();

    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-style=\"italic\" \
         font-size=\"{fs}\" letter-spacing=\"{ls_px}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + LINE_SPACING_EM * fs);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, false);
        for (x, word) in xs.iter().zip(&segment.words[start..end]) {
            // Highlight pulse runs on the word's own narration window with
            // its full duration as the animation length.
            let elapsed = (now_ms - word.start_ms) / 1000.0;
            let duration = (word.end_ms - word.start_ms) / 1000.0;
            let fill = mix_hex(PRIMARY, HIGHLIGHT, pulse.eval(elapsed, duration));
            let text = layout::escape_xml(&word.display);
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\" \
                 filter=\"url(#anya-shadow)\">{text}</text>"
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
                <filter id="anya-shadow" x="-30%" y="-60%" width="160%" height="260%">
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
    fn embedded_playfair_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(PLAYFAIR_700_TTF, 0).unwrap();
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
    fn pulse_holds_the_highlight_through_the_middle_of_the_window() {
        let pulse = recipes::HighlightPulse::with();
        // Rest at the window's edges, full highlight through 20%-80%.
        assert_eq!(pulse.eval(0.0, 1.0), 0.0);
        assert_eq!(pulse.eval(1.0, 1.0), 0.0);
        assert_eq!(pulse.eval(0.5, 1.0), 1.0);
        // Monotonic climb into the hold and back out of it.
        assert!(pulse.eval(0.1, 1.0) > 0.0);
        assert!(pulse.eval(0.1, 1.0) < 1.0);
        // Before the word starts (fill both) the word rests in primary.
        assert_eq!(mix_hex(PRIMARY, HIGHLIGHT, pulse.eval(-0.2, 1.0)), PRIMARY);
        assert_eq!(mix_hex(PRIMARY, HIGHLIGHT, 1.0), HIGHLIGHT);
        assert_eq!(mix_hex(PRIMARY, HIGHLIGHT, 0.0), PRIMARY);
    }

    #[test]
    fn anya_splitter_is_sentence_plus_thirty_six_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "She waited by the door until the rain finally stopped falling."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 8, "chunk too long: {chars}");
        }
    }
}
