#![cfg(target_arch = "wasm32")]

use caption_core::CaptionVideo;
use fframes_editor_controller::prelude::*;

impl_wasm_bridge_for!(CaptionVideo);

#[wasm_bindgen]
pub fn create_wasm_bridge() -> WasmBridge {
    console_error_panic_hook::set_once();
    WasmBridge::new(CaptionVideo)
}

#[wasm_bindgen]
pub fn set_document_json(json: &str) -> Result<(), JsValue> {
    caption_core::set_document_json(json).map_err(|err| JsValue::from_str(&err))
}

#[wasm_bindgen]
pub fn replace_cues_json(json: &str) -> Result<(), JsValue> {
    caption_core::replace_cues_json(json).map_err(|err| JsValue::from_str(&err))
}

/// Daftar kata dokumen aktif (JSON) untuk element inspector per-kata.
#[wasm_bindgen]
pub fn list_words_json() -> String {
    caption_core::list_words_json()
}

/// Params JSON → CSS subset (panel Code settings).
#[wasm_bindgen]
pub fn params_to_css(params_json: &str) -> Result<String, JsValue> {
    caption_core::params_to_css_json(params_json).map_err(|err| JsValue::from_str(&err))
}

/// CSS subset → params JSON (merge ke base; field di luar CSS dipertahankan).
#[wasm_bindgen]
pub fn params_from_css(base_params_json: &str, css: &str) -> Result<String, JsValue> {
    caption_core::params_from_css_json(base_params_json, css).map_err(|err| JsValue::from_str(&err))
}

/// Parse an SRT/VTT file → JSON `{"duration_frames", "cues"}`. Pure; the
/// caller merges the result into the document.
#[wasm_bindgen]
pub fn parse_subtitle(text: &str, format: &str) -> Result<String, JsValue> {
    caption_core::parse_subtitle_json(text, format).map_err(|err| JsValue::from_str(&err))
}

/// Cues JSON (`[Cue, …]`) → subtitle file text (`format` is `"srt"`/`"vtt"`).
#[wasm_bindgen]
pub fn format_subtitle(cues_json: &str, format: &str) -> Result<String, JsValue> {
    caption_core::format_subtitle_json(cues_json, format).map_err(|err| JsValue::from_str(&err))
}

/// The one template catalog: JSON `[{"id": "ada", "label": "Ada"}, …]`.
/// Consumers must show this instead of hardcoding their own template list.
#[wasm_bindgen]
pub fn template_catalog_json() -> String {
    caption_core::template_catalog_json()
}

/// Canonical empty document as JSON (default params, no cues). The one
/// source of caption defaults for every consumer.
#[wasm_bindgen]
pub fn default_document_json() -> String {
    caption_core::default_document_json()
}
