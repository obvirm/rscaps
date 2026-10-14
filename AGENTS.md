# AGENTS.md — Aturan Kerja rscaps

## ATURAN KERAS

1. **caption-core = SATU-SATUNYA sumber logika & API caption (satu titik)**
   - Semua logika caption HIDUP di `crates/caption-core`: render, template, wrap, arah teks, timing/animasi, SRT/VTT, **daftar template** (`templates::REGISTRY`), dan **default params** (`CaptionParams::default`).
   - API keluar untuk JS/app lain HANYA lewat `crates/caption-bridge` (wasm-bindgen → JS generated di `pkg/`). Satu set API, jangan ada cabang.
   - Konsumen (`studio/`, app lain) DILARANG meng-hardcode ulang daftar template, default params, tipe dokumen, atau logika apa pun. Semua diambil dari bridge: `template_catalog_json()`, `default_document_json()`, `set_document_json()`, `render_frame()`, dll.
   - Daftar template di-caption-core AUTO-GENERATE oleh `build.rs` dari folder `src/templates/<id>/` (`template.json` → label, category, galleryOrder). Menambah template = bikin folder + tambah satu baris `pub mod <id>;` di `templates/mod.rs` — REGISTRY, catalog, gallery studio-slint, dan konsumen lain ikut otomatis. Dilarang menempel daftar manual di mana pun.
   - Menambah API di caption-core/bridge: boleh — itu memperkaya satu titik. Menggandakan logika/metadata di konsumen: dilarang.
   - Fix/penyesuaian visual overlay: di `crates/studio-slint` (bukan di caption-core).

2. **`crates/studio-slint` = cross-platform (WASM + native)** — lihat seksi "Arsitektur Cross-Platform" di bawah
   - Raster overlay WASM: `crates/studio-slint/src/app/overlay_render.rs`
   - UI/state WASM: `crates/studio-slint/src/app.rs` (modul `app`, feature `wasm`)
   - Native entry: `crates/studio-slint/src/bin/studio_native.rs` → `platform/native.rs` (feature `native`)
   - **Shared (cross-platform)**: `caption-core`, `svgr`, `ui.slint` — JANGAN di-hardcode per-platform.

3. **Jangan asal patch**
   - Pahami root cause dulu sebelum fix.
   - Jangan tambal-tambal (strip filter kasar, hack font, dll) tanpa paham kenapa.
   - Bandingkan dengan tscaps (Svelte) sebagai reference visual.

## Port & Server

- Port 5173: tscaps main (Svelte)
- Port 5177: tscaps studio (Svelte)  
- Port 5178: studio-slint (trunk)
- Build: `cd crates/studio-slint; $env:PATH = "E:\temp\opencode\ffmpeg-9.0.2-full_build-shared\bin;$env:PATH"; trunk build --release`

## Arsitektur Cross-Platform (WASM + Native + Android)

`crates/studio-slint` mendukung **multi-target**. Satu codebase UI (`.slint`), dua jalur runtime.

| | WASM (browser) | Native (desktop) | Android (target masa depan) |
|---|---|---|---|
| Entry | Trunk → `#[wasm_bindgen(start)]` di `app.rs` | `bin/studio_native.rs` → `platform/native.rs` | via NDK / JNI (belum ada) |
| Feature flag | `wasm` (default Trunk) | `native` | — |
| **Decode video** | `<video>` DOM (browser HW decode) | `fframes-media::FFmpegDecoder` → `SwsScaler` → `SharedPixelBuffer` | `mediacodec` (fframes-media, HW decode) |
| **Caption raster** | `svgr` → `slint::Image` (`overlay_render.rs`) | `fframes::Previewer` → `RgbaFrame` → `slint::Image` | sama dengan native |
| **Window/render** | winit-web + femtovg (WebGL) | winit-native + femtovg (OpenGL/Vulkan) | Slint Android backend |
| **File picker** | `<input type=file>` DOM | `rfd` (native dialog) | `rfd` / Storage Access Framework |
| **Transcribe** | `whisper-spike` (WASM ONNX) | ONNX Runtime native (belum ada) | ONNX Runtime mobile |

**Prinsip:**
- **Video decode = 2 implementasi di balik 1 interface.** Browser pakai `<video>` DOM (HW decode); native pakai `FFmpegDecoder` (sudah di dependency tree fframes). TIDAK mungkin disatukan — ini batas platform.
- **Caption raster = SATU engine.** `svgr` (tiny-skia) dipakai di semua platform — preview & export pakai mesin yang sama, nol divergensi.
- **Shared module** (`caption-core`, `svgr`, `ui.slint`) jangan di-hardcode per-platform. Platform-specific code (`web_sys`, `fframes-media`, `rfd`) di-gate `cfg(feature = ...)` / `cfg(target_arch)`.

**Build native (Fase 3):**
```bash
cd crates/studio-slint
$env:PATH = "E:\temp\opencode\ffmpeg-9.0.2-full_build-shared\bin;$env:PATH"
cargo build --release --bin studio-native --features native
```
**GOTCHA WAJIB (PATH):** proc-macro `fframes-media-dir-macro` (milik fframes) mengimpor
DLL FFmpeg 9.0.2 (`avcodec-63.dll` dsb) yang hanya ada di
`E:\temp\opencode\ffmpeg-9.0.2-full_build-shared\bin`. Folder itu **bukan bagian dari
PATH default** — kalau cargo/rustc dijalankan tanpa prepend di atas, rustc gagal load
proc-macro dan gagal dengan `error[E0463]: can't find crate for fframes_media_dir_macro`
(padahal file DLL-nya ada). Semua perintah cargo yang menyentuh fframes (dev/release,
check/build, native maupun lewat wasm) HARUS memakai prepend PATH yang sama — sama
seperti perintah trunk build di atas. Kalau lihat E0463 untuk dir_macro: bukan bug kode,
PATH ffmpeg belum masuk.

Belum ada: audio playback native (`cpal`), transcribe native, export native.

**Runtime native:** exe butuh 5 DLL FFmpeg (`avutil-61, avcodec-63, avformat-63,
swscale-10, swresample-7`) — salin dari `E:\temp\opencode\ffmpeg-9.0.2-full_build-shared\bin`
ke `target\release\` (samping exe). Kalau exe gagal start dengan dialog
"avutil-61.dll was not found", itulah penyebabnya.

## Known Issues

- Selene template: text hilang di studio-slint. Akar utama (FIXED): family "Montserrat" tak resolve di fontdb rasterizer karena fontsource membawa nama internal "Montserrat Thin" — kini semua emit `font-family` lewat `stack::svg_font_family()` (font-stack: nama browser + alternat name-table), menyelesaikan juga mira/rhea/kai/lyra/pastor/luna. Sisa risiko: kalau selene MASIH kosong teks setelah fix ini, akar keduanya `filter="url(#selene-word)"` per-`<text>` pada raster svgr — perlu investigasi terpisah (jangan asal strip; pahami dulu).
- fframes `CpuFrameRenderer` tidak tersedia di WASM (gated `#[cfg(not(target_arch="wasm32"))]`).
- svgr/tiny-skia: support filter tapi text+filter bisa bermasalah — test dulu sebelum assume.
- Template yang emit `font-family` ke SVG WAJIB lewat `stack::svg_font_family()` (lihat stack.rs) — jangan tempel nama keluarga mentah, atau rasterizer non-browser bisa kehilangan teks (fontsource name-table tidak selalu sama nama @font-face).
