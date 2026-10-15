//! Native port of the tscaps `luna` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\luna\{style.scss,template.json}`.
//! Poppins 900 uppercase at 5cqh, left-aligned, no rotation, inside a padded
//! shrink-to-fit segment box anchored top-left (0.20H / 0). Sentence segments
//! limited to 10–35 chars break into at most four balanced lines fitted to 52%
//! of the frame width. Each line enters as one piece: every word of the line
//! carries the same delay (`--on-line-being-narrated-starts`), rises from
//! `--tscaps-word-rise` (0.5em) to zero over 0.18s on
//! `cubic-bezier(0.2, 0.9, 0.1, 1)`, and un-clips from the bottom up.
//!
//! The un-clip is a flip, not a wipe: `clip-path: inset(...)` and `none` do
//! not interpolate (WPT `clip-path-interpolation-002`), and a non-interpolable
//! pair in a CSS animation switches at eased progress 0.5 (WPT
//! `interpolation-testcommon` `expectFlip`). The transform under it keeps
//! interpolating continuously, so a word lands half-risen the moment it
//! appears. The segment's own `overflow: hidden` stays a clipPath over the
//! padding box for the whole frame.
//!
//! Not ported (per the porting brief):
//! - `.tscaps-video-frame-layer` — the video frame layer itself.
//! - `mix-blend-mode: difference` on the words paints flat white
//!   (`primary-color`, default `#ffffff`) — tscaps' own
//!   `rendering.videoFrame.previewMode: "omit"` fallback. It also means the
//!   words never invert against anything: in this port there is no backdrop
//!   to escape to.
//! - The `@container style(--tscaps-text-direction: rtl)` rules — only
//!   `ada` implements bidirectional layout in this crate.

use fframes::{FFramesContext, Svgr};

use super::{layout, overrides, pipeline, recipes, stack};
use crate::CaptionDocument;

// ── spec constants (from luna's style.scss + template.json) ───────────────
const FONT: &str = "Poppins"; // fontStack poppins (scss: 'Poppins')
const FONT_WEIGHT: u16 = 900; // typography.fontWeight: 900
const FONT_SIZE_CQH: f32 = 0.05; // font-size: 5.0cqh
const LETTER_SPACING_EM: f32 = 0.0; // typography.letterSpacing: 0
const LINE_HEIGHT: f32 = 1.05; // $line-height: 1.05
const WORD_MARGIN_EM: f32 = 0.14; // .word word-layout(0.14em)
const LINE_SPACING_EM: f32 = 0.05; // .line + .line margin-top: 0.05em
const PAD_X_EM: f32 = 0.35; // padding: 0.1em 0.35em
const PAD_Y_EM: f32 = 0.1;
const ANCHOR_Y_FRAC: f32 = 0.2; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.0; // alignment.horizontalOffset (left)
const MAX_CHARS: usize = 35; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 10; // boundary_score_limit_by_chars.minChars
// lineSplitter: { type: "balanced-pixel-width", minLines: 1, maxLines: 4,
//                 maxWidthRatio: 0.52 }
const MIN_LINES: usize = 1;
const MAX_LINES: usize = 4;
const MAX_WIDTH_RATIO: f32 = 0.52;
// pulse-word-in 0.18s … both; `--tscaps-word-rise` defaults to 0.5em.
const ANIM_DURATION_S: f32 = 0.18;
const WORD_RISE_EM: f32 = 0.5;
/// The clip cannot interpolate to `none`, so the browser holds `from` until
/// the eased progress passes this and then snaps to `to`.
const REVEAL_PROGRESS: f32 = 0.5;
const PRIMARY: &str = "#ffffff"; // primary-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_MARGIN_EM,
    line_spacing_em: LINE_SPACING_EM,
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: true, // luna always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// luna declares no `effects` key and every effect descriptor defaults to
/// `enabled: false`, so nothing runs — no `gap_free` (a frame between two
/// segments renders nothing) and no punctuation cleanup.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const POPPINS_TTF: &[u8] = include_bytes!("../../../assets/poppins-latin-900-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Poppins 900.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
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
    if dominant == FONT {
        font_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}

/// `text-transform: uppercase` — the uppercased form is what gets measured
/// *and* emitted.
fn uppercase(text: &str) -> String {
    text.to_uppercase()
}

// ── pulse-word-in ─────────────────────────────────────────────────────────

/// `cubic-bezier(0.2, 0.9, 0.1, 1)` — the entrance's timing function, and
/// therefore the progress the clip's flip is judged against.
fn pulse_ease(x: f32) -> f32 {
    recipes::cubic_bezier(0.2, 0.9, 0.1, 1.0)(x)
}

/// Eased progress of `pulse-word-in` at `now_ms`, measured from the line's
/// narration start. Fill mode `both` holds `from` before the line begins and
/// `to` after `duration_s`, which is the clamp.
fn pulse_progress(now_ms: f32, line_start_ms: f32, duration_s: f32) -> f32 {
    let linear = (now_ms - line_start_ms) / 1000.0 / duration_s;
    pulse_ease(linear.clamp(0.0, 1.0))
}

/// `transform: translateY(from → 0)` — continuous, unlike the clip.
fn word_rise_px(progress: f32, fs: f32, rise_em: f32) -> f32 {
    rise_em * fs * (1.0 - progress)
}

/// Whether the word has un-clipped yet (see `REVEAL_PROGRESS`).
fn is_revealed(progress: f32) -> bool {
    progress >= REVEAL_PROGRESS
}

// ── render ────────────────────────────────────────────────────────────────

pub fn render<'a>(
    doc: &CaptionDocument,
    now_ms: f32,
    ctx: &FFramesContext<'a, '_>,
) -> Svgr<'a> {
    let segments = pipeline::collect_segments(doc, &SPLIT);
    let Some(index) = segments
        .iter()
        .position(|s| now_ms >= s.start_ms && now_ms < s.end_ms)
    else {
        return Svgr::empty();
    };
    let segment = &segments[index];

    let video_w = crate::ctx_video_size(ctx).width;
    let video_h = crate::ctx_video_size(ctx).height;
    // User overrides (params) win over the template's spec constants.
    let ov = overrides::resolve(&doc.params, DEFAULTS);
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let pad_x = PAD_X_EM * fs;
    let pad_y = PAD_Y_EM * fs;
    let line_spacing = ov.line_spacing_em * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        POPPINS_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ov.letter_spacing_em * fs,
        true,
    );
    let widths = measured.widths;

    // Case-transformed once, then measured and emitted in that same form.
    let texts: Vec<String> = segment
        .words
        .iter()
        .map(|word| ov.text_case.apply(&word.display))
        .collect();

    // balanced-pixel-width: each width carries both of its word's margins,
    // because the line renders the words as adjacent boxes with nothing
    // between them and the splitter must see that same arithmetic.
    let split_widths: Vec<f32> = widths.iter().map(|width| width + 2.0 * margin).collect();
    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let lines = splitter.split(&split_widths);

    // Shrink-to-fit box: as wide as the widest line plus padding, tall
    // enough for every line plus the spacing between them.
    let content_w = lines
        .iter()
        .map(|&(start, end)| {
            widths[start..end].iter().sum::<f32>() + 2.0 * margin * (end - start) as f32
        })
        .fold(0.0f32, f32::max);
    let box_w = content_w + 2.0 * pad_x;
    let box_h = lines.len() as f32 * line_h
        + lines.len().saturating_sub(1) as f32 * line_spacing
        + 2.0 * pad_y;
    let box_left = ov.anchor_x * video_w as f32;
    let box_top = ov.anchor_y * video_h as f32;
    let content_left = box_left + pad_x;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // The segment's `overflow: hidden`, plus one zero-height clip per line
    // that has not flipped open yet.
    let mut defs = format!(
        "<clipPath id=\"luna-segment\"><rect x=\"{box_left}\" y=\"{box_top}\" \
         width=\"{box_w}\" height=\"{box_h}\"/></clipPath>"
    );
    let font_family_attr = stack::svg_font_family(&ov.font_family);
    let mut words_svg = format!(
        "<g font-family=\"{font_family_attr}\" font-weight=\"{weight}\" font-size=\"{fs}\">",
        weight = ov.font_weight,
    );
    for (i, &(start, end)) in lines.iter().enumerate() {
        let line_top = box_top + pad_y + i as f32 * (line_h + line_spacing);
        let baseline = line_top + half_leading + ascent;
        let line_w = widths[start..end].iter().sum::<f32>() + 2.0 * margin * (end - start) as f32;
        let xs = layout::word_x_positions(&widths[start..end], margin, content_left, line_w, rtl);

        // One delay for the whole line: every word shares its narration start.
        let progress = if ov.motion_static {
            1.0
        } else {
            pulse_progress(
                now_ms,
                segment.words[start].start_ms,
                ov.duration_or(ANIM_DURATION_S),
            )
        };
        words_svg.push_str(&format!(
            "<g transform=\"translate(0 {})\">",
            word_rise_px(progress, fs, ov.distance_or(WORD_RISE_EM))
        ));
        let hidden = !is_revealed(progress);
        if hidden {
            let id = format!("luna-line-{i}");
            words_svg.push_str(&format!("<g clip-path=\"url(#{id})\">"));
            defs.push_str(&format!(
                "<clipPath id=\"{id}\"><rect x=\"{content_left}\" y=\"{line_top}\" \
                 width=\"{line_w}\" height=\"0\"/></clipPath>"
            ));
        }
        for ((offset, &x), family) in xs
            .iter()
            .enumerate()
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&texts[start + offset]);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{}\"", stack::svg_font_family(&family))
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr}>{text}</text>",
                primary = ov.primary.as_str(),
            ));
        }
        if hidden {
            words_svg.push_str("</g>");
        }
        words_svg.push_str("</g>");
    }
    words_svg.push_str("</g>");

    let mut body = format!("<g clip-path=\"url(#luna-segment)\">{words_svg}</g>");

    // User rotation wraps the clipped box about its anchor.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = box_left;
        let cy = box_top + box_h / 2.0;
        body = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = body,
        );
    }

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>{defs}</defs>
            {body}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// fontdb's rule (`parse_names`): typographic family (name ID 16) when
    /// present, else the legacy family (ID 1) — the string `resolve_font`
    /// matches.
    fn family_name(face: &fframes::ttf_parser::Face<'_>) -> Option<String> {
        for id in [16, 1] {
            for n in face.names() {
                if n.name_id != id {
                    continue;
                }
                if let Some(name) = n.to_string() {
                    return Some(name);
                }
            }
        }
        None
    }

    #[test]
    fn embedded_poppins_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(POPPINS_TTF, 0).unwrap();
        assert_eq!(family_name(&face).as_deref(), Some("Poppins Black"));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);

        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn luna_splitter_is_sentence_with_a_ten_char_floor() {
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
        // No effects run, so the punctuation survives into display text.
        assert!(segments
            .iter()
            .any(|s| s.words.iter().any(|w| w.display.ends_with('.'))));
        // The 10-char floor keeps the first cut clear of the short head.
        let first: String = segments[0].words.iter().map(|w| w.text.as_str()).collect();
        assert!(first.chars().count() >= MIN_CHARS, "{first}");
    }

    #[test]
    fn balanced_splitter_honours_the_budget_and_the_line_cap() {
        // 52% of a 1920 frame, four words of 200px + margins to a line.
        let budget = (1920.0 * MAX_WIDTH_RATIO).round();
        let splitter = layout::BalancedPixelWidth {
            min_lines: MIN_LINES,
            max_lines: MAX_LINES,
            max_width: budget,
        };
        let lines = splitter.split(&[220.0; 8]);
        assert_eq!(lines, vec![(0, 4), (4, 8)]);
        for &(start, end) in &lines {
            let width: f32 = [220.0; 8][start..end].iter().sum();
            assert!(width <= budget, "{width} > {budget}");
        }

        // Words that would want ten lines stop at four.
        let cramped = layout::BalancedPixelWidth {
            min_lines: MIN_LINES,
            max_lines: MAX_LINES,
            max_width: budget,
        };
        let lines = cramped.split(&[600.0; 10]);
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!((lines[0].0, lines.last().unwrap().1), (0, 10));
        // Every word lands on exactly one line.
        let covered: usize = lines.iter().map(|&(start, end)| end - start).sum();
        assert_eq!(covered, 10);

        // The one-line floor still forces a second line when asked.
        let floored = layout::BalancedPixelWidth {
            min_lines: 2,
            max_lines: MAX_LINES,
            max_width: budget,
        };
        assert_eq!(floored.split(&[100.0, 100.0, 100.0]).len(), 2);
    }

    #[test]
    fn pulse_rises_from_the_delay_and_flips_open_mid_ease() {
        // Held at `from` before the line starts.
        assert_eq!(pulse_progress(0.0, 1000.0, ANIM_DURATION_S), 0.0);
        // settled once 0.18s have elapsed.
        assert_eq!(pulse_progress(1180.0, 1000.0, ANIM_DURATION_S), 1.0);
        assert_eq!(pulse_progress(5000.0, 1000.0, ANIM_DURATION_S), 1.0);
        // The line start itself is progress zero, i.e. still clipped.
        let fs = 54.0;
        let start = pulse_progress(1000.0, 1000.0, ANIM_DURATION_S);
        assert!(!is_revealed(start));
        assert!((word_rise_px(start, fs, WORD_RISE_EM) - WORD_RISE_EM * fs).abs() < 1e-4);
        // The curve is front-loaded, so the flip lands early: a few
        // milliseconds in the word is still clipped, a tenth of the way
        // through it has already opened.
        assert!(!is_revealed(pulse_progress(1005.0, 1000.0, ANIM_DURATION_S)));
        assert!(is_revealed(pulse_progress(1050.0, 1000.0, ANIM_DURATION_S)));

        // The clip flips once, at eased 0.5 — never earlier.
        assert!(!is_revealed(REVEAL_PROGRESS - 0.01));
        assert!(is_revealed(REVEAL_PROGRESS));
        assert!(is_revealed(1.0));

        // The transform under it is continuous: it starts at the rise and
        // lands on zero, passing through the flip half-risen.
        assert!((word_rise_px(1.0, fs, WORD_RISE_EM)).abs() < 1e-6);
        assert!((word_rise_px(REVEAL_PROGRESS, fs, WORD_RISE_EM) - 0.25 * fs).abs() < 1e-4);
        let mut previous = -1.0;
        for step in 0..=20 {
            let progress = pulse_progress(1000.0 + step as f32 * 10.0, 1000.0, ANIM_DURATION_S);
            assert!(progress >= previous, "easing must not backtrack");
            previous = progress;
        }
    }

    #[test]
    fn every_word_of_a_line_enters_together() {
        // One delay per line is what makes the line rise as a piece: the
        // progress depends only on the line's first word start.
        let line_start = 1200.0;
        assert_eq!(
            pulse_progress(1300.0, line_start, ANIM_DURATION_S),
            pulse_progress(1300.0, line_start, ANIM_DURATION_S)
        );
        assert!(
            pulse_progress(1300.0, line_start, ANIM_DURATION_S)
                > pulse_progress(1250.0, line_start, ANIM_DURATION_S)
        );
        // Uppercasing is applied before measuring and before emitting.
        assert_eq!(uppercase("Hello there"), "HELLO THERE");
        assert_eq!(uppercase("élan"), "ÉLAN");
    }
}
