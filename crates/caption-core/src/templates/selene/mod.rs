//! Native port of the tscaps `selene` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\selene\{style.scss,template.json}`.
//! Montserrat SemiBold at 3.6cqh inside a glass plate: the segment is an
//! inline-block padded `0.45em 0.9em`, rounded `2.0em`, and wears a
//! `0 0.15em 0.5em rgba(0,0,0,0.28)` outer shadow. Its `::after` body
//! carries the top-down white tint (`0.192` at 0% → `0.12` at 100%) and
//! the six inset hairlines, each `0.04em`, painted in CSS order (the
//! last-listed shadow under the first): white `0.22` border bands on all
//! four edges, then the black `0.18` bottom band and the white `0.4` top
//! highlight landing on top of theirs. Words wear two black 0.55 shadows
//! (`0 0 0.22em`, `0 0.04em 0.08em`). One line, top-anchored at 0.78H and
//! centred at 0.5W; sentence boundary with a 10–20 char limit; `gap_free`
//! + `smart_punctuation`. No entrance animation. The video sits *under*
//! the plate (`isolation: isolate`, `z-index: -1` layer), so the plate's
//! shadow composites over the frame.
//!
//! Unported: `.tscaps-video-frame-layer` (the blurred, saturated video
//! backdrop the plate is made of glass over) — the native render draws no
//! video backdrop layer; `.emphasis/.accent/.entity` weight and
//! `text-decoration` key off word tags and the decoration toggle, which
//! the native `Cue` text does not carry; the `variants` colour palette is
//! editor UI (every variant pairs `highlight-color` with `primary-color`,
//! so `.word-being-narrated` never differs from `.word` — one fill).
//!
//! Family note: the embedded Montserrat 600 carries the name-table family
//! "Montserrat Thin" (fontsource packaging) — that is the string fontdb
//! resolves (`FONTDB_FAMILY`, used to measure), while the SVG emits `FONT`
//! so the studio's @font-face ("Montserrat") matches in the browser.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, stack};
use crate::CaptionDocument;

// ── spec constants (from selene's style.scss + template.json) ─────────────
const FONT: &str = "Montserrat"; // fontStack montserrat (browser @font-face)
const FONT_WEIGHT: u16 = 600; // typography.fontWeight: 600
const FONT_SIZE_CQH: f32 = 0.036; // font-size: 3.6cqh
const LETTER_SPACING_EM: f32 = 0.0; // typography.letterSpacing: 0
const LINE_HEIGHT: f32 = 1.25; // line-height: 1.25
const WORD_SPACING_EM: f32 = 0.1; // word-layout(0.1em) → .word margin
const LINE_SPACING_EM: f32 = 0.1; // .line + .line margin-top: 0.1em
const PAD_Y_EM: f32 = 0.45; // .segment padding-block: 0.45em
const PAD_X_EM: f32 = 0.9; // .segment padding-inline: 0.9em
const CORNER_RADIUS_EM: f32 = 2.0; // border-radius: 2.0em
const ANCHOR_Y_FRAC: f32 = 0.78; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center default)
const MAX_CHARS: usize = 20; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 10; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const MAX_GAP_MS: f32 = 1000.0; // gap_free default maxGapMs
const PRIMARY: &str = "#ffffff"; // primary-color / highlight-color default
// box-shadow: 0 0.15em 0.5em rgba(0, 0, 0, 0.28)  (CSS blur r → σ = r / 2)
const PLATE_DY_EM: f32 = 0.15;
const PLATE_BLUR_EM: f32 = 0.5;
const PLATE_SHADOW_ALPHA: f32 = 0.28;
// ::after tint: rgba(255,255,255, tint-opacity * 1.6) at 0% → tint at 100%
const TINT_TOP_ALPHA: f32 = 0.192; // 0.12 × 1.6
const TINT_BOTTOM_ALPHA: f32 = 0.12; // --tscaps-tint-opacity default
// inset box-shadow bands: 0.04em each, emitted last-listed first (CSS
// paint order — the first-listed inset lands on top of the rest).
const HAIRLINE_EM: f32 = 0.04;
const TOP_HIGHLIGHT_ALPHA: f32 = 0.4; // inset 0 0.04em 0 rgba(255,255,255,.4)
const BOTTOM_EDGE_ALPHA: f32 = 0.18; // inset 0 -0.04em 0 rgba(0,0,0,.18)
const BORDER_ALPHA: f32 = 0.22; // inset ±0.04em 0 0 / 0 ±0.04em 0 rgba(255,255,255,.22)
// text-shadow: 0 0 0.22em rgba(0,0,0,.55), 0 0.04em 0.08em rgba(0,0,0,.55)
const WORD_SHADOW1_BLUR_EM: f32 = 0.22;
const WORD_SHADOW2_DY_EM: f32 = 0.04;
const WORD_SHADOW2_BLUR_EM: f32 = 0.08;
const WORD_SHADOW_ALPHA: f32 = 0.55;

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: LINE_SPACING_EM,
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: false,
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// selene's pipeline: sentence boundary → 10–20 char limit → gap_free +
/// smart_punctuation.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: MAX_GAP_MS,
    effects: &[effects::Effect::GapFree, effects::Effect::SmartPunctuation],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const MONTSERRAT_600_TTF: &[u8] =
    include_bytes!("../../../assets/montserrat-latin-600-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Montserrat 600.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face =
            fframes::ttf_parser::Face::parse(MONTSERRAT_600_TTF, 0).expect("Montserrat parses");
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

// ── plate geometry ────────────────────────────────────────────────────────

/// Border box of the `.segment`: shrink-to-fit around `content_w` (the
/// widest line) plus the padding, top edge at `verticalOffset`, centred on
/// `horizontalOffset`. Returns `(x, y, w, h)`.
fn plate_box(
    fs: f32,
    content_w: f32,
    lines: usize,
    video_w: f32,
    video_h: f32,
) -> (f32, f32, f32, f32) {
    let w = content_w + 2.0 * PAD_X_EM * fs;
    let h = lines as f32 * LINE_HEIGHT * fs
        + lines.saturating_sub(1) as f32 * LINE_SPACING_EM * fs
        + 2.0 * PAD_Y_EM * fs;
    (
        ANCHOR_X_FRAC * video_w - w / 2.0,
        ANCHOR_Y_FRAC * video_h,
        w,
        h,
    )
}

/// CSS `border-radius` clamping: the radii scale uniformly until they fit
/// each edge, so a 2em corner on a plate shorter than 4em becomes a
/// capsule.
fn corner_radius(fs: f32, w: f32, h: f32) -> f32 {
    (CORNER_RADIUS_EM * fs).min(w / 2.0).min(h / 2.0)
}

/// The six `.segment::after` inset hairlines over a plate box `(x, y,
/// w, h)`, each `0.04em`, emitted in CSS paint order — the box-shadow
/// list reversed, so the first-listed inset lands on top: the four white
/// border bands, the black bottom edge, then the white top highlight.
/// The caller clips the group to the plate's rounded silhouette.
fn hairlines_svg(fs: f32, x: f32, y: f32, w: f32, h: f32) -> String {
    let hair = HAIRLINE_EM * fs;
    let bottom = y + h - hair;
    let right = x + w - hair;
    format!(
        "<rect x=\"{right}\" y=\"{y}\" width=\"{hair}\" height=\"{h}\" \
         fill=\"#ffffff\" fill-opacity=\"{BORDER_ALPHA}\"/>\
         <rect x=\"{x}\" y=\"{y}\" width=\"{hair}\" height=\"{h}\" \
         fill=\"#ffffff\" fill-opacity=\"{BORDER_ALPHA}\"/>\
         <rect x=\"{x}\" y=\"{bottom}\" width=\"{w}\" height=\"{hair}\" \
         fill=\"#ffffff\" fill-opacity=\"{BORDER_ALPHA}\"/>\
         <rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{hair}\" \
         fill=\"#ffffff\" fill-opacity=\"{BORDER_ALPHA}\"/>\
         <rect x=\"{x}\" y=\"{bottom}\" width=\"{w}\" height=\"{hair}\" \
         fill=\"#000000\" fill-opacity=\"{BOTTOM_EDGE_ALPHA}\"/>\
         <rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{hair}\" \
         fill=\"#ffffff\" fill-opacity=\"{TOP_HIGHLIGHT_ALPHA}\"/>"
    )
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
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    let ls_px = ov.letter_spacing_em * fs;
    let margin = ov.word_spacing_em * fs;
    let line_h = LINE_HEIGHT * fs;
    let line_gap = ov.line_spacing_em * fs;
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        MONTSERRAT_600_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
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

    // Shrink-to-fit plate around the widest line; the block's top edge
    // lands on 0.78H and lines centre inside the content box. The plate's
    // size comes from plate_box; its position honours the user's anchor
    // overrides (the same formula plate_box applies).
    let content_w = lines
        .iter()
        .map(|&(start, end)| widths[start..end].iter().map(|w| w + 2.0 * margin).sum::<f32>())
        .fold(0.0f32, f32::max);
    let (_, _, plate_w, plate_h) =
        plate_box(fs, content_w, lines.len(), video_w as f32, video_h as f32);
    let plate_x = ov.anchor_x * video_w as f32 - plate_w / 2.0;
    let plate_y = ov.anchor_y * video_h as f32;
    let radius = corner_radius(fs, plate_w, plate_h);
    let content_left = plate_x + PAD_X_EM * fs;
    let content_top = plate_y + PAD_Y_EM * fs;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Box-shadow geometry (px from em ratios; CSS blur r → σ = r / 2).
    let plate_dy = PLATE_DY_EM * fs;
    let plate_std = PLATE_BLUR_EM * fs / 2.0;
    let word1_std = WORD_SHADOW1_BLUR_EM * fs / 2.0;
    let word2_dy = WORD_SHADOW2_DY_EM * fs;
    let word2_std = WORD_SHADOW2_BLUR_EM * fs / 2.0;

    // The plate: outer shadow (clipped away from the plate itself, as CSS
    // casts it from an opaque border-box), the tint body, then the six
    // hairlines clipped to the rounded silhouette.
    let hairlines = hairlines_svg(fs, plate_x, plate_y, plate_w, plate_h);
    let plate_svg = format!(
        "<rect x=\"{plate_x}\" y=\"{plate_y}\" width=\"{plate_w}\" height=\"{plate_h}\" \
         rx=\"{radius}\" ry=\"{radius}\" fill=\"#000000\" filter=\"url(#selene-plate)\"/>\
         <rect x=\"{plate_x}\" y=\"{plate_y}\" width=\"{plate_w}\" height=\"{plate_h}\" \
         rx=\"{radius}\" ry=\"{radius}\" fill=\"url(#selene-tint)\"/>\
         <g clip-path=\"url(#selene-clip)\">{hairlines}</g>"
    );

    let font_family_attr = stack::svg_font_family(&ov.font_family);
    let mut words_svg = format!(
        "<g font-family=\"{font_family_attr}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" fill=\"{fill}\">",
        weight = ov.font_weight,
        fill = ov.primary,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = content_top + line_idx as f32 * (line_h + line_gap);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = content_left + (content_w - line_w) / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{}\"", stack::svg_font_family(&family))
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" filter=\"url(#selene-word)\"{family_attr}>{text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    let defs_svg = format!(
        "<filter id=\"selene-plate\" x=\"-40%\" y=\"-60%\" width=\"180%\" height=\"260%\" \
         color-interpolation-filters=\"sRGB\">\
         <feDropShadow dx=\"0\" dy=\"{plate_dy}\" stdDeviation=\"{plate_std}\" \
         flood-color=\"#000000\" flood-opacity=\"{PLATE_SHADOW_ALPHA}\" result=\"shadow\"/>\
         <feComposite in=\"shadow\" in2=\"SourceAlpha\" operator=\"out\"/>\
         </filter>\
         <filter id=\"selene-word\" x=\"-40%\" y=\"-60%\" width=\"180%\" height=\"260%\" \
         color-interpolation-filters=\"sRGB\">\
         <feDropShadow dx=\"0\" dy=\"0\" stdDeviation=\"{word1_std}\" flood-color=\"#000000\" \
         flood-opacity=\"{WORD_SHADOW_ALPHA}\" result=\"s1\"/>\
         <feDropShadow in=\"s1\" dx=\"0\" dy=\"{word2_dy}\" stdDeviation=\"{word2_std}\" \
         flood-color=\"#000000\" flood-opacity=\"{WORD_SHADOW_ALPHA}\"/>\
         </filter>\
         <linearGradient id=\"selene-tint\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\
         <stop offset=\"0\" stop-color=\"#ffffff\" stop-opacity=\"{TINT_TOP_ALPHA}\"/>\
         <stop offset=\"1\" stop-color=\"#ffffff\" stop-opacity=\"{TINT_BOTTOM_ALPHA}\"/>\
         </linearGradient>\
         <clipPath id=\"selene-clip\">\
         <rect x=\"{plate_x}\" y=\"{plate_y}\" width=\"{plate_w}\" height=\"{plate_h}\" \
         rx=\"{radius}\" ry=\"{radius}\"/>\
         </clipPath>"
    );

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                {defs_svg}
            </defs>
            {plate_svg}
            {words_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn joined_segments(doc: &CaptionDocument) -> Vec<String> {
        pipeline::collect_segments(doc, &SPLIT)
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    #[test]
    fn embedded_montserrat_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(MONTSERRAT_600_TTF, 0).unwrap();
        // fontdb's rule (parse_names): typographic family (name ID 16)
        // when present, else the legacy family (ID 1) — this is the
        // string `resolve_font` matches, and the TTF ships ID 16 as
        // "Montserrat Thin".
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
        assert_eq!(name.as_deref(), Some("Montserrat Thin"));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn plate_wraps_the_line_with_padding_and_clamps_its_radius() {
        let fs = FONT_SIZE_CQH * 1080.0;
        let content_w = 400.0;
        let (x, y, w, h) = plate_box(fs, content_w, 1, 1920.0, 1080.0);
        assert!((w - (content_w + 2.0 * PAD_X_EM * fs)).abs() < 1e-3, "w {w}");
        // One line: line-height 1.25em plus both 0.45em padding blocks.
        assert!((h - ((LINE_HEIGHT + 2.0 * PAD_Y_EM) * fs)).abs() < 1e-3, "h {h}");
        assert!((y - (ANCHOR_Y_FRAC * 1080.0)).abs() < 1e-3, "y {y}");
        assert!((x - (ANCHOR_X_FRAC * 1920.0 - w / 2.0)).abs() < 1e-3, "x {x}");
        // 2.0em asks for more than half the plate's height, so CSS scales
        // the radius down until the corners fit — the plate is a capsule.
        let r = corner_radius(fs, w, h);
        assert!((r - h / 2.0).abs() < 1e-3, "radius {r}");
        assert!(r < CORNER_RADIUS_EM * fs);
    }

    #[test]
    fn tint_gradient_runs_from_the_control_default_times_one_point_six() {
        assert!((TINT_BOTTOM_ALPHA * 1.6 - TINT_TOP_ALPHA).abs() < 1e-6);
        assert!((TINT_BOTTOM_ALPHA - 0.12).abs() < 1e-6);
    }

    #[test]
    fn hairlines_paint_all_six_insets_in_css_order() {
        let svg = hairlines_svg(10.0, 4.0, 8.0, 100.0, 50.0);
        assert_eq!(svg.matches("<rect").count(), 6, "{svg}");
        fn attr<'a>(svg: &'a str, key: &str) -> Vec<&'a str> {
            svg.split(key)
                .skip(1)
                .map(|s| s.split('"').next().unwrap())
                .collect()
        }
        // Four white border bands first (they are the last-listed insets,
        // so they paint underneath), then the black bottom edge and the
        // white top highlight on top of them.
        assert_eq!(
            attr(&svg, "fill=\""),
            ["#ffffff", "#ffffff", "#ffffff", "#ffffff", "#000000", "#ffffff"]
        );
        assert_eq!(
            attr(&svg, "fill-opacity=\""),
            [
                BORDER_ALPHA.to_string(),
                BORDER_ALPHA.to_string(),
                BORDER_ALPHA.to_string(),
                BORDER_ALPHA.to_string(),
                BOTTOM_EDGE_ALPHA.to_string(),
                TOP_HIGHLIGHT_ALPHA.to_string(),
            ]
        );
        // Every band is one hairline (0.04em) thick.
        let hair = HAIRLINE_EM * 10.0;
        assert_eq!(svg.matches(&format!("height=\"{hair}\"")).count(), 4);
        assert_eq!(svg.matches(&format!("width=\"{hair}\"")).count(), 2);
        // Bands hug their own edges of the plate box.
        assert!(svg.contains(&format!("x=\"{}\"", 4.0 + 100.0 - hair)), "{svg}");
        assert!(svg.contains(&format!("y=\"{}\"", 8.0 + 50.0 - hair)), "{svg}");
    }

    #[test]
    fn selene_splits_at_sentence_ends_within_twenty_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Hello world. Bye now.".into(),
            }],
            ..Default::default()
        };
        assert_eq!(joined_segments(&doc), vec!["Hello world.", "Bye now."]);
    }

    #[test]
    fn char_limit_cuts_long_sentences_into_ten_to_twenty_char_segments() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "one two three four five six seven eight nine ten".into(),
            }],
            ..Default::default()
        };
        let segments = joined_segments(&doc);
        assert!(segments.len() >= 2, "expected a char-limit split");
        for segment in &segments {
            let weight = segment.encode_utf16().count();
            assert!(weight <= MAX_CHARS, "{segment} weighs {weight}");
        }
    }

    #[test]
    fn smart_punctuation_typographs_the_display() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait... \"hi\" -- ok".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let display: Vec<&str> = segments
            .iter()
            .flat_map(|s| s.words.iter().map(|w| w.display.as_str()))
            .collect();
        assert_eq!(
            display,
            vec![
                "Wait\u{2026}",
                "\u{201C}hi\u{201D}",
                "\u{2014}",
                "ok"
            ]
        );
        // Splitters still see the raw text.
        assert_eq!(segments[0].words[0].text, "Wait...");
    }
}
