//! CLI transkripsi native: audio apapun (ffmpeg untuk non-WAV, WAV 16k mono
//! dibaca langsung) → JSON transkrip di stdout.
//!
//! Contoh: transcribe-cli E:\audio.mp4 --model base --lang id
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut audio: Option<String> = None;
    let mut model = "base".to_string();
    let mut lang = "id".to_string();
    let mut models_dir = default_models_dir();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().unwrap_or(model),
            "--lang" => lang = args.next().unwrap_or(lang),
            "--models-dir" => models_dir = PathBuf::from(args.next().unwrap_or_default()),
            "--lang-help" => {
                println!("id zh de es ru ko ja it fr pt hi th vi pl ar ms tr nl (default en)");
                return;
            }
            _ => audio = Some(a),
        }
    }
    let audio = match audio {
        Some(a) => a,
        None => {
            eprintln!("pakai: transcribe-cli <audio> [--model tiny|base|small|medium|large|turbo] [--lang id] [--models-dir path]");
            std::process::exit(2);
        }
    };

    let t0 = std::time::Instant::now();
    let samples = match load_audio(Path::new(&audio)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("audio gagal: {e}");
            std::process::exit(1);
        }
    };
    let load_ms = t0.elapsed().as_millis();

    let dir = models_dir.join(format!("whisper-{model}-fp16"));
    let t1 = std::time::Instant::now();
    let mut engine = match whisper_spike::native::NativeEngine::load(&model, &dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("model gagal dimuat ({}): {e}", dir.display());
            std::process::exit(1);
        }
    };
    let model_ms = t1.elapsed().as_millis();

    let t2 = std::time::Instant::now();
    let out = match engine.transcribe(
        &samples,
        whisper_spike::native::lang_token(&lang),
        true,
        448,
    ) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("transkripsi gagal: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "audio {:.1}s · decode {:.1}s · model load {}ms · audio read {}ms",
        samples.len() as f32 / 16000.0,
        t2.elapsed().as_secs_f32(),
        model_ms,
        load_ms
    );
    println!("{out}");
}

fn default_models_dir() -> PathBuf {
    // cwd dulu, lalu relative ke exe (target/... → repo/models).
    let cwd = PathBuf::from("models");
    if cwd.exists() {
        return cwd;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.ancestors().nth(4).map(|r| r.join("models")))
        .filter(|p| p.exists())
        .unwrap_or(cwd)
}

/// Baca WAV PCM16/f32 mono 16 kHz; format lain → ffmpeg → WAV 16k mono (tmp).
fn load_audio(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if let Some(f) = parse_wav(&bytes) {
        return Ok(f);
    }
    // Non-WAV: decode via ffmpeg (harus ada di PATH).
    let tmp = std::env::temp_dir().join(format!("rscaps-cli-{}.wav", std::process::id()));
    let status = Command::new("ffmpeg")
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
