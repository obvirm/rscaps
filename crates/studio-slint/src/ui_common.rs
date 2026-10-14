// Glue UI bersama cross-platform (WASM + native). TANPA dependensi web_sys /
// wasm_bindgen — hanya caption-core (pure Rust) + format string.
// Dipakai app.rs (jalur WASM) dan platform/native.rs (jalur native).
// LIHAT AGENTS.md — shared module jangan di-hardcode per-platform.

/// detik -> "m:ss" (paritas tscaps).
pub fn fmt_dur(sec: f64) -> String {
    let s = sec.max(0.0).floor() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// detik -> "m:ss.mmm" (playbar presisi ms).
pub fn fmt_dur_ms(sec: f64) -> String {
    let s = sec.max(0.0);
    let m = (s / 60.0).floor() as i64;
    format!("{}:{:06.3}", m, s - m as f64 * 60.0)
}

/// Teks cue aktif pada t_ms dari dokumen caption-core (kosong = tak ada).
pub fn active_cue_at(t_ms: u32) -> String {
    let Ok(doc) = caption_core::document().read() else {
        return String::new();
    };
    for cue in &doc.cues {
        if cue.start_ms <= t_ms && t_ms < cue.end_ms {
            return cue.text.clone();
        }
    }
    String::new()
}

/// Index galeri 1-based (0 = Default v0) -> id template caption-core.
pub fn template_id(idx: i32) -> String {
    if idx <= 0 {
        return String::new();
    }
    caption_core::templates::template_catalog()
        .get(idx as usize - 1)
        .map(|entry| entry.id.to_string())
        .unwrap_or_default()
}

/// Item galeri satu seksi: (nama-tampil, idx-1-based, row, col).
/// Sorting: galleryOrder dulu (null = akhir), lalu label alfabet. Filter case-insensitive.
/// Konversi ke tipe Slint (TemplateItem) dilakukan masing-masing platform.
pub fn section_rows(section: &str, query: &str) -> Vec<(String, i32, i32, i32)> {
    let mut entries: Vec<(usize, &caption_core::templates::TemplateEntry)> =
        caption_core::templates::template_catalog()
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.category == section)
            .collect();
    entries.sort_by_key(|(_, entry)| (entry.gallery_order.unwrap_or(u32::MAX), entry.label));
    let q = query.to_lowercase();
    entries
        .into_iter()
        .filter(|(_, entry)| q.is_empty() || entry.label.to_lowercase().contains(&q))
        .enumerate()
        .map(|(pos, (i, entry))| {
            (
                entry.label.to_string(),
                (i + 1) as i32,
                (pos / 3) as i32,
                (pos % 3) as i32,
            )
        })
        .collect()
}
