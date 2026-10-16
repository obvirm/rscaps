//! Native port of the tscaps `milo` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\milo\{style.scss,template.json}`.
//! Lobster at 3.5cqh inside a padded, shrink-to-fit segment box anchored
//! top-centre (0.60H / 0.5W); every segment is a single balanced line
//! (`maxLines: 1`, 67% of the frame). Two layouts share that box. Plain
//! segments centre one flex row with a 0.2em gap. When `--milo-geometry`
//! resolves to 1 — at least two words *and* an anchor of three or more
//! characters — the line's width snaps to `min-content` (the anchor's own
//! nowrap width): the small words spread across it with `space-between`,
//! each shrunk to a size the anchor's length budgets for them, and the
//! anchor drops onto a second row in Anton 400, uppercase, no shadow,
//! clamped to `clamp(2em, 27em / char-count, 4.5em)`.
//!
//! Entrances fire at `--on-segment-starts`: ordinary segments rise 0.1em
//! from opacity 0.5 over 0.25s on `cubic-bezier(0.22, 0.6, 0.3, 1)`.
//! Breathing segments — the document's first segment and any segment
//! preceded by a pause — replace that whole `animation` with `slide-in`:
//! the small words arrive from the left, the anchor answers from the
//! right, 1.5em over the recipe's 0.4s.
//!
//! Not ported (per the porting brief):
//! - `.tscaps-video-frame-layer` — the video frame layer itself.
//! - `mix-blend-mode: difference` on the anchor word paints flat white
//!   (`blend-color`, default `#ffffff`) — tscaps' own
//!   `rendering.videoFrame.previewMode: "omit"` fallback; with no video
//!   behind the caption there is nothing for the difference to invert
//!   against, so flat white is the whole of the effect.
//! - The `@container style(--tscaps-text-direction: rtl)` rules — only
//!   `ada` implements bidirectional layout in this crate.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from milo's style.scss + template.json) ───────────────
const FONT: &str = "Lobster"; // fontStack lobster (scss: 'Lobster')
// typography.fontWeight: 600, but the only Lobster in assets is 400
// (`lobster-latin-400-normal.ttf`) — the query, the SVG and the embedded
// face all speak 400 so the weight matches the glyphs actually measured.
const FONT_WEIGHT: u16 = 400;
const ANTON_FONT: &str = "Anton"; // --tscaps-big-word-font-family default
const ANTON_FONT_WEIGHT: u16 = 400; // font-weight: 400 on the anchor
const FONT_SIZE_CQH: f32 = 0.035; // font-size: 3.5cqh
const LETTER_SPACING_EM: f32 = 0.0; // typography.letterSpacing: 0
const LINE_HEIGHT: f32 = 1.1; // $line-height: 1.1
const WORD_GAP_EM: f32 = 0.2; // .line column-gap: var(--tscaps-word-spacing, 0.2em)
const LINE_SPACING_EM: f32 = 0.05; // .line + .line margin-top: 0.05em
const PAD_X_EM: f32 = 0.35; // padding: 0.1em 0.35em
const PAD_Y_EM: f32 = 0.1;
const ANCHOR_Y_FRAC: f32 = 0.6; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 35; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.67; // lineSplitter.maxWidthRatio
// text-shadow: 0 0.04em 0.22em rgba(0, 0, 0, 0.7) — blur → σ = blur / 2.
const SHADOW_DY_EM: f32 = 0.04;
const SHADOW_BLUR_EM: f32 = 0.22;
const SHADOW_OPACITY: f32 = 0.7;
const SHADOW_COLOR: &str = "#000000";
// rise-in($distance: 0.1em, $from-opacity: 0.5, $duration: 0.25s,
//         $easing: cubic-bezier(0.22, 0.6, 0.3, 1))
const RISE_EM: f32 = 0.1;
const RISE_FROM_OPACITY: f32 = 0.5;
const RISE_DURATION_S: f32 = 0.25;
// slide-in(...) at a breathing point: `--entrance-slide` ±1.5em, recipe
// defaults for the rest (0.4s, entrance easing, from-opacity 0).
const SLIDE_EM: f32 = 1.5;
/// `PauseTagger` `minGapSeconds` (the studio wires it with `1`).
const PAUSE_GAP_MS: f32 = 1000.0;
// .word.last-word-in-line font-size: clamp(2em, 27em / char-count, 4.5em)
const BIG_MIN_EM: f32 = 2.0;
const BIG_MID_EM: f32 = 27.0;
const BIG_MAX_EM: f32 = 4.5;
const BIG_LINE_HEIGHT: f32 = 0.95; // line-height: 0.95
const BIG_LETTER_SPACING_EM: f32 = -0.01; // letter-spacing: -0.01em
// .word:not(.last-word-in-line) font-size:
//   clamp(0.3em, clamp(2em×L, 27em, 4.5em×L) / max(S − L, 1), 1em)
const SMALL_MIN_EM: f32 = 0.3;
const SMALL_MAX_EM: f32 = 1.0;
const SMALL_NUM_MIN_EM: f32 = 2.0; // numerator floor: 2em × L
const SMALL_NUM_CAP_EM: f32 = 27.0; // numerator cap: 27em
const SMALL_NUM_MAX_EM: f32 = 4.5; // numerator growth: 4.5em × L
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
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// milo's `effects` array: `gap_free`, `remove_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause, // segmentSplitters[0].mode: "clause"
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree, effects::Effect::RemovePunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const LOBSTER_TTF: &[u8] = include_bytes!("../../../assets/lobster-latin-400-normal.ttf");
const ANTON_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Lobster 400.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(LOBSTER_TTF, 0).expect("Lobster parses");
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
/// register: Lobster; anchor: Anton — both go through the stack for
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

// ── the container query ───────────────────────────────────────────────────

/// `--milo-geometry` on `.segment`:
/// `min(min(1, max(0, word-count − 1)), min(1, max(0,
/// last-word-char-count − 2)))`, evaluated over the engine's integer
/// variables. 1 mounts the big-word layout: a small word has to exist
/// *and* the closing word has to carry at least three characters.
fn milo_geometry(word_count: usize, last_word_chars: usize) -> i32 {
    let by_count = (word_count as i32 - 1).clamp(0, 1);
    let by_anchor = (last_word_chars as i32 - 2).clamp(0, 1);
    by_count.min(by_anchor)
}

/// `Segment.getText().length` — the segment's raw `text` joined with
/// single spaces, in UTF-16 units (the JS string length the engine
/// reports; `remove_punctuation` only rewrites `display`).
fn segment_char_count(segment: &pipeline::WindowedSegment) -> usize {
    let words = segment.words.len();
    segment
        .words
        .iter()
        .map(|w| w.text.encode_utf16().count())
        .sum::<usize>()
        + words.saturating_sub(1)
}

/// `.word:last-child` — `clamp(2em, 27em / word-char-count, 4.5em)`, in px.
fn big_font_size(fs: f32, char_count: usize) -> f32 {
    let count = char_count.max(1) as f32;
    (BIG_MID_EM * fs / count).clamp(BIG_MIN_EM * fs, BIG_MAX_EM * fs)
}

/// `.word:not(.last-word-in-line)` —
/// `clamp(0.3em, clamp(2em×L, 27em, 4.5em×L) / max(S − L, 1), 1em)` in px,
/// where `L` is the anchor's code-point count and `S` the segment's
/// UTF-16 length. One size for every small word of the segment.
fn small_font_size(fs: f32, segment_chars: usize, last_word_chars: usize) -> f32 {
    let rest = segment_chars.saturating_sub(last_word_chars).max(1) as f32;
    let anchor = last_word_chars as f32;
    let numerator = (SMALL_NUM_MIN_EM * anchor).max(SMALL_NUM_CAP_EM.min(SMALL_NUM_MAX_EM * anchor));
    (numerator / rest).clamp(SMALL_MIN_EM, SMALL_MAX_EM) * fs
}

/// Baseline inside a line box of `line_height_em × fs`: half the leading
/// over the font's (ascent + descent), then the ascent.
fn baseline_y(fs: f32, line_height_em: f32, asc_ratio: f32, desc_ratio: f32) -> f32 {
    (line_height_em - asc_ratio - desc_ratio) * fs / 2.0 + asc_ratio * fs
}

/// `rise-in($easing: cubic-bezier(0.22, 0.6, 0.3, 1))`.
fn rise_ease(x: f32) -> f32 {
    recipes::cubic_bezier(0.22, 0.6, 0.3, 1.0)(x)
}

/// A breathing word's travel: `+1.5em` for the line's last word (the
/// `.word.last-word-in-line` slide-in enters from the right), `-1.5em` for
/// every other. Those selectors sit outside the geometry query, so a plain
/// segment's final word takes the anchor's direction too.
fn breathing_slide_em(last_in_line: bool) -> f32 {
    if last_in_line {
        SLIDE_EM
    } else {
        -SLIDE_EM
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

/// Control value (tscaps fontStack id, e.g. "anton") → the display name
/// the SVG emits. Known ids map to the embedded face names; unknown ids
/// pass through (svg_font_family) so the browser can resolve a
/// user-installed @font-face — native measurement stays on Anton.
fn big_word_family(id: &str) -> String {
    match id {
        "anton" => ANTON_FONT.to_string(),
        "playfair-display" => "Playfair Display".to_string(),
        other => stack::svg_font_family(other),
    }
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
    // Style controls data-driven (panel Style → template.json defaults).
    let st = style::Style::of(&doc.params, "milo");
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let gap = ov.word_spacing_em * fs;
    let margin = gap / 2.0; // the column-gap rides each word as a half-margin
    let line_h = LINE_HEIGHT * fs;
    let pad_x = PAD_X_EM * fs;
    let pad_y = PAD_Y_EM * fs;

    let main_stack = stack::Stack::for_latin(FONT);
    let anton_stack = stack::Stack::for_latin(ANTON_FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(FONT, stack::dominant_family(&segment.words, &main_stack));
    // Media exception: word_x positions are cumulative from the padded
    // left edge; RTL v1 deviation — the flex-row placement stays LTR.

    // Measured once at the segment size: what the plain flex row paints
    // and what the balanced splitter sees.
    let measured = stack::measure_words(
        &segment.words,
        &main_stack,
        LOBSTER_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ov.letter_spacing_em * fs,
        false,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);
    if lines.is_empty() {
        return Svgr::empty();
    }

    // The container query reads segment variables. `maxLines: 1` gives the
    // segment exactly one line, so the line's word list *is* the segment's
    // and the line/segment counts the scss keeps apart agree here.
    let word_count = segment.words.len();
    let last_word_chars = segment
        .words
        .last()
        .map(|w| w.display.chars().count())
        .unwrap_or(0);
    let segment_chars = segment_char_count(segment);
    let geometry = milo_geometry(word_count, last_word_chars) == 1;
    // Every non-anchor word renders at this one size.
    let plain_fs = if geometry {
        small_font_size(fs, segment_chars, last_word_chars)
    } else {
        fs
    };

    // `.last-word-in-line` is a structure tag on every line's final word
    // (`StructureTagger`), independent of the geometry query: the
    // breathing entrances key off the class, so a plain segment's last
    // word still answers from the right.
    let mut last_in_line = vec![false; segment.words.len()];
    for &(start, end) in &lines {
        if end > start {
            last_in_line[end - 1] = true;
        }
    }

    // Placement: each word's x from the content box's left edge, its
    // baseline from the content box's top edge, and the size it paints at.
    let mut xs = vec![0.0f32; segment.words.len()];
    let mut baselines = vec![0.0f32; segment.words.len()];
    let mut word_fs = vec![fs; segment.words.len()];
    let mut is_big = vec![false; segment.words.len()];
    let mut content_w = 0.0f32;
    let mut cursor_top = 0.0f32;

    for (line_index, &(start, end)) in lines.iter().enumerate() {
        if line_index > 0 {
            cursor_top += ov.line_spacing_em * fs;
        }
        let line_top = cursor_top;
        let (line_width, line_height) = if geometry {
            // `.line { width: min-content; justify-content: space-between;
            // column-gap: 0 }` — the width is the largest item's own
            // nowrap width, the small words share row 1 out to both
            // edges, the anchor owns row 2 (`order: 1`, `flex-basis: 100%`).
            let anchor_word = &segment.words[end - 1];
            let big_fs = big_font_size(fs, last_word_chars);
            let big_ls = BIG_LETTER_SPACING_EM * big_fs;
            let anton_measured = stack::measure_words(
                std::slice::from_ref(anchor_word),
                &anton_stack,
                ANTON_TTF,
                ANTON_FONT_WEIGHT,
                ctx.font_source,
                big_fs,
                big_ls,
                true,
            );
            let big_w = anton_measured.widths[0];
            let (anton_asc, anton_desc, _) = anton_metrics();
            let small_measured = stack::measure_words(
                &segment.words[start..end - 1],
                &main_stack,
                LOBSTER_TTF,
                ov.font_weight,
                ctx.font_source,
                plain_fs,
                ov.letter_spacing_em * plain_fs,
                false,
            );
            let small_widths = &small_measured.widths;
            let widest_small = small_widths.iter().cloned().fold(0.0, f32::max);
            let width = big_w.max(widest_small);

            // Row 1: `space-between` with no gap to spare — equal runs
            // between the small words, a lone one pinned at the start.
            let spread = if small_widths.len() >= 2 {
                ((width - small_widths.iter().sum::<f32>()) / (small_widths.len() - 1) as f32)
                    .max(0.0)
            } else {
                0.0
            };
            let row1_h = if small_widths.is_empty() {
                0.0
            } else {
                LINE_HEIGHT * plain_fs
            };
            let small_baseline = line_top + baseline_y(plain_fs, LINE_HEIGHT, asc_ratio, desc_ratio);
            let mut x = 0.0;
            for (offset, &small_w) in small_widths.iter().enumerate() {
                xs[start + offset] = x;
                baselines[start + offset] = small_baseline;
                word_fs[start + offset] = plain_fs;
                x += small_w + spread;
            }

            // Row 2: the anchor, alone, at the start of the line.
            xs[end - 1] = 0.0;
            baselines[end - 1] = line_top
                + row1_h
                + baseline_y(big_fs, BIG_LINE_HEIGHT, anton_asc, anton_desc);
            word_fs[end - 1] = big_fs;
            is_big[end - 1] = true;

            (width, row1_h + BIG_LINE_HEIGHT * big_fs)
        } else {
            // `.line { justify-content: center; column-gap: 0.2em }` over
            // a shrink-to-fit box: the row *is* the content box.
            let width = widths[start..end].iter().sum::<f32>()
                + gap * (end - start).saturating_sub(1) as f32;
            let baseline = line_top + baseline_y(fs, LINE_HEIGHT, asc_ratio, desc_ratio);
            let mut x = 0.0;
            for (offset, &word_w) in widths[start..end].iter().enumerate() {
                xs[start + offset] = x;
                baselines[start + offset] = baseline;
                word_fs[start + offset] = fs;
                x += word_w + gap;
            }
            (width, line_h)
        };
        content_w = content_w.max(line_width);
        cursor_top += line_height;
    }

    // Shrink-to-fit box: as wide as its widest line plus padding. Its
    // height never surfaces — the segment clips nothing (`overflow` stays
    // visible) and paints no chrome, so the content simply flows down
    // from the padded top edge.
    let box_w = content_w + 2.0 * pad_x;
    let box_left = ov.anchor_x * video_w as f32 - box_w / 2.0;
    let box_top = ov.anchor_y * video_h as f32;
    let content_left = box_left + pad_x;
    let content_top = box_top + pad_y;

    // Entrance: `--on-segment-starts`. The breathing selectors carry a
    // whole `animation` shorthand, so they *replace* the rise rather than
    // adding to it.
    let elapsed_s = (now_ms - segment.start_ms) / 1000.0;
    let breathing = is_breathing(&segments, index);
    let rise = recipes::RiseIn::custom(
        ov.distance_or(RISE_EM),
        RISE_FROM_OPACITY,
        ov.duration_or(RISE_DURATION_S),
        rise_ease,
    );

    // `text-shadow` on the anchor is `none`; every other word carries it,
    // and `em` in a shadow resolves against that word's own font size.
    let shadow_dy = SHADOW_DY_EM * plain_fs;
    let shadow_std = SHADOW_BLUR_EM * plain_fs / 2.0;
    let blend = st.color("blend-color", BLEND);
    let big_font = big_word_family(&st.str("big-word-font-family", "anton"));

    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (i, word) in segment.words.iter().enumerate() {
        let anchor = is_big[i];
        let wfs = word_fs[i];
        // `em` in a transform resolves against the element's own size too.
        let (opacity, tx, ty) = if ov.motion_static {
            (1.0, 0.0, 0.0)
        } else if breathing {
            let mut slide = recipes::SlideIn::with(breathing_slide_em(last_in_line[i]), 0.0);
            slide.duration_s = ov.duration_or(slide.duration_s);
            slide.distance_em = slide.distance_em.signum() * ov.distance_or(SLIDE_EM);
            let (opacity, dx_em) = slide.eval(elapsed_s);
            (opacity, dx_em * wfs, 0.0)
        } else {
            let (opacity, dy_em) = rise.eval(elapsed_s);
            (opacity, 0.0, dy_em * wfs)
        };
        let shown = if anchor {
            ov.text_case.apply(&word.display)
        } else {
            word.display.clone()
        };
        let text = layout::escape_xml(&shown);
        let x = content_left + xs[i];
        let y = content_top + baselines[i];
        if anchor {
            let ls_px = BIG_LETTER_SPACING_EM * wfs;
            // tscaps: `.big-word { mix-blend-mode: difference }` — anchor
            // membalik warna video di belakangnya (CSS compositing).
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{y}\" fill=\"{blend}\" font-family=\"{big_font}\" \
                 font-weight=\"{ANTON_FONT_WEIGHT}\" font-size=\"{wfs}\" \
                 letter-spacing=\"{ls_px}\" opacity=\"{opacity}\" \
                 style=\"mix-blend-mode: difference\" \
                 transform=\"translate({tx} {ty})\">{text}</text>"
            ));
        } else {
            let family_attr = if measured.families[i] == FONT {
                String::new()
            } else {
                format!(" font-family=\"{}\"", measured.families[i])
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{y}\" fill=\"{primary}\"{family_attr} font-size=\"{wfs}\" \
                 filter=\"url(#milo-shadow)\" opacity=\"{opacity}\" \
                 transform=\"translate({tx} {ty})\">{text}</text>",
                primary = ov.primary.as_str(),
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the shrink-to-fit box about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let block_h = 2.0 * pad_y + cursor_top;
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
                <filter id="milo-shadow" x="-30%" y="-60%" width="160%" height="260%">
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
        let lobster = fframes::ttf_parser::Face::parse(LOBSTER_TTF, 0).unwrap();
        assert_eq!(family_name(&lobster).as_deref(), Some(FONT));
        assert_eq!(lobster.weight().to_number(), FONT_WEIGHT);

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
    fn geometry_gates_on_a_second_word_and_a_three_char_anchor() {
        // Two words alone are not enough if the anchor is a stub.
        assert_eq!(milo_geometry(1, 12), 0, "a lone word has no small one");
        assert_eq!(milo_geometry(2, 2), 0, "anchor too short");
        assert_eq!(milo_geometry(2, 3), 1, "both gates open");
        assert_eq!(milo_geometry(7, 3), 1);
        assert_eq!(milo_geometry(7, 40), 1);
        // The scss variable defaults the query would fall back to.
        assert_eq!(milo_geometry(0, 99), 0, "--word-count: 0");
        assert_eq!(milo_geometry(4, 0), 0, "--last-word-char-count: 0");
    }

    #[test]
    fn breathing_direction_follows_the_line_class_not_the_geometry() {
        // `.word.last-word-in-line` sits outside the container query: even
        // in a plain segment the last word answers from the right (+1.5em)
        // while every other word arrives from the left (-1.5em).
        assert_eq!(breathing_slide_em(true), SLIDE_EM);
        assert_eq!(breathing_slide_em(false), -SLIDE_EM);
    }

    #[test]
    fn anchor_font_size_clamps_both_ends() {
        let fs = 100.0;
        // 27em / 1 char sits far above the 4.5em cap.
        assert!((big_font_size(fs, 1) - BIG_MAX_EM * fs).abs() < 1e-4);
        // Six characters: 27 / 6 = 4.5em, exactly the cap.
        assert!((big_font_size(fs, 6) - BIG_MAX_EM * fs).abs() < 1e-4);
        assert!((big_font_size(fs, 10) - 270.0).abs() < 1e-4);
        // Twenty characters: 27 / 20 = 1.35em, under the 2em floor.
        assert!((big_font_size(fs, 20) - BIG_MIN_EM * fs).abs() < 1e-4);
        // A zero-length word still divides by one.
        assert!((big_font_size(fs, 0) - BIG_MAX_EM * fs).abs() < 1e-4);
    }

    #[test]
    fn small_words_take_only_what_the_anchor_leaves_over() {
        let fs = 100.0;
        // Six-character anchor: numerator 27em. Twenty characters of text,
        // fourteen of them the anchor's → 27 / 14 ≈ 1.93em → capped at 1em.
        assert!((small_font_size(fs, 20, 6) - fs).abs() < 1e-4);
        // A very long segment leaves the anchor little room: 27 / 94 is
        // under the 0.3em floor.
        assert!((small_font_size(fs, 100, 6) - SMALL_MIN_EM * fs).abs() < 1e-4);
        // Mid-window: a 6-char anchor in a 33-char segment → 27 / 27 = 1em.
        assert!((small_font_size(fs, 33, 6) - fs).abs() < 1e-4);
        // Short anchor shrinks the numerator only to 4.5em × 3 = 13.5em:
        // 13.5 / 14 of the segment size.
        assert!((small_font_size(fs, 17, 3) - (13.5 / 14.0) * fs).abs() < 1e-4);
        // The denominator never falls below one.
        assert!((small_font_size(fs, 3, 3) - SMALL_MAX_EM * fs).abs() < 1e-4);
    }

    #[test]
    fn milo_splitter_is_clause_plus_thirty_five_chars() {
        let doc = CaptionDocument {
            duration_frames: 6000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 6000,
                text: "We said, this is quite a long sentence that will need to be split into several segments here."
                    .into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
        // Clause mode: the comma closes the first segment.
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
        assert!(joined.contains("said,|"), "{joined}");
        // remove_punctuation drops the trailing punctuation from display.
        assert!(segments.iter().all(|s| {
            s.words
                .iter()
                .all(|w| !w.display.ends_with(',') && !w.display.ends_with('.'))
        }));
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
