//! Native port of the tscaps `ada` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\ada\{style.scss,template.json}`.
//! One segment is on screen at a time (gap_free windows), a segment is always
//! a single line (`balanced-pixel-width` with `maxLines: 1`), and the only
//! per-frame change is the word colour state:
//! not-narrated → `#a6a6a6`, being/already → `#1c1e1d`.
//!
//! Shared stages (tokenizer, splitters, gap_free, layout helpers) live in
//! [`super::pipeline`] and [`super::layout`]; script stacks and direction
//! live in [`super::stack`]; this module owns ada's typography and plate.

use fframes::{FFramesContext, Svgr};
use std::sync::OnceLock;

use super::{effects, layout, pipeline, stack};
use crate::CaptionDocument;

// ── spec constants (cqh/em values from ada's style.scss) ──────────────────
const FONT_POPPINS: &str = "Poppins";
const FONT_WEIGHT: u16 = 700;
const FONT_SIZE_CQH: f32 = 0.028; // font-size: 2.8cqh
const LINE_HEIGHT: f32 = 1.16;
const WORD_SPACING_EM: f32 = 0.11; // .word margin: 0 <spacing>
const PLATE_PAD_X_EM: f32 = 1.3; // .line::before inset x
const PLATE_PAD_Y_EM: f32 = 0.2; // .line::before inset y
const PLATE_RADIUS_EM: f32 = 0.3; // border-radius
const SHADOW_DY_EM: f32 = 0.03; // box-shadow: 0 0.03em 0.11em
const SHADOW_BLUR_EM: f32 = 0.11;
const SHADOW_ALPHA: f32 = 0.12;
const ANCHOR_Y_FRAC: f32 = 0.78; // alignment.verticalOffset
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset
const MAX_CHARS: usize = 26; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 8; // boundary_score_limit_by_chars.minChars
const MAX_GAP_MS: f32 = 1000.0; // gap_free default maxGapMs
const PRIMARY: &str = "#1c1e1d";
const UPCOMING: &str = "#a6a6a6";
const BG: &str = "#e7e5e7";

/// ada's segment pipeline: sentence boundary → char limit → gap_free.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: MAX_GAP_MS,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ──────────────────────────────────────────────────────────────────
// Script stacks and direction live in [`super::stack`] (shared with every
// template); ada's stack is the tscaps catalog's "Poppins" entry - latin →
// Poppins, arabic → Cairo, hebrew → Assistant. The Latin face stays
// embedded here so its metrics drive the baseline model and its upem is
// the exact measurement scale.

const POPPINS_TTF: &[u8] = include_bytes!("../../../assets/poppins-latin-700-normal.ttf");


/// `(ascent, descent, unitsPerEm)` with the first two as em ratios from
/// the embedded Poppins; script faces resolve through
/// [`stack::script_metrics`].
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: OnceLock<(f32, f32, f32)> = OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(POPPINS_TTF, 0).expect("Poppins parses");
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
    if dominant == FONT_POPPINS {
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

    let rtl = segment.direction.is_rtl();

    let video_w = ctx.current_video_size.width;
    let script_stack = stack::Stack::for_latin(FONT_POPPINS);
    let video_h = ctx.current_video_size.height;
    let fs = FONT_SIZE_CQH * video_h as f32;
    let margin = WORD_SPACING_EM * fs; // one side of `.word` margin
    let line_h = LINE_HEIGHT * fs;
    let (asc_ratio, desc_ratio, _) = line_metrics(stack::dominant_family(&segment.words, &script_stack));

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        POPPINS_TTF,
        FONT_WEIGHT,
        ctx.font_source,
        fs,
        0.0, // ada has no letter-spacing
        false,
    );
    let widths = measured.widths;

    let line_w: f32 = widths.iter().map(|w| w + 2.0 * margin).sum();
    let y_bottom = ANCHOR_Y_FRAC * video_h as f32;
    let x_center = ANCHOR_X_FRAC * video_w as f32;
    let seg_top = y_bottom - line_h;
    let line_left = x_center - line_w / 2.0;

    // Half-leading model: line box = line-height, content area = ascent +
    // descent, baseline sits half the leading below the content-area top.
    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;
    let baseline = seg_top + half_leading + ascent;

    // Plate: `.line::before` inset (−padY, −padX), radius, drop shadow.
    let plate_x = line_left - PLATE_PAD_X_EM * fs;
    let plate_y = seg_top - PLATE_PAD_Y_EM * fs;
    let plate_w = line_w + 2.0 * PLATE_PAD_X_EM * fs;
    let plate_h = line_h + 2.0 * PLATE_PAD_Y_EM * fs;
    let plate_r = PLATE_RADIUS_EM * fs;
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0; // CSS blur radius → σ

    let xs = layout::word_x_positions(&widths, margin, line_left, line_w, rtl);
    let mut words_svg = format!(
        "<g font-family=\"{FONT_POPPINS}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\">"
    );
    for ((word, &x), family) in segment.words.iter().zip(&xs).zip(&measured.families) {
        let fill = if now_ms >= word.start_ms {
            PRIMARY
        } else {
            UPCOMING
        };
        let family_attr = if *family == FONT_POPPINS {
            String::new()
        } else {
            format!(" font-family=\"{family}\"")
        };
        let text = layout::escape_xml(&word.display);
        words_svg.push_str(&format!(
            "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}>{text}</text>"
        ));
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
                <filter id="ada-plate-shadow" x="-30%" y="-30%" width="160%" height="160%">
                    <feDropShadow dx="0" dy={shadow_dy} stdDeviation={shadow_std} flood-color="#000000" flood-opacity={SHADOW_ALPHA}/>
                </filter>
            </defs>
            <rect x={plate_x} y={plate_y} width={plate_w} height={plate_h} rx={plate_r} ry={plate_r} fill={BG} filter="url(#ada-plate-shadow)"/>
            {words_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use pipeline::TimedWord;

    fn word(text: &str) -> TimedWord {
        TimedWord::new(text, 0.0, 100.0)
    }

    fn doc_with(texts: &[&str]) -> CaptionDocument {
        CaptionDocument {
            duration_frames: 300,
            cues: texts
                .iter()
                .enumerate()
                .map(|(i, text)| crate::Cue {
                    start_ms: i as u32 * 2000,
                    end_ms: i as u32 * 2000 + 1500,
                    text: (*text).into(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn script_stack() -> stack::Stack {
        stack::Stack::for_latin(FONT_POPPINS)
    }

    #[test]
    fn script_families_follow_stacks_json() {
        let s = script_stack();
        assert_eq!(stack::word_family("مرحبا", &s), stack::FONT_CAIRO);
        assert_eq!(stack::word_family("שלום", &s), stack::FONT_ASSISTANT);
        assert_eq!(stack::word_family("عام.", &s), stack::FONT_CAIRO);
        assert_eq!(stack::word_family("؟", &s), stack::FONT_CAIRO);
        assert_eq!(stack::word_family("hello", &s), FONT_POPPINS);
        assert_eq!(stack::word_family("123", &s), FONT_POPPINS);
    }

    #[test]
    fn document_direction_uses_strong_majority() {
        use stack::Direction;
        assert_eq!(
            stack::detect_document_direction(&doc_with(&["مرحبا بالعالم"])),
            Direction::Rtl
        );
        assert_eq!(
            stack::detect_document_direction(&doc_with(&["Hello world"])),
            Direction::Ltr
        );
        // One strong char each → tie → LTR.
        assert_eq!(
            stack::detect_document_direction(&doc_with(&["a ש"])),
            Direction::Ltr
        );
        // Digits are neutral, never RTL.
        assert_eq!(
            stack::detect_document_direction(&doc_with(&["123 456"])),
            Direction::Ltr
        );
        assert_eq!(
            stack::detect_document_direction(&CaptionDocument::default()),
            Direction::Ltr
        );
    }

    #[test]
    fn line_direction_only_flips_single_direction_lines() {
        use stack::Direction;
        assert_eq!(
            stack::resolve_line_direction("مرحبا", Direction::Ltr),
            Direction::Rtl
        );
        assert_eq!(
            stack::resolve_line_direction("hello", Direction::Rtl),
            Direction::Ltr
        );
        assert_eq!(
            stack::resolve_line_direction("hello", Direction::Ltr),
            Direction::Ltr
        );
        // Mixed line keeps the declared direction.
        assert_eq!(
            stack::resolve_line_direction("hello مرحبا", Direction::Ltr),
            Direction::Ltr
        );
        assert_eq!(
            stack::resolve_line_direction("hello مرحبا", Direction::Rtl),
            Direction::Rtl
        );
        // No strong chars → declared direction stands.
        assert_eq!(
            stack::resolve_line_direction("123", Direction::Rtl),
            Direction::Rtl
        );
        assert_eq!(
            stack::resolve_line_direction("", Direction::Ltr),
            Direction::Ltr
        );
    }

    #[test]
    fn embedded_font_has_expected_identity_and_metrics() {
        let face = fframes::ttf_parser::Face::parse(POPPINS_TTF, 0).unwrap();
        assert_eq!(face.weight().to_number(), 700);
        let mut typographic = None;
        let mut legacy = None;
        for name in face.names() {
            let Some(text) = name.to_string() else { continue };
            match name.name_id {
                fframes::ttf_parser::name_id::TYPOGRAPHIC_FAMILY => typographic = Some(text),
                fframes::ttf_parser::name_id::FAMILY => legacy = Some(text),
                _ => {}
            }
        }
        assert_eq!(typographic.or(legacy).unwrap(), FONT_POPPINS);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent for Poppins");
        assert!(desc > 0.05 && desc < 1.0, "descent for Poppins");
        assert!(upem >= 16.0, "upem for Poppins");
        // Script faces resolve through the shared module.
        let (c_asc, c_desc, c_upem) = stack::script_metrics(stack::FONT_CAIRO).unwrap();
        assert!(c_asc > 0.5 && c_asc < 1.5);
        assert!(c_desc > 0.05 && c_desc < 1.0);
        assert!(c_upem >= 16.0);
    }

    #[test]
    fn dominant_family_prefers_strongest_script() {
        let s = script_stack();
        let arabic: Vec<TimedWord> = ["مرحبا", "بالعالم"]
            .iter()
            .map(|t| word(t))
            .collect();
        assert_eq!(stack::dominant_family(&arabic, &s), stack::FONT_CAIRO);
        let latin: Vec<TimedWord> = ["Hello", "world"].iter().map(|t| word(t)).collect();
        assert_eq!(stack::dominant_family(&latin, &s), FONT_POPPINS);
        // Tie → Poppins.
        let mixed: Vec<TimedWord> = ["hello", "مرحبا"].iter().map(|t| word(t)).collect();
        assert_eq!(stack::dominant_family(&mixed, &s), FONT_POPPINS);
        let hebrew: Vec<TimedWord> = ["שלום", "עולם"].iter().map(|t| word(t)).collect();
        assert_eq!(
            stack::dominant_family(&hebrew, &s),
            stack::FONT_ASSISTANT
        );
    }
}
