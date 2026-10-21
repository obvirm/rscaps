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

**GOTCHA WAJIB (PROFILE):** `[profile.release]` workspace WAJIB `opt-level = 3`
(JANGAN `"s"`) + `lto = "thin"` (lto penuh = rustc ICE `STATUS_STACK_BUFFER_OVERRUN`
di crate `exr`). opt-level `"s"` memperlambat loop raster per-piksel svgr/tiny-skia
~3-4× (cold render cleo 124ms → 150-450ms) dan dulu membuat SEMUA varian CLI + WASM
lambat. `svgr-wasm` adalah workspace member — `[profile]` di Cargo.toml-nya DIABAIKAN
cargo; profil root yang berlaku.

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
- **Style controls**: semua kontrol non-tag/non-motion sudah di-wire ke render via `templates/style.rs` (`Style::of`), default diambil dari template.json (tanpa duplikasi). **Tag Tier-1 SUDAH DI-PORT** (commit `1a39997`): subsistem tag semantik (`TimedWord.tags` — quote span/number auto, `doc.word_tags` manual via popover klik-kanan, `Sheet.role` peak/hook) + styling warna/shadow/bg: cleo/mira/noor quote, kel peak-bg, tala tape, sara/enzo/hugo/lewis warna+glow tag. **Tag Tier-2 SUDAH DI-PORT** (`templates/faces.rs` + `tagstyle.rs` + `stack::measure_words_typed`): `tag-weight/italic/font-family/font-scale/tag-bold` — face statis per weight (fontsource, `scripts/woff2-to-ttf.mjs`), snap ke face yang ada (Lora berhenti di 700, JBM di 800), italic asli (lora/montserrat/eb-garamond), ukur per-kata dari bytes faces.rs, line box membesar mengikuti scale maksimum line (algoritma line-box CSS). Template: cleo, mira (+quote-italic face asli), enzo (face switch + scale parsial 0.85×), lena, selene, pico, sara, sol (line aksen Kalam — ganti substitusi Caveat lama). Kata script (arab/ibrani) mengabaikan tipografi tag. **Sisa DEFER**: `pastor/behind-*` (segmentasi orang MediaPipe ~60 file di tscaps — disproportionate; di tscaps pun gated), `iris/mask` custom upload (kontrol `type: "image"` tak dirender StyleControlRow; stroke bawaan `marker-stroke.b64` bekerja).
- **fisika kontrol (bukan bug)**: `hugo/outline-color` butuh `outline-thickness>0` (default0); `otto/pill-mode` identik pada cue baris-tunggal; `pico/caret` ships-off; `iris/highlight-color-2..5` butuh resep non-single; `selene/corner-radius` di-cap kapsul (hanya <h/2 terlihat); `rhea/resting-weight` hanya terlihat di luar state Being.
- Selene template: pernah text hilang — akar utama (FIXED): family "Montserrat" tak resolve di fontdb karena fontsource membawa nama internal "Montserrat Thin" — kini semua emit `font-family` lewat `stack::svg_font_family()` (font-stack: nama browser + alternat name-table), menyelesaikan juga mira/rhea/kai/lyra/pastor/luna. Sisa risiko: `filter="url(#selene-word)"` per-`<text>` — kalau selene kosong, investigasi terpisah.
- Template yang emit `font-family` ke SVG WAJIB lewat `stack::svg_font_family()` (lihat stack.rs) — jangan tempel nama keluarga mentah, atau rasterizer non-browser bisa kehilangan teks.
- Glif non-latin TIDAK dimiliki font template: ukur via `stack::fallback_char_advance` (CJK/kana/hangul 1em, Thai/Lao dasar 0.62em + gabung 0, lain 0.5em — perkiraan font sistem; tscaps mengukur via DOM). Jangan kembali ke 0 (tumpang tindih) atau `utf16*0.5` blanket (CJK melebar setengah).
- **usvgr TIDAK men-decode `data:` URL** — `<image href>` (kaca selene, stroke iris) butuh `Options.image_data` berisi `PreloadedImageData` berkunci PERSIS string href. svgr-wasm menyediakan `register_image/unregister_image`; host self-serve: worker lewat `ensureSvgImages` (burnWorker — kaca = piksel frame dari kanvas komposit, statis = decode-once), main-thread cermin `ensureInlineImages` (svgrwasm.ts). Kunci kaca berganti tiap frame → unregister kunci lama.
- **svgr 0.50.1 `SvgrCache`/`PixmapPool` TIDAK sound — JANGAN pakai LRU-nya**: pool menyimpan pixmap virtual dengan blok pow2²; LRU/layer-cache bisa menyeret pixmap kapasitas-exact (`Pixmap::new`) ke `release` → class teracuni → `take_or_allocate` berikutnya `set_len` di atas kapasitas → korupsi heap dlmalloc (assertion `psize >= size + min_overhead`, trap "memory access out of bounds" flaky ~10-20% load studio). FIX 2026-10: pakai `SvgrCache::none()` di svgr-wasm + rscaps-cli (semua cache mati → satu-satunya sumber entry pool = `take_or_allocate` → invarian terjaga); POOL thread-local tetap boleh. Konten caption berubah tiap frame sehingga cache lintas frame nyaris tak pernah hit (biaya ≈ 0). **Sisa: signature dlmalloc MASIH muncul jarang (~1/20 load) pasca-fix — vektor lain belum teridentifikasi**; plus race boot studio terpisah: `fframes-editor-controller wasm_editor.rs "TimeBase must be set"` (~5-10% load, retry berhasil — panic ini menyusul kaskade mutex no_threads). Keduanya pre-Tier-2; reimvisi bila makin sering. Revisit cache bila upgrade svgr.
- **Raster CPU per-frame (usvgr)**: SVG cold (konten beda tiap frame) dengan filter blur = jalur mahal — box blur 5-pass + salinan backbuf per layer; region filter % besar (400%/600%) melipatgandakan areanya (dulu ~450-636ms/frame @1080p wasm, 124-150ms native opt-3). FIX di emit cleo: region selimut bleed saja (3σ+dy, % adaptif dari ink-bbox) + 3 layer soft identik didedupe via `feFuncA type="table"` 17 titik (eksak 1-(1-A)³, Δα≤2/255, nonzero piksel identik) → ~16ms native. **JANGAN kembalikan region % besar atau layer blur duplikat** — sama sekali tak terlihat matanya, tetapi memakan waktu raster 8×. Band crop `compute_caption_band` margin 128px ≥ bleed 105px ✓.
- **Diagnostik raster** (env-gated): `RSCAPS_DEBUG_TIMING` (frames cmd: render vs encode PNG), `RSCAPS_DEBUG_SLOW=<us>` (log render > ambang + fase parse/alloc/render/copy di profil slice), `RSCAPS_NO_CAPTION=1` (fframes: lewati caption — isolasi biaya caption vs video di profil potongan), counter HITS/MISSES/PHASE_* di `render.rs`. Env opsional: `RSCAPS_SLICES` (default 3), `RSCAPS_FFMPEG_THREADS` (default apa adanya ffmpeg). Allocator global CLI = **mimalloc** (heap lock Windows menghambat lane paralel).
- **Audio chunked = video-only + mux sekali**: potongan fframes dirender TANPA audio (`audio: HashMap::new()` + nama kosong → `AudioMap::none()`; nama tidak kosong membuat fframes resolve durasi audio di tiap potongan → `CanNotProcessAudioDuration` saat map kosong); setelah concat, wav ekstraksi di-mux SEKALI (`mux_audio_once`, AAC 192k, `-shortest`) — satu priming di awal file, tanpa drift akumulatif ~23ms/batas segmen. Single-chunk tetap embed audio (satu encode, setara). **Reparse SVG per-frame di fframes**: inheren (dok fframes: tanpa `compile-time-svgtree`, markup string di-parse tiap frame; macro tak bisa menelan subtree string dinamis); diukur: caption cleo ≈ 2.9ms/frame @320×240 dev (~34% render) — termasuk raster filter, parse saja lebih kecil; tuas nyata = upgrade fframes (subtree hash) atau jalur `--engine cpu`.
- **Engine fframes — decode dulunya 62% dinding (plot twist)**: `PreloadedImageData::new` (usvgr) mem-blend RGBA per-piksel via f32 skalar (~11ms/frame @1080p) — no-op untuk frame video h264 yang selalu opaque (α=255). FIX: baca langsung ke Vec milik frame + konstruksi `PreloadedImageData` langsung (field pub) tanpa blend (debug_assert opaque). Chunk kini **pipelined**: decode chunk N+1 di thread lain selagi chunk N dirender (RAM: 2 × chunk; `CHUNK_FRAMES` default 300). Hasil: fframes+nvenc_cuda 165,3s → 80,0s; fframes+x264 288,7s → 219,3s (medium). Sisa dinding fframes = raster komposit serial ~8-11ms/frame (image blit 2Mpx + caption + filter) + encoder internal.
- Masih gap vs tscaps: element inspector; `pastor/behind-*`; custom upload iris. (Sudah beres: gallery template live-preview, undo/redo + restore template defaults, timeline C6, transcript word-level C8, sheets multi-sheet C7, whisper multi-model C5 — tiny/base/small; preview=export C9; selene kaca D4; iris mask D5; tag Tier-2 D6.)
- **Web export (2026-10, commit `5d6f566`)**: pipeline = decode → `createImageBitmap` → transfer ke 6 worker (`burnWorker.ts`: readback pixel + burn WASM + bungkus `VideoFrame` transferable) → `VideoSampleSource` (tanpa canvas encode; mp4/avc pakai `prefer-hardware`). 4-menit 1080p30 ≈ 61-68s (baseline 140.6s), CPU avg ~89% peak 100%. **Streaming ke disk (B4)**: `showSaveFilePicker` + `StreamTarget({chunked})` (`wrapFileSystemWritable` di export.ts membungkus FSA writable — mediabunny mengunci wrapper-nya dan MENUTUP file saat `output.finalize()`, jadi JANGAN close manual = "Cannot close a locked stream"; FSA asli bebas → gagal bisa `abort()`); mp4 streaming pakai `fastStart: false` (moov di ekor). Browser tanpa FSA → fallback BufferTarget + download (RAM ≈ 2× file). **Dua jebakan regresi caption — JANGAN DIPERBAIKI ULANG:** (1) `prepare(fps)` **mereset dokumen bridge** — `setDocumentJson` wajib dijalankan SETELAH `add_video_source_placeholder` + `prepare`, kalau sebelumnya `render_frame` mengembalikan grup `<g></g>` kosong (tak ada teks); (2) split base/blend (`renderFrameSvgLayers`) harus **mempertahankan struktur grup** (font attrs/rise/clip defs) di kedua layer — menyalin node `<text>` saja ke svg baru membuat usvgr kehilangan `font-family`/`font-size` → blend raster 0 glyph (luna: semua kata difference, base kosong = normal, blend yang membawa teks). Download Chrome headless tak reliable untuk blob 288MB — harness menyimpan blob via stream chunk (`__saveChunk` di `bench_web_4min.mjs`).
