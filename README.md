# rscaps

Browser caption editor built with the [fframes](https://crates.io/crates/fframes) engine (Rust/wasm). Renders captions natively via SVG for both preview and export — no CSS-only browser path.

Inspired by the caption templates and document format of [tscaps](https://github.com/francozanardi/tscaps). This is an independent rewrite in Rust; no runtime code was copied.

## Run

```bash
# build wasm
node scripts/build-wasm.mjs

# start studio (port 5177)
node studio/server.mjs
```

Open http://localhost:5177.

## Architecture

- `crates/caption-core` — template rendering pipeline, 41 templates, non-Latin stack
- `crates/caption-bridge` — wasm-bindgen bridge to the browser
- `crates/whisper-spike` — on-device Whisper transcription spike
- `studio/` — lightweight web editor UI (HTML/JS, no framework build step)

## License

- `crates/`: [MIT](crates/LICENSE) — engine, templates, rendering pipeline
- `studio/`: [AGPL-3.0](studio/LICENSE) — editor app

See each LICENSE file for full terms.
