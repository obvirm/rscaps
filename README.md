# rscaps

> **The faster alternative to tscaps — headless, browser-free caption rendering.**
> Animated, burned-in subtitles rendered by a Rust engine that emits SVG. No DOM per word, no CSS animation contract, no browser in the render path — measured 1.5x faster than tscaps on the web export - the headless CLI burns a 4-minute 1080p30 video in ~29 seconds.

rscaps is a clean-room alternative to the caption pipeline of [tscaps](https://github.com/francozanardi/tscaps). tscaps styles captions as HTML + CSS and lets the **browser** rasterize the DOM at every frame. rscaps takes the same document/template idea and re-solves it as a **pure rendering engine**: one Rust string of SVG per frame, produced by `caption-core`, identical for preview and export.

The architecture was resolved from tscaps and re-realized without the browser: instead of "captions = live CSS in a DOM the browser paints", captions are computed geometry + paint in Rust. The result renders anywhere a rasterizer runs — a CLI, a server, a native shell — not only where a browser exists. And because the export path never round-trips pixels through the DOM, it wins on speed too (see [Benchmarks](#benchmarks)).

## What it is

- A **caption render engine** (`caption-core`, Rust) that turns an SRT/VTT + a template + params into a caption frame as SVG.
- A **headless CLI** (`rscaps`) that renders frames and burns captions into video with no browser at all.
- A **thin editor** (`studio`, Svelte 5 + TypeScript) for tuning templates and previewing — optional, not the render path.
- A **native desktop shell** (Tauri v2) sharing the same WASM engine.

41 animated caption templates ship in the repo, ported clean-room from the tscaps gallery (structure and visuals mirrored, code not copied).

## Philosophy: no browser in the render path

| | tscaps | rscaps |
|---|---|---|
| Caption representation | HTML elements + CSS | one SVG string per frame |
| Rasterizer (web) | the browser DOM | `svgr-wasm` (usvgr + tiny-skia, WASM) |
| Rasterizer (native) | — | `svgr` / `fframes` (tiny-skia, Skia) |
| Preview vs export | browser both times | same engine; preview paints the same SVG |
| Headless | no (needs a browser) | yes (CLI, server, native) |
| Per-word edit surface | CSS classes / custom properties | document model + per-word overrides |

One engine, one output format (SVG), one place where caption logic lives. Preview and export cannot drift because they are the same call.

## Benchmarks

Same machine, same clip, measured back-to-back (4-minute 1080p30 burn-in, 7200 frames, 120 cues; 6-core/12-thread desktop + NVIDIA GPU). Reproduce with `scripts/bench_web_4min.mjs`, `scripts/tmp_tscaps_bench.mjs`, and `scripts/bench_cli_matrix.mjs`.

### Web export vs tscaps

Export click → completed export, measured back-to-back on the same day and load:

| Pipeline | Wall | Speedup |
|---|---|---|
| **rscaps web export** | **33.8s** | **1.5×** |
| tscaps (web) | 51.0s | - |

### CLI engine matrix

Every engine runs through the same slice architecture (band crop + per-SVG raster cache + double-buffer + K parallel ffmpeg processes). First column = before the optimization pass.

| Engine + encode | Before | Now | Total speedup |
|---|---|---|---|
| `skia` + NVENC | 65.3s | **29.4s** | 2.2× |
| `cpu` + NVENC | 328.8s | **28.8s** | **11.4×** |
| `cpu` + NVENC + CUDA decode | 321.9s | **30.6s** | 10.5× |
| `fframes` + NVENC + CUDA | 168.5s | **82.2s** | 2.1× |
| `cpu` + x264 | 546.3s | **155.2s** | 3.5× |
| `cpu` + x264 + CUDA decode | 519.2s | **148.9s** | 3.5× |
| `skia` + x264 | 193.4s | **151.4s** | 1.3× |
| `fframes` + x264 | 301.6s | **216.5s** | 1.4× |

Software x264 sits on encoder physics (`medium` on noisy 1080p ≈ 46fps aggregate across 12 threads). `--preset fast` trades ~10% larger files for 1.2× (130s); `veryfast` reaches 69.5s at ~30% smaller files with a visible quality drop. The benchmark clip is noise-stressed - real-world content encodes 3-5× faster.

Why it wins:

- **Zero pixel transport on web**: the video frame stays on the GPU, captions composite in workers, and the encoder consumes GPU frames directly. tscaps snapshots a canvas per frame through a serial main-thread loop.
- **The encoder is the only wall**: decode, caption raster, and muxing overlap fully behind the hardware encoder's pace (`encOut` diagnostics in the export profiler). The NVENC CLI paths land on the same ~29-31s floor.
- **CLI band crop**: captions occupy a fraction of the frame, so only that vertical band is rasterized and piped (~2.7× less blur work and pipe bytes), with a sampled-bbox fallback to full-frame when captions spread.
- **Per-SVG raster cache**: static frames inside one cue produce identical SVGs — a ~75% hit rate skips raster and GPU readback entirely.
- **Tight filter regions + deduplicated shadow layers**: caption SVGs carry only the blur bleed they need (3σ + offset) and identical stacked shadow layers collapse into one blur behind an exact alpha-transfer curve — the same pixels for a fraction of the filter work (worth ~8× on CPU raster, 2× on the fframes pipeline).

Export features on the web path: codec fallback chain (AVC → HEVC → AV1, hardware-accelerated), an explicit bitrate table (7/14/21/46 Mbps baselines at 720p/1080p/1440p/4K), `contentHint: 'text'`, and audio interleaved with the video drain (no separate audio phase).

## Architecture

| Path | What it is |
|---|---|
| `crates/caption-core` | **The engine** — the single source of caption logic: render, templates, wrapping, text direction, timing/animation, SRT/VTT, template registry, default params, real video size. |
| `crates/caption-bridge` | wasm-bindgen bridge exposing `caption-core` to JS (`pkg/`, generated). |
| `crates/svgr-wasm` | WASM rasterizer (usvgr + tiny-skia) — web export rasterizes/composites captions here, not in the browser canvas. |
| `crates/rscaps-cli` | Headless CLI — `templates`, `convert`, `frames`, `render`, `serve`. No browser. |
| `crates/whisper-spike` | On-device Whisper transcription spike (ONNX runtime, WASM). |
| `studio/` | Editor UI (Svelte 5 + TypeScript + Vite). Consumes the bridge only. |
| `src-tauri/` | Tauri v2 desktop shell wrapping `studio/`. |
| `scripts/` | WASM build + self-verification harnesses (screenshots, crosschecks, chunk verify). |

The template list is auto-generated by `caption-core`'s `build.rs` from `src/templates/<id>/`. Adding a template = add a folder + one `pub mod` line; the registry, catalog, and every consumer update automatically.

## Run

Prerequisites: Rust (edition 2024), Node + pnpm, and `wasm-pack` for the engine build.

```bash
# 1. Build the WASM modules (caption-bridge + svgr-wasm)
node scripts/build-wasm.mjs

# 2. Start the editor at http://localhost:5177
pnpm dev            # or: npx vite --port 5177
```

Build the editor bundle (output `studio/dist/`):

```bash
pnpm build:studio   # or: npx vite build
```

Desktop shell (Tauri v2, dev server on port 5177):

```bash
pnpm exec tauri dev
```

## CLI (headless)

The CLI renders captions without any browser. All caption logic still goes through `caption-core`; the CLI only handles I/O.

```bash
# list templates
cargo run -p rscaps-cli -- templates

# convert between subtitle formats
cargo run -p rscaps-cli -- convert subs.srt --to vtt

# render caption frames (transparent SVG/PNG, no video) — headless
cargo run -p rscaps-cli -- frames --srt subs.srt --template milo \
  --size 1080x1920 --fps 30 --format png --out out/

# burn captions into a video -> mp4
cargo run -p rscaps-cli -- render --video in.mp4 --srt subs.srt \
  --template milo --out out.mp4
```

Engines and encode options:

- `--engine skia` — **fastest headless path**: 12 GPU raster lanes, per-SVG raster cache, double-buffered pipe, and 3 parallel ffmpeg processes over a band-cropped caption transport, joined with `concat -c copy`. Pair with `--hw-encode` for hardware encode.
- `--engine fframes` — full fframes pipeline (raster + encode + audio mux in Rust). Long videos are rendered in pipelined chunks - chunk N+1 decodes on another thread while chunk N renders - then joined with `ffmpeg concat`, so there is no length cap. Canvas follows the real video size (portrait works).
- `--engine cpu` - same slice architecture as `skia` with CPU (usvgr) raster lanes - the no-GPU fallback, ~29s for a 4-minute 1080p burn.
- `--hw-encode` + `--hw-encoder h264_nvenc|h264_qsv|h264_vaapi|h264_videotoolbox|h264_amf` — GPU encode (falls back to `libx264` when the encoder is unavailable).
- `--hwaccel <auto|cuda|d3d11va|vaapi|qsv>` — decode with an FFmpeg hardware accel.
- `--crf`, `--preset`, `--ffmpeg-dir` — encoding controls (FFmpeg/ffprobe located via flag, `RSCAPS_FFMPEG_DIR`, or `PATH`).

Fonts follow the studio contract by default: only the template fonts (`tplfonts`) plus Arial/Arial Black are loaded — a full system fontdb slows text layout. Set `RSCAPS_LOAD_SYSTEM_FONTS=1` to restore full system font coverage for exotic scripts.

`serve` runs a localhost API (`--bind`/`--port`, default `127.0.0.1:8787`) for render jobs with progress.

GPU raster build:

```bash
cargo build --release -p rscaps-cli --features gpu-skia
```

## Templates

41 templates across the tscaps families (Modern, Key moments, Viral, Classic, Lab), ported clean-room. Each is a folder under `crates/caption-core/src/templates/<id>/` with a `template.json` (label, category, gallery order, style controls, variants, defaults) consumed by the auto-generated registry.

Render is the single path: `render_frame()` → one SVG string → one `<svg>` element (`preserveAspectRatio="none"`). Templates that emit `font-family` go through `stack::svg_font_family()` so the font resolves in any rasterizer, not just a browser.

## Self-verification

Rendering is verified end-to-end with scripts, not by eye:

- `node scripts/shot_editor.mjs out.png "--tabs=style,layout"` — screenshot the editor over a test project.
- `node scripts/verify_params.mjs`, `verify_portrait.mjs`, `verify_position.mjs` — control wiring.
- `node scripts/crosscheck.mjs` — multi-language RTL/bidi battery against tscaps (`scripts/battery.srt`).
- `node scripts/verify_chunk.mjs` — chunked vs single-pass `--engine fframes` parity (per-frame `framemd5`).

## Status

Pre-1.0, moving fast. The engine and CLI work end-to-end; the editor is a work in progress. Known gaps and their current state are tracked in [`AGENTS.md`](AGENTS.md).

Dependency note: the engine currently tracks `fframes` `1.2.1-rc.38`.

## License

- `crates/` (engine, bridge, CLI, transcription): [MIT](crates/LICENSE)
- `studio/` (editor app): [AGPL-3.0](studio/LICENSE)

rscaps mirrors the structure and visual design of tscaps but contains no copied tscaps runtime code. See each LICENSE file for full terms.
