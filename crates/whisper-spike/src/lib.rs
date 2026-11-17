pub fn placeholder() {}

mod engine;
mod mel;
mod tokenizer;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_arch = "wasm32")]
pub mod wasm {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use ndarray::{Array1, Array2, Array3, Array4};
    use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
    use ort::session::{RunOptions, Session, SessionInputValue};
    use ort::value::Value;
    use ort_web::{SyncDirection, ValueExt};
    use wasm_bindgen::prelude::*;

    use crate::mel;
    use crate::tokenizer::{self, WhisperTokenizer};

    // Architecture constants (see each Xenova/whisper-* config.json). Defaults
    // are whisper-base; JS calls set_arch() before load_encoder to select the
    // model architecture (tiny/base/small/medium/large-v3 all share encoder
    // len 1500, head_dim 64 and the token-id layout).
    const ENC_LEN: usize = 1500;
    const HEAD_DIM_DEFAULT: usize = 64;

    /// One whisper model architecture. All fields are read per call so a
    /// single wasm build can serve every model size. `dec_layers` is separate
    /// from `layers` because large-v3-turbo is asymmetric: 32 encoder layers
    /// but only 4 decoder layers (the KV-cache loops below are decoder-side,
    /// so they iterate `dec_layers`; the encoder needs no layer count — its
    /// ONNX graph is fixed).
    #[derive(Clone, Copy)]
    struct Arch {
        layers: usize,
        dec_layers: usize,
        heads: usize,
        head_dim: usize,
        d_model: usize,
        vocab: usize,
        n_mels: usize,
    }

    impl Arch {
        const BASE: Arch = Arch {
            layers: 6,
            dec_layers: 6,
            heads: 8,
            head_dim: HEAD_DIM_DEFAULT,
            d_model: 512,
            vocab: 51865,
            n_mels: 80,
        };
    }

    static ARCH: Mutex<Arch> = Mutex::new(Arch::BASE);

    fn arch() -> Arch {
        // A poisoned lock only means a previous call panicked mid-run; the
        // stored config itself is still valid, so keep serving it.
        *ARCH.lock().unwrap_or_else(|p| p.into_inner())
    }

    // Vocab variant — ground truth generation_config.json tiap repo (2026-11):
    // klasik (whisper-tiny/base/small/medium *_timestamped): pad/bos=50257,
    // decoder_start=50258, en=50259, id=50275, translate=50358,
    // transcribe=50359, startoflm=50360, startofprev=50361, nospeech=50362,
    // notimestamps=50363, timestamps 50364..51864 (0.02 s/langkah, 30.00
    // dalam-range), suppress tail 50358..50362 (yue TIDAK ADA di vocab).
    // v3 (whisper-large-v3-ONNX & whisper-large-v3-turbo_timestamped): geser
    // +1 pasca bahasa (yue=50358 = bahasa ke-100): translate=50359,
    // transcribe=50360, startoflm=50361, startofprev=50362, nospeech=50363,
    // notimestamps=50364, timestamps 50365..51865 — 51865 di luar logits
    // (vocab 51865 = indeks 0..51864), jadi TS_END=51864 = 29.98;
    // suppress tail 50359..50363.
    // Sama di KEDUA variant: decoder_start=50258, semua id bahasa
    // (en=50259, id=50275), begin_suppress [220, 50257], max_initial=50.
    // Kedua spec diatur runtime oleh set_arch(classic) — default klasik
    // (Arch::BASE = whisper-base).
    static VARIANT_CLASSIC: AtomicBool = AtomicBool::new(true);

    struct VocabSpec {
        transcribe: i64,
        notimestamps: i64,
        ts_begin: i64,
        nospeech: i64,
        ts_end: i64,
        suppress_tail: &'static [i64],
    }

    const CLASSIC_SPEC: VocabSpec = VocabSpec {
        transcribe: 50359,
        notimestamps: 50363,
        ts_begin: 50364,
        nospeech: 50362,
        ts_end: 51864,
        suppress_tail: &[50254, 50258, 50358, 50359, 50360, 50361, 50362],
    };
    const V3_SPEC: VocabSpec = VocabSpec {
        transcribe: 50360,
        notimestamps: 50364,
        ts_begin: 50365,
        nospeech: 50363,
        ts_end: 51864,
        suppress_tail: &[50254, 50258, 50359, 50360, 50361, 50362, 50363],
    };

    fn vocab() -> VocabSpec {
        if VARIANT_CLASSIC.load(Ordering::Relaxed) {
            CLASSIC_SPEC
        } else {
            V3_SPEC
        }
    }

    const MAX_INITIAL_TIMESTAMP_INDEX: i64 = 50; // generation_config resmi (1.00 s)

    // Anti-loop floors (mirrors tscaps WhisperLoopAbortLogitsProcessor). Two
    // verbatim repeats can be emphasis; five full periods over >=30 text tokens
    // cannot realistically be speech. Text tokens only — timestamps advance
    // every segment and would defeat plain periodicity.
    const MIN_FULL_PERIODS: usize = 3;
    const MIN_REPETITION_TOKENS: usize = 30;
    // Short single-token loops (e.g. "Sampai Sampai x10" on a silent tail
    // window) never reach the 30-token floor — a consecutive run of the SAME
    // token ≥ 8 times cannot be speech, so catch it with a much lower floor.
    const MIN_SAME_TOKEN_RUN: usize = 8;

    // A healthy 30 s window of real speech almost never transcribes to fewer
    // than 10 characters; a window that does (or is empty) is the instant-EOS
    // / one-word failure of weak quant variants, so it gets the retry pass.
    const MIN_TEXT_CHARS: usize = 10;
    // Token id of <|en|> (alphabetical order starts at <|en|> = 50259). The
    // last retry pass re-prompts in English: mismatched language tokens make
    // the 4-layer turbo decoder end instantly (large tolerates them; A/B:
    // turbo/q8 Download1+id = "Sampang", +en = the full correct transcript).
    const LANG_EN_TOKEN: i64 = 50259;

    /// Tokens suppressed at every step (whisper generation_config.suppress_tokens).
    const SUPPRESS_TOKENS: &[i64] = &[
        1, 2, 7, 8, 9, 10, 14, 25, 26, 27, 28, 29, 31, 58, 59, 60, 61, 62, 63, 90, 91, 92, 93, 359,
        503, 522, 542, 873, 893, 902, 918, 922, 931, 1350, 1853, 1982, 2460, 2627, 3246, 3253,
        3268, 3536, 3846, 3961, 4183, 4667, 6585, 6647, 7273, 9061, 9383, 10428, 10929, 11938,
        12033, 12331, 12562, 13793, 14157, 14635, 15265, 15618, 16553, 16604, 18362, 18956, 20075,
        21675, 22520, 26130, 26161, 26435, 28279, 29464, 31650, 32302, 32470, 36865, 42863, 47425,
        49870, 50254, 50258, 50359, 50360, 50361, 50362, 50363,
    ];
    /// Tokens suppressed only on the first generated token (begin_suppress_tokens).
    const BEGIN_SUPPRESS_TOKENS: &[i64] = &[220, tokenizer::EOS];

    // HF WhisperGenerationMixin fallback criteria (generation_config defaults):
    // a greedy attempt whose average token logprob falls below the gate
    // (set_logprob_gate, default -1.0) or whose text compresses too well
    // (compression ratio > 2.4 = repetition) is NOT trusted; the decode retries
    // at higher temperatures before the instant-EOS pass.
    const COMPRESSION_RATIO_THRESHOLD: f32 = 2.4;
    const NO_SPEECH_THRESHOLD: f32 = 0.6;
    const TEMPERATURE_LADDER: &[f32] = &[0.2, 0.4, 0.6, 0.8, 1.0];

    /// zlib compression ratio of `text` (raw len / compressed len). Healthy
    /// English speech lands ~1.2-2.0; repeated phrases compress hard (>2.4).
    fn compression_ratio(text: &str) -> f32 {
        let bytes = text.as_bytes();
        if bytes.len() < 32 {
            return 0.0;
        }
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6);
        if compressed.is_empty() {
            return 0.0;
        }
        bytes.len() as f32 / compressed.len() as f32
    }

    /// Deterministic multinomial sampling over softmax(logits/T) via xorshift64*.
    /// Seeded per (window, pass) so a rerun of the same audio reproduces exactly.
    fn sample_token(logits: &[f32], temp: f32, state: &mut u64) -> usize {
        let mut max = f32::NEG_INFINITY;
        for &l in logits {
            if l > max {
                max = l;
            }
        }
        let mut sum = 0f32;
        let probs: Vec<f32> = logits
            .iter()
            .map(|&l| {
                let e = ((l - max) / temp).exp();
                sum += e;
                e
            })
            .collect();
        let mut x = if *state == 0 {
            0x9E3779B97F4A7C15
        } else {
            *state
        };
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        let r = (x >> 11) as f64 / (1u64 << 53) as f64;
        let target = (r as f32) * sum;
        let mut acc = 0f32;
        for (i, &p) in probs.iter().enumerate() {
            acc += p;
            if acc >= target {
                return i;
            }
        }
        probs.len() - 1
    }

    /// log-sum-exp of a logit slice (already masked by the processors).
    fn log_sum_exp(logits: &[f32]) -> f32 {
        let mut max = f32::NEG_INFINITY;
        for &l in logits {
            if l > max {
                max = l;
            }
        }
        if max == f32::NEG_INFINITY {
            return f32::NEG_INFINITY;
        }
        let mut sum = 0f32;
        for &l in logits {
            sum += (l - max).exp();
        }
        max + sum.ln()
    }

    /// Apply whisper's logits processors in-place to one position's vocab logits.
    ///
    /// `generated` holds the tokens produced so far (after the prompt); empty on
    /// the first prediction. Mirrors HF transformers WhisperTimeStampLogitsProcessor
    /// plus the suppress_tokens / begin_suppress_tokens lists from generation_config.
    /// `audio_duration` (seconds) clamps timestamps to the real signal length so
    /// the model cannot run past the end of speech into a hallucination loop.
    fn apply_logits_processors(
        logits: &mut [f32],
        generated: &[i64],
        return_timestamps: bool,
        audio_duration: f32,
    ) {
        for &t in SUPPRESS_TOKENS {
            logits[t as usize] = f32::NEG_INFINITY;
        }
        for &t in vocab().suppress_tail {
            logits[t as usize] = f32::NEG_INFINITY;
        }
        if generated.is_empty() {
            for &t in BEGIN_SUPPRESS_TOKENS {
                logits[t as usize] = f32::NEG_INFINITY;
            }
        }
        // Clamp timestamps past the end of the audio (keep <|30.00|> so the
        // model retains its natural stop). Only meaningful when timestamps are on.
        if return_timestamps && audio_duration > 0.0 {
            for t in vocab().ts_begin..=vocab().ts_end {
                if t != vocab().ts_end && tokenizer::timestamp_seconds(t) > audio_duration {
                    logits[t as usize] = f32::NEG_INFINITY;
                }
            }
        }
        if return_timestamps {
            apply_timestamp_processor(logits, generated);
        }
    }

    /// True when the tail of `generated` has degenerated into a repetition loop
    /// over text tokens (timestamps stripped). On true, the caller should force
    /// end-of-sequence. Mirrors tscaps' WhisperLoopAbortLogitsProcessor.
    fn ends_in_repetition_loop(generated: &[i64]) -> bool {
        // Text-only view: drop timestamp tokens, which advance every segment.
        let text: Vec<i64> = generated
            .iter()
            .copied()
            .filter(|&t| !tokenizer::is_timestamp(t))
            .collect();

        // Short single-token run: same text token repeated back-to-back.
        let mut run = 1usize;
        for i in 1..text.len() {
            if text[i] == text[i - 1] {
                run += 1;
                if run >= MIN_SAME_TOKEN_RUN {
                    return true;
                }
            } else {
                run = 1;
            }
        }

        let max_period = text.len() / MIN_FULL_PERIODS;
        for period in 1..=max_period {
            let window = (period * MIN_FULL_PERIODS)
                .max(period * ((MIN_REPETITION_TOKENS + period - 1) / period));
            if window > text.len() {
                continue;
            }
            let from = text.len() - window;
            let mut periodic = true;
            for i in (from + period)..text.len() {
                if text[i] != text[i - period] {
                    periodic = false;
                    break;
                }
            }
            if periodic {
                return true;
            }
        }
        false
    }

    /// Trim a degenerate repetition tail detected by `ends_in_repetition_loop`
    /// so the looped tokens never reach the transcript. Works on the raw
    /// generated list (timestamps included): finds the smallest periodic tail
    /// window (or a same-token run) and drops it.
    fn trim_repetition_tail(generated: &mut Vec<i64>) {
        // Same-token run at the tail (timestamps between repeats are dropped
        // for the check, so scan the timestamp-filtered view but remember the
        // raw span).
        let raw: Vec<(usize, i64)> = generated
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, t)| !tokenizer::is_timestamp(t))
            .collect();
        if raw.len() < MIN_SAME_TOKEN_RUN {
            // fall through to periodic check below (short single-token loops
            // need >= MIN_SAME_TOKEN_RUN anyway)
        } else {
            let mut run_start = raw.len() - 1;
            for i in (1..raw.len()).rev() {
                if raw[i].1 == raw[i - 1].1 {
                    run_start = i - 1;
                    if raw.len() - run_start >= MIN_SAME_TOKEN_RUN {
                        generated.truncate(raw[run_start].0);
                        return;
                    }
                } else {
                    break;
                }
            }
        }
        // Periodic tail: drop the last full window for the smallest period.
        let text: Vec<(usize, i64)> = raw;
        let max_period = text.len() / MIN_FULL_PERIODS;
        for period in 1..=max_period {
            let window = (period * MIN_FULL_PERIODS)
                .max(period * ((MIN_REPETITION_TOKENS + period - 1) / period));
            if window > text.len() {
                continue;
            }
            let from = text.len() - window;
            let mut periodic = true;
            for i in (from + period)..text.len() {
                if text[i].1 != text[i - period].1 {
                    periodic = false;
                    break;
                }
            }
            if periodic {
                generated.truncate(text[from].0);
                return;
            }
        }
    }

    /// WhisperTimeStampLogitsProcessor: forces timestamp pairs, suppresses
    /// <|notimestamps|>, and prefers a timestamp when its total probability
    /// exceeds any single text token.
    fn apply_timestamp_processor(logits: &mut [f32], generated: &[i64]) {
        let tb = vocab().ts_begin as usize;
        logits[vocab().notimestamps as usize] = f32::NEG_INFINITY;

        if generated.is_empty() {
            // First token must be a timestamp; cap how far ahead it may start.
            for v in logits.iter_mut().take(tb) {
                *v = f32::NEG_INFINITY;
            }
            let last_allowed = tb + MAX_INITIAL_TIMESTAMP_INDEX as usize;
            for v in logits.iter_mut().skip(last_allowed + 1) {
                *v = f32::NEG_INFINITY;
            }
            return;
        }

        let last_was_ts = *generated.last().unwrap() >= tb as i64;
        let penultimate_was_ts =
            generated.len() < 2 || generated[generated.len() - 2] >= tb as i64;
        if last_was_ts {
            if penultimate_was_ts {
                // Two timestamps in a row: next must be text (segment start).
                for v in logits.iter_mut().skip(tb) {
                    *v = f32::NEG_INFINITY;
                }
            } else {
                // Segment end: next must be EOS or a new start timestamp.
                for v in logits.iter_mut().take(tokenizer::EOS as usize) {
                    *v = f32::NEG_INFINITY;
                }
            }
        }

        // If the summed probability over all timestamps beats the best single
        // text token, force a timestamp.
        let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        if !max_logit.is_finite() {
            return;
        }
        let mut sum_exp_ts = 0.0f32;
        let mut max_text = f32::NEG_INFINITY;
        for (i, &l) in logits.iter().enumerate() {
            if !l.is_finite() {
                continue;
            }
            if (i as i64) >= tb as i64 {
                sum_exp_ts += (l - max_logit).exp();
            } else if (i as i64) < tokenizer::EOS && l > max_text {
                max_text = l;
            }
        }
        // log(sum_ts exp(l)) = max_logit + log(sum exp(l - max_logit)); compare vs max_text.
        if sum_exp_ts > 0.0 && max_logit + sum_exp_ts.ln() > max_text {
            for v in logits.iter_mut().take(tb) {
                *v = f32::NEG_INFINITY;
            }
        }
    }

    static ENCODER: Mutex<Option<Session>> = Mutex::new(None);
    static DECODER: Mutex<Option<Session>> = Mutex::new(None);
    static TOKENIZER: Mutex<Option<WhisperTokenizer>> = Mutex::new(None);
    // Graph optimization level for the NEXT session loads (0=disable,
    // 1=basic, 2=extended, 3=all). fp16 exports crash ORT-wasm's fusion pass
    // (SimplifiedLayerNormFusion on InsertedPrecisionFreeCast) at the default
    // level 3 — JS drops to basic (1) for those files.
    static OPT_LEVEL: Mutex<u32> = Mutex::new(3);
    // HF avg-logprob gate (default -1.0); JS may tune it via set_logprob_gate
    // so A/B harness runs don't need a rebuild.
    static LOGPROB_GATE: Mutex<f32> = Mutex::new(-1.0);
    // Debug knob: suppress EOS until this many tokens are generated (0 = off).
    static MIN_NEW: Mutex<u32> = Mutex::new(0);

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
        let dist = ort_web::Dist::new(
            dist_url
                .as_deref()
                .unwrap_or("http://localhost:5177/nm/onnxruntime-web/dist/"),
        )
        .with_script_name("ort.min.js");
        let api = ort_web::api(dist).await.map_err(js)?;
        ort::set_api(api);
        ort::init().with_telemetry(false).commit();
        Ok(())
    }

    /// Select the model architecture BEFORE load_encoder/load_decoder (the
    /// decoder graph shape and mel size depend on it). Defaults to whisper-base.
    /// `layers` = encoder layer count (kept for completeness), `dec_layers` =
    /// decoder layer count — equal except on large-v3-turbo (32 / 4).
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
        let a = Arch {
            layers: layers.max(1) as usize,
            dec_layers: dec_layers.max(1) as usize,
            heads: heads.max(1) as usize,
            head_dim: head_dim.max(1) as usize,
            d_model: d_model.max(1) as usize,
            vocab: vocab.max(1) as usize,
            n_mels: n_mels.max(1) as usize,
        };
        *ARCH.lock().unwrap_or_else(|p| p.into_inner()) = a;
        // Vocab variant: klasik (tiny/base/small/medium *_timestamped) vs v3
        // (large-v3-ONNX, large-v3-turbo_timestamped). Timestamp range di
        // tokenizer mengikuti — semua id bahasa & decoder_start sama di keduanya.
        VARIANT_CLASSIC.store(classic, Ordering::Relaxed);
        let spec = if classic { CLASSIC_SPEC } else { V3_SPEC };
        tokenizer::set_ts_begin(spec.ts_begin);
    }

    /// Set the graph optimization level used by subsequent load_encoder /
    /// load_decoder calls (0=disable, 1=basic, 2=extended, 3=layout, 4=all).
    #[wasm_bindgen]
    pub fn set_opt_level(level: u32) {
        *OPT_LEVEL.lock().unwrap_or_else(|p| p.into_inner()) = level.min(4);
    }

    /// Tune the HF avg-logprob quality gate (default -1.0, HF generation_config).
    /// Attempts decoding below it retry on the temperature ladder.
    #[wasm_bindgen]
    pub fn set_logprob_gate(v: f32) {
        *LOGPROB_GATE.lock().unwrap_or_else(|p| p.into_inner()) = v;
    }

    fn logprob_gate() -> f32 {
        *LOGPROB_GATE.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Debug/quality knob: ban EOS until this many tokens exist (0 = off).
    #[wasm_bindgen]
    pub fn set_min_new_tokens(v: u32) {
        *MIN_NEW.lock().unwrap_or_else(|p| p.into_inner()) = v;
    }

    fn min_new_tokens() -> u32 {
        *MIN_NEW.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn session_builder() -> Result<SessionBuilder, JsValue> {
        let level = *OPT_LEVEL.lock().map_err(js)?;
        let g = match level {
            0 => GraphOptimizationLevel::Disable,
            1 => GraphOptimizationLevel::Level1,
            2 => GraphOptimizationLevel::Level2,
            3 => GraphOptimizationLevel::Level3,
            _ => GraphOptimizationLevel::All,
        };
        Session::builder()
            .map_err(js)?
            .with_optimization_level(g)
            .map_err(js)
    }

    /// Load the whisper encoder ONNX model into memory.
    #[wasm_bindgen]
    pub async fn load_encoder(bytes: &[u8]) -> Result<(), JsValue> {
        let session = session_builder()?
            .commit_from_memory(bytes)
            .await
            .map_err(js)?;
        *ENCODER.lock().map_err(js)? = Some(session);
        Ok(())
    }

    /// Load the whisper merged decoder ONNX model into memory.
    #[wasm_bindgen]
    pub async fn load_decoder(bytes: &[u8]) -> Result<(), JsValue> {
        let session = session_builder()?
            .commit_from_memory(bytes)
            .await
            .map_err(js)?;
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

    /// Compute the whisper log-mel (n_mels × 3000, mel-major) for a mono 16 kHz
    /// signal. Exposed so the feature extractor can be verified in isolation.
    #[wasm_bindgen]
    pub fn mel_from_audio(samples: &[f32]) -> Vec<f32> {
        mel::log_mel(samples, arch().n_mels)
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

    /// Run the whisper encoder directly on log-mel features shaped (1, n_mels, 3000),
    /// flattened row-major. Returns the (1, 1500, d_model) hidden states flattened.
    #[wasm_bindgen]
    pub async fn encode(features: &[f32]) -> Result<Vec<f32>, JsValue> {
        let a = arch();
        let expected = a.n_mels * mel::N_FRAMES;
        if features.len() != expected {
            return Err(JsValue::from_str(&format!(
                "expected {} floats, got {}",
                expected,
                features.len()
            )));
        }

        let mut guard = ENCODER.lock().map_err(js)?;
        let session = guard
            .as_mut()
            .ok_or_else(|| JsValue::from_str("load_encoder not called"))?;
        let input_name = session.inputs()[0].name().to_string();

        let mel_arr =
            Array3::from_shape_vec((1, a.n_mels, mel::N_FRAMES), features.to_vec()).map_err(js)?;
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
    /// and run the encoder. Returns (1, 1500, d_model) hidden states flattened.
    #[wasm_bindgen]
    pub async fn encode_audio(samples: &[f32]) -> Result<Vec<f32>, JsValue> {
        let features = mel::log_mel(samples, arch().n_mels);
        encode(&features).await
    }

    /// KV input as an owned clone (status-quo path). The borrowed/ownership-
    /// transfer alternatives (kv_ref/kv_take) were measured NOT faster and
    /// introduced run-to-run nondeterminism (wasm-arena layout shifts perturb
    /// ort's SIMD GEMM alignments enough to flip the logprob gate and spawn
    /// extra decode passes); they are kept out of the hot path.
    fn kv_clone(name: String, arr: &Array4<f32>) -> Result<(String, SessionInputValue<'static>), JsValue> {
        Ok((name, Value::from_array(arr.clone()).map_err(js)?.into()))
    }

    /// Debug: run ONE decoder prefill and report the shape of every input and
    /// output as JSON. Used to diagnose the present-encoder-KV shape issue.
    #[wasm_bindgen]
    pub async fn debug_prefill(hidden: &[f32]) -> Result<String, JsValue> {
        let a = arch();
        drop(vec![0u8; 64 * 1024 * 1024]);
        let mut dec_guard = DECODER.lock().map_err(js)?;
        let session = dec_guard
            .as_mut()
            .ok_or_else(|| JsValue::from_str("load_decoder not called"))?;

        let enc = Array3::from_shape_vec((1, ENC_LEN, a.d_model), hidden.to_vec()).map_err(js)?;
        let enc_value = Value::from_array(enc).map_err(js)?;
        let empty = || Array4::zeros((1, a.heads, 0, a.head_dim));
        let past_dec: Vec<(Array4<f32>, Array4<f32>)> =
            (0..a.dec_layers).map(|_| (empty(), empty())).collect();
        let past_enc: Vec<(Array4<f32>, Array4<f32>)> =
            (0..a.dec_layers).map(|_| (empty(), empty())).collect();

        let prompt: Vec<i64> = vec![tokenizer::SOT, 50259, vocab().transcribe, vocab().notimestamps];
        let ids_arr = Array2::from_shape_vec((1, prompt.len()), prompt).map_err(js)?;

        let mut inputs: Vec<(String, SessionInputValue)> = Vec::new();
        inputs.push((
            "input_ids".to_string(),
            Value::from_array(ids_arr).map_err(js)?.into(),
        ));
        inputs.push(("encoder_hidden_states".to_string(), (&enc_value).into()));
        inputs.push((
            "use_cache_branch".to_string(),
            Value::from_array(Array1::from_vec(vec![false]))
                .map_err(js)?
                .into(),
        ));
        for i in 0..a.dec_layers {
            for (arr, name) in [
                (&past_dec[i].0, "decoder.key"),
                (&past_dec[i].1, "decoder.value"),
                (&past_enc[i].0, "encoder.key"),
                (&past_enc[i].1, "encoder.value"),
            ] {
                inputs.push(kv_clone(format!("past_key_values.{i}.{name}"), arr)?);
            }
        }

        let run_opts = RunOptions::new().map_err(js)?;
        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        let output_names: Vec<String> = session
            .outputs()
            .iter()
            .map(|o| o.name().to_string())
            .collect();
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
                        out_list.push(
                            serde_json::json!({ "name": name, "dims": dims, "len": data.len() }),
                        );
                    } else {
                        out_list
                            .push(serde_json::json!({ "name": name, "err": "extract f32 failed" }));
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
        audio_duration: f32,
    ) -> Result<String, JsValue> {
        let a = arch();
        if hidden.len() != ENC_LEN * a.d_model {
            return Err(JsValue::from_str(&format!(
                "hidden expects {} floats, got {}",
                ENC_LEN * a.d_model,
                hidden.len()
            )));
        }

        let tok_guard = TOKENIZER.lock().map_err(js)?;
        let tokenizer = tok_guard
            .as_ref()
            .ok_or_else(|| JsValue::from_str("load_tokenizer not called"))?;

        let mut dec_guard = DECODER.lock().map_err(js)?;
        let session = dec_guard
            .as_mut()
            .ok_or_else(|| JsValue::from_str("load_decoder not called"))?;

        // Grow the wasm heap to a stable high-water mark BEFORE creating any
        // tensor. ort-web builds non-owning TypedArray views into our linear
        // memory; if the heap grows after a view is made, that view detaches and
        // `session.run` throws "Cannot perform Construct on a detached
        // ArrayBuffer". Wasm heaps only ever grow, so allocating + freeing a big
        // block here reserves headroom that all later input/output allocations
        // reuse — keeping every view valid for the whole decode loop.
        drop(vec![0u8; 64 * 1024 * 1024]);

        let enc = Array3::from_shape_vec((1, ENC_LEN, a.d_model), hidden.to_vec()).map_err(js)?;
        // Create the encoder-hidden tensor ONCE and reuse it via a borrowed View
        // each step. Cloning the 3 MB array every iteration grew the wasm heap,
        // detaching buffer views the ORT runtime held mid-run ("detached
        // ArrayBuffer"). The tensor's data stays put, so the view stays valid.
        let enc_value = Value::from_array(enc).map_err(js)?;

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
        // Compute all outputs (default). We previously pruned to just logits +
        // present KV via OutputSelector, but that interfered with dynamic-shape
        // resolution of the present encoder KV (came back [0,8,1,64] instead of
        // [1,8,1500,64]). The wasm-heap detach that motivated pruning is fixed
        // by the 64 MB pre-grow above, so we no longer need to prune.
        let run_opts = RunOptions::new().map_err(js)?;

        // Instant-EOS fallback (whisper-style retry): the first pass stays
        // pure greedy (exactly the old behavior, healthy models untouched).
        // Only when a window produces NO text at all (weak quant variants pick
        // instant-EOS on hard audio) retry once with EOS banned while the
        // window has no text token yet — argmax(logits/T) would be a no-op
        // (uniform scaling never changes the ranking), so suppression is the
        // deterministic way to force the decoder past its instant-EOS pick.
        let empty = || Array4::zeros((1, a.heads, 0, a.head_dim));
        // Pass list: [greedy, temperature ladder..., instant-EOS retry, English
        // recovery]. The ladder (HF fallback) only consumes passes when earlier
        // attempts fail the quality gate, so healthy windows stop at pass 1 and
        // the old behavior is untouched.
        let mut passes: Vec<(bool, i64, f32)> = vec![(false, lang_id, 0.0)];
        for &t in TEMPERATURE_LADDER {
            passes.push((false, lang_id, t));
        }
        passes.push((true, lang_id, 0.0));
        passes.push((false, LANG_EN_TOKEN, 0.0));
        let mut chosen: Vec<i64> = Vec::new();
        let mut best_chars: usize = 0;
        // Fallback (no attempt passes the quality gate): HF keeps the attempt
        // with the highest average logprob — a longer garbage repetition must
        // NOT win over a shorter confident one.
        let mut best_fb_logprob: f32 = f32::NEG_INFINITY;
        // Track the greedy attempt's no-speech probability and best logprob for
        // the silent-window gate: a low-confidence decode over a believed-silent
        // window is a Whisper hallucination ("Thank you." on silence), so empty
        // it instead of retrying forever.
        let mut pass1_no_speech: f32 = 0.0;
        let mut pass1_avg_logprob: f32 = 0.0;
        let mut chosen_ok = false;
        for (pass_i, &(suppress_eos, pass_lang, temp)) in passes.iter().enumerate() {
            // English selected: the third pass would repeat the first one.
            if pass_lang == lang_id && pass_i == passes.len() - 1 {
                continue;
            }
            // KV cache rebuilt per pass: past grows each step, a fresh decode
            // needs seq=0 zero placeholders again (see long note below the
            // original introduction of this block).
            let mut past_dec: Vec<(Array4<f32>, Array4<f32>)> =
                (0..a.dec_layers).map(|_| (empty(), empty())).collect();
            let mut past_enc: Vec<(Array4<f32>, Array4<f32>)> =
                (0..a.dec_layers).map(|_| (empty(), empty())).collect();

            // Retry pass runs WITHOUT timestamps: with timestamps on, the pair
            // rules + "summed ts prob beats text" force the no-text path
            // (timestamps until the duration clamp breaks the loop), so EOS
            // suppression there never reaches a text token. Plain
            // notimestamps mode lets text flow from the very first token.
            let ts = return_timestamps && !suppress_eos;
            let mut prompt: Vec<i64> = vec![tokenizer::SOT, pass_lang, task_id];
            if !ts {
                prompt.push(vocab().notimestamps);
            }
            let mut input_ids: Vec<i64> = prompt;
            let prompt_len = input_ids.len();
            let mut generated: Vec<i64> = Vec::new();
            let mut use_cache = false;
            let mut iter: usize = 0;
            // Deterministic PRNG per (window, pass): xorshift64* state. The
            // audio duration pins the seed so a rerun of the same window
            // reproduces the same sampling sequence.
            let mut prng: u64 = (audio_duration.to_bits() as u64)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add((pass_i as u64) << 32)
                | 1;
            let mut sum_logprob = 0f32;
            let mut n_steps = 0usize;
            let mut no_speech_prob = 0f32;

            loop {
                iter += 1;
                // Block-scoped: the borrowed KV inputs (kv_ref) live only inside
                // this block; the awaited future owns them, so the borrows end
                // here — the KV cache below can then be updated in place.
                let mut outputs = {
                    let ids_arr =
                        Array2::from_shape_vec((1, input_ids.len()), input_ids.clone())
                            .map_err(js)?;
                    let mut inputs: Vec<(String, SessionInputValue)> =
                        Vec::with_capacity(3 + a.dec_layers * 4);
                    inputs.push((
                        "input_ids".to_string(),
                        Value::from_array(ids_arr).map_err(js)?.into(),
                    ));
                    inputs.push(("encoder_hidden_states".to_string(), (&enc_value).into()));
                    inputs.push((
                        "use_cache_branch".to_string(),
                        Value::from_array(Array1::from_vec(vec![use_cache]))
                            .map_err(js)?
                            .into(),
                    ));
                    for i in 0..a.dec_layers {
                        inputs.push(kv_clone(
                            format!("past_key_values.{i}.decoder.key"),
                            &past_dec[i].0,
                        )?);
                        inputs.push(kv_clone(
                            format!("past_key_values.{i}.decoder.value"),
                            &past_dec[i].1,
                        )?);
                        inputs.push(kv_clone(
                            format!("past_key_values.{i}.encoder.key"),
                            &past_enc[i].0,
                        )?);
                        inputs.push(kv_clone(
                            format!("past_key_values.{i}.encoder.value"),
                            &past_enc[i].1,
                        )?);
                    }

                    session.run_async(inputs, &run_opts).await.map_err(js)?
                };

                // Logits → greedy argmax over the last decoder position.
                let (logits_dims, mut logits) = take_f32(&mut outputs, "logits")
                    .await
                    .map_err(|e| JsValue::from_str(&format!("take logits: {:?}", e)))?;
                let seq_len = *logits_dims.get(1).unwrap_or(&1);
                let base = seq_len.saturating_sub(1) * a.vocab;
                if base + a.vocab > logits.len() {
                    return Err(JsValue::from_str(&format!(
                        "logits bounds: dims={:?} len={} base={} vocab={}",
                        logits_dims,
                        logits.len(),
                        base,
                        a.vocab
                    )));
                }
                // First step: record the <|nospeech|> probability for the
                // silent-window gate (HF no_speech_threshold criterion) —
                // from the RAW logits BEFORE the processors (the suppress list
                // includes <|nospeech|>, so post-processor values are always 0).
                if generated.is_empty() {
                    let lse = log_sum_exp(&logits[base..base + a.vocab]);
                    if lse.is_finite() {
                        no_speech_prob = (logits[base + vocab().nospeech as usize] - lse).exp().max(0.0);
                    }
                }
                apply_logits_processors(
                    &mut logits[base..base + a.vocab],
                    &generated,
                    ts,
                    audio_duration,
                );
                // Pass 2: force past instant-EOS — while the window holds no
                // REAL text token yet (id < EOS; language/task special tokens
                // decode to "" and must not free the ban), ban EOS entirely
                // so the decoder must emit its best text guess instead.
                if suppress_eos && !generated.iter().any(|&id| id < tokenizer::EOS) {
                    logits[base + tokenizer::EOS as usize] = f32::NEG_INFINITY;
                }
                if (min_new_tokens() as usize) > generated.len() {
                    logits[base + tokenizer::EOS as usize] = f32::NEG_INFINITY;
                }
                // Attempt selection: temperature 0 = greedy argmax (old
                // behavior, healthy models untouched); raised temperature = the
                // HF fallback ladder — deterministic multinomial sampling.
                let best_id: i64 = if temp > 0.0 {
                    sample_token(&logits[base..base + a.vocab], temp, &mut prng) as i64
                } else {
                    let mut id = 0i64;
                    let mut sc = f32::NEG_INFINITY;
                    for (k, &logit) in logits[base..base + a.vocab].iter().enumerate() {
                        if logit > sc {
                            sc = logit;
                            id = k as i64;
                        }
                    }
                    id
                };
                // HF logprob gate: log P(chosen | context) from the
                // post-processor distribution (suppressed ids carry -inf, i.e.
                // zero probability, consistent with the selection above).
                let lse = log_sum_exp(&logits[base..base + a.vocab]);
                if lse.is_finite() {
                    sum_logprob += logits[base + best_id as usize] - lse;
                    n_steps += 1;
                }

                // Advance KV cache from present.* outputs.
                for i in 0..a.dec_layers {
                    let (dkd, k) = take_f32(&mut outputs, &format!("present.{i}.decoder.key"))
                        .await
                        .map_err(|e| {
                            JsValue::from_str(&format!("take present.{}.decoder.key: {:?}", i, e))
                        })?;
                    let (_dvd, v) = take_f32(&mut outputs, &format!("present.{i}.decoder.value"))
                        .await
                        .map_err(|e| {
                            JsValue::from_str(&format!("take present.{}.decoder.value: {:?}", i, e))
                        })?;
                    let ds = *dkd.get(2).unwrap_or(&1);
                    let expected = a.heads * ds * a.head_dim;
                    if k.len() != expected || v.len() != expected {
                        return Err(JsValue::from_str(&format!(
                            "present.{i}.decoder shape: dk={:?} k_len={} v_len={} expected={}",
                            dkd,
                            k.len(),
                            v.len(),
                            expected
                        )));
                    }
                    past_dec[i].0 =
                        Array4::from_shape_vec((1, a.heads, ds, a.head_dim), k).map_err(js)?;
                    past_dec[i].1 =
                        Array4::from_shape_vec((1, a.heads, ds, a.head_dim), v).map_err(js)?;

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
                            .map_err(|e| {
                                JsValue::from_str(&format!(
                                    "take present.{}.encoder.key: {:?}",
                                    i, e
                                ))
                            })?;
                        let (_evd, ev) =
                            take_f32(&mut outputs, &format!("present.{i}.encoder.value"))
                                .await
                                .map_err(|e| {
                                    JsValue::from_str(&format!(
                                        "take present.{}.encoder.value: {:?}",
                                        i, e
                                    ))
                                })?;
                        let es = *ekd.get(2).unwrap_or(&1);
                        let expected_e = a.heads * es * a.head_dim;
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
                        past_enc[i].0 =
                            Array4::from_shape_vec((1, a.heads, es, a.head_dim), ek).map_err(js)?;
                        past_enc[i].1 =
                            Array4::from_shape_vec((1, a.heads, es, a.head_dim), ev).map_err(js)?;
                        es
                    };
                    let _ = es;
                }

                // Stop when the model opens a NEW segment whose start timestamp is
                // at or past the end of the audio. Timestamps come in pairs
                // (start ... end, start ... end), so two consecutive timestamps
                // mean a segment just closed and another is opening; if that opener
                // sits at/after the audio length there is no signal left to
                // transcribe — this is exactly where Whisper's end-of-audio
                // hallucination ("Thank you." loops) begins. Compare in integer
                // frames (id - TIMESTAMP_BEGIN) to avoid float error in `* 0.02`.
                if ts && audio_duration > 0.0 && tokenizer::is_timestamp(best_id) {
                    if let Some(&prev) = generated.last() {
                        if tokenizer::is_timestamp(prev) {
                            let dur_frames = (audio_duration / 0.02).round() as i64;
                            if best_id - vocab().ts_begin >= dur_frames {
                                break;
                            }
                        }
                    }
                }
                // Anti-loop: if the generated text has degenerated into a
                // repetition (e.g. Whisper's "Thank you." hallucination), stop
                // before burning the rest of the token budget on garbage AND trim
                // the degenerate tail so it never reaches the transcript.
                if ends_in_repetition_loop(&generated) {
                    trim_repetition_tail(&mut generated);
                    break;
                }
                if best_id == tokenizer::EOS {
                    break;
                }
                generated.push(best_id);
                // Past-KV seq = prompt_len + generated.len() must stay within the
                // graph's baked max_target_positions (448): the self-attn mask is
                // a fixed [heads, 448, 448] tile, so a cache longer than 448 makes
                // the reshape inside the merged decoder throw ("Input shape:
                // {heads,0,448}, requested {1,heads,448,448}" — heads sesuai tiny/
                // small). Cap lama hanya menghitung token generated sehingga
                // decode yang mengulang (loop) sampai 448 membawa cache ke
                // prompt_len + 448 > 448. Batasi total di sini, sebelum step
                // berikutnya dijalankan.
                if prompt_len + generated.len() >= max_tokens {
                    break;
                }

                input_ids = vec![best_id];
                use_cache = true;
            }

            let (pass_text, _) = parse_segments(tokenizer, &generated, ts);
            let n = pass_text.chars().count();
            let avg_logprob = if n_steps > 0 {
                sum_logprob / n_steps as f32
            } else {
                f32::NEG_INFINITY
            };
            // HF quality gate: enough text, confident decode, non-degenerate
            // repetition. Failing attempts fall through to the next pass
            // (temperature ladder, then instant-EOS / English recovery).
            let quality_ok = n >= MIN_TEXT_CHARS
                && avg_logprob >= logprob_gate()
                && compression_ratio(&pass_text) <= COMPRESSION_RATIO_THRESHOLD;
            if pass_i == 0 {
                pass1_no_speech = no_speech_prob;
                pass1_avg_logprob = avg_logprob;
            }
            // Quality-gated attempts beat garbage longer ones; among garbage
            // attempts keep the highest-logprob one (HF fallback rule).
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
            // A healthy attempt ends the retries. Suppressed (instant-EOS)
            // passes always fall through: their text is a forced guess that
            // must not preempt the language-recovery pass.
            if quality_ok && !suppress_eos {
                break;
            }
        }

        // Silent-window gate (HF): when the model believed the window silent
        // (no_speech prob high) AND its decode is low-confidence, the "text"
        // is a hallucination on silence ("Thank you." loops) — emit nothing.
        let silent = !chosen_ok
            && pass1_no_speech > NO_SPEECH_THRESHOLD
            && pass1_avg_logprob < logprob_gate();
        let (text, segments) = if silent {
            (String::new(), Vec::new())
        } else {
            parse_segments(tokenizer, &chosen, return_timestamps)
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
