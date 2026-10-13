//! Overlay caption: paritas main, tapi raster via engine fframes sendiri.
//!
//! main: `bridge.render_frame(BigInt(f))` -> string SVG -> `{@html}` di atas
//! `<video>` (browser raster SVG). Sini: `WasmEditor<CaptionVideo>` in-process
//! -> SVG string -> `usvgr::Tree::from_str` -> `svgr::render` (tiny-skia,
//! engine yang SAMA dengan export fframes) -> `Pixmap` -> `slint::Image`.
//! Bukan `load_from_svg_data` (browser HTMLImage raster).
//!
//! Font: TTF di-ingest ke WasmEditor (text_break_lines) + disimpan utk
//! `usvgr::fontdb::Database` (glyph raster oleh tiny-skia, bukan browser).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use caption_core::CaptionVideo;
use fframes_editor_controller::WasmEditor;
use slint::ComponentHandle;

use crate::{log, App};

// Nomor urut render (lintas thread_local - dipakai langsung, bukan LocalKey).
static GEN: AtomicU32 = const { AtomicU32::new(0) };

thread_local! {
    static EDITOR: Rc<WasmEditor<CaptionVideo, ()>> =
        Rc::new(WasmEditor::new(CaptionVideo, &()));
    // TTF bytes per family+weight utk usvgr fontdb (glyph raster tiny-skia).
    static FONT_DATA: RefCell<Vec<(String, u16, Vec<u8>)>> = RefCell::new(Vec::new());
    // fontdb di-build lazy dari FONT_DATA; di-cache karena build mahal.
    static FONTDB: RefCell<Option<Rc<svgr::usvgr::fontdb::Database>>> = RefCell::new(None);
    // svgr raster state di-cache antar frame ( pixmap + SvgrCache + PixmapPool ).
    static SVGR_STATE: RefCell<Option<SvgrState>> = RefCell::new(None);
    // Satu render async dalam terbang + frame terbaru yg diminta (coalesce).
    static BUSY: AtomicBool = const { AtomicBool::new(false) };
    static PENDING: RefCell<Option<(slint::Weak<App>, i32)>> = RefCell::new(None);
    // Dokumen berubah (set_document_json/replace_cues) -> prepare ulang dulu.
    static DOC_DIRTY: AtomicBool = const { AtomicBool::new(true) };
    // Cache: svg persis sama -> Image lama dipakai ulang (tak ada blob/texture
    // churn di pump saat cue statis).
    static LAST_SVG: RefCell<String> = RefCell::new(String::new());
    static LIVE: Cell<bool> = const { Cell::new(false) };
}

/// State raster svgr yang di-cache antar frame (reuse pixmap + LRU cache).
struct SvgrState {
    pixmap: Option<svgr::tiny_skia::Pixmap>,
    cache: svgr::SvgrCache,
    pool: svgr::PixmapPool,
}

impl SvgrState {
    fn new() -> Self {
        Self {
            pixmap: None,
            cache: svgr::SvgrCache::new(20),
            pool: svgr::PixmapPool::new(),
        }
    }
}

/// Dokumen diganti (import/transcribe/undo/redo/pilih template): prepare()
/// wajib ulang sebelum render_frame berikutnya (time_base/duration ikut
/// berubah - wasm_editor.rs:247 expect tanpa prepare).
pub fn invalidate() {
    DOC_DIRTY.with(|d| d.store(true, Ordering::SeqCst));
}

/// Ganti project / dokumen baru total: buang cache SVG + flag live supaya
/// overlay berikutnya pasti render ulang & pasang Image baru (tanpa ini,
/// SVG identik = early-return tanpa set overlay-live, caption hilang).
pub fn reset(ui: &App) {
    DOC_DIRTY.with(|d| d.store(true, Ordering::SeqCst));
    LAST_SVG.with(|s| s.borrow_mut().clear());
    PENDING.with(|p| *p.borrow_mut() = None);
    LIVE.with(|l| l.set(false));
    ui.set_overlay_live(false);
}

/// Daftarkan TTF ke fframes (pengukuran text_break_lines) + simpan utk
/// usvgr fontdb (glyph raster). Pola file
/// `<family>-<script>-<weight>-normal.ttf` (TPLFONTS).
pub fn ingest_font(file: &str, bytes: &[u8]) {
    EDITOR.with(|e| {
        let _ = e.ingest_font(bytes);
    });
    if let Some((family, weight)) = parse_face(file) {
        FONT_DATA.with(|v| v.borrow_mut().push((family, weight, bytes.to_vec())));
        FONTDB.with(|d| *d.borrow_mut() = None); // invalidate
    }
}

/// Font dengan family TIDAK terbaca dari nama file (font sistem: file
/// `ariblk.ttf` tapi family "Arial Black" - paritas vite.config.ts fontRoot).
pub fn ingest_system_font(family: &str, weight: u16, bytes: &[u8]) {
    EDITOR.with(|e| {
        let _ = e.ingest_font(bytes);
    });
    FONT_DATA.with(|v| v.borrow_mut().push((family.to_string(), weight, bytes.to_vec())));
    FONTDB.with(|d| *d.borrow_mut() = None);
}

/// `poppins-latin-700-normal.ttf` -> ("Poppins", 700).
fn parse_face(file: &str) -> Option<(String, u16)> {
    let name = file.strip_suffix(".ttf")?;
    let script = ["-latin-", "-arabic-", "-hebrew-"]
        .iter()
        .find(|s| name.contains(**s))?;
    let idx = name.find(script)?;
    let fam_raw = &name[..idx];
    let weight = name[idx + script.len()..].split('-').next()?.parse().ok()?;
    let family = fam_raw
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some((family, weight))
}

/// Build (atau ambil cache) usvgr fontdb dari FONT_DATA. Rc clone murah.
fn fontdb() -> Rc<svgr::usvgr::fontdb::Database> {
    FONTDB.with(|slot| {
        if slot.borrow().is_none() {
            let mut db = svgr::usvgr::fontdb::Database::new();
            FONT_DATA.with(|fonts| {
                for (_fam, _weight, bytes) in fonts.borrow().iter() {
                    let _ = db.load_font_data(bytes.clone());
                }
            });
            *slot.borrow_mut() = Some(Rc::new(db));
        }
        slot.borrow().clone().expect("fontdb built")
    })
}

/// Minta overlay frame `frame` dirender & dipasang ke UI. Coalesce: hanya satu
/// async dalam terbang; permintaan terbaru menang.
pub fn request(ui: &App, frame: i32) {
    GEN.fetch_add(1, Ordering::SeqCst);
    PENDING.with(|p| *p.borrow_mut() = Some((ui.as_weak(), frame)));
    pump();
}

fn pump() {
    if BUSY.with(|b| b.swap(true, Ordering::SeqCst)) {
        return; // ada yang terbang; ia akan mem-pump ulang (PENDING terisi)
    }
    let Some((weak, frame)) = PENDING.with(|p| p.borrow_mut().take()) else {
        BUSY.with(|b| b.store(false, Ordering::SeqCst));
        return;
    };
    let render_gen = GEN.load(Ordering::SeqCst);
    wasm_bindgen_futures::spawn_local(async move {
        let ed = EDITOR.with(|e| e.clone());
        // prepare dulu bila dokumen baru/berubah (render_frame panic tanpa
        // time_base - wasm_editor.rs:247).
        if DOC_DIRTY.with(|d| d.swap(false, Ordering::SeqCst)) {
            let _ = ed.prepare(Some(30)).await;
        }
        let svg = ed.render_frame(frame as i64);
        let fresh = GEN.load(Ordering::SeqCst) == render_gen;
        if fresh {
            if let Some(ui) = weak.upgrade() {
                apply_svg(&ui, &svg);
            }
        }
        BUSY.with(|b| b.store(false, Ordering::SeqCst));
        if PENDING.with(|p| p.borrow().is_some()) {
            pump();
        }
    });
}

fn apply_svg(ui: &App, svg: &str) {
    if svg.trim().is_empty() {
        if LIVE.with(|l| l.get()) {
            ui.set_overlay_live(false);
            LIVE.with(|l| l.set(false));
            LAST_SVG.with(|s| s.borrow_mut().clear());
        }
        return;
    }
    // Cue statis: svg identik -> pertahankan Image (tanpa raster ulang).
    let same = LAST_SVG.with(|s| *s.borrow() == svg);
    if same {
        return;
    }

    // Raster via engine fframes: usvgr parse -> svgr::render (tiny-skia)
    // -> Pixmap. Ini rasterizer yang SAMA dengan export; bukan browser.
    let (w, h) = (1920u32, 1080u32); // CaptionVideo::WIDTH/HEIGHT
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        raster_svg(svg, w, h)
    }));

    match result {
        Ok(Some(img)) => {
            LAST_SVG.with(|s| *s.borrow_mut() = svg.to_string());
            ui.set_overlay_image(img);
            ui.set_overlay_live(true);
            LIVE.with(|l| l.set(true));
        }
        Ok(None) => {
            log("overlay: raster_svg failed");
            ui.set_overlay_live(false);
            LIVE.with(|l| l.set(false));
        }
        Err(_) => {
            log("overlay: raster_svg panic");
            ui.set_overlay_live(false);
            LIVE.with(|l| l.set(false));
        }
    }
}

/// Parse SVG string -> usvgr tree -> raster tiny-skia -> slint::Image.
/// Background transparan (overlay di atas video).
fn raster_svg(svg: &str, width: u32, height: u32) -> Option<slint::Image> {
    let db = fontdb();
    let options = svgr::usvgr::Options {
        font_family: "Arial".into(),
        ..Default::default()
    };
    let tree = svgr::usvgr::Tree::from_str(svg, &options, &db).ok()?;

    SVGR_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let state = state.get_or_insert_with(SvgrState::new);

        // Ambil / buat pixmap ukuran target.
        let pixmap = match state.pixmap.take() {
            Some(p) if p.width() == width && p.height() == height => p,
            _ => svgr::tiny_skia::Pixmap::new(width, height)?,
        };
        let mut pixmap = pixmap;

        // Background transparan.
        pixmap.fill(svgr::tiny_skia::Color::from_rgba8(0, 0, 0, 0));

        // Transform: scale tree viewbox ke ukuran pixmap (paritas
        // fframes fit_transform di preview.rs).
        let ts = tree.view_box().to_transform(tree.size());
        let sx = width as f32 / tree.size().width();
        let sy = height as f32 / tree.size().height();
        let scale = svgr::tiny_skia::Transform::from_scale(sx, sy);
        let transform = scale.pre_concat(ts);

        let ctx = svgr::Context::new_from_pixmap(&pixmap);
        svgr::render(
            &tree,
            transform,
            &mut pixmap.as_mut(),
            &mut state.cache,
            &state.pool,
            &ctx,
        );

        // Ambil pixel ( premultiplied RGBA ) -> un-premultiply -> straight RGBA8.
        let mut pixels = pixmap.data().to_vec();
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a != 255 && a != 0 {
                for c in &mut px[..3] {
                    *c = ((u32::from(*c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
                }
            }
        }

        // Simpan pixmap balik utk reuse.
        state.pixmap = Some(pixmap);

        // Slint SharedPixelBuffer Rgba8Pixel = straight RGBA per pixel.
        let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
        buffer.make_mut_bytes().copy_from_slice(&pixels);
        Some(slint::Image::from_rgba8(buffer))
    })
}
