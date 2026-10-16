//! Bridge svgr (usvgr + tiny-skia) ke WASM — raster caption di WASM,
//! menggantikan raster canvas browser saat export web.
//!
//! Filosofi: fframes/svgr TIDAK diubah. Crate ini hanya "membandel"
//! (membungkus) svgr agar bisa dipanggil dari JS. Alur export baru:
//!
//! 1. WebCodecs decode frame video → RGBA            [browser, hanya I/O]
//! 2. caption-core WASM → string SVG                 [WASM, sudah ada]
//! 3. svgr-wasm: SVG + frame → pixel komposit        [WASM, KRA - fungsi ini]
//! 4. WebCodecs encode                               [browser, hanya I/O]
//!
//! State (fontdb + cache) persist antar-panggilan via thread-local,
//! supaya layout teks tidak dihitung ulang tiap frame.

use std::cell::RefCell;
use svgr::usvgr::fontdb;
use wasm_bindgen::prelude::*;

thread_local! {
    static FONTDB: RefCell<fontdb::Database> = RefCell::new(fontdb::Database::new());
    static CACHE: RefCell<Option<svgr::SvgrCache>> = RefCell::new(None);
    static POOL: RefCell<svgr::PixmapPool> = RefCell::new(svgr::PixmapPool::new());
}

/// Load satu font (bytes TTF/OTF/TTC) ke fontdb global.
/// Dipanggil sekali per font saat inisialisasi export.
#[wasm_bindgen]
pub fn load_font(data: &[u8]) {
    FONTDB.with(|db| db.borrow_mut().load_font_data(data.to_vec()));
}

/// Jumlah font yang sudah dimuat (untuk verifikasi).
#[wasm_bindgen]
pub fn font_count() -> usize {
    FONTDB.with(|db| db.borrow().faces().count())
}

/// Raster string SVG → pixels RGBA transparan (width×height×4 bytes).
///
/// Hasilnya layer caption di atas latar transparan — belum dikomposit
/// dengan video. Untuk komposit sekali panggil pakai [`burn_frame`].
#[wasm_bindgen]
pub fn raster_svg(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, JsValue> {
    let pixmap = render_svg_to_pixmap(svg, width, height)
        .map_err(|err| JsValue::from_str(&format!("raster svg: {err}")))?;
    Ok(pixmap.data().to_vec())
}

/// Satu frame burn-in penuh di WASM: komposit frame video (base_rgba)
/// dengan caption SVG, kembalikan pixels RGBA hasil komposit.
///
/// - `base_rgba`: pixels frame video (RGBA, `width*height*4` bytes) dari WebCodecs
/// - `svg`: string SVG caption (dari caption-core)
/// - `blend_svg`: layer opsional `mix-blend-mode: difference` ("" = tanpa)
///
/// Komposit dilakukan di WASM - browser hanya memindahkan pixels
/// (putImageData), TIDAK me-raster SVG.
#[wasm_bindgen]
pub fn burn_frame(
    base_rgba: &[u8],
    svg: &str,
    blend_svg: &str,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, JsValue> {
    let expected = width as usize * height as usize * 4;
    if base_rgba.len() != expected {
        return Err(JsValue::from_str(&format!(
            "base_rgba length {} != expected {}",
            base_rgba.len(),
            expected
        )));
    }

    // Salin base → out (kita kerjakan in-place di out).
    let mut out = base_rgba.to_vec();

    // Layer 1: caption utama → alpha-blend source-over ke out.
    if !svg.trim().is_empty() {
        let layer = render_svg_to_pixmap(svg, width, height)
            .map_err(|err| JsValue::from_str(&format!("raster caption: {err}")))?;
        alpha_blend_over(&mut out, layer.data(), expected);
    }

    // Layer 2: blend difference (milo/luca/luna) → difference terhadap out.
    if !blend_svg.trim().is_empty() {
        let layer = render_svg_to_pixmap(blend_svg, width, height)
            .map_err(|err| JsValue::from_str(&format!("raster blend: {err}")))?;
        difference_blend(&mut out, layer.data(), expected);
    }

    Ok(out)
}

// ── internal: raster SVG → Pixmap ──────────────────────────────────────────

fn render_svg_to_pixmap(svg: &str, width: u32, height: u32) -> Result<svgr::tiny_skia::Pixmap, String> {
    let fontdb = FONTDB.with(|db| db.borrow().clone());
    let opt = svgr::usvgr::Options::default();
    let tree = svgr::usvgr::Tree::from_str(svg, &opt, &fontdb)
        .map_err(|err| format!("parse: {err}"))?;

    let mut pixmap = svgr::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| format!("alokasi pixmap {width}x{height}"))?;

    // Scale tree → pixmap (mirip drawImage(img, 0, 0, outW, outH) di browser:
    // stretch non-uniform bila ukuran beda, mis. output di-downscale).
    let ts = tree.size();
    let transform = if ts.width() > 0.0 && ts.height() > 0.0
        && (ts.width() != width as f32 || ts.height() != height as f32)
    {
        svgr::tiny_skia::Transform::from_row(
            width as f32 / ts.width(),
            0.0,
            0.0,
            height as f32 / ts.height(),
            0.0,
            0.0,
        )
    } else {
        svgr::tiny_skia::Transform::identity()
    };

    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = Some(svgr::SvgrCache::new(4));
        }
        let cache = c.as_mut().unwrap();
        POOL.with(|p| {
            let pool = p.borrow();
            let ctx = svgr::Context::new_from_pixmap(&pixmap);
            svgr::render(
                &tree,
                transform,
                &mut pixmap.as_mut(),
                cache,
                &pool,
                &ctx,
            );
        });
    });

    Ok(pixmap)
}

// ── internal: komposit pixel ───────────────────────────────────────────────

/// Source-over alpha blend: layer (premultiplied RGBA dari tiny-skia)
/// di atas base (straight RGBA dari WebCodecs).
/// tiny-skia menghasilkan premultiplied - konversi dulu ke straight.
fn alpha_blend_over(base: &mut [u8], layer: &[u8], len: usize) {
    for i in (0..len).step_by(4) {
        let la = layer[i + 3];
        if la == 0 {
            continue;
        }
        // Premultiplied → straight.
        let inv = 255.0 / la as f32;
        let lr = (layer[i] as f32 * inv).min(255.0) as u8;
        let lg = (layer[i + 1] as f32 * inv).min(255.0) as u8;
        let lb = (layer[i + 2] as f32 * inv).min(255.0) as u8;

        let ba = base[i + 3] as f32 / 255.0;
        let la_f = la as f32 / 255.0;
        let out_a = la_f + ba * (1.0 - la_f);
        if out_a <= 0.0 {
            continue;
        }
        let out_a255 = out_a * 255.0;
        let blend = |bc: u8, lc: u8| -> u8 {
            let bv = bc as f32 * ba;
            let lv = lc as f32 * la_f;
            (((lv + bv * (1.0 - la_f)) / out_a) * 255.0 + 0.5).min(255.0) as u8
        };
        base[i] = blend(base[i], lr);
        base[i + 1] = blend(base[i + 1], lg);
        base[i + 2] = blend(base[i + 2], lb);
        base[i + 3] = out_a255 as u8;
    }
}

/// Difference blend (mix-blend-mode: difference) layer di atas base.
/// Sama seperti ctx.globalCompositeOperation = "difference" di canvas.
fn difference_blend(base: &mut [u8], layer: &[u8], len: usize) {
    for i in (0..len).step_by(4) {
        let la = layer[i + 3];
        if la == 0 {
            continue;
        }
        // Premultiplied → straight.
        let inv = 255.0 / la as f32;
        let lr = (layer[i] as f32 * inv).min(255.0) as u8;
        let lg = (layer[i + 1] as f32 * inv).min(255.0) as u8;
        let lb = (layer[i + 2] as f32 * inv).min(255.0) as u8;

        let la_f = la as f32 / 255.0;
        // Difference color = |base - layer|.
        let diff = |bc: u8, lc: u8| -> u8 {
            (bc as i16 - lc as i16).unsigned_abs().min(255) as u8
        };
        // Compositing: out = (1-la)*base + la*diff  (backdrop opaque, alpha=1).
        let comp = |bc: u8, lc: u8| -> u8 {
            let b = bc as f32;
            let d = diff(bc, lc) as f32;
            ((1.0 - la_f) * b + la_f * d + 0.5).min(255.0) as u8
        };
        base[i] = comp(base[i], lr);
        base[i + 1] = comp(base[i + 1], lg);
        base[i + 2] = comp(base[i + 2], lb);
    }
}
