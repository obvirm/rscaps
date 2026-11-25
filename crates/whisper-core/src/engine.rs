//! Logika model-agnostic whisper (arsitektur, vocab variant, processor logits,
//! anti-loop, segment parsing) — DIBAGIKAN antara jalur wasm (ort-web) dan
//! jalur native (ort-candle). Satu sumber: JANGAN duplikasi di kedua jalur.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::tokenizer;

// Architecture constants (see each Xenova/whisper-* config.json). Defaults
// are whisper-base; set_arch() selects the model architecture (tiny/base/
// small/medium/large-v3 all share encoder len 1500, head_dim 64 and the
// token-id layout).
pub const ENC_LEN: usize = 1500;
pub const HEAD_DIM_DEFAULT: usize = 64;

/// One whisper model architecture. `dec_layers` is separate from `layers`
/// because large-v3-turbo is asymmetric: 32 encoder layers but only 4 decoder
/// layers (the KV-cache loops are decoder-side).
#[derive(Clone, Copy)]
pub struct Arch {
    pub layers: usize,
    pub dec_layers: usize,
    pub heads: usize,
    pub head_dim: usize,
    pub d_model: usize,
    pub vocab: usize,
    pub n_mels: usize,
}

impl Arch {
    pub const BASE: Arch = Arch {
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

pub fn arch() -> Arch {
    // A poisoned lock only means a previous call panicked mid-run; the
    // stored config itself is still valid, so keep serving it.
    *ARCH.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn set_arch_value(
    layers: u32,
    dec_layers: u32,
    heads: u32,
    head_dim: u32,
    d_model: u32,
    vocab_size: u32,
    n_mels: u32,
    classic: bool,
) {
    let a = Arch {
        layers: layers.max(1) as usize,
        dec_layers: dec_layers.max(1) as usize,
        heads: heads.max(1) as usize,
        head_dim: head_dim.max(1) as usize,
        d_model: d_model.max(1) as usize,
        vocab: vocab_size.max(1) as usize,
        n_mels: n_mels.max(1) as usize,
    };
    *ARCH.lock().unwrap_or_else(|p| p.into_inner()) = a;
    VARIANT_CLASSIC.store(classic, Ordering::Relaxed);
    let spec = if classic { CLASSIC_SPEC } else { V3_SPEC };
    tokenizer::set_ts_begin(spec.ts_begin);
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

pub struct VocabSpec {
    pub transcribe: i64,
    pub notimestamps: i64,
    pub ts_begin: i64,
    pub nospeech: i64,
    pub ts_end: i64,
    pub suppress_tail: &'static [i64],
}

pub const CLASSIC_SPEC: VocabSpec = VocabSpec {
    transcribe: 50359,
    notimestamps: 50363,
    ts_begin: 50364,
    nospeech: 50362,
    ts_end: 51864,
    suppress_tail: &[50254, 50258, 50358, 50359, 50360, 50361, 50362],
};
pub const V3_SPEC: VocabSpec = VocabSpec {
    transcribe: 50360,
    notimestamps: 50364,
    ts_begin: 50365,
    nospeech: 50363,
    ts_end: 51864,
    suppress_tail: &[50254, 50258, 50359, 50360, 50361, 50362, 50363],
};

pub fn vocab() -> VocabSpec {
    if VARIANT_CLASSIC.load(Ordering::Relaxed) {
        CLASSIC_SPEC
    } else {
        V3_SPEC
    }
}

pub const MAX_INITIAL_TIMESTAMP_INDEX: i64 = 50; // generation_config resmi (1.00 s)

// Anti-loop floors (mirrors tscaps WhisperLoopAbortLogitsProcessor). Two
// verbatim repeats can be emphasis; five full periods over >=30 text tokens
// cannot realistically be speech. Text tokens only — timestamps advance
// every segment and would defeat plain periodicity.
pub const MIN_FULL_PERIODS: usize = 3;
pub const MIN_REPETITION_TOKENS: usize = 30;
// Short single-token loops (e.g. "Sampai Sampai x10" on a silent tail
// window) never reach the 30-token floor — a consecutive run of the SAME
// token ≥ 8 times cannot be speech, so catch it with a much lower floor.
pub const MIN_SAME_TOKEN_RUN: usize = 8;

// A healthy 30 s window of real speech almost never transcribes to fewer
// than 10 characters; a window that does (or is empty) is the instant-EOS
// / one-word failure of weak quant variants, so it gets the retry pass.
pub const MIN_TEXT_CHARS: usize = 10;
// Token id of <|en|> (alphabetical order starts at <|en|> = 50259). The
// last retry pass re-prompts in English: mismatched language tokens make
// the 4-layer turbo decoder end instantly (large tolerates them; A/B:
// turbo/q8 Download1+id = "Sampang", +en = the full correct transcript).
pub const LANG_EN_TOKEN: i64 = 50259;

/// Tokens suppressed at every step (whisper generation_config.suppress_tokens).
pub const SUPPRESS_TOKENS: &[i64] = &[
    1, 2, 7, 8, 9, 10, 14, 25, 26, 27, 28, 29, 31, 58, 59, 60, 61, 62, 63, 90, 91, 92, 93, 359,
    503, 522, 542, 873, 893, 902, 918, 922, 931, 1350, 1853, 1982, 2460, 2627, 3246, 3253,
    3268, 3536, 3846, 3961, 4183, 4667, 6585, 6647, 7273, 9061, 9383, 10428, 10929, 11938,
    12033, 12331, 12562, 13793, 14157, 14635, 15265, 15618, 16553, 16604, 18362, 18956, 20075,
    21675, 22520, 26130, 26161, 26435, 28279, 29464, 31650, 32302, 32470, 36865, 42863, 47425,
    49870, 50254, 50258, 50359, 50360, 50361, 50362, 50363,
];
/// Tokens suppressed only on the first generated token (begin_suppress_tokens).
pub const BEGIN_SUPPRESS_TOKENS: &[i64] = &[220, tokenizer::EOS];

// HF WhisperGenerationMixin fallback criteria (generation_config defaults):
// a greedy attempt whose average token logprob falls below the gate
// (logprob_gate, default -1.0) or whose text compresses too well
// (compression ratio > 2.4 = repetition) is NOT trusted; the decode retries
// at higher temperatures before the instant-EOS pass.
pub const COMPRESSION_RATIO_THRESHOLD: f32 = 2.4;
pub const NO_SPEECH_THRESHOLD: f32 = 0.6;
pub const TEMPERATURE_LADDER: &[f32] = &[0.2, 0.4, 0.6, 0.8, 1.0];

// HF avg-logprob gate (default -1.0); hosts may tune it (wasm set_logprob_gate,
// native set_logprob_gate_value) so harness runs don't need a rebuild.
static LOGPROB_GATE: Mutex<f32> = Mutex::new(-1.0);
// Debug knob: suppress EOS until this many tokens are generated (0 = off).
static MIN_NEW: Mutex<u32> = Mutex::new(0);

pub fn set_logprob_gate_value(v: f32) {
    *LOGPROB_GATE.lock().unwrap_or_else(|p| p.into_inner()) = v;
}

pub fn logprob_gate() -> f32 {
    *LOGPROB_GATE.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn set_min_new_tokens_value(v: u32) {
    *MIN_NEW.lock().unwrap_or_else(|p| p.into_inner()) = v;
}

pub fn min_new_tokens() -> u32 {
    *MIN_NEW.lock().unwrap_or_else(|p| p.into_inner())
}

/// zlib compression ratio of `text` (raw len / compressed len). Healthy
/// English speech lands ~1.2-2.0; repeated phrases compress hard (>2.4).
pub fn compression_ratio(text: &str) -> f32 {
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
pub fn sample_token(logits: &[f32], temp: f32, state: &mut u64) -> usize {
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
pub fn log_sum_exp(logits: &[f32]) -> f32 {
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
pub fn apply_logits_processors(
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
pub fn ends_in_repetition_loop(generated: &[i64]) -> bool {
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
pub fn trim_repetition_tail(generated: &mut Vec<i64>) {
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
pub fn apply_timestamp_processor(logits: &mut [f32], generated: &[i64]) {
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

/// Parse generated token ids into (full_text, [(start, end, text)]).
pub fn parse_segments(
    tokenizer: &tokenizer::WhisperTokenizer,
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
