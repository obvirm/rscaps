// Extracts the font-stack table (id → face per writing system) from the
// tscaps reference into studio/tplfonts/stacks.json as plain data.
import { readFileSync, writeFileSync } from "node:fs";

const SRC =
  "E:/project/tscaps/apps/studio/src/core/fonts/domain/FontStackCatalog.ts";
const OUT = new URL("../studio/tplfonts/stacks.json", import.meta.url);

const ts = readFileSync(SRC, "utf8");
const start = ts.indexOf("FONT_STACKS");
const body = ts.slice(start);

const stacks = {};
const stackRe =
  /\{\s*id:\s*'([^']+)',\s*faces:\s*\{([^}]*)\}/g;
let m;
while ((m = stackRe.exec(body)) !== null) {
  const id = m[1];
  const facesBlock = m[2];
  const faces = {};
  const faceRe = /(\w+):\s*'([^']+)'/g;
  let f;
  while ((f = faceRe.exec(facesBlock)) !== null) faces[f[1]] = f[2];
  stacks[id] = faces;
}

if (Object.keys(stacks).length === 0) {
  console.error("extract-font-stacks: no stacks parsed");
  process.exit(1);
}
writeFileSync(OUT, JSON.stringify(stacks, null, 2) + "\n");
console.log(
  `extract-font-stacks: ${Object.keys(stacks).length} stacks -> ${OUT.pathname}`,
);
