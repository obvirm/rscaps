//! Native port of the tscaps `noor` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\noor\{style.scss,template.json}`.
//! Huge uppercase Anton at 6.2cqh filled by a three-stop vertical gold
//! gradient (`background-clip: text`), held on shot by a four-way 0.015em
//! black outline, a soft cast shadow (`0 0.08em 0.5em`) and — last in the
//! chain on purpose — a warm halo in the fill colour (`glow-filter(0.12)`).
//! The segment settles in (0.3s from 80% opacity, entering 0.05em from
//! above) and grows for short captions (`dynamic-font-size` 0.08 under a
//! twelve-character threshold). Clause-boundary segments with `gap_free`,
//! `carry_quotes` and `smart_punctuation`; block bottom-anchored at 0.82H.
//!
//! Unported: the `.emphasis/.accent/.entity` and `.quote` gradients key
//! off word *tags* (`semanticTags`), which the native `Cue` text does not
//! carry — only the plain `.word` gradient renders.
//!
//! Family note: Anton ships one static face (400) while the spec asks for
//! weight 900 — the browser resolves to 400 too, so we query what we embed.

use fframes::{FFramesContext, Svgr};

use super::{effects, layout, overrides, pipeline, recipes, stack, style};
use crate::CaptionDocument;

// ── spec constants (from noor's style.scss + template.json) ───────────────
const FONT: &str = "Anton"; // fontStack anton / segment-typography $font-family
// template.json + scss ask weight 900; Anton ships a single static face
// (400) and the browser resolves to it too — we query what we embed.
const FONT_WEIGHT: u16 = 400;
const FONT_SIZE_CQH: f32 = 0.062; // font-size: 6.2cqh (typography.fontSize)
const LETTER_SPACING_EM: f32 = 0.02; // letter-spacing: 0.02em
const LINE_HEIGHT: f32 = 1.05; // line-height: 1.05
const WORD_SPACING_EM: f32 = 0.1; // .word margin: 0 0.1em
const ANCHOR_Y_FRAC: f32 = 0.82; // alignment.verticalOffset (bottom)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 15; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 5; // boundary_score_limit_by_chars.minChars
const MIN_LINES: usize = 1; // lineSplitter.minLines
const MAX_LINES: usize = 1; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.77; // lineSplitter.maxWidthRatio
const DYN_THRESHOLD: f32 = 12.0; // dynamic-font-size toggle `valueOn` ('12')
const DYN_STRENGTH: f32 = 0.08; // dynamic-font-size(0.08)
// settle-in($element: 'segment', $distance: -0.05em, $from-opacity: 0.8,
// $delay: var(--on-segment-starts), $duration: 0.3s,
// $easing: cubic-bezier(0.1, 0.55, 0.2, 1))
const SETTLE_DISTANCE_EM: f32 = -0.05;
const SETTLE_FROM_OPACITY: f32 = 0.8;
const SETTLE_DURATION_S: f32 = 0.3;
const PRIMARY: &str = "#ffd84d"; // primary-color default
// -webkit-text-stroke is replaced by the four `drop-shadow` outline passes
// below: `--tscaps-outline-thickness` / `--tscaps-outline-color` defaults.
const OUTLINE_THICKNESS_EM: f32 = 0.015;
const OUTLINE_COLOR: &str = "#000000";
// filter: … drop-shadow(soft-drop-shadow(0.08, 0.5)) — 3rd length of a CSS
// `drop-shadow()` *is* the Gaussian σ (filter-effects-1 §drop-shadow).
const SHADOW_DY_EM: f32 = 0.08;
const SHADOW_SIGMA_EM: f32 = 0.5;
// glow-filter(0.12): halves the radius for `drop-shadow`, which is why the
// halo ends up at the same σ the `glow(0.12)` text-shadow would give. The
// halo's ink is `var(--tscaps-primary-color, #ffd84d)` → the effective
// primary (wired via `ov.primary` in the filter below).
const GLOW_SIGMA_EM: f32 = 0.12 / 2.0;
// .word background: 180deg, black-mixed stops at 0% / 100% around the fill
// at 25% — `color-mix(in srgb, $primary, #000 N%)`.
const GRADIENT_TOP_SHARE: f32 = 0.28;
const GRADIENT_BOTTOM_SHARE: f32 = 0.48;

/// The template's spec — handed to `overrides::resolve` every frame so
/// user params win over these constants when set.
const DEFAULTS: overrides::TemplateDefaults = overrides::TemplateDefaults {
    font_size_cqh: FONT_SIZE_CQH,
    letter_spacing_em: LETTER_SPACING_EM,
    word_spacing_em: WORD_SPACING_EM,
    line_spacing_em: 0.0, // no LINE_SPACING_EM in noor's spec
    anchor_x: ANCHOR_X_FRAC,
    anchor_y: ANCHOR_Y_FRAC,
    font: FONT,
    font_weight: FONT_WEIGHT,
    primary: PRIMARY,
    highlight: "",
    force_uppercase: true, // noor always uppercases
    min_lines: MIN_LINES,
    max_lines: MAX_LINES,
    max_width_ratio: MAX_WIDTH_RATIO,
};

/// noor's `effects` array: `gap_free`, `carry_quotes`, `smart_punctuation`.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Clause,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[
        effects::Effect::GapFree,
        effects::Effect::CarryQuotes,
        effects::Effect::SmartPunctuation,
    ],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const ANTON_TTF: &[u8] = include_bytes!("../../../assets/anton-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// Anton.
fn font_metrics() -> (f32, f32, f32) {
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

/// Baseline metrics for whichever family dominates the segment.
fn line_metrics(dominant: &str) -> (f32, f32, f32) {
    if dominant == FONT {
        font_metrics()
    } else {
        stack::script_metrics(dominant).expect("script face")
    }
}

// ── dynamic font size ─────────────────────────────────────────────────────

/// `--segment-char-count`: the segment's raw texts joined by single
/// spaces, measured in UTF-16 units (JS `Segment.getText().length`).
fn segment_char_count(words: &[pipeline::TimedWord]) -> usize {
    let utf16: usize = words.iter().map(|w| w.text.encode_utf16().count()).sum();
    utf16 + words.len().saturating_sub(1)
}

/// `max(1, 1 + (threshold - char_count) * strength)` — the
/// `dynamic-font-size(0.08)` toggle.
fn font_size_scale(char_count: usize) -> f32 {
    (1.0 + (DYN_THRESHOLD - char_count as f32) * DYN_STRENGTH).max(1.0)
}

// ── gradient fill ─────────────────────────────────────────────────────────

/// `color-mix(in srgb, <hex>, #000 <share*100>%)` — channel × (1 − share).
fn mixed_down(hex: &str, share: f32) -> String {
    let parse = |s: &str| -> [f32; 3] {
        let h = s.trim_start_matches('#');
        [0, 2, 4]
            .iter()
            .map(|i| u8::from_str_radix(&h[*i..*i + 2], 16).unwrap_or(255) as f32)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    };
    let [r, g, b] = parse(hex);
    let m = 1.0 - share;
    format!(
        "#{:02x}{:02x}{:02x}",
        (r * m).round().clamp(0.0, 255.0) as u8,
        (g * m).round().clamp(0.0, 255.0) as u8,
        (b * m).round().clamp(0.0, 255.0) as u8
    )
}

/// `cubic-bezier(0.1, 0.55, 0.2, 1)` — noor's settle easing.
fn ease_noor(x: f32) -> f32 {
    recipes::cubic_bezier(0.1, 0.55, 0.2, 1.0)(x)
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
    let st = style::Style::of(&doc.params, "noor");
    let quote_color = st.color("quote-color", "#9ce874");
    let outline_color = st.color("outline-color", OUTLINE_COLOR);
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32)
        * font_size_scale(segment_char_count(&segment.words));
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
        ANTON_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        true,
    );
    let widths = measured.widths;
    // text-transform: uppercase — measured and emitted as displayed.
    let display_texts: Vec<String> = segment
        .words
        .iter()
        .map(|w| ov.text_case.apply(&w.display))
        .collect();

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Bottom-aligned block: the block's bottom edge sits at 0.82H.
    let block_h = lines.len() as f32 * line_h;
    let block_top = ov.anchor_y * video_h as f32 - block_h;
    let center_x = ov.anchor_x * video_w as f32;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // The segment settles in as a whole: `--on-segment-starts`.
    let settle = recipes::SettleIn {
        distance_em: ov.distance_or(SETTLE_DISTANCE_EM),
        from_opacity: SETTLE_FROM_OPACITY,
        duration_s: ov.duration_or(SETTLE_DURATION_S),
        ease: ease_noor,
    };
    let (opacity, ty_em) = if ov.motion_static {
        (1.0, 0.0)
    } else {
        settle.eval((now_ms - segment.start_ms) / 1000.0)
    };
    let ty = ty_em * fs;

    let outline = st.f32("outline-thickness", OUTLINE_THICKNESS_EM) * fs;
    let shadow_dy = SHADOW_DY_EM * fs;
    let shadow_sigma = SHADOW_SIGMA_EM * fs;
    let glow_sigma = GLOW_SIGMA_EM * fs;
    // The fill gradient and the halo key off the effective primary
    // (style control → user color → template default).
    let primary = ov.primary.clone();
    let grad_top = mixed_down(&primary, GRADIENT_TOP_SHARE);
    let grad_bottom = mixed_down(&primary, GRADIENT_BOTTOM_SHARE);

    // One gradient per line: every word in a line shares its box top and
    // height, so `.word`'s own 180° background lands on the same stops.
    let mut defs_svg = String::new();
    for (line_idx, _) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let grad_id = format!("noor-grad-{line_idx}");
        defs_svg.push_str(&format!(
            "<linearGradient id=\"{grad_id}\" gradientUnits=\"userSpaceOnUse\" \
             x1=\"0\" y1=\"{line_top}\" x2=\"0\" y2=\"{}\">\
             <stop offset=\"0\" stop-color=\"{grad_top}\"/>\
             <stop offset=\"0.25\" stop-color=\"{primary}\"/>\
             <stop offset=\"1\" stop-color=\"{grad_bottom}\"/>\
             </linearGradient>",
            line_top + line_h
        ));
    }

    let mut words_svg = format!(
        "<g transform=\"translate(0,{ty})\" opacity=\"{opacity}\">\
         <g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\" filter=\"url(#noor-fx)\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = block_top + line_idx as f32 * line_h;
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = widths[start..end].iter().map(|w| w + 2.0 * margin).sum();
        let line_left = center_x - line_w / 2.0;
        let xs = layout::word_x_positions(&widths[start..end], margin, line_left, line_w, rtl);
        let grad_id = format!("noor-grad-{line_idx}");
        for (((x, word), text), family) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&display_texts[start..end])
            .zip(&measured.families[start..end])
        {
            let text = layout::escape_xml(text);
            let family_attr = if *family == FONT {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            // `.quote` words take the whole-word quote colour instead of
            // the gold gradient (semantik tag quote tscaps).
            let fill = if style::word_has_quote(&word.display) {
                quote_color.clone()
            } else {
                format!("url(#{grad_id})")
            };
            words_svg.push_str(&format!(
                "<text x=\"{x}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr}>{text}</text>"
            ));
        }
    }
    words_svg.push_str("</g></g>");

    // User rotation wraps the caption around the block centre.
    if ov.angle_deg.abs() > f32::EPSILON {
        let cx = center_x;
        let cy = block_top + block_h / 2.0;
        words_svg = format!(
            "<g transform=\"rotate({angle},{cx},{cy})\">{inner}</g>",
            angle = ov.angle_deg,
            inner = words_svg,
        );
    }

    // The full chain, in scss order: four outline passes, the soft cast
    // shadow, then the halo — each function feeds the next.
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>
                <filter id="noor-fx" x="-50%" y="-120%" width="200%" height="340%">
                    <feDropShadow dx={outline} dy={0.0} stdDeviation={0.0} flood-color={outline_color.as_str()} flood-opacity={1.0} result="p1"/>
                    <feDropShadow in="p1" dx={-outline} dy={0.0} stdDeviation={0.0} flood-color={outline_color.as_str()} flood-opacity={1.0} result="p2"/>
                    <feDropShadow in="p2" dx={0.0} dy={outline} stdDeviation={0.0} flood-color={outline_color.as_str()} flood-opacity={1.0} result="p3"/>
                    <feDropShadow in="p3" dx={0.0} dy={-outline} stdDeviation={0.0} flood-color={outline_color.as_str()} flood-opacity={1.0} result="p4"/>
                    <feDropShadow in="p4" dx={0.0} dy={shadow_dy} stdDeviation={shadow_sigma} flood-color="#000000" flood-opacity={1.0} result="p5"/>
                    <feDropShadow in="p5" dx={0.0} dy={0.0} stdDeviation={glow_sigma} flood-color={primary.as_str()} flood-opacity={1.0}/>
                </filter>
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

    #[test]
    fn embedded_anton_has_expected_identity() {
        let face = fframes::ttf_parser::Face::parse(ANTON_TTF, 0).unwrap();
        let mut name = None;
        for n in face.names() {
            if n.name_id == 1 {
                name = n.to_string();
                break;
            }
        }
        assert_eq!(name.as_deref(), Some("Anton"));
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
    }

    #[test]
    fn gold_gradient_stops_mix_black_at_the_documented_shares() {
        assert_eq!(mixed_down(PRIMARY, GRADIENT_TOP_SHARE), "#b89c37");
        assert_eq!(mixed_down(PRIMARY, GRADIENT_BOTTOM_SHARE), "#857028");
        // The 25% stop is the untouched fill.
        assert_eq!(PRIMARY, "#ffd84d");
    }

    #[test]
    fn short_segments_grow_at_eight_percent_per_missing_character() {
        assert_eq!(font_size_scale(12), 1.0);
        assert!((font_size_scale(5) - 1.56).abs() < 1e-6);
    }

    #[test]
    fn glow_halves_the_radius_for_drop_shadow() {
        // glow-filter(0.12) → drop-shadow σ 0.06, the σ a `glow(0.12)`
        // text-shadow would produce.
        assert_eq!(GLOW_SIGMA_EM, 0.06);
        assert_eq!(SHADOW_SIGMA_EM, 0.5);
        assert_eq!(SHADOW_DY_EM, 0.08);
    }

    #[test]
    fn noor_clause_segments_split_and_smartify() {
        let doc = CaptionDocument {
            duration_frames: 3000,
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: "Wait, she said \"go now\"...".into(),
            }],
            ..Default::default()
        };
        let segments = pipeline::collect_segments(&doc, &SPLIT);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| w.display.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        // Clause boundary at the comma; the long second clause then splits
        // on the 15-char limit, and smart_punctuation turns the ellipsis
        // into … (and the surrounding quotes into typographic forms).
        assert_eq!(
            texts,
            vec![
                "Wait,".to_string(),
                "she said \u{201C}go".to_string(),
                "now\u{201D}\u{2026}".to_string(),
            ]
        );
    }
}
