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

/// Opsi render native (mirip CLI `RenderVideoOpts`).
#[derive(serde::Deserialize)]
struct NativeRenderOpts {
    engine: String,           // "cpu" | "skia" | "fframes"
    hw_encode: bool,
    hw_encoder: String,       // "h264_nvenc" | "h264_qsv" | "h264_vaapi" | "h264_videotoolbox" | "h264_amf"
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
    // Validasi engine
    let valid_engines = ["cpu", "skia", "fframes"];
    if !valid_engines.contains(&opts.engine.as_str()) {
        return Err(format!("engine tidak valid: {} (pilih: cpu, skia, fframes)", opts.engine));
    }
    // skia butuh feature gpu-skia
    #[cfg(not(feature = "gpu-skia"))]
    if opts.engine == "skia" {
        return Err("engine skia butuh build dengan --features gpu-skia".to_string());
    }

    // Clone string values before moving into closure (lifetime issue with spawn_blocking)
    let engine = opts.engine.clone();
    let hw_encode = opts.hw_encode;
    let hw_encoder = opts.hw_encoder.clone();
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
            native_render_video,
            native_list_templates,
            native_render_preview_svg,
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
