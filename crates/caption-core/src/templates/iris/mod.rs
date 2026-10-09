//! Native port of the tscaps `iris` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\iris\{style.scss,template.json,filters.svg}`.
//! Righteous at 4.5cqh (the scss's `'Caveat Variable'` is stale — the
//! json `fontStack: "righteous"` wins). The whole segment sits inside a
//! single watercolour stroke: the `.segment::before` pseudo is an
//! alpha-masked rectangle tinted `#ffe066` (the `highlight-color-1`
//! control default; the CSS fallback `#a3d8f5` is dead once the control
//! is emitted) that bleeds 0.84em left/right and 0.56em up/down past
//! the text box and drifts +0.06em (`mask-offset-y`) down. Each word
//! wears a 0.08em black `brush-outline` ring scoped to its own glyph
//! silhouette (so neighbouring words never clip each other's outline
//! and the dilation never swells the stroke blob). Clause segments of
//! 10 chars (min 0, descriptor default), one balanced row, block
//! centre-anchored at 0.81H, `gap_free` holding the window open.
//!
//! Mask port: `templates/iris/marker-stroke.b64` is the base64 of
//! `tscaps/templates/_assets/marker-stroke.png` (21624 bytes); an
//! alpha-typed `<mask>` stretches it over the bleed rect (CSS
//! `mask-size: 100% 100%; mask-repeat: no-repeat; mask-position:
//! center`), and a solid rect of the highlight colour is painted
//! through it *before* the text. The stroke is a decorative background
//! behind the words, not a content mask over them — that is what the
//! `.segment::before` pseudo does in scss.

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{effects, layout, pipeline};
use crate::CaptionDocument;

// ── spec constants (from iris's style.scss + template.json) ──────────────
const FONT: &str = "Righteous"; // fontStack righteous (json wins over scss)
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_SIZE_CQH: f32 = 0.045; // font-size: 4.5cqh
const LETTER_SPACING_EM: f32 = 0.02; // letter-spacing: 0.02em
const LINE_HEIGHT: f32 = 1.4; // line-height: 1.4 (scss explicit)
const WORD_SPACING_EM: f32 = 0.16; // @include word-layout(0.16em)
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top: 0
const ANCHOR_Y_FRAC: f32 = 0.81; // alignment.verticalOffset (center)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalAlign default (center)
const MAX_CHARS: usize = 10; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // ...minChars (descriptor default; absent in json)
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
const PRIMARY: &str = "#ffffff"; // primary-color default
const STROKE_COLOR: &str = "#ffe066"; // highlight-color-1 control default
const MASK_PAD_X_EM: f32 = 0.84; // mask-padding-x control default
const MASK_PAD_Y_EM: f32 = 0.56; // mask-padding-y control default
const MASK_OFFSET_Y_EM: f32 = 0.06; // mask-offset-y control default
const OUTLINE_THICKNESS_EM: f32 = 0.08; // outline-thickness control default
const OUTLINE_COLOR: &str = "#000000"; // outline-color default

/// The bundled watercolour stroke (alpha-only PNG), base64, as emitted
/// into the `data:` URL. Source: `tscaps/templates/_assets/marker-stroke.png`.
const MARKER_STROKE_B64: &str = include_str!("marker-stroke.b64");

/// iris's `effects` array: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const RIGHTEOUS_400_TTF: &[u8] = include_bytes!("../../../assets/righteous-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Righteous.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(RIGHTEOUS_400_TTF, 0).expect("Righteous parses");
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

/// `text-transform: uppercase` is *not* set in iris's scss — the words
/// pass through unchanged.
fn normalize(text: &str) -> String {
    text.to_string()
}

// ── stroke mask + brush outline ───────────────────────────────────────────

/// The `.segment::before` watercolour stroke: the bundled PNG as an
/// alpha-typed `<mask>` stretched over the bleed box (CSS
/// `mask-size: 100% 100%`), plus the highlight-coloured rect painted
/// through it. `block_left/top/w/h` are the *text* block; the bleed
/// grows the mask box by the padding controls.
fn stroke_defs(block_left: f32, block_top: f32, block_w: f32, block_h: f32, fs: f32) -> String {
    let px = MASK_PAD_X_EM * fs;
    let py = MASK_PAD_Y_EM * fs;
    let x = block_left - px;
    let y = block_top - py + MASK_OFFSET_Y_EM * fs;
    let w = block_w + 2.0 * px;
    let h = block_h + 2.0 * py;
    format!(
        "<mask id=\"iris-stroke\" mask-type=\"alpha\" x=\"{x}\" y=\"{y}\" width=\"{w}\" \
         height=\"{h}\" maskUnits=\"userSpaceOnUse\">\
         <image href=\"data:image/png;base64,{MARKER_STROKE_B64}\" x=\"{x}\" y=\"{y}\" \
         width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\"/>\
         </mask>\
         <rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{STROKE_COLOR}\" \
         mask=\"url(#iris-stroke)\"/>"
    )
}

/// The `brush-outline` filter body from filters.svg: the `tscaps:outline`
/// recipe at the control-default thickness, merged under SourceGraphic.
/// Sits on each `.word`, not the segment, so the dilation stays scoped
/// to glyph silhouettes.
fn filter_defs(fs: f32) -> String {
    let radius = OUTLINE_THICKNESS_EM * fs;
    format!(
        "<filter id=\"iris-brush-outline\" x=\"-20%\" y=\"-30%\" width=\"140%\" height=\"160%\" \
         color-interpolation-filters=\"sRGB\">\
         <feMorphology in=\"SourceAlpha\" operator=\"dilate\" radius=\"{radius}\" \
         result=\"outline-shape\"/>\
         <feFlood flood-color=\"{OUTLINE_COLOR}\" result=\"outline-ink\"/>\
         <feComposite in=\"outline-ink\" in2=\"outline-shape\" operator=\"in\" \
         result=\"outline\"/>\
         <feMerge><feMergeNode in=\"outline\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>\
         </filter>"
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
        let upper = normalize(&word.display);
        let width_px = match &face {
            Some(face) => measure_word(face.as_ref(), fs, ls_px, &upper),
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

    // Centre-anchored block: the block's *centre* sits at 0.81H; lines
    // center horizontally.
    let line_gap = LINE_SPACING_EM * fs;
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * line_gap;
    let block_top = ANCHOR_Y_FRAC * video_h as f32 - block_h / 2.0;
    let block_w: f32 = lines
        .iter()
        .map(|&(start, end)| widths[start..end].iter().map(|w| w + 2.0 * margin).sum::<f32>())
        .fold(0.0, f32::max);
    let center_x = ANCHOR_X_FRAC * video_w as f32;
    let block_left = center_x - block_w / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let mut words_svg = format!(
        "<g font-family=\"{FONT}\" font-weight=\"{FONT_WEIGHT}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">"
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + line_gap);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, false);
        for (x, word) in xs.iter().zip(&segment.words[start..end]) {
            let text = layout::escape_xml(&normalize(&word.display));
            // Per-word outline, per the scss `.word { filter: … }`.
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{PRIMARY}\" \
                 filter=\"url(#iris-brush-outline)\">{text}</text>"
            ));
        }
    }
    words_svg.push_str("</g>");

    // Stroke first (z-index: -1 in scss), then the words on top.
    let stroke = stroke_defs(block_left, block_top, block_w, block_h, fs);
    let defs = format!("{stroke}{}", filter_defs(fs));
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 1920 1080"
            width={video_w}
            height={video_h}
        >
            <defs>
                {defs}
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
    fn embedded_righteous_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(RIGHTEOUS_400_TTF, 0).unwrap();
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
    fn stroke_mask_covers_the_bleed_box_and_tints_it_highlight() {
        // Text block 200×80 at (100, 200), fs 50 → pad 42 / 28, offset 3.
        let defs = stroke_defs(100.0, 200.0, 200.0, 80.0, 50.0);
        assert!(defs.contains("id=\"iris-stroke\""), "{defs}");
        assert!(defs.contains("mask-type=\"alpha\""), "{defs}");
        assert!(defs.contains("preserveAspectRatio=\"none\""), "{defs}");
        assert!(
            defs.contains("data:image/png;base64,iVBORw0KGgo"),
            "bundled PNG data URL, {defs:.80}"
        );
        // Bleed: x 58 (=100-42), y 175 (=200-28+3), w 284, h 136.
        assert!(defs.contains("x=\"58\" y=\"175\" width=\"284\""), "{defs}");
        assert!(defs.contains("height=\"136\""), "{defs}");
        assert!(
            defs.contains(&format!("fill=\"{STROKE_COLOR}\" mask=\"url(#iris-stroke)\"")),
            "{defs}"
        );
        assert_eq!(STROKE_COLOR, "#ffe066");
        assert_eq!(MASK_OFFSET_Y_EM, 0.06);
        // The stroke PNG is the 21624-byte marker-stroke asset.
        assert_eq!(MARKER_STROKE_B64.len(), 28832, "base64 of 21624 bytes");
    }

    #[test]
    fn brush_outline_dilates_each_word_and_rings_under_the_source() {
        let fs = 54.0; // 4.5cqh at 1200px
        let defs = filter_defs(fs);
        assert!(defs.contains("id=\"iris-brush-outline\""), "{defs}");
        assert!(
            defs.contains("x=\"-20%\" y=\"-30%\" width=\"140%\" height=\"160%\""),
            "{defs}"
        );
        assert!(
            defs.contains(&format!(
                "operator=\"dilate\" radius=\"{}\"",
                OUTLINE_THICKNESS_EM * fs
            )),
            "{defs}"
        );
        assert!(defs.contains(&format!("flood-color=\"{OUTLINE_COLOR}\"")), "{defs}");
        let outline_at = defs.find("in=\"outline\"").expect("ring merged");
        let source_at = defs.find("in=\"SourceGraphic\"").expect("source on top");
        assert!(outline_at < source_at, "ring under the source");
        assert_eq!(OUTLINE_THICKNESS_EM, 0.08);
        assert_eq!(LINE_HEIGHT, 1.4);
    }

    #[test]
    fn iris_clause_segments_split_at_ten_chars() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, hold on now and look at this one here.".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        assert!(segments.len() >= 2, "expected a char-limit split");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 4, "chunk too long: {chars}");
        }
        assert_eq!(MAX_LINES, 1, "iris is always one line");
        assert_eq!(normalize("Mixed Case"), "Mixed Case");
    }
}
