//! Satu sumber metadata model Whisper (arch, vocab variant, repo, sufiks
//! quant) yang dipakai jalur native dan web.
//!
//! File `config/whisper-models.json` juga diimpor langsung oleh studio
//! (`studio/src/lib/transcribe.ts`), jadi Rust dan TS tidak lagi menyimpan
//! tabel arch/vocab terpisah.

use std::sync::OnceLock;

use serde_json::Value;

static POLICY: OnceLock<Value> = OnceLock::new();

fn policy() -> &'static Value {
    POLICY.get_or_init(|| {
        serde_json::from_str(include_str!("../../../config/whisper-models.json"))
            .expect("config/whisper-models.json tidak valid")
    })
}

fn model_spec(model: &str) -> &'static Value {
    &policy()["models"][model]
}

fn u32_field(spec: &Value, key: &str, fallback: u32) -> u32 {
    spec[key].as_u64().map(|v| v as u32).unwrap_or(fallback)
}

/// (layers, dec_layers, heads, d_model, n_mels, classic) per model.
pub fn arch_table(model: &str) -> (u32, u32, u32, u32, u32, bool) {
    let spec = model_spec(model);
    if spec.is_null() {
        return arch_table("base");
    }
    (
        u32_field(spec, "layers", 6),
        u32_field(spec, "decLayers", 6),
        u32_field(spec, "heads", 8),
        u32_field(spec, "dModel", 512),
        u32_field(spec, "nMels", 80),
        spec["classic"].as_bool().unwrap_or(true),
    )
}

/// Nama folder model lokal (`whisper-<model>-<quant>`).
pub fn model_dir(model: &str, quant: &str) -> String {
    policy()["dirTemplate"]
        .as_str()
        .unwrap_or("whisper-{model}-{quant}")
        .replace("{model}", model)
        .replace("{quant}", quant)
}

/// Cari folder `models/` yang benar: cwd dulu, lalu di sekitar executable
/// (target/release/... → repo/models). Satu logika untuk CLI dan Tauri.
pub fn models_dir_for(model: &str, quant: &str) -> std::path::PathBuf {
    let sub = model_dir(model, quant);
    let mut candidates = vec![std::path::PathBuf::from("models")];
    if let Ok(exe) = std::env::current_exe() {
        for a in exe.ancestors() {
            candidates.push(a.join("models"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join(&sub).is_dir())
        .unwrap_or_else(|| std::path::PathBuf::from("models"))
}

pub fn head_dim() -> u32 {
    u32_field(policy(), "headDim", 64)
}

pub fn vocab() -> u32 {
    u32_field(policy(), "vocab", 51865)
}

pub fn max_tokens() -> u32 {
    u32_field(policy(), "maxTokens", 448)
}

/// Daftar model yang dikenal (urutan JSON).
pub fn supported_models() -> Vec<&'static str> {
    policy()["models"]
        .as_object()
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

/// Kombinasi model/quant yang sudah terbukti benar di EP WebGPU.
pub fn webgpu_safe(model: &str, quant: &str) -> bool {
    let key = format!("{model}:{quant}");
    policy()["webgpuSafe"]
        .as_array()
        .map(|a| a.iter().any(|v| v.as_str() == Some(key.as_str())))
        .unwrap_or(false)
}
