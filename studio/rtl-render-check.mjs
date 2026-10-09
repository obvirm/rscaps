// Render the Arabic cue (default doc, v0 path) and print the SVG body.
import { loadBridge, baseDoc } from "./lib.mjs";

const { bridge, set_document_json } = await loadBridge();

const doc = baseDoc({
  duration_frames: 300,
  cues: [
    { start_ms: 2500, end_ms: 5500, text: "مرحبا بالعالم من" },
  ],
});
delete doc.template;
set_document_json(JSON.stringify(doc));
const meta = await bridge.prepare(30);
console.log("prepared", JSON.stringify(meta));
const svg = bridge.render_frame(120n);
console.log(svg);
