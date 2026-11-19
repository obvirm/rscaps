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

type KVTensor = ort::value::DynValue;

fn trace_enabled() -> bool {
    std::env::var("RSCAPS_NATIVE_TRACE").is_ok()
}

/// Segmen absolut (detik) hasil transkrip satu file audio.
pub struct NativeSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

/// Baca audio apapun → mono 16 kHz f32. WAV PCM16/f32 16k mono dibaca
/// langsung; format lain didekode via ffmpeg (biner eksplisit bila diberi,
/// else "ffmpeg" di PATH) ke WAV.
pub fn load_audio(path: &Path, ffmpeg_bin: Option<&Path>) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if let Some(f) = parse_wav(&bytes) {
        return Ok(f);
    }
    let tmp = std::env::temp_dir().join(format!("rscaps-native-{}.wav", std::process::id()));
    let status = std::process::Command::new(
        ffmpeg_bin.map(Path::to_path_buf).as_deref().unwrap_or(Path::new("ffmpeg")),
    )
    .args([
        "-y",
        "-i",
        path.to_str().ok_or("path audio")?,
        "-ac",
        "1",
        "-ar",
        "16000",
        tmp.to_str().unwrap(),
    ])
    .output()
    .map_err(|e| format!("ffmpeg tidak ditemukan/erro: {e}"))?;
    if !status.status.success() {
        return Err(format!(
            "ffmpeg gagal: {}",
            String::from_utf8_lossy(&status.stderr).chars().take(300).collect::<String>()
        ));
    }
    let b = std::fs::read(&tmp).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp);
    parse_wav(&b).ok_or_else(|| "WAV hasil ffmpeg tak terbaca".into())
}

fn parse_wav(b: &[u8]) -> Option<Vec<f32>> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    let (mut fmt, mut data): (Option<(u16, u16, u32, u16)>, &[u8]) = (None, &[]);
    while i + 8 <= b.len() {
        let tag = &b[i..i + 4];
        let size = u32::from_le_bytes([b[i+4], b[i+5], b[i+6], b[i+7]]) as usize;
        let body = &b[i + 8..(i + 8 + size).min(b.len())];
        if tag == b"fmt " && body.len() >= 16 {
            fmt = Some((
                u16::from_le_bytes([body[0], body[1]]),
                u16::from_le_bytes([body[2], body[3]]),
                u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
                u16::from_le_bytes([body[14], body[15]]),
            ));
        } else if tag == b"data" {
            data = body;
        }
        i += 8 + size + (size & 1);
    }
    let (_fmt_tag, channels, rate, bits) = fmt?;
    if channels != 1 || rate != 16000 {
        return None;
    }
    Some(match bits {
        16 => data
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect(),
        32 => data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        _ => return None,
    })
}

/// VAD-lite: puncak RMS per frame 100 ms > 0.01 (sama dengan hasSpeechEnergy
/// di transcribe.ts).
fn has_speech_energy(samples: &[f32]) -> bool {
    let frame = 1600; // 100 ms @ 16 kHz
    let mut peak = 0.0f32;
    let mut i = 0;
    while i + frame <= samples.len() {
        let sum: f32 = samples[i..i + frame].iter().map(|s| s * s).sum();
        let rms = (sum / frame as f32).sqrt();
        if rms > peak {
            peak = rms;
        }
        i += frame;
    }
    peak > 0.01
}

fn word_count(text: &str) -> usize {
    text.trim().split_whitespace().count()
}

/// Fraksi kata terbanyak (sinyal degradasi repetisi).
fn top_word_frac(text: &str) -> f32 {
    let mut hist: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let words = text
        .to_lowercase()
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .count();
    if words == 0 {
        return 0.0;
    }
    for w in text.to_lowercase().split_whitespace() {
        *hist.entry(w.to_string()).or_default() += 1;
    }
    let max = hist.values().copied().max().unwrap_or(0);
    max as f32 / words as f32
}

/// Cakupan waktu (detik) dari segmen hasil decode relatif terhadap durasi
/// jendela. Decode yang terpotong/early-EOS punya coverage sangat kecil.
fn coverage_sec(parsed: &serde_json::Value) -> f32 {
    parsed["segments"]
        .as_array()
        .and_then(|a| a.iter().map(|s| s["end"].as_f64().unwrap_or(0.0)).fold(0.0f64, f64::max).into())
        .unwrap_or(0.0) as f32
}

/// Buang ekor halu: segmen sangat pendek (<0,25 s) yang mengulang teks
/// segmen sebelumnya (kasus "Thank you for watching / Please subscribe"
/// berulang di ekor audio sulit). Kembalikan juga teks gabungan baru.
fn trim_duplicate_tail(segs: &mut Vec<NativeSegment>) {
    let tail_start = segs.len().saturating_sub(6);
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for s in &segs[tail_start..] {
        if s.end - s.start < 0.3 {
            *counts.entry(s.text.to_lowercase()).or_default() += 1;
        }
    }
    let first_bad = (tail_start..segs.len()).find(|&i| {
        segs[i].end - segs[i].start < 0.3
            && counts.get(&segs[i].text.to_lowercase()).copied().unwrap_or(0) >= 2
    });
    if let Some(i) = first_bad {
        segs.truncate(i);
    }
}

pub struct NativeEngine {
    pub encoder: Session,
    pub decoder: Session,
    pub tokenizer: WhisperTokenizer,
    model: String,
    quant: String,
    dir: std::path::PathBuf,
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

/// Mode GPU saat binary di-build dengan feature `gpu-directml`:
/// off (default) = CPU semua, encoder = encoder DML + decoder CPU,
/// full = DML di dua-duanya. Env RSCAPS_GPU_MODE untuk A/B tanpa rebuild.
fn gpu_mode() -> &'static str {
    if let Ok(v) = std::env::var("RSCAPS_GPU_MODE") {
        return match v.as_str() {
            "encoder" => "encoder",
            "full" => "full",
            "cuda" => "cuda",
            "cuda-encoder" => "cuda-encoder",
            "vulkan" => "vulkan",
            "vulkan-encoder" => "vulkan-encoder",
            "off" => "off",
            _ => default_gpu_mode(),
        };
    }
    default_gpu_mode()
}

/// GPU dipakai secara default bila binary di-build dengan fitur GPU.
fn default_gpu_mode() -> &'static str {
    #[cfg(feature = "gpu-cuda")]
    {
        "cuda"
    }
    #[cfg(all(not(feature = "gpu-cuda"), feature = "gpu-directml"))]
    {
        "full"
    }
    #[cfg(all(not(feature = "gpu-cuda"), not(feature = "gpu-directml"), feature = "gpu-webgpu"))]
    {
        "vulkan"
    }
    #[cfg(all(
        not(feature = "gpu-cuda"),
        not(feature = "gpu-directml"),
        not(feature = "gpu-webgpu")
    ))]
    {
        "off"
    }
}

fn native_builder(
    level: ort::session::builder::GraphOptimizationLevel,
    ep: &str,
) -> Result<ort::session::builder::SessionBuilder, Box<dyn Error>> {
    let mut b = Session::builder()?.with_optimization_level(level)?;
    if let Some(n) = std::env::var("RSCAPS_THREADS").ok().and_then(|s| s.parse::<usize>().ok()) {
        b = b.with_intra_threads(n)?.with_inter_threads(1)?;
    }
    if std::env::var("RSCAPS_SPIN").is_ok() {
        b = b.with_intra_op_spinning(true)?;
    }
    #[cfg(feature = "gpu-directml")]
    if ep == "encoder" || ep == "full" {
        b = b.with_execution_providers(&[ort::execution_providers::DirectML::default().into()])?;
    }
    #[cfg(feature = "gpu-cuda")]
    if ep == "cuda" || ep == "cuda-encoder" {
        let mut cuda = ort::execution_providers::CUDA::default();
        let graph = std::env::var("RSCAPS_CUDA_GRAPH").as_deref() != Ok("0");
        if graph {
            cuda = cuda.with_cuda_graph(true);
        }
        b = b.with_execution_providers(&[cuda.into()])?;
    }
    #[cfg(feature = "gpu-webgpu")]
    if ep == "vulkan" || ep == "vulkan-encoder" {
        use ort::execution_providers::webgpu::{DawnBackendType, WebGPU};
        b = b.with_execution_providers(&[WebGPU::default()
            .with_dawn_backend_type(DawnBackendType::Vulkan)
            .into()])?;
    }
    Ok(b)
}

impl NativeEngine {
    pub fn load(model: &str, dir: &Path, quant: &str) -> Result<Self, Box<dyn Error>> {
        Self::load_mode(model, dir, quant, gpu_mode())
    }

    fn load_mode(model: &str, dir: &Path, quant: &str, mode: &str) -> Result<Self, Box<dyn Error>> {
        let (layers, dec_layers, heads, d_model, n_mels, classic) = arch_table(model);
        engine::set_arch_value(layers, dec_layers, heads, 64, d_model, 51865, n_mels, classic);
        // Mapping opt-level SAMA dengan transcribe.ts (ort-web fork):
        // fp16/q4f16 + small|medium-q8 = Basic (fusion ORT #26717), lainnya = All.
        let level = if quant == "fp16"
            || quant == "q4f16"
            || ((model == "small" || model == "medium") && quant == "q8")
        {
            GraphOptimizationLevel::Level1
        } else {
            GraphOptimizationLevel::Level3
        };
        // Folder large-fp16 hanya menyimpan decoder versi split ext-data
        // (decoder_model_merged.ext.onnx + .ext.data) — ORT memuat data
        // eksternal otomatis dari referensi path di proto.
        let dec_file = if dir.join("decoder_model_merged.onnx").exists() {
            dir.join("decoder_model_merged.onnx")
        } else {
            dir.join("decoder_model_merged.ext.onnx")
        };
        let make_sessions = |m: &str| -> Result<(Session, Session), Box<dyn Error>> {
            let encoder = native_builder(level, m)?
                .commit_from_file(dir.join("encoder_model.onnx"))?;
            let decoder = native_builder(
                level,
                if m == "full" || m == "cuda" || m == "vulkan" {
                    m
                } else {
                    "off"
                },
            )?
            .commit_from_file(&dec_file)?;
            Ok((encoder, decoder))
        };
        let (encoder, decoder) = match make_sessions(mode) {
            Ok(pair) => pair,
            Err(e) if mode != "off" => {
                eprintln!("GPU session gagal ({e}) — fallback CPU");
                make_sessions("off")?
            }
            Err(e) => return Err(e),
        };
        if std::env::var("RSCAPS_DEBUG_IO").is_ok() {
            eprintln!("encoder inputs: {:?}", encoder.inputs().iter().map(|v| v.name()).collect::<Vec<_>>());
            eprintln!("decoder inputs: {:?}", decoder.inputs().iter().map(|v| v.name()).collect::<Vec<_>>());
            eprintln!("decoder outputs: {:?}", decoder.outputs().iter().map(|v| v.name()).collect::<Vec<_>>());
        }
        let tok_json = std::fs::read_to_string(dir.join("tokenizer.json"))?;
        let tokenizer = WhisperTokenizer::from_json(&tok_json)?;
        Ok(Self {
            encoder,
            decoder,
            tokenizer,
            model: model.to_string(),
            quant: quant.to_string(),
            dir: dir.to_path_buf(),
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
        let enc_value = ort::value::Tensor::from_array(enc.clone())?;

        let empty = || -> Array4<f32> { Array4::zeros((1, a.heads, 0, a.head_dim)) };
        let empty_kv = || -> Result<KVTensor, Box<dyn Error>> {
            Ok(Value::from_array(empty())?.into_dyn())
        };
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
            let mut past_dec: Vec<(KVTensor, KVTensor)> = (0..a.dec_layers)
                .map(|_| Ok((empty_kv()?, empty_kv()?)))
                .collect::<Result<_, Box<dyn Error>>>()?;
            let mut past_enc: Vec<(KVTensor, KVTensor)> = (0..a.dec_layers)
                .map(|_| Ok((empty_kv()?, empty_kv()?)))
                .collect::<Result<_, Box<dyn Error>>>()?;
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
                let mut inputs: Vec<(String, ort::session::SessionInputValue<'_>)> = Vec::new();
                inputs.push(("input_ids".into(), Value::from_array(ids_arr)?.into()));
                inputs.push(("encoder_hidden_states".into(), enc_value.view().into()));
                inputs.push((
                    "use_cache_branch".into(),
                    Value::from_array(Array1::from_vec(vec![use_cache]))?.into(),
                ));
                for i in 0..a.dec_layers {
                    inputs.push((
                        format!("past_key_values.{i}.decoder.key"),
                        past_dec[i].0.view().into(),
                    ));
                    inputs.push((
                        format!("past_key_values.{i}.decoder.value"),
                        past_dec[i].1.view().into(),
                    ));
                    inputs.push((
                        format!("past_key_values.{i}.encoder.key"),
                        past_enc[i].0.view().into(),
                    ));
                    inputs.push((
                        format!("past_key_values.{i}.encoder.value"),
                        past_enc[i].1.view().into(),
                    ));
                }

                // Outputs diekstrak OWNED dalam block ini; SessionOutputs
                // meminjam &mut session sampai dibuang.
                let (lg_seq, mut logits, mut pd, mut pe) = {
                    let mut out = self.decoder.run(inputs)?;
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
                    let mut pd: Vec<(KVTensor, KVTensor)> = Vec::new();
                    let mut pe: Vec<(KVTensor, KVTensor)> = Vec::new();
                    for i in 0..a.dec_layers {
                        // Ambil tensor KV sebagai MILIK (remove), jangan
                        // try_extract_tensor → to_vec → from_array: itu
                        // memaksa device→host→device copy tiap step dan
                        // membuat GPU idle di antara kernel.
                        let k = out.remove(format!("present.{i}.decoder.key")).ok_or("missing dec K")?;
                        let v = out.remove(format!("present.{i}.decoder.value")).ok_or("missing dec V")?;
                        let ds = k.shape().get(2).copied().unwrap_or(1) as usize;
                        let expected = a.heads * ds * a.head_dim;
                        if k.shape().iter().map(|&d| d as usize).product::<usize>() != expected
                            || v.shape().iter().map(|&d| d as usize).product::<usize>() != expected
                        {
                            return Err(format!(
                                "present.{i}.decoder shape: {:?} expected={expected}",
                                k.shape()
                            )
                            .into());
                        }
                        pd.push((k, v));
                        if !use_cache {
                            let e_k = out
                                .remove(format!("present.{i}.encoder.key"))
                                .ok_or("missing enc K")?;
                            let e_v = out
                                .remove(format!("present.{i}.encoder.value"))
                                .ok_or("missing enc V")?;
                            let es = e_k.shape().get(2).copied().unwrap_or(1) as usize;
                            let expected_e = a.heads * es * a.head_dim;
                            if e_k.shape().iter().map(|&d| d as usize).product::<usize>() != expected_e
                                || e_v.shape().iter().map(|&d| d as usize).product::<usize>() != expected_e
                            {
                                return Err(format!(
                                    "present.{i}.encoder shape: {:?} expected={expected_e}",
                                    e_k.shape()
                                )
                                .into());
                            }
                            pe.push((e_k, e_v));
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
                    past_enc = std::mem::take(&mut pe);
                }
                past_dec = std::mem::take(&mut pd);

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

    /// Transkrip file penuh: loop jendela 30 s + rescue slice sparse-decode
    /// (port 1:1 dari transcribeBlob di studio/src/lib/transcribe.ts —
    /// sumber tunggal perilaku transkrip kedua host). Mengembalikan segmen
    /// ABSOLUT (detik) + teks gabungan.
    pub fn transcribe_windows(
        &mut self,
        samples: &[f32],
        lang: &str,
        progress: &mut dyn FnMut(f64, &str),
    ) -> Result<(Vec<NativeSegment>, String), Box<dyn Error>> {
        const CHUNK_SAMPLES: usize = 30 * 16000;
        const SAMPLE_RATE: usize = 16000;
        let lang_id = lang_token(lang);
        let total = samples.len();
        if total == 0 {
            return Ok((Vec::new(), String::new()));
        }
        let n_windows = total.div_ceil(CHUNK_SAMPLES);
        // Prototipe batch: N jendela dalam SATU run decoder per step —
        // matmul GPU jauh lebih besar daripada N stream batch-1.
        if n_windows > 1
            && gpu_mode() != "off"
            && std::env::var("RSCAPS_BATCH").as_deref() != Ok("0")
        {
            progress(0.4, "transcribe");
            let chunks: Vec<&[f32]> = (0..n_windows)
                .map(|i| &samples[i * CHUNK_SAMPLES..((i + 1) * CHUNK_SAMPLES).min(total)])
                .collect();
            let results = self.transcribe_batch(&chunks, lang_id)?;
            progress(1.0, "done");
            let mut out: Vec<NativeSegment> = Vec::new();
            let mut texts: Vec<String> = Vec::new();
            for (i, (segs, text)) in results.into_iter().enumerate() {
                let offset = (i * CHUNK_SAMPLES / SAMPLE_RATE) as f32;
                out.extend(segs.into_iter().map(|mut s| {
                    s.start += offset;
                    s.end += offset;
                    s
                }));
                texts.push(text);
            }
            out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
            return Ok((out, texts.join(" ")));
        }
        // GPU full: beberapa jendela didekode paralel supaya kernel GPU cukup
        // besar untuk saturasi (decode satu token terlalu kecil). Env = A/B.
        let want_parallel = gpu_mode() == "full"
            && std::env::var("RSCAPS_WINDOWS_PAR").is_ok()
            && n_windows > 1;
        let mut extras: Vec<NativeEngine> = Vec::new();
        let pool_size = std::env::var("RSCAPS_GPU_POOL")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(3)
            .clamp(1, 8);
        if want_parallel {
            for _ in 1..n_windows.min(pool_size) {
                match NativeEngine::load(&self.model, &self.dir, &self.quant) {
                    Ok(e) => extras.push(e),
                    Err(_) => {
                        extras.clear();
                        break;
                    }
                }
            }
        }

        if !extras.is_empty() {
            progress(0.4, "transcribe");
            let n_threads = extras.len() + 1;
            let mut engines: Vec<&mut NativeEngine> = vec![self];            engines.extend(extras.iter_mut());
            let results: std::sync::Mutex<Vec<Option<Result<(Vec<NativeSegment>, String), String>>>> =
                std::sync::Mutex::new((0..n_windows).map(|_| None).collect());
            std::thread::scope(|scope| {
                for (k, eng) in engines.iter_mut().enumerate() {
                    let results = &results;
                    scope.spawn(move || {
                        let mut i = k;
                        while i < n_windows {
                            let start = i * CHUNK_SAMPLES;
                            let end = (start + CHUNK_SAMPLES).min(total);
                            let chunk = &samples[start..end];
                            let r = eng
                                .transcribe_window(
                                    chunk,
                                    lang_id,
                                    (start / SAMPLE_RATE) as f32,
                                    chunk.len() as f32 / SAMPLE_RATE as f32,
                                )
                                .map_err(|e| e.to_string());
                            results.lock().unwrap()[i] = Some(r);
                            i += n_threads;
                        }
                    });
                }
            });
            let mut out: Vec<NativeSegment> = Vec::new();
            let mut texts: Vec<String> = Vec::new();
            for r in results.into_inner().unwrap() {
                let (segs, text) = r.ok_or("parallel window tidak selesai")??;
                out.extend(segs);
                texts.push(text);
            }
            out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
            progress(1.0, "done");
            return Ok((out, texts.join(" ")));
        }

        let mut out: Vec<NativeSegment> = Vec::new();
        let mut texts: Vec<String> = Vec::new();

        for i in 0..n_windows {
            let start = i * CHUNK_SAMPLES;
            let end = (start + CHUNK_SAMPLES).min(total);
            let chunk = &samples[start..end];
            let offset = (start / SAMPLE_RATE) as f32;
            let window_dur = chunk.len() as f32 / SAMPLE_RATE as f32;

            progress(
                0.4 + (i as f64) / n_windows as f64 * 0.58,
                "transcribe",
            );
            let (win_segs, win_text) =
                self.transcribe_window(chunk, lang_id, offset, window_dur)?;
            texts.push(win_text);
            out.extend(win_segs);
        }
        progress(1.0, "done");
        Ok((out, texts.join(" ")))
    }

    /// Prototipe decode BATCH: N jendela berjalan lockstep dalam satu run()
    /// decoder → GPU melihat matmul batch N (bukan N stream kecil). Hanya
    /// pass greedy; fallback/rescue tetap ditangani pemanggil.
    fn transcribe_batch(
        &mut self,
        chunks: &[&[f32]],
        lang_id: i64,
    ) -> Result<Vec<(Vec<NativeSegment>, String)>, Box<dyn Error>> {
        let n = chunks.len();
        let a = engine::arch();
        let task_id = engine::vocab().transcribe;
        let mut hiddens: Vec<f32> = Vec::with_capacity(n * engine::ENC_LEN * a.d_model);
        let mut durations: Vec<f32> = Vec::with_capacity(n);
        for chunk in chunks {
            let features = mel::log_mel(chunk, a.n_mels);
            let mel_arr = Array3::from_shape_vec((1, a.n_mels, mel::N_FRAMES), features)?;
            let enc_name = self.encoder.inputs()[0].name().to_string();
            let out = self.encoder.run(ort::inputs![enc_name.as_str() => Value::from_array(mel_arr)?])?;
            let (_, hidden) = out["last_hidden_state"].try_extract_tensor::<f32>()?;
            hiddens.extend_from_slice(hidden);
            durations.push(chunk.len() as f32 / 16000.0);
        }
        let enc_value: KVTensor = Value::from_array(Array3::from_shape_vec(
            (n, engine::ENC_LEN, a.d_model),
            hiddens,
        )?)?
        .into_dyn();

        let prompt = vec![tokenizer::SOT, lang_id, task_id];
        let mut input_ids: Vec<i64> = (0..n).flat_map(|_| prompt.iter().copied()).collect();
        let prompt_len = prompt.len();
        let mut generated: Vec<Vec<i64>> = vec![Vec::new(); n];
        let mut done = vec![false; n];
        let mut use_cache = false;
        let empty = || -> Result<KVTensor, Box<dyn Error>> {
            Ok(Value::from_array(Array4::<f32>::zeros((n, a.heads, 0, a.head_dim)))?.into_dyn())
        };
        let mut past_dec: Vec<(KVTensor, KVTensor)> = (0..a.dec_layers)
            .map(|_| Ok((empty()?, empty()?)))
            .collect::<Result<_, Box<dyn Error>>>()?;
        let mut past_enc: Vec<(KVTensor, KVTensor)> = (0..a.dec_layers)
            .map(|_| Ok((empty()?, empty()?)))
            .collect::<Result<_, Box<dyn Error>>>()?;

        let mut step = 0usize;
        while step < 448 && done.iter().any(|&d| !d) {
            step += 1;
            let ids = Array2::from_shape_vec((n, input_ids.len() / n), input_ids.clone())?;
            let mut inputs: Vec<(String, ort::session::SessionInputValue<'_>)> = Vec::new();
            inputs.push(("input_ids".into(), Value::from_array(ids)?.into()));
            inputs.push(("encoder_hidden_states".into(), enc_value.view().into()));
            inputs.push((
                "use_cache_branch".into(),
                Value::from_array(Array1::from_vec(vec![use_cache]))?.into(),
            ));
            for i in 0..a.dec_layers {
                inputs.push((format!("past_key_values.{i}.decoder.key"), past_dec[i].0.view().into()));
                inputs.push((format!("past_key_values.{i}.decoder.value"), past_dec[i].1.view().into()));
                inputs.push((format!("past_key_values.{i}.encoder.key"), past_enc[i].0.view().into()));
                inputs.push((format!("past_key_values.{i}.encoder.value"), past_enc[i].1.view().into()));
            }
            let (mut logits, mut pd, mut pe) = {
                let mut out = self.decoder.run(inputs)?;
                let (sh, lg) = out["logits"].try_extract_tensor::<f32>()?;
                let seq = sh.get(1).copied().unwrap_or(1) as usize;
                let vocab = sh.get(2).copied().unwrap_or(1) as usize;
                let mut rows: Vec<Vec<f32>> = Vec::with_capacity(n);
                let mut pd: Vec<(KVTensor, KVTensor)> = Vec::with_capacity(a.dec_layers);
                let mut pe: Vec<(KVTensor, KVTensor)> = Vec::with_capacity(a.dec_layers);
                for r in 0..n {
                    rows.push(lg[r * seq * vocab + (seq - 1) * vocab
                        ..r * seq * vocab + seq * vocab]
                        .to_vec());
                }
                for i in 0..a.dec_layers {
                    pd.push((
                        out.remove(format!("present.{i}.decoder.key")).ok_or("missing dec K")?,
                        out.remove(format!("present.{i}.decoder.value")).ok_or("missing dec V")?,
                    ));
                    if !use_cache {
                        pe.push((
                            out.remove(format!("present.{i}.encoder.key")).ok_or("missing enc K")?,
                            out.remove(format!("present.{i}.encoder.value")).ok_or("missing enc V")?,
                        ));
                    }
                }
                drop(out);
                (rows, pd, pe)
            };
            if !use_cache {
                past_enc = std::mem::take(&mut pe);
            }
            past_dec = std::mem::take(&mut pd);

            let mut next: Vec<i64> = Vec::with_capacity(n);
            for r in 0..n {
                if done[r] {
                    next.push(tokenizer::EOS);
                    continue;
                }
                let row = &mut logits[r];
                engine::apply_logits_processors(row, &generated[r], true, durations[r]);
                let mut id = 0i64;
                let mut score = f32::NEG_INFINITY;
                for (k, v) in row.iter().copied().enumerate() {
                    if v > score {
                        score = v;
                        id = k as i64;
                    }
                }
                if id == tokenizer::EOS {
                    done[r] = true;
                } else if tokenizer::is_timestamp(id) {
                    if let Some(&prev) = generated[r].last() {
                        if tokenizer::is_timestamp(prev) {
                            let frames = (durations[r] / 0.02).round() as i64;
                            if id - engine::vocab().ts_begin >= frames {
                                done[r] = true;
                            }
                        }
                    }
                    if !done[r] {
                        generated[r].push(id);
                    }
                } else {
                    generated[r].push(id);
                }
                next.push(id);
            }
            if done.iter().all(|&d| d)
                || prompt_len + generated.iter().map(|g| g.len()).max().unwrap_or(0) >= 448
            {
                break;
            }
            input_ids = next;
            use_cache = true;
        }

        let mut results = Vec::with_capacity(n);
        for r in 0..n {
            let (text, segs) = engine::parse_segments(&self.tokenizer, &generated[r], true);
            let mut out: Vec<NativeSegment> = segs
                .into_iter()
                .map(|(s, e, t)| NativeSegment { start: s, end: e, text: t })
                .collect();
            let coverage = out.iter().map(|s| s.end).fold(0.0f32, f32::max);
            if coverage < durations[r] * 0.5 && has_speech_energy(chunks[r]) {
                if let Ok((better, better_text)) =
                    self.transcribe_window(chunks[r], lang_id, 0.0, durations[r])
                {
                    if better.iter().map(|s| s.end).fold(0.0f32, f32::max) > coverage {
                        out = better;
                        let _ = better_text;
                    }
                }
            }
            trim_duplicate_tail(&mut out);
            // Decode batch greedy menghasilkan lebih banyak pasangan timestamp
            // daripada jalur quality-ladder. Gabungkan cue yang jaraknya
            // sangat rapat supaya segmentasi setara jalur sequential.
            let mut merged: Vec<NativeSegment> = Vec::new();
            for seg in out {
                if let Some(last) = merged.last_mut() {
                    if seg.start - last.end < 0.5 {
                        last.end = last.end.max(seg.end);
                        last.text.push(' ');
                        last.text.push_str(&seg.text);
                        continue;
                    }
                }
                merged.push(seg);
            }
            let text = merged.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            results.push((merged, text));
        }
        Ok(results)
    }

    /// Satu jendela 30 s: decode + early-EOS fallback + rescue slice + anti-halu.
    /// Sumber tunggal dipakai jalur sequential dan paralel (GPU pool).
    fn transcribe_window(
        &mut self,
        chunk: &[f32],
        lang_id: i64,
        offset: f32,
        window_dur: f32,
    ) -> Result<(Vec<NativeSegment>, String), Box<dyn Error>> {
        const SAMPLE_RATE: usize = 16000;
        let mut parsed: serde_json::Value =
            serde_json::from_str(&self.transcribe(chunk, lang_id, true, 448)?)?;

        if coverage_sec(&parsed) < window_dur * 0.5 && has_speech_energy(chunk) {
            if let Ok(mut cpu) = NativeEngine::load_mode(&self.model, &self.dir, &self.quant, "off") {
                if let Ok(raw) = cpu.transcribe(chunk, lang_id, true, 448) {
                    if let Ok(p) = serde_json::from_str::<serde_json::Value>(&raw) {
                        if coverage_sec(&p) > coverage_sec(&parsed) {
                            parsed = p;
                        }
                    }
                }
            }
        }

        let wcount = word_count(parsed["text"].as_str().unwrap_or(""));
        let top_frac = top_word_frac(parsed["text"].as_str().unwrap_or(""));
        if parsed["quality"]["silent"] != serde_json::Value::Bool(true)
            && window_dur > 8.0
            && (wcount < (window_dur * 0.4).max(2.0) as usize || top_frac > 0.3)
            && has_speech_energy(chunk)
        {
            let slice_n = chunk.len().div_ceil(4 * SAMPLE_RATE);
            let mut slice_segments: Vec<NativeSegment> = Vec::new();
            let mut slice_texts: Vec<String> = Vec::new();
            for s in 0..slice_n {
                let s_start = s * 4 * SAMPLE_RATE;
                let slice = &chunk[s_start..((s_start + 4 * SAMPLE_RATE).min(chunk.len()))];
                if slice.len() < SAMPLE_RATE / 2 {
                    break;
                }
                let slice_start = (s_start / SAMPLE_RATE) as f32;
                let raw = self.transcribe(slice, lang_id, true, 448)?;
                let p2: serde_json::Value = serde_json::from_str(&raw)?;
                for seg in p2["segments"].as_array().unwrap_or(&vec![]) {
                    let text = seg["text"].as_str().unwrap_or("").trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    slice_segments.push(NativeSegment {
                        start: seg["start"].as_f64().unwrap_or(0.0) as f32 + slice_start,
                        end: seg["end"].as_f64().unwrap_or(0.0) as f32 + slice_start,
                        text,
                    });
                }
                let t = p2["text"].as_str().unwrap_or("").trim();
                if !t.is_empty() {
                    slice_texts.push(t.to_string());
                }
            }
            if word_count(&slice_texts.join(" ")) > wcount {
                parsed["text"] = serde_json::Value::String(slice_texts.join(" "));
                let arr: Vec<serde_json::Value> = slice_segments
                    .iter()
                    .map(|s| serde_json::json!({"start": s.start, "end": s.end, "text": s.text}))
                    .collect();
                parsed["segments"] = serde_json::Value::Array(arr);
            }
        }

        let mut win_segs: Vec<NativeSegment> = Vec::new();
        for seg in parsed["segments"].as_array().unwrap_or(&vec![]) {
            let text = seg["text"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                continue;
            }
            win_segs.push(NativeSegment {
                start: seg["start"].as_f64().unwrap_or(0.0) as f32 + offset,
                end: seg["end"].as_f64().unwrap_or(0.0) as f32 + offset,
                text,
            });
        }
        trim_duplicate_tail(&mut win_segs);
        let text = win_segs
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Ok((win_segs, text))
    }
}
