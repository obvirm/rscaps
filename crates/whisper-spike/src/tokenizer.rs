//! Whisper BPE tokenizer: decode token ids -> text. We only need decoding
//! (the encoder emits text token ids directly); encoding is not required for
//! transcription. Byte-level decoding reverses the GPT-2 `bytes_to_unicode`
//! mapping so token pieces reassemble into the original UTF-8 bytes.

use std::collections::HashMap;

/// Ids at/above this are "added tokens": 50257.. are specials and timestamps.
const ADDED_TOKEN_START: usize = 50257;
/// First timestamp token id; token T maps to (T - TIMESTAMP_BEGIN) * 0.02 s.
pub const TIMESTAMP_BEGIN: i64 = 50364;
/// Number of timestamp tokens (0.00 s .. 30.00 s inclusive at 0.02 s steps).
pub const TIMESTAMP_COUNT: i64 = 1501;

pub struct WhisperTokenizer {
    /// id -> byte-encoded token string (only for regular text ids 0..50257).
    id_to_token: Vec<String>,
    /// Unicode codepoint -> original byte (inverse of bytes_to_unicode).
    byte_decoder: HashMap<u32, u8>,
}

/// GPT-2 / whisper `bytes_to_unicode`, inverted for decoding.
fn build_byte_decoder() -> HashMap<u32, u8> {
    let mut bs: Vec<u8> = Vec::new();
    for b in b'!'..=b'~' {
        bs.push(b);
    }
    for b in 0xA1u8..=0xAC {
        bs.push(b);
    }
    for b in 0xAEu8..=0xFF {
        bs.push(b);
    }
    let mut cs: Vec<u32> = bs.iter().map(|&b| b as u32).collect();
    let mut n: u32 = 0;
    for b in 0..256u32 {
        if !bs.contains(&(b as u8)) {
            bs.push(b as u8);
            cs.push(256 + n);
            n += 1;
        }
    }
    cs.into_iter().zip(bs).collect()
}

impl WhisperTokenizer {
    /// Parse a Hugging Face `tokenizer.json` (we only read `model.vocab`).
    pub fn from_json(json: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let vocab = v
            .get("model")
            .and_then(|m| m.get("vocab"))
            .and_then(|x| x.as_object())
            .ok_or_else(|| "tokenizer.json missing model.vocab".to_string())?;

        let mut max_id = 0usize;
        for (_tok, id) in vocab {
            if let Some(id) = id.as_u64() {
                max_id = max_id.max(id as usize);
            }
        }
        let mut id_to_token = vec![String::new(); max_id + 1];
        for (tok, id) in vocab {
            if let Some(id) = id.as_u64() {
                id_to_token[id as usize] = tok.clone();
            }
        }
        Ok(Self {
            id_to_token,
            byte_decoder: build_byte_decoder(),
        })
    }

    /// Decode a sequence of generated token ids into plain text. Ids >= 50257
    /// (specials + timestamps) are skipped; timestamps are handled separately
    /// by the caller via `is_timestamp` / `timestamp_seconds`.
    pub fn decode(&self, ids: &[i64]) -> String {
        let mut bytes: Vec<u8> = Vec::with_capacity(ids.len() * 2);
        for &id in ids {
            let idx = id as usize;
            if idx >= ADDED_TOKEN_START {
                continue;
            }
            if let Some(token) = self.id_to_token.get(idx) {
                for ch in token.chars() {
                    if let Some(&b) = self.byte_decoder.get(&(ch as u32)) {
                        bytes.push(b);
                    }
                }
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[inline]
pub fn is_timestamp(id: i64) -> bool {
    id >= TIMESTAMP_BEGIN && id < TIMESTAMP_BEGIN + TIMESTAMP_COUNT
}

#[inline]
pub fn timestamp_seconds(id: i64) -> f32 {
    (id - TIMESTAMP_BEGIN) as f32 * 0.02
}

/// Special-token ids we care about for building prompts / stopping.
pub const SOT: i64 = 50258;
pub const EOS: i64 = 50257;
