//! Aksesor gaya data-driven: `params.style_values` + default dari
//! `template.json` (via REGISTRY `styleControls[].default`).
//!
//! Satu-satunya jembatan antara kontrol di panel Style dan render template.
//! Template memanggil `Style::of(&doc.params, "id")` lalu membaca tiap
//! kontrol dengan fallback konstanta lokal — default SEBENARNYA diambil
//! dari template.json sehingga tak ada duplikasi sumber kebenaran.
//!
//! Prioritas nilai: `style_values[id]` (user) → `styleControls[].default`
//! (template.json) → `fallback` (konstanta render, safety net).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::CaptionParams;

type DefaultsMap = Arc<HashMap<String, serde_json::Value>>;

/// Parse `styleControls` (JSON array) sekali per template id, cache statis.
fn defaults_for(template_id: &str) -> DefaultsMap {
    static CACHE: OnceLock<Mutex<HashMap<String, DefaultsMap>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache.lock().expect("style defaults cache");
        if let Some(hit) = guard.get(template_id) {
            return Arc::clone(hit);
        }
    }
    let raw = crate::templates::style_controls_json(template_id);
    let map: HashMap<String, serde_json::Value> = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|control| {
                        let id = control.get("id")?.as_str()?.to_string();
                        let default = control.get("default")?.clone();
                        Some((id, default))
                    })
                    .collect()
            })
        })
        .unwrap_or_default();
    let mut guard = cache.lock().expect("style defaults cache");
    let map = Arc::new(map);
    guard.insert(template_id.to_string(), Arc::clone(&map));
    map
}

fn value_as_str(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn value_as_f32(value: &serde_json::Value) -> Option<f32> {
    match value {
        serde_json::Value::Number(n) => n.as_f64().map(|f| f as f32),
        serde_json::Value::String(s) => s.trim().parse::<f32>().ok(),
        _ => None,
    }
}

fn value_as_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0),
        _ => None,
    }
}

/// Pembaca `params.style_values` untuk SATU template render.
pub struct Style<'a> {
    values: &'a serde_json::Map<String, serde_json::Value>,
    defaults: DefaultsMap,
}

impl<'a> Style<'a> {
    pub fn of(params: &'a CaptionParams, template_id: &str) -> Self {
        Style {
            values: &params.style_values,
            defaults: defaults_for(template_id),
        }
    }

    fn raw(&self, id: &str) -> Option<&serde_json::Value> {
        self.values.get(id).or_else(|| self.defaults.get(id))
    }

    /// String control: user → default json → fallback.
    pub fn str(&self, id: &str, fallback: &str) -> String {
        self.raw(id)
            .and_then(value_as_str)
            .unwrap_or_else(|| fallback.to_string())
    }

    /// Angka (float/integer) control.
    pub fn f32(&self, id: &str, fallback: f32) -> f32 {
        self.raw(id)
            .and_then(value_as_f32)
            .unwrap_or(fallback)
    }

    /// Toggle control.
    pub fn bool(&self, id: &str, fallback: bool) -> bool {
        self.raw(id)
            .and_then(value_as_bool)
            .unwrap_or(fallback)
    }

    /// Alias warna (string heks) — sama dengan `str` tapi menegaskan intent.
    pub fn color(&self, id: &str, fallback: &str) -> String {
        self.str(id, fallback)
    }
}

/// Kata mengandung tanda kutip — semantik tag `quote` tscaps (kelas `.quote`
/// menempel di SELURUH kata, bukan per-huruf).
pub fn word_has_quote(word: &str) -> bool {
    word.chars()
        .any(|c| matches!(c, '"' | '\'' | '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{AB}' | '\u{BB}'))
}
