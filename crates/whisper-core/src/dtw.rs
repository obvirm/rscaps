//! Monotonic DTW alignment of decoded text tokens to encoder time frames from
//! Whisper cross-attention weights (the `cross_attentions.*` ONNX outputs).
//! Frame -> seconds uses the 20 ms encoder frame grid (1500 frames = 30 s).

#[derive(Debug, Clone, PartialEq)]
pub struct TokenTiming {
    pub start_s: f32,
    pub end_s: f32,
}

/// Align `n_tokens` text tokens to `n_frames` time frames.
///
/// `weights` is row-major `[n_tokens, n_heads, n_frames]`, i.e.
/// `((token * n_heads) + head) * n_frames + frame`. Values are normalized per
/// (token, head), averaged over heads, then minimized by DTW (cost = -weight).
pub fn dtw_token_times(
    weights: &[f32],
    n_tokens: usize,
    n_heads: usize,
    n_frames: usize,
) -> Result<Vec<TokenTiming>, String> {
    if n_tokens == 0 {
        return Ok(Vec::new());
    }
    if n_frames == 0 {
        return Ok(vec![
            TokenTiming {
                start_s: 0.0,
                end_s: 0.0
            };
            n_tokens
        ]);
    }
    if n_heads == 0 {
        return Err("dtw: n_heads must be > 0".into());
    }
    let expected = n_tokens
        .checked_mul(n_heads)
        .and_then(|v| v.checked_mul(n_frames))
        .ok_or_else(|| "dtw: weight shape overflow".to_string())?;
    if weights.len() != expected {
        return Err(format!(
            "dtw: weights len {} != {} ({}x{}x{})",
            weights.len(),
            expected,
            n_tokens,
            n_heads,
            n_frames
        ));
    }

    // Average the per-head normalized attention distribution over frames.
    let mut cost = vec![0.0f32; n_tokens * n_frames];
    for t in 0..n_tokens {
        for f in 0..n_frames {
            let mut sum = 0.0f32;
            for h in 0..n_heads {
                let w = weights[(t * n_heads + h) * n_frames + f].max(0.0);
                sum += w;
            }
            cost[t * n_frames + f] = -sum / n_heads as f32;
        }
        // Normalize this token's averaged distribution to sum 1 so tokens with
        // uniformly small attention do not dominate the path.
        let row = &cost[t * n_frames..(t + 1) * n_frames];
        let norm: f32 = row.iter().map(|v| -v).sum();
        if norm > 0.0 {
            for v in &mut cost[t * n_frames..(t + 1) * n_frames] {
                *v /= norm;
            }
        }
    }

    // dp[(i)*(n_frames+1) + j] = cost of aligning first i tokens to first j frames.
    let cols = n_frames + 1;
    let mut dp = vec![f32::INFINITY; (n_tokens + 1) * cols];
    let mut back = vec![0u8; (n_tokens + 1) * cols];
    dp[0] = 0.0;
    for j in 1..=n_frames {
        dp[j] = f32::INFINITY;
    }
    for i in 1..=n_tokens {
        dp[i * cols] = f32::INFINITY;
        for j in 1..=n_frames {
            let c = cost[(i - 1) * n_frames + (j - 1)];
            let diag = dp[(i - 1) * cols + (j - 1)];
            let up = dp[(i - 1) * cols + j];
            let left = dp[i * cols + (j - 1)];
            let (best, dir) = if diag <= up && diag <= left {
                (diag, 0u8)
            } else if up <= left {
                (up, 1u8)
            } else {
                (left, 2u8)
            };
            dp[i * cols + j] = c + best;
            back[i * cols + j] = dir;
        }
    }

    // Backtrace and collect the frame range assigned to each token.
    let mut token_frame_min = vec![usize::MAX; n_tokens];
    let mut token_frame_max = vec![0usize; n_tokens];
    let (mut i, mut j) = (n_tokens, n_frames);
    while i > 0 && j > 0 {
        token_frame_min[i - 1] = token_frame_min[i - 1].min(j - 1);
        token_frame_max[i - 1] = token_frame_max[i - 1].max(j - 1);
        match back[i * cols + j] {
            0 => {
                i -= 1;
                j -= 1;
            }
            1 => i -= 1,
            _ => j -= 1,
        }
    }
    // Any token not visited by the path (should not happen with the DP above)
    // gets the nearest frame so the output stays monotonic and non-empty.
    let mut last = 0usize;
    for t in 0..n_tokens {
        if token_frame_min[t] == usize::MAX {
            token_frame_min[t] = last;
            token_frame_max[t] = last;
        }
        last = token_frame_max[t];
    }

    Ok((0..n_tokens)
        .map(|t| TokenTiming {
            start_s: token_frame_min[t] as f32 * 0.02,
            end_s: (token_frame_max[t] + 1) as f32 * 0.02,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_attention_maps_tokens_to_expected_frames() {
        let (tokens, frames) = (4usize, 8usize);
        let mut w = vec![0.0f32; tokens * frames];
        for t in 0..tokens {
            let f = t * 2;
            w[t * frames + f] = 1.0;
        }
        let times = dtw_token_times(&w, tokens, 1, frames).unwrap();
        assert_eq!(times.len(), tokens);
        for (t, timing) in times.iter().enumerate() {
            assert!((timing.start_s - (t * 2) as f32 * 0.02).abs() < 0.021);
        }
    }

    #[test]
    fn mismatched_shape_is_an_error() {
        assert!(dtw_token_times(&[0.0; 3], 2, 1, 2).is_err());
    }
}
