// Decompress fontsource woff2 files into the static TTFs the caption stack
// embeds (caption-core/assets) and serves (/tplfonts/ttf/). fframes has no
// variable-font support, so we use the static @fontsource packages and pick
// the exact weights each template asks for.
//
// Usage: node scripts/woff2-to-ttf.mjs
import { readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { decompress } from "wawoff2";

const pkgRoot = fileURLToPath(new URL("../", import.meta.url));
const nm = join(pkgRoot, "node_modules");
const outs = [
  join(pkgRoot, "studio", "tplfonts", "ttf"),
  join(pkgRoot, "crates", "caption-core", "assets"),
];

// scope/weight picks per template (latin subset only — the templates that
// need Arabic/Hebrew ship those subsets separately).
const JOBS = [
  ["@fontsource/inter", "inter-latin-400-normal"],
  ["@fontsource/inter", "inter-latin-600-normal"],
  ["@fontsource/inter", "inter-latin-700-normal"],
  ["@fontsource/inter", "inter-latin-900-normal"],
  ["@fontsource/anton", "anton-latin-400-normal"],
  ["@fontsource/bungee", "bungee-latin-400-normal"],
  ["@fontsource/playfair-display", "playfair-display-latin-700-normal"],
  ["@fontsource/playfair-display", "playfair-display-latin-800-normal"],
  ["@fontsource/lora", "lora-latin-500-normal"],
  ["@fontsource/gabarito", "gabarito-latin-600-normal"],
  ["@fontsource/gabarito", "gabarito-latin-900-normal"],
  ["@fontsource/bricolage-grotesque", "bricolage-grotesque-latin-800-normal"],
  ["@fontsource/jetbrains-mono", "jetbrains-mono-latin-400-normal"],
  ["@fontsource/caveat", "caveat-latin-600-normal"],
  ["@fontsource/caveat", "caveat-latin-700-normal"],
  ["@fontsource/montserrat", "montserrat-latin-300-normal"],
  ["@fontsource/montserrat", "montserrat-latin-600-normal"],
  ["@fontsource/montserrat", "montserrat-latin-700-normal"],
  ["@fontsource/manrope", "manrope-latin-500-normal"],
  ["@fontsource/poppins", "poppins-latin-900-normal"],
  ["@fontsource/righteous", "righteous-latin-400-normal"],
  ["@fontsource/vt323", "vt323-latin-400-normal"],
  ["@fontsource/im-fell-english", "im-fell-english-latin-400-normal"],
  ["@fontsource/lobster", "lobster-latin-400-normal"],
  // Script-stack faces (tscaps FontStackCatalog): arabic = Cairo (poppins,
  // gabarito, comfortaa, nunito stacks) / Vazirmatn (inter, montserrat, ...)
  // / Lalezar (anton, bungee, ...) / Amiri (playfair-display, lora, ...);
  // hebrew = Assistant (poppins, playfair...) / Heebo (inter, anton, ...).
  ["@fontsource/cairo", "cairo-arabic-700-normal"],
  ["@fontsource/assistant", "assistant-hebrew-700-normal"],
  ["@fontsource/vazirmatn", "vazirmatn-arabic-700-normal"],
  ["@fontsource/lalezar", "lalezar-arabic-400-normal"],
  ["@fontsource/amiri", "amiri-arabic-700-normal"],
  ["@fontsource/heebo", "heebo-hebrew-700-normal"],
  // Komika Axis is not on fontsource; tscaps ships the original freeware
  // woff2 (license: apostrophic-lab freeware, "use as is" — copied next to
  // the TTF as komika-axis_license.txt).
  { src: "E:/project/tscaps/apps/studio/src/styles/fonts/komika-axis.woff2", base: "komika-axis-latin-400-normal" },
];

let done = 0;
for (const job of JOBS) {
  const [src, base] = Array.isArray(job)
    ? [join(nm, job[0], "files", `${job[1]}.woff2`), job[1]]
    : [job.src, job.base];
  const ttf = await decompress(new Uint8Array(await readFile(src)));
  for (const outDir of outs) {
    const dest = join(outDir, `${base}.ttf`);
    if (!existsSync(dest)) {
      await writeFile(dest, Buffer.from(ttf));
      done++;
    }
  }
  console.log(`${base}.ttf (${ttf.length} bytes)`);
}
console.log(`wrote ${done} files`);
