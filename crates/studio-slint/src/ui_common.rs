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

// ═════════════════════════════════════════════════════════════════════════
// DocParams global ↔ caption-core — wiring bersama WASM + native.
// ═════════════════════════════════════════════════════════════════════════
#[cfg(any(target_arch = "wasm32", feature = "native"))]
mod params {
    use slint::ComponentHandle;

    #[cfg(target_arch = "wasm32")]
    use crate::app::{App, DocParams, StyleColorItem, StyleFloatItem};
    #[cfg(all(not(target_arch = "wasm32"), feature = "native"))]
    use crate::platform::native::{App, DocParams, StyleColorItem, StyleFloatItem};

    pub fn hex_to_slint_color(hex: &str) -> slint::Color {
        let h = hex.trim_start_matches('#');
        let parse = |i: usize| -> u8 {
            u8::from_str_radix(h.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255)
        };
        slint::Color::from_rgb_u8(parse(0), parse(2), parse(4))
    }

    pub fn slint_color_to_hex(c: slint::Color) -> String {
        let c = c.to_argb_u8();
        format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue)
    }

    /// Push live document params into the DocParams global (panel display).
    pub fn sync_params_global(ui: &App) {
        let Ok(doc) = caption_core::document().read() else { return };
        let p = &doc.params;
        let g = ui.global::<DocParams>();
        g.set_font_size(p.font_size);
        g.set_font_weight(p.font_weight as i32);
        g.set_letter_spacing(p.letter_spacing_em.unwrap_or(0.0));
        g.set_word_spacing(p.word_spacing_em.unwrap_or(0.0));
        g.set_line_spacing(p.line_spacing_em.unwrap_or(0.0));
        g.set_italic(p.italic.unwrap_or(false));
        g.set_underline(p.underline.unwrap_or(false));
        g.set_strikethrough(p.strikethrough.unwrap_or(false));
        g.set_text_case(match p.text_case.as_deref() {
            Some("uppercase") => 1,
            Some("lowercase") => 2,
            _ => 0,
        });
        g.set_text_align(match p.text_align.as_deref() {
            Some("left") => 0,
            Some("right") => 2,
            _ => 1,
        });
        g.set_color(hex_to_slint_color(&p.color));
        // Position
        let v_idx = match p.v_align.as_deref() {
            Some("top") => 0,
            Some("bottom") => 2,
            _ => 1,
        };
        let h_idx = match p.h_align.as_deref() {
            Some("left") => 0,
            Some("right") => 2,
            _ => 1,
        };
        if p.v_align.is_some() || p.h_align.is_some() {
            g.set_anchor(v_idx * 3 + h_idx);
        }
        g.set_angle(p.angle_deg.unwrap_or(0.0));
        // Effects
        g.set_fx_gap_free(p.effects.gap_free.unwrap_or(false));
        g.set_fx_remove_punct(p.effects.remove_punctuation.unwrap_or(false));
        g.set_fx_smart_punct(p.effects.smart_punctuation.unwrap_or(false));
        g.set_fx_carry_quotes(p.effects.carry_quotes.unwrap_or(false));
        g.set_fx_smart_lowercase(p.effects.smart_lowercase.unwrap_or(false));
        g.set_fx_emoji_placement(match p.effects.emoji_placement.as_deref() {
            Some("segment-above") => 1,
            Some("word") => 2,
            _ => 0,
        });
        g.set_fx_emoji_size(p.effects.emoji_size.unwrap_or(1.8));
        g.set_fx_emoji_gap(p.effects.emoji_gap.unwrap_or(0.1));
        // Layout (0 = template default)
        g.set_max_lines(p.max_lines.unwrap_or(0) as i32);
        g.set_max_width_frac(p.max_width_frac.unwrap_or(0.0));
        g.set_max_chars(p.max_chars.unwrap_or(0) as i32);
        // Motion
        g.set_motion_static(p.motion_preset.as_deref() == Some("static"));
        g.set_motion_duration_ms(p.motion_duration_ms.unwrap_or(0) as i32);
        g.set_motion_distance_em(p.motion_distance_em.unwrap_or(0.0));
    }

    /// Wire every DocParams change callback to `patch_params_json`.
    /// - `before(ui)` dipanggil SEBELUM patch (WASM: push undo snapshot).
    /// - `refresh(ui)` dipanggil sesudah patch sukses (invalidate/render).
    pub fn wire_doc_params<F1, F2>(ui: &App, before: F1, refresh: F2)
    where
        F1: Fn(&App) + 'static,
        F2: Fn(&App) + 'static,
    {
        use std::rc::Rc;
        let before: Rc<F1> = Rc::new(before);
        let refresh: Rc<F2> = Rc::new(refresh);

        // Patch params dengan JSON hasil `$fmt(v)`, lalu refresh.
        macro_rules! patch {
            ($method:ident, $fmt:expr) => {{
                let weak = ui.as_weak();
                let before = before.clone();
                let refresh = refresh.clone();
                ui.global::<DocParams>().$method(move |v| {
                    let Some(ui) = weak.upgrade() else { return };
                    let json = ($fmt)(v);
                    before(&ui);
                    if let Err(e) = caption_core::patch_params_json(&json) {
                        ui.set_status(format!("params patch failed: {e}").into());
                        ui.set_status_is_error(true);
                        return;
                    }
                    refresh(&ui);
                });
            }};
        }
        // Reset slice params (kembalikan default), sync global, refresh.
        macro_rules! reset {
            ($method:ident, $clear:expr) => {{
                let weak = ui.as_weak();
                let before = before.clone();
                let refresh = refresh.clone();
                ui.global::<DocParams>().$method(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    before(&ui);
                    if let Ok(mut doc) = caption_core::document().write() {
                        ($clear)(&mut doc.params);
                    }
                    sync_params_global(&ui);
                    sync_style_controls(&ui);
                    refresh(&ui);
                });
            }};
        }

        // ── Typography ──
        patch!(on_font_size_changed, |v: f32| format!(r#"{{"font_size": {v}}}"#));
        patch!(on_font_weight_changed, |v: i32| format!(r#"{{"font_weight": {v}}}"#));
        patch!(on_letter_spacing_changed, |v: f32| format!(r#"{{"letter_spacing_em": {v}}}"#));
        patch!(on_word_spacing_changed, |v: f32| format!(r#"{{"word_spacing_em": {v}}}"#));
        patch!(on_line_spacing_changed, |v: f32| format!(r#"{{"line_spacing_em": {v}}}"#));
        patch!(on_italic_changed, |v: bool| format!(r#"{{"italic": {v}}}"#));
        patch!(on_underline_changed, |v: bool| format!(r#"{{"underline": {v}}}"#));
        patch!(on_strikethrough_changed, |v: bool| format!(r#"{{"strikethrough": {v}}}"#));
        patch!(on_text_case_changed, |v: i32| {
            let s = match v { 1 => "uppercase", 2 => "lowercase", _ => "none" };
            format!(r#"{{"text_case": "{s}"}}"#)
        });
        patch!(on_text_align_changed, |v: i32| {
            let s = match v { 0 => "left", 2 => "right", _ => "center" };
            format!(r#"{{"text_align": "{s}"}}"#)
        });
        patch!(on_color_changed, |v: slint::Color| {
            format!(r#"{{"color": "{}"}}"#, slint_color_to_hex(v))
        });

        // ── Position ──
        patch!(on_anchor_changed, |v: i32| {
            let row = v / 3;
            let col = v % 3;
            let va = match row { 0 => "top", 2 => "bottom", _ => "center" };
            let ha = match col { 0 => "left", 2 => "right", _ => "center" };
            format!(r#"{{"v_align": "{va}", "h_align": "{ha}", "v_offset": 0.0, "h_offset": 0.0}}"#)
        });
        patch!(on_v_offset_changed, |v: f32| format!(r#"{{"v_offset": {v}}}"#));
        patch!(on_h_offset_changed, |v: f32| format!(r#"{{"h_offset": {v}}}"#));
        patch!(on_angle_changed, |v: f32| format!(r#"{{"angle_deg": {v}}}"#));

        // ── Effects ──
        patch!(on_fx_gap_free_changed, |v: bool| format!(r#"{{"effects": {{"gap_free": {v}}}}}"#));
        patch!(on_fx_remove_punct_changed, |v: bool| format!(r#"{{"effects": {{"remove_punctuation": {v}}}}}"#));
        patch!(on_fx_smart_punct_changed, |v: bool| format!(r#"{{"effects": {{"smart_punctuation": {v}}}}}"#));
        patch!(on_fx_carry_quotes_changed, |v: bool| format!(r#"{{"effects": {{"carry_quotes": {v}}}}}"#));
        patch!(on_fx_smart_lowercase_changed, |v: bool| format!(r#"{{"effects": {{"smart_lowercase": {v}}}}}"#));
        patch!(on_fx_emoji_placement_changed, |v: i32| {
            let s = match v { 1 => "segment-above", 2 => "word", _ => "segment-below" };
            format!(r#"{{"effects": {{"emoji_placement": "{s}"}}}}"#)
        });
        patch!(on_fx_emoji_size_changed, |v: f32| format!(r#"{{"effects": {{"emoji_size": {v}}}}}"#));
        patch!(on_fx_emoji_gap_changed, |v: f32| format!(r#"{{"effects": {{"emoji_gap": {v}}}}}"#));

        // ── Layout ──
        patch!(on_max_lines_changed, |v: i32| format!(r#"{{"max_lines": {v}}}"#));
        patch!(on_max_width_frac_changed, |v: f32| format!(r#"{{"max_width_frac": {v}}}"#));
        patch!(on_max_chars_changed, |v: i32| format!(r#"{{"max_chars": {v}}}"#));

        // ── Style (data-driven): tulis per-key ke style_values (merge) ──
        {
            let weak = ui.as_weak();
            let before = before.clone();
            let refresh = refresh.clone();
            ui.global::<DocParams>()
                .on_style_color_changed(move |id: slint::SharedString, v: slint::Color| {
                    let Some(ui) = weak.upgrade() else { return };
                    before(&ui);
                    let json = format!(
                        r#"{{"style_values": {{"{id}": "{}"}}}}"#,
                        slint_color_to_hex(v)
                    );
                    if let Err(e) = caption_core::patch_params_json(&json) {
                        ui.set_status(format!("style patch failed: {e}").into());
                        ui.set_status_is_error(true);
                        return;
                    }
                    sync_style_controls(&ui);
                    refresh(&ui);
                });
        }
        {
            let weak = ui.as_weak();
            let before = before.clone();
            let refresh = refresh.clone();
            ui.global::<DocParams>()
                .on_style_float_changed(move |id: slint::SharedString, v: f32| {
                    let Some(ui) = weak.upgrade() else { return };
                    before(&ui);
                    let json = format!(r#"{{"style_values": {{"{id}": {v}}}}}"#);
                    if let Err(e) = caption_core::patch_params_json(&json) {
                        ui.set_status(format!("style patch failed: {e}").into());
                        ui.set_status_is_error(true);
                        return;
                    }
                    sync_style_controls(&ui);
                    refresh(&ui);
                });
        }
        // Variant pilih → apply semua override varian + pilih indeks.
        {
            let weak = ui.as_weak();
            let before = before.clone();
            let refresh = refresh.clone();
            ui.global::<DocParams>().on_variant_chosen(move |i: i32| {
                let Some(ui) = weak.upgrade() else { return };
                let tid: String = caption_core::document()
                    .read()
                    .map(|d| d.template.clone())
                    .unwrap_or_default();
                let raw = caption_core::templates::variants_json(&tid);
                let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else { return };
                let Some(list) = val.as_array() else { return };
                let Some(v) = list.get(i.max(0) as usize) else { return };
                let Some(overrides) = v.get("overrides") else { return };
                before(&ui);
                let json = format!(r#"{{"style_values": {overrides}}}"#);
                let _ = caption_core::patch_params_json(&json);
                sync_style_controls(&ui);
                ui.global::<DocParams>().set_variant_selected(i);
                refresh(&ui);
            });
        }
        // Reset style: buang semua style_values → sync control → refresh.
        reset!(on_reset_style, |p: &mut caption_core::CaptionParams| {
            p.style_values.clear();
        });

        // ── Motion ──
        patch!(on_motion_static_changed, |v: bool| {
            let s = if v { "static" } else { "" };
            format!(r#"{{"motion_preset": "{s}"}}"#)
        });
        patch!(on_motion_duration_changed, |v: i32| {
            if v <= 0 {
                r#"{"motion_duration_ms": null}"#.to_string()
            } else {
                format!(r#"{{"motion_duration_ms": {v}}}"#)
            }
        });
        patch!(on_motion_distance_changed, |v: f32| {
            if v <= 0.0 {
                r#"{"motion_distance_em": null}"#.to_string()
            } else {
                format!(r#"{{"motion_distance_em": {v}}}"#)
            }
        });
        reset!(on_reset_motion, |p: &mut caption_core::CaptionParams| {
            p.motion_preset = None;
            p.motion_duration_ms = None;
            p.motion_distance_em = None;
        });

        // ── Resets (effets dipertahankan saat reset typography) ──
        reset!(on_reset_typography, |p: &mut caption_core::CaptionParams| {
            let fx = p.effects.clone();
            *p = caption_core::CaptionParams { effects: fx, ..Default::default() };
        });
        reset!(on_reset_position, |p: &mut caption_core::CaptionParams| {
            let d = caption_core::CaptionParams::default();
            p.h_align = None;
            p.h_offset = None;
            p.v_align = None;
            p.v_offset = None;
            p.y_frac = d.y_frac;
            p.angle_deg = None;
        });
        reset!(on_reset_effects, |p: &mut caption_core::CaptionParams| {
            p.effects = Default::default();
        });
        reset!(on_reset_layout, |p: &mut caption_core::CaptionParams| {
            p.max_lines = None;
            p.max_width_frac = None;
            p.max_chars = None;
        });
    }

    /// Push the Style tab's data-driven models (controls + variants) for the
    /// ACTIVE template into DocParams. Values = style_values > control default.
    pub fn sync_style_controls(ui: &App) {
        use slint::{ModelRc, SharedString, VecModel};
        let template_id: String = caption_core::document()
            .read()
            .map(|d| d.template.clone())
            .unwrap_or_default();
        let controls_raw = caption_core::templates::style_controls_json(&template_id);
        let variants_raw = caption_core::templates::variants_json(&template_id);
        let style_values = caption_core::document()
            .read()
            .ok()
            .map(|d| d.params.style_values.clone())
            .unwrap_or_default();

        let parse = |raw: &str| -> Vec<serde_json::Value> {
            serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        };

        let mut colors = Vec::new();
        let mut floats = Vec::new();
        for c in parse(controls_raw) {
            let (Some(id), Some(label), Some(ty)) = (
                c.get("id").and_then(|v| v.as_str()),
                c.get("label").and_then(|v| v.as_str()),
                c.get("type").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            match ty {
                "color" => {
                    let default = c
                        .get("default")
                        .and_then(|v| v.as_str())
                        .unwrap_or("#ffffff");
                    let value = style_values
                        .get(id)
                        .and_then(|v| v.as_str())
                        .unwrap_or(default);
                    colors.push(StyleColorItem {
                        id: SharedString::from(id),
                        label: SharedString::from(label),
                        value: hex_to_slint_color(value),
                    });
                }
                "float" | "integer" => {
                    let default = c
                        .get("default")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let value = style_values
                        .get(id)
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(default);
                    floats.push(StyleFloatItem {
                        id: SharedString::from(id),
                        label: SharedString::from(label),
                        value,
                        min: c.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                        max: c.get("max").and_then(|v| v.as_f64()).unwrap_or(100.0) as f32,
                        step: c
                            .get("step")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(if ty == "integer" { 1.0 } else { 0.05 })
                            as f32,
                        unit: SharedString::from(
                            c.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
                        ),
                    });
                }
                _ => {}
            }
        }
        let variant_labels: Vec<SharedString> = parse(variants_raw)
            .iter()
            .map(|v| {
                SharedString::from(v.get("label").and_then(|x| x.as_str()).unwrap_or(""))
            })
            .collect();

        let g = ui.global::<DocParams>();
        g.set_style_colors(ModelRc::new(VecModel::from(colors)));
        g.set_style_floats(ModelRc::new(VecModel::from(floats)));
        g.set_style_variants(ModelRc::new(VecModel::from(variant_labels)));
        // Variant terpilih tidak dipersist (override sudah di style_values).
        g.set_variant_selected(-1);
    }

    /// Push the cue list (display strings + count) into the DocParams global.
    pub fn sync_cue_list(ui: &App) {        use slint::SharedString;
        let Ok(doc) = caption_core::document().read() else { return };
        let rows: Vec<SharedString> = doc
            .cues
            .iter()
            .map(|c| {
                let s = super::fmt_dur_ms(c.start_ms as f64 / 1000.0);
                let e = super::fmt_dur_ms(c.end_ms as f64 / 1000.0);
                SharedString::from(format!("{s} – {e}   {}", c.text))
            })
            .collect();
        let n = doc.cues.len() as i32;
        let g = ui.global::<DocParams>();
        g.set_cue_count(n);
        g.set_cue_list(slint::ModelRc::new(slint::VecModel::from(rows)));
        // Batasi seleksi yang mungkin.
        if g.get_cue_selected() >= n {
            g.set_cue_selected(-1);
        }
    }

    /// Transcript callbacks: pilih/edit/insert/delete cue → document.
    /// `before` dipanggil SEBELUM mutasi (undo snapshot); `refresh` sesudah.
    pub fn wire_doc_cues<F1, F2>(ui: &App, before: F1, refresh: F2)
    where
        F1: Fn(&App) + 'static,
        F2: Fn(&App) + 'static,
    {
        use slint::SharedString;
        use std::rc::Rc;
        let before: Rc<F1> = Rc::new(before);
        let refresh: Rc<F2> = Rc::new(refresh);

        // Muat field editor dari cue terpilih (dipakai after select/list sync).
        macro_rules! load_cue {
            ($ui:expr, $idx:expr) => {{
                let g = $ui.global::<DocParams>();
                if let Ok(doc) = caption_core::document().read() {
                    if let Some(c) = doc.cues.get($idx as usize) {
                        g.set_cue_text(SharedString::from(c.text.as_str()));
                        g.set_cue_start(c.start_ms as f32 / 1000.0);
                        g.set_cue_end(c.end_ms as f32 / 1000.0);
                    }
                }
            }};
        }
        macro_rules! mut_cue {
            ($body:expr) => {{
                let weak = ui.as_weak();
                let before = before.clone();
                let refresh = refresh.clone();
                move || {
                    let Some(ui) = weak.upgrade() else { return };
                    before(&ui);
                    let idx = ui.global::<DocParams>().get_cue_selected();
                    if let Ok(mut doc) = caption_core::document().write() {
                        if (idx as usize) < doc.cues.len() {
                            ($body)(idx as usize, &mut doc.cues);
                        }
                    }
                    sync_cue_list(&ui);
                    refresh(&ui);
                }
            }};
        }

        // Pilih cue → muat field editor.
        {
            let weak = ui.as_weak();
            ui.global::<DocParams>().on_cue_selected_changed(move |i| {
                let Some(ui) = weak.upgrade() else { return };
                load_cue!(ui, i);
            });
        }
        // Edit teks.
        {
            let weak = ui.as_weak();
            let before = before.clone();
            let refresh = refresh.clone();
            ui.global::<DocParams>().on_cue_text_edited(move |t: SharedString| {
                let Some(ui) = weak.upgrade() else { return };
                before(&ui);
                let idx = ui.global::<DocParams>().get_cue_selected();
                if let Ok(mut doc) = caption_core::document().write() {
                    if let Some(c) = doc.cues.get_mut(idx as usize) {
                        c.text = t.to_string();
                    }
                }
                refresh(&ui);
            });
        }
        // Edit waktu (detik → ms, clamp end > start).
        {
            let weak = ui.as_weak();
            let before = before.clone();
            let refresh = refresh.clone();
            ui.global::<DocParams>().on_cue_start_changed(move |v: f32| {
                let Some(ui) = weak.upgrade() else { return };
                before(&ui);
                let idx = ui.global::<DocParams>().get_cue_selected();
                if let Ok(mut doc) = caption_core::document().write() {
                    if let Some(c) = doc.cues.get_mut(idx as usize) {
                        let ms = (v * 1000.0).round().max(0.0) as u32;
                        c.start_ms = ms.min(c.end_ms.saturating_sub(1));
                    }
                }
                load_cue!(ui, idx);
                refresh(&ui);
            });
        }
        {
            let weak = ui.as_weak();
            let before = before.clone();
            let refresh = refresh.clone();
            ui.global::<DocParams>().on_cue_end_changed(move |v: f32| {
                let Some(ui) = weak.upgrade() else { return };
                before(&ui);
                let idx = ui.global::<DocParams>().get_cue_selected();
                if let Ok(mut doc) = caption_core::document().write() {
                    if let Some(c) = doc.cues.get_mut(idx as usize) {
                        let ms = (v * 1000.0).round().max(0.0) as u32;
                        c.end_ms = ms.max(c.start_ms + 1);
                    }
                }
                load_cue!(ui, idx);
                refresh(&ui);
            });
        }
        // Insert after seleksi (durasi 1s dari akhir cue / berikutnya).
        ui.global::<DocParams>().on_cue_insert_after(mut_cue!(
            |i: usize, cues: &mut Vec<caption_core::Cue>| {
                let start = cues[i].end_ms;
                let end = cues
                    .get(i + 1)
                    .map(|c| c.start_ms.min(start + 1000))
                    .unwrap_or(start + 1000)
                    .max(start + 1);
                cues.insert(
                    i + 1,
                    caption_core::Cue {
                        start_ms: start,
                        end_ms: end,
                        text: String::new(),
                    },
                );
            }
        ));
        // Hapus seleksi.
        ui.global::<DocParams>().on_cue_delete(mut_cue!(
            |i: usize, cues: &mut Vec<caption_core::Cue>| {
                cues.remove(i);
            }
        ));
    }
}

#[cfg(any(target_arch = "wasm32", feature = "native"))]
pub use params::{
    hex_to_slint_color, slint_color_to_hex, sync_cue_list, sync_params_global,
    sync_style_controls, wire_doc_cues, wire_doc_params,
};
