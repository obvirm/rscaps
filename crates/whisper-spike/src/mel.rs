//! Whisper log-mel feature extractor (matches openai/whisper `log_mel_spectrogram`
//! and Xenova's `WhisperFeatureExtractor`): 80 mel bins, n_fft 400, hop 160,
//! 16 kHz, padded to a 30 s (480 000-sample) window → (80, 3000) log-mel.

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

pub const SAMPLE_RATE: usize = 16000;
pub const N_FFT: usize = 400;
pub const HOP_LENGTH: usize = 160;
pub const N_MELS: usize = 80;
pub const N_FRAMES: usize = 3000;
pub const N_SAMPLES: usize = 480000; // 30 s
pub const FMIN: f64 = 0.0;
pub const FMAX: f64 = 8000.0;

// ── Slaney mel scale (librosa default: htk=False, norm="slaney") ──────────

fn hz_to_mel(hz: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    if hz <= 1000.0 {
        hz / f_sp
    } else {
        let min_log_hz = 1000.0;
        let min_log_mel = min_log_hz / f_sp; // 15.0
        let logstep = (6.4f64).ln() / 27.0;
        min_log_mel + (hz / min_log_hz).ln() / logstep
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    if mel <= 15.0 {
        mel * f_sp
    } else {
        let min_log_mel = 15.0;
        let min_log_hz = 1000.0;
        let logstep = (6.4f64).ln() / 27.0;
        min_log_hz * (logstep * (mel - min_log_mel)).exp()
    }
}

/// Triangular Slaney-normalized mel filterbank, row-major (N_MELS × (N_FFT/2+1)).
pub fn mel_filterbank() -> Vec<f32> {
    let n_bins = N_FFT / 2 + 1; // 201
    let fft_freqs: Vec<f64> = (0..n_bins).map(|i| i as f64 * SAMPLE_RATE as f64 / N_FFT as f64).collect();

    let mel_min = hz_to_mel(FMIN);
    let mel_max = hz_to_mel(FMAX);
    let n_pts = N_MELS + 2;
    let mel_points: Vec<f64> = (0..n_pts)
        .map(|i| mel_min + (mel_max - mel_min) * i as f64 / (n_pts - 1) as f64)
        .map(mel_to_hz)
        .collect();

    let mut filters = vec![0.0f32; N_MELS * n_bins];
    for m in 0..N_MELS {
        let lower = mel_points[m];
        let center = mel_points[m + 1];
        let upper = mel_points[m + 2];
        let bandwidth = upper - lower;
        for (i, &f) in fft_freqs.iter().enumerate() {
            let w = if f >= lower && f <= center {
                (f - lower) / (center - lower)
            } else if f > center && f <= upper {
                (upper - f) / (upper - center)
            } else {
                0.0
            };
            // norm="slaney": divide by filter bandwidth for unit area.
            filters[m * n_bins + i] = (w / bandwidth) as f32;
        }
    }
    filters
}

/// Periodic Hann window of length N_FFT (torch.hann_window default periodic=True).
fn hann_periodic(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
        .map(|v| v as f32)
        .collect()
}

/// numpy/torch "reflect" padding (edge value excluded).
fn reflect_pad(x: &[f32], pad: usize) -> Vec<f32> {
    let n = x.len();
    let mut out = Vec::with_capacity(n + 2 * pad);
    // left: x[pad], x[pad-1], ..., x[1]
    for i in (1..=pad).rev() {
        out.push(x[i.min(n - 1)]);
    }
    out.extend_from_slice(x);
    // right: x[n-2], x[n-3], ..., x[n-pad-1]
    for i in 1..=pad {
        let idx = n.saturating_sub(1 + i);
        out.push(x[idx]);
    }
    out
}

/// Compute whisper log-mel for a mono 16 kHz signal. Returns a flat
/// (N_MELS × N_FRAMES) = (80 × 3000) mel-major buffer. Audio is zero-padded
/// (or truncated) to exactly a 30 s window.
pub fn log_mel(audio: &[f32]) -> Vec<f32> {
    // Fixed 30 s window.
    let mut samples = vec![0.0f32; N_SAMPLES];
    let copy = audio.len().min(N_SAMPLES);
    samples[..copy].copy_from_slice(&audio[..copy]);

    let padded = reflect_pad(&samples, N_FFT / 2); // N_SAMPLES + N_FFT
    let window = hann_periodic(N_FFT);

    // FFT planner (400 is composite; rustfft picks a good algorithm).
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);

    let n_bins = N_FFT / 2 + 1; // 201
    let filters = mel_filterbank();

    // Magnitude-squared STFT: (N_FRAMES × n_bins).
    let mut mag2 = vec![0.0f32; N_FRAMES * n_bins];
    let mut frame = vec![Complex::new(0.0f32, 0.0f32); N_FFT];
    let mut scratch = vec![Complex::new(0.0f32, 0.0f32); N_FFT];

    for t in 0..N_FRAMES {
        let start = t * HOP_LENGTH;
        for i in 0..N_FFT {
            frame[i] = Complex::new(padded[start + i] * window[i], 0.0);
        }
        fft.process_with_scratch(&mut frame, &mut scratch);
        for b in 0..n_bins {
            let c = frame[b];
            mag2[t * n_bins + b] = c.re * c.re + c.im * c.im;
        }
    }

    // mel_spec (N_FRAMES × N_MELS) then log-normalize, output (N_MELS × N_FRAMES).
    let mut out = vec![0.0f32; N_MELS * N_FRAMES];
    let mut logmax = f32::NEG_INFINITY;
    // First pass: log10(clamp(mel,1e-10)) into out (mel-major layout).
    for t in 0..N_FRAMES {
        for m in 0..N_MELS {
            let mut acc = 0.0f32;
            let row = &filters[m * n_bins..(m + 1) * n_bins];
            let mrow = &mag2[t * n_bins..(t + 1) * n_bins];
            for b in 0..n_bins {
                acc += mrow[b] * row[b];
            }
            let v = acc.max(1e-10).log10();
            if v > logmax {
                logmax = v;
            }
            out[m * N_FRAMES + t] = v;
        }
    }
    // Second pass: floor at (max-8), then (x+4)/4.
    let floor = logmax - 8.0;
    for v in out.iter_mut() {
        let x = if *v < floor { floor } else { *v };
        *v = (x + 4.0) / 4.0;
    }
    out
}
