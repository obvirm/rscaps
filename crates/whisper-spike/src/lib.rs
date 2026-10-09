pub fn placeholder() {}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::sync::Mutex;
    use wasm_bindgen::prelude::*;

    static SESSION: Mutex<Option<ort::session::Session>> = Mutex::new(None);

    fn js(err: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&err.to_string())
    }

    /// Initialize the ort-web backend (fetches onnxruntime-web dist from
    /// `dist_url`, which must be an absolute URL — ort-web constructs
    /// `new URL(...)` without a base). Must run before any other function here.
    #[wasm_bindgen]
    pub async fn init(dist_url: Option<String>) -> Result<(), JsValue> {
        std::panic::set_hook(Box::new(|info| {
            let bt = std::backtrace::Backtrace::force_capture();
            web_sys::console::error_1(&format!("PANIC: {info}\nBT: {bt}").as_str().into());
        }));
        let dist = ort_web::Dist::new(dist_url.as_deref().unwrap_or(
            "http://localhost:5177/nm/onnxruntime-web/dist/",
        ))
        .with_script_name("ort.min.js");
        let api = ort_web::api(dist).await.map_err(js)?;
        ort::set_api(api);
        ort::init().with_telemetry(false).commit();
        Ok(())
    }

    #[wasm_bindgen]
    pub async fn load_model(bytes: &[u8]) -> Result<(), JsValue> {
        let session = ort::session::Session::builder()
            .map_err(js)?
            .commit_from_memory(bytes)
            .await
            .map_err(js)?;
        *SESSION.lock().map_err(js)? = Some(session);
        Ok(())
    }

    /// Run the whisper encoder on log-mel features shaped (1, 80, 3000),
    /// flattened row-major. Returns the (1, 1500, 384) hidden states flattened.
    #[wasm_bindgen]
    pub async fn encode(features: &[f32]) -> Result<Vec<f32>, JsValue> {
        if features.len() != 80 * 3000 {
            return Err(JsValue::from_str(&format!(
                "expected {} floats, got {}",
                80 * 3000,
                features.len()
            )));
        }

        let mut guard = SESSION.lock().map_err(js)?;
        let session = guard.as_mut().ok_or_else(|| JsValue::from_str("load_model not called"))?;
        let input_name = session.inputs()[0].name().to_string();

        let mel = ndarray::Array3::from_shape_vec((1, 80, 3000), features.to_vec())
            .map_err(js)?;
        let value = ort::value::Value::from_array(mel).map_err(js)?;
        let run_opts = ort::session::RunOptions::new().map_err(js)?;

        let mut outputs = session
            .run_async(ort::inputs![input_name.as_str() => value], &run_opts)
            .await
            .map_err(js)?;

        use ort_web::{SyncDirection, ValueExt};
        let mut hidden = outputs
            .remove("last_hidden_state")
            .ok_or_else(|| JsValue::from_str("missing last_hidden_state output"))?;
        hidden.sync(SyncDirection::Rust).await.map_err(js)?;

        let (_shape, data) = hidden.try_extract_tensor::<f32>().map_err(js)?;
        Ok(data.to_vec())
    }
}
