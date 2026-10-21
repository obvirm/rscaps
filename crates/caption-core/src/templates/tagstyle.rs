//! Tipografi per-kata ber-tag (Tier 2 kontrol `tag-*` tscaps):
//! `tag-weight` (weight), `tag-italic` (toggle), `tag-font-family`
//! (kontrol `font`, slug FontStackCatalog), `tag-font-scale`
//! (pengali ukuran), `tag-bold` (toggle on="900"/off=inherit).
//!
//! tscaps me-resolve-nya lewat CSS (kelas `.emphasis/.accent/.entity` +
//! custom property `--tscaps-tag-*` per template stylesheet); port ini
//! meniru SEMANTIK per template (aturan selector di style.scss masing-
//! masing), snap ke face statis yang benar-benar dimuat
//! (`faces::snap`), lalu mengukur + emit atribut per-kata. Element
//! inspector tetap menang: attrs tag disisipkan SEBELUM `{wo}` dan
//! `layout::dedupe_text_attrs` mempertahankan nilai terakhir.
//!
//! Kata script (arab/ibrani) mengabaikan tipografi tag (face tag hanya
//! latin; browser pun akan jatuh ke fallback script).

use super::faces;
use super::pipeline::TimedWord;
use super::stack::WordType;
use super::style::Style;

/// Nilai kontrol tag satu template (default dari template.json via Style).
#[derive(Clone, Copy, Debug)]
pub struct TagType {
    pub weight: u16,
    pub italic: bool,
    pub family: &'static str,
    pub scale: f32,
}

/// Baca kontrol `tag-*` milik `template`. `base_family`/`base_weight` =
/// face dasar template (untuk tag-bold off / pembanding snap).
pub fn resolve(
    st: &Style,
    template: &str,
    _base_family: &'static str,
    base_weight: u16,
) -> TagType {
    match template {
        // cleo scss: `.emphasis { font-weight: var(--tag-weight, 900) }`,
        // `.accent,.entity { font-style: var(--tag-italic, italic) }`.
        "cleo" => TagType {
            weight: st.f32("tag-weight", 900.0).round() as u16,
            italic: st.bool("tag-italic", true),
            family: "",
            scale: 1.0,
        },
        // mira scss: `.emphasis,.accent,.entity { font-weight: …900 }`.
        "mira" => TagType {
            weight: st.f32("tag-weight", 900.0).round() as u16,
            italic: false,
            family: "",
            scale: 1.0,
        },
        // enzo scss: `.emphasis,.entity` ganti face + weight + italic +
        // ukuran penuh; `.accent` tetap grotesque, ukuran 0.85×.
        "enzo" => TagType {
            family: faces::family_from_font_id(&st.str("tag-font-family", "eb-garamond")),
            weight: st.f32("tag-font-weight", 400.0).round() as u16,
            italic: true,
            scale: st.f32("tag-font-scale", 1.4),
        },
        // lena scss: family + weight pada ketiga tag.
        "lena" => TagType {
            family: faces::family_from_font_id(&st.str("tag-font-family", "inter")),
            weight: st.f32("tag-font-weight", 900.0).round() as u16,
            italic: false,
            scale: 1.0,
        },
        // selene scss: family + tag-bold (on="900", off=inherit).
        "selene" => TagType {
            family: faces::family_from_font_id(&st.str("tag-font-family", "montserrat")),
            weight: if st.bool("tag-bold", true) {
                900
            } else {
                base_weight
            },
            italic: false,
            scale: 1.0,
        },
        // pico scss: tag-bold saja (on="900", off=inherit).
        "pico" => TagType {
            weight: if st.bool("tag-bold", true) {
                900
            } else {
                base_weight
            },
            italic: false,
            family: "",
            scale: 1.0,
        },
        // sara scss: `font-size: calc(1em * var(--tag-font-scale, 1.4))`.
        "sara" => TagType {
            weight: base_weight,
            italic: false,
            family: "",
            scale: st.f32("tag-font-scale", 1.4),
        },
        // sol scss: `.line + .line` — line aksen UTUH ganti face + weight
        // (bukan per-tag); ditangani khusus di template, TagType hanya
        // pembawa nilainya.
        "sol" => TagType {
            family: faces::family_from_font_id(&st.str("tag-font-family", "kalam")),
            weight: st.f32("tag-font-weight", 700.0).round() as u16,
            italic: false,
            scale: 1.0,
        },
        _ => TagType {
            weight: base_weight,
            italic: false,
            family: "",
            scale: 1.0,
        },
    }
}

/// Override tipografi satu kata, atau `None` bila kata ini dirender
/// dengan face dasar (bukan kata ber-tag, template lain, snap gagal, atau
/// hasil snap identik dengan dasar).
pub fn word_type(
    template: &str,
    tt: &TagType,
    word: &TimedWord,
    base_family: &'static str,
    base_weight: u16,
) -> Option<WordType> {
    let lifted = word.has_tag("emphasis") || word.has_tag("accent") || word.has_tag("entity");
    if !lifted {
        return None;
    }
    let mut ty = WordType {
        family: "",
        weight: base_weight,
        italic: false,
        scale: 1.0,
    };
    match template {
        "cleo" => {
            if word.has_tag("emphasis") {
                ty.weight = tt.weight;
            }
            if word.has_tag("accent") || word.has_tag("entity") {
                ty.italic = tt.italic;
            }
        }
        "mira" => ty.weight = tt.weight,
        "enzo" => {
            if word.has_tag("emphasis") || word.has_tag("entity") {
                ty.family = tt.family;
                ty.weight = tt.weight;
                ty.italic = tt.italic;
                ty.scale = tt.scale;
            } else {
                // `.accent`: tetap grotesque, ukuran parsial 0.85×.
                ty.scale = tt.scale * 0.85;
            }
        }
        "lena" => {
            ty.family = tt.family;
            ty.weight = tt.weight;
        }
        "selene" => {
            ty.family = tt.family;
            ty.weight = tt.weight;
        }
        "pico" => ty.weight = tt.weight,
        "sara" => ty.scale = tt.scale,
        _ => return None,
    }
    let family = if ty.family.is_empty() { base_family } else { ty.family };
    match faces::snap(family, ty.weight, ty.italic) {
        Some((weight, italic)) => {
            // Identik dengan dasar → tak ada yang diubah (hemat attrs + ukur).
            if weight == base_weight && !italic && ty.scale == 1.0 && family == base_family {
                return None;
            }
            Some(WordType {
                family,
                weight,
                italic,
                scale: ty.scale,
            })
        }
        None => {
            // Keluarga di luar tabel FACES: weight/italic inert (face dasar
            // tetap dipakai), scale tetap berlaku — advance dasar × scale.
            if ty.scale == 1.0 {
                return None;
            }
            Some(WordType {
                family: "",
                weight: base_weight,
                italic: false,
                scale: ty.scale,
            })
        }
    }
}

/// Atribut SVG per-kata untuk `WordType` — sisipkan SEBELUM `{wo}`
/// (element inspector menang via `dedupe_text_attrs`). `base_family`
/// untuk melewatkan `font-family` bila sama dengan dasar.
pub fn svg_attrs(ty: &WordType, fs: f32, base_family: &str, base_weight: u16) -> String {
    let mut s = String::new();
    if !ty.family.is_empty() && ty.family != base_family {
        s.push_str(&format!(
            " font-family=\"{}\"",
            super::stack::svg_font_family(ty.family)
        ));
    }
    if ty.weight != base_weight {
        s.push_str(&format!(" font-weight=\"{}\"", ty.weight));
    }
    if ty.italic {
        s.push_str(" font-style=\"italic\"");
    }
    if (ty.scale - 1.0).abs() > f32::EPSILON {
        s.push_str(&format!(" font-size=\"{:.2}\"", fs * ty.scale));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaptionParams;

    fn st_with(template: &str, values: &[(&str, serde_json::Value)]) -> Style<'static> {
        let mut params = Box::new(CaptionParams::default());
        for (id, v) in values {
            params.style_values.insert(id.to_string(), v.clone());
        }
        let leaked: &'static mut CaptionParams = Box::leak(params);
        Style::of(leaked, template)
    }

    fn word(text: &str, tags: &[&str]) -> TimedWord {
        let mut w = TimedWord::new(text, 0.0, 1000.0);
        for t in tags {
            w.tags.push((*t).to_string());
        }
        w
    }

    #[test]
    fn cleo_weight_and_italic_split() {
        let st = st_with("cleo", &[]);
        let tt = resolve(&st, "cleo", "Lora", 500);
        assert_eq!(tt.weight, 900);
        assert!(tt.italic);
        let em = word("wow", &["emphasis"]);
        let wt = word_type("cleo", &tt, &em, "Lora", 500).unwrap();
        // Lora berhenti di 700: snap 900 → 700 (browser pun memilih 700).
        assert_eq!((wt.weight, wt.italic), (700, false));
        let acc = word("nice", &["accent"]);
        let wt = word_type("cleo", &tt, &acc, "Lora", 500).unwrap();
        assert_eq!((wt.weight, wt.italic), (500, true));
        // Kedua tag → weight + italic (snap Lora 900→700 + italic 700).
        let both = word("big", &["emphasis", "accent"]);
        let wt = word_type("cleo", &tt, &both, "Lora", 500).unwrap();
        assert_eq!((wt.weight, wt.italic), (700, true));
        // Bukan kata ber-tag → None.
        assert!(word_type("cleo", &tt, &word("plain", &[]), "Lora", 500).is_none());
        assert!(word_type("cleo", &tt, &word("q", &["quote"]), "Lora", 500).is_none());
    }

    #[test]
    fn enzo_face_switch_and_partial_accent_scale() {
        let st = st_with("enzo", &[]);
        let tt = resolve(&st, "enzo", "Inter", 900);
        assert_eq!(tt.family, "EB Garamond");
        assert_eq!(tt.weight, 400);
        let em = word("boom", &["emphasis"]);
        let wt = word_type("enzo", &tt, &em, "Inter", 900).unwrap();
        assert_eq!(wt.family, "EB Garamond");
        assert_eq!((wt.weight, wt.italic, (wt.scale - 1.4).abs() < 1e-6), (400, true, true));
        let acc = word("small", &["accent"]);
        let wt = word_type("enzo", &tt, &acc, "Inter", 900).unwrap();
        assert_eq!(wt.family, "Inter");
        assert_eq!(wt.weight, 900);
        assert!((wt.scale - 1.4 * 0.85).abs() < 1e-6);
    }

    #[test]
    fn scale_only_templates_keep_base_face() {
        let st = st_with("sara", &[]);
        let tt = resolve(&st, "sara", "IM Fell English", 400);
        let w = word("lift", &["entity"]);
        let wt = word_type("sara", &tt, &w, "IM Fell English", 400).unwrap();
        // IM Fell di luar tabel FACES: family kosong (face dasar diukur),
        // scale tetap diterapkan.
        assert_eq!(wt.family, "");
        assert!((wt.scale - 1.4).abs() < 1e-6);
    }

    #[test]
    fn tag_bold_off_falls_back_to_base() {
        let st = st_with(
            "selene",
            &[("tag-bold", serde_json::Value::Bool(false))],
        );
        let tt = resolve(&st, "selene", "Montserrat", 600);
        let w = word("x", &["emphasis"]);
        assert!(word_type("selene", &tt, &w, "Montserrat", 600).is_none());
    }

    #[test]
    fn unknown_font_family_falls_back_to_base_face() {
        let st = st_with(
            "lena",
            &[("tag-font-family", serde_json::Value::String("no-such-face".into()))],
        );
        let tt = resolve(&st, "lena", "Inter", 400);
        let w = word("x", &["emphasis"]);
        // Id tak dikenal → keluarga jatuh ke face dasar; weight kontrol
        // tetap berlaku (kontrol terpisah).
        let wt = word_type("lena", &tt, &w, "Inter", 400).unwrap();
        assert_eq!(wt.family, "Inter");
        assert_eq!(wt.weight, 900);
    }

    #[test]
    fn attrs_skip_base_values_and_scale_emits_size() {
        let wt = WordType { family: "Inter", weight: 900, italic: false, scale: 1.0 };
        // Base weight 400 → 900 perlu attr; kalau base 900 → skip (inherit).
        assert_eq!(svg_attrs(&wt, 40.0, "Inter", 400), " font-weight=\"900\"");
        assert_eq!(svg_attrs(&wt, 40.0, "Inter", 900), "");
        let scaled = WordType { family: "", weight: 400, italic: true, scale: 1.4 };
        let a = svg_attrs(&scaled, 40.0, "Inter", 400);
        assert_eq!(a, " font-style=\"italic\" font-size=\"56.00\"");
        let fam = WordType { family: "EB Garamond", weight: 400, italic: true, scale: 1.4 };
        let a = svg_attrs(&fam, 40.0, "Inter", 900);
        assert!(a.contains("font-family=\"EB Garamond\""));
        assert!(a.contains("font-weight=\"400\""));
    }
}
