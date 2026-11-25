//! Browser math bridge for the hybrid transcription path. ONNX sessions and
//! WebGPU tensor I/O live in the JS worker; this module owns the pure pieces:
//! log-mel, tokenizer, logits processors, quality gates, and DTW.

use wasm_bindgen::prelude::*;
use whisper_core::{dtw, engine, mel, tokenizer, words};

#[wasm_bindgen]
pub fn mel_from_audio(samples: &[f32], n_mels: u32) -> Vec<f32> {
    mel::log_mel(samples, n_mels.max(1) as usize)
}

#[wasm_bindgen]
pub fn set_arch(
    layers: u32,
    dec_layers: u32,
    heads: u32,
    head_dim: u32,
    d_model: u32,
    vocab: u32,
    n_mels: u32,
    classic: bool,
) {
    engine::set_arch_value(
        layers, dec_layers, heads, head_dim, d_model, vocab, n_mels, classic,
    );
}

fn ids64(ids: &[i32]) -> Vec<i64> {
    ids.iter().map(|&v| v as i64).collect()
}

#[wasm_bindgen]
pub fn process_logits(
    mut logits: Vec<f32>,
    generated: &[i32],
    return_timestamps: bool,
    audio_duration: f32,
) -> Vec<f32> {
    let generated = ids64(generated);
    engine::apply_logits_processors(&mut logits, &generated, return_timestamps, audio_duration);
    logits
}

#[wasm_bindgen]
pub fn ends_in_repetition(generated: &[i32]) -> bool {
    engine::ends_in_repetition_loop(&ids64(generated))
}

#[wasm_bindgen]
pub fn logsumexp(logits: &[f32]) -> f32 {
    engine::log_sum_exp(logits)
}

/// Parse generated tokens into `{"text","segments":[{start,end,text}]}`.
#[wasm_bindgen]
pub fn segments_json(
    tokenizer_json: &str,
    ids: &[i32],
    timestamps: bool,
) -> Result<String, JsValue> {
    let tok = tokenizer::WhisperTokenizer::from_json(tokenizer_json)
        .map_err(|e| JsValue::from_str(&e))?;
    let ids = ids64(ids);
    let (text, segments) = engine::parse_segments(&tok, &ids, timestamps);
    let segs: Vec<serde_json::Value> = segments
        .iter()
        .map(|(s, e, t)| serde_json::json!({ "start": s, "end": e, "text": t }))
        .collect();
    Ok(serde_json::json!({ "text": text, "segments": segs }).to_string())
}

#[wasm_bindgen]
pub fn decode_tokens(tokenizer_json: &str, ids: &[i32]) -> Result<String, JsValue> {
    let tok = tokenizer::WhisperTokenizer::from_json(tokenizer_json)
        .map_err(|e| JsValue::from_str(&e))?;
    let ids: Vec<i64> = ids.iter().map(|&v| v as i64).collect();
    Ok(tok.decode(&ids))
}

#[wasm_bindgen]
pub fn compression_ratio(text: &str) -> f32 {
    engine::compression_ratio(text)
}

/// `weights` is `[n_tokens, n_heads, n_frames]`; returns flat `[start_s, end_s]`
/// pairs for each token.
#[wasm_bindgen]
pub fn dtw_token_times(
    weights: &[f32],
    n_tokens: u32,
    n_heads: u32,
    n_frames: u32,
) -> Result<Vec<f32>, JsValue> {
    let times = dtw::dtw_token_times(
        weights,
        n_tokens as usize,
        n_heads as usize,
        n_frames as usize,
    )
    .map_err(|e| JsValue::from_str(&e))?;
    let mut out = Vec::with_capacity(times.len() * 2);
    for t in times {
        out.push(t.start_s);
        out.push(t.end_s);
    }
    Ok(out)
}

/// `ids`/`starts`/`ends` are parallel text-token sequences. Returns
/// `[{"text","start_s","end_s"}]`.
#[wasm_bindgen]
pub fn words_json(
    tokenizer_json: &str,
    ids: &[i32],
    starts: &[f32],
    ends: &[f32],
) -> Result<String, JsValue> {
    if ids.len() != starts.len() || ids.len() != ends.len() {
        return Err(JsValue::from_str("words_json: parallel arrays mismatch"));
    }
    let tok = tokenizer::WhisperTokenizer::from_json(tokenizer_json)
        .map_err(|e| JsValue::from_str(&e))?;
    let ids64: Vec<i64> = ids.iter().map(|&v| v as i64).collect();
    let times: Vec<dtw::TokenTiming> = starts
        .iter()
        .zip(ends.iter())
        .map(|(&s, &e)| dtw::TokenTiming {
            start_s: s,
            end_s: e,
        })
        .collect();
    let words = words::words_from_tokens(&tok, &ids64, &times);
    let items: Vec<serde_json::Value> = words
        .iter()
        .map(|w| {
            serde_json::json!({
                "text": w.text,
                "start_s": w.start_s,
                "end_s": w.end_s,
            })
        })
        .collect();
    Ok(serde_json::Value::Array(items).to_string())
}
