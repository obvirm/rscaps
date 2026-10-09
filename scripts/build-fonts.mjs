// Copy the fontsource packages the caption stack uses into studio/tplfonts/
// and emit a registry the studio runtime uses to inline fonts as base64
// @font-face into per-frame SVG foreignObject tiles.
//
// Usage: node scripts/build-fonts.mjs   (after `pnpm install`)
import { readdir, readFile, writeFile, copyFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const pkgRoot = fileURLToPath(new URL("../", import.meta.url));
const nmRoot = join(pkgRoot, "node_modules");
const outRoot = join(pkgRoot, "studio", "tplfonts");

// Mirrors tscaps' fonts.css import list: variable packages at their default
// entry, static packages at the weights actually used.
const SPECS = [
  ["@fontsource-variable", "inter", "index.css"],
  ["@fontsource-variable", "manrope", "index.css"],
  ["@fontsource-variable", "montserrat", "index.css"],
  ["@fontsource-variable", "nunito", "index.css"],
  ["@fontsource-variable", "raleway", "index.css"],
  ["@fontsource-variable", "dm-sans", "index.css"],
  ["@fontsource-variable", "comfortaa", "index.css"],
  ["@fontsource-variable", "gabarito", "index.css"],
  ["@fontsource-variable", "bricolage-grotesque", "index.css"],
  ["@fontsource-variable", "oswald", "index.css"],
  ["@fontsource-variable", "eb-garamond", "index.css"],
  ["@fontsource-variable", "eb-garamond", "wght-italic.css"],
  ["@fontsource-variable", "fraunces", "index.css"],
  ["@fontsource-variable", "fraunces", "opsz-italic.css"],
  ["@fontsource-variable", "lora", "index.css"],
  ["@fontsource-variable", "playfair-display", "index.css"],
  ["@fontsource-variable", "caveat", "index.css"],
  ["@fontsource-variable", "dancing-script", "index.css"],
  ["@fontsource-variable", "jetbrains-mono", "index.css"],
  ["@fontsource", "kalam", "400.css"],
  ["@fontsource", "kalam", "700.css"],
  ["@fontsource", "poppins", "400.css"],
  ["@fontsource", "poppins", "700.css"],
  ["@fontsource", "roboto", "400.css"],
  ["@fontsource", "roboto", "700.css"],
  ["@fontsource", "anton", "400.css"],
  ["@fontsource", "bebas-neue", "400.css"],
  ["@fontsource", "bungee", "400.css"],
  ["@fontsource", "righteous", "400.css"],
  ["@fontsource", "bangers", "400.css"],
  ["@fontsource", "dm-serif-display", "400.css"],
  ["@fontsource", "im-fell-english", "400.css"],
  ["@fontsource", "pacifico", "400.css"],
  ["@fontsource", "lobster", "400.css"],
  ["@fontsource", "permanent-marker", "400.css"],
  ["@fontsource", "vt323", "400.css"],
  ["@fontsource", "press-start-2p", "400.css"],
  ["@fontsource-variable", "vazirmatn", "index.css"],
  ["@fontsource-variable", "cairo", "index.css"],
  ["@fontsource-variable", "noto-sans-arabic", "index.css"],
  ["@fontsource-variable", "noto-kufi-arabic", "index.css"],
  ["@fontsource", "tajawal", "400.css"],
  ["@fontsource", "tajawal", "700.css"],
  ["@fontsource", "lalezar", "400.css"],
  ["@fontsource", "amiri", "400.css"],
  ["@fontsource", "amiri", "700.css"],
  ["@fontsource-variable", "heebo", "index.css"],
  ["@fontsource-variable", "assistant", "index.css"],
  ["@fontsource-variable", "rubik", "index.css"],
  ["@fontsource", "noto-nastaliq-urdu", "400.css"],
  ["@fontsource-variable", "noto-sans-bengali", "wght.css"],
  ["@fontsource-variable", "noto-serif-bengali", "wght.css"],
  ["@fontsource-variable", "noto-sans-telugu", "wght.css"],
  ["@fontsource-variable", "noto-serif-telugu", "wght.css"],
  ["@fontsource-variable", "noto-sans-tamil", "wght.css"],
  ["@fontsource-variable", "noto-serif-tamil", "wght.css"],
  ["@fontsource-variable", "noto-sans-thai", "wght.css"],
  ["@fontsource-variable", "noto-serif-thai", "wght.css"],
];

const registry = {}; // family -> [{ url, weight, style, range }]
let copied = 0;

function addFace(family, { url, weight, style, range }) {
  (registry[family] ??= []).push({ url, weight, style, range });
}

async function copyFont(pkgDir, relUrl, outSubdir) {
  const src = join(pkgDir, relUrl.replace(/^\.\//, ""));
  const base = relUrl.split("/").pop();
  const destRel = `/tplfonts/${outSubdir}/${base}`;
  const dest = join(outRoot, outSubdir, base);
  if (!existsSync(dest)) {
    await mkdir(dirname(dest), { recursive: true });
    await copyFile(src, dest);
    copied++;
  }
  return destRel;
}

for (const [scope, name, cssFile] of SPECS) {
  const pkgName = scope.startsWith("@") ? `${scope}/${name}` : `@${scope}/${name}`;
  const pkgDir = join(nmRoot, scope.startsWith("@") ? scope : `@${scope}`, name);
  const cssPath = join(pkgDir, cssFile);
  if (!existsSync(cssPath)) {
    console.error(`missing ${pkgName} (${cssFile})`);
    process.exit(1);
  }
  const css = await readFile(cssPath, "utf8");
  const outSubdir = name;
  for (const block of css.matchAll(/@font-face\s*\{([^}]*)\}/g)) {
    const body = block[1];
    const family = body.match(/font-family:\s*'([^']+)'/)?.[1];
    const src = body.match(/url\((['"]?)([^)'"]+)\1\)/)?.[2];
    if (!family || !src) continue;
    const weight = body.match(/font-weight:\s*([^;]+);/)?.[1]?.trim() ?? "400";
    const style = body.match(/font-style:\s*([^;]+);/)?.[1]?.trim() ?? "normal";
    const range = body.match(/unicode-range:\s*([^;]+);/)?.[1]?.trim() ?? null;
    const url = await copyFont(pkgDir, src, outSubdir);
    addFace(family, { url, weight, style, range });
  }
}

// Komika Axis ships in-repo (not on Fontsource).
{
  const src = join(pkgRoot, "..", "tscaps", "apps", "studio", "src", "styles", "fonts", "komika-axis.woff2");
  const destDir = join(outRoot, "komika-axis");
  await mkdir(destDir, { recursive: true });
  const dest = join(destDir, "komika-axis.woff2");
  if (!existsSync(dest)) {
    await copyFile(src, dest);
    copied++;
  }
  addFace("Komika Axis", { url: "/tplfonts/komika-axis/komika-axis.woff2", weight: "400", style: "normal", range: null });
}

await mkdir(outRoot, { recursive: true });
await writeFile(join(outRoot, "registry.json"), JSON.stringify(registry, null, 1));

const faces = Object.values(registry).reduce((n, a) => n + a.length, 0);
console.log(`fonts: ${Object.keys(registry).length} families, ${faces} faces, ${copied} files copied`);
