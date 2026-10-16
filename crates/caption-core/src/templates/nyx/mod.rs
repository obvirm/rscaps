//! Native port of the tscaps `nyx` template.
//!
//! Visual spec source of truth: `E:\project\tscaps\templates\nyx\{style.scss,template.json}`.
//! Phosphor terminal: VT323 at 3.44cqh in `#00ff41` types in letter by
//! letter inside a floating dark window (`#0d1117`, radius 0.227 ×
//! chrome-unit) with a Mac title bar (`bash`, three traffic lights) and a
//! two-layer box shadow. Every letter glows (glow 0.18 → feDropShadow
//! σ = 0.09em) and a glowing block caret rides only the currently-typing
//! letter's slot (`step-start`, no blink), suppressed on the last letter
//! of each line's last word. Sentence segments of 36 chars (min 0),
//! `gap_free`, up to two rows of 0.72W; the window hugs the widest line
//! (min-width 25cqh, border-box) and sits top-anchored at 0.7H with
//! left-aligned lines.
//!
//! Unported: the `--tscaps-rotation` / `--tscaps-fixed-width` /
//! `--tscaps-chrome-scale` knobs (defaults `0deg` / `0cqw` / `1` make
//! the default render identical) and the typewriter's whole-word fallback
//! for joining scripts (not reachable for this template's latin path).

use fframes::{FFramesContext, FontQuery, Svgr};

use super::{effects, element_css, layout, overrides, pipeline, stack, style};
use crate::CaptionDocument;

// ── spec constants (from nyx's style.scss + template.json) ────────────────
const FONT: &str = "VT323"; // fontStack vt323
const FONT_WEIGHT: u16 = 400; // fontWeight: 400
const FONT_TITLE: &str = "Inter"; // ::before font-family: -apple-system, 'Inter'
const FONT_TITLE_WEIGHT: u16 = 400; // inherited font-weight (mixin 400)
const FONT_SIZE_CQH: f32 = 0.0344; // font-size: 3.44cqh
const LETTER_SPACING_EM: f32 = 0.0; // letterSpacing: 0
const LINE_HEIGHT: f32 = 1.15; // segment-typography line-height: 1.15
const WORD_SPACING_EM: f32 = 0.14; // .word { word-layout(0.14em) }
const LINE_SPACING_EM: f32 = 0.045; // .line + .line margin-top: 0.045em
const ANCHOR_Y_FRAC: f32 = 0.7; // alignment.verticalOffset (top)
const ANCHOR_X_FRAC: f32 = 0.5; // alignment.horizontalOffset (center)
const MAX_CHARS: usize = 36; // boundary_score_limit_by_chars.maxChars
const MIN_CHARS: usize = 0; // boundary_score_limit_by_chars minChars default 0
const MIN_LINES: usize = 1; // balanced-pixel-width minLines default
const MAX_LINES: usize = 2; // lineSplitter.maxLines
const MAX_WIDTH_RATIO: f32 = 0.72; // balanced-pixel-width maxWidthRatio default
const MIN_WIDTH_CQH: f32 = 0.25; // min-width: max(25cqh, fixed-width 0cqw)
const FIXED_WIDTH_CQW: f32 = 0.722; // fixed-width toggle valueOn: 72.2cqw
// --chrome-unit = font-size × chrome-scale(1) → one chrome unit is `fs`.
const WIN_BG: &str = "#0d1117"; // --tscaps-window-bg default
const WIN_RADIUS_C: f32 = 0.227; // border-radius: 0.227 × chrome-unit
const PAD_TOP_C: f32 = 0.82; // padding-top: 0.82 × chrome-unit
const PAD_X_EM: f32 = 0.5; // padding sides: 0.5em
const PAD_BOTTOM_EM: f32 = 0.41; // padding-bottom: 0.41em
// box-shadow: 0 0.32em 0.91em rgba(0,0,0,.55), 0 0.045em 0.14em rgba(0,0,0,.4)
const SHADOW1_DY_EM: f32 = 0.32;
const SHADOW1_BLUR_EM: f32 = 0.91;
const SHADOW1_ALPHA: f32 = 0.55;
const SHADOW2_DY_EM: f32 = 0.045;
const SHADOW2_BLUR_EM: f32 = 0.14;
const SHADOW2_ALPHA: f32 = 0.4;
const TITLE_TEXT: &str = "bash"; // --tscaps-title default
const TITLEBAR_H_C: f32 = 0.68; // ::before height: 0.68 × chrome-unit
const TITLE_FONT_SIZE_C: f32 = 0.273; // ::before font-size
const TITLE_LS_EM: f32 = 0.02; // ::before letter-spacing: 0.02em
const TITLEBAR_TEXT: &str = "#7d8590"; // --tscaps-titlebar-text default
const TITLEBAR_BG: &str = "#1a1f29"; // --tscaps-titlebar-bg default
// ::before radial-gradients: (circle at x, y) color core → transparent edge.
const LIGHTS: [(f32, &str); 3] =
    [(0.363, "#ff5f57"), (0.817, "#febc2e"), (1.273, "#28c840")];
const LIGHT_Y_C: f32 = 0.340;
const LIGHT_R_IN_C: f32 = 0.090; // solid stop …
const LIGHT_R_OUT_C: f32 = 0.137; // … to transparent
const LIGHT_STOP: f32 = LIGHT_R_IN_C / LIGHT_R_OUT_C; // gradient stop offset
const PRIMARY: &str = "#00ff41"; // --tscaps-primary-color default
// glow(0.18, $color: #00ff41) → text-shadow: 0 0 0.18em #00ff41; as a
// filter the blur halves (same rule glow-filter documents).
const GLOW_BLUR_EM: f32 = 0.18;
const SHOW_CARET: bool = true; // typewriter($show-caret: true) default

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

/// nyx's `effects` array: `gap_free` only.
const SPLIT: pipeline::SplitConfig = pipeline::SplitConfig {
    boundary: pipeline::Boundary::Sentence,
    max_chars: MAX_CHARS,
    min_chars: MIN_CHARS,
    max_gap_ms: 1000.0,
    effects: &[effects::Effect::GapFree],
};

// ── fonts ─────────────────────────────────────────────────────────────────

const VT323_TTF: &[u8] = include_bytes!("../../../assets/vt323-latin-400-normal.ttf");
const INTER_TTF: &[u8] = include_bytes!("../../../assets/inter-latin-400-normal.ttf");

/// `(ascent, descent, unitsPerEm)` as em ratios + upem, from the embedded
/// VT323.
fn font_metrics() -> (f32, f32, f32) {
    static METRICS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let face = fframes::ttf_parser::Face::parse(VT323_TTF, 0).expect("VT323 parses");
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

/// Same triple for the embedded Inter — the title bar only; it sets no
/// line-height of its own, so it inherits the segment's `1.15` ratio.
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

/// Raw word width: advance + one letter-spacing per UTF-16 unit (the
/// `DomProbeCanvasTextMeasurer` replica; margins live in the placement).
/// `upem` is the face's own units-per-em — the typewriter measures each
/// letter against the face of the word's script.
fn measure_word(
    face: &dyn fframes::FontFace,
    upem: usize,
    fs: f32,
    ls_px: f32,
    text: &str,
) -> f32 {
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

/// The segment's three faces resolved once for the typewriter, paired
/// with the upem each letter advances at — the per-letter mirror of
/// [`stack::measure_words`], which serves the whole-word boxes.
fn stack_faces<'a>(
    s: &stack::Stack,
    latin_ttf: &'static [u8],
    latin_weight: u16,
    source: Option<&'a dyn fframes::FontSource<'a>>,
) -> ([Option<Box<dyn fframes::FontFace<'a> + 'a>>; 3], [usize; 3]) {
    let upems = [
        fframes::ttf_parser::Face::parse(latin_ttf, 0)
            .expect("template font parses")
            .units_per_em() as usize,
        stack::script_metrics(s.arabic).unwrap().2 as usize,
        stack::script_metrics(s.hebrew).unwrap().2 as usize,
    ];
    let families = [s.latin, s.arabic, s.hebrew];
    let mut faces: [Option<Box<dyn fframes::FontFace<'a> + 'a>>; 3] = [None, None, None];
    if let Some(source) = source {
        for (i, family) in families.into_iter().enumerate() {
            let weight = if i == 0 {
                latin_weight
            } else {
                stack::native_weight(family)
            };
            let probe = FontQuery {
                family,
                size: 1,
                weight,
                ..Default::default()
            };
            faces[i] = source.resolve_font(family, probe.weight, probe.style, probe.stretch);
        }
    }
    (faces, upems)
}

// ── typewriter (tscaps `_lib/animation/typewriter`) ───────────────────────

/// The slot of letter `index` inside the word's narration window: the
/// reveal is a `step-start` at the slot's start, the caret covers the
/// whole slot.
fn letter_slot(word_start: f32, word_end: f32, index: usize, count: usize) -> (f32, f32) {
    let dur = word_end - word_start;
    let start = word_start + dur * index as f32 / count as f32;
    let end = word_start + dur * (index + 1) as f32 / count as f32;
    (start, end)
}

/// Letter `index` is on screen once `now_ms` reaches its slot start —
/// letters never disappear once shown, and before the window opens the
/// whole letter set is still hidden.
fn letter_visible(now_ms: f32, word_start: f32, word_end: f32, index: usize, count: usize) -> bool {
    count > 0 && now_ms >= letter_slot(word_start, word_end, index, count).0
}

/// The caret rides only the slot of the letter it follows (a constant
/// window between the slot edges — no blink anywhere inside it).
fn caret_on(now_ms: f32, word_start: f32, word_end: f32, index: usize, count: usize) -> bool {
    if count == 0 {
        return false;
    }
    let (start, end) = letter_slot(word_start, word_end, index, count);
    now_ms >= start && now_ms < end
}

/// `SHOW_CARET` (the template default of `--tscaps-show-caret`) gated by
/// `$hide-caret-at-line-end`: the caret dies on the last letter of each
/// line's last word, where it would paint past the text box.
fn caret_shown(
    now_ms: f32,
    word_start: f32,
    word_end: f32,
    index: usize,
    count: usize,
    hide_at_line_end: bool,
) -> bool {
    SHOW_CARET && !hide_at_line_end && caret_on(now_ms, word_start, word_end, index, count)
}

// ── chrome ────────────────────────────────────────────────────────────────

/// The title bar's top-rounded-only outline (its own bottom edge stays
/// square; the window's rounded corners show through underneath).
fn titlebar_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> String {
    let r = r.min(w / 2.0).min(h);
    let (x0, y0) = (x + r, y + r); // just past the top-left corner
    let (x1, xr) = (x + w - r, x + w); // top-right corner in / out
    let y1 = y + h;
    format!(
        "M {x0} {y} H {x1} A {r} {r} 0 0 1 {xr} {y0} V {y1} H {x} V {y0} \
         A {r} {r} 0 0 1 {x0} {y} Z"
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
    // Style controls data-driven (panel Style → template.json defaults).
    let st = style::Style::of(&doc.params, "nyx");
    let fs = overrides::resolve_font_size(&doc.params, DEFAULTS, video_h as f32);
    // --chrome-unit = font-size × chrome-scale.
    let chrome = fs * st.f32("chrome-scale", 1.0);
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
        VT323_TTF,
        ov.font_weight,
        ctx.font_source,
        fs,
        ls_px,
        false,
    );
    let widths = measured.widths;

    // The typewriter walks letter by letter, so it needs the same faces
    // (and upems) `measure_words` measured the word boxes with.
    let (faces, upems) = stack_faces(&script_stack, VT323_TTF, FONT_WEIGHT, ctx.font_source);

    let splitter = layout::BalancedPixelWidth {
        min_lines: ov.min_lines,
        max_lines: ov.max_lines,
        max_width: (video_w as f32 * ov.max_width_ratio).round(),
    };
    let split_widths: Vec<f32> = widths.iter().map(|w| w + 2.0 * margin).collect();
    let lines = splitter.split(&split_widths);

    // Window (border-box): the widest line plus its side padding, floored
    // at the 25cqh min-width (or the fixed-width toggle's 72.2cqw); height
    // stacks the title-bar clearance, the lines (0.045em apart) and the
    // bottom padding.
    let content_w = lines
        .iter()
        .map(|&(s, e)| split_widths[s..e].iter().sum::<f32>())
        .fold(0.0, f32::max);
    let min_w = if st.bool("fixed-width", false) {
        FIXED_WIDTH_CQW * video_w as f32
    } else {
        MIN_WIDTH_CQH * video_h as f32
    };
    let win_w = (content_w + 2.0 * PAD_X_EM * fs).max(min_w);
    let lines_h = lines.len() as f32 * line_h
        + lines.len().saturating_sub(1) as f32 * ov.line_spacing_em * fs;
    let win_h = PAD_TOP_C * chrome + lines_h + PAD_BOTTOM_EM * fs;
    let win_x = ov.anchor_x * video_w as f32 - win_w / 2.0;
    let win_y = ov.anchor_y * video_h as f32;
    let content_left = win_x + PAD_X_EM * fs;
    let content_top = win_y + PAD_TOP_C * chrome;

    let ascent = asc_ratio * fs;
    let descent = desc_ratio * fs;
    let half_leading = (line_h - (ascent + descent)) / 2.0;

    // Filter geometry (px from em ratios): the window box-shadow chain
    // (σ = blur/2) and the per-letter glow (σ = 0.09em).
    let s1_dy = SHADOW1_DY_EM * fs;
    let s1_std = SHADOW1_BLUR_EM * fs / 2.0;
    let s2_dy = SHADOW2_DY_EM * fs;
    let s2_std = SHADOW2_BLUR_EM * fs / 2.0;
    let glow_std = GLOW_BLUR_EM * fs / 2.0;

    // Title baseline: flex-centered line box of Inter at 0.273c, the
    // segment's 1.15 line-height inherited.
    let title_fs = TITLE_FONT_SIZE_C * chrome;
    let (inter_asc, inter_desc, _) = inter_metrics();
    let title_lh = LINE_HEIGHT * title_fs;
    let title_baseline = win_y + (TITLEBAR_H_C * chrome - title_lh) / 2.0
        + (title_lh - (inter_asc + inter_desc) * title_fs) / 2.0
        + inter_asc * title_fs;
    let title_ls = TITLE_LS_EM * title_fs;

    // ── chrome: window shadow, title bar, traffic lights, title text ──
    let win_bg = st.color("window-bg", WIN_BG);
    let titlebar_bg = st.color("titlebar-bg", TITLEBAR_BG);
    let titlebar_text = st.color("titlebar-text", TITLEBAR_TEXT);
    let title = layout::escape_xml(&st.str("title", TITLE_TEXT));
    let bar_h = TITLEBAR_H_C * chrome;
    let bar_path = titlebar_path(win_x, win_y, win_w, bar_h, WIN_RADIUS_C * chrome);
    let mut chrome_svg = format!(
        "<rect x=\"{win_x}\" y=\"{win_y}\" width=\"{win_w}\" height=\"{win_h}\" \
         rx=\"{}\" ry=\"{}\" fill=\"{win_bg}\" filter=\"url(#nyx-shadow)\"/>",
        WIN_RADIUS_C * chrome,
        WIN_RADIUS_C * chrome
    );
    chrome_svg.push_str(&format!(
        "<path d=\"{bar_path}\" fill=\"{titlebar_bg}\"/>"
    ));
    for (x_c, color) in LIGHTS {
        chrome_svg.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"url(#nyx-light-{})\"/>",
            win_x + x_c * chrome,
            win_y + LIGHT_Y_C * chrome,
            LIGHT_R_OUT_C * chrome,
            &color[1..]
        ));
    }
    chrome_svg.push_str(&format!(
        "<text x=\"{}\" y=\"{title_baseline}\" text-anchor=\"middle\" \
         font-family=\"{FONT_TITLE}\" font-weight=\"{FONT_TITLE_WEIGHT}\" \
         font-size=\"{title_fs}\" letter-spacing=\"{title_ls}\" \
         fill=\"{titlebar_text}\">{title}</text>",
        win_x + win_w / 2.0
    ));

    // ── letters (typewriter) + caret ──
    let mut words_svg = format!(
        "<g font-family=\"{font}\" font-weight=\"{weight}\" font-size=\"{fs}\" \
         letter-spacing=\"{ls_px}\">",
        font = ov.font_family,
        weight = ov.font_weight,
    );
    let mut carets_svg = String::new();
    for (line_idx, &(start, end)) in lines.iter().enumerate() {
        let line_top = content_top + line_idx as f32 * (line_h + LINE_SPACING_EM * fs);
        let baseline = line_top + half_leading + ascent;
        let line_w: f32 = split_widths[start..end].iter().sum();
        let xs = layout::word_x_positions(&widths[start..end], margin, content_left, line_w, rtl);
        for (j, ((x, word), family)) in xs
            .iter()
            .zip(&segment.words[start..end])
            .zip(&measured.families[start..end])
            .enumerate()
        {
            let slot = if *family == script_stack.latin {
                0
            } else if *family == script_stack.arabic {
                1
            } else {
                2
            };
            let family_attr = if slot == 0 {
                String::new()
            } else {
                format!(" font-family=\"{family}\"")
            };
            let face = faces[slot].as_deref();
            let upem = upems[slot];
            let mut cursor = *x;
            let count = word.display.chars().count();
            // Element inspector: override per-kata (attrs + transform) —
            // berlaku sama untuk tiap huruf kata ini (transformat affine
            // tentang pusat kata identik diterapkan per huruf).
            let wo_ctx = element_css::WordCtx {
                base_fs: fs,
                ls_px,
                video_w: video_w as f32,
                video_h: video_h as f32,
                center_x: *x + widths[start + j] / 2.0,
                center_y: baseline - fs * 0.35,
                word_start_s: word.start_ms / 1000.0,
                now_s: now_ms / 1000.0,
                font_family_svg: &stack::svg_font_family,
            };
            let wo = element_css::word_svg_attrs(&doc.params, word.index, &wo_ctx);
            let wo_transform = element_css::word_svg_transform(&doc.params, word.index, &wo_ctx)
                .map(|t| format!(" transform=\"{t}\""))
                .unwrap_or_default();
            for (i, ch) in word.display.chars().enumerate() {
                let ch_str = ch.to_string();
                let ch_w = match face {
                    Some(face) => measure_word(face, upem, fs, ls_px, &ch_str),
                    None => ch.len_utf16() as f32 * fs * 0.5 + ch.len_utf16() as f32 * ls_px,
                };
                if letter_visible(now_ms, word.start_ms, word.end_ms, i, count) {
                    let text = layout::escape_xml(&ch_str);
                    words_svg.push_str(&format!(
                        "<text x=\"{cursor}\" y=\"{baseline}\" fill=\"{fill}\"{family_attr} \
                         filter=\"url(#nyx-glow)\"{wo}{wo_transform}>{text}</text>",
                        fill = ov.primary,
                    ));
                }
                // `$hide-caret-at-line-end`: the caret dies on the last
                // letter of each line's last word.
                let hide = j + 1 == end - start && i + 1 == count;
                if caret_shown(now_ms, word.start_ms, word.end_ms, i, count, hide) {
                    carets_svg.push_str(&format!(
                        "<rect x=\"{}\" y=\"{line_top}\" width=\"{ch_w}\" height=\"{line_h}\" \
                         fill=\"{fill}\" filter=\"url(#nyx-glow)\"/>",
                        cursor + ch_w,
                        fill = ov.primary,
                    ));
                }
                cursor += ch_w;
            }
        }
    }
    words_svg.push_str("</g>");

    // ── defs ──
    let mut defs_svg = format!(
        "<filter id=\"nyx-shadow\" filterUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" \
         width=\"{}\" height=\"{}\">\
         <feDropShadow dx=\"0\" dy=\"{s1_dy}\" stdDeviation=\"{s1_std}\" \
         flood-color=\"#000000\" flood-opacity=\"{SHADOW1_ALPHA}\" result=\"s1\"/>\
         <feDropShadow in=\"s1\" dx=\"0\" dy=\"{s2_dy}\" stdDeviation=\"{s2_std}\" \
         flood-color=\"#000000\" flood-opacity=\"{SHADOW2_ALPHA}\"/></filter>",
        win_x - 2.0 * fs,
        win_y - 1.5 * fs,
        win_w + 4.0 * fs,
        win_h + 4.0 * fs
    );
    defs_svg.push_str(&format!(
        "<filter id=\"nyx-glow\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\">\
         <feDropShadow dx=\"0\" dy=\"0\" stdDeviation=\"{glow_std}\" \
         flood-color=\"{glow}\" flood-opacity=\"1\"/></filter>",
        glow = ov.primary
    ));
    for (_, color) in LIGHTS {
        defs_svg.push_str(&format!(
            "<radialGradient id=\"nyx-light-{}\">\
             <stop offset=\"{LIGHT_STOP}\" stop-color=\"{color}\"/>\
             <stop offset=\"1\" stop-color=\"{color}\" stop-opacity=\"0\"/>\
             </radialGradient>",
            &color[1..]
        ));
    }

    let words_svg = layout::dedupe_text_attrs(&words_svg);
    fframes::svgr!(
        <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox={format!("0 0 {} {}", video_w, video_h)}
            width={video_w}
            height={video_h}
        >
            <defs>{defs_svg}</defs>
            {chrome_svg}
            {words_svg}
            {carets_svg}
        </svg>
    )
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn family_of(ttf: &[u8]) -> Option<String> {
        let face = fframes::ttf_parser::Face::parse(ttf, 0).unwrap();
        let mut typographic = None;
        let mut legacy = None;
        for name in face.names() {
            let Some(text) = name.to_string() else { continue };
            match name.name_id {
                fframes::ttf_parser::name_id::TYPOGRAPHIC_FAMILY => typographic = Some(text),
                fframes::ttf_parser::name_id::FAMILY => legacy = Some(text),
                _ => {}
            }
        }
        typographic.or(legacy)
    }

    #[test]
    fn embedded_fonts_have_expected_identity() {
        assert_eq!(family_of(VT323_TTF).as_deref(), Some(FONT));
        assert_eq!(family_of(INTER_TTF).as_deref(), Some(FONT_TITLE));
        let face = fframes::ttf_parser::Face::parse(VT323_TTF, 0).unwrap();
        assert_eq!(face.weight().to_number(), FONT_WEIGHT);
        let (asc, desc, upem) = font_metrics();
        assert!(asc > 0.5 && asc < 1.5, "ascent {asc}");
        assert!(desc > 0.01 && desc < 1.0, "descent {desc}");
        assert!(upem >= 16.0, "upem {upem}");
        let (i_asc, i_desc, i_upem) = inter_metrics();
        assert!(i_asc > 0.5 && i_asc < 1.5, "inter ascent {i_asc}");
        assert!(i_desc > 0.01 && i_desc < 1.0, "inter descent {i_desc}");
        assert!(i_upem >= 16.0, "inter upem {i_upem}");
    }

    #[test]
    fn letters_appear_on_a_step_at_their_slot() {
        // 100..200, four letters → 25ms slots.
        assert!(!letter_visible(99.0, 100.0, 200.0, 0, 4));
        assert!(letter_visible(100.0, 100.0, 200.0, 0, 4));
        assert!(!letter_visible(124.0, 100.0, 200.0, 1, 4));
        assert!(letter_visible(125.0, 100.0, 200.0, 1, 4));
        assert!(letter_visible(175.0, 100.0, 200.0, 3, 4));
        // Letters never disappear once shown — all four still there after
        // the word's window (and the segment) moved on.
        assert!(letter_visible(500.0, 100.0, 200.0, 0, 4));
        assert!(letter_visible(500.0, 100.0, 200.0, 3, 4));
    }

    #[test]
    fn caret_rides_only_the_typing_slot_and_drops_at_line_end() {
        // One constant window per slot, hidden before and after — no
        // blink phase inside it.
        assert!(!caret_on(50.0, 100.0, 200.0, 0, 4));
        assert!(caret_on(110.0, 100.0, 200.0, 0, 4));
        assert!(!caret_on(125.0, 100.0, 200.0, 0, 4));
        assert!(caret_on(125.0, 100.0, 200.0, 1, 4));
        assert!(!caret_on(200.0, 100.0, 200.0, 3, 4));
        // nyx ships with the caret on, and `$hide-caret-at-line-end`
        // suppresses it on the last letter of each line's last word.
        assert!(SHOW_CARET);
        assert!(!caret_shown(180.0, 100.0, 200.0, 3, 4, true));
        assert!(caret_shown(180.0, 100.0, 200.0, 3, 4, false));
    }
}
