//! Jalur NATIVE (desktop/CLI) whisper transkripsi — backend ORT penuh
//! (onnxruntime prebuilt) menjalankan ONNX yang sama dengan jalur wasm:
//! quants, opt-level, dan perilaku decode identik. Semua logika (arch,
//! vocab variant, processors, anti-loop, gates) HIDUP DI engine.rs — satu
//! sumber dengan jalur wasm; JANGAN duplikasi di sini.
#![cfg(not(target_arch = "wasm32"))]

use std::error::Error;
use std::path::Path;

use ndarray::{Array1, Array2, Array3, Array4};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;

use crate::engine;
use crate::mel;
use crate::tokenizer::{self, WhisperTokenizer};

type SV = ort::session::SessionInputValue<'static>;

fn trace_enabled() -> bool {
    std::env::var("RSCAPS_NATIVE_TRACE").is_ok()
}

pub struct NativeEngine {
    pub encoder: Session,
    pub decoder: Session,
    pub tokenizer: WhisperTokenizer,
}

/// (layers, dec_layers, heads, d_model, n_mels, classic) per model —
/// ground truth config.json onnx-community (sama dengan tabel transcribe.ts).
pub fn arch_table(model: &str) -> (u32, u32, u32, u32, u32, bool) {
    match model {
        "tiny" => (4, 4, 6, 384, 80, true),
        "base" => (6, 6, 8, 512, 80, true),
        "small" => (12, 12, 12, 768, 80, true),
        "medium" => (24, 24, 16, 1024, 80, true),
        "large" => (32, 32, 20, 1280, 128, false),
        "turbo" => (32, 4, 20, 1280, 128, false),
        _ => (6, 6, 8, 512, 80, true),
    }
}

/// Token id bahasa (urutan alfabetis whisper, en=50259; id=50275).
pub fn lang_token(lang: &str) -> i64 {
    match lang {
        "id" | "id-ID" => 50275,
        "zh" => 50260,
        "de" => 50261,
        "es" => 50262,
        "ru" => 50263,
        "ko" => 50264,
        "ja" => 50265,
        "it" => 50266,
        "fr" => 50267,
        "pt" => 50268,
        "hi" => 50269,
        "th" => 50270,
        "vi" => 50271,
        "pl" => 50272,
        "ar" => 50273,
        "ms" => 50274,
        "tr" => 50276,
        "nl" => 50277,
        _ => 50259,
    }
}

impl NativeEngine {
    pub fn load(model: &str, dir: &Path) -> Result<Self, Box<dyn Error>> {
        let (layers, dec_layers, heads, d_model, n_mels, classic) = arch_table(model);
        engine::set_arch_value(layers, dec_layers, heads, 64, d_model, 51865, n_mels, classic);
        // Mapping opt-level SAMA dengan transcribe.ts (ort-web fork):
        // fp16/q4f16 = Basic (fusion ORT #26717), lainnya = All.
        let level = GraphOptimizationLevel::Level1;
        let encoder = Session::builder()?
            .with_optimization_level(level)?
            .commit_from_file(dir.join("encoder_model.onnx"))?;
        // Folder large-fp16 hanya menyimpan decoder versi split ext-data
        // (decoder_model_merged.ext.onnx + .ext.data) — ORT memuat data
        // eksternal otomatis dari referensi path di proto.
        let dec_file = if dir.join("decoder_model_merged.onnx").exists() {
            dir.join("decoder_model_merged.onnx")
        } else {
            dir.join("decoder_model_merged.ext.onnx")
        };
        let decoder = Session::builder()?
            .with_optimization_level(level)?
            .commit_from_file(dec_file)?;
        let tok_json = std::fs::read_to_string(dir.join("tokenizer.json"))?;
        let tokenizer = WhisperTokenizer::from_json(&tok_json)?;
        Ok(Self {
            encoder,
            decoder,
            tokenizer,
        })
    }

    /// Transkrip satu jendela audio mono 16 kHz → JSON {text, segments, tokens,
    /// quality} (skema sama dengan jalur wasm, tanpa rescue slice — itu tingkat
    /// JS yang berlaku untuk kedua host).
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        lang_id: i64,
        return_timestamps: bool,
        max_tokens: usize,
    ) -> Result<String, Box<dyn Error>> {
        let a = engine::arch();
        let audio_duration = samples.len() as f32 / 16000.0;
        let task_id = engine::vocab().transcribe;

        // --- encode sekali ---
        let features = mel::log_mel(samples, a.n_mels);
        let mel_arr = Array3::from_shape_vec((1, a.n_mels, mel::N_FRAMES), features)?;
        let hidden: Vec<f32> = {
            let input_name = self.encoder.inputs()[0].name().to_string();
            let value = Value::from_array(mel_arr)?;
            let out = self.encoder.run(ort::inputs![input_name.as_str() => value])?;
            let (_, data) = out["last_hidden_state"].try_extract_tensor::<f32>()?;
            let data = data.to_vec();
            drop(out);
            data
        };
        let enc = Array3::from_shape_vec((1, engine::ENC_LEN, a.d_model), hidden)?;

        let empty = || Array4::zeros((1, a.heads, 0, a.head_dim));
        let mut passes: Vec<(bool, i64, f32)> = vec![(false, lang_id, 0.0)];
        for &t in engine::TEMPERATURE_LADDER {
            passes.push((false, lang_id, t));
        }
        passes.push((true, lang_id, 0.0));
        passes.push((false, engine::LANG_EN_TOKEN, 0.0));
        let mut chosen: Vec<i64> = Vec::new();
        let mut best_chars: usize = 0;
        let mut best_fb_logprob: f32 = f32::NEG_INFINITY;
        let mut pass1_no_speech: f32 = 0.0;
        let mut pass1_avg_logprob: f32 = 0.0;
        let mut chosen_ok = false;
        for (pass_i, &(suppress_eos, pass_lang, temp)) in passes.iter().enumerate() {
            if pass_lang == lang_id && pass_i == passes.len() - 1 {
                continue;
            }
            let mut past_dec: Vec<(Array4<f32>, Array4<f32>)> =
                (0..a.dec_layers).map(|_| (empty(), empty())).collect();
            let mut past_enc: Vec<(Array4<f32>, Array4<f32>)> =
                (0..a.dec_layers).map(|_| (empty(), empty())).collect();
            let ts = return_timestamps && !suppress_eos;
            let mut prompt: Vec<i64> = vec![tokenizer::SOT, pass_lang, task_id];
            if !ts {
                prompt.push(engine::vocab().notimestamps);
            }
            let mut input_ids: Vec<i64> = prompt;
            let prompt_len = input_ids.len();
            let mut generated: Vec<i64> = Vec::new();
            let mut use_cache = false;
            let mut iter: usize = 0;
            let mut prng: u64 = (audio_duration.to_bits() as u64)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add((pass_i as u64) << 32)
                | 1;
            let mut sum_logprob = 0f32;
            let mut n_steps = 0usize;
            let mut no_speech_prob = 0f32;

            loop {
                iter += 1;
                let ids_arr =
                    Array2::from_shape_vec((1, input_ids.len()), input_ids.clone())?;
                let mut inputs: Vec<(String, SV)> = Vec::new();
                inputs.push(("input_ids".into(), Value::from_array(ids_arr)?.into()));
                inputs.push((
                    "encoder_hidden_states".into(),
                    Value::from_array(enc.clone())?.into(),
                ));
                inputs.push((
                    "use_cache_branch".into(),
                    Value::from_array(Array1::from_vec(vec![use_cache]))?.into(),
                ));
                for i in 0..a.dec_layers {
                    inputs.push((
                        format!("past_key_values.{i}.decoder.key"),
                        Value::from_array(past_dec[i].0.clone())?.into(),
                    ));
                    inputs.push((
                        format!("past_key_values.{i}.decoder.value"),
                        Value::from_array(past_dec[i].1.clone())?.into(),
                    ));
                    inputs.push((
                        format!("past_key_values.{i}.encoder.key"),
                        Value::from_array(past_enc[i].0.clone())?.into(),
                    ));
                    inputs.push((
                        format!("past_key_values.{i}.encoder.value"),
                        Value::from_array(past_enc[i].1.clone())?.into(),
                    ));
                }

                // Outputs diekstrak OWNED dalam block ini; SessionOutputs
                // meminjam &mut session sampai dibuang.
                let (lg_seq, mut logits, present_dec, present_enc) = {
                    let out = self.decoder.run(inputs)?;
                    let (lg_sh, lg) = out["logits"].try_extract_tensor::<f32>()?;
                    let seq = *lg_sh.get(1).unwrap_or(&1) as usize;
                    let vocab = *lg_sh.get(2).unwrap_or(&1) as usize;
                    if seq < 1 || lg.len() < seq * vocab {
                        return Err(format!(
                            "logits bounds: shape={lg_sh:?} len={}",
                            lg.len()
                        ).into());
                    }
                    let logits = lg[(seq - 1) * vocab..seq * vocab].to_vec();
                    let mut pd: Vec<(Array4<f32>, Array4<f32>)> = Vec::new();
                    let mut pe: Vec<(Array4<f32>, Array4<f32>)> = Vec::new();
                    for i in 0..a.dec_layers {
                        let (k_sh, k) = out[format!("present.{i}.decoder.key").as_str()]
                            .try_extract_tensor::<f32>()?;
                        let (_, v) = out[format!("present.{i}.decoder.value").as_str()]
                            .try_extract_tensor::<f32>()?;
                        let ds = *k_sh.get(2).unwrap_or(&1) as usize;
                        let expected = a.heads * ds * a.head_dim;
                        if k.len() != expected || v.len() != expected {
                            return Err(format!(
                                "present.{i}.decoder shape: {k_sh:?} k_len={} expected={expected}",
                                k.len()
                            )
                            .into());
                        }
                        pd.push((
                            Array4::from_shape_vec((1, a.heads, ds, a.head_dim), k.to_vec())?,
                            Array4::from_shape_vec((1, a.heads, ds, a.head_dim), v.to_vec())?,
                        ));
                        if !use_cache {
                            let (e_sh, e_k) = out[format!("present.{i}.encoder.key").as_str()]
                                .try_extract_tensor::<f32>()?;
                            let (_, e_v) = out[format!("present.{i}.encoder.value").as_str()]
                                .try_extract_tensor::<f32>()?;
                            let es = *e_sh.get(2).unwrap_or(&1) as usize;
                            let expected_e = a.heads * es * a.head_dim;
                            if e_k.len() != expected_e || e_v.len() != expected_e {
                                return Err(format!(
                                    "present.{i}.encoder shape: {e_sh:?} k_len={} expected={expected_e}",
                                    e_k.len()
                                )
                                .into());
                            }
                            pe.push((
                                Array4::from_shape_vec((1, a.heads, es, a.head_dim), e_k.to_vec())?,
                                Array4::from_shape_vec((1, a.heads, es, a.head_dim), e_v.to_vec())?,
                            ));
                        }
                    }
                    drop(out);
                    (seq, logits, pd, pe)
                };

                let vocab_size = a.vocab;
                if logits.len() < vocab_size {
                    return Err(format!(
                        "logits short: got {} expected {vocab_size} (seq={lg_seq})",
                        logits.len()
                    )
                    .into());
                }
                if generated.is_empty() {
                    let lse = engine::log_sum_exp(&logits[..vocab_size]);
                    if lse.is_finite() {
                        no_speech_prob = (logits[engine::vocab().nospeech as usize] - lse)
                            .exp()
                            .max(0.0);
                    }
                }
                engine::apply_logits_processors(
                    &mut logits[..vocab_size],
                    &generated,
                    ts,
                    audio_duration,
                );
                if suppress_eos && !generated.iter().any(|&id| id < tokenizer::EOS) {
                    logits[tokenizer::EOS as usize] = f32::NEG_INFINITY;
                }
                if (engine::min_new_tokens() as usize) > generated.len() {
                    logits[tokenizer::EOS as usize] = f32::NEG_INFINITY;
                }
                let best_id: i64 = if temp > 0.0 {
                    engine::sample_token(&logits[..vocab_size], temp, &mut prng) as i64
                } else {
                    let mut id = 0i64;
                    let mut sc = f32::NEG_INFINITY;
                    for (k, &logit) in logits[..vocab_size].iter().enumerate() {
                        if logit > sc {
                            sc = logit;
                            id = k as i64;
                        }
                    }
                    id
                };
                if generated.is_empty() && pass_i == 0 && trace_enabled() {
                    let mut top: Vec<(f32, i64)> = logits[..vocab_size]
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(i, v)| (v, i as i64))
                        .collect();
                    top.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                    eprintln!(
                        "trace first-token: {best_id} · top5 {:?}",
                        &top[..5.min(top.len())]
                    );
                }
                let lse = engine::log_sum_exp(&logits[..vocab_size]);
                if lse.is_finite() {
                    sum_logprob += logits[best_id as usize] - lse;
                    n_steps += 1;
                }

                // Advance KV cache. Decoder grows each step; encoder KV is
                // refilled only on prefill (use_cache=false) and stays after.
                if !use_cache {
                    past_enc = present_enc;
                }
                past_dec = present_dec;

                if ts
                    && audio_duration > 0.0
                    && tokenizer::is_timestamp(best_id)
                {
                    if let Some(&prev) = generated.last() {
                        if tokenizer::is_timestamp(prev) {
                            let dur_frames = (audio_duration / 0.02).round() as i64;
                            if best_id - engine::vocab().ts_begin >= dur_frames {
                                break;
                            }
                        }
                    }
                }
                if engine::ends_in_repetition_loop(&generated) {
                    engine::trim_repetition_tail(&mut generated);
                    break;
                }
                if best_id == tokenizer::EOS {
                    break;
                }
                generated.push(best_id);
                // Past-KV seq = prompt_len + generated.len() must stay within
                // the graph's baked max_target_positions (448).
                if prompt_len + generated.len() >= max_tokens {
                    break;
                }
                input_ids = vec![best_id];
                use_cache = true;
            }

            let (pass_text, _) = engine::parse_segments(&self.tokenizer, &generated, ts);
            let n = pass_text.chars().count();
            let avg_logprob = if n_steps > 0 {
                sum_logprob / n_steps as f32
            } else {
                f32::NEG_INFINITY
            };
            if trace_enabled() {
                eprintln!(
                    "trace pass {pass_i} suppress_eos={suppress_eos} temp={temp} lang={pass_lang} steps={n_steps} tokens={} chars={n} avg_logprob={avg_logprob:.3} no_speech={no_speech_prob:.3}",
                    generated.len()
                );
            }
            let quality_ok = n >= engine::MIN_TEXT_CHARS
                && avg_logprob >= engine::logprob_gate()
                && engine::compression_ratio(&pass_text)
                    <= engine::COMPRESSION_RATIO_THRESHOLD;
            if pass_i == 0 {
                pass1_no_speech = no_speech_prob;
                pass1_avg_logprob = avg_logprob;
            }
            if quality_ok {
                if chosen_ok {
                    if n > best_chars {
                        best_chars = n;
                        chosen = generated;
                    }
                } else {
                    best_chars = n;
                    chosen = generated;
                    chosen_ok = true;
                }
            } else if !chosen_ok && n > 0 && avg_logprob > best_fb_logprob {
                best_fb_logprob = avg_logprob;
                best_chars = n;
                chosen = generated;
            }
            if quality_ok && !suppress_eos {
                break;
            }
        }

        let silent = !chosen_ok
            && pass1_no_speech > engine::NO_SPEECH_THRESHOLD
            && pass1_avg_logprob < engine::logprob_gate();
        let (text, segments) = if silent {
            (String::new(), Vec::new())
        } else {
            engine::parse_segments(&self.tokenizer, &chosen, return_timestamps)
        };
        let seg_json: Vec<serde_json::Value> = segments
            .iter()
            .map(|(s, e, t)| serde_json::json!({ "start": s, "end": e, "text": t }))
            .collect();
        let doc = serde_json::json!({
            "text": text,
            "segments": seg_json,
            "tokens": chosen,
            "quality": {
                "silent": silent,
                "avg_logprob": pass1_avg_logprob,
                "no_speech": pass1_no_speech,
            }
        });
        Ok(doc.to_string())
    }
}
