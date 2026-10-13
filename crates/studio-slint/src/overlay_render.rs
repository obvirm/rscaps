//! Overlay caption asli: paritas branch main.
//!
//! main: `bridge.render_frame(BigInt(f))` -> string SVG -> `{@html}` di atas
//! `<video>` (preview) atau `<style>` @font-face base64 inline -> raster
//! (export, state.svelte.ts). Sini preview = SVG-as-image, jadi font WAJIB
//! inline (resource eksternal dilarang di mode image) - pola export main.
//!
//! Sini: `WasmEditor<CaptionVideo>` IN-PROCESS (caption-core rlib - satu memori
//! dokumen dengan 20 call sites yang masih rlib; pakai module caption-bridge
//! utk overlay = dokumen terbelah). `render_frame` -> inline @font-face utk
//! family yang dipakai svg -> `slint::Image::load_from_svg_data` (wasm:
//! browser render via HTMLImage, i-slint-core graphics/image.rs - tajam di
//! ukuran tampilan, tanpa JS authored).
//!
//! Sumber fakta: fframes-editor-controller 1.2.0 impl_wasm_bridge_macro.rs /
//! wasm_editor.rs (prepare/ingest_font/render_frame), caption-core lib.rs:210.

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
    // (@font-face family, blok @font-face base64) utk family template yang
    // sudah ter-ingest; diisi preload_tplfonts. Disaring per svg agar frame
    // animasi tak menyalin seluruh 30-an font tiap render.
    static FONT_FACES: RefCell<Vec<(String, String)>> = RefCell::new(Vec::new());
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

/// Dokumen diganti (import/transcribe/undo/redo/pilih template): prepare()
/// wajib ulang sebelum render_frame berikutnya (time_base/duration ikut
/// berubah - wasm_editor.rs:247 expect tanpa prepare).
pub fn invalidate() {
    DOC_DIRTY.with(|d| d.store(true, Ordering::SeqCst));
}

/// Daftarkan TTF ke fframes (pengukuran text_break_lines) + simpan @font-face
/// utk inline ke SVG (browser yang menggambar glyph). Pola file
/// `<family>-<script>-<weight>-normal.ttf` (TPLFONTS).
pub fn ingest_font(file: &str, bytes: &[u8]) {
    EDITOR.with(|e| {
        let _ = e.ingest_font(bytes);
    });
    if let Some((family, weight)) = parse_face(file) {
        let b64 = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        };
        let block = format!(
            "@font-face{{font-family:\"{family}\";font-weight:{weight};src:url(data:font/ttf;base64,{b64});}}"
        );
        FONT_FACES.with(|v| v.borrow_mut().push((family, block)));
    }
}

/// Font dengan family TIDAK terbaca dari nama file (font sistem: file
/// `ariblk.ttf` tapi family "Arial Black" - paritas vite.config.ts fontRoot).
pub fn ingest_system_font(family: &str, weight: u16, bytes: &[u8]) {
    EDITOR.with(|e| {
        let _ = e.ingest_font(bytes);
    });
    let b64 = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    };
    let block = format!(
        "@font-face{{font-family:\"{family}\";font-weight:{weight};src:url(data:font/ttf;base64,{b64});}}"
    );
    FONT_FACES.with(|v| v.borrow_mut().push((family.to_string(), block)));
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
    // Cue statis: svg identik -> pertahankan Image (tanpa blob/decode ulang).
    let same = LAST_SVG.with(|s| *s.borrow() == svg);
    if same {
        return;
    }
    let inlined = inline_fonts(svg);
    match slint::Image::load_from_svg_data(inlined.as_bytes()) {
        Ok(img) => {
            LAST_SVG.with(|s| *s.borrow_mut() = svg.to_string());
            ui.set_overlay_image(img);
            ui.set_overlay_live(true);
            LIVE.with(|l| l.set(true));
        }
        Err(_) => {
            log("overlay: load_from_svg_data failed");
            ui.set_overlay_live(false);
            LIVE.with(|l| l.set(false));
        }
    }
}

/// Sisip `<style>@font-face…</style>` tepat setelah tag `<svg …>` pembuka -
/// pola sama dengan main (state.svelte.ts: `raw.replace(/^<svg[^>]*>/, …)`).
/// Wajib data: URI - SVG sbg image/blob dilarang memuat resource eksternal
/// (alasan main juga inline saat export). Blok difilter: hanya family yang
/// disebut svg (render_frame per frame -> salinan kecil, bukan ~3 MB).
fn inline_fonts(svg: &str) -> String {
    let blocks: String = FONT_FACES
        .with(|v| {
            v.borrow()
                .iter()
                .filter(|(fam, _)| svg.contains(fam.as_str()))
                .map(|(_, b)| b.as_str())
                .collect::<Vec<_>>()
                .join("")
        });
    if blocks.is_empty() {
        return svg.to_string();
    }
    let s = svg.trim_start();
    let Some(gt) = s.find('>') else {
        return svg.to_string();
    };
    let mut out = String::with_capacity(s.len() + blocks.len() + 16);
    out.push_str(&s[..=gt]);
    out.push_str("<style>");
    out.push_str(&blocks);
    out.push_str("</style>");
    out.push_str(&s[gt + 1..]);
    out
}
