//! CLI transkripsi native: audio apapun (ffmpeg untuk non-WAV, WAV 16k mono
//! dibaca langsung) → JSON transkrip di stdout.
//!
//! Contoh: transcribe-cli E:\audio.mp4 --model base --quant q8 --lang id
use std::path::{Path, PathBuf};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut audio: Option<String> = None;
    let mut model = "base".to_string();
    let mut lang = "id".to_string();
    let mut quant = "fp16".to_string();
    let mut models_dir = default_models_dir();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().unwrap_or(model),
            "--lang" => lang = args.next().unwrap_or(lang),
            "--quant" => quant = args.next().unwrap_or(quant),
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
            eprintln!("pakai: transcribe-cli <audio> [--model tiny|base|small|medium|large|turbo] [--quant fp16|q8|q4|q4f16|int8|uint8|bnb4] [--lang id] [--models-dir path]");
            std::process::exit(2);
        }
    };

    // Backend ggml (whisper.cpp): GPU-native, CPU hanya orkestrasi.
    if std::env::var("RSCAPS_BACKEND").as_deref() == Ok("ggml") {
        let t = std::time::Instant::now();
        let (segments, text) =
            match whisper_spike::native::transcribe_ggml(Path::new(&audio), &model, &lang) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("ggml gagal: {e}");
                    std::process::exit(1);
                }
            };
        let segs: Vec<serde_json::Value> = segments
            .iter()
            .map(|s| serde_json::json!({"start": s.start, "end": s.end, "text": s.text}))
            .collect();
        eprintln!("ggml decode {:.1}s", t.elapsed().as_secs_f32());
        println!("{}", serde_json::json!({"text": text, "segments": segs}));
        return;
    }

    let t0 = std::time::Instant::now();
    let samples = match whisper_spike::native::load_audio(Path::new(&audio), None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("audio gagal: {e}");
            std::process::exit(1);
        }
    };
    let load_ms = t0.elapsed().as_millis();

    let dir = models_dir.join(format!("whisper-{model}-{quant}"));
    let t1 = std::time::Instant::now();
    let mut engine =
        match whisper_spike::native::NativeEngine::load(&model, &dir, &quant) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("model gagal dimuat ({}): {e}", dir.display());
                std::process::exit(1);
            }
        };
    let model_ms = t1.elapsed().as_millis();

    let t2 = std::time::Instant::now();
    let mut win = 0usize;
    let res = engine.transcribe_windows(
        &samples,
        &lang,
        &mut |f, stage| {
            win += 1;
            eprintln!("[{stage} {win} {f:.2}]");
        },
    );
    let out = match res {
        Ok((segments, text)) => {
            let segs: Vec<serde_json::Value> = segments
                .iter()
                .map(|s| serde_json::json!({"start": s.start, "end": s.end, "text": s.text}))
                .collect();
            serde_json::json!({ "text": text, "segments": segs }).to_string()
        }
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
