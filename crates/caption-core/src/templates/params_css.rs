//! Translator subset-CSS ↔ `CaptionParams` untuk panel Code.
//!
//! Panel Code dulu menampilkan JSON mentah (cues+params campur) — ribet.
//! Kini captions = SRT, settings = **CSS subset** yang ditafsir engine
//! (bukan browser): property di-blossom jadi field `CaptionParams`.
//! Field yang tak punya padanan CSS visual (effects, style_values,
//! element_overrides) TIDAK ikut CSS — dikelola panel masing-masing dan
//! **dipertahankan** saat merge (CSS hanya menimpa field yang disebut).
//!
//! Property yang didukung (kebab-case):
//! font-family, font-size (px), font-weight, color,
//! letter-spacing/word-spacing/line-spacing (em),
//! font-style: italic, text-decoration: underline|line-through,
//! text-transform: none|uppercase|lowercase, text-align,
//! direction, y-frac, width-frac, pop-in (ms), travel (px),
//! h-align, h-offset, v-align, v-offset, angle (deg),
//! motion, motion-duration (ms), motion-distance (em),
//! max-lines, max-width-frac, max-chars.

use crate::CaptionParams;

/// Emit CSS subset dari params (hanya field yang terisi / non-default visual).
pub fn params_to_css(p: &CaptionParams) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("font-family: {};", p.font_family));
    lines.push(format!("font-size: {}px;", p.font_size));
    lines.push(format!("font-weight: {};", p.font_weight));
    lines.push(format!("color: {};", p.color));
    lines.push(format!("y-frac: {};", fmt_f(p.y_frac)));
    lines.push(format!("width-frac: {};", fmt_f(p.width_frac)));
    lines.push(format!("pop-in: {}ms;", p.pop_in_ms));
    lines.push(format!("travel: {}px;", fmt_f(p.travel_px)));
    lines.push(format!("direction: {};", p.direction));
    if let Some(v) = p.letter_spacing_em {
        lines.push(format!("letter-spacing: {}em;", fmt_f(v)));
    }
    if let Some(v) = p.word_spacing_em {
        lines.push(format!("word-spacing: {}em;", fmt_f(v)));
    }
    if let Some(v) = p.line_spacing_em {
        lines.push(format!("line-spacing: {}em;", fmt_f(v)));
    }
    if let Some(true) = p.italic {
        lines.push("font-style: italic;".into());
    }
    let mut deco: Vec<&str> = Vec::new();
    if let Some(true) = p.underline {
        deco.push("underline");
    }
    if let Some(true) = p.strikethrough {
        deco.push("line-through");
    }
    if !deco.is_empty() {
        lines.push(format!("text-decoration: {};", deco.join(" ")));
    }
    if let Some(c) = p.text_case.as_deref() {
        lines.push(format!("text-transform: {};", c));
    }
    if let Some(a) = p.text_align.as_deref() {
        lines.push(format!("text-align: {};", a));
    }
    if let Some(a) = p.h_align.as_deref() {
        lines.push(format!("h-align: {};", a));
    }
    if let Some(v) = p.h_offset {
        lines.push(format!("h-offset: {};", fmt_f(v)));
    }
    if let Some(a) = p.v_align.as_deref() {
        lines.push(format!("v-align: {};", a));
    }
    if let Some(v) = p.v_offset {
        lines.push(format!("v-offset: {};", fmt_f(v)));
    }
    if let Some(v) = p.angle_deg {
        lines.push(format!("angle: {}deg;", fmt_f(v)));
    }
    if let Some(m) = p.motion_preset.as_deref().filter(|s| !s.is_empty()) {
        lines.push(format!("motion: {};", m));
    }
    if let Some(v) = p.motion_duration_ms {
        lines.push(format!("motion-duration: {}ms;", v));
    }
    if let Some(v) = p.motion_distance_em {
        lines.push(format!("motion-distance: {}em;", fmt_f(v)));
    }
    if let Some(v) = p.max_lines {
        lines.push(format!("max-lines: {};", v));
    }
    if let Some(v) = p.max_width_frac {
        lines.push(format!("max-width-frac: {};", fmt_f(v)));
    }
    if let Some(v) = p.max_chars {
        lines.push(format!("max-chars: {};", v));
    }
    lines.join("\n")
}

/// Parse CSS subset lalu timpa field yang cocok ke `params`.
/// Property tak dikenal = error (supaya ketik salah cepat ketahuan).
/// Field di luar CSS (effects/style_values/element_overrides) tidak disentuh.
pub fn apply_css_to_params(params: &mut CaptionParams, css: &str) -> Result<(), String> {
    for raw in css.split(';') {
        let decl = raw.trim();
        if decl.is_empty() || decl.starts_with("/*") {
            continue;
        }
        let (prop, value) = decl
            .split_once(':')
            .ok_or_else(|| format!("invalid declaration (no ':'): {decl}"))?;
        let prop = prop.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            return Err(format!("empty value for {prop}"));
        }
        match prop.as_str() {
            "font-family" => params.font_family = value.to_string(),
            "font-size" => params.font_size = parse_px(value, "font-size")?,
            "font-weight" => {
                params.font_weight = value.parse::<u16>().map_err(|_| format!("invalid font-weight: {value}"))?
            }
            "color" => params.color = value.to_string(),
            "y-frac" => params.y_frac = parse_frac(value, "y-frac")?,
            "width-frac" => params.width_frac = parse_frac(value, "width-frac")?,
            "pop-in" => params.pop_in_ms = parse_ms(value, "pop-in")?,
            "travel" => params.travel_px = parse_px(value, "travel")?,
            "direction" => {
                let d = value.to_ascii_lowercase();
                if !matches!(d.as_str(), "auto" | "ltr" | "rtl") {
                    return Err(format!("invalid direction (auto|ltr|rtl): {value}"));
                }
                params.direction = d;
            }
            "letter-spacing" => params.letter_spacing_em = Some(parse_em(value, "letter-spacing")?),
            "word-spacing" => params.word_spacing_em = Some(parse_em(value, "word-spacing")?),
            "line-spacing" => params.line_spacing_em = Some(parse_em(value, "line-spacing")?),
            "font-style" => {
                let v = value.to_ascii_lowercase();
                if v == "italic" {
                    params.italic = Some(true);
                } else if v == "normal" {
                    params.italic = Some(false);
                } else {
                    return Err(format!("invalid font-style (italic|normal): {value}"));
                }
            }
            "text-decoration" => {
                let v = value.to_ascii_lowercase();
                params.underline = Some(v.contains("underline"));
                params.strikethrough = Some(v.contains("line-through") || v.contains("linethrough"));
            }
            "text-transform" => {
                let v = value.to_ascii_lowercase();
                if !matches!(v.as_str(), "none" | "uppercase" | "lowercase" | "capitalize") {
                    return Err(format!("invalid text-transform: {value}"));
                }
                params.text_case = Some(v);
            }
            "text-align" => {
                let v = value.to_ascii_lowercase();
                if !matches!(v.as_str(), "left" | "center" | "right") {
                    return Err(format!("invalid text-align: {value}"));
                }
                params.text_align = Some(v);
            }
            "h-align" => {
                let v = value.to_ascii_lowercase();
                if !matches!(v.as_str(), "left" | "center" | "right") {
                    return Err(format!("invalid h-align: {value}"));
                }
                params.h_align = Some(v);
            }
            "h-offset" => params.h_offset = Some(parse_frac(value, "h-offset")?),
            "v-align" => {
                let v = value.to_ascii_lowercase();
                if !matches!(v.as_str(), "top" | "center" | "bottom") {
                    return Err(format!("invalid v-align: {value}"));
                }
                params.v_align = Some(v);
            }
            "v-offset" => params.v_offset = Some(parse_frac(value, "v-offset")?),
            "angle" => params.angle_deg = Some(parse_deg(value, "angle")?),
            "motion" => {
                let v = value.to_ascii_lowercase();
                if v == "none" || v == "static" || v == "inherited" {
                    params.motion_preset = Some(String::new()); // template default
                } else {
                    params.motion_preset = Some(v);
                }
            }
            "motion-duration" => {
                params.motion_duration_ms = Some(parse_ms(value, "motion-duration")?)
            }
            "motion-distance" => {
                params.motion_distance_em = Some(parse_em(value, "motion-distance")?)
            }
            "max-lines" => {
                params.max_lines = Some(
                    value.parse::<u32>().map_err(|_| format!("invalid max-lines: {value}"))?,
                )
            }
            "max-width-frac" => params.max_width_frac = Some(parse_frac(value, "max-width-frac")?),
            "max-chars" => {
                params.max_chars = Some(
                    value.parse::<u32>().map_err(|_| format!("invalid max-chars: {value}"))?,
                )
            }
            other => return Err(format!("unknown property: {other}")),
        }
    }
    Ok(())
}

fn fmt_f(v: f32) -> String {
    // Buang nol ekor: 0.780 → 0.78, 110.0 → 110
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn parse_px(v: &str, prop: &str) -> Result<f32, String> {
    let s = v.strip_suffix("px").unwrap_or(v).trim();
    s.parse::<f32>()
        .map_err(|_| format!("invalid {prop} (expect Npx): {v}"))
}

fn parse_em(v: &str, prop: &str) -> Result<f32, String> {
    let s = v.strip_suffix("em").unwrap_or(v).trim();
    s.parse::<f32>()
        .map_err(|_| format!("invalid {prop} (expect Nem): {v}"))
}

fn parse_ms(v: &str, prop: &str) -> Result<u32, String> {
    let s = v.strip_suffix("ms").unwrap_or(v).trim();
    s.parse::<u32>()
        .map_err(|_| format!("invalid {prop} (expect Nms): {v}"))
}

fn parse_deg(v: &str, prop: &str) -> Result<f32, String> {
    let s = v.strip_suffix("deg").unwrap_or(v).trim();
    s.parse::<f32>()
        .map_err(|_| format!("invalid {prop} (expect Ndeg): {v}"))
}

fn parse_frac(v: &str, prop: &str) -> Result<f32, String> {
    let n: f32 = v
        .parse()
        .map_err(|_| format!("invalid {prop} (expect number 0..1): {v}"))?;
    if !(0.0..=1.0).contains(&n) {
        return Err(format!("{prop} out of range 0..1: {v}"));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_roundtrip_core() {
        let mut p = CaptionParams::default();
        apply_css_to_params(
            &mut p,
            "font-family: Poppins; font-size: 96px; font-weight: 700; color: gold;\n\
             y-frac: 0.75; width-frac: 0.8; pop-in: 300ms; travel: 40px; direction: rtl;",
        )
        .unwrap();
        assert_eq!(p.font_family, "Poppins");
        assert_eq!(p.font_size, 96.0);
        assert_eq!(p.font_weight, 700);
        assert_eq!(p.color, "gold");
        assert!((p.y_frac - 0.75).abs() < 1e-6);
        assert_eq!(p.pop_in_ms, 300);
        assert_eq!(p.direction, "rtl");

        let css = params_to_css(&p);
        let mut p2 = CaptionParams::default();
        apply_css_to_params(&mut p2, &css).unwrap();
        assert_eq!(p2.font_family, "Poppins");
        assert_eq!(p2.font_weight, 700);
        assert_eq!(p2.direction, "rtl");
    }

    #[test]
    fn css_optional_fields() {
        let mut p = CaptionParams::default();
        apply_css_to_params(
            &mut p,
            "font-style: italic; text-decoration: underline line-through; \
             text-transform: uppercase; text-align: center; \
             h-align: left; h-offset: 0.3; v-align: bottom; v-offset: 0.2; angle: -5deg; \
             motion: pop-in; motion-duration: 250ms; motion-distance: 1.2em; \
             max-lines: 2; max-width-frac: 0.9; max-chars: 40; \
             letter-spacing: 0.05em; word-spacing: 0.1em; line-spacing: 0.2em;",
        )
        .unwrap();
        assert_eq!(p.italic, Some(true));
        assert_eq!(p.underline, Some(true));
        assert_eq!(p.strikethrough, Some(true));
        assert_eq!(p.text_case.as_deref(), Some("uppercase"));
        assert_eq!(p.h_align.as_deref(), Some("left"));
        assert_eq!(p.v_align.as_deref(), Some("bottom"));
        assert_eq!(p.angle_deg, Some(-5.0));
        assert_eq!(p.motion_preset.as_deref(), Some("pop-in"));
        assert_eq!(p.motion_duration_ms, Some(250));
        assert_eq!(p.max_lines, Some(2));
        assert_eq!(p.max_chars, Some(40));

        let css = params_to_css(&p);
        let mut p2 = CaptionParams::default();
        apply_css_to_params(&mut p2, &css).unwrap();
        assert_eq!(p2.italic, Some(true));
        assert_eq!(p2.motion_preset.as_deref(), Some("pop-in"));
    }

    #[test]
    fn unknown_property_rejected() {
        let mut p = CaptionParams::default();
        assert!(apply_css_to_params(&mut p, "bogus-prop: 1;").is_err());
    }

    #[test]
    fn preserves_effects_and_overrides() {
        let mut p = CaptionParams::default();
        p.effects.gap_free = Some(true);
        p.element_overrides.push(crate::WordOverride {
            word: 3,
            ..Default::default()
        });
        apply_css_to_params(&mut p, "color: red;").unwrap();
        assert_eq!(p.effects.gap_free, Some(true));
        assert_eq!(p.element_overrides.len(), 1);
    }
}
