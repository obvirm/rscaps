//! Wajah font statis untuk tipografi per-kata ber-tag (Tier 2 `tag-*`):
//! `tag-weight/italic/font-family/font-scale/bold` butuh FACE ASLI —
//! tscaps memuat font variable (satu file, seluruh axis) + italic asli;
//! fframes tidak punya dukungan variable font, jadi kita memuat static
//! weight (lihat `scripts/woff2-to-ttf.mjs`) dan SNAP permintaan user ke
//! face yang benar-benar ada.
//!
//! Tabel ini SATU-SATUNYA sumber kebenaran face ber-tag: ukur (lebar kata
//! via hmtx), snap, dan pemetaan id kontrol `font` → nama keluarga. File
//! TTF yang sama dimuat fontdb rasterizer (CLI: `studio/tplfonts/ttf`,
//! web: fontManifest) — menambah face = tambah JOBS di woff2-to-ttf.mjs +
//! entri `FACES` di sini + entri `FONT_SOURCES` di fontManifest.ts.

use std::sync::OnceLock;

use fframes::ttf_parser;

/// Satu face statis yang bisa dipakai kata ber-tag.
pub struct Face {
    pub family: &'static str,
    pub weight: u16,
    pub italic: bool,
    pub ttf: &'static [u8],
}

/// `$file` prefix nama file aset (lowercase), `$family` nama keluarga
/// name-table (fontdb ID16/ID1), `$style` literal ("normal"/"italic") —
/// `concat!` menuntut literal murni.
macro_rules! face {
    ($file:literal, $family:literal, $weight:literal, $italic:literal, $style:literal) => {
        Face {
            family: $family,
            weight: $weight,
            italic: $italic,
            ttf: include_bytes!(concat!(
                "../../assets/",
                $file,
                "-latin-",
                $weight,
                "-",
                $style,
                ".ttf"
            )),
        }
    };
}

/// Face untuk keluarga yang membawa kontrol tipografi tag. Keluarga tanpa
/// kontrol (caveat dsb.) tidak perlu masuk — `snap` mengembalikan `None`
/// dan kata dirender dengan face dasar template (kontrol inert).
pub static FACES: &[Face] = &[
    // cleo: tag-weight (Lora 100-900 → snap 400-700; Lora berhenti di 700),
    // tag-italic (accent/entity, weight dasar 500 / emphasis 700).
    face!("lora", "Lora", 400, false, "normal"),
    face!("lora", "Lora", 500, false, "normal"),
    face!("lora", "Lora", 600, false, "normal"),
    face!("lora", "Lora", 700, false, "normal"),
    face!("lora", "Lora", 500, true, "italic"),
    face!("lora", "Lora", 700, true, "italic"),
    // mira (tag-weight 900) + quote-italic (600 italic); selene
    // (tag-font-family montserrat + tag-bold 900).
    face!("montserrat", "Montserrat", 300, false, "normal"),
    face!("montserrat", "Montserrat", 400, false, "normal"),
    face!("montserrat", "Montserrat", 500, false, "normal"),
    face!("montserrat", "Montserrat", 600, false, "normal"),
    face!("montserrat", "Montserrat", 700, false, "normal"),
    face!("montserrat", "Montserrat", 800, false, "normal"),
    face!("montserrat", "Montserrat", 900, false, "normal"),
    face!("montserrat", "Montserrat", 600, true, "italic"),
    // lena (tag-font-family inter + tag-font-weight 900).
    face!("inter", "Inter", 400, false, "normal"),
    face!("inter", "Inter", 500, false, "normal"),
    face!("inter", "Inter", 600, false, "normal"),
    face!("inter", "Inter", 700, false, "normal"),
    face!("inter", "Inter", 800, false, "normal"),
    face!("inter", "Inter", 900, false, "normal"),
    // enzo (tag-font-family eb-garamond + tag-font-weight 400 + italic).
    face!("eb-garamond", "EB Garamond", 400, false, "normal"),
    face!("eb-garamond", "EB Garamond", 400, true, "italic"),
    // sol (tag-font-family kalam + tag-font-weight 700).
    face!("kalam", "Kalam", 400, false, "normal"),
    face!("kalam", "Kalam", 700, false, "normal"),
    // pico (tag-bold 900 → snap 800; JetBrains Mono berhenti di 800).
    face!("jetbrains-mono", "JetBrains Mono", 400, false, "normal"),
    face!("jetbrains-mono", "JetBrains Mono", 800, false, "normal"),
    // lena/selene boleh menunjuk poppins via kontrol font.
    face!("poppins", "Poppins", 700, false, "normal"),
    face!("poppins", "Poppins", 900, false, "normal"),
];

/// Face bytes untuk kombinasi eksak (setelah `snap`).
pub fn ttf_bytes(family: &str, weight: u16, italic: bool) -> Option<&'static [u8]> {
    FACES
        .iter()
        .find(|f| f.family == family && f.weight == weight && f.italic == italic)
        .map(|f| f.ttf)
}

/// Snap permintaan (weight, italic) ke face yang ADA: gaya cocok
/// diutamakan (penalti), weight terdekat, seri → lebih berat. `None` =
/// keluarga tak punya face di tabel → kata dirender face dasar (kontrol
/// inert).
pub fn snap(family: &str, weight: u16, italic: bool) -> Option<(u16, bool)> {
    let mut best: Option<(u16, bool, i32, u16)> = None; // (w, italic, dist, w utk tie)
    for f in FACES.iter().filter(|f| f.family == family) {
        let dist =
            (f.weight as i32 - weight as i32).abs() + if f.italic != italic { 10_000 } else { 0 };
        let take = match best {
            None => true,
            Some((_, _, best_dist, best_w)) => dist < best_dist || (dist == best_dist && f.weight > best_w),
        };
        if take {
            best = Some((f.weight, f.italic, dist, f.weight));
        }
    }
    best.map(|(w, i, _, _)| (w, i))
}

/// Id kontrol `font` tscaps (FontStackCatalog slug) → nama keluarga yang
/// punya face di `FACES`. Id tak dikenal → `""` (kata dirender face dasar;
/// kontrol font hanya mendukung keluarga di tabel).
pub fn family_from_font_id(id: &str) -> &'static str {
    match id {
        "anton" => "Anton",
        "bebas-neue" => "Bebas Neue",
        "bricolage-grotesque" => "Bricolage Grotesque",
        "caveat" => "Caveat",
        "eb-garamond" => "EB Garamond",
        "gabarito" => "Gabarito",
        "im-fell-english" => "IM Fell English",
        "inter" => "Inter",
        "jetbrains-mono" => "JetBrains Mono",
        "kalam" => "Kalam",
        "komika-axis" => "Komika Axis",
        "lobster" => "Lobster",
        "lora" => "Lora",
        "manrope" => "Manrope",
        "montserrat" => "Montserrat",
        "playfair-display" => "Playfair Display",
        "poppins" => "Poppins",
        "righteous" => "Righteous",
        "vt323" => "VT323",
        _ => "",
    }
}

/// Metrics face (index tabel) — di-parse sekali per proses.
pub struct FaceMetrics {
    pub upem: u16,
}

/// Index face di tabel (untuk `face_metrics` + lebar per karakter).
pub fn face_index(family: &str, weight: u16, italic: bool) -> Option<usize> {
    FACES
        .iter()
        .position(|f| f.family == family && f.weight == weight && f.italic == italic)
}

/// upem face ke-N — cache statis (Face::parse per-frame × per-kata
/// menumpuk; upem tak pernah berubah).
pub fn face_metrics(face_index: usize) -> Option<&'static FaceMetrics> {
    static CACHE: OnceLock<Vec<Option<FaceMetrics>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        FACES
            .iter()
            .map(|f| {
                ttf_parser::Face::parse(f.ttf, 0)
                    .ok()
                    .map(|face| FaceMetrics {
                        upem: face.units_per_em(),
                    })
            })
            .collect()
    });
    cache.get(face_index).and_then(|m| m.as_ref())
}

/// Advance glyph `c` dalam unit font (hmtx), `None` bila glyph tak ada.
pub fn char_advance_units(face_index: usize, c: char) -> Option<u16> {
    let f = &FACES[face_index];
    let face = ttf_parser::Face::parse(f.ttf, 0).ok()?;
    let glyph = face.glyph_index(c)?;
    face.tables().hmtx?.advance(glyph)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nama keluarga dari name table (ID16 dulu, else ID1) — aturan
    /// fontdb::parse_names. Harus cocok `family` tabel supaya font-family
    /// attr resolve di browser (fontManifest) DAN di fontdb rasterizer.
    fn family_name(ttf: &[u8]) -> String {
        let face = ttf_parser::Face::parse(ttf, 0).expect("face parses");
        for id in [16, 1] {
            for n in face.names() {
                if n.name_id == id && n.is_unicode() {
                    if let Some(s) = n.to_string() {
                        return s;
                    }
                }
            }
        }
        panic!("no family name");
    }

    #[test]
    fn faces_resolve_by_table_family_and_weight() {
        for f in FACES {
            // fontsource membawa ID16 aneh untuk sebagian keluarga
            // ("Montserrat Thin") — stack::svg_font_family menyertakan
            // alternat itu di font-family attr, jadi rasterizer tetap
            // resolve. Nama utama ATAU alternat keduanya sah di sini.
            let name = family_name(f.ttf);
            assert!(
                name == f.family || super::super::stack::svg_font_family(f.family).contains(&name),
                "face {} → name-table {name:?}",
                f.family
            );
            let idx = face_index(f.family, f.weight, f.italic).unwrap();
            let metrics = face_metrics(idx).unwrap();
            let face = ttf_parser::Face::parse(f.ttf, 0).unwrap();
            assert_eq!(face.units_per_em(), metrics.upem);
            // Weight OS/2 cocok nominal (fontsource static per-weight).
            assert_eq!(face.weight().to_number(), f.weight, "{} {}", f.family, f.weight);
            assert_eq!(face.style() == ttf_parser::Style::Italic, f.italic);
        }
    }

    #[test]
    fn snap_prefers_exact_then_nearest_heavier() {
        assert_eq!(snap("Lora", 500, false), Some((500, false)));
        assert_eq!(snap("Lora", 900, false), Some((700, false)));
        assert_eq!(snap("Lora", 100, false), Some((400, false)));
        assert_eq!(snap("Lora", 650, false), Some((700, false)));
        // Italic ada di 500/700: 400 italic → 500 italic (weight terdekat).
        assert_eq!(snap("Lora", 400, true), Some((500, true)));
        assert_eq!(snap("JetBrains Mono", 900, false), Some((800, false)));
        assert_eq!(snap("JetBrains Mono", 700, false), Some((800, false)));
        assert_eq!(snap("EB Garamond", 400, true), Some((400, true)));
        assert_eq!(snap("EB Garamond", 800, false), Some((400, false)));
        // Keluarga di luar tabel → None (kontrol inert).
        assert_eq!(snap("Caveat", 700, false), None);
    }

    #[test]
    fn font_ids_map_to_families() {
        assert_eq!(family_from_font_id("eb-garamond"), "EB Garamond");
        assert_eq!(family_from_font_id("jetbrains-mono"), "JetBrains Mono");
        assert_eq!(family_from_font_id("inter"), "Inter");
        // Id tak dikenal → kosong (inert).
        assert_eq!(family_from_font_id("something-else"), "");
    }

    #[test]
    fn advance_units_match_hmtx() {
        let idx = face_index("Lora", 500, false).unwrap();
        // 'i' jelas lebih sempit dari 'm' di serif mana pun.
        let i = char_advance_units(idx, 'i').unwrap();
        let m = char_advance_units(idx, 'm').unwrap();
        assert!(i < m);
        assert!(char_advance_units(idx, '\u{4E2D}').is_none()); // CJK tak dimiliki
    }
}
