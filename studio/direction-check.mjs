// Direction-override smoke test: force RTL on a Latin-majority doc and check
// the rendered segment places the first logical word at the right end.
import { loadBridge, baseDoc } from "./lib.mjs";

const { bridge, set_document_json } = await loadBridge();

const doc = baseDoc({
  cues: [
    { start_ms: 0, end_ms: 3000, text: "hello world rscaps مرحبا" },
  ],
  params: {
    ...baseDoc().params,
    direction: "rtl",
    template: "remi",
  },
  template: "remi",
});

set_document_json(JSON.stringify(doc));
const meta = await bridge.prepare(30);
console.log("prepared:", meta.durationInFrames, "frames");

// Render a frame in the middle of the cue and dump word x positions.
const svg = bridge.render_frame(30n);
const xs = [...svg.matchAll(/<text x="([\d.]+)"/g)].map((m) => parseFloat(m[1]));
const words = [...svg.matchAll(/<text x="[\d.]+"[^>]*>([^<]+)<\/text>/g)].map((m) => m[1]);
console.log("words (logical):", words);
console.log("x positions:", xs);

if (xs.length < 2) {
  console.error("FAIL: expected multiple words");
  process.exit(1);
}

// RTL: first word's x should be the largest (rightmost).
const firstIsRightmost = xs[0] > xs[xs.length - 1];
console.log("first word rightmost (RTL):", firstIsRightmost);

// LTR for comparison
doc.params.direction = "ltr";
set_document_json(JSON.stringify(doc));
await bridge.prepare(30);
const svg2 = bridge.render_frame(30n);
const xs2 = [...svg2.matchAll(/<text x="([\d.]+)"/g)].map((m) => parseFloat(m[1]));
const firstIsLeftmost = xs2[0] < xs2[xs2.length - 1];
console.log("LTR first word leftmost:", firstIsLeftmost);

if (firstIsRightmost && firstIsLeftmost) {
  console.log("PASS");
} else {
  console.error("FAIL");
  process.exit(1);
}
