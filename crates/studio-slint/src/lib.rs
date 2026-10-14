// ═════════════════════════════════════════════════════════════════════════
// studio-slint — cross-platform (WASM + native). LIHAT AGENTS.md.
//
// Layout crate:
//   src/app.rs          — WASM app (jalur browser; di-gate feature "wasm")
//   src/overlay_render.rs — raster caption svgr (WASM; jalur native di
//                          platform/native/ pakai fframes Previewer)
//   src/wasm_mods.rs    — glue WASM ke pkg standalone (whisper dsb)
//   src/platform/native.rs — bin native (fitur "native"): window + video
//   src/bin/studio_native.rs — entry point native
//
// Shared (cross-platform, TANPA gate): caption-core, svgr, ui.slint.
// ═════════════════════════════════════════════════════════════════════════

// Shared (cross-platform, TANPA gate): caption-core, svgr, ui.slint,
// ui_common (glue fmt/cue/template tanpa dependensi web).
pub mod ui_common;

// ── Jalur WASM (target wasm32; Trunk build) ──────────────────────────────
#[cfg(target_arch = "wasm32")]
mod app;

#[cfg(target_arch = "wasm32")]
pub use app::*;

// ── Jalur native (fitur "native"; crate bin terpisah) ─────────────────────
#[cfg(feature = "native")]
pub mod platform {
    pub mod native;
}
