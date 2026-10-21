//! Script font stacks + bidi direction, shared by every template.
//!
//! Port of tscaps `FontStackCatalog` (apps/studio/src/core/fonts/domain):
//! a template names its Latin face and the stack assigns the face that
//! draws each script under it (arabic/hebrew in v1), so non-Latin cues
//! measure with the right advances and emit the right `font-family`
//! without per-template work. Direction ports
//! `StrongCharacterMajorityTextDirectionDetector` +
//! `LineBaseDirectionResolver` (ada's verified v1 port).
//!
//! A new template gets correct non-Latin layout by calling
//! [`measure_words`] and passing [`WindowedSegment::direction`] into
//! `layout::word_x_positions` - nothing else to wire.

use crate::templates::pipeline::TimedWord;
use crate::CaptionDocument;
use std::sync::OnceLock;
use unicode_bidi::{bidi_class, BidiClass};

// ── script faces ───────────────────────────────────────────────────────────

pub const FONT_CAIRO: &str = "Cairo";
pub const FONT_ASSISTANT: &str = "Assistant";
pub const FONT_VAZIRMATN: &str = "Vazirmatn";
pub const FONT_HEEBO: &str = "Heebo";
pub const FONT_LALEZAR: &str = "Lalezar";
pub const FONT_AMIRI: &str = "Amiri";

const CAIRO_TTF: &[u8] = include_bytes!("../../assets/cairo-arabic-700-normal.ttf");
const ASSISTANT_TTF: &[u8] = include_bytes!("../../assets/assistant-hebrew-700-normal.ttf");
const VAZIRMATN_TTF: &[u8] = include_bytes!("../../assets/vazirmatn-arabic-700-normal.ttf");
const HEEBO_TTF: &[u8] = include_bytes!("../../assets/heebo-hebrew-700-normal.ttf");
const LALEZAR_TTF: &[u8] = include_bytes!("../../assets/lalezar-arabic-400-normal.ttf");
const AMIRI_TTF: &[u8] = include_bytes!("../../assets/amiri-arabic-700-normal.ttf");

const SCRIPT_FACES: [&str; 6] = [
    FONT_CAIRO,
    FONT_ASSISTANT,
    FONT_VAZIRMATN,
    FONT_HEEBO,
    FONT_LALEZAR,
    FONT_AMIRI,
];

fn script_ttf(family: &str) -> Option<&'static [u8]> {
    match family {
        FONT_CAIRO => Some(CAIRO_TTF),
        FONT_ASSISTANT => Some(ASSISTANT_TTF),
        FONT_VAZIRMATN => Some(VAZIRMATN_TTF),
        FONT_HEEBO => Some(HEEBO_TTF),
        FONT_LALEZAR => Some(LALEZAR_TTF),
        FONT_AMIRI => Some(AMIRI_TTF),
        _ => None,
    }
}

/// Native weight each script face ships as (what `resolve_font` asks for;
/// fontdb fuzzy-matches if a template probes a heavier Latin weight).
pub fn native_weight(family: &str) -> u16 {
    match family {
        FONT_LALEZAR => 400,
        _ => 700,
    }
}

/// `(ascent, descent, unitsPerEm)` with the first two as em ratios, from
/// the embedded TTF of a script face. `None` when `family` is the
/// template's own Latin face (templates keep their local metrics for it).
pub fn script_metrics(family: &str) -> Option<(f32, f32, f32)> {
    static METRICS: OnceLock<[(f32, f32, f32); 6]> = OnceLock::new();
    let all = METRICS.get_or_init(|| {
        SCRIPT_FACES.map(|family| {
            let face = fframes::ttf_parser::Face::parse(script_ttf(family).unwrap(), 0)
                .expect("script font parses");
            let upem = face.units_per_em() as f32;
            (
                face.ascender() as f32 / upem,
                face.descender().abs() as f32 / upem,
                upem,
            )
        })
    });
    SCRIPT_FACES
        .iter()
        .position(|&f| f == family)
        .map(|i| all[i])
}

// ── SVG font-family emission ───────────────────────────────────────────────

/// Browser-facing family name → extra name-table alternates (fontsource
/// packaging ships odd ID16 typographic families: "Montserrat Thin",
/// "Bricolage Grotesque 96pt ExtraBold", …). Templates MUST emit
/// `font-family` through this helper: the result is a CSS font stack whose
/// first name the browser matches against `@font-face`, while usvgr/fontdb
/// (the native rasterizer / measurement path) tries every name in order, so the
/// text survives both renderers. A family without a known alternate is
/// returned unchanged.
///
/// Daftar ini juga dipakai [`font_stack_names`] untuk RESOLVE saat mengukur
/// (fontsource menyimpan nama aneh di name-table — ID1/ID16 bisa berbeda
/// dari nama @font-face).
const FONT_ALTERNATES: &[(&str, &str)] = &[
    ("Montserrat", "Montserrat Thin"),
    ("Bricolage Grotesque", "Bricolage Grotesque 96pt ExtraBold"),
    ("Manrope", "Manrope ExtraLight"),
    ("Poppins", "Poppins Black"),
];

/// Semua kandidat nama keluarga untuk `family`: nama utama + alternate
/// name-table (urutan = urutan resolve saat mengukur lebar kata).
pub fn font_stack_names(family: &str) -> impl Iterator<Item = &str> + '_ {
    let alternate: Option<&str> = FONT_ALTERNATES
        .iter()
        .find(|(name, _)| *name == family)
        .map(|(_, alt)| *alt);
    std::iter::once(family).chain(alternate)
}

pub fn svg_font_family(family: &str) -> String {
    match FONT_ALTERNATES.iter().find(|(name, _)| *name == family) {
        Some((name, alternate)) => format!("{name}, {alternate}"),
        None => family.to_string(),
    }
}

// ── stacks ─────────────────────────────────────────────────────────────────

/// The faces that draw under one Latin family: the template's own face
/// plus the script faces for arabic and hebrew (tscaps `FontStackFaces`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stack {
    pub latin: &'static str,
    pub arabic: &'static str,
    pub hebrew: &'static str,
}

impl Stack {
    /// tscaps `FontStackCatalog` entries for the Latin families the
    /// templates use (latin is copied per entry there; grouped here when
    /// the script faces match). Unknown families fall back to the
    /// cairo/assistant pair, the same v1 default ada shipped with.
    /// Urdu/Nastaliq (Noto Nastaliq Urdu) is deferred with the rest of
    /// the non-arabic/hebrew scripts.
    pub fn for_latin(latin: &'static str) -> Stack {
        let (arabic, hebrew) = match latin {
            "Inter" | "Montserrat" | "Manrope" | "Bricolage Grotesque" | "Caveat" | "Lobster"
            | "JetBrains Mono" => (FONT_VAZIRMATN, FONT_HEEBO),
            "Anton" | "Bebas Neue" | "Komika Axis" | "Bungee" | "Righteous" | "VT323" => {
                (FONT_LALEZAR, FONT_HEEBO)
            }
            "Playfair Display" | "IM FELL English" | "Lora" => (FONT_AMIRI, FONT_ASSISTANT),
            _ => (FONT_CAIRO, FONT_ASSISTANT),
        };
        Stack {
            latin,
            arabic,
            hebrew,
        }
    }
}

// ── script detection ───────────────────────────────────────────────────────

/// The stack's face for one char: Hebrew ranges -> hebrew face, the
/// Arabic-script unicode ranges (incl. presentation forms) -> arabic
/// face, everything else `None` (Latin face).
fn script_face_for_char(c: char, stack: &Stack) -> Option<&'static str> {
    let cp = c as u32;
    if (0x0590..=0x05FF).contains(&cp) || (0xFB1D..=0xFB4F).contains(&cp) {
        Some(stack.hebrew)
    } else if (0x0600..=0x06FF).contains(&cp)
        || (0x0750..=0x077F).contains(&cp)
        || (0x08A0..=0x08FF).contains(&cp)
        || (0xFB50..=0xFDFF).contains(&cp)
        || (0xFE70..=0xFEFF).contains(&cp)
    {
        Some(stack.arabic)
    } else {
        None
    }
}

/// Family of the word's first script-assigned char (words are mono-script
/// in practice; close enough to the browser's per-glyph fallback). Uses
/// the raw `text`, not `display`, so text effects cannot mask the script.
pub fn word_family(word: &str, stack: &Stack) -> &'static str {
    word.chars()
        .find_map(|c| script_face_for_char(c, stack))
        .unwrap_or(stack.latin)
}

/// Family whose script chars dominate the segment - drives the
/// baseline/half-leading metrics (ties fall back to the Latin face,
/// matching the browser's strut).
pub fn dominant_family(words: &[TimedWord], stack: &Stack) -> &'static str {
    let mut counts = [0usize; 3]; // [latin, arabic, hebrew]
    for word in words {
        for c in word.text.chars() {
            let slot = match script_face_for_char(c, stack) {
                Some(f) if f == stack.arabic => 1,
                Some(_) => 2,
                None => 0,
            };
            counts[slot] += 1;
        }
    }
    let mut best = 0;
    for i in 1..counts.len() {
        if counts[i] > counts[best] {
            best = i;
        }
    }
    [stack.latin, stack.arabic, stack.hebrew][best]
}

// ── direction ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Ltr,
    Rtl,
}

impl Direction {
    pub fn is_rtl(self) -> bool {
        matches!(self, Direction::Rtl)
    }

    fn opposite(self) -> Self {
        match self {
            Direction::Ltr => Direction::Rtl,
            Direction::Rtl => Direction::Ltr,
        }
    }
}

/// Port of `StrongCharacterMajorityTextDirectionDetector`: RTL iff the
/// document has strictly more strong-RTL (R/AL) than strong-LTR (L)
/// chars; ties, digits and empty text count as LTR.
pub fn detect_document_direction(doc: &CaptionDocument) -> Direction {
    let (mut ltr, mut rtl) = (0usize, 0usize);
    for cue in &doc.cues {
        for c in cue.text.chars() {
            match bidi_class(c) {
                BidiClass::L => ltr += 1,
                BidiClass::R | BidiClass::AL => rtl += 1,
                _ => {}
            }
        }
    }
    if rtl > ltr {
        Direction::Rtl
    } else {
        Direction::Ltr
    }
}

/// Port of `LineBaseDirectionResolver`: a line keeps its declared
/// direction unless every strong char in it points the other way (only
/// single-direction lines flip).
pub fn resolve_line_direction(text: &str, declared: Direction) -> Direction {
    let mut saw_opposite = false;
    for c in text.chars() {
        match bidi_class(c) {
            BidiClass::L if declared == Direction::Ltr => return Direction::Ltr,
            BidiClass::R | BidiClass::AL if declared == Direction::Rtl => return Direction::Rtl,
            BidiClass::L | BidiClass::R | BidiClass::AL => saw_opposite = true,
            _ => {}
        }
    }
    if saw_opposite {
        declared.opposite()
    } else {
        declared
    }
}

// ── measure ────────────────────────────────────────────────────────────────

/// Override tipografi per-kata dari tag (Tier 2 `tag-*`), dibangun oleh
/// `tagstyle::word_type`. `family` kosong = latin dasar template.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WordType {
    pub family: &'static str,
    pub weight: u16,
    pub italic: bool,
    pub scale: f32,
}

/// Per-word widths (px, advance + letter-spacing, no margins) and the
/// script family each word draws with.
pub struct Measured {
    pub widths: Vec<f32>,
    pub families: Vec<&'static str>,
}

/// Shaped advance of `text` in a script face, px at `fs`. Sums the
/// harfbuzz x-advances of the *joined* glyphs — summing isolated
/// per-char advances overcounts Arabic/Hebrew badly (cursive joining
/// drops side bearings), which spread RTL words apart vs tscaps'
/// canvas-measured layout.
fn shaped_advance(family: &str, text: &str, fs: f32) -> Option<f32> {
    script_ttf(family)?;
    static FACES: OnceLock<[Option<rustybuzz::Face<'static>>; 6]> = OnceLock::new();
    let faces = FACES.get_or_init(|| {
        SCRIPT_FACES.map(|f| rustybuzz::Face::from_slice(script_ttf(f).unwrap(), 0))
    });
    let idx = SCRIPT_FACES.iter().position(|&f| f == family)?;
    let face = faces[idx].as_ref()?;
    let upem = face.units_per_em() as f32;
    let mut buf = rustybuzz::UnicodeBuffer::new();
    buf.push_str(text);
    let out = rustybuzz::shape(face, &[], buf);
    let units: i32 = out.glyph_positions().iter().map(|p| p.x_advance).sum();
    Some(units as f32 * fs / upem)
}

/// Perkiraan lebar SATU karakter yang tak dimiliki face template —
/// meniru fallback font sistem di browser (tscaps mengukur via DOM dgn
/// font sistem; kita offline): ideograph/kana/hangul & bentuk fullwidth
/// = 1em (lebar penuh, konsisten antar font CJK), Thai/Lao dasar =
/// 0.62em dengan tanda gabung 0, sisanya 0.5em (konvensi fallback lama).
/// Tanpa ini glif non-latin terukur 0 → kata per-kata tumpang tindih.
pub fn fallback_char_advance(c: char, fs: f32) -> f32 {
    let u = c as u32;
    let fullwidth = (0x1100..=0x11FF).contains(&u)   // Hangul jamo
        || (0x2E80..=0x303F).contains(&u)             // CJK radikal & tanda ideografis
        || (0x3040..=0x30FF).contains(&u)             // Kana
        || (0x3130..=0x318F).contains(&u)             // Hangul compat jamo
        || (0x31F0..=0x31FF).contains(&u)             // Katakana extension
        || (0x3400..=0x4DBF).contains(&u)
        || (0x4E00..=0x9FFF).contains(&u)
        || (0xAC00..=0xD7AF).contains(&u)             // Hangul syllables
        || (0xF900..=0xFAFF).contains(&u)             // CJK compatibility
        || (0xFF01..=0xFF60).contains(&u)             // Fullwidth forms
        || (0x20000..=0x3FFFF).contains(&u);          // CJK ext B..
    if fullwidth {
        return fs;
    }
    if (0xFF61..=0xFFDC).contains(&u) {
        // Halfwidth forms (kana setengah lebar).
        return fs * 0.5;
    }
    if (0x0E00..=0x0EFF).contains(&u) {
        // Thai/Lao: tanda di atas/di bawah nol, dasar ≈0.62em (setara Leelawadee sistem).
        let combining = u == 0x0E31
            || (0x0E34..=0x0E3A).contains(&u)
            || (0x0E47..=0x0E4E).contains(&u)
            || u == 0x0EB1
            || (0x0EB4..=0x0EBC).contains(&u)
            || (0x0EC8..=0x0ECD).contains(&u);
        return if combining { 0.0 } else { fs * 0.62 };
    }
    fs * 0.5
}

/// Total `fallback_char_advance` untuk `text`.
pub fn script_fallback_width(text: &str, fs: f32) -> f32 {
    text.chars().map(|c| fallback_char_advance(c, fs)).sum()
}

/// Measure every word with the face its script resolves to. Script words
/// (arabic/hebrew) are shaped with harfbuzz so cursive joining is
/// accounted for; Latin words keep the `DomProbeCanvasTextMeasurer`
/// per-char convention every template already uses: advance at size ==
/// unitsPerEm (exact font units through fframes' integer math) scaled
/// back to px, plus one letter-spacing per UTF-16 unit; words whose face
/// is not loaded fall back to `utf16 * fs * 0.5` so plates still appear.
///
/// Tipografi tag (Tier 2): kata latin dengan `Some(WordType)` diukur
/// dengan face hasil snap dari [`crate::templates::faces`] (bytes yang
/// sama dimuat fontdb rasterizer); keluarga di luar tabel tetap memakai
/// face dasar dengan `advance × scale` (advance linear terhadap fs).
/// Kata script (arab/ibrani) mengabaikan tipografi tag.
pub fn measure_words<'a>(
    words: &[TimedWord],
    stack: &Stack,
    latin_ttf: &'static [u8],
    latin_weight: u16,
    font_source: Option<&'a dyn fframes::FontSource<'a>>,
    fs: f32,
    ls_px: f32,
    uppercase: bool,
) -> Measured {
    measure_words_typed(
        words, stack, latin_ttf, latin_weight, font_source, fs, ls_px, uppercase, &[],
    )
}

pub fn measure_words_typed<'a>(
    words: &[TimedWord],
    stack: &Stack,
    latin_ttf: &'static [u8],
    latin_weight: u16,
    font_source: Option<&'a dyn fframes::FontSource<'a>>,
    fs: f32,
    ls_px: f32,
    uppercase: bool,
    types: &[Option<WordType>],
) -> Measured {
    let latin_upem = fframes::ttf_parser::Face::parse(latin_ttf, 0)
        .expect("template font parses")
        .units_per_em();
    // Slot 0 = latin, 1 = arabic, 2 = hebrew.
    let script_upems = [
        0u16,
        script_metrics(stack.arabic).unwrap().2 as u16,
        script_metrics(stack.hebrew).unwrap().2 as u16,
    ];
    let upems = [latin_upem, script_upems[1], script_upems[2]];
    let families = [stack.latin, stack.arabic, stack.hebrew];
    let mut faces: [Option<Box<dyn fframes::FontFace<'a> + 'a>>; 3] = [None, None, None];
    if let Some(source) = font_source {
        for (i, family) in families.into_iter().enumerate() {
            let weight = if i == 0 { latin_weight } else { native_weight(family) };
            let probe = fframes::FontQuery {
                family,
                size: 1,
                weight,
                ..Default::default()
            };
            // Coba semua nama dalam font-stack: name-table fontsource kadang
            // hanya mengenal alternate ("Poppins Black", "Montserrat Thin", …).
            // Tanpa ini resolve gagal → fallback 0.5em/char → kata tumpang
            // tindih saat dirender dengan lebar asli.
            faces[i] = font_stack_names(family)
                .find_map(|name| source.resolve_font(name, probe.weight, probe.style, probe.stretch));
        }
    }

    let mut widths = Vec::with_capacity(words.len());
    let mut word_families = Vec::with_capacity(words.len());
    for (word_i, word) in words.iter().enumerate() {
        let ty = types.get(word_i).copied().flatten();
        let family = word_family(&word.text, stack);
        let slot = if family == stack.latin {
            0
        } else if family == stack.arabic {
            1
        } else {
            2
        };
        let upem = upems[slot] as usize;
        let display: String = if uppercase {
            word.display.to_uppercase()
        } else {
            word.display.clone()
        };
        let utf16 = display.encode_utf16().count() as f32;
        let fallback = script_fallback_width(&display, fs);
        let base_advance = if slot != 0 {
            shaped_advance(family, &display, fs).unwrap_or(fallback)
        } else {
            match faces[slot].as_deref() {
                Some(face) => match face.font_variant(upem) {
                    Some(variant) => {
                        // Per karakter: resolve face → unit asli; glif tak
                        // dimiliki face (CJK/Thai dsb) → perkiraan sistem.
                        let mut acc = 0.0f32;
                        for ch in display.chars() {
                            let units = match variant {
                                fframes::FontVariant::Monospaced(mono) => Some(mono),
                                fframes::FontVariant::Variable => {
                                    face.resolve_char_width(upem, ch)
                                }
                            };
                            acc += match units {
                                Some(u) => u as f32 * fs / upem as f32,
                                None => fallback_char_advance(ch, fs),
                            };
                        }
                        acc
                    }
                    None => fallback,
                },
                None => fallback,
            }
        };
        // Tier-2: kata latin ber-tag diukur dengan face snap-nya sendiri;
        // keluarga di luar tabel FACES → face dasar dengan advance × scale.
        let advance = match ty {
            Some(t) if slot == 0 => {
                let eff = if t.family.is_empty() { stack.latin } else { t.family };
                let scale_px = fs * t.scale;
                match crate::templates::faces::face_index(eff, t.weight, t.italic) {
                    Some(idx) => {
                        let fupem =
                            crate::templates::faces::face_metrics(idx).map(|m| m.upem as f32);
                        match fupem {
                            Some(fupem) if fupem > 0.0 => {
                                let mut acc = 0.0f32;
                                for ch in display.chars() {
                                    acc += match crate::templates::faces::char_advance_units(idx, ch)
                                    {
                                        Some(u) => u as f32 * scale_px / fupem,
                                        None => fallback_char_advance(ch, scale_px),
                                    };
                                }
                                acc
                            }
                            _ => base_advance * t.scale,
                        }
                    }
                    None => base_advance * t.scale,
                }
            }
            _ => base_advance,
        };
        widths.push(advance + utf16 * ls_px);
        word_families.push(family);
    }
    Measured {
        widths,
        families: word_families,
    }
}

// ── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::pipeline;

    const TEST_POPPINS_TTF: &[u8] =
        include_bytes!("../../assets/poppins-latin-700-normal.ttf");

    fn word(text: &str) -> TimedWord {
        TimedWord::new(text, 0.0, 1000.0)
    }

    fn doc(text: &str) -> CaptionDocument {
        CaptionDocument {
            cues: vec![crate::Cue {
                start_ms: 0,
                end_ms: 3000,
                text: text.into(),
                sheet: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn svg_font_family_appends_name_table_alternates() {
        assert_eq!(svg_font_family("Montserrat"), "Montserrat, Montserrat Thin");
        assert_eq!(
            svg_font_family("Bricolage Grotesque"),
            "Bricolage Grotesque, Bricolage Grotesque 96pt ExtraBold"
        );
        assert_eq!(svg_font_family("Manrope"), "Manrope, Manrope ExtraLight");
        assert_eq!(svg_font_family("Poppins"), "Poppins, Poppins Black");
        // Families whose name-table already matches stay untouched.
        assert_eq!(svg_font_family("Anton"), "Anton");
        assert_eq!(svg_font_family("Inter"), "Inter");
        assert_eq!(svg_font_family("Cairo"), "Cairo");
    }

    #[test]
    fn stack_table_matches_tscaps_catalog() {
        assert_eq!(
            Stack::for_latin("Poppins"),
            Stack {
                latin: "Poppins",
                arabic: FONT_CAIRO,
                hebrew: FONT_ASSISTANT
            }
        );
        assert_eq!(Stack::for_latin("Inter").arabic, FONT_VAZIRMATN);
        assert_eq!(Stack::for_latin("Inter").hebrew, FONT_HEEBO);
        assert_eq!(Stack::for_latin("Anton").arabic, FONT_LALEZAR);
        assert_eq!(Stack::for_latin("Bungee").arabic, FONT_LALEZAR);
        assert_eq!(Stack::for_latin("Playfair Display").arabic, FONT_AMIRI);
        assert_eq!(Stack::for_latin("Lora").arabic, FONT_AMIRI);
        assert_eq!(Stack::for_latin("JetBrains Mono").hebrew, FONT_HEEBO);
        // Unknown / default pair.
        assert_eq!(Stack::for_latin("Something New").arabic, FONT_CAIRO);
        assert_eq!(Stack::for_latin("Something New").hebrew, FONT_ASSISTANT);
    }

    #[test]
    fn missing_glyphs_estimate_system_fallback_widths() {
        // CJK/kana/hangul & fullwidth → 1em penuh (lebar konsisten antar font).
        assert_eq!(fallback_char_advance('欢', 10.0), 10.0);
        assert_eq!(fallback_char_advance('한', 10.0), 10.0);
        assert_eq!(fallback_char_advance('あ', 10.0), 10.0);
        assert_eq!(fallback_char_advance('\u{FF21}', 10.0), 10.0); // fullwidth A
        // Thai dasar ≈0.62em; tanda gabung nol.
        assert_eq!(fallback_char_advance('ก', 10.0), 6.2);
        assert_eq!(fallback_char_advance('\u{0E48}', 10.0), 0.0); // mai ek
        // Latin/generic tetap konvensi lama 0.5em.
        assert_eq!(fallback_char_advance('a', 10.0), 5.0);
        // Total: 4 ideograph = 4.0em.
        assert_eq!(script_fallback_width("欢迎测试", 10.0), 40.0);
        // Mixed: latin + CJK.
        assert_eq!(script_fallback_width("r字", 10.0), 15.0);
    }

    #[test]
    fn word_family_by_script() {
        let stack = Stack::for_latin("Poppins");
        assert_eq!(word_family("hello", &stack), "Poppins");
        assert_eq!(word_family("مرحبا", &stack), FONT_CAIRO);
        assert_eq!(word_family("שלום", &stack), FONT_ASSISTANT);
        let inter = Stack::for_latin("Inter");
        assert_eq!(word_family("مرحبا", &inter), FONT_VAZIRMATN);
        assert_eq!(word_family("rscaps", &inter), "Inter");
    }

    #[test]
    fn dominant_family_majority() {
        let stack = Stack::for_latin("Poppins");
        let mixed: Vec<TimedWord> = ["Welcome", "to", "مرحبا", "بالعالم"]
            .iter()
            .map(|t| word(t))
            .collect();
        assert_eq!(dominant_family(&mixed, &stack), FONT_CAIRO);
        let hebrew: Vec<TimedWord> = ["שלום", "עולם"].iter().map(|t| word(t)).collect();
        assert_eq!(dominant_family(&hebrew, &stack), FONT_ASSISTANT);
        let latin: Vec<TimedWord> = ["Welcome", "to", "rscaps"].iter().map(|t| word(t)).collect();
        assert_eq!(dominant_family(&latin, &stack), "Poppins");
    }

    #[test]
    fn document_direction_majority() {
        assert_eq!(
            detect_document_direction(&doc("مرحبا بالعالم من rscaps")),
            Direction::Rtl
        );
        assert_eq!(
            detect_document_direction(&doc("Welcome to rscaps")),
            Direction::Ltr
        );
    }

    #[test]
    fn line_direction_flip_rules() {
        assert_eq!(
            resolve_line_direction("مرحبا بالعالم", Direction::Rtl),
            Direction::Rtl
        );
        // Single-direction line opposite to the declared direction flips.
        assert_eq!(
            resolve_line_direction("Welcome", Direction::Rtl),
            Direction::Ltr
        );
        // Mixed line keeps the declared direction.
        assert_eq!(
            resolve_line_direction("Welcome مرحبا", Direction::Ltr),
            Direction::Ltr
        );
        assert_eq!(resolve_line_direction("123", Direction::Rtl), Direction::Rtl);
    }

    #[test]
    fn script_metrics_upems() {
        let (_, _, cairo_upem) = script_metrics(FONT_CAIRO).unwrap();
        assert!(cairo_upem > 0.0);
        assert!(script_metrics(FONT_VAZIRMATN).is_some());
        assert!(script_metrics(FONT_LALEZAR).unwrap().2 > 0.0);
        assert!(script_metrics("Poppins").is_none());
    }

    #[test]
    fn measure_fallback_and_families() {
        let stack = Stack::for_latin("Poppins");
        let words: Vec<TimedWord> = ["Welcome", "to", "مرحبا"]
            .iter()
            .map(|t| word(t))
            .collect();
        // font_source None -> latin falls back to utf16 * fs * 0.5; the
        // Arabic word ignores font_source and shapes with embedded Cairo
        // (cursive joining — narrower than the per-char isolated sum).
        let m = measure_words(
            &words,
            &stack,
            TEST_POPPINS_TTF,
            700,
            None,
            40.0,
            0.0,
            false,
        );
        assert_eq!(
            m.families,
            vec!["Poppins", "Poppins", FONT_CAIRO]
        );
        assert!((m.widths[0] - 7.0 * 40.0 * 0.5).abs() < 1e-3);
        let isolated_sum = 5.0 * 40.0 * 0.5; // old per-char fallback
        assert!(m.widths[2] > 0.0 && m.widths[2] < isolated_sum);
        assert_eq!(m.widths[2], shaped_advance(FONT_CAIRO, "مرحبا", 40.0).unwrap());
    }

    #[test]
    fn shaped_arabic_matches_canvas_shaped_width() {
        // Browser canvas measureText("بالعالم") @ 700 64.8px Vazirmatn
        // returns 173.2px; the isolated per-char sum is 258.8px.
        let w = shaped_advance(FONT_VAZIRMATN, "بالعالم", 64.8).unwrap();
        assert!((w - 173.2).abs() < 2.0, "shaped={w}");
    }

    #[test]
    fn pipeline_segments_carry_direction() {
        let cfg = pipeline::SplitConfig {
            boundary: pipeline::Boundary::Sentence,
            max_chars: 26,
            min_chars: 5,
            max_gap_ms: 1000.0,
            effects: &[],
        };
        let segments = pipeline::collect_segments(&doc("مرحبا بالعالم من rscaps"), &cfg);
        assert!(!segments.is_empty());
        assert!(segments.iter().all(|s| s.direction == Direction::Rtl));
    }
}
