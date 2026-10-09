// Headless SRT/VTT smoke test: parse → format → roundtrip through wasm.
import { loadBridge } from "./lib.mjs";
import { readFileSync } from "node:fs";

const mod = await loadBridge();

const srt = readFileSync(new URL("../rscaps-sample.srt", import.meta.url), "utf8");
const parsed = JSON.parse(mod.parse_subtitle(srt, "srt"));
console.log("cues:", parsed.cues.length, "duration_frames:", parsed.duration_frames);
console.log("cue2:", JSON.stringify(parsed.cues[1].text.slice(0, 40)));

const vtt = mod.format_subtitle(JSON.stringify(parsed.cues), "vtt");
console.log("vtt head:", JSON.stringify(vtt.split("\n").slice(0, 4).join(" | ")));

const back = JSON.parse(mod.parse_subtitle(vtt, "vtt"));
console.log(
  "roundtrip equal:",
  JSON.stringify(back.cues) === JSON.stringify(parsed.cues),
);

try {
  mod.parse_subtitle("no timing here", "srt");
  console.log("error passthrough: MISSING");
} catch (e) {
  console.log("error passthrough:", String(e).slice(0, 90));
}
