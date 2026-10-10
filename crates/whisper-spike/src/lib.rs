pub fn placeholder() {}

mod mel;
mod tokenizer;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::sync::Mutex;

    use ndarray::{Array1, Array2, Array3, Array4};
    use ort::session::{RunOptions, Session, SessionInputValue};
    use ort::value::Value;
    use ort_web::{SyncDirection, ValueExt};
    use wasm_bindgen::prelude::*;

    use crate::mel;
    use crate::tokenizer::{self, WhisperTokenizer};

    // whisper-base architecture constants (see Xenova/whisper-base config.json).
    const NUM_LAYERS: usize = 6;
    const NUM_HEADS: usize = 8;
    const HEAD_DIM: usize = 64;
    const D_MODEL: usize = 512;
    const ENC_LEN: usize = 1500;
    const VOCAB: usize = 51865;
    const NOTIMESTAMPS: i64 = 50363;

    static ENCODER: Mutex<Option<Session>> = Mutex::new(None);
    static DECODER: Mutex<Option<Session>> = Mutex::new(None);
    static TOKENIZER: Mutex<Option<WhisperTokenizer>> = Mutex::new(None);

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

    /// Load the whisper encoder ONNX model into memory.
    #[wasm_bindgen]
    pub async fn load_encoder(bytes: &[u8]) -> Result<(), JsValue> {
        let session = Session::builder().map_err(js)?.commit_from_memory(bytes).await.map_err(js)?;
        *ENCODER.lock().map_err(js)? = Some(session);
        Ok(())
    }

    /// Load the whisper merged decoder ONNX model into memory.
    #[wasm_bindgen]
    pub async fn load_decoder(bytes: &[u8]) -> Result<(), JsValue> {
        let session = Session::builder().map_err(js)?.commit_from_memory(bytes).await.map_err(js)?;
        *DECODER.lock().map_err(js)? = Some(session);
        Ok(())
    }

    /// Load the whisper tokenizer (parses `tokenizer.json`).
    #[wasm_bindgen]
    pub fn load_tokenizer(json: &str) -> Result<(), JsValue> {
        let tok = WhisperTokenizer::from_json(json).map_err(js)?;
        *TOKENIZER.lock().map_err(js)? = Some(tok);
        Ok(())
    }

    /// Back-compat alias for the spike page.
    #[wasm_bindgen]
    pub async fn load_model(bytes: &[u8]) -> Result<(), JsValue> {
        load_encoder(bytes).await
    }

    /// Compute the whisper log-mel (80 × 3000, mel-major) for a mono 16 kHz
    /// signal. Exposed so the feature extractor can be verified in isolation.
    #[wasm_bindgen]
    pub fn mel_from_audio(samples: &[f32]) -> Vec<f32> {
        mel::log_mel(samples)
    }

    /// Take an f32 tensor output by name, sync it to Rust memory, and return
    /// (dims, flat data). Required for ort-web where outputs live in wasm/JS
    /// memory until synced.
    async fn take_f32(
        outputs: &mut ort::session::SessionOutputs<'_>,
        name: &str,
    ) -> Result<(Vec<usize>, Vec<f32>), JsValue> {
        let mut v = outputs
            .remove(name)
            .ok_or_else(|| JsValue::from_str(&format!("missing output {name}")))?;
        v.sync(SyncDirection::Rust).await.map_err(js)?;
        let (shape, data) = v.try_extract_tensor::<f32>().map_err(js)?;
        let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        Ok((dims, data.to_vec()))
    }

    /// Run the whisper encoder directly on log-mel features shaped (1, 80, 3000),
    /// flattened row-major. Returns the (1, 1500, 512) hidden states flattened.
    #[wasm_bindgen]
    pub async fn encode(features: &[f32]) -> Result<Vec<f32>, JsValue> {
        if features.len() != mel::N_MELS * mel::N_FRAMES {
            return Err(JsValue::from_str(&format!(
                "expected {} floats, got {}",
                mel::N_MELS * mel::N_FRAMES,
                features.len()
            )));
        }

        let mut guard = ENCODER.lock().map_err(js)?;
        let session = guard.as_mut().ok_or_else(|| JsValue::from_str("load_encoder not called"))?;
        let input_name = session.inputs()[0].name().to_string();

        let mel_arr =
            Array3::from_shape_vec((1, mel::N_MELS, mel::N_FRAMES), features.to_vec()).map_err(js)?;
        let value = Value::from_array(mel_arr).map_err(js)?;
        let run_opts = RunOptions::new().map_err(js)?;

        let mut outputs = session
            .run_async(ort::inputs![input_name.as_str() => value], &run_opts)
            .await
            .map_err(js)?;

        let mut hidden = outputs
            .remove("last_hidden_state")
            .ok_or_else(|| JsValue::from_str("missing last_hidden_state"))?;
        hidden.sync(SyncDirection::Rust).await.map_err(js)?;
        let (_shape, data) = hidden.try_extract_tensor::<f32>().map_err(js)?;
        Ok(data.to_vec())
    }

    /// End-to-end audio → encoder: compute log-mel from a mono 16 kHz signal
    /// and run the encoder. Returns (1, 1500, 512) hidden states flattened.
    #[wasm_bindgen]
    pub async fn encode_audio(samples: &[f32]) -> Result<Vec<f32>, JsValue> {
        let features = mel::log_mel(samples);
        encode(&features).await
    }

    /// Debug: run ONE decoder prefill and report the shape of every input and
    /// output as JSON. Used to diagnose the present-encoder-KV shape issue.
    #[wasm_bindgen]
    pub async fn debug_prefill(hidden: &[f32]) -> Result<String, JsValue> {
        drop(vec![0u8; 64 * 1024 * 1024]);
        let mut dec_guard = DECODER.lock().map_err(js)?;
        let session = dec_guard.as_mut().ok_or_else(|| JsValue::from_str("load_decoder not called"))?;

        let enc = Array3::from_shape_vec((1, ENC_LEN, D_MODEL), hidden.to_vec()).map_err(js)?;
        let enc_value = Value::from_array(enc).map_err(js)?;
        let empty = || Array4::zeros((1, NUM_HEADS, 0, HEAD_DIM));
        let past_dec: Vec<(Array4<f32>, Array4<f32>)> = (0..NUM_LAYERS).map(|_| (empty(), empty())).collect();
        let past_enc: Vec<(Array4<f32>, Array4<f32>)> = (0..NUM_LAYERS).map(|_| (empty(), empty())).collect();

        let prompt: Vec<i64> = vec![tokenizer::SOT, 50259, 50359, NOTIMESTAMPS];
        let ids_arr = Array2::from_shape_vec((1, prompt.len()), prompt).map_err(js)?;

        let mut inputs: Vec<(String, SessionInputValue)> = Vec::new();
        inputs.push(("input_ids".to_string(), Value::from_array(ids_arr).map_err(js)?.into()));
        inputs.push(("encoder_hidden_states".to_string(), (&enc_value).into()));
        inputs.push((
            "use_cache_branch".to_string(),
            Value::from_array(Array1::from_vec(vec![false])).map_err(js)?.into(),
        ));
        for i in 0..NUM_LAYERS {
            inputs.push((format!("past_key_values.{i}.decoder.key"), Value::from_array(past_dec[i].0.clone()).map_err(js)?.into()));
            inputs.push((format!("past_key_values.{i}.decoder.value"), Value::from_array(past_dec[i].1.clone()).map_err(js)?.into()));
            inputs.push((format!("past_key_values.{i}.encoder.key"), Value::from_array(past_enc[i].0.clone()).map_err(js)?.into()));
            inputs.push((format!("past_key_values.{i}.encoder.value"), Value::from_array(past_enc[i].1.clone()).map_err(js)?.into()));
        }

        let run_opts = RunOptions::new().map_err(js)?;
        let input_names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let output_names: Vec<String> = session.outputs().iter().map(|o| o.name().to_string()).collect();
        let mut outputs = session.run_async(inputs, &run_opts).await.map_err(js)?;

        let mut report = serde_json::Map::new();
        let in_list: Vec<serde_json::Value> = input_names
            .iter()
            .map(|n| serde_json::json!({ "name": n }))
            .collect();
        report.insert("inputs".into(), serde_json::Value::Array(in_list));
        let mut out_list: Vec<serde_json::Value> = Vec::new();
        for name in output_names {
            if let Some(mut v) = outputs.remove(&name) {
                if v.sync(SyncDirection::Rust).await.is_ok() {
                    if let Ok((shape, data)) = v.try_extract_tensor::<f32>() {
                        let dims: Vec<i64> = shape.iter().map(|&d| d).collect();
                        out_list.push(serde_json::json!({ "name": name, "dims": dims, "len": data.len() }));
                    } else {
                        out_list.push(serde_json::json!({ "name": name, "err": "extract f32 failed" }));
                    }
                } else {
                    out_list.push(serde_json::json!({ "name": name, "err": "sync failed" }));
                }
            }
        }
        report.insert("outputs".into(), serde_json::Value::Array(out_list));
        Ok(serde_json::Value::Object(report).to_string())
    }

    /// Full transcription: encoder hidden states → greedy decoder with KV cache
    /// → tokenized text + timestamp segments. `lang_id`/`task_id` are whisper
    /// special-token ids (e.g. 50259 = <|en|>, 50359 = <|transcribe|>).
    /// Returns a JSON string: { text, segments: [{ start, end, text }] }.
    #[wasm_bindgen]
    pub async fn transcribe(
        hidden: &[f32],
        lang_id: i64,
        task_id: i64,
        return_timestamps: bool,
        max_tokens: usize,
    ) -> Result<String, JsValue> {
        if hidden.len() != ENC_LEN * D_MODEL {
            return Err(JsValue::from_str(&format!(
                "hidden expects {} floats, got {}",
                ENC_LEN * D_MODEL,
                hidden.len()
            )));
        }

        let tok_guard = TOKENIZER.lock().map_err(js)?;
        let tokenizer = tok_guard
            .as_ref()
            .ok_or_else(|| JsValue::from_str("load_tokenizer not called"))?;

        let mut dec_guard = DECODER.lock().map_err(js)?;
        let session = dec_guard.as_mut().ok_or_else(|| JsValue::from_str("load_decoder not called"))?;

        // Grow the wasm heap to a stable high-water mark BEFORE creating any
        // tensor. ort-web builds non-owning TypedArray views into our linear
        // memory; if the heap grows after a view is made, that view detaches and
        // `session.run` throws "Cannot perform Construct on a detached
        // ArrayBuffer". Wasm heaps only ever grow, so allocating + freeing a big
        // block here reserves headroom that all later input/output allocations
        // reuse — keeping every view valid for the whole decode loop.
        drop(vec![0u8; 64 * 1024 * 1024]);

        let enc = Array3::from_shape_vec((1, ENC_LEN, D_MODEL), hidden.to_vec()).map_err(js)?;
        // Create the encoder-hidden tensor ONCE and reuse it via a borrowed View
        // each step. Cloning the 3 MB array every iteration grew the wasm heap,
        // detaching buffer views the ORT runtime held mid-run ("detached
        // ArrayBuffer"). The tensor's data stays put, so the view stays valid.
        let enc_value = Value::from_array(enc).map_err(js)?;

        // Prompt: <|startoftranscript|> <|lang|> <|task|> [ <|notimestamps|> ].
        let mut prompt: Vec<i64> = vec![tokenizer::SOT, lang_id, task_id];
        if !return_timestamps {
            prompt.push(NOTIMESTAMPS);
        }

        // KV cache: per layer, (decoder key, decoder value) for self-attn and
        // (encoder key, encoder value) for cross-attn. Decoder seq grows each
        // step; encoder seq becomes 1500 after prefill and stays fixed. Prefill
        // (use_cache_branch=false) ignores the past KV and computes them fresh,
        // so we seed seq=0 zero placeholders — matching transformers.js, which
        // resolves the symbolic dims ("past_decoder_sequence_length",
        // "encoder_sequence_length_out") to 0 on the first call. Using seq=1
        // made the model treat the cache as already populated and it returned
        // present.* at the wrong length. The wasm-heap detach that zero-length
        // tensors can trigger is prevented by the 64 MB pre-grow above.
        let empty = || Array4::zeros((1, NUM_HEADS, 0, HEAD_DIM));
        let mut past_dec: Vec<(Array4<f32>, Array4<f32>)> =
            (0..NUM_LAYERS).map(|_| (empty(), empty())).collect();
        let mut past_enc: Vec<(Array4<f32>, Array4<f32>)> =
            (0..NUM_LAYERS).map(|_| (empty(), empty())).collect();

        let mut input_ids: Vec<i64> = prompt.clone();
        let mut generated: Vec<i64> = Vec::new();
        let mut use_cache = false;
        let mut iter: usize = 0;

        // Compute all outputs (default). We previously pruned to just logits +
        // present KV via OutputSelector, but that interfered with dynamic-shape
        // resolution of the present encoder KV (came back [0,8,1,64] instead of
        // [1,8,1500,64]). The wasm-heap detach that motivated pruning is fixed
        // by the 64 MB pre-grow above, so we no longer need to prune.
        let run_opts = RunOptions::new().map_err(js)?;

        loop {
            iter += 1;
            let ids_arr = Array2::from_shape_vec((1, input_ids.len()), input_ids.clone()).map_err(js)?;
            let mut inputs: Vec<(String, SessionInputValue)> = Vec::with_capacity(3 + NUM_LAYERS * 4);
            inputs.push((
                "input_ids".to_string(),
                Value::from_array(ids_arr).map_err(js)?.into(),
            ));
            inputs.push((
                "encoder_hidden_states".to_string(),
                (&enc_value).into(),
            ));
            inputs.push((
                "use_cache_branch".to_string(),
                Value::from_array(Array1::from_vec(vec![use_cache])).map_err(js)?.into(),
            ));
            for i in 0..NUM_LAYERS {
                inputs.push((
                    format!("past_key_values.{i}.decoder.key"),
                    Value::from_array(past_dec[i].0.clone()).map_err(js)?.into(),
                ));
                inputs.push((
                    format!("past_key_values.{i}.decoder.value"),
                    Value::from_array(past_dec[i].1.clone()).map_err(js)?.into(),
                ));
                inputs.push((
                    format!("past_key_values.{i}.encoder.key"),
                    Value::from_array(past_enc[i].0.clone()).map_err(js)?.into(),
                ));
                inputs.push((
                    format!("past_key_values.{i}.encoder.value"),
                    Value::from_array(past_enc[i].1.clone()).map_err(js)?.into(),
                ));
            }

            let mut outputs = session.run_async(inputs, &run_opts).await.map_err(js)?;

            // Logits → greedy argmax over the last decoder position.
            let (logits_dims, logits) = take_f32(&mut outputs, "logits")
                .await
                .map_err(|e| JsValue::from_str(&format!("take logits: {:?}", e)))?;
            let seq_len = *logits_dims.get(1).unwrap_or(&1);
            let base = seq_len.saturating_sub(1) * VOCAB;
            if base + VOCAB > logits.len() {
                return Err(JsValue::from_str(&format!(
                    "logits bounds: dims={:?} len={} base={} vocab={}",
                    logits_dims,
                    logits.len(),
                    base,
                    VOCAB
                )));
            }
            let mut best_id = 0i64;
            let mut best_score = f32::NEG_INFINITY;
            for (k, &logit) in logits[base..base + VOCAB].iter().enumerate() {
                if logit > best_score {
                    best_score = logit;
                    best_id = k as i64;
                }
            }

            // Advance KV cache from present.* outputs.
            for i in 0..NUM_LAYERS {
                let (dkd, k) = take_f32(&mut outputs, &format!("present.{i}.decoder.key"))
                    .await
                    .map_err(|e| JsValue::from_str(&format!("take present.{}.decoder.key: {:?}", i, e)))?;
                let (_dvd, v) = take_f32(&mut outputs, &format!("present.{i}.decoder.value"))
                    .await
                    .map_err(|e| JsValue::from_str(&format!("take present.{}.decoder.value: {:?}", i, e)))?;
                let ds = *dkd.get(2).unwrap_or(&1);
                let expected = NUM_HEADS * ds * HEAD_DIM;
                if k.len() != expected || v.len() != expected {
                    return Err(JsValue::from_str(&format!(
                        "present.{i}.decoder shape: dk={:?} k_len={} v_len={} expected={}",
                        dkd,
                        k.len(),
                        v.len(),
                        expected
                    )));
                }
                past_dec[i].0 = Array4::from_shape_vec((1, NUM_HEADS, ds, HEAD_DIM), k).map_err(js)?;
                past_dec[i].1 = Array4::from_shape_vec((1, NUM_HEADS, ds, HEAD_DIM), v).map_err(js)?;

                let es = if use_cache {
                    // Encoder KV is constant across decode steps. When
                    // use_cache_branch=true the model's present.*.encoder
                    // outputs are a degenerate passthrough ([0,8,1,64], 0 elts)
                    // that ORT does not populate — transformers.js likewise
                    // ignores them and reuses the previous past encoder KV
                    // (modeling_utils.js getPastKeyValues, is_encoder_pkv
                    // branch). So we keep past_enc as-is.
                    past_enc[i].0.shape()[2]
                } else {
                    let (ekd, ek) = take_f32(&mut outputs, &format!("present.{i}.encoder.key"))
                        .await
                        .map_err(|e| JsValue::from_str(&format!("take present.{}.encoder.key: {:?}", i, e)))?;
                    let (_evd, ev) = take_f32(&mut outputs, &format!("present.{i}.encoder.value"))
                        .await
                        .map_err(|e| JsValue::from_str(&format!("take present.{}.encoder.value: {:?}", i, e)))?;
                    let es = *ekd.get(2).unwrap_or(&1);
                    let expected_e = NUM_HEADS * es * HEAD_DIM;
                    if ek.len() != expected_e || ev.len() != expected_e {
                        return Err(JsValue::from_str(&format!(
                            "iter={} use_cache={} ids_len={} present.{i}.encoder shape: ek={:?} k_len={} v_len={} expected={} (present.{i}.decoder dk={:?})",
                            iter,
                            use_cache,
                            input_ids.len(),
                            ekd,
                            ek.len(),
                            ev.len(),
                            expected_e,
                            dkd
                        )));
                    }
                    past_enc[i].0 = Array4::from_shape_vec((1, NUM_HEADS, es, HEAD_DIM), ek).map_err(js)?;
                    past_enc[i].1 = Array4::from_shape_vec((1, NUM_HEADS, es, HEAD_DIM), ev).map_err(js)?;
                    es
                };
                let _ = es;
            }

            if best_id == tokenizer::EOS {
                break;
            }
            generated.push(best_id);
            if generated.len() >= max_tokens {
                break;
            }

            input_ids = vec![best_id];
            use_cache = true;
        }

        let (text, segments) = parse_segments(tokenizer, &generated, return_timestamps);
        let seg_json: Vec<serde_json::Value> = segments
            .iter()
            .map(|(s, e, t)| serde_json::json!({ "start": s, "end": e, "text": t }))
            .collect();
        let doc = serde_json::json!({ "text": text, "segments": seg_json });
        Ok(doc.to_string())
    }

    /// Parse generated token ids into (full_text, [(start, end, text)]).
    fn parse_segments(
        tokenizer: &WhisperTokenizer,
        ids: &[i64],
        timestamps: bool,
    ) -> (String, Vec<(f32, f32, String)>) {
        if !timestamps {
            return (tokenizer.decode(ids), Vec::new());
        }
        let mut segments: Vec<(f32, f32, String)> = Vec::new();
        let mut seg_start: Option<f32> = None;
        let mut last_ts: f32 = 0.0;
        let mut text_ids: Vec<i64> = Vec::new();

        for &id in ids {
            if tokenizer::is_timestamp(id) {
                let t = tokenizer::timestamp_seconds(id);
                last_ts = t;
                if !text_ids.is_empty() {
                    let start = seg_start.unwrap_or(0.0);
                    segments.push((start, t, tokenizer.decode(&text_ids)));
                    text_ids.clear();
                    seg_start = Some(t);
                } else {
                    seg_start = Some(t);
                }
            } else {
                text_ids.push(id);
            }
        }
        if !text_ids.is_empty() {
            let start = seg_start.unwrap_or(0.0);
            segments.push((start, last_ts, tokenizer.decode(&text_ids)));
        }
        let full = tokenizer.decode(ids);
        (full, segments)
    }
}
