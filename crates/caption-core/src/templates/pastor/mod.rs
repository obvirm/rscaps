//! Native port of the tscaps `pastor` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\pastor\{style.scss,template.json}`.
//! Two typographic registers.
//!
//! Quiet (what renders under [`BEHIND_ACTOR`] = `false`): Manrope 500 at
//! 3.6cqh in `#e6d647`, 0.13em word margins, no line gap, one line
//! top-anchored at 0.75H and centred at 0.5W, wearing the template's
//! `soft-drop-shadow(0.06, 0.35)` — `text-shadow` reads its third length
//! as a radius, so the filter carries `dy 0.06em`, `σ 0.175em`, opaque
//! black. Clause-boundary segments (5–13 chars) with `gap_free` +
//! `remove_punctuation`; no entrance animation.
//!
//! Behind the actor ([`BEHIND_ACTOR`] = `true`): Anton 400 uppercase at
//! line-height 0.95, the block's top edge pinned at 0.08H (the scss
//! undoes the quiet anchor and re-lays the box at `behind-y`), every line
//! sized from its own measured width at one em —
//! `clamp(4cqh, behind-fill × 100cqw / line-width-em, 30cqh)` — and a
//! line-level rise-in (0.18em, 0.34s, `cubic-bezier(0.22, 1, 0.36, 1)`)
//! keyed off when the line starts being narrated. The scss stacks the
//! word shadow twice here; both copies are the same opaque black, so the
//! chain is emitted twice to stay faithful (they only part company under
//! a translucent shadow-colour control, which the port does not carry).
//!
//! Unported: the `behind-actor-active` *switch* — tscaps drives it from a
//! per-segment behind-actor flag the native `Cue` does not carry, so
//! [`BEHIND_ACTOR`] is the const toggle the render reads. The
//! `behind-font-family` / `behind-y` / `behind-fill` / shadow / colour
//! controls keep their defaults.
//!
//! Family note: the embedded Manrope 500 reports the name-table family
//! "Manrope ExtraLight" (fontsource packaging) — that is the string fontdb
//! resolves (`FONTDB_FAMILY`, used to measure), while the SVG emits `FONT`
//! so the studio's @font-face ("Manrope") matches in the browser. The
//! behind register measures against Anton's own face.

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{effects, layout, pipeline, recipes, state};
use crate::CaptionDocument;

// ── spec constants (from pastor's style.scss + template.json) ─────────────
/// `false` renders the quiet register; `true` the behind-actor one.
const BEHIND_ACTOR: bool = false;
const FONT: &str = "Manrope"; // fontStack manrope (browser @font-face)
const FONTDB_FAMILY: &str = "Manrope ExtraLight"; // TTF name-table family (measure)
const FONT_WEIGHT: u16 = 500; // typography.fontWeight: 500
const FONT_SIZE_CQH: f32 = 0.036; // font-size: 3.6cqh
const LETTER_SPACING_EM: f32 = 0.0; // typography.letterSpacing: 0
const LINE_HEIGHT: f32 = 1.25; // line-height: 1.25
const WORD_SPACING_EM: f32 = 0.13; // word-layout(0.13em) → .word margin
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top: 0 (lineSpacing)
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 13; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 5; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines (type "balanced")
const MAX_LINES: usize = 1; // lineSplitter.maxLines — one line
const MAX_WIDTH_RATIO: f32 = 0.72; // unused at maxLines 1, kept explicit
const MAX_GAP_MS: f32 = 1000.0; // gap_free default maxGapMs
const PRIMARY: &str = "#e6d647"; // primary-color control default
// soft-drop-shadow(0.06, 0.35) → text-shadow: 0 0.06em 0.35em #000000
// (text-shadow blur is a radius → σ = r / 2; the colour is opaque).
const SHADOW_DY_EM: f32 = 0.06;
const SHADOW_BLUR_EM: f32 = 0.35;
// ── behind register ──────────────────────────────────────────────────────
const BEHIND_FONT: &str = "Anton"; // behind-font-family control default: "anton"
const BEHIND_FONTDB_FAMILY: &str = "Anton"; // TTF name-table family (measure)
const BEHIND_FONT_WEIGHT: u16 = 400; // Anton ships one static face
const BEHIND_LINE_HEIGHT: f32 = 0.95; // line-height: 0.95
const BEHIND_Y_FRAC: f32 = 0.08; // --tscaps-behind-y control default
const BEHIND_FILL: f32 = 0.98; // --tscaps-behind-fill control default
const BEHIND_MIN_CQH: f32 = 0.04; // clamp() floor: 4cqh
const BEHIND_MAX_CQH: f32 = 0.30; // clamp() ceiling: 30cqh
// rise-in($distance: 0.18em, $duration: 0.34s, cubic-bezier(0.22,1,0.36,1))
const RISE_DISTANCE_EM: f32 = 0.18;
const RISE_DURATION_S: f32 = 0.34;
// The scss declares the word shadow twice in this register (same call
// stacked); both copies are opaque black, so the chain repeats verbatim.
const BEHIND_SHADOW_COPIES: usize = 2;
const QUIET_SHADOW_COPIES: usize = 1;

/// pastor's pipeline: clause boundary → 5–13 char limit → gap_free +
/// remove_punctuation. The line splitter is `{ type: "balanced",
/// minLines: 1, maxLines: 1 }` — one line whatever the algorithm, which
/// is what `BalancedPixelWidth` at `max_lines` 1 yields.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: MAX_GAP_MS,
    effects: &[
        effects::Effect::GapFree,
        effects::Effect::RemovePunctuation,
    ],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const MANROPE_500_TTF: &[u8] = include_bytes!("../../../assets/manrope-latin-500-normal.ttf");
const ANTON_400_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// Quiet register metrics (Manrope 500).
fn quiet_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face =
            fframes::ttf_parser::Face::parse(MANROPE_500_TTF, 0).expect("Manrope parses");
        let upem = face.units_per_em() as f32;
        (
            face.ascender() as f32 / upem,
            face.descender().abs() as f32 / upem,
            upem,
        )
    })
}

/// Behind register metrics (Anton 400) — the face whose line-height 0.95
/// and width-at-one-em the clamp solves against.
fn behind_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(ANTON_400_TTF, 0).expect("Anton parses");
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
fn measure_word(
    face: &dyn fframes::FontFace,
    upem: usize,
    fs: f32,
    ls_px: f32,
    text: &str,
) -> f32 {
    let fallback = text.encode_utf16().count() as f32 * fs * 0.5;
    let advance = match face.font_variant(upem) {
        Some(variant) => layout::measure_units(face, variant, upem, text) as f32 * fs / upem as f32,
        None => fallback,
    };
    advance + text.encode_utf16().count() as f32 * ls_px
}

/// `clamp(4cqh, behind-fill × 100cqw / line-width-em, 30cqh)` — the line
/// fills the frame whatever letters it holds, until the vertical ceiling
/// takes over on a wide frame.
fn behind_font_size(line_w_em: f32, video_w: f32, video_h: f32) -> f32 {
    (BEHIND_FILL * video_w / line_w_em).clamp(BEHIND_MIN_CQH * video_h, BEHIND_MAX_CQH * video_h)
}

/// The `soft-drop-shadow(0.06, 0.35)` chain: `copies` identical opaque
/// black shadows, each fed by the previous one's result.
fn word_shadow_filter(copies: usize, dy: f32, std: f32) -> String {
    let mut out = String::new();
    let mut input = String::from("SourceAlpha");
    for i in 0..copies {
        let result = format!("s{i}");
        out.push_str(&format!(
            "<feDropShadow in=\"{input}\" dx=\"0\" dy=\"{dy}\" stdDeviation=\"{std}\" \
             flood-color=\"#000000\" flood-opacity=\"1.0\" result=\"{result}\"/>"
        ));
        input = result;
    }
    out
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

    // The register picks the face, the case, the metrics and the anchor.
    let (svg_font, db_family, font_weight) = if BEHIND_ACTOR {
        (BEHIND_FONT, BEHIND_FONTDB_FAMILY, BEHIND_FONT_WEIGHT)
    } else {
        (FONT, FONTDB_FAMILY, FONT_WEIGHT)
    };
    let (asc_ratio, desc_ratio, upem) = if BEHIND_ACTOR {
        behind_metrics()
    } else {
        quiet_metrics()
    };
    // `text-transform: uppercase` is part of the behind register, and the
    // width the clamp solves against is measured after it.
    let display: Vec<String> = segment
        .words
        .iter()
        .map(|w| {
            if BEHIND_ACTOR {
                w.display.to_uppercase()
            } else {
                w.display.clone()
            }
        })
        .collect();

    let probe = FontQuery {
        family: db_family,
        size: 1,
        weight: font_weight,
        ..Default::default()
    };
    let face = ctx
        .font_source
        .and_then(|source| source.resolve_font(db_family, probe.weight, probe.style, probe.stretch));

    // Measure at one em so the behind clamp can divide a target width by
    // the line's cost at that em; the quiet register scales the same
    // numbers by its fixed size.
    let unit_widths: Vec<f32> = display
        .iter()
        .map(|text| match &face {
            Some(face) => measure_word(face.as_ref(), upem as usize, 1.0, LETTER_SPACING_EM, text),
            None => text.encode_utf16().count() as f32 * 0.5,
        })
        .collect();

    let fs = if BEHIND_ACTOR {
        let line_w_em: f32 = unit_widths
            .iter()
            .map(|w| w + 2.0 * WORD_SPACING_EM)
            .sum();
        behind_font_size(line_w_em, video_w as f32, video_h as f32)
    } else {
        FONT_SIZE_CQH * video_h as f32
    };
    let margin = WORD_SPACING_EM * fs;
    let ls_px = LETTER_SPACING_EM * fs;
    let line_h = if BEHIND_ACTOR {
        BEHIND_LINE_HEIGHT * fs
    } else {
        LINE_HEIGHT * fs
    };
    let line_gap = LINE_SPACING_EM * fs;
    let widths: Vec<f32> = unit_widths.iter().map(|w| w * fs).collect();

    let splitter = layout::BalancedPixelWidth {
        min_lines: MIN_LINES,
        max_lines: MAX_LINES,
        max_width: (video_w as f32 * MAX_WIDTH_RATIO).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Quiet: top edge on the 0.75H anchor. Behind: the scss re-anchors the
    // box so its top edge lands on `behind-y` however tall it grew.
    let block_top = if BEHIND_ACTOR {
        BEHIND_Y_FRAC * video_h as f32
    } else {
        ANCHOR_Y_FRAC * video_h as f32
    };
    let center_x = ANCHOR_X_FRAC * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // soft-drop-shadow(0.06, 0.35): blur is a radius in `text-shadow`.
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;
    let shadow_copies = if BEHIND_ACTOR {
        BEHIND_SHADOW_COPIES
    } else {
        QUIET_SHADOW_COPIES
    };
    let defs_svg = format!(
        "<filter id=\"pastor-word\" x=\"-40%\" y=\"-60%\" width=\"180%\" height=\"260%\" \
         color-interpolation-filters=\"sRGB\">{}</filter>",
        word_shadow_filter(shadow_copies, shadow_dy, shadow_std)
    );

    let mut words_svg = format!(
        "<g font-family=\"{svg_font}\" font-weight=\"{font_weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" fill=\"{PRIMARY}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + line_gap);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, false);

        // Behind: the line rises in as its own narration starts (the
        // delay `--on-line-being-narrated-starts` resolves to); quiet
        // lines just are.
        let (opacity, dy_em) = if BEHIND_ACTOR {
            let span = state::LineSpan::of_words(&segment.words[start..end]);
            recipes::RiseIn::custom(
                RISE_DISTANCE_EM,
                0.0,
                RISE_DURATION_S,
                recipes::ease_entrance,
            )
            .eval((now_ms - span.start_ms) / 1000.0)
        } else {
            (1.0, 0.0)
        };
        let dy = dy_em * fs;
        words_svg.push_str(&format!(
            "<g opacity=\"{opacity}\" transform=\"translate(0,{dy})\">"
        ));
        for (x, text) in xs.iter().zip(&display[start..end]) {
            let text = layout::escape_xml(text);
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" filter=\"url(#pastor-word)\">{text}</text>"
            ));
        }
        words_svg.push_str("</g>");
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
                {defs_svg}
            </defs>
            {words_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn family_of(ttf: &[u8]) -> Option<String> {
        let face = fframes::ttf_parser::Face::parse(ttf, 0).unwrap();
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
        name
    }

    #[test]
    fn embedded_manrope_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(MANROPE_500_TTF, 0).unwrap();
        assert_eq!(family_of(MANROPE_500_TTF).as_deref(), Some(FONTDB_FAMILY));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = quiet_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn embedded_anton_is_the_behind_register_face() {
        let face = fframes::ttf_parser::Face::parse(ANTON_400_TTF, 0).unwrap();
        assert_eq!(family_of(ANTON_400_TTF).as_deref(), Some(BEHIND_FONTDB_FAMILY));
        // Single static face — what the weight-400 request resolves to.
        assert_eq!(face.weight().to_number(), 400);
        let (asc, desc, upem) = behind_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn behind_line_size_is_solved_from_width_then_clamped() {
        // A line costing ten em fills 98% of the frame width.
        let fs = behind_font_size(10.0, 1920.0, 1080.0);
        assert!((fs - (0.98 * 1920.0 / 10.0)).abs() < 1e-3, "fs {fs}");
        // One em would ask for a fifth of the frame's height twice over —
        // the 30cqh ceiling takes over.
        assert!((behind_font_size(1.0, 1920.0, 1080.0) - 0.30 * 1080.0).abs() < 1e-6);
        // A hundred em cannot reach the width either — the 4cqh floor.
        assert!((behind_font_size(100.0, 1920.0, 1080.0) - 0.04 * 1080.0).abs() < 1e-6);
    }

    #[test]
    fn clause_boundary_and_the_char_limit_shape_the_segments() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Well, my friends, today we gather.".into(),
            }],
            ..Default::default()
        };
        let segments: Vec<String> = pipeline::collect_segments(&doc, &SPLIT)
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        // Clause flush at both commas; the last group exceeds 13 chars and
        // is cut by the char limit (min 5).
        assert_eq!(segments, vec!["Well,", "my friends,", "today we", "gather."]);
    }

    #[test]
    fn remove_punctuation_strips_the_trailing_dot_from_display_only() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Hello there.".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let display: Vec<&str> = segments[0].words.iter().map(|w| w.display.as_str()).collect();
        let text: Vec<&str> = segments[0].words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(display, vec!["Hello", "there"]);
        assert_eq!(text, vec!["Hello", "there."]);
    }

    #[test]
    fn the_word_shadow_repeats_only_in_the_behind_register() {
        let dy = SHADOW_DY_EM * 100.0;
        let std = SHADOW_BLUR_EM * 100.0 / 2.0;
        let quiet = word_shadow_filter(QUIET_SHADOW_COPIES, dy, std);
        let behind = word_shadow_filter(BEHIND_SHADOW_COPIES, dy, std);
        assert_eq!(quiet.matches("feDropShadow").count(), 1);
        assert_eq!(behind.matches("feDropShadow").count(), 2);
        assert!(quiet.contains("stdDeviation=\"17.5\""), "{quiet}");
    }
}
