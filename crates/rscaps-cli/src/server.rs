//! Fase 3 — API server localhost (`rscaps serve`).
//!
//! Endpoints:
//! - `GET  /api/health`      → {ok, version}
//! - `GET  /api/templates`   → katalog template (caption-core)
//! - `POST /api/render`      → mulai job burn-in (path lokal) → {job}
//! - `GET  /api/jobs/{id}`   → status job (queued/running/done/error + progress)
//!
//! Render dijalankan di thread terpisah; `caption-core` menyimpan dokumen di
//! satu tempat global, jadi SEMUA render diserialisasi lewat `render_lock`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use axum::extract::{Json as JsonExtract, Path};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::render::{RenderVideoOpts, render_video};

pub struct Job {
    pub id: u64,
    pub video: String,
    pub srt: String,
    pub template: String,
    pub out: String,
    pub status: String, // queued | running | done | error
    pub done_frames: u32,
    pub total_frames: u32,
    pub error: Option<String>,
}

#[derive(Serialize)]
struct JobView {
    id: u64,
    status: String,
    done_frames: u32,
    total_frames: u32,
    video: String,
    srt: String,
    template: String,
    out: String,
    error: Option<String>,
}

impl From<&Job> for JobView {
    fn from(j: &Job) -> Self {
        Self {
            id: j.id,
            status: j.status.clone(),
            done_frames: j.done_frames,
            total_frames: j.total_frames,
            video: j.video.clone(),
            srt: j.srt.clone(),
            template: j.template.clone(),
            out: j.out.clone(),
            error: j.error.clone(),
        }
    }
}

pub struct AppState {
    jobs: Mutex<HashMap<u64, Arc<Mutex<Job>>>>,
    next_id: AtomicU64,
    render_lock: Mutex<()>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            render_lock: Mutex::new(()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderRequest {
    /// Path video lokal (mis. D:/video/in.mp4)
    video: String,
    /// Path subtitle lokal (.srt / .vtt)
    srt: String,
    template: String,
    /// Output; default: <video>_captioned.mp4 di samping video
    out: Option<String>,
    params: Option<String>,
    fonts: Option<String>,
    ffmpeg_dir: Option<String>,
    crf: Option<u32>,
    preset: Option<String>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/templates", get(templates))
        .route("/api/render", post(start_render))
        .route("/api/jobs/{id}", get(job_status))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

async fn templates() -> Json<serde_json::Value> {
    let raw = caption_core::template_catalog_json();
    match serde_json::from_str(&raw) {
        Ok(v) => Json(v),
        Err(_) => Json(serde_json::json!([])),
    }
}

async fn start_render(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    JsonExtract(req): JsonExtract<RenderRequest>,
) -> Result<Json<JobView>, (StatusCode, Json<serde_json::Value>)> {
    if req.template.is_empty() || req.video.is_empty() || req.srt.is_empty() {
        return Err(bad("field `video`, `srt`, `template` wajib diisi"));
    }
    let out: PathBuf = match req.out.clone() {
        Some(o) => PathBuf::from(o),
        None => {
            let video = PathBuf::from(&req.video);
            let stem = video
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "video".into());
            video
                .with_file_name(format!("{stem}_captioned.mp4"))
        }
    };

    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let job = Arc::new(Mutex::new(Job {
        id,
        video: req.video.clone(),
        srt: req.srt.clone(),
        template: req.template.clone(),
        out: out.to_string_lossy().to_string(),
        status: "queued".into(),
        done_frames: 0,
        total_frames: 0,
        error: None,
    }));
    state.jobs.lock().unwrap().insert(id, job.clone());

    let view = JobView::from(&*job.lock().unwrap());
    let app_state = state.clone();
    let job_in = job;
    std::thread::spawn(move || {
        // Satu render pada satu waktu (dokumen caption-core global).
        let _guard = app_state.render_lock.lock();
        {
            let mut j = job_in.lock().unwrap();
            j.status = "running".into();
        }
        let video_path = PathBuf::from(&req.video);
        let srt_path = PathBuf::from(&req.srt);
        let params_path = req.params.as_deref().map(PathBuf::from);
        let fonts_path = req.fonts.as_deref().map(PathBuf::from);
        let ffmpeg_path = req.ffmpeg_dir.as_deref().map(PathBuf::from);
        let opts = RenderVideoOpts {
            engine: "cpu",
            hw_encode: false,
            hw_encoder: "h264_nvenc",
            hwaccel: None,
            video: video_path.as_path(),
            srt: srt_path.as_path(),
            template: &req.template,
            out: out.as_path(),
            params: params_path.as_deref(),
            fonts: fonts_path.as_deref(),
            ffmpeg_dir: ffmpeg_path.as_deref(),
            crf: req.crf.unwrap_or(18),
            preset: req.preset.as_deref().unwrap_or("medium"),
        };
        let job_c = job_in.clone();
        let result = render_video(&opts, |done, total| {
            let mut j = job_c.lock().unwrap();
            j.done_frames = done;
            j.total_frames = total;
        });
        let mut j = job_in.lock().unwrap();
        match result {
            Ok(()) => j.status = "done".into(),
            Err(err) => {
                j.status = "error".into();
                j.error = Some(format!("{err:#}"));
            }
        }
    });

    Ok(Json(view))
}

async fn job_status(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Path(id): Path<u64>,
) -> Result<Json<JobView>, (StatusCode, Json<serde_json::Value>)> {
    let jobs = state.jobs.lock().unwrap();
    match jobs.get(&id) {
        Some(job) => Ok(Json(JobView::from(&*job.lock().unwrap()))),
        None => Err(bad(&format!("job {id} tidak ditemukan"))),
    }
}

fn bad(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}

pub async fn serve(bind: &str, port: u16) -> Result<()> {
    let state = Arc::new(AppState::new());
    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|err| anyhow!("bind {addr}: {err}"))?;
    eprintln!("rscaps serve → http://{addr}");
    eprintln!("  GET  /api/health");
    eprintln!("  GET  /api/templates");
    eprintln!("  POST /api/render      {{video, srt, template, out?, params?}}");
    eprintln!("  GET  /api/jobs/{{id}}");
    axum::serve(listener, router(state))
        .await
        .map_err(|err| anyhow!("serve: {err}"))?;
    Ok(())
}
