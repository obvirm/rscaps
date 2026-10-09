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
