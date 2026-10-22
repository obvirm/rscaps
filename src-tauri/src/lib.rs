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
            shell_info
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
