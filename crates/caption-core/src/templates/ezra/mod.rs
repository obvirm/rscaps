//! Native port of the tscaps `ezra` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\ezra\{style.scss,template.json}`.
//! Inter 400 at 3.6cqh in cream (`#f5f0d0`), left-anchored at 0.06W /
//! top 0.55H, soft two-layer black text-shadow. The line splitter
//! lifts the segment's latest qualifying word onto a second line of
//! its own (isolated-word, min 4 letters / 0.8s, fallback 0.6s), where
//! it is re-set in Playfair Display 800 at
//! `clamp(1.2em, 0.86·100cqw / line-width-em, 2.1em)` and slides in
//! from off-frame left; the words around it scale in from 1.05 with
//! origin bottom-left over 0.12s on `cubic-bezier(0.25,0.46,0.45,0.94)`.
//! Sentence segments (6/35, `gap_free`), 0.1em gaps on the small lines
//! around the big one.
//!
//! Port decisions (behaviour-preserving emulations of what the engine
//! does through the browser):
//! - `text-box: trim-both cap alphabetic` on the big line is emulated
//!   with the embedded Playfair's OS/2 `sCapHeight`: the trimmed box
//!   spans cap-top → alphabetic baseline, so its height is the cap
//!   height and `line-height: 0.9` has no effect on it.
//! - Small lines wrap with `white-space: normal` inside the segment's
//!   `max-width: 88cqw`; here that is a greedy word wrap per line at
//!   0.88W (0.88W − margin-left for the third line) — the shrink-wrapped
//!   segment never sits narrower than a line already fits, so the wrap
//!   threshold is exactly the cap.
//! - `--line-width-em` / `--previous-line-width-em` come from the DOM
//!   measurer as `box width / font-size`; the ratio is scale-invariant
//!   (every length in the rule is em-based), so it is measured directly
//!   here instead of mounting a probe subtree.
//! - Per-word `scale-in` about `transform-origin: 0% 100%` is baked
//!   into an explicit `translate(ox(1−s) oy(1−s)) scale(s)` (SVG has
//!   no transform-origin).
//! - Controls at defaults: `contrast-shadow` 0.25 → second layer
//!   `0 0 0.5em rgba(0,0,0,0.2125)`; `big-word-italic` off → upright;
//!   `big-word-font-family` `playfair-display` → "Playfair Display".

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from ezra's style.scss + template.json) ───────────────
const FONT: &str = "Inter"; // fontStack inter
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const BIG_FONT: &str = "Playfair Display"; // --tscaps-big-word-font-family default
const BIG_FONT_WEIGHT: u16 = 800; // .line font-weight: 800
const FONT_SIZE_CQH: f32 = 0.036; // font-size: 3.6cqh
const LINE_HEIGHT: f32 = 1.1; // line-height: 1.1
const WORD_MARGIN_EM: f32 = 0.08; // word-layout(0.08em) = typography.wordSpacing
const LINE_SPACING_EM: f32 = 0.1; // --tscaps-line-spacing fallback (small lines only)
const ANCHOR_Y_FRAC: f32 = 0.55; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.06; // alignment.horizontalOffset (left)
const MAX_WIDTH_RATIO: f32 = 0.88; // .segment max-width: 88cqw
const MAX_CHARS: usize = 35; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 6; // boundary_score_limit_by_chars.minChars
const PRIMARY: &str = "#f5f0d0"; // primary-color default
// text-shadow: soft-drop-shadow(0.03, 0.25) = `0 0.03em 0.25em #000`
// (text-shadow reads the third length as a radius → σ = radius/2),
// then `0 0 0.5em rgb(0 0 0 / calc(0.25 * 0.85))`.
const SHADOW_DY_EM: f32 = 0.03;
const SHADOW_BLUR_EM: f32 = 0.25;
const CONTRAST_SHADOW: f32 = 0.25; // --tscaps-contrast-shadow default
const CONTRAST_BLUR_EM: f32 = 0.5;
// .word scale-in($from: 1.05, $from-opacity: 0, $duration: 0.12s,
// $easing: cubic-bezier(0.25, 0.46, 0.45, 0.94)) — straight settle.
const SCALE_FROM: f32 = 1.05;
const SCALE_DURATION_S: f32 = 0.12;
// big word slide-in($distance: -100cqw, duration.scaled(0.12s, 1.67))
const SLIDE_DURATION_S: f32 = 0.12 * 1.67;
// .first + .line font-size: clamp(1.2em, calc(0.86 * 100cqw /
// var(--line-width-em)), 2.1em) — `em` of the line = segment font-size.
const BIG_SIZE_MIN_EM: f32 = 1.2;
const BIG_SIZE_MAX_EM: f32 = 2.1;
const BIG_FILL_FRAC: f32 = 0.86;
// lineSplitter: isolated-word { minLetters: 4, minSecondsOnScreen: 0.8,
// fallbackMinSecondsOnScreen: 0.6 }.
const MIN_LETTERS: usize = 4;
const MIN_SECONDS: f32 = 0.8;
const FALLBACK_SECONDS: f32 = 0.6;

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: 0.0, // ezra has no letter-spacing
    word_spacing_em: WORD_MARGIN_EM,
    line_spacing_em: LINE_SPACING_EM,
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: false, // ezra keeps the spoken case
    min_lines: 1, // ezra wraps greedily, never balances
    max_lines: 1,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// ezra's `segmentSplitters`: boundary + boundary_score_limit_by_chars
/// 6/35, `effects: [gap_free]`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const INTER_TTF: &[u8] = include_bytes!("../../../assets/inter-latin-400-normal.ttf");
const PLAYFAIR_TTF: &[u8] = include_bytes!("../../../assets/playfair-display-latin-800-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the
/// embedded Inter (the small lines' typography).
fn inter_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(INTER_TTF, 0).expect("Inter parses");
        let upem = face.units_per_em() as f32;
        (
            face.ascender() as f32 / upem,
            face.descender().abs() as f32 / upem,
            upem,
        )
    })
}

/// The embedded Playfair's unitsPerEm (the big word is measured and
/// laid out against its own face).
fn playfair_upem() -> f32 {
    static UPEM: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *UPEM.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(PLAYFAIR_TTF, 0).expect("Playfair parses");
        face.units_per_em() as f32
    })
}

/// The big word's cap height as an em ratio — the trimmed line box
/// (`text-box: trim-both cap alphabetic`) spans cap-top → baseline.
fn playfair_cap_ratio() -> f32 {
    static RATIO: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *RATIO.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(PLAYFAIR_TTF, 0).expect("Playfair parses");
        face.tables()
            .os2
            .and_then(|os2| os2.capital_height())
            .map(|cap| cap as f32 / playfair_upem())
            // OS/2 below version 2 omits sCapHeight; Playfair ships v4,
            // so the fallback never fires — 0.7 is the usual cap/em.
            .unwrap_or(0.7)
    })
}

/// Baseline metrics for whichever family dominates the segment (small
/// register: Inter; big word: Playfair — both go through the stack for
/// script fallback).
fn line_metrics(latin: &str, dominant: &str) -> (f32, f32, f32) {
    if dominant == latin {
        inter_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}

// ── easing + entrance recipes ─────────────────────────────────────────────

/// `cubic-bezier(0.25, 0.46, 0.45, 0.94)` — both of ezra's entrances
/// share it (the .word scale-in and the big word's slide-in).
fn ease_ezra(x: f32) -> f32 {
    recipes::cubic_bezier(0.25, 0.46, 0.45, 0.94)(x)
}

/// `scale-in($element: 'word', $from: 1.05, $from-opacity: 0,
/// $delay: word.narration start, $duration: 0.12s, $easing: ezra's)`.
fn scale_in() -> recipes::ScaleIn {
    recipes::ScaleIn {
        from_scale: SCALE_FROM,
        from_opacity: 0.0,
        peak: None,
        peak_at: 0.6,
        duration_s: SCALE_DURATION_S,
        ease: ease_ezra,
    }
}

/// `slide-in($element: 'word', $distance: -100cqw, $from-opacity: 0,
/// $duration: duration.scaled(0.12s, 1.67), $easing: ezra's)`. The
/// mixin's distance is `-100cqw` — an absolute −W px, not an em of the
/// element — so `distance_em` carries pixels here.
fn slide_in(video_w: f32) -> recipes::SlideIn {
    recipes::SlideIn {
        distance_em: -video_w,
        from_opacity: 0.0,
        duration_s: SLIDE_DURATION_S,
        ease: ease_ezra,
    }
}

// ── isolated-word line splitter ───────────────────────────────────────────

/// Letters/numbers in `text` — the `/[\p{L}\p{N}]/u` count of
/// `IsolatedWordLineSplitter.letterCount`.
fn letter_count(text: &str) -> usize {
    text.chars()
        .filter(|c| c.is_alphabetic() || c.is_numeric())
        .count()
}

/// The word lifted onto its own second line, or `None` for a single
/// line — the port of `IsolatedWordLineSplitter.isolatedWordIndex`:
/// the latest word (index ≥ 1; the first word never qualifies because
/// the line above it would be empty) with enough letters that has been
/// on screen — said → segment end — for `min_seconds`. Pass 1 uses
/// [`MIN_SECONDS`]; pass 2 retries at [`FALLBACK_SECONDS`] only when
/// that is looser and pass 1 found nothing.
fn isolated_word_index(words: &[pipeline::TimedWord], leaves_screen_at_ms: f32) -> Option<usize> {
    fn latest_standing_alone(
        words: &[pipeline::TimedWord],
        leaves_screen_at_ms: f32,
        min_seconds: f32,
    ) -> Option<usize> {
        (1..words.len()).rev().find(|&i| {
            letter_count(&words[i].text) >= MIN_LETTERS
                && leaves_screen_at_ms - words[i].start_ms >= min_seconds * 1000.0
        })
    }
    latest_standing_alone(words, leaves_screen_at_ms, MIN_SECONDS).or_else(|| {
        if FALLBACK_SECONDS < MIN_SECONDS {
            latest_standing_alone(words, leaves_screen_at_ms, FALLBACK_SECONDS)
        } else {
            None
        }
    })
}

// ── layout helpers ────────────────────────────────────────────────────────

/// Greedy word wrap of one line's words: rows of `(start, end)` over
/// `widths`, breaking before the word that would pass `max_w` (a lone
/// word wider than `max_w` keeps its row — no hyphenation).
fn wrap_rows(widths: &[f32], margin: f32, max_w: f32) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut start = 0usize;
    let mut run = 0.0f32;
    for (k, &w) in widths.iter().enumerate() {
        let outer = w + 2.0 * margin;
        if run + outer > max_w && k > start {
            rows.push((start, k));
            start = k;
            run = outer;
        } else {
            run += outer;
        }
    }
    if !widths.is_empty() {
        rows.push((start, widths.len()));
    }
    rows
}

/// `clamp(1.2em, calc(0.86 * 100cqw / line_width_em), 2.1em)` — the
/// big line's font-size. `line_width_em` is the line's width at any
/// font-size divided by that font-size (the measurer's definition);
/// the result makes the word span 0.86W when the clamp is open.
fn big_font_size(fs: f32, video_w: f32, line_width_em: f32) -> f32 {
    (BIG_FILL_FRAC * video_w / line_width_em).clamp(BIG_SIZE_MIN_EM * fs, BIG_SIZE_MAX_EM * fs)
}

/// Control value (tscaps fontStack id, e.g. "playfair-display") → the
/// display name the SVG emits. Known ids map to the embedded face
/// names; unknown ids pass through (svg_font_family) so the browser can
/// resolve a user-installed @font-face — native measurement stays on
/// the embedded Playfair face.
fn big_word_family(id: &str) -> String {
    match id {
        "playfair-display" => BIG_FONT.to_string(),
        "anton" => "Anton".to_string(),
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
    let Some(segment) = segments
        .iter()
        .find(|s| now_ms >= s.start_ms && now_ms < s.end_ms)
    else {
        return Svgr::empty();
    };
    if segment.words.is_empty() {
        return Svgr::empty();
    }

    let video_w = crate::ctx_video_size(ctx).width;
    let video_h = crate::ctx_video_size(ctx).height;
    let w = video_w as f32;
    // User overrides (params) win over the template's spec constants.
    let ov = overrides::resolve(&doc.params, DEFAULTS);
    // Style controls data-driven (panel Style → template.json defaults).
    let st = style::Style::of(&doc.params, "ezra");
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let max_row_w = ov.max_width_ratio * w;
    let block_left = ov.anchor_x * w;
    let block_top = ov.anchor_y * video_h as f32;

    let inter_stack = stack::Stack::for_latin(FONT);
    let big_stack = stack::Stack::for_latin(BIG_FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(FONT, stack::dominant_family(&segment.words, &inter_stack));
    let rtl = segment.direction.is_rtl();
    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let measured = stack::measure_words(
        &segment.words,
        &inter_stack,
        INTER_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    // A small word: scale-in about its own bottom-left corner. The box
    // left is the text's x (the margin sits outside the inline-block)
    // and the box bottom is baseline + descent + half-leading (the box
    // is one 1.1em line).
    let mut scale_in = scale_in();
    scale_in.duration_s = ov.duration_or(scale_in.duration_s);

    let mut body = String::new();
    let mut right_extent = block_left;
    let bottom;

    // small_word with optional family override for non-Latin words.
    let small_word_with_family = |body: &mut String, x: f32, baseline: f32, text: &str, start_ms: f32, family: &str| {
        let (opacity, scale) = if ov.motion_static {
            (1.0, 1.0)
        } else {
            scale_in.eval((now_ms - start_ms) / 1000.0)
        };
        let text = layout::escape_xml(text);
        let transform = if (scale - 1.0).abs() > 1e-4 {
            let ox = x;
            let oy = baseline + descent + half_leading;
            format!(
                " transform=\"translate({} {}) scale({scale})\"",
                ox * (1.0 - scale),
                oy * (1.0 - scale)
            )
        } else {
            String::new()
        };
        let opacity = if opacity < 0.9999 {
            format!(" opacity=\"{opacity}\"")
        } else {
            String::new()
        };
        let family_attr = if family == FONT {
            String::new()
        } else {
            format!(" font-family=\"{family}\"")
        };
        body.push_str(&format!(
            "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\"{family_attr} \
             filter=\"url(#ezra-shadow)\"{transform}{opacity}>{text}</text>",
            primary = ov.primary.as_str(),
        ));
    };

    let isolated = isolated_word_index(&segment.words, segment.end_ms);
    match isolated {
        // One line for the whole segment — every word scales in as it
        // is narrated, rows greedy-wrapped at the segment's 88cqw.
        None => {
            body.push_str(&format!(
                "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
                 letter-spacing=\"{ls_px}\">",
                font = ov.font_family,
                weight = ov.font_weight,
            ));
            let mut y = block_top;
            for (start, end) in wrap_rows(&widths, margin, max_row_w) {
                let run: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
                right_extent = right_extent.max(block_left + run);
                let xs = layout::word_x_positions(&widths[start..end], margin, block_left, run, rtl);
                let baseline = y + half_leading + ascent;
                for ((x, wi), family) in xs.iter().zip(start..end).zip(&measured.families[start..end]) {
                    let word = &segment.words[wi];
                    small_word_with_family(&mut body, *x, baseline, &word.display, word.start_ms, family);
                }
                y += line_h;
            }
            bottom = y;
            body.push_str("</g>");
        }
        // Three shapes from the splitter: [words before, word, after].
        Some(iso) => {
            // ── line 1: the words before the big one (small type) ──
            body.push_str(&format!(
                "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
                 letter-spacing=\"{ls_px}\">",
                font = ov.font_family,
                weight = ov.font_weight,
            ));
            let mut y = block_top;
            for (start, end) in wrap_rows(&widths[..iso], margin, max_row_w) {
                let run: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
                right_extent = right_extent.max(block_left + run);
                let xs = layout::word_x_positions(&widths[start..end], margin, block_left, run, rtl);
                let baseline = y + half_leading + ascent;
                for ((x, wi), family) in xs.iter().zip(start..end).zip(&measured.families[start..end]) {
                    let word = &segment.words[wi];
                    small_word_with_family(&mut body, *x, baseline, &word.display, word.start_ms, family);
                }
                y += line_h;
            }
            body.push_str("</g>");
            // The gap sits on the small line above the big one
            // (`.first:not(.last) { margin-bottom: 0.1em }`, resolved
            // against the small line's font-size).
            y += ov.line_spacing_em * fs;

            // ── line 2: the big word alone, Playfair, trimmed to its
            //    cap height, sliding in from off-frame left ──
            let big_word = &segment.words[iso];
            let big_measured = stack::measure_words(
                std::slice::from_ref(big_word),
                &big_stack,
                PLAYFAIR_TTF,
                BIG_FONT_WEIGHT,
                ctx.font_source,
                fs,
                0.0,
                false,
            );
            let big_adv = big_measured.widths[0];
            // The line's width includes the word's own margins, as the
            // DOM measurer's `getBoundingClientRect` would.
            let big_em = (big_adv + 2.0 * margin) / fs;
            let f_big = big_font_size(fs, w, big_em);
            let cap_px = playfair_cap_ratio() * f_big;
            let baseline_big = y + cap_px;
            right_extent = right_extent.max(block_left + big_em * f_big);

            let mut slide = slide_in(w);
            slide.duration_s = ov.duration_or(slide.duration_s);
            slide.distance_em = ov.distance_or(slide.distance_em);
            let (opacity, dx) = if ov.motion_static {
                (1.0, 0.0)
            } else {
                slide.eval((now_ms - big_word.start_ms) / 1000.0)
            };
            let text = layout::escape_xml(&big_word.display);
            let x = block_left + ov.word_spacing_em * f_big;
            let transform = if dx.abs() > 0.05 {
                format!(" transform=\"translate({dx} 0)\"")
            } else {
                String::new()
            };
            let opacity = if opacity < 0.9999 {
                format!(" opacity=\"{opacity}\"")
            } else {
                String::new()
            };
            let big_font = big_word_family(&st.str("big-word-font-family", "playfair-display"));
            let italic_attr = if st.bool("big-word-italic", false) {
                " font-style=\"italic\""
            } else {
                ""
            };
            body.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline_big}\" fill=\"{primary}\" \
                 font-family=\"{big_font}\" font-weight=\"{BIG_FONT_WEIGHT}\" \
                 font-size=\"{f_big}\" filter=\"url(#ezra-shadow)\"{italic_attr}{transform}{opacity}>\
                 {text}</text>",
                primary = ov.primary.as_str(),
            ));
            y += cap_px;

            // ── line 3: the words after the big one — pushed right so
            //    it ends where the big word ends when the big word is
            //    wider (`.first + .line + .line { margin-left: max(0,
            //    clamp(...) * previous-width-em − own width) }`) ──
            if iso + 1 < segment.words.len() {
                let tail = &widths[iso + 1..];
                let w3: f32 = tail.iter().map(|w| w + 2.0 * margin).sum();
                let delta = (big_em * f_big - w3).max(0.0);
                y += ov.line_spacing_em * fs;
                let left3 = block_left + delta;
                for (start, end) in wrap_rows(tail, margin, (max_row_w - delta).max(0.0)) {
                    let abs_start = iso + 1 + start;
                    let abs_end = iso + 1 + end;
                    let run: f32 = widths[abs_start..abs_end].iter().map(|w| w + 2.0 * margin).sum();
                    right_extent = right_extent.max(left3 + run);
                    let xs =
                        layout::word_x_positions(&widths[abs_start..abs_end], margin, left3, run, rtl);
                    let baseline = y + half_leading + ascent;
                    for ((x, wi), family) in xs.iter().zip(abs_start..abs_end).zip(&measured.families[abs_start..abs_end]) {
                        let word = &segment.words[wi];
                        small_word_with_family(&mut body, *x, baseline, &word.display, word.start_ms, family);
                    }
                    y += line_h;
                }
            }
            bottom = y;
        }
    }

    // User rotation wraps the block about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = block_left;
        let cy = (block_top + bottom) / 2.0;
        body = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = body,
        );
    }

    // The shadow chain lives in user space around the block so the
    // 0.5em blur (σ = 0.25em) never clips: CSS text-shadow doesn't.
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_std = SHADOW_BLUR_EM * fs / 2.0;
    let contrast_std = CONTRAST_BLUR_EM * fs / 2.0;
    // `contrast-shadow` scales the second layer's alpha: 0.25 → 0.2125.
    let contrast_alpha = st.f32("contrast-shadow", CONTRAST_SHADOW) * 0.85;
    let filter = format!(
        "<filter id=\"ezra-shadow\" filterUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" \
         width=\"{}\" height=\"{}\">\
         <feDropShadow dx=\"0\" dy=\"{shadow_dy}\" stdDeviation=\"{shadow_std}\" \
         flood-color=\"#000000\" flood-opacity=\"1.0\" result=\"s1\"/>\
         <feDropShadow in=\"s1\" dx=\"0\" dy=\"0\" stdDeviation=\"{contrast_std}\" \
         flood-color=\"#000000\" flood-opacity=\"{contrast_alpha}\"/></filter>",
        block_left - 2.0 * fs,
        block_top - 1.0 * fs,
        (right_extent - block_left) + 4.0 * fs,
        (bottom - block_top) + 3.0 * fs,
    );

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                {filter}
            </defs>
            {body}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn named(face: &fframes::ttf_parser::Face) -> Option<String> {
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1) — this is the
        // string `resolve_font` matches.
        for id in [16, 1] {
            for n in face.names() {
                if n.name_id == id {
                    if let Some(s) = n.to_string() {
                        return Some(s);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn embedded_inter_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(INTER_TTF, 0).unwrap();
        assert_eq!(named(&face).as_deref(), Some(FONT));
        let (asc, desc, upem) = inter_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn embedded_playfair_800_has_expected_identity_and_cap_height() {
        let face = fframes::ttf_parser::Face::parse(PLAYFAIR_TTF, 0).unwrap();
        assert_eq!(named(&face).as_deref(), Some(BIG_FONT));
        // The RIBBI name (ID 2) says "Regular" — this face carries its
        // 800 weight in the full name (ID 4) and typographic subfamily
        // (ID 17) instead.
        let mut full_name = String::new();
        for n in face.names() {
            if n.name_id == 4 {
                if let Some(s) = n.to_string() {
                    full_name = s.to_lowercase();
                    break;
                }
            }
        }
        assert!(
            full_name.contains("playfair display extrabold"),
            "full name {full_name:?}"
        );
        let cap = playfair_cap_ratio();
        assert!((0.5..0.9).contains(&cap), "cap/em {cap}");
    }

    fn words(times: &[(&str, f32)]) -> Vec<pipeline::TimedWord> {
        times
            .iter()
            .map(|(t, s)| pipeline::TimedWord::new(*t, *s, *s + 200.0))
            .collect()
    }

    #[test]
    fn isolated_splitter_lifts_the_latest_word_that_stands_alone() {
        // The last word has the letters and the screen time → it wins.
        let w = words(&[
            ("so", 0.0),
            ("this", 100.0),
            ("caption", 200.0),
            ("right", 300.0),
            ("ends", 400.0),
        ]);
        assert_eq!(isolated_word_index(&w, 10_000.0), Some(4));
        // The last word too short → the previous candidate wins.
        let w = words(&[
            ("so", 0.0),
            ("this", 100.0),
            ("caption", 200.0),
            ("right", 300.0),
            ("oh", 400.0),
        ]);
        assert_eq!(isolated_word_index(&w, 10_000.0), Some(3));
        // The first word never qualifies — the line above it would be
        // empty.
        let w = words(&[("caption", 0.0), ("x", 100.0), ("y", 200.0)]);
        assert_eq!(isolated_word_index(&w, 10_000.0), None);
        // Nobody has enough letters → one line.
        let w = words(&[("a", 0.0), ("bb", 100.0), ("cc", 200.0)]);
        assert_eq!(isolated_word_index(&w, 10_000.0), None);
        // On-screen time: said → segment end, not word duration.
        // 0.8s exactly reaches pass 1.
        let w = words(&[
            ("so", 0.0),
            ("xx", 100.0),
            ("yy", 200.0),
            ("word", 9_200.0),
        ]);
        assert_eq!(isolated_word_index(&w, 10_000.0), Some(3));
        // 0.65s misses pass 1 but passes the 0.6s fallback.
        let w = words(&[
            ("so", 0.0),
            ("xx", 100.0),
            ("yy", 200.0),
            ("word", 9_350.0),
        ]);
        assert_eq!(isolated_word_index(&w, 10_000.0), Some(3));
        // Below both gates → one line.
        let w = words(&[
            ("so", 0.0),
            ("xx", 100.0),
            ("yy", 200.0),
            ("word", 9_450.0),
        ]);
        assert_eq!(isolated_word_index(&w, 10_000.0), None);
        // Pass 1 outranks the fallback: an earlier word with 0.8s
        // beats a later word that only reaches the fallback.
        let w = words(&[
            ("so", 0.0),
            ("word", 9_100.0), // 0.9s → pass 1
            ("xx", 100.0),
            ("word", 9_350.0), // 0.65s → fallback only
        ]);
        assert_eq!(isolated_word_index(&w, 10_000.0), Some(1));
    }

    #[test]
    fn letter_count_sees_letters_and_numbers_only() {
        assert_eq!(letter_count("it's 42!"), 5);
        assert_eq!(letter_count("..."), 0);
        assert_eq!(letter_count("æø å"), 3);
    }

    #[test]
    fn big_word_size_clamps_to_the_frame() {
        let fs = 38.88;
        let w = 1920.0;
        // Very wide line (em high) → floor at 1.2em.
        assert_eq!(big_font_size(fs, w, 100.0), BIG_SIZE_MIN_EM * fs);
        // Very narrow line → ceiling at 2.1em.
        assert_eq!(big_font_size(fs, w, 1.0), BIG_SIZE_MAX_EM * fs);
        // Open range: 0.86W/em lands between the rails.
        let em = BIG_FILL_FRAC * w / 60.0;
        assert!((big_font_size(fs, w, em) - 60.0).abs() < 1e-3);
    }

    #[test]
    fn word_entrances_settle_on_the_shared_easing() {
        assert_eq!(ease_ezra(0.0), 0.0);
        assert!((ease_ezra(1.0) - 1.0).abs() < 1e-5);
        let scale = scale_in();
        let (o0, s0) = scale.eval(0.0);
        assert_eq!(o0, 0.0);
        assert!((s0 - SCALE_FROM).abs() < 1e-6, "scale {s0}");
        let (o1, s1) = scale.eval(SCALE_DURATION_S);
        assert!((o1 - 1.0).abs() < 1e-4, "opacity {o1}");
        assert!((s1 - 1.0).abs() < 1e-4, "settle {s1}");
        // Before narration start the from-keyframe holds (fill: both).
        let (op, sc) = scale.eval(-0.5);
        assert_eq!(op, 0.0);
        assert!((sc - SCALE_FROM).abs() < 1e-6);

        let slide = slide_in(1920.0);
        let (op0, dx0) = slide.eval(0.0);
        assert_eq!(op0, 0.0);
        assert_eq!(dx0, -1920.0); // -100cqw = one frame width, from the left
        let (op1, dx1) = slide.eval(SLIDE_DURATION_S);
        assert!((op1 - 1.0).abs() < 1e-4, "opacity {op1}");
        assert!(dx1.abs() < 0.01, "dx {dx1}");
    }

    #[test]
    fn small_lines_wrap_at_the_88cqw_cap() {
        // 100 + 100 don't fit in 150 → break before the second word.
        let rows = wrap_rows(&[100.0, 100.0], 10.0, 150.0);
        assert_eq!(rows, vec![(0, 1), (1, 2)]);
        // A lone oversized word keeps its row.
        let rows = wrap_rows(&[400.0], 10.0, 150.0);
        assert_eq!(rows, vec![(0, 1)]);
        // Everything fits → one row; empty input → no rows.
        let rows = wrap_rows(&[40.0, 40.0], 10.0, 150.0);
        assert_eq!(rows, vec![(0, 2)]);
        assert!(wrap_rows(&[], 10.0, 150.0).is_empty());
    }

    #[test]
    fn renders_the_default_document_without_losing_the_spec() {
        use crate::Cue;
        let doc = CaptionDocument {
            duration_frames: 300,
            cues: vec![
                Cue {
                    start_ms: 0,
                    end_ms: 2500,
                    text: "Every word lands right here now".into(),
                },
                Cue {
                    start_ms: 2500,
                    end_ms: 5500,
                    text: "وما زال الكتاب مفتوحاً".into(),
                },
            ],
            ..Default::default()
        };
        // No font source: widths fall back to the UTF-16 heuristic, which
        // still exercises every layout branch — the split, the big-word
        // clamp, the wrap and the shadow region.
        let ctx = fframes::FFramesContext {
            time_base: fframes::TimeBase {
                fps: 30,
                sample_rate: 48_000,
            },
            current_video_size: fframes::VideoSize {
                width: 1920,
                height: 1080,
            },
            duration_in_frames: 300,
            mode: fframes::FFramesMode::Editor,
            scenes: None,
            media_source: None,
            font_source: None,
            abort_signal: None,
        };
        let svg = render(&doc, 1200.0, &ctx);
        let s = svg.to_string();
        assert!(s.contains("ezra-shadow"), "no shadow filter: {s}");
        assert!(s.contains("font-family=\"Inter\""), "no Inter: {s}");
        assert!(s.contains("font-family=\"Playfair Display\""), "no big word: {s}");
        assert!(s.contains("font-weight=\"800\""), "no big weight: {s}");
        assert!(!s.contains("NaN") && !s.contains("undefined"), "{s}");
        // At 1200ms the segment is narrating: one word slid home (no
        // transform), the rest still mid-entrance or held at 0.
        assert!(s.contains(">lands</text>") || s.contains(">right</text>"), "{s}");
        // Nothing on screen before the segment starts.
        let before = render(&doc, -100.0, &ctx);
        assert!(before.to_string().is_empty(), "{before}");
    }
}
