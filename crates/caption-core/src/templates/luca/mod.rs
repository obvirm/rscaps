//! Native port of the tscaps `luca` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\luca\{style.scss,template.json}`.
//! Hand-lettered Caveat 600 at 4cqh inside a padded, shrink-to-fit segment
//! box anchored top-centre (0.55H / 0.5W). Sentence segments of 35 chars
//! (min 0) break into at most two lines with `fixed-tail`: the head line
//! enters as a whole from the left, the tail's words enter from the right,
//! and whenever the tail really carries three words its first word swells
//! into an Anton anchor spanning two grid rows beside the other two.
//!
//! Not ported (per the porting brief):
//! - `.tscaps-video-frame-layer` — the video frame layer itself.
//! - `mix-blend-mode: difference` on the anchor word paints flat white
//!   (`blend-color`, default `#ffffff`) — tscaps' own
//!   `rendering.videoFrame.previewMode: "omit"` fallback.
//! - The `@container style(--tscaps-text-direction: rtl)` rules — only
//!   `ada` implements bidirectional layout in this crate.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack};
use crate::CaptionDocument;

// ── spec constants (from luca's style.scss + template.json) ───────────────
const FONT: &str = "Caveat"; // fontStack caveat (scss: 'Caveat Variable')
const FONT_WEIGHT: u16 = 600; // typography.fontWeight: 600
const ANTON_FONT: &str = "Anton"; // --tscaps-big-word-font-family default
const ANTON_FONT_WEIGHT: u16 = 400; // font-weight: 400 on the anchor word
const FONT_SIZE_CQH: f32 = 0.04; // font-size: 4cqh
const LETTER_SPACING_EM: f32 = 0.0; // typography.letterSpacing: 0
const LINE_HEIGHT: f32 = 1.1; // $line-height: 1.1
const WORD_GAP_EM: f32 = 0.25; // .line column-gap: var(--tscaps-word-spacing)
const LINE_SPACING_EM: f32 = 0.05; // .line + .line margin-top: 0.05em
const PAD_X_EM: f32 = 0.35; // padding: 0.1em 0.35em
const PAD_Y_EM: f32 = 0.1;
const ANCHOR_Y_FRAC: f32 = 0.55; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 35; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // boundary_score_limit_by_chars.minChars
// text-shadow: 0 0.04em 0.22em rgba(0, 0, 0, 0.7)
const SHADOW_DY_EM: f32 = 0.04;
const SHADOW_BLUR_EM: f32 = 0.22;
const SHADOW_OPACITY: f32 = 0.7;
const SHADOW_COLOR: &str = "#000000";
// slide-in($duration: var(--luca-anim-duration)) — 0.25s, 0.4s at a
// breathing point; `--entrance-slide` is -1.5em on line 1, +1.5em on the
// words of later lines.
const ANIM_DURATION_S: f32 = 0.25;
const ANIM_DURATION_BREATH_S: f32 = 0.4;
const SLIDE_EM: f32 = 1.5;
/// `PauseTagger` `minGapSeconds` (the studio wires it with `1`).
const PAUSE_GAP_MS: f32 = 1000.0;
// fixed-tail: tailWordCount 3, minWordsToSplit default 3.
const TAIL_WORD_COUNT: usize = 3;
const MIN_WORDS_TO_SPLIT: usize = 3;
/// Line 2 must carry exactly this many words for the anchor grid.
const ANCHOR_WORD_COUNT: usize = 3;
// Anchor word font-size: clamp(1.8em, 15em / char-count, 2.6em).
const ANCHOR_MIN_EM: f32 = 1.8;
const ANCHOR_MID_EM: f32 = 15.0;
const ANCHOR_MAX_EM: f32 = 2.6;
const ANCHOR_LINE_HEIGHT: f32 = 0.95; // line-height: 0.95
const ANCHOR_LETTER_SPACING_EM: f32 = -0.01; // letter-spacing: -0.01em
const PRIMARY: &str = "#ffffff"; // primary-color default
const BLEND: &str = "#ffffff"; // blend-color default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_GAP_EM,
    line_spacing_em: LINE_SPACING_EM,
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: true, // the anchor word uppercases; the rest pass through
    min_lines: 1, // luca splits with fixed-tail, never balances
    max_lines: 1,
    max_width_ratio: 0.88, // unused — no splitter
};

/// luca's `effects` array: `gap_free`, `remove_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::RemovePunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const CAVEAT_TTF: &[u8] = include_bytes!("../../../assets/caveat-latin-600-normal.ttf");
const ANTON_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Caveat 600.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(CAVEAT_TTF, 0).expect("Caveat parses");
        let upem = face.units_per_em() as f32;
        (
            face.ascender() as f32 / upem,
            face.descender().abs() as f32 / upem,
            upem,
        )
    })
}

/// The same for the anchor's Anton 400.
fn anton_metrics() -> (f32, f32, f32) {
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

/// Baseline metrics for whichever family dominates the segment (main
/// register: Caveat; anchor: Anton — both go through the stack for
/// script fallback).
fn line_metrics(latin: &str, dominant: &str) -> (f32, f32, f32) {
    if dominant == latin {
        font_metrics()
    } else if dominant == ANTON_FONT {
        anton_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}

// ── splitting helpers ─────────────────────────────────────────────────────

/// `lineSplitter: { type: "fixed-tail", tailWordCount: 3 }` — at most two
/// lines: the head takes every word but the last three. Segments shorter
/// than `minWordsToSplit` (3) stay on one line; a segment of exactly three
/// words falls back to a one-word head so neither line ends up empty.
///
/// Returns line ranges `(start, end_exclusive)` over the segment's words.
fn fixed_tail_lines(word_count: usize) -> Vec<(usize, usize)> {
    if word_count == 0 {
        return Vec::new();
    }
    if word_count < MIN_WORDS_TO_SPLIT {
        return vec![(0, word_count)];
    }
    let head = if word_count > TAIL_WORD_COUNT {
        word_count - TAIL_WORD_COUNT
    } else {
        1
    };
    vec![(0, head), (head, word_count)]
}

/// `is_anchor_line(lines)` — the anchor grid only mounts on line 2 and only
/// when that line carries exactly three words.
fn is_anchor_line(lines: &[(usize, usize)]) -> bool {
    match lines {
        [_, (start, end)] => end - start == ANCHOR_WORD_COUNT && start < end,
        _ => false,
    }
}

/// `.first-segment-in-document` / `.segment-after-pause`: the document's
/// first segment, or one whose first word starts at least a second after
/// the previous segment's last word. (tscaps measures the words themselves,
/// never the `gap_free`-padded windows; gaps never cross a section and
/// this crate renders one section per document.)
fn is_breathing(segments: &[pipeline::WindowedSegment], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let (Some(previous), Some(current)) = (segments.get(index - 1), segments.get(index)) else {
        return false;
    };
    let (Some(previous_end), Some(start)) = (
        previous.words.last().map(|w| w.end_ms),
        current.words.first().map(|w| w.start_ms),
    ) else {
        return false;
    };
    start - previous_end >= PAUSE_GAP_MS
}

/// The anchor word's `font-size: clamp(1.8em, 15em / char-count, 2.6em)`,
/// with `char-count` counting code points of the display text.
fn anchor_font_size(fs: f32, char_count: usize) -> f32 {
    let count = char_count.max(1) as f32;
    (ANCHOR_MID_EM * fs / count).clamp(ANCHOR_MIN_EM * fs, ANCHOR_MAX_EM * fs)
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
    let gap = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let pad_x = PAD_X_EM * fs;
    let pad_y = PAD_Y_EM * fs;
    let line_spacing = ov.line_spacing_em * fs;

    let main_stack = stack::Stack::for_latin(FONT);
    let anton_stack = stack::Stack::for_latin(ANTON_FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(FONT, stack::dominant_family(&segment.words, &main_stack));
    // Media exception: word_x positions are cumulative from the padded
    // left edge; RTL v1 deviation — the flex-row placement stays LTR.

    // Every word measured once in Caveat at the segment size.
    let measured = stack::measure_words(
        &segment.words,
        &main_stack,
        CAVEAT_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ov.letter_spacing_em * fs,
        false,
    );
    let widths = measured.widths;

    let lines = fixed_tail_lines(segment.words.len());
    let anchor = is_anchor_line(&lines);

    // The anchor word: its own font, size, case and tracking.
    let anchor_word = if anchor {
        let word = &segment.words[lines[1].0];
        let text = ov.text_case.apply(&word.display);
        // `--word-char-count` counts the display text's code points, but
        // `text-transform: uppercase` runs after it and can change the
        // length, so count the pre-transform text.
        let fs_big = anchor_font_size(fs, word.display.chars().count());
        let ls_px = ANCHOR_LETTER_SPACING_EM * fs_big;
        let anton_measured = stack::measure_words(
            std::slice::from_ref(word),
            &anton_stack,
            ANTON_TTF,
            ANTON_FONT_WEIGHT,
            ctx.font_source,
            fs_big,
            ls_px,
            true,
        );
        let width = anton_measured.widths[0];
        Some((text, fs_big, ls_px, width))
    } else {
        None
    };

    // Per-line content sizes: `box_w` is the widest line plus padding, the
    // height is every line plus the spacing between them.
    let mut line_widths = Vec::with_capacity(lines.len());
    let mut line_heights = Vec::with_capacity(lines.len());
    for (i, &(start, end)) in lines.iter().enumerate() {
        let n = end - start;
        if i == 1 && anchor {
            let (_, fs_big, _, big_w) = anchor_word.as_ref().unwrap();
            // `grid-template-rows: 1fr 1fr` splits the anchor's own height
            // across both tracks, so each track is at least its small word
            // and at least half the anchor.
            let row = line_h.max(ANCHOR_LINE_HEIGHT * fs_big / 2.0);
            let smalls: f32 = widths[start + 1..end].iter().cloned().fold(0.0, f32::max);
            line_widths.push(big_w + gap + smalls);
            line_heights.push(2.0 * row);
        } else {
            line_widths.push(widths[start..end].iter().sum::<f32>() + gap * n.saturating_sub(1) as f32);
            line_heights.push(line_h);
        }
    }

    // The box is shrink-to-fit: as wide as its widest line plus padding,
    // tall enough for every line plus the spacing between them. Only its
    // top edge matters here — the anchor is `verticalAlign: top`.
    let content_w = line_widths.iter().cloned().fold(0.0, f32::max);
    let box_w = content_w + 2.0 * pad_x;
    let box_top = ov.anchor_y * video_h as f32;
    let box_left = ov.anchor_x * video_w as f32 - box_w / 2.0;
    let content_left = box_left + pad_x;
    let mut cursor_top = box_top + pad_y;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;
    let (anton_asc, anton_desc, _) = anton_metrics();

    // Entrance: `--on-segment-starts`, 0.25s (0.4s at a breathing point).
    let elapsed_s = (now_ms - segment.start_ms) / 1000.0;
    let breathing = is_breathing(&segments, index);
    let duration = ov.duration_or(if breathing {
        ANIM_DURATION_BREATH_S
    } else {
        ANIM_DURATION_S
    });
    let slide_em = ov.distance_or(SLIDE_EM);
    // Line 1 enters as a whole from the left; the words of line 2 enter
    // from the right; the anchor word only fades.
    let first_line = recipes::SlideIn {
        distance_em: if breathing { -slide_em } else { 0.0 },
        from_opacity: 0.0,
        duration_s: duration,
        ease: recipes::ease_entrance,
    };
    let word_slide = recipes::SlideIn {
        distance_em: if breathing { slide_em } else { 0.0 },
        from_opacity: 0.0,
        duration_s: duration,
        ease: recipes::ease_entrance,
    };
    let anchor_fade = recipes::FadeIn::with(0.0, duration);

    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;

    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (i, &(start, end)) in lines.iter().enumerate() {
        let line_top = cursor_top;
        if i > 0 {
            cursor_top += line_heights[i - 1] + line_spacing;
        }

        if i == 0 {
            // `.line:first-child` carries the entrance; its words are
            // `animation: none`.
            let (opacity, dx_em) = if ov.motion_static {
                (1.0, 0.0)
            } else {
                first_line.eval(elapsed_s)
            };
            words_svg.push_str(&format!(
                "<g opacity=\"{opacity}\" transform=\"translate({})\">",
                dx_em * fs
            ));
            let baseline = line_top + half_leading + ascent;
            let mut x = content_left;
            for ((word, &width), family) in segment.words[start..end]
                .iter()
                .zip(&widths[start..end])
                .zip(&measured.families[start..end])
            {
                let text = layout::escape_xml(&word.display);
                let family_attr = if *family == FONT {
                    String::new()
                } else {
                    format!(" font-family=\"{family}\"")
                };
                words_svg.push_str(&format!(
                    "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr} \
                     filter=\"url(#luca-shadow)\">{text}</text>",
                    primary = ov.primary.as_str(),
                ));
                x += width + gap;
            }
            words_svg.push_str("</g>");
            continue;
        }

        // Line 2: the anchor grid when the tail holds three words, otherwise
        // a plain flex line; every word carries its own entrance.
        match &anchor_word {
            Some((text, fs_big, ls_px, big_w)) => {
                let row = line_h.max(ANCHOR_LINE_HEIGHT * fs_big / 2.0);
                let grid_h = 2.0 * row;
                let big_h = ANCHOR_LINE_HEIGHT * fs_big;
                let big_top = line_top + (grid_h - big_h) / 2.0;
                let big_hl = (big_h - (anton_asc + anton_desc) * fs_big) / 2.0;
                let big_baseline = big_top + big_hl + anton_asc * fs_big;
                let opacity = if ov.motion_static {
                    1.0
                } else {
                    anchor_fade.eval(elapsed_s)
                };
                let escaped = layout::escape_xml(text);
                words_svg.push_str(&format!(
                    "<text x=\"{content_left}\" y=\"{big_baseline}\" fill=\"{BLEND}\" \
                     font-family=\"{ANTON_FONT}\" font-weight=\"{ANTON_FONT_WEIGHT}\" \
                     font-size=\"{fs_big}\" letter-spacing=\"{ls_px}\" \
                     opacity=\"{opacity}\">{escaped}</text>"
                ));
                let col2_x = content_left + big_w + gap;
                for ((offset, word_index), family) in (start + 1..end)
                    .enumerate()
                    .zip(&measured.families[start + 1..end])
                {
                    let row_top = line_top + offset as f32 * row;
                    let small_top = row_top + (row - line_h) / 2.0;
                    let baseline = small_top + half_leading + ascent;
                    let (opacity, dx_em) = if ov.motion_static {
                        (1.0, 0.0)
                    } else {
                        word_slide.eval(elapsed_s)
                    };
                    let text = layout::escape_xml(&segment.words[word_index].display);
                    let family_attr = if *family == FONT {
                        String::new()
                    } else {
                        format!(" font-family=\"{family}\"")
                    };
                    words_svg.push_str(&format!(
                        "<text x=\"{col2_x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr} \
                         opacity=\"{opacity}\" transform=\"translate({})\" \
                         filter=\"url(#luca-shadow)\">{text}</text>",
                        dx_em * fs,
                        primary = ov.primary.as_str(),
                    ));
                }
            }
            None => {
                let baseline = line_top + half_leading + ascent;
                let mut x = content_left;
                for ((word, &width), family) in segment.words[start..end]
                    .iter()
                    .zip(&widths[start..end])
                    .zip(&measured.families[start..end])
                {
                    let (opacity, dx_em) = if ov.motion_static {
                        (1.0, 0.0)
                    } else {
                        word_slide.eval(elapsed_s)
                    };
                    let text = layout::escape_xml(&word.display);
                    let family_attr = if *family == FONT {
                        String::new()
                    } else {
                        format!(" font-family=\"{family}\"")
                    };
                    words_svg.push_str(&format!(
                        "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr} \
                         opacity=\"{opacity}\" transform=\"translate({})\" \
                         filter=\"url(#luca-shadow)\">{text}</text>",
                        dx_em * fs,
                        primary = ov.primary.as_str(),
                    ));
                    x += width + gap;
                }
            }
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the shrink-to-fit box about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let block_h = 2.0 * pad_y
            + line_heights.iter().sum::<f32>()
            + line_spacing * lines.len().saturating_sub(1) as f32;
        let cx = ov.anchor_x * video_w as f32;
        let cy = box_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="luca-shadow" x="-30%" y="-60%" width="160%" height="260%">
                    <feDropShadow dx={0.0} dy={shadow_dy} stdDeviation={shadow_std} flood-color={SHADOW_COLOR} flood-opacity={SHADOW_OPACITY}/>
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
    fn embedded_fonts_have_expected_identity() {
        let caveat = fframes::ttf_parser::Face::parse(CAVEAT_TTF, 0).unwrap();
        assert_eq!(family_name(&caveat).as_deref(), Some(FONT));
        assert_eq!(caveat.weight().to_number(), FONT_WEIGHT);

        let anton = fframes::ttf_parser::Face::parse(ANTON_TTF, 0).unwrap();
        assert_eq!(family_name(&anton).as_deref(), Some(ANTON_FONT));
        assert_eq!(anton.weight().to_number(), ANTON_FONT_WEIGHT);

        for (asc, desc, upem) in [font_metrics(), anton_metrics()] {
            assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
            assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
            assert!(upem >= 16.0, "upem {upem}");
        }
    }

    #[test]
    fn fixed_tail_reserves_three_words_for_the_tail() {
        assert_eq!(fixed_tail_lines(0), Vec::<(usize, usize)>::new());
        // Shorter segments stay on one line.
        assert_eq!(fixed_tail_lines(2), vec![(0, 2)]);
        // Exactly three words: one-word head so neither line is empty.
        assert_eq!(fixed_tail_lines(3), vec![(0, 1), (1, 3)]);
        // Four or more: the tail is exactly three words wide.
        assert_eq!(fixed_tail_lines(4), vec![(0, 1), (1, 4)]);
        assert_eq!(fixed_tail_lines(7), vec![(0, 4), (4, 7)]);
    }

    #[test]
    fn anchor_grid_mounts_only_on_a_three_word_tail() {
        for word_count in 4..=9 {
            let lines = fixed_tail_lines(word_count);
            assert!(is_anchor_line(&lines), "word_count={word_count}");
        }
        // A three-word tail (a three-word segment) degrades to flex.
        let lines = fixed_tail_lines(3);
        assert!(!is_anchor_line(&lines));
        // So does the single-line case.
        assert!(!is_anchor_line(&fixed_tail_lines(2)));
    }

    #[test]
    fn anchor_font_size_clamps_both_ends() {
        let fs = 100.0;
        // 15em / 1 char is far above the 2.6em cap.
        assert!((anchor_font_size(fs, 1) - ANCHOR_MAX_EM * fs).abs() < 1e-4);
        assert!((anchor_font_size(fs, 5) - ANCHOR_MAX_EM * fs).abs() < 1e-4);
        // Six characters: 15 / 6 = 2.5em, inside the window.
        assert!((anchor_font_size(fs, 6) - 250.0).abs() < 1e-4);
        // Long words sit on the 1.8em floor.
        assert!((anchor_font_size(fs, 9) - ANCHOR_MIN_EM * fs).abs() < 1e-4);
        assert!((anchor_font_size(fs, 64) - ANCHOR_MIN_EM * fs).abs() < 1e-4);
        // A zero-length word still divides by one.
        assert!((anchor_font_size(fs, 0) - ANCHOR_MAX_EM * fs).abs() < 1e-4);
    }

    #[test]
    fn luca_splitter_is_sentence_plus_thirty_five_chars() {
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
        // remove_punctuation drops the trailing period from the display.
        assert!(segments
            .iter()
            .all(|s| s.words.iter().all(|w| !w.display.ends_with('.'))));
    }

    #[test]
    fn breathing_is_the_first_segment_or_one_after_a_pause() {
        let doc = CaptionDocument {
            duration_frames: 6000,
            cues: vec![
                crate::Cue {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "One two.".into(),
                },
                crate::Cue {
                    start_ms: 1200,
                    end_ms: 2000,
                    text: "Three four.".into(),
                },
                crate::Cue {
                    start_ms: 4000,
                    end_ms: 5000,
                    text: "Five six.".into(),
                },
            ],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert_eq!(segments.len(), 3);
        assert!(is_breathing(&segments, 0), "first segment in document");
        // 200ms of silence is not a pause…
        assert!(!is_breathing(&segments, 1));
        // …two seconds is.
        assert!(is_breathing(&segments, 2));
    }
}
