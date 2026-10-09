// Render an Arabic cue through a native template and dump word x positions.
import { loadBridge, baseDoc } from "./lib.mjs";

const { bridge, set_document_json } = await loadBridge();

const template = process.argv[2] || "remi";
const text = process.argv[3] || "مرحبا بالعالم من";

set_document_json(
  JSON.stringify(
    baseDoc({
      template,
      cues: [{ start_ms: 0, end_ms: 5000, text }],
    })
  )
);
await bridge.prepare(30);
const svg = bridge.render_frame(60n);
console.log(svg);
