//! Token timings -> whitespace-delimited words for the caption document.
//! CJK/no-space languages are intentionally one unit per token for now.

use crate::dtw::TokenTiming;
use crate::tokenizer::WhisperTokenizer;

#[derive(Debug, Clone, PartialEq)]
pub struct WordTiming {
    pub text: String,
    pub start_s: f32,
    pub end_s: f32,
}

/// `ids` and `times` must be parallel (the text-token subsequence used for
/// alignment). Specials/timestamps in `ids` are skipped.
pub fn words_from_tokens(
    tokenizer: &WhisperTokenizer,
    ids: &[i64],
    times: &[TokenTiming],
) -> Vec<WordTiming> {
    if ids.is_empty() || times.len() != ids.len() {
        return Vec::new();
    }
    let mut text = String::new();
    let mut char_token: Vec<usize> = Vec::new();
    for (ti, &id) in ids.iter().enumerate() {
        if let Some(piece) = tokenizer.decode_piece(id) {
            for ch in piece.chars() {
                text.push(ch);
                char_token.push(ti);
            }
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        if start == i || i > char_token.len() {
            continue;
        }
        let first = char_token[start];
        let last = char_token[i - 1];
        let t0 = times.get(first).map(|t| t.start_s).unwrap_or(0.0);
        let t1 = times.get(last).map(|t| t.end_s).unwrap_or(t0);
        out.push(WordTiming {
            text: chars[start..i].iter().collect(),
            start_s: t0,
            end_s: t1.max(t0),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_empty() {
        let tok = WhisperTokenizer::from_json("{\"model\":{\"vocab\":{}}}").unwrap();
        assert!(words_from_tokens(&tok, &[], &[]).is_empty());
    }
}
