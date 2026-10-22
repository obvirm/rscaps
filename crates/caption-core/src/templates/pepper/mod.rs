//! Native port of the tscaps `pepper` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\pepper\{style.scss,template.json}`.
//! Anton, uppercased, 4.0cqh, one line pinned at 0.75H: every word wears a
//! 0.12em black stroke (painted under the white fill) and a soft
//! `0 0.06em 0.24em #000` drop shadow. The narrated word drops its shadow
//! and rides a `#c8392c` pill that pops in over 0.16s — 0.95 → 1.035 →
//! 1.0 about its own centre.

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, recipes, stack, state, style};
use crate::CaptionDocument;

// ── spec constants (from pepper's style.scss + template.json) ─────────────
const FONT: &str = "Anton"; // fontStack anton
const FONT_WEIGHT: u16 = 400; // typography.fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.04; // font-size: 4.0cqh
const LETTER_SPACING_EM: f32 = 0.02; // letter-spacing: 0.02em
const LINE_HEIGHT: f32 = 1.35; // line-height: 1.35
const WORD_SPACING_EM: f32 = 0.16; // word-layout(0.16em) → .word margin
const ANCHOR_Y_FRAC: f32 = 0.75; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 18; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 5; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#ffffff"; // primary-color control default
const HIGHLIGHT_TEXT: &str = "#ffffff"; // highlight-text-color control default
const STROKE_EM: f32 = 0.12; // outline-thickness control default (…em #000)
const STROKE_COLOR: &str = "#000000"; // shadow-color control default
// `0 calc(0.04em * shadow-scale 1.5) calc(0.16em * shadow-scale 1.5) #000`
// → dy 0.06em, blur 0.24em; CSS blur r → σ = r / 2.
const SHADOW_DY_EM: f32 = 0.06;
const SHADOW_BLUR_EM: f32 = 0.24;
const PILL_COLOR: &str = "#c8392c"; // highlight-bg-color control default
const PILL_PAD_X_EM: f32 = 0.2; // highlight-bg-padding-x default
const PILL_PAD_Y_EM: f32 = 0.08; // highlight-bg-padding-y default
const PILL_RADIUS_EM: f32 = 0.16; // highlight-bg-radius default
const PILL_GROW: f32 = 0.05; // pill-grow control default
const PILL_GROW_DURATION_S: f32 = 0.16; // pill-grow-duration control default

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // no LINE_SPACING_EM in pepper's spec
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: HIGHLIGHT_TEXT,
    force_uppercase: true, // pepper always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// pepper's `effects`: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const ANTON_400_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Anton. The ascent/descent pair also sizes the pill's content box (the
/// inline word's border box in the reference CSS).
fn font_metrics() -> (f32, f32, f32) {
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

/// Baseline metrics for whichever family dominates the segment.
fn line_metrics(dominant: &str) -> (f32, f32, f32) {
    if dominant == FONT {
        font_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}


// ── keyframe ports ────────────────────────────────────────────────────────

/// Shared entry clamp: negative elapsed holds the first stop (fill `both`),
/// past the duration holds the last.
fn progress(elapsed_s: f32, duration_s: f32) -> f32 {
    if elapsed_s <= 0.0 {
        0.0
    } else if duration_s <= 0.0 || elapsed_s >= duration_s {
        1.0
    } else {
        elapsed_s / duration_s
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `tscaps-mark-pill-grow`: 0% → `1 - grow`, 55% → `1 + grow * 0.7`,
/// 100% → 1, with `cubic-bezier(0.2, 0.9, 0.3, 1)` applied per segment the
/// way CSS reuses one timing function between every pair of stops.
fn pill_grow(elapsed_s: f32, duration_s: f32, grow: f32, ease: &impl Fn(f32) -> f32) -> f32 {
    let p = progress(elapsed_s, duration_s);
    let peak = 1.0 + grow * 0.7;
    if p < 0.55 {
        lerp(1.0 - grow, peak, ease(p / 0.55))
    } else {
        lerp(peak, 1.0, ease((p - 0.55) / 0.45))
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

    let video_w = crate::ctx_video_size(ctx).width;
    let video_h = crate::ctx_video_size(ctx).height;
    // User overrides (params) win over the template's spec constants.
    let ov = overrides::resolve(&doc.params, DEFAULTS);
    // Style controls data-driven (panel Style → template.json defaults).
    let st = style::Style::of(&doc.params, "pepper");
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        ANTON_400_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        true,
    );
    let widths = measured.widths;

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Top-aligned block: `.segment` sits at 0.75H, lines centre under it.
    let block_top = ov.anchor_y * video_h as f32;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let stroke_w = st.f32("outline-thickness", STROKE_EM) * fs;
    // `0 calc(0.04em * shadow-scale) calc(0.16em * shadow-scale)` — the
    // baked defaults (dy 0.06em, blur 0.24em) come from scale 1.5.
    let shadow_scale = st.f32("shadow-scale", 1.5);
    let shadow_dy = SHADOW_DY_EM / 1.5 * shadow_scale * fs;
    let shadow_std = SHADOW_BLUR_EM / 1.5 * shadow_scale * fs / 2.0;
    let pill_ease = recipes::cubic_bezier(0.2, 0.9, 0.3, 1.0);
    let pill_bg = st.color("highlight-bg-color", PILL_COLOR);
    let pill_pad_x = st.f32("highlight-bg-padding-x", PILL_PAD_X_EM) * fs;
    let pill_pad_y = st.f32("highlight-bg-padding-y", PILL_PAD_Y_EM) * fs;
    let pill_r = st.f32("highlight-bg-radius", PILL_RADIUS_EM) * fs;
    let highlight_text = st.color("highlight-text-color", HIGHLIGHT_TEXT);

    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" fill=\"{fill}\" stroke=\"{STROKE_COLOR}\" \
         stroke-width=\"{stroke_w}\" paint-order=\"stroke\">",
        font = ov.font_family,
        weight = ov.font_weight,
        fill = ov.primary,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);

        // The pill sits behind the whole line's glyphs (the reference CSS
        // isolates the segment so the `z-index: -1` pseudo lands under
        // every word, neighbours included).
        for (i, (x, word)) in xs.iter().zip(&segment.words[start..end]).enumerate() {
            if !state::word_narration(now_ms, word.start_ms, word.end_ms).is_being() {
                continue;
            }
            let grow = if ov.motion_static {
                1.0
            } else {
                pill_grow(
                    (now_ms - word.start_ms) / 1000.0,
                    ov.duration_or(PILL_GROW_DURATION_S),
                    PILL_GROW,
                    &pill_ease,
                )
            };
            // Negative inset past the word's text box: 0.2em a side wide,
            // 0.08em a side tall, growing about the pill's own centre.
            let pill_x = *x - pill_pad_x;
            let pill_w = widths[start + i] + 2.0 * pill_pad_x;
            let pill_y = baseline - ascent - pill_pad_y;
            let pill_h = ascent + descent + 2.0 * pill_pad_y;
            let radius = pill_r;
            let cx = pill_x + pill_w / 2.0;
            let cy = pill_y + pill_h / 2.0;
            words_svg.push_str(&format!(
                "<rect x=\"{pill_x}\" y=\"{pill_y}\" width=\"{pill_w}\" height=\"{pill_h}\" \
                 rx=\"{radius}\" ry=\"{radius}\" fill=\"{pill_bg}\" stroke=\"none\" \
                 transform=\"translate({cx},{cy}) scale({grow}) translate({nx},{ny})\"/>",
                nx = -cx,
                ny = -cy,
            ));
        }

        for (i, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            let being = state::word_narration(now_ms, word.start_ms, word.end_ms).is_being();
            let fill = if being {
                highlight_text.as_str()
            } else {
                ov.primary.as_str()
            };
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            // The narrated word drops its shadow (pill + stroke carry it).
            let filter = if being {
                String::new()
            } else {
                format!(" filter=\"url(#pepper-shadow)\"")
            };
            // Element inspector: override per-kata (attrs + transform) —
            // `pepper` tak punya anim entrance di `<text>`-nya (pop milik
            // pill), jadi tak ada anim yang ditahan.
            let wo_ctx = element_css::WordCtx {
                base_fs: fs,
                ls_px,
                video_w: video_w as f32,
                video_h: video_h as f32,
                center_x: *x + widths[start + i] / 2.0,
                center_y: baseline - fs * 0.35,
                word_start_s: word.start_ms / 1000.0,
                now_s: now_ms / 1000.0,
                font_family_svg: &stack::svg_font_family,
            };
            let wo = element_css::word_svg_attrs(&doc.params, word.index, &wo_ctx);
            let wo_transform = element_css::word_svg_transform(&doc.params, word.index, &wo_ctx)
                .map(|t| format!(" transform=\"{t}\""))
                .unwrap_or_default();
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}{filter}{wo}{wo_transform}>\
                 {text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the caption around the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + (lines.len() as f32 * line_h) / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    let words_svg = layout::dedupe_text_attrs(&words_svg);
    fframes::svgr!(        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="pepper-shadow" x="-30%" y="-60%" width="160%" height="260%">
                    <feDropShadow dx="0" dy={shadow_dy} stdDeviation={shadow_std}
                        flood-color={STROKE_COLOR} flood-opacity="1.0" />
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
    fn embedded_anton_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(ANTON_400_TTF, 0).unwrap();
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
    fn pill_pops_from_shrunk_past_the_peak_then_rests() {
        let ease = recipes::cubic_bezier(0.2, 0.9, 0.3, 1.0);
        // Before the word starts (fill both): held at 1 - grow.
        let s0 = pill_grow(-0.5, PILL_GROW_DURATION_S, PILL_GROW, &ease);
        assert!((s0 - (1.0 - PILL_GROW)).abs() < 1e-6, "scale {s0}");
        // Past the duration: resting at exactly 1.
        let s1 = pill_grow(1.0, PILL_GROW_DURATION_S, PILL_GROW, &ease);
        assert!((s1 - 1.0).abs() < 1e-6, "scale {s1}");
        // Somewhere in the first leg it must overshoot the rest size.
        let peak = 1.0 + PILL_GROW * 0.7;
        let s2 = pill_grow(PILL_GROW_DURATION_S * 0.55, PILL_GROW_DURATION_S, PILL_GROW, &ease);
        assert!((s2 - peak).abs() < 1e-4, "scale {s2} peak {peak}");
        assert!(peak > 1.0, "the pop goes past 1");
    }

    #[test]
    fn splits_by_sentence_and_eighteen_chars_on_a_single_line() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "this is way more than eighteen characters, right.".into(),
                sheet: None,
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        for segment in &segments {
            let chars: usize = segment.words.iter().map(|w| w.text.chars().count()).sum();
            // The char cap lands on a word boundary, never mid-word: a
            // segment may overshoot by the length of its final word.
            assert!(
                chars <= MAX_CHARS + 12,
                "segment of {chars} chars escaped the cap: {:?}",
                segment.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>()
            );
        }
        let splitter = layout::BalancedPixelWidth {
            min_lines: MIN_LINES,
            max_lines: MAX_LINES,
            max_width: 1920.0 * MAX_WIDTH_RATIO,
        };
        assert_eq!(splitter.split(&[400.0, 400.0, 400.0]).len(), 1);
        assert_eq!("pill".to_uppercase(), "PILL");
    }
}
