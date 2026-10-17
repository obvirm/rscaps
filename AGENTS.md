# AGENTS.md — Aturan Kerja rscaps

## ATURAN KERAS

1. **caption-core = SATU-SATUNYA sumber logika & API caption (satu titik)**
   - Semua logika caption HIDUP di `crates/caption-core`: render, template, wrap, arah teks, timing/animasi, SRT/VTT, **daftar template** (`templates::REGISTRY`), **default params** (`CaptionParams::default`), dan **ukuran video nyata** (`ctx_video_size(ctx)` — baca `media_source`, fallback kanvas 1920×1080).
   - API keluar untuk JS/app lain HANYA lewat `crates/caption-bridge` (wasm-bindgen → JS generated di `pkg/`). Satu set API, jangan ada cabang.
   - Konsumen (`studio/`, app lain) DILARANG meng-hardcode ulang daftar template, default params, tipe dokumen, style controls, atau logika apa pun. Semua diambil dari bridge: `template_catalog_json()` (termasuk `styleControls`/`variants`/`defaults`), `default_document_json()`, `set_document_json()`, `render_frame()`, `add_video_source_placeholder()`, dll.
   - Daftar template di-caption-core AUTO-GENERATE oleh `build.rs` dari folder `src/templates/<id>/` (`template.json` → label, category, galleryOrder, styleControls, variants, defaults). Menambah template = bikin folder + tambah satu baris `pub mod <id>;` di `templates/mod.rs` — REGISTRY, catalog, dan konsumen lain ikut otomatis. Dilarang menempel daftar manual di mana pun.
   - Menambah API di caption-core/bridge: boleh — itu memperkaya satu titik. Menggandakan logika/metadata di konsumen: dilarang.
   - Fix/penyesuaian visual overlay/preview: di `studio/` (bukan di caption-core), KECuali akar masalahnya layout/wrap/anchor engine.

2. **`studio/` = SATU-SATUNYA UI (Svelte 5 + TypeScript + Vite), clean-room tscaps**
   - Dev: `npx vite --port 5177` di root repo (atau `pnpm dev`); asset luar root (WASM pkg, fonts) diserve oleh plugin di `vite.config.ts`.
   - UI di-clean-room dari `E:\project\tscaps\apps\studio` (React, **AGPL**): boleh MENIRU struktur/visual/ikon (lucide) tapi JANGAN copy kode. `packages/engine` & `templates/` tscaps = MIT (rujukan perilaku saja — kita pakai caption-core).
   - Render caption di preview = **satu string SVG** dari `render_frame()` → satu elemen `<svg>` (`preserveAspectRatio="none"`). TANPA DOM per kata, tanpa CSS animation contract.
   - Panel & kontrol: `studio/src/components/controls/` (atoms) + `panels/` (9 tab). Semua nilai efektif dari `lib/params.ts` (params vs `defaults` template).
   - Verifikasi mandiri: `node scripts/shot_editor.mjs out.png "--tabs=tab1,tab2"` (puppeteer-core, buat project dari video test + SRT), `scripts/verify_params.mjs`, `scripts/verify_portrait.mjs`.
   - **studio-slint = DIHAPUS** (riwayat di git, commit terakhir `f905929`). Jangan ditulis ulang.

3. **Jangan asal patch**
   - Pahami root cause dulu sebelum fix.
   - Jangan tambal-tambal (strip filter kasar, hack font, dll) tanpa paham kenapa.
   - Bandingkan dengan tscaps (React) sebagai reference visual; verifikasi screenshot sendiri sebelum lapor.

4. **DILARANG distorsi percakapan**
   - Jawab maksud user APA ADANYA. Jangan ganti topik, jangan menambah asumsi, jangan menjawab pertanyaan yang tidak ditanya.
   - Kalau user sudah menyebut hal spesifik, JANGAN bergeser ke hal lain.
   - Kalau maksud user tidak 100% jelas: **TANYA dulu** (ya/tidak atau pilihan konkret), lalu tunggu. Jangan menebak lalu mengerjakan hal lain.
   - Jangan mengarang jawaban/analisis. Kalau tidak tahu: bilang **"saya tidak tahu"**.
   - Sebelum bertindak, ulangi 1 kalimat maksud user; kalau salah, user bisa koreksi lebih awal.

## Port & Server

- Port 5173: tscaps main (React — referensi)
- Port 5177: **rscaps studio (Svelte)** — `npx vite --port 5177` dari root
- Build web: `npx vite build` (output `studio/dist/`)
- Build WASM engine: `node scripts/build-wasm.mjs` (wasm-pack, PATH ffmpeg otomatis)

## Arsitektur Cross-Platform (Web + Tauri desktop + Android)

Satu codebase UI (`studio/`, Svelte+TS), satu engine (`caption-core` → WASM).

| | Web (browser) | Native desktop | Android (target) |
|---|---|---|---|
| Shell | Vite build biasa | **Tauri v2** (webview) | **Tauri v2 mobile** / Capacitor |
| Engine caption | `caption-bridge` WASM | WASM di webview (sama) | WASM (sama) |
| Decode video | `<video>` DOM / mediabunny WebCodecs | webview (WebView2/WKWebView) | Android WebView (WebCodecs terbatas → fallback `<video>`) |
| File picker | `<input type=file>` | Tauri dialog plugin | Storage Access Framework |
| Transcribe | `whisper-spike` (WASM ONNX) | WASM (jalan, CPU) | WASM (CPU, lambat) |
| Export | mediabunny (WebCodecs); raster caption di `svgr-wasm` (WASM, bukan canvas browser) | sama via webview | sama (cek WebCodecs WebView) |

**Prinsip:**
- **Render caption = SATU jalur**: `render_frame()` → string SVG. Web/export me-raster string itu di **`crates/svgr-wasm`** (usvgr+tiny-skia, WASM); CLI native me-raster dengan `svgr`/fframes. Preview studio memakai satu `<svg>` yang sama (browser). Logika caption tak bercabang.
- **Dimensi video nyata WAJIB** didaftarkan ke bridge (`add_video_source_placeholder(file, url, w, h, dur)`) sebelum `prepare()` — kalau tidak, engine fallback 1920×1080 dan non-16:9 salah layout.

**GOTCHA WAJIB (PATH):** proc-macro `fframes-media-dir-macro` (milik fframes) mengimpor
DLL FFmpeg 9.0.2 (`avcodec-63.dll` dsb) yang hanya ada di
`E:\temp\opencode\ffmpeg-9.0.2-full_build-shared\bin`. Folder itu **bukan bagian dari
PATH default** — semua perintah cargo yang menyentuh fframes (dev/release, check/build,
test) HARUS prepend PATH itu dulu. Kalau lihat `E0463` untuk `fframes_media_dir_macro`:
bukan bug kode, PATH ffmpeg belum masuk.

## CLI native (`rscaps-cli`)

- `render --engine fframes` = **pipeline fframes penuh** (raster + encode + audio mux); `--engine cpu|skia` = jalur overlay lama (svgr raster → PNG pipe → ffmpeg overlay). `frames --engine fframes` tak didukung (error ramah).
- **Chunked**: video > `CHUNK_FRAMES` (600) dirender per potongan via `RenderOptions::frame_range`, digabung `ffmpeg concat -c copy`. Hanya potongan aktif di RAM → **tak ada cap total**. Override uji: env `RSCAPS_FFRAMES_CHUNK`.
- Kanvas **ikut ukuran video nyata** (`CompositeVideo<W,H>` const generics + dispatch; fframes menyetel `ctx.current_video_size`) → portrait 1080×1920 benar.
- Encoder: `--hw-encode` + `--hw-encoder h264_nvenc|h264_qsv|h264_vaapi|h264_videotoolbox|h264_amf` (fallback `libx264` bila tak tersedia); `--hwaccel` diteruskan ke decode; `--format` divalidasi (svg|png).
- Verifikasi chunk: `node scripts/verify_chunk.mjs` — bandingkan hash per-frame (framemd5) chunked vs single. **JANGAN pakai filter `psnr`**: concat menggeser PTS → skor palsu.
- Web **tidak** memakai fframes (renderer fframes di-gate `not(wasm32)`); raster web = `crates/svgr-wasm` (usvgr+tiny-skia, versi sama dengan CLI `--engine cpu` → paritas).

## Known Issues

- **RTL/bidi**: DIVERGENSI BESAR SUDAH SELESAI (battery multi-bahasa): urutan kata RTL, konten lintas-kue, dan wrap kini identik dengan tscaps (verifikasi `scripts/crosscheck.mjs` + `scripts/battery.srt`, semua `TIMES`). Sisa micro-diff: garis *leak-heavy* campur-skrip (mis. luna@8.2) bisa patah baris beda 1 kata — balanced-splitter borderline, bukan bug arah.
- **`mix-blend-mode: difference`** (milo/luca anchor, luna SEMUA kata): preview = attr inline di `<text>`; **SVG `clip-path` — meski di elemen blend sendiri — MENGISOLASI di Chrome** (kata jadi putih datar; beda dengan CSS HTML tscaps yang `overflow:hidden` non-isolating). Karena itu clip segment luna DIHAPUS (hanya semasa rise ~0.18s kata boleh menyembul). Export: `renderFrameLayers` memisah layer blend → `burnInExport` komposit dengan `globalCompositeOperation='difference'` terhadap PIKSEL VIDEO (jangan digambar ke kanvas caption transparan — di sana blend = no-op).
- **Style controls**: semua kontrol non-tag/non-motion sudah di-wire ke render via `templates/style.rs` (`Style::of`), default diambil dari template.json (tanpa duplikasi). **`tag-*` (tag-color/weight/italic/font-*) = DEFER** — butuh subsystem tagging tscaps (regex/wordlist/NER) yang Cue SRT tak bawa; proyek SRT polos = tak ada kata bertag di kedua sisi (inert dua-dua = paritas). Juga defer: `kel/peak-bg` + `tala/highlight-*` (aktif-kata bertag), `selene/blur|saturation` (video-frame layer tak di-port), `pastor/behind-*` (gated `BEHIND_ACTOR` const, switch per-segment tak di-port), `iris/mask` (type image).
- **fisika kontrol (bukan bug)**: `hugo/outline-color` butuh `outline-thickness>0` (default0); `otto/pill-mode` identik pada cue baris-tunggal; `pico/caret` ships-off; `iris/highlight-color-2..5` butuh resep non-single; `selene/corner-radius` di-cap kapsul (hanya <h/2 terlihat); `rhea/resting-weight` hanya terlihat di luar state Being.
- Selene template: pernah text hilang — akar utama (FIXED): family "Montserrat" tak resolve di fontdb karena fontsource membawa nama internal "Montserrat Thin" — kini semua emit `font-family` lewat `stack::svg_font_family()` (font-stack: nama browser + alternat name-table), menyelesaikan juga mira/rhea/kai/lyra/pastor/luna. Sisa risiko: `filter="url(#selene-word)"` per-`<text>` — kalau selene kosong, investigasi terpisah.
- Template yang emit `font-family` ke SVG WAJIB lewat `stack::svg_font_family()` (lihat stack.rs) — jangan tempel nama keluarga mentah, atau rasterizer non-browser bisa kehilangan teks.
- Glif non-latin TIDAK dimiliki font template: ukur via `stack::fallback_char_advance` (CJK/kana/hangul 1em, Thai/Lao dasar 0.62em + gabung 0, lain 0.5em — perkiraan font sistem; tscaps mengukur via DOM). Jangan kembali ke 0 (tumpang tindih) atau `utf16*0.5` blanket (CJK melebar setengah).
- Masih gap vs tscaps: timeline, transcript tingkat kata/scene, sheets multi-track, element inspector, export WebCodecs belum diuji end-to-end setelah fix dimensi. (Sudah beres: gallery template live-preview, undo/redo + restore template defaults.)
