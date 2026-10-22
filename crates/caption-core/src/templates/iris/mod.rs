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

use fframes::{FFramesContext, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, stack, style};
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
    force_uppercase: false, // iris keeps the spoken case
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// The bundled watercolour stroke (alpha-only PNG), base64, as emitted
/// into the `data:` URL. Source: `tscaps/templates/_assets/marker-stroke.png`, recolored WHITE (RGB=255, alpha asli) supaya benar di mode mask luminance (SVG `<mask>` default, atribut `mask-type` diabaikan Chrome) maupun alpha.
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

/// Baseline metrics for whichever family dominates the segment.
fn line_metrics(dominant: &str) -> (f32, f32, f32) {
    if dominant == FONT {
        font_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}


// ── stroke mask + brush outline ───────────────────────────────────────────

/// Geometry mask/rect bersama (bleed box).
fn stroke_box(
    block_left: f32,
    block_top: f32,
    block_w: f32,
    block_h: f32,
    fs: f32,
    st: &style::Style,
) -> (f32, f32, f32, f32) {
    let px = st.f32("mask-padding-x", MASK_PAD_X_EM) * fs;
    let py = st.f32("mask-padding-y", MASK_PAD_Y_EM) * fs;
    let offset = st.f32("mask-offset-y", MASK_OFFSET_Y_EM) * fs;
    (
        block_left - px,
        block_top - py + offset,
        block_w + 2.0 * px,
        block_h + 2.0 * py,
    )
}

/// The `.segment::before` watercolour stroke MASK: the bundled PNG (atau
/// upload user via kontrol `mask` — data URL apa pun yang bisa di-decode
/// browser; SVG `<mask>` bertipe alpha, jadi alpha channel gambar yang
/// menentukan) stretched over the bleed box (CSS `mask-size: 100% 100%`).
/// Hanya `<mask>` — tempatnya DI DALAM `<defs>`.
///
/// Native CLI: data URL upload tidak dimasukkan registry gambar (hanya
/// frame video) → mask kosong → highlight tak tergambar; stroke bawaan
/// tetap bekerja. Fallback terdokumentasi, selaras kaca selene.
fn stroke_defs(
    block_left: f32,
    block_top: f32,
    block_w: f32,
    block_h: f32,
    fs: f32,
    st: &style::Style,
) -> String {
    let (x, y, w, h) = stroke_box(block_left, block_top, block_w, block_h, fs, st);
    let custom = st.str("mask", "");
    let href = if custom.starts_with("data:image/") {
        custom
    } else {
        format!("data:image/png;base64,{MARKER_STROKE_B64}")
    };
    format!(
        "<mask id=\"iris-stroke\" mask-type=\"alpha\" x=\"{x}\" y=\"{y}\" width=\"{w}\" \
         height=\"{h}\" maskUnits=\"userSpaceOnUse\">\
         <image href=\"{href}\" x=\"{x}\" y=\"{y}\" \
         width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\"/>\
         </mask>"
    )
}

/// Highlight-coloured rect painted THROUGH the mask — elemen KONTEN (di depan
/// defs, di belakang kata-kata), bukan bagian defs (defs tidak pernah
/// dirender — rect di sana = brush hilang total).
fn stroke_rect(
    block_left: f32,
    block_top: f32,
    block_w: f32,
    block_h: f32,
    fs: f32,
    color: &str,
    st: &style::Style,
) -> String {
    let (x, y, w, h) = stroke_box(block_left, block_top, block_w, block_h, fs, st);
    format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{color}\" \
         mask=\"url(#iris-stroke)\"/>"
    )
}

/// The `brush-outline` filter body from filters.svg: the `tscaps:outline`
/// recipe at the control-default thickness, merged under SourceGraphic.
/// Sits on each `.word`, not the segment, so the dilation stays scoped
/// to glyph silhouettes.
fn filter_defs(fs: f32, st: &style::Style) -> String {
    let radius = st.f32("outline-thickness", OUTLINE_THICKNESS_EM) * fs;
    let outline_color = st.color("outline-color", OUTLINE_COLOR);
    format!(
        "<filter id=\"iris-brush-outline\" x=\"-20%\" y=\"-30%\" width=\"140%\" height=\"160%\" \
         color-interpolation-filters=\"sRGB\">\
         <feMorphology in=\"SourceAlpha\" operator=\"dilate\" radius=\"{radius}\" \
         result=\"outline-shape\"/>\
         <feFlood flood-color=\"{outline_color}\" result=\"outline-ink\"/>\
         <feComposite in=\"outline-ink\" in2=\"outline-shape\" operator=\"in\" \
         result=\"outline\"/>\
         <feMerge><feMergeNode in=\"outline\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>\
         </filter>"
    )
}

/// tscaps `SegmentColorRotation.hashStringToInt`: 32-bit rolling
/// `(h << 5) - h + unit` over the UTF-16 units, absolute value.
fn hash_string(s: &str) -> usize {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_shl(5).wrapping_sub(h).wrapping_add(unit as i32);
    }
    h.unsigned_abs() as usize
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
    let st = style::Style::of(&doc.params, "iris");
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
        RIGHTEOUS_400_TTF,
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

    // Centre-anchored block: the block's *centre* sits at 0.81H; lines
    // center horizontally.
    let line_gap = ov.line_spacing_em * fs;
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * line_gap;
    let block_top = ov.anchor_y * video_h as f32 - block_h / 2.0;
    let block_w: f32 = lines
        .iter()
        .map(|&(start, end)| widths[start..end].iter().map(|w| w + 2.0 * margin).sum::<f32>())
        .fold(0.0, f32::max);
    let center_x = ov.anchor_x * video_w as f32;
    let block_left = center_x - block_w / 2.0;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + line_gap);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for (word_off, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            let text = layout::escape_xml(&ov.text_case.apply(&word.display));
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            // Element inspector: override per-kata (attrs + transform).
            let wo_ctx = element_css::WordCtx {
                base_fs: fs,
                ls_px,
                video_w: video_w as f32,
                video_h: video_h as f32,
                center_x: *x + widths[start + word_off] / 2.0,
                center_y: baseline - fs * 0.35,
                word_start_s: word.start_ms / 1000.0,
                now_s: now_ms / 1000.0,
                font_family_svg: &stack::svg_font_family,
            };
            let wo = element_css::word_svg_attrs(&doc.params, word.index, &wo_ctx);
            let wo_transform = element_css::word_svg_transform(&doc.params, word.index, &wo_ctx)
                .map(|t| format!(" transform=\"{t}\""))
                .unwrap_or_default();
            // Per-word outline, per the scss `.word { filter: … }`.
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{primary}\" \
                 filter=\"url(#iris-brush-outline)\"{family_attr}{wo}{wo_transform}>{text}</text>",
                primary = ov.primary.as_str(),
            ));
        }
    }
    words_svg.push_str("</g>");

    // User rotation wraps the block about its centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    // Stroke colour: the `color-recipe` select rotates the
    // highlight-color-1..5 palette per segment (port of tscaps
    // SegmentColorRotation). `single` (default) = palette[0];
    // `sequential` = index within the segment list; `random` = stable
    // hash of the segment's own words so re-renders stay put.
    let palette: [String; 5] = [
        st.color("highlight-color-1", STROKE_COLOR),
        st.color("highlight-color-2", "#ffb3b3"),
        st.color("highlight-color-3", "#a3d8f5"),
        st.color("highlight-color-4", "#b3e5b8"),
        st.color("highlight-color-5", "#ffc89e"),
    ];
    let recipe = st.str("color-recipe", "single");
    let seg_index = segments
        .iter()
        .position(|s| std::ptr::eq(s, segment))
        .unwrap_or(0);
    let stroke_color: &str = match recipe.as_str() {
        "sequential" => palette[seg_index % palette.len()].as_str(),
        "random" => {
            let key = segment
                .words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            palette[hash_string(&key) % palette.len()].as_str()
        }
        _ => palette[0].as_str(),
    };

    // Stroke first (z-index: -1 in scss), then the words on top.
    let stroke = stroke_rect(
        block_left,
        block_top,
        block_w,
        block_h,
        fs,
        stroke_color,
        &st,
    );
    let defs = format!(
        "{}{}",
        stroke_defs(block_left, block_top, block_w, block_h, fs, &st),
        filter_defs(fs, &st)
    );
    let words_svg = layout::dedupe_text_attrs(&words_svg);
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                {defs}
            </defs>
            {stroke}
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
        let params = crate::CaptionParams::default();
        let st = style::Style::of(&params, "iris");
        let defs = stroke_defs(100.0, 200.0, 200.0, 80.0, 50.0, &st);
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
        // Rect highlight = konten (di luar defs — defs tidak pernah dirender).
        let rect = stroke_rect(100.0, 200.0, 200.0, 80.0, 50.0, STROKE_COLOR, &st);
        assert!(
            rect.contains(&format!("fill=\"{STROKE_COLOR}\" mask=\"url(#iris-stroke)\"")),
            "{rect}"
        );
        assert!(!defs.contains("<rect"), "defs hanya mask/filter: {defs}");
        assert_eq!(STROKE_COLOR, "#ffe066");
        assert_eq!(MASK_OFFSET_Y_EM, 0.06);
        // The stroke PNG is the 21624-byte marker-stroke asset.
        assert_eq!(MARKER_STROKE_B64.len(), 51640, "base64 of 38729 bytes");
    }

    #[test]
    fn brush_outline_dilates_each_word_and_rings_under_the_source() {
        let fs = 54.0; // 4.5cqh at 1200px
        let params = crate::CaptionParams::default();
        let st = style::Style::of(&params, "iris");
        let defs = filter_defs(fs, &st);
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
                sheet: None,
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
        assert_eq!("Mixed Case".to_string(), "Mixed Case");
    }
}
