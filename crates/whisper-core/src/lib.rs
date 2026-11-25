//! Pure, host-agnostic whisper logic shared by the native (ORT) and browser
//! (WASM math + JS ORT session) paths: mel features, tokenizer, arch/vocab,
//! logits processors, quality gates, and segment parsing.

pub mod dtw;
pub mod engine;
pub mod mel;
pub mod tokenizer;
pub mod words;
