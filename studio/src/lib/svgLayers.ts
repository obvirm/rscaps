// Split SVG render_frame menjadi layer base + blend (`mix-blend-mode: difference`).
// Dipakai state (preview/export inline) dan burnWorker (render di worker) —
// SATU implementasi, jangan digandakan.
export interface SvgLayers {
  base: string;
  blend?: string | null;
}

export function splitSvgLayers(raw: string): SvgLayers | null {
  if (!raw) return null;
  // Split base/blend: hapus node <text> satunya - struktur grup (font attrs,
  // rise, clip defs) TETAP dipertahankan di kedua layer. Hanya menyalin node
  // teks tanpa konteks grup membuat usvgr kehilangan font-family/size
  // (render kosong - bug 2026-10).
  const BLEND = "mix-blend-mode: difference";
  if (!raw.includes(BLEND)) return { base: raw };
  const TEXT_RE = /<text[\s\S]*?<\/text>/g;
  const texts = raw.match(TEXT_RE) ?? [];
  const blendTexts = texts.filter((t) => t.includes(BLEND));
  const baseTexts = texts.filter((t) => !t.includes(BLEND));
  let baseSvg = raw;
  for (const t of blendTexts) baseSvg = baseSvg.replace(t, "");
  let blendSvg = raw;
  for (const t of baseTexts) blendSvg = blendSvg.replace(t, "");
  return { base: baseSvg, blend: blendSvg };
}
