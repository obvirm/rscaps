//! Registry gambar `data:` URL untuk raster native (usvgr).
//!
//! usvgr TIDAK men-decode `data:` URL — `<image href>` di-resolve dari
//! `Options.image_data` dengan kunci PERSIS string href. Kontraknya sama
//! dengan svgr-wasm `register_image`: host mendaftarkan piksel RGBA
//! straight ter-decode, kunci = atribut href utuh di SVG.
//!
//! Pemakaian:
//! - [`ensure_svg_images`]: pindai SVG, decode sekali per href (global,
//!   idempoten, thread-safe — lane raster rayon bisa memanggil bersamaan).
//! - [`snapshot`]: salin registry (clone Arc murah) untuk `Options.image_data`.
//!
//! Kaca selene (`;rscaps-glass`) SENGAJA dilewati: konten data URL-nya
//! hanyalah kunci/pengganti — web men-suplai piksel frame hidup di bawah
//! kunci itu (burnWorker/ensureInlineImages), CLI fframes menulis ulang
//! href ke frame video terdaftar (`frame_N`, lihat fframes_engine). Kaca
//! = frame penuh kualitas penuh di kedua jalur, tanpa decode JPEG.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine as _;
use svgr::usvgr::PreloadedImageData;

/// Awalan atribut href gambar data URL (unit pencarian lengkap, supaya teks
/// isi caption yang kebetulan memuat "data:image/" tidak ikut terpindai).
const HREF_START: &str = "href=\"data:image/";

fn registry() -> &'static Mutex<HashMap<String, Arc<PreloadedImageData>>> {
    static IMAGES: OnceLock<Mutex<HashMap<String, Arc<PreloadedImageData>>>> = OnceLock::new();
    IMAGES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Pindai SVG untuk semua `href="data:image/…;base64,…"` → decode
/// (PNG/JPEG/WebP/GIF) → daftarkan dengan kunci href persis. Decode sekali
/// per href. Kaca selene (`rscaps-glass`) dilewati — lihat docs modul.
pub fn ensure_svg_images(svg: &str) -> usize {
    let mut added = 0;
    let mut rest = svg;
    while let Some(pos) = rest.find(HREF_START) {
        let after = &rest[pos + "href=\"".len()..];
        // Data URL base64 tidak memuat `"` — penutup atribut = akhir href.
        let Some(end) = after.find('"') else { break };
        let href = &after[..end];
        rest = &after[end..];
        if href.contains("rscaps-glass") {
            continue;
        }
        if registry().lock().expect("image registry").contains_key(href) {
            continue;
        }
        match decode_data_url(href) {
            Ok(img) => {
                registry()
                    .lock()
                    .expect("image registry")
                    .insert(href.to_string(), Arc::new(img));
                added += 1;
            }
            Err(err) => {
                // Gambar tak ter-decode → elemen <image> kosong di raster
                // (mask stroke iris hilang). Raster tetap jalan, sisanya
                // caption normal.
                eprintln!("[images] WARN gagal decode gambar caption: {err}");
            }
        }
    }
    added
}

/// Salin registry untuk `Options.image_data` (clone Arc murah; snapshot
/// dibuang setelah parse — lock tidak ditahan selama raster).
pub fn snapshot() -> HashMap<String, Arc<PreloadedImageData>> {
    registry().lock().expect("image registry").clone()
}

/// Decode `data:image/…;base64,…` → `PreloadedImageData` RGBA straight.
fn decode_data_url(href: &str) -> Result<PreloadedImageData, String> {
    let b64 = href
        .split("base64,")
        .nth(1)
        .ok_or_else(|| "bukan data URL base64".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64: {e}"))?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("decode gambar: {e}"))?;
    let rgba = img.to_rgba8();
    Ok(PreloadedImageData::new(
        href.to_string(),
        rgba.width(),
        rgba.height(),
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    #[test]
    fn decode_register_and_reuse() {
        // PNG betulan (2x1: merah opaque + hijau transparan) via image crate.
        let mut px = image::RgbaImage::new(2, 1);
        px.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        px.put_pixel(1, 0, image::Rgba([0, 255, 0, 0]));
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(px.as_raw(), 2, 1, image::ExtendedColorType::Rgba8)
            .expect("png encode");
        let href = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let svg = format!("<image href=\"{href}\" x=\"0\" y=\"0\"/>");

        assert_eq!(ensure_svg_images(&svg), 1, "decode pertama");
        assert_eq!(ensure_svg_images(&svg), 0, "href sama = decode-once");

        let snap = snapshot();
        let img = snap.get(&href).expect("terdaftar");
        assert_eq!((img.width, img.height), (2, 1));
        // Piksel merah opaque terbawa apa adanya (straight RGBA).
        assert_eq!(&img.data[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn glass_marker_is_skipped() {
        let svg = "<image href=\"data:image/jpeg;rscaps-glass;base64,AAAA\" x=\"0\" y=\"0\"/>";
        assert_eq!(ensure_svg_images(svg), 0, "kaca tidak lewat registry");
        assert!(snapshot().iter().all(|(k, _)| !k.contains("rscaps-glass")));
    }

    #[test]
    fn broken_payload_does_not_panic() {
        let n_before = snapshot().len();
        let svg = "<image href=\"data:image/png;base64,%%%%bukan-base64%%%%\" x=\"0\"/>";
        let _ = ensure_svg_images(svg);
        assert_eq!(snapshot().len(), n_before, "gagal decode tak mendaftar");
    }
}
