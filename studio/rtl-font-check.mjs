import { readFile } from "node:fs/promises";
import { loadBridge, baseDoc } from "./lib.mjs";

const { bridge, set_document_json } = await loadBridge();

const candidates = [
  "arial.ttf",
  "arialbd.ttf",
  "tahoma.ttf",
  "calibri.ttf",
  "segoeui.ttf",
  "times.ttf",
];

const arabic = "هذا نص عربي طويل جدا يستخدم لاختبار تقسيم الأسطر في محرك التخطيط";

for (const file of candidates) {
  try {
    const bytes = await readFile(`C:/Windows/Fonts/${file}`);
    const face = bridge.ingest_font(bytes);
    const family = face?.name;
    set_document_json(
      JSON.stringify(
        baseDoc({
          cues: [{ start_ms: 0, end_ms: 10000, text: arabic }],
          params: { ...baseDoc().params, font_family: family, font_weight: face?.weight ?? 400 },
        })
      )
    );
    await bridge.prepare(30);
    const svg = bridge.render_frame(50n);
    const tspans = [...svg.matchAll(/<tspan[^>]*>([^<]*)</g)].map((m) => m[1]);
    const dx = /dx="(\d+)"/.exec(svg)?.[1];
    console.log(
      `${file} -> face=${JSON.stringify(face)} tspans=${tspans.length} dx=${dx} lines=[${tspans
        .map((t) => t.slice(0, 30))
        .join(" | ")}]`
    );
  } catch (err) {
    console.log(`${file}: FAILED ${err.message}`);
  }
}
