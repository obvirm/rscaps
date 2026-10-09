import { loadBridge, baseDoc } from "./lib.mjs";

const { bridge, set_document_json } = await loadBridge();

const cases = [
  ["arabic", "مرحبا بالعالم من rscaps"],
  ["hebrew", "שלום עולם"],
  ["mixed", "Hello مرحبا world"],
  ["long-arabic", "هذا نص عربي طويل جدا يستخدم لاختبار تقسيم الأسطر في محرك التخطيط"],
];

for (const [name, text] of cases) {
  set_document_json(
    JSON.stringify(baseDoc({ cues: [{ start_ms: 0, end_ms: 10000, text }] }))
  );
  await bridge.prepare(30);
  const svg = bridge.render_frame(50n);
  console.log(`--- ${name} (${text.length} chars) ---`);
  console.log(svg || "(empty)");
  console.log();
}
