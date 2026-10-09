import { loadBridge, baseDoc } from "./lib.mjs";

const { bridge, set_document_json } = await loadBridge();

const cases = [
  ["long-en", "The quick brown fox jumps over the lazy dog while the engine measures every single glyph in rust using the ingested font file".repeat(1)],
  ["very-long-en", "The quick brown fox jumps over the lazy dog while the engine measures every single glyph in rust using the ingested font file. Pack my box with five dozen liquor jugs.".repeat(1)],
  ["long-arabic", "هذا نص عربي طويل جدا يستخدم لاختبار تقسيم الأسطر في محرك التخطيط"],
  ["long-hebrew", "זהו טקסט עברי ארוך מאוד שמשמש לבדיקת שורות במנוע הפריסה"],
];

for (const [name, text] of cases) {
  set_document_json(
    JSON.stringify(baseDoc({ cues: [{ start_ms: 0, end_ms: 10000, text }] }))
  );
  await bridge.prepare(30);
  const svg = bridge.render_frame(50n);
  const tspans = [...svg.matchAll(/<tspan[^>]*>([^<]*)</g)].map((m) => m[1]);
  console.log(`--- ${name}: len=${text.length}, tspans=${tspans.length}`);
  tspans.forEach((t, i) => console.log(`  [${i}] (${t.length}) ${t.slice(0, 80)}`));
}
