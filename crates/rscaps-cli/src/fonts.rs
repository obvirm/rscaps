//! Font source untuk render headless: fontdb sistem + folder template,
//! resolve keluarga font → lebar glyph asli (ttf hmtx), supaya `measure_words`
//! di caption-core mengukur sama persis dengan jalur browser (WASM).
//!
//! Pola trait sama dengan `fframes::RendererFontSource` (yang tidak bisa
//! dieksternal karena field `pub(crate)`), diimplementasi ulang dari trait
//! publik `FontSource`/`FontFace`.

//! …
use std::path::Path;
use std::sync::Arc;

use fframes::{FontFace, FontSource, FontStyle, FontStretch};
use svgr::usvgr::fontdb::{self, Family, Query, Source, Style, Weight};

/// Lokasi manifest font relatif CWD (satu sumber dengan studio web).
pub const MANIFEST_RELPATH: &str = "studio/src/lib/fontManifest.json";

/// Logger `log` minimal (stderr) — hanya terpasang saat env
/// `RSCAPS_FONT_DEBUG=1`; memunculkan pesan fallback font usvgr
/// ("Fallback from Anton to Cairo") untuk diagnostik paritas glif.
pub fn init_font_debug_logger() {
    struct StderrLogger;
    static LOGGER: StderrLogger = StderrLogger;
    impl log::Log for StderrLogger {
        fn enabled(&self, meta: &log::Metadata) -> bool {
            meta.level() <= log::Level::Warn
        }
        fn log(&self, record: &log::Record) {
            eprintln!("[usvgr] {} {}", record.level(), record.args());
        }
        fn flush(&self) {}
    }
    if std::env::var("RSCAPS_FONT_DEBUG").as_deref() == Ok("1") {
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Warn);
    }
}

#[derive(Debug)]
pub struct CliFontSource {
    db: fontdb::Database,
}

impl CliFontSource {
    pub fn new() -> Self {
        Self {
            db: fontdb::Database::new(),
        }
    }

    pub fn load_system(&mut self) {
        self.db.load_system_fonts();
    }

    /// Muat font PERSIS seperti studio web: baca `fontManifest.json` (satu
    /// sumber dengan `studio/src/lib/fontManifest.ts`) dan muat tiap file
    /// dalam urutan manifest. Urutan db menentukan fallback glif usvgr —
    /// pemilih fallback mengambil face PERTAMA yang mencakup karakter — jadi
    /// paritas raster non-latin (Arab/Ibrani/dll.) menuntut urutan identik.
    ///
    /// Pemetaan path (sama dengan plugin static-dirs vite.config.ts):
    /// `/fonts/X` → `C:\Windows\Fonts\X`, `/tplfonts/X` → `<repo>/studio/tplfonts/X`.
    /// Return jumlah file termuat.
    pub fn load_manifest(&mut self, manifest_path: &Path) -> usize {
        let Ok(text) = std::fs::read_to_string(manifest_path) else {
            eprintln!(
                "[fonts] WARN manifest font tidak ketemu: {} — fallback non-latin bisa berbeda dari web",
                manifest_path.display()
            );
            return 0;
        };
        let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
            eprintln!("[fonts] WARN manifest font tidak valid: {}", manifest_path.display());
            return 0;
        };
        let mut loaded = 0usize;
        for entry in &entries {
            let Some(url) = entry["path"].as_str() else { continue };
            let Some(file) = manifest_url_to_file(url) else {
                eprintln!("[fonts] WARN path manifest tak dikenal: {url}");
                continue;
            };
            if !file.is_file() {
                eprintln!("[fonts] WARN file manifest hilang: {}", file.display());
                continue;
            }
            match std::fs::read(&file) {
                Ok(bytes) => {
                    self.db.load_font_data(bytes);
                    loaded += 1;
                }
                Err(err) => eprintln!("[fonts] WARN baca {} gagal: {err}", file.display()),
            }
        }
        loaded
    }

    pub fn load_dir(&mut self, path: impl AsRef<Path>) {
        self.db.load_fonts_dir(path);
    }

    /// Database yang sama untuk rasterizer SVG (usvgr text→path).
    pub fn database(&self) -> &fontdb::Database {
        &self.db
    }
}

impl Default for CliFontSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Pemetaan URL manifest → file di disk (cermin plugin static-dirs vite).
/// `/fonts/X` = font sistem Windows (arial/ariblk), `/tplfonts/X` = font
/// template repo. Return None untuk URL lain (di-skip dengan peringatan).
pub fn manifest_url_to_file(url: &str) -> Option<std::path::PathBuf> {
    if let Some(rest) = url.strip_prefix("/fonts/") {
        return Some(Path::new("C:\\Windows\\Fonts").join(rest));
    }
    if let Some(rest) = url.strip_prefix("/tplfonts/") {
        // Relatif terhadap CWD (konvensi sama dengan candidate_font_dirs).
        return Some(std::path::PathBuf::from("studio/tplfonts").join(rest));
    }
    None
}

/// Daftar file font sesuai urutan `fontManifest.json` (dipakai jalur fframes
/// yang mengumpulkan bytes font mentah, bukan fontdb). `extra` = folder
/// tambahan --fonts (di-scan setelah manifest, sebagai suplemen).
pub fn manifest_font_files(
    manifest_path: &Path,
    extra: Option<&Path>,
) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(manifest_path) {
        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            for entry in &entries {
                let Some(url) = entry["path"].as_str() else { continue };
                if let Some(file) = manifest_url_to_file(url) {
                    if file.is_file() {
                        out.push(file);
                    }
                }
            }
        }
    } else {
        eprintln!(
            "[fonts] WARN manifest font tidak ketemu: {} — fallback non-latin bisa berbeda dari web",
            manifest_path.display()
        );
    }
    if let Some(dir) = extra {
        if dir.is_dir() {
            collect_dir_ttf(dir, &mut out);
        }
    }
    out
}

/// Kumpulkan *.ttf/*.otf dari folder (non-rekursif, urutan nama) — pelengkap
/// --fonts.
fn collect_dir_ttf(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("ttf") | Some("otf") | Some("ttc")
            )
        })
        .collect();
    files.sort();
    out.extend(files);
}

impl<'a> FontSource<'a> for CliFontSource {
    fn add_font(&mut self, _filename: String, font_data: Arc<dyn AsRef<[u8]> + Sync + Send>) {
        self.db.load_font_source(Source::Binary(font_data));
    }

    fn resolve_font(
        &'a self,
        font_name: &str,
        font_weight: u16,
        font_style: FontStyle,
        font_stretch: FontStretch,
    ) -> Option<Box<dyn FontFace<'a> + 'a>> {
        let id = self.db.query(&Query {
            families: &[Family::Name(font_name)],
            weight: Weight(font_weight),
            style: match font_style {
                FontStyle::Normal => Style::Normal,
                FontStyle::Italic => Style::Italic,
                FontStyle::Oblique => Style::Oblique,
            },
            stretch: match font_stretch {
                FontStretch::ExtraCondensed => fontdb::Stretch::ExtraCondensed,
                FontStretch::UltraCondensed => fontdb::Stretch::UltraCondensed,
                FontStretch::Condensed => fontdb::Stretch::Condensed,
                FontStretch::SemiCondensed => fontdb::Stretch::SemiCondensed,
                FontStretch::Normal => fontdb::Stretch::Normal,
                FontStretch::SemiExpanded => fontdb::Stretch::SemiExpanded,
                FontStretch::Expanded => fontdb::Stretch::Expanded,
                FontStretch::ExtraExpanded => fontdb::Stretch::ExtraExpanded,
                FontStretch::UltraExpanded => fontdb::Stretch::UltraExpanded,
            },
        })?;

        let (source, index) = self.db.face_source(id)?;
        let data: Arc<dyn AsRef<[u8]> + Send + Sync> = match source {
            Source::Binary(data) | Source::SharedFile(_, data) => data,
            Source::File(path) => Arc::new(std::fs::read(path).ok()?),
        };
        Some(Box::new(CliFont { data, index }))
    }
}

struct CliFont {
    data: Arc<dyn AsRef<[u8]> + Send + Sync>,
    index: u32,
}

impl std::fmt::Debug for CliFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliFont").field("index", &self.index).finish()
    }
}

impl<'a> FontFace<'a> for CliFont {
    fn is_monospaced(&self) -> Option<bool> {
        let face = fframes::ttf_parser::Face::parse(self.data.as_ref().as_ref(), self.index).ok()?;
        Some(face.is_monospaced())
    }

    fn resolve_char_width(&self, font_size: usize, c: char) -> Option<usize> {
        let face = fframes::ttf_parser::Face::parse(self.data.as_ref().as_ref(), self.index).ok()?;
        let glyph = face.glyph_index(c)?;
        Some(
            font_size * face.tables().hmtx?.advance(glyph)? as usize
                / face.units_per_em() as usize,
        )
    }
}
