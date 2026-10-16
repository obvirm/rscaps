//! Interpreter subset-CSS untuk override per-kata (element inspector).
//!
//! tscaps membiarkan user menulis CSS bebas per elemen (`color: gold;
//! animation: pop 0.4s var(--on-word-being-narrated-starts) …`) karena
//! render mereka DOM+CSS. Kita tidak: render = string SVG dari
//! `render_frame()` (preview = export = CLI headless, tanpa browser).
//! Teks CSS di sini ditafsir ENGINE — property list di-blossom jadi
//! atribut SVG, `animation:` dipetakan ke mesin motion (`recipes`) yang
//! sudah dievaluasi per frame. `css` menang atas field terstruktur
//! `WordOverride` (paritas catatan tscaps "Wins over the template").

use super::recipes;
use crate::WordOverride;
use crate::CaptionParams;

/// Preset animasi per-kata — paritas picker tscaps
/// (Inherited/None/Fade/Rise/Slide/Scale/Pop/Snap/Wobble/Flash).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimPreset {
    None,
    Fade,
    Rise,
    Slide,
    Scale,
    Pop,
    Snap,
    Wobble,
    Flash,
}

impl AnimPreset {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "fade" | "fade-in" => Self::Fade,
            "rise" | "rise-in" => Self::Rise,
            "slide" | "slide-in" => Self::Slide,
            "scale" | "scale-in" => Self::Scale,
            "pop" | "pop-in" => Self::Pop,
            "snap" | "snap-in" => Self::Snap,
            "wobble" | "wobble-in" => Self::Wobble,
            "flash" | "flash-in" => Self::Flash,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimSpec {
    pub preset: AnimPreset,
    pub duration_s: f32,
}

/// Hasil merge: field terstruktur + CSS subset (CSS menang).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ResolvedWord {
    pub color: Option<String>,
    pub font_family: Option<String>,
    pub font_weight: Option<u16>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    /// Skala ukuran relatif base (1.0 = default template).
    pub size_scale: Option<f32>,
    /// Ukuran absolut (px) — menang atas `size_scale` bila keduanya ada.
    pub size_px: Option<f32>,
    /// Skala letter-spacing relatif base.
    pub letter_spacing_scale: Option<f32>,
    /// Letter-spacing absolut (px) — menang atas skala.
    pub letter_spacing_px: Option<f32>,
    pub rotation_deg: Option<f32>,
    pub opacity: Option<f32>,
    /// Anchor pusat kata (fraksi frame); None = layout asli.
    pub h_frac: Option<f32>,
    pub v_frac: Option<f32>,
    pub anim: Option<AnimSpec>,
}

fn find_override(params: &CaptionParams, word_index: u32) -> Option<&WordOverride> {
    params.element_overrides.iter().find(|o| o.word == word_index)
}

/// Resolve override satu kata: field terstruktur dulu, lalu CSS subset
/// menimpa. `None` = kata tanpa override (template tampil normal).
pub fn resolve_word(params: &CaptionParams, word_index: u32) -> Option<ResolvedWord> {
    let o = find_override(params, word_index)?;
    let anim = match o.animation.as_deref() {
        Some(name) if !name.eq_ignore_ascii_case("inherited") && !name.is_empty() => {
            AnimPreset::parse(name).map(|preset| AnimSpec {
                preset,
                duration_s: o.animation_duration_s.unwrap_or(0.35),
            })
        }
        _ => None,
    };
    let mut resolved = ResolvedWord {
        color: o.color.clone(),
        font_family: o.font_family.clone(),
        font_weight: o.font_weight,
        italic: o.italic,
        underline: o.underline,
        strikethrough: o.strikethrough,
        size_scale: o.size_scale,
        size_px: None,
        letter_spacing_scale: None,
        letter_spacing_px: None,
        rotation_deg: o.rotation_deg,
        opacity: None,
        h_frac: o.h_frac,
        v_frac: o.v_frac,
        anim,
    };
    if let Some(css) = o.css.as_deref().filter(|s| !s.trim().is_empty()) {
        apply_css(&mut resolved, css);
    }
    Some(resolved)
}

fn parse_weight(v: &str) -> Option<u16> {
    match v.trim().to_ascii_lowercase().as_str() {
        "bold" => Some(700),
        "normal" => Some(400),
        other => other.parse::<u16>().ok(),
    }
}

/// `120%` / `1.2em` → skala; `24px` / `24` → absolut.
fn parse_font_size(v: &str) -> (Option<f32>, Option<f32>) {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        if let Some(n) = p.trim().parse::<f32>().ok() {
            return (Some(n / 100.0), None);
        }
    }
    if let Some(e) = v.strip_suffix("em") {
        if let Some(n) = e.trim().parse::<f32>().ok() {
            return (Some(n), None);
        }
    }
    let px = v.strip_suffix("px").unwrap_or(v);
    match px.trim().parse::<f32>() {
        Ok(n) => (None, Some(n)),
        Err(_) => (None, None),
    }
}

fn parse_letter_spacing(v: &str) -> (Option<f32>, Option<f32>) {
    let v = v.trim();
    if let Some(e) = v.strip_suffix("em") {
        if let Some(n) = e.trim().parse::<f32>().ok() {
            return (Some(n), None);
        }
    }
    let px = v.strip_suffix("px").unwrap_or(v);
    match px.trim().parse::<f32>() {
        Ok(n) => (None, Some(n)),
        Err(_) => (None, None),
    }
}

fn parse_deg(v: &str) -> Option<f32> {
    let v = v.trim();
    let n = v.strip_suffix("deg").unwrap_or(v);
    n.trim().parse::<f32>().ok()
}

/// `pop 0.4s var(--on-word-being-narrated-starts) ease both` → preset+durasi.
/// Jangkar delay variabel diterima apa adanya (trigger = mulai narasi kata).
fn parse_animation(v: &str) -> Option<AnimSpec> {
    let mut tokens = v.split_whitespace();
    let name = tokens.next()?;
    let preset = AnimPreset::parse(name)?;
    let mut duration_s = 0.35f32;
    for tok in tokens {
        if let Ok(n) = tok.trim_end_matches('s').parse::<f32>() {
            if n > 0.0 {
                duration_s = n;
                break;
            }
        }
    }
    Some(AnimSpec { preset, duration_s })
}

/// Tafsir satu blok deklarasi (allowlist — properti lain diabaikan, bukan
/// error render).
fn apply_css(resolved: &mut ResolvedWord, css: &str) {
    for decl in css.split(';') {
        let Some((key, value)) = decl.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "color" | "fill" => resolved.color = Some(value.to_string()),
            "font-style" => {
                resolved.italic = Some(matches!(
                    value.to_ascii_lowercase().as_str(),
                    "italic" | "oblique"
                ));
            }
            "font-weight" => resolved.font_weight = parse_weight(value),
            "font-family" => {
                resolved.font_family = Some(value.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
            "font-size" => {
                let (scale, px) = parse_font_size(value);
                resolved.size_scale = scale.or(resolved.size_scale);
                resolved.size_px = px.or(resolved.size_px);
            }
            "letter-spacing" => {
                let (scale, px) = parse_letter_spacing(value);
                resolved.letter_spacing_scale = scale.or(resolved.letter_spacing_scale);
                resolved.letter_spacing_px = px.or(resolved.letter_spacing_px);
            }
            "opacity" => resolved.opacity = value.parse::<f32>().ok(),
            "rotate" => resolved.rotation_deg = parse_deg(value),
            "transform" => {
                // Hanya `rotate(Ndeg)` yang diambil dari transform (subset).
                if let Some(idx) = value.find("rotate(") {
                    let rest = &value[idx + "rotate(".len()..];
                    if let Some(end) = rest.find(')') {
                        resolved.rotation_deg = parse_deg(&rest[..end]);
                    }
                }
            }
            "text-decoration" => {
                let v = value.to_ascii_lowercase();
                if v.contains("underline") {
                    resolved.underline = Some(true);
                }
                if v.contains("line-through") {
                    resolved.strikethrough = Some(true);
                }
                if v.contains("none") {
                    resolved.underline = Some(false);
                    resolved.strikethrough = Some(false);
                }
            }
            "animation" => resolved.anim = parse_animation(value),
            _ => {}
        }
    }
}

/// State anim terurai untuk satu frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AnimState {
    pub tx_em: f32,
    pub ty_em: f32,
    pub scale: f32,
    pub rot_deg: f32,
    pub opacity: f32,
}

/// Evaluasi preset ke state per frame (`elapsed_s` sejak narasi kata).
pub fn anim_state(spec: &AnimSpec, elapsed_s: f32) -> AnimState {
    let dur = spec.duration_s;
    match spec.preset {
        AnimPreset::None => AnimState::default(),
        AnimPreset::Fade => AnimState {
            opacity: recipes::FadeIn::with(0.0, dur).eval(elapsed_s),
            ..AnimState::default()
        },
        AnimPreset::Rise => {
            let mut rise = recipes::RiseIn::with(0.5, 0.0);
            rise.duration_s = dur;
            let (opacity, ty_em) = rise.eval(elapsed_s);
            AnimState {
                opacity,
                ty_em,
                ..AnimState::default()
            }
        }
        AnimPreset::Slide => {
            let mut slide = recipes::SlideIn::with(0.6, 0.0);
            slide.duration_s = dur;
            let (opacity, tx_em) = slide.eval(elapsed_s);
            AnimState {
                opacity,
                tx_em,
                ..AnimState::default()
            }
        }
        AnimPreset::Scale => {
            let mut scale = recipes::ScaleIn::with(0.4, 0.0);
            scale.duration_s = dur;
            let (opacity, scale_v) = scale.eval(elapsed_s);
            AnimState {
                opacity,
                scale: scale_v,
                ..AnimState::default()
            }
        }
        AnimPreset::Pop => {
            let mut pop = recipes::PopIn::with(0.35);
            pop.duration_s = dur;
            AnimState {
                scale: pop.eval(elapsed_s),
                ..AnimState::default()
            }
        }
        AnimPreset::Snap => {
            let mut snap = recipes::SnapIn::with(0.5);
            snap.duration_s = dur;
            let (opacity, ty_em, rot_deg, scale) = snap.eval(elapsed_s);
            AnimState {
                opacity,
                ty_em,
                rot_deg,
                scale,
                ..AnimState::default()
            }
        }
        AnimPreset::Wobble => {
            let mut wobble = recipes::WobbleIn::with(14.0, 0.0);
            wobble.duration_s = dur;
            AnimState {
                rot_deg: wobble.eval(elapsed_s),
                ..AnimState::default()
            }
        }
        AnimPreset::Flash => {
            let mut flash = recipes::FlashIn::with();
            flash.duration_s = dur;
            AnimState {
                opacity: flash.eval(elapsed_s).element_opacity,
                ..AnimState::default()
            }
        }
    }
}

/// Konteks geometri kata di frame ini (semua px di ruang viewBox).
pub struct WordCtx<'a> {
    pub base_fs: f32,
    pub ls_px: f32,
    pub video_w: f32,
    pub video_h: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub word_start_s: f32,
    pub now_s: f32,
    /// Supaya `font-family` melewati `stack::svg_font_family` (kontrak
    /// template) — passed in sebagai fn untuk hindari siklus import.
    pub font_family_svg: &'a dyn Fn(&str) -> String,
}

fn esc_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Satu-linier untuk template: string atribut SVG tambahan non-transform
/// (`fill`/`font-*`/`text-decoration`/`opacity`; kosong `""` bila tak ada
/// override). Sisipkan `{wo}` di emit loop `<text>`.
pub fn word_svg_attrs(params: &CaptionParams, word_index: u32, ctx: &WordCtx) -> String {
    let Some(r) = resolve_word(params, word_index) else {
        return String::new();
    };
    let mut attrs = String::new();
    if let Some(color) = &r.color {
        attrs.push_str(&format!(" fill=\"{}\"", esc_attr(color)));
    }
    if let Some(family) = &r.font_family {
        let svg_family = (ctx.font_family_svg)(family);
        attrs.push_str(&format!(" font-family=\"{}\"", esc_attr(&svg_family)));
    }
    if let Some(weight) = r.font_weight {
        attrs.push_str(&format!(" font-weight=\"{weight}\""));
    }
    let mut decoration: Vec<&str> = Vec::new();
    if r.italic == Some(true) {
        attrs.push_str(" font-style=\"italic\"");
    }
    if r.underline == Some(true) {
        decoration.push("underline");
    }
    if r.strikethrough == Some(true) {
        decoration.push("line-through");
    }
    if !decoration.is_empty() {
        attrs.push_str(&format!(" text-decoration=\"{}\"", decoration.join(" ")));
    }
    let fs = r.size_px.unwrap_or(r.size_scale.unwrap_or(1.0) * ctx.base_fs);
    if r.size_px.is_some() || r.size_scale.is_some() {
        attrs.push_str(&format!(" font-size=\"{fs}\""));
    }
    if let Some(px) = r.letter_spacing_px.or(r.letter_spacing_scale.map(|s| s * ctx.ls_px)) {
        attrs.push_str(&format!(" letter-spacing=\"{px}\""));
    }
    let opacity = word_opacity(&r, ctx);
    if opacity < 1.0 - f32::EPSILON {
        attrs.push_str(&format!(" opacity=\"{opacity}\""));
    }
    attrs
}

/// Nilai transform override (isi atribut `transform="…"`, tanpa attr
/// itu sendiri): anchor absolut + anim + rotasi/skala tentang pusat kata.
/// `None` = kata ini pakai transform bawaan template.
pub fn word_svg_transform(params: &CaptionParams, word_index: u32, ctx: &WordCtx) -> Option<String> {
    let r = resolve_word(params, word_index)?;
    let fs = r.size_px.unwrap_or(r.size_scale.unwrap_or(1.0) * ctx.base_fs);
    let anim = r
        .anim
        .as_ref()
        .filter(|a| a.preset != AnimPreset::None)
        .map(|a| anim_state(a, ctx.now_s - ctx.word_start_s));
    let mut tx = 0.0f32;
    let mut ty = 0.0f32;
    let mut scale = 1.0f32;
    let mut rot = r.rotation_deg.unwrap_or(0.0);
    if let Some(state) = anim {
        tx += state.tx_em * fs;
        ty += state.ty_em * fs;
        scale *= if state.scale != 0.0 { state.scale } else { 1.0 };
        rot += state.rot_deg;
    }
    if let Some(h) = r.h_frac {
        tx += h * ctx.video_w - ctx.center_x;
    }
    if let Some(v) = r.v_frac {
        ty += v * ctx.video_h - ctx.center_y;
    }
    let has_anim_tx = anim.map(|s| s.tx_em != 0.0 || s.ty_em != 0.0).unwrap_or(false);
    let has_anchor = r.h_frac.is_some() || r.v_frac.is_some();
    if !has_anim_tx && !has_anchor && (scale - 1.0).abs() <= f32::EPSILON && rot.abs() <= f32::EPSILON
    {
        return None;
    }
    if (scale - 1.0).abs() > f32::EPSILON || rot.abs() > f32::EPSILON {
        let (cx, cy) = (ctx.center_x + tx, ctx.center_y + ty);
        Some(format!(
            "translate({cx},{cy}) rotate({rot}) scale({scale}) translate({ncx},{ncy})",
            ncx = -cx,
            ncy = -cy,
        ))
    } else {
        Some(format!("translate({tx},{ty})"))
    }
}

/// True bila kata ini punya override APA SAJA (template memakai ini untuk
/// menahan animasi entrance per-katanya sendiri — paritas tscaps
/// "animation custom menggantikan animasi template").
pub fn has_override(params: &CaptionParams, word_index: u32) -> bool {
    find_override(params, word_index).is_some()
}

fn word_opacity(r: &ResolvedWord, ctx: &WordCtx) -> f32 {
    let mut opacity = r.opacity.unwrap_or(1.0);
    let anim = r
        .anim
        .as_ref()
        .filter(|a| a.preset != AnimPreset::None)
        .map(|a| anim_state(a, ctx.now_s - ctx.word_start_s));
    if let Some(state) = anim {
        let has_opacity = matches!(
            r.anim.as_ref().map(|a| a.preset),
            Some(
                AnimPreset::Fade
                    | AnimPreset::Rise
                    | AnimPreset::Slide
                    | AnimPreset::Scale
                    | AnimPreset::Snap
                    | AnimPreset::Flash
            )
        );
        if has_opacity {
            opacity *= state.opacity;
        }
    }
    opacity
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn params_with(o: WordOverride) -> CaptionParams {
        CaptionParams {
            element_overrides: vec![o],
            ..Default::default()
        }
    }

    fn ctx<'a>(f: &'a dyn Fn(&str) -> String) -> WordCtx<'a> {
        WordCtx {
            base_fs: 10.0,
            ls_px: 1.0,
            video_w: 320.0,
            video_h: 240.0,
            center_x: 100.0,
            center_y: 80.0,
            word_start_s: 1.0,
            now_s: 1.2,
            font_family_svg: f,
        }
    }

    #[test]
    fn structured_fields_resolve() {
        let params = params_with(WordOverride {
            word: 3,
            color: Some("#ff0000".into()),
            italic: Some(true),
            size_scale: Some(1.5),
            animation: Some("rise".into()),
            animation_duration_s: Some(0.4),
            ..Default::default()
        });
        let r = resolve_word(&params, 3).expect("resolved");
        assert_eq!(r.color.as_deref(), Some("#ff0000"));
        assert_eq!(r.italic, Some(true));
        assert_eq!(r.size_scale, Some(1.5));
        assert_eq!(
            r.anim,
            Some(AnimSpec {
                preset: AnimPreset::Rise,
                duration_s: 0.4
            })
        );
        assert!(resolve_word(&params, 4).is_none(), "index lain tak tersentuh");
    }

    #[test]
    fn css_wins_over_structured() {
        let params = params_with(WordOverride {
            word: 0,
            color: Some("#111111".into()),
            css: Some("color: gold; font-weight: bold; opacity: 0.5; rotate: 12deg".into()),
            ..Default::default()
        });
        let r = resolve_word(&params, 0).unwrap();
        assert_eq!(r.color.as_deref(), Some("gold"));
        assert_eq!(r.font_weight, Some(700));
        assert_eq!(r.opacity, Some(0.5));
        assert_eq!(r.rotation_deg, Some(12.0));
    }

    #[test]
    fn animation_declaration_parses_name_duration_and_var_anchor() {
        let params = params_with(WordOverride {
            word: 0,
            css: Some(
                "animation: pop 0.4s var(--on-word-being-narrated-starts) ease both".into(),
            ),
            ..Default::default()
        });
        let r = resolve_word(&params, 0).unwrap();
        assert_eq!(
            r.anim,
            Some(AnimSpec {
                preset: AnimPreset::Pop,
                duration_s: 0.4
            })
        );
    }

    #[test]
    fn font_size_percent_and_px() {
        let mut resolved = ResolvedWord::default();
        apply_css(&mut resolved, "font-size: 150%");
        assert_eq!(resolved.size_scale, Some(1.5));
        apply_css(&mut resolved, "font-size: 24px");
        assert_eq!(resolved.size_px, Some(24.0));
    }

    #[test]
    fn svg_attrs_empty_without_override() {
        let params = CaptionParams::default();
        let idents = |s: &str| s.to_string();
        let f: &dyn Fn(&str) -> String = &idents;
        assert_eq!(word_svg_attrs(&params, 0, &ctx(f)), "");
    }

    #[test]
    fn svg_attrs_include_fill_decoration_and_animation_opacity() {
        let params = params_with(WordOverride {
            word: 0,
            color: Some("#00ff00".into()),
            underline: Some(true),
            h_frac: Some(0.5),
            v_frac: Some(0.5),
            animation: Some("flash".into()),
            animation_duration_s: Some(0.3), // elapsed0.2 → p≈0.67 → fase silap (opacity<1)
            ..Default::default()
        });
        let idents = |s: &str| s.to_string();
        let f: &dyn Fn(&str) -> String = &idents;
        let attrs = word_svg_attrs(&params, 0, &ctx(f));
        assert!(attrs.contains("fill=\"#00ff00\""), "{attrs}");
        assert!(attrs.contains("text-decoration=\"underline\""), "{attrs}");
        assert!(attrs.contains("opacity="), "{attrs}");
        // Anchor (160,120) vs pusat (100,80) → translate(60,40).
        let t = word_svg_transform(&params, 0, &ctx(f)).expect("transform");
        assert!(t.contains("translate(60,40)"), "{t}");
        assert!(has_override(&params, 0));
        assert!(!has_override(&params, 9));
    }

    #[test]
    fn flash_in_holds_dips_and_settles() {
        let mut flash = recipes::FlashIn::with();
        flash.duration_s = 0.4;
        assert_eq!(flash.eval(0.0).element_opacity, 1.0);
        // p≈0.63 masuk jendela silap plate → element menyembunyi sebentar.
        assert!(flash.eval(0.25).element_opacity < 1.0);
        assert_eq!(flash.eval(0.5).element_opacity, 1.0);
    }
}
