import { loadBridge, baseDoc } from "./lib.mjs";

const { bridge, set_document_json } = await loadBridge();
set_document_json(JSON.stringify(baseDoc()));
await bridge.prepare(30);

const FPS = 30;
const FRAMES = 300;

// warmup (font resolution, caches, lazy inits)
for (let f = 0n; f < 30n; f++) bridge.render_frame(f);

const times = [];
for (let f = 0; f < FRAMES; f++) {
  const t0 = performance.now();
  bridge.render_frame(BigInt(f));
  times.push(performance.now() - t0);
}

const sorted = [...times].sort((a, b) => a - b);
const sum = sorted.reduce((a, b) => a + b, 0);
const pct = (p) => sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))];

console.log(`render_frame: n=${FRAMES} frames @${FPS}fps`);
console.log(`  avg  ${(sum / sorted.length).toFixed(2)} ms/frame`);
console.log(`  p50  ${pct(50).toFixed(2)} ms`);
console.log(`  p95  ${pct(95).toFixed(2)} ms`);
console.log(`  max  ${sorted[sorted.length - 1].toFixed(2)} ms`);
console.log(`  total ${sum.toFixed(0)} ms for ${FRAMES} frames (${(FRAMES / FPS).toFixed(1)}s video)`);
console.log(`  realtime budget @${FPS}fps = ${(1000 / FPS).toFixed(2)} ms/frame`);

// preview variant (base64-embeds media; no media here but measures the path)
const t0 = performance.now();
for (let f = 0; f < 30; f++) bridge.render_preview_frame(BigInt(f));
console.log(`render_preview_frame: avg ${((performance.now() - t0) / 30).toFixed(2)} ms/frame`);

// cost of a frame with no active cue (early return path)
const emptyDoc = baseDoc({ cues: [] });
set_document_json(JSON.stringify(emptyDoc));
await bridge.prepare(30);
const e0 = performance.now();
for (let f = 0; f < 30; f++) bridge.render_frame(BigInt(f));
console.log(`empty frame: avg ${((performance.now() - e0) / 30).toFixed(2)} ms/frame`);

// SVG payload sizes
set_document_json(JSON.stringify(baseDoc()));
await bridge.prepare(30);
const withText = bridge.render_frame(10n).length;
set_document_json(JSON.stringify(emptyDoc));
await bridge.prepare(30);
const withoutText = bridge.render_frame(10n).length;
console.log(`svg bytes: withText=${withText} withoutText=${withoutText}`);
