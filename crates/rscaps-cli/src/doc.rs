//! Pembangunan dokumen caption dari file subtitle — dipakai `frames`,
//! `render`, dan `serve`. Semua melalui API caption-core (satu titik).

use std::path::Path;

use anyhow::{Context, Result, bail};

use caption_core::CaptionDocument;

pub fn ext_format(path: &Path) -> Result<String> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("vtt") => Ok("vtt".into()),
        Some("srt") => Ok("srt".into()),
        other => bail!("ekstensi subtitle tak dikenal: {other:?} (pakai .srt / .vtt)"),
    }
}

pub fn parse_size(size: &str) -> Result<(u32, u32)> {
    let (w, h) = size
        .split_once(['x', 'X'])
        .ok_or_else(|| anyhow::anyhow!("format ukuran: <lebar>x<tinggi> (didapat: {size:?})"))?;
    let w: u32 = w.trim().parse().context("lebar bukan angka")?;
    let h: u32 = h.trim().parse().context("tinggi bukan angka")?;
    if w == 0 || h == 0 {
        bail!("ukuran harus > 0");
    }
    Ok((w, h))
}

/// Parse file subtitle → set dokumen di engine (+ patch params opsional) →
/// baca balik `document()` (termasuk hasil patch) sebagai satu sumber kebenaran.
///
/// `duration_override` menggantikan durasi hasil parse (mis. durasi video
/// untuk burn-in, supaya timing engine menutupi seluruh video).
pub fn build_document(
    srt: &Path,
    template: &str,
    params: Option<&Path>,
    duration_override: Option<u32>,
) -> Result<CaptionDocument> {
    let text =
        std::fs::read_to_string(srt).with_context(|| format!("baca {}", srt.display()))?;
    let from = ext_format(srt)?;
    let parsed: serde_json::Value = serde_json::from_str(
        &caption_core::parse_subtitle_json(&text, &from)
            .map_err(|err| anyhow::anyhow!("parse subtitle: {err}"))?,
    )?;
    let duration_frames = duration_override
        .unwrap_or(parsed["duration_frames"].as_u64().unwrap_or(0).max(1) as u32);
    let cues: Vec<caption_core::Cue> = serde_json::from_value(parsed["cues"].clone())?;

    let mut doc_json: serde_json::Value =
        serde_json::from_str(&caption_core::default_document_json())?;
    doc_json["duration_frames"] = serde_json::json!(duration_frames);
    doc_json["cues"] = serde_json::to_value(&cues)?;
    doc_json["template"] = serde_json::json!(template);
    caption_core::set_document_json(&doc_json.to_string())
        .map_err(|err| anyhow::anyhow!("set document: {err}"))?;

    if let Some(path) = params {
        let patch =
            std::fs::read_to_string(path).with_context(|| format!("baca params {}", path.display()))?;
        caption_core::patch_params_json(&patch)
            .map_err(|err| anyhow::anyhow!("patch params: {err}"))?;
    }

    let doc: CaptionDocument = caption_core::document()
        .read()
        .map_err(|_| anyhow::anyhow!("document lock poisoned"))?
        .clone();
    if doc.template != template {
        bail!(
            "template tak dikenal: {template:?} (pakai `rscaps templates`)"
        );
    }
    Ok(doc)
}

pub fn candidate_font_dirs(extra: Option<&Path>) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = extra {
        dirs.push(dir.to_path_buf());
    }
    dirs.push(std::path::PathBuf::from("studio/tplfonts/ttf"));
    dirs.push(std::path::PathBuf::from("tplfonts/ttf"));
    dirs
}
