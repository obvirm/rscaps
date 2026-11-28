use rscaps_cli::backend::{AsrBackend, CaptionBackend, EncodeBackend};
use rscaps_cli::{render_video, RenderVideoOpts, probe_video, tool_bin, hw_encoder_available};
use caption_core::{templates::{render as render_template, template_catalog}, CaptionParams, CaptionDocument};
use fframes::{FFramesContext, FFramesMode, TimeBase, VideoSize};
use std::path::Path;
use tauri::Emitter;
use tempfile::tempdir;

/// Dialog Save native → path terpilih (dibuat kosong). Chunk ditulis via
/// `save_export_append` — blob besar tak pernah lewat satu payload IPC.
#[tauri::command]
fn save_export_pick(
    app: tauri::AppHandle,
    file_name: String,
    filter_name: Option<String>,
    filter_ext: Option<Vec<String>>,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;

    let mut dialog = app.dialog().file().set_file_name(&file_name);
    if let (Some(name), Some(exts)) = (filter_name, filter_ext) {
        let ext_refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name, &ext_refs);
    }
    let picked = dialog
        .blocking_save_file()
        .ok_or_else(|| "cancel".to_string())?;
    let path = picked.into_path().map_err(|e| format!("path: {e}"))?;
    std::fs::write(&path, b"").map_err(|e| format!("create: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Tulis satu chunk (append) ke path hasil `save_export_pick`.
#[tauri::command]
fn save_export_append(path: String, data: Vec<u8>) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|e| format!("open: {e}"))?;
    f.write_all(&data).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Info shell: versi + penanda desktop (untuk badge/fitur khusus shell).
#[tauri::command]
fn shell_info() -> serde_json::Value {
    serde_json::json!({
        "shell": "tauri",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// Dapatkan direktori temp sistem untuk file sementara export.
#[tauri::command]
fn native_get_temp_dir() -> Result<String, String> {
    let dir = tempdir().map_err(|e| format!("temp dir: {e}"))?;
    let path = dir.keep();
    Ok(path.to_string_lossy().into_owned())
}

/// Tulis file (untuk video/SRT/params sementara).
#[tauri::command]
fn native_write_file(path: String, data: Vec<u8>) -> Result<(), String> {
    std::fs::write(&path, data).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Probe video metadata (width, height, fps, duration, has_audio).
#[tauri::command]
async fn native_probe_video(
    video_path: String,
    ffmpeg_dir: Option<String>,
) -> Result<NativeVideoProbe, String> {
    let ffprobe = tool_bin("ffprobe", ffmpeg_dir.as_deref().map(Path::new))
        .map_err(|e| format!("ffprobe: {e}"))?;
    let probe = probe_video(&ffprobe, Path::new(&video_path))
        .map_err(|e| format!("probe: {e}"))?;
    Ok(NativeVideoProbe {
        width: probe.width,
        height: probe.height,
        fps_num: probe.fps_num,
        fps_den: probe.fps_den,
        duration_sec: probe.duration_sec,
        has_audio: probe.has_audio,
    })
}

/// Cek ketersediaan encoder hardware di ffmpeg.
#[tauri::command]
async fn native_check_hw_encoder(
    ffmpeg_dir: Option<String>,
    encoder: String,
) -> Result<bool, String> {
    let ffmpeg = tool_bin("ffmpeg", ffmpeg_dir.as_deref().map(Path::new))
        .map_err(|e| format!("ffmpeg: {e}"))?;
    Ok(hw_encoder_available(&ffmpeg, &encoder))
}

/// Status backend untuk UI: semua opsi tampil, yang tidak tersedia diberi
/// alasan eksplisit (tidak ada fallback diam).
#[tauri::command]
async fn native_backend_status(ffmpeg_dir: Option<String>) -> Result<serde_json::Value, String> {
    let ffmpeg = tool_bin("ffmpeg", ffmpeg_dir.as_deref().map(Path::new)).ok();
    let encoder = |name: &str| -> bool {
        ffmpeg
            .as_deref()
            .map(|path| hw_encoder_available(path, name))
            .unwrap_or(false)
    };
    let caption = vec![
        serde_json::json!({"value": "auto", "enabled": true, "reason": null}),
        serde_json::json!({"value": "tiny-skia", "enabled": true, "reason": null}),
        serde_json::json!({"value": "skia-gpu", "enabled": true, "reason": null}),
        serde_json::json!({"value": "skia-cpu", "enabled": true, "reason": null}),
        serde_json::json!({"value": "fframes", "enabled": true, "reason": null}),
        serde_json::json!({"value": "fframes-pipeline", "enabled": true, "reason": null}),
        serde_json::json!({"value": "wgpu-lyon", "enabled": cfg!(feature = "wgpu-lyon"), "reason": if cfg!(feature = "wgpu-lyon") { serde_json::Value::Null } else { serde_json::json!("build tanpa wgpu-lyon") }}),
    ];
    let nvenc = encoder("h264_nvenc");
    let qsv = encoder("h264_qsv");
    let amf = encoder("h264_amf");
    let vaapi = encoder("h264_vaapi");
    let videotoolbox = encoder("h264_videotoolbox");
    let encode = vec![
        serde_json::json!({"value": "auto", "enabled": true, "reason": null}),
        serde_json::json!({"value": "cpu", "enabled": true, "reason": null}),
        serde_json::json!({"value": "nvenc", "enabled": nvenc, "reason": if nvenc { serde_json::Value::Null } else { serde_json::json!("h264_nvenc tidak ada di ffmpeg") }}),
        serde_json::json!({"value": "qsv", "enabled": qsv, "reason": if qsv { serde_json::Value::Null } else { serde_json::json!("h264_qsv tidak ada di ffmpeg") }}),
        serde_json::json!({"value": "amf", "enabled": amf, "reason": if amf { serde_json::Value::Null } else { serde_json::json!("h264_amf tidak ada di ffmpeg") }}),
        serde_json::json!({"value": "vaapi", "enabled": vaapi, "reason": if vaapi { serde_json::Value::Null } else { serde_json::json!("h264_vaapi tidak ada di ffmpeg") }}),
        serde_json::json!({"value": "videotoolbox", "enabled": videotoolbox, "reason": if videotoolbox { serde_json::Value::Null } else { serde_json::json!("h264_videotoolbox tidak ada di ffmpeg") }}),
    ];
    let asr = vec![
        serde_json::json!({"value": "auto", "enabled": true, "reason": null}),
        serde_json::json!({"value": "cpu", "enabled": true, "reason": null}),
        serde_json::json!({"value": "cuda", "enabled": cfg!(feature = "gpu-cuda"), "reason": if cfg!(feature = "gpu-cuda") { serde_json::Value::Null } else { serde_json::json!("build tanpa gpu-cuda") }}),
        serde_json::json!({"value": "dml", "enabled": cfg!(feature = "gpu-directml"), "reason": if cfg!(feature = "gpu-directml") { serde_json::Value::Null } else { serde_json::json!("build tanpa gpu-directml") }}),
        serde_json::json!({"value": "vulkan", "enabled": cfg!(feature = "gpu-webgpu"), "reason": if cfg!(feature = "gpu-webgpu") { serde_json::Value::Null } else { serde_json::json!("build tanpa gpu-webgpu") }}),
        serde_json::json!({"value": "boost", "enabled": cfg!(feature = "gpu-cuda") && cfg!(feature = "gpu-directml"), "reason": if cfg!(feature = "gpu-cuda") && cfg!(feature = "gpu-directml") { serde_json::Value::Null } else { serde_json::json!("build tanpa cuda+directml") }}),
    ];
    Ok(serde_json::json!({ "caption": caption, "encode": encode, "asr": asr }))
}

/// Opsi render native (mirip CLI `RenderVideoOpts`).
#[derive(serde::Deserialize)]
struct NativeRenderOpts {
    /// Backend caption baru: auto|tiny-skia|skia-gpu|skia-cpu|fframes|fframes-pipeline
    cap: Option<String>,
    /// Encoder baru: auto|cpu|nvenc|qsv|amf|vaapi|videotoolbox
    enc: Option<String>,
    /// Alias lama dari UI/build sebelumnya.
    engine: Option<String>,
    hw_encode: Option<bool>,
    hw_encoder: Option<String>,
    hwaccel: Option<String>,  // "auto" | "cuda" | "d3d11va" | "vaapi" | "qsv"
    video_path: String,
    srt_path: String,
    template: String,
    out_path: String,
    params_json: Option<String>,
    fonts_dir: Option<String>,
    ffmpeg_dir: Option<String>,
    crf: u32,
    preset: String,
}

/// Lock encoder native sebelum job. `auto` memilih yang benar-benar ada;
/// pilihan eksplisit yang tidak ada adalah hard error (tanpa fallback diam).
fn resolve_encode_native(
    enc: EncodeBackend,
    ffmpeg: &Path,
) -> Result<(bool, &'static str), String> {
    if enc == EncodeBackend::Cpu {
        return Ok((false, "libx264"));
    }
    if let Some(name) = enc.hw_encoder() {
        if !hw_encoder_available(ffmpeg, name) {
            return Err(format!(
                "encoder {name} tidak tersedia di build ffmpeg ini; pilih enc=cpu atau encoder lain"
            ));
        }
        return Ok((true, name));
    }
    for name in rscaps_cli::backend::AUTO_ENCODERS {
        if hw_encoder_available(ffmpeg, name) {
            return Ok((true, name));
        }
    }
    Ok((false, "libx264"))
}

/// Progress event untuk frontend.
#[derive(serde::Serialize, Clone)]
struct RenderProgress {
    current: u32,
    total: u32,
    stage: String,
}

/// Render video via native pipeline (CLI engines).
#[tauri::command]
async fn native_render_video(
    app: tauri::AppHandle,
    opts: NativeRenderOpts,
) -> Result<(), String> {
    // Backend caption + encode: satu kontrak dengan CLI. `auto` mengunci satu
    // kandidat; pilihan eksplisit yang tidak tersedia adalah hard error.
    let cap_raw = opts
        .cap
        .as_deref()
        .or(opts.engine.as_deref())
        .unwrap_or("auto");
    let cap = CaptionBackend::parse(cap_raw)
        .map_err(|e| format!("caption backend: {e}"))?
        .resolve()
        .map_err(|e| format!("caption backend: {e}"))?;
    let enc_raw = opts.enc.as_deref().unwrap_or_else(|| {
        if opts.hw_encode.unwrap_or(false) {
            opts.hw_encoder.as_deref().unwrap_or("auto")
        } else {
            "auto"
        }
    });
    let enc = EncodeBackend::parse(enc_raw).map_err(|e| format!("encode backend: {e}"))?;
    let ffmpeg = tool_bin("ffmpeg", opts.ffmpeg_dir.as_deref().map(Path::new))
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let (hw_encode, hw_encoder) = resolve_encode_native(enc, &ffmpeg)?;

    // Clone string values before moving into closure (lifetime issue with spawn_blocking)
    let engine = cap.engine_arg().to_string();
    let hw_encoder = hw_encoder.to_string();
    let hwaccel = opts.hwaccel.clone();
    let video_path = opts.video_path.clone();
    let srt_path = opts.srt_path.clone();
    let out_path = opts.out_path.clone();
    let params_json = opts.params_json.clone();
    let fonts_dir = opts.fonts_dir.clone();
    let ffmpeg_dir = opts.ffmpeg_dir.clone();
    let crf = opts.crf;
    let preset = opts.preset.clone();
    let template = opts.template.clone();

    // Emit progress awal
    let _ = app.emit("native-render-progress", RenderProgress {
        current: 0,
        total: 1,
        stage: "memulai".to_string(),
    });

    // Jalankan render di thread terpisah (blocking) dengan progress callback
    let app_handle = app.clone();
    tokio::task::spawn_blocking(move || {
        let video_path = Path::new(&video_path);
        let srt_path = Path::new(&srt_path);
        let out_path = Path::new(&out_path);
        let params_path = params_json.as_deref().map(Path::new);
        let fonts_dir = fonts_dir.as_deref().map(Path::new);
        let ffmpeg_dir = ffmpeg_dir.as_deref().map(Path::new);

        let render_opts = RenderVideoOpts {
            engine: &engine,
            hw_encode,
            hw_encoder: &hw_encoder,
            hwaccel: hwaccel.as_deref(),
            video: video_path,
            srt: srt_path,
            template: &template,
            out: out_path,
            params: params_path,
            fonts: fonts_dir,
            ffmpeg_dir,
            crf,
            preset: &preset,
        };

        render_video(&render_opts, |current, total| {
            let stage = if current == 0 { "memulai" }
                else if current >= total { "selesai" }
                else { "rendering" };
            let _ = app_handle.emit("native-render-progress", RenderProgress {
                current,
                total,
                stage: stage.to_string(),
            });
        })
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
    .map_err(|e| format!("render gagal: {e}"))?;

    Ok(())
}

/// Daftar template dari caption-core.
#[tauri::command]
async fn native_list_templates() -> Result<Vec<NativeTemplate>, String> {
    let templates: Vec<NativeTemplate> = template_catalog().iter().map(|t| NativeTemplate {
        id: t.id.to_string(),
        label: t.label.to_string(),
        category: t.category.to_string(),
        gallery_order: t.gallery_order.unwrap_or(0),
    }).collect();
    Ok(templates)
}

/// Render preview frame SVG (untuk template gallery native).
#[tauri::command]
async fn native_render_preview_svg(
    template: String,
    width: u32,
    height: u32,
    params_json: Option<String>,
) -> Result<String, String> {
    let doc = if let Some(p) = params_json {
        let params: CaptionParams = serde_json::from_str(&p)
            .map_err(|e| format!("params JSON invalid: {e}"))?;
        CaptionDocument {
            template,
            params,
            ..Default::default()
        }
    } else {
        CaptionDocument {
            template,
            ..Default::default()
        }
    };

    let ctx = FFramesContext {
        time_base: TimeBase {
            fps: 30,
            sample_rate: 48000,
        },
        current_video_size: VideoSize {
            width: width as usize,
            height: height as usize,
        },
        duration_in_frames: 30,
        mode: FFramesMode::Renderer,
        scenes: None,
        media_source: None,
        font_source: None,
        abort_signal: None,
    };

    let svg_opt = render_template(&doc.template, &doc, 500.0, &ctx);  // t=0.5s = 500ms
    let svg = svg_opt.ok_or_else(|| format!("template tidak dikenal"))?;

    Ok(svg.to_string())
}

/// Struct untuk response probe video.
#[derive(serde::Serialize)]
struct NativeVideoProbe {
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    duration_sec: f64,
    has_audio: bool,
}

/// Struct untuk template catalog.
#[derive(serde::Serialize)]
struct NativeTemplate {
    id: String,
    label: String,
    category: String,
    gallery_order: u32,
}

// ===== Transkrip native (engine ORT penuh — BUKAN WASM) =====

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Progress transkrip per run_id: (fraction 0..1, stage).
static TRANSCRIBE_PROG: LazyLock<Mutex<HashMap<String, (f64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn set_prog(run_id: &str, frac: f64, stage: &str) {
    if let Ok(mut m) = TRANSCRIBE_PROG.lock() {
        m.insert(run_id.to_string(), (frac, stage.to_string()));
    }
}

/// Validasi pilihan ASR terhadap EP yang benar-benar dikompilasi. `auto`
/// membiarkan build memilih; pilihan eksplisit yang tidak ada = hard error.
fn asr_env_mode(asr: AsrBackend) -> Result<Option<&'static str>, String> {
    match asr {
        AsrBackend::Auto => Ok(None),
        AsrBackend::Cpu => Ok(Some("off")),
        AsrBackend::Cuda => {
            if cfg!(feature = "gpu-cuda") {
                Ok(Some("cuda"))
            } else {
                Err("ASR cuda tidak tersedia di build ini; pilih cpu/auto".into())
            }
        }
        AsrBackend::Dml => {
            if cfg!(feature = "gpu-directml") {
                Ok(Some("full"))
            } else {
                Err("ASR dml tidak tersedia di build ini; pilih cpu/auto".into())
            }
        }
        AsrBackend::Vulkan => {
            if cfg!(feature = "gpu-webgpu") {
                Ok(Some("vulkan"))
            } else {
                Err("ASR vulkan tidak tersedia di build ini; pilih cpu/auto".into())
            }
        }
        AsrBackend::Boost => {
            if cfg!(feature = "gpu-cuda") && cfg!(feature = "gpu-directml") {
                Ok(Some("cuda-dml"))
            } else {
                Err("ASR boost butuh build cuda+directml; pilih cpu/auto".into())
            }
        }
    }
}

/// Poll progress dari frontend (invoke per ~500 ms).
#[tauri::command]
fn transcribe_native_progress(run_id: String) -> (f64, String) {
    TRANSCRIBE_PROG
        .lock()
        .ok()
        .and_then(|m| m.get(&run_id).cloned())
        .unwrap_or((0.0, "engine".to_string()))
}

#[tauri::command]
async fn transcribe_native(
    path: String,
    model: String,
    quant: String,
    lang: String,
    run_id: String,
    ffmpeg_dir: Option<String>,
    asr: Option<String>,
) -> Result<serde_json::Value, String> {
    let asr = AsrBackend::parse(asr.as_deref().unwrap_or("auto"))
        .map_err(|e| format!("ASR backend: {e}"))?;
    let mode = asr_env_mode(asr)?;
    let previous = std::env::var_os("RSCAPS_GPU_MODE");
    if let Some(mode) = mode {
        // NativeEngine membaca env saat load/transcribe; transkripsi native
        // berjalan serial, lalu env dikembalikan seperti semula.
        unsafe { std::env::set_var("RSCAPS_GPU_MODE", mode) };
    }
    // ffmpeg: eksplisit bila diberi, else PATH (tool_bin punya fallback bundel).
    let ff = rscaps_cli::tool_bin("ffmpeg", ffmpeg_dir.as_deref().map(Path::new)).ok();
    let ff2 = ff.clone();
    let handle = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        set_prog(&run_id, 0.05, "audio");
        let samples = whisper_spike::native::load_audio(Path::new(&path), ff2.as_deref())
            .map_err(|e| format!("audio: {e}"))?;
        set_prog(&run_id, 0.12, "audio");

        let models_dir = whisper_spike::policy::models_dir_for(&model, &quant);
        let dir = models_dir.join(whisper_spike::policy::model_dir(&model, &quant));
        set_prog(&run_id, 0.2, "model");
        let mut engine = whisper_spike::native::NativeEngine::load(&model, &dir, &quant)
            .map_err(|e| format!("model {dir:?}: {e}"))?;
        set_prog(&run_id, 0.4, "transcribe");

        let run_id2 = run_id.clone();
        let (segments, text) = engine
            .transcribe_windows(&samples, &lang, &mut |f, stage| {
                set_prog(&run_id2, f, stage);
            })
            .map_err(|e| format!("transkripsi: {e}"))?;

        let segs: Vec<serde_json::Value> = segments
            .iter()
            .map(|s| serde_json::json!({"start": s.start, "end": s.end, "text": s.text}))
            .collect();
        Ok(serde_json::json!({ "text": text, "segments": segs }))
    });
    let result = handle
        .await
        .map_err(|e| format!("join: {e}"))?;
    unsafe {
        match previous {
            Some(value) => std::env::set_var("RSCAPS_GPU_MODE", value),
            None => std::env::remove_var("RSCAPS_GPU_MODE"),
        }
    }
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Info
                } else {
                    log::LevelFilter::Warn
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            save_export_pick,
            save_export_append,
            shell_info,
            native_get_temp_dir,
            native_write_file,
            native_probe_video,
            native_check_hw_encoder,
            native_backend_status,
            native_render_video,
            native_list_templates,
            native_render_preview_svg,
            transcribe_native,
            transcribe_native_progress,
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
