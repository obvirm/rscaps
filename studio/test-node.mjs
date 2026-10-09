import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

// The generated glue imports @fframes/editor (a React bundle that probes
// AudioContext at import time). For the headless Node smoke test we swap it
// for a stub; the browser keeps using the real module.
const pkgDir = fileURLToPath(new URL("../crates/caption-bridge/pkg/", import.meta.url));
const glue = await readFile(pkgDir + "caption_bridge.js", "utf8");
const patched = glue.replaceAll(
  "from '@fframes/editor'",
  "from './fframes-editor-shim.mjs'"
);
if (patched === glue) throw new Error("expected @fframes/editor import not found");
await writeFile(pkgDir + "fframes-editor-shim.mjs", "export function load_audio_wasm_callback() {}\n");
await writeFile(pkgDir + "caption_bridge.node.js", patched);

const { default: init, create_wasm_bridge, set_document_json } = await import(
  pathToFileURL(pkgDir + "caption_bridge.node.js").href
);

const wasmBytes = await readFile(pkgDir + "caption_bridge_bg.wasm");
await init({ module_or_path: wasmBytes });

const bridge = create_wasm_bridge();

const fontBytes = await readFile("C:/Windows/Fonts/ariblk.ttf");
const face = bridge.ingest_font(fontBytes);
console.log("font face:", JSON.stringify(face));

const doc = {
  duration_frames: 300,
  cues: [
    { start_ms: 0, end_ms: 2000, text: "Welcome to rscaps" },
    { start_ms: 2200, end_ms: 5000, text: "Captions rendered by fframes in Rust" },
  ],
  params: {
    font_family: "Arial",
    font_size: 110,
    font_weight: 900,
    color: "#ffdd57",
    y_frac: 0.78,
    width_frac: 0.78,
    pop_in_ms: 250,
    travel_px: 48,
  },
};
set_document_json(JSON.stringify(doc));

const meta = await bridge.prepare(30);
console.log("duration:", meta.durationInFrames, "fps:", meta.fps, "size:", meta.width, meta.height);

for (const frame of [0n, 10n, 65n, 150n, 200n]) {
  const svg = bridge.render_frame(frame);
  console.log(`frame ${frame}: ${svg.length} chars, texts=${(svg.match(/<text/g) ?? []).length}`);
}

const svg = bridge.render_frame(10n);
console.log("--- sample (frame 10, first 700 chars) ---");
console.log(svg.slice(0, 700));
