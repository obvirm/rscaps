//! Font source untuk render headless: fontdb sistem + folder template,
//! resolve keluarga font → lebar glyph asli (ttf hmtx), supaya `measure_words`
//! di caption-core mengukur sama persis dengan jalur browser (WASM).
//!
//! Pola trait sama dengan `fframes::RendererFontSource` (yang tidak bisa
//! dieksternal karena field `pub(crate)`), diimplementasi ulang dari trait
//! publik `FontSource`/`FontFace`.

use std::path::Path;
use std::sync::Arc;

use fframes::{FontFace, FontSource, FontStyle, FontStretch};
use svgr::usvgr::fontdb::{self, Family, Query, Source, Style, Weight};

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
