# AGENTS.md — Aturan Kerja rscaps

## ATURAN KERAS

1. **caption-core = SATU-SATUNYA sumber logika & API caption (satu titik)**
   - Semua logika caption HIDUP di `crates/caption-core`: render, template, wrap, arah teks, timing/animasi, SRT/VTT, **daftar template** (`templates::REGISTRY`), dan **default params** (`CaptionParams::default`).
   - API keluar untuk JS/app lain HANYA lewat `crates/caption-bridge` (wasm-bindgen → JS generated di `pkg/`). Satu set API, jangan ada cabang.
   - Konsumen (`studio/`, app lain) DILARANG meng-hardcode ulang daftar template, default params, tipe dokumen, atau logika apa pun. Semua diambil dari bridge: `template_catalog_json()`, `default_document_json()`, `set_document_json()`, `render_frame()`, dll.
   - Daftar template di-caption-core AUTO-GENERATE oleh `build.rs` dari folder `src/templates/<id>/` (`template.json` → label, category, galleryOrder). Menambah template = bikin folder + tambah satu baris `pub mod <id>;` di `templates/mod.rs` — REGISTRY, catalog, gallery studio-slint, dan konsumen lain ikut otomatis. Dilarang menempel daftar manual di mana pun.
   - Menambah API di caption-core/bridge: boleh — itu memperkaya satu titik. Menggandakan logika/metadata di konsumen: dilarang.
   - Fix/penyesuaian visual overlay: di `crates/studio-slint` (bukan di caption-core).

2. **HANYA boleh edit `E:\project\rscaps\crates\studio-slint`** untuk keperluan overlay desktop
   - Fix rendering overlay: `crates/studio-slint/src/overlay_render.rs`
   - UI/state: `crates/studio-slint/src/lib.rs`

3. **Jangan asal patch**
   - Pahami root cause dulu sebelum fix.
   - Jangan tambal-tambal (strip filter kasar, hack font, dll) tanpa paham kenapa.
   - Bandingkan dengan tscaps (Svelte) sebagai reference visual.

## Port & Server

- Port 5173: tscaps main (Svelte)
- Port 5177: tscaps studio (Svelte)  
- Port 5178: studio-slint (trunk)
- Build: `cd crates/studio-slint; $env:PATH = "E:\temp\opencode\ffmpeg-9.0.2-full_build-shared\bin;$env:PATH"; trunk build --release`

## Known Issues

- Selene template: text hilang di studio-slint. Akar utama (FIXED): family "Montserrat" tak resolve di fontdb rasterizer karena fontsource membawa nama internal "Montserrat Thin" — kini semua emit `font-family` lewat `stack::svg_font_family()` (font-stack: nama browser + alternat name-table), menyelesaikan juga mira/rhea/kai/lyra/pastor/luna. Sisa risiko: kalau selene MASIH kosong teks setelah fix ini, akar keduanya `filter="url(#selene-word)"` per-`<text>` pada raster svgr — perlu investigasi terpisah (jangan asal strip; pahami dulu).
- fframes `CpuFrameRenderer` tidak tersedia di WASM (gated `#[cfg(not(target_arch="wasm32"))]`).
- svgr/tiny-skia: support filter tapi text+filter bisa bermasalah — test dulu sebelum assume.
- Template yang emit `font-family` ke SVG WAJIB lewat `stack::svg_font_family()` (lihat stack.rs) — jangan tempel nama keluarga mentah, atau rasterizer non-browser bisa kehilangan teks (fontsource name-table tidak selalu sama nama @font-face).
