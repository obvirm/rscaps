//! Fase 3 glue (staged): thin async wrappers over the two standalone WASM
//! modules (`whisper-spike/pkg`, `caption-bridge/pkg`).
//!
//! Contract: `index.html` publishes the loaded modules on the global scope as
//! `globalThis.__rscaps_whisper` and `globalThis.__rscaps_bridge` before the
//! app starts. All values cross the boundary as strings / opaque JsValues so
//! this crate no longer needs the rlib sources, only these wrappers.
//!
//! Status: additive-only. The 20 existing call sites still use the rlib deps;
//! flipping them to these wrappers (+ loader ordering in index.html) is the
//! remaining Fase 3 step. Document read/write/template access has NO bridge
//! getter yet — see PLAN Fase 3 kickoff.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::JsValue;
use wasm_bindgen_futures::JsFuture;

#[allow(dead_code)]
async fn sleep_100() {
    let p = js_sys::Promise::new(&mut |yes, _no| {
        web_sys::window()
            .expect("window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&yes, 100)
            .expect("timeout");
    });
    let _ = JsFuture::from(p).await;
}

// The loader module script publishes the globals ASAP, but a fast
// transcribe start (e2e hook, quick fingers) can win the race — so wait
// here (up to ~30s) instead of failing on first sight.
#[allow(dead_code)]
async fn mod_obj(name: &str) -> Result<JsValue, String> {
    let g = js_sys::global();
    for _ in 0..300 {
        let v = js_sys::Reflect::get(&g, &name.into())
            .map_err(|_| format!("global {name} missing (loader not run?)"))?;
        if !v.is_undefined() && !v.is_null() {
            // NOTE: no dyn_into::<Object> — a module namespace has a null
            // prototype, so `instanceof Object` is false for it. The raw
            // JsValue works for Reflect::get; exports ignore `this`.
            return Ok(v);
        }
        sleep_100().await;
    }
    Err(format!("global {name} missing after 30s (loader failed?)"))
}

#[allow(dead_code)]
fn to_string_lossy(v: &JsValue) -> String {
    if let Some(s) = v.as_string() {
        return s;
    }
    js_sys::JSON::stringify(v)
        .ok()
        .and_then(|j| j.as_string())
        .unwrap_or_default()
}

#[allow(dead_code)]
fn jerr(e: JsValue) -> String {
    let s = to_string_lossy(&e);
    if s.is_empty() {
        "js error".to_string()
    } else {
        s
    }
}

#[allow(dead_code)]
async fn call_raw(mod_name: &str, fn_name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let obj = mod_obj(mod_name).await?;
    let f = js_sys::Reflect::get(&obj, &fn_name.into())
        .map_err(|_| format!("{mod_name}.{fn_name} missing"))?;
    let f = f
        .dyn_into::<js_sys::Function>()
        .map_err(|_| format!("{mod_name}.{fn_name} is not a function"))?;
    let argv = js_sys::Array::new();
    for a in args {
        argv.push(a);
    }
    let ret = f.apply(&JsValue::UNDEFINED, &argv).map_err(jerr)?;
    // Works for both Promise and plain-value returns.
    JsFuture::from(js_sys::Promise::resolve(&ret))
        .await
        .map_err(jerr)
}

#[allow(dead_code)]
async fn call_str(mod_name: &str, fn_name: &str, args: &[JsValue]) -> Result<String, String> {
    call_raw(mod_name, fn_name, args).await.map(|v| to_string_lossy(&v))
}

#[allow(dead_code)]
async fn call_unit(mod_name: &str, fn_name: &str, args: &[JsValue]) -> Result<(), String> {
    call_raw(mod_name, fn_name, args).await.map(|_| ())
}

fn s(v: &str) -> JsValue {
    JsValue::from_str(v)
}

/// BigInt for 64-bit-class params: wasm_bindgen maps Rust i64/usize to JS
/// BigInt, and passing a Number throws "Cannot convert N to a BigInt".
#[allow(dead_code)]
fn bigint<T: ToString>(n: T) -> JsValue {
    js_sys::BigInt::new(&JsValue::from_str(&n.to_string()))
        .map(|b| b.unchecked_into())
        .unwrap_or_else(|_| JsValue::from(0))
}

// ---- whisper module (mirrors the whisper pkg export signatures) ----

const WMOD: &str = "__rscaps_whisper";

#[allow(dead_code)]
pub async fn w_init(dist: Option<&str>) -> Result<(), String> {
    let d = match dist {
        Some(u) => s(u),
        None => JsValue::UNDEFINED,
    };
    call_unit(WMOD, "init", &[d]).await
}

#[allow(dead_code)]
pub async fn w_load_encoder(bytes: &[u8]) -> Result<(), String> {
    let arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    arr.copy_from(bytes);
    call_unit(WMOD, "load_encoder", &[arr.into()]).await
}

#[allow(dead_code)]
pub async fn w_load_decoder(bytes: &[u8]) -> Result<(), String> {
    let arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    arr.copy_from(bytes);
    call_unit(WMOD, "load_decoder", &[arr.into()]).await
}

#[allow(dead_code)]
pub async fn w_load_tokenizer(tok: &str) -> Result<(), String> {
    call_unit(WMOD, "load_tokenizer", &[s(tok)]).await
}

#[allow(dead_code)]
pub async fn w_encode_audio(pcm: &[f32]) -> Result<JsValue, String> {
    let arr = js_sys::Float32Array::new_with_length(pcm.len() as u32);
    arr.copy_from(pcm);
    call_raw(WMOD, "encode_audio", &[arr.into()]).await
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub async fn w_transcribe(
    hidden: &JsValue,
    lang_id: i64,
    task_id: i64,
    translate: bool,
    max_tokens: usize,
    window_dur: f32,
) -> Result<String, String> {
    call_str(
        WMOD,
        "transcribe",
        &[
            hidden.clone(),
            bigint(lang_id),
            bigint(task_id),
            translate.into(),
            max_tokens.into(),
            window_dur.into(),
        ],
    )
    .await
}

// ---- caption module (mirrors caption_core JSON API) ----

const CMOD: &str = "__rscaps_bridge";

/// Local mirror of `caption_core::Cue` so cue building no longer needs the rlib.
#[allow(dead_code)]
#[derive(serde::Serialize)]
pub struct Cue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
}

#[allow(dead_code)]
pub fn cues_to_json(cues: &[Cue]) -> Result<String, String> {
    serde_json::to_string(cues).map_err(|e| e.to_string())
}

#[allow(dead_code)]
pub async fn c_replace_cues_json(cues_json: &str) -> Result<(), String> {
    call_unit(CMOD, "replace_cues_json", &[s(cues_json)]).await
}

#[allow(dead_code)]
pub async fn c_set_document_json(doc_json: &str) -> Result<(), String> {
    call_unit(CMOD, "set_document_json", &[s(doc_json)]).await
}

#[allow(dead_code)]
pub async fn c_parse_subtitle_json(text: &str, fmt: &str) -> Result<String, String> {
    call_str(CMOD, "parse_subtitle", &[s(text), s(fmt)]).await
}

#[allow(dead_code)]
pub async fn c_format_subtitle_json(cues_json: &str, fmt: &str) -> Result<String, String> {
    call_str(CMOD, "format_subtitle", &[s(cues_json), s(fmt)]).await
}
