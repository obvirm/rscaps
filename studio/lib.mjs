import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

export const pkgDir = fileURLToPath(new URL("../crates/caption-bridge/pkg/", import.meta.url));

// The generated glue imports @fframes/editor (a React bundle that probes
// AudioContext at import time). For headless Node we swap it for a stub; the
// browser keeps using the real module.
export async function loadBridge() {
  const glue = await readFile(pkgDir + "caption_bridge.js", "utf8");
  const patched = glue.replaceAll(
    "from '@fframes/editor'",
    "from './fframes-editor-shim.mjs'"
  );
  if (patched === glue) throw new Error("expected @fframes/editor import not found");
  await writeFile(pkgDir + "fframes-editor-shim.mjs", "export function load_audio_wasm_callback() {}\n");
  await writeFile(pkgDir + "caption_bridge.node.js", patched);

  const mod = await import(pathToFileURL(pkgDir + "caption_bridge.node.js").href);
  await mod.default({ module_or_path: await readFile(pkgDir + "caption_bridge_bg.wasm") });

  const bridge = mod.create_wasm_bridge();
  const fontBytes = await readFile("C:/Windows/Fonts/ariblk.ttf");
  bridge.ingest_font(fontBytes);
  return { ...mod, bridge };
}

export function baseDoc(overrides = {}) {
  return {
    duration_frames: 300,
    cues: [
      { start_ms: 0, end_ms: 3400, text: "Welcome to rscaps" },
      { start_ms: 3400, end_ms: 6700, text: "Captions rendered by fframes in Rust" },
      { start_ms: 6700, end_ms: 10000, text: "One frame at a time, synchronously" },
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
      direction: "auto",
    },
    ...overrides,
  };
}
