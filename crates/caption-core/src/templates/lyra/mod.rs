//! Native port of the tscaps `lyra` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\lyra\{style.scss,template.json}`.
//! Bricolage Grotesque 800 at 6.0cqh filled with a four-hue `100deg`
//! gradient clipped to the glyphs over a 400%-wide tile, swept on a loop
//! by `shimmer` (6s ÷ speed) so the foil travels across the block, with
//! two soft black drop-shadows. Sentence segments of 14 chars (min 6),
//! a single row of 0.72W, block bottom-anchored at 0.82H.

use fframes::{FFramesContext, Svgr};

use super::{layout, overrides, pipeline, recipes, stack};
use crate::CaptionDocument;

// ── spec constants (from lyra's style.scss + template.json) ───────────────
const FONT: &str = "Bricolage Grotesque"; // fontStack bricolage-grotesque
const FONT_WEIGHT: u16 = 800; // fontWeight: 800
const FONT_SIZE_CQH: f32 = 0.06; // font-size: 6.0cqh
const LETTER_SPACING_EM: f32 = -0.015; // letter-spacing: -0.015em
const LINE_HEIGHT: f32 = 1.2; // segment-typography default line-height
const WORD_SPACING_EM: f32 = 0.1; // @include word-layout(0.1em)
const LINE_SPACING_EM: f32 = 0.0; // .line + .line margin-top: 0
const ANCHOR_Y_FRAC: f32 = 0.82; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // horizontalAlign default (center)
const MAX_CHARS: usize = 14; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 6; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // balanced-pixel-width default minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width default ratio
// background-image: linear-gradient(100deg, c1, c2, c3, c4, c1)
const GRADIENT_ANGLE_DEG: f32 = 100.0; // --tscaps-gradient-angle default
const GRADIENT_COLORS: [&str; 5] = ["#ff6ec4", "#4dd4ff", "#b6f56d", "#ffc857", "#ff6ec4"];
const BACKGROUND_SIZE_W: f32 = 4.0; // background-size: 400% 100%
// filter: drop-shadow(0 0.04em 0.08em rgba(0,0,0,.55)) then
// drop-shadow(0 0 0.22em rgba(0,0,0,.35)) (saturate(1) is a no-op).
const SHADE1_DY_EM: f32 = 0.04;
const SHADE1_BLUR_EM: f32 = 0.08;
const SHADE1_OPACITY: f32 = 0.55;
const SHADE2_BLUR_EM: f32 = 0.22;
const SHADE2_OPACITY: f32 = 0.35;
// @include shimmer($travel: 400%, $duration: calc(6s / shimmer-speed))
const SHIMMER_DURATION_S: f32 = 6.0;
const SHIMMER_SPEED: f32 = 1.0; // --tscaps-shimmer-speed default

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
    primary: "#ffffff", // gradient fill; the primary-color default stands in
    highlight: "",
    force_uppercase: false, // lyra keeps the spoken case
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// lyra declares no `effects` array — windows stay the raw word span.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const BRICOLAGE_TTF: &[u8] = include_bytes!("../../../assets/bricolage-grotesque-latin-800-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Bricolage Grotesque.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face =
            fframes::ttf_parser::Face::parse(BRICOLAGE_TTF, 0).expect("Bricolage parses");
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

// ── gradient geometry ─────────────────────────────────────────────────────

/// Left edge of the 400%-wide background box once `shimmer` has slid it.
///
/// `background-position-x: 0% → 400%` moves a box of width `4W` (its own
/// 100% is `W − 4W = −3W`) from `0` to `−12W` per cycle, so the offset is
/// `−12W·phase`, wrapped into `(-4W, 0]` — one full tile, which is what
/// `background-repeat` would draw under that window anyway.
fn gradient_box_left(video_w: f32, phase: f32) -> f32 {
    let tile = BACKGROUND_SIZE_W * video_w;
    let left = (-3.0 * video_w * BACKGROUND_SIZE_W) * phase % tile;
    if left == 0.0 { 0.0 } else { left }
}

/// Endpoints of the gradient line covering the tile box, projected along
/// the CSS angle (0deg = up, clockwise) onto the box's corner projections.
fn gradient_line(
    box_left: f32,
    box_top: f32,
    box_w: f32,
    box_h: f32,
    angle_deg: f32,
) -> ((f32, f32), (f32, f32)) {
    let angle = angle_deg.to_radians();
    let (dx, dy) = (angle.sin(), -angle.cos());
    let corners = [
        (box_left, box_top),
        (box_left + box_w, box_top),
        (box_left + box_w, box_top + box_h),
        (box_left, box_top + box_h),
    ];
    let (mut t_min, mut t_max) = (f32::INFINITY, f32::NEG_INFINITY);
    for (x, y) in corners {
        let t = x * dx + y * dy;
        t_min = t_min.min(t);
        t_max = t_max.max(t);
    }
    ((t_min * dx, t_min * dy), (t_max * dx, t_max * dy))
}

/// Phase of the shimmer loop: the mixin's delay is the segment start, so
/// the sweep holds at 0 until then (`fill: both`).
fn shimmer_phase(local_s: f32) -> f32 {
    if local_s <= 0.0 {
        return 0.0;
    }
    recipes::Shimmer {
        duration_s: SHIMMER_DURATION_S / SHIMMER_SPEED,
    }
    .eval(local_s)
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
    let script_stack = stack::Stack::for_latin(FONT);
    let (asc_ratio, desc_ratio, _) =
        line_metrics(stack::dominant_family(&segment.words, &script_stack));
    let rtl = segment.direction.is_rtl();

    let measured = stack::measure_words(
        &segment.words,
        &script_stack,
        BRICOLAGE_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let lines = splitter.split(&split_widths);

    // Bottom-anchored block: the block's bottom edge sits at 0.82H; lines
    // center horizontally. The gradient box is the segment's own box —
    // full video width × the block's height.
    let block_h = lines.len() as f32 * line_h + lines.len().saturating_sub(1) as f32 * ov.line_spacing_em * fs;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Shimmer: the sweep runs on the segment's own clock.
    let phase = shimmer_phase((now_ms - segment.start_ms) / 1000.0);
    let box_left = gradient_box_left(video_w as f32, phase);
    let ((gx1, gy1), (gx2, gy2)) = gradient_line(
        box_left,
        block_top,
        BACKGROUND_SIZE_W * video_w as f32,
        block_h,
        GRADIENT_ANGLE_DEG,
    );
    let stops: String = GRADIENT_COLORS
        .iter()
        .enumerate()
        .map(|(i, color)| {
            let offset = i as f32 / (GRADIENT_COLORS.len() - 1) as f32;
            format!("<stop offset=\"{offset}\" stop-color=\"{color}\"/>")
        })
        .collect();
    let gradient_svg = format!(
        "<linearGradient id=\"lyra-grad\" gradientUnits=\"userSpaceOnUse\" \
         spreadMethod=\"repeat\" x1=\"{gx1}\" y1=\"{gy1}\" x2=\"{gx2}\" y2=\"{gy2}\">{stops}</linearGradient>"
    );

    let shade1_dy = SHADE1_DY_EM * fs;
    let shade1_std = SHADE1_BLUR_EM * fs / 2.0;
    let shade2_std = SHADE2_BLUR_EM * fs / 2.0;

    let font_family_attr = stack::svg_font_family(&ov.font_family);
    let mut words_svg = format!(
        "<g filter=\"url(#lyra-shade)\" font-family=\"{font_family_attr}\" \
         font-weight=\"{weight}\" font-size=\"{fs}\" letter-spacing=\"{ls_px}\">",
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * (line_h + ov.line_spacing_em * fs);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        for ((x, word), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(&word.display);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{}\"", stack::svg_font_family(&family))
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"url(#lyra-grad)\"{family_attr}>{text}</text>"
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

    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                {gradient_svg}
                <filter id="lyra-shade" x="-30%" y="-30%" width="160%" height="170%">
                    <feDropShadow dx={0.0} dy={shade1_dy} stdDeviation={shade1_std} flood-color="#000000" flood-opacity={SHADE1_OPACITY} result="s1"/>
                    <feDropShadow in="s1" dx={0.0} dy={0.0} stdDeviation={shade2_std} flood-color="#000000" flood-opacity={SHADE2_OPACITY}/>
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
    fn embedded_bricolage_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(BRICOLAGE_TTF, 0).unwrap();
        // fontdb's rule: typographic family (name ID 16) when present,
        // else the legacy family (ID 1) — this is what `resolve_font`
        // matches for lyra.
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
        // The fontsource face's name-table family (ID 16 or legacy 1).
        assert_eq!(name.as_deref(), Some("Bricolage Grotesque 96pt ExtraBold"));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        assert_eq!(FONT_WEIGHT, 800);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn lyra_splitter_is_sentence_plus_fourteen_chars() {
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
        assert!(segments.len() >= 2, "{segments:?}");
        for s in &segments {
            let chars: usize = s.words.iter().map(|w| w.text.encode_utf16().count()).sum();
            assert!(chars <= MAX_CHARS + 6, "chunk too long: {chars}");
        }
        assert_eq!(MAX_LINES, 1);
    }

    #[test]
    fn shimmer_slides_the_background_box_by_full_tiles() {
        let w = 100.0;
        // A cycle walks 0 → −12W and wraps every 4W, so the window shows
        // the same tiling CSS would.
        assert_eq!(gradient_box_left(w, 0.0), 0.0);
        assert_eq!(gradient_box_left(w, 0.25), -300.0);
        assert_eq!(gradient_box_left(w, 0.5), -200.0);
        assert_eq!(gradient_box_left(w, 0.75), -100.0);
        assert_eq!(gradient_box_left(w, 1.0), 0.0);
        assert!(gradient_box_left(w, 0.33) < 0.0);
        assert!(gradient_box_left(w, 0.33) > -400.0);
        // The loop holds before the segment starts.
        assert_eq!(shimmer_phase(0.0), 0.0);
        assert_eq!(shimmer_phase(-2.0), 0.0);
        assert_eq!(shimmer_phase(6.0), 0.0);
        assert!((shimmer_phase(3.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn gradient_line_runs_along_the_declared_angle() {
        let ((x1, y1), (x2, y2)) = gradient_line(0.0, 0.0, 400.0, 100.0, GRADIENT_ANGLE_DEG);
        let (dx, dy) = (GRADIENT_ANGLE_DEG.to_radians().sin(), -GRADIENT_ANGLE_DEG.to_radians().cos());
        // Both endpoints sit on the axis through the origin, and the
        // segment points along (sin θ, −cos θ) — down-right at 100deg.
        assert!((x1 - 0.0).abs() < 1e-4 && (y1 - 0.0).abs() < 1e-4, "start {x1} {y1}");
        assert!(x2 > 300.0 && y2 > 0.0, "end {x2} {y2}");
        let (ex, ey) = (x2 - x1, y2 - y1);
        let (kx, ky) = (ex / dx, ey / dy);
        assert!((kx - ky).abs() < 1e-3, "axis {kx} vs {ky}");
        assert_eq!(GRADIENT_COLORS.len(), 5);
        assert_eq!(SHIMMER_SPEED, 1.0);
    }
}
