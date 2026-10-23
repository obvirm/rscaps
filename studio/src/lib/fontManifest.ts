// Single source of the caption font manifest for the studio AND the CLI
// (rscaps-cli/src/fonts.rs membaca fontManifest.json yang sama - urutan file
// identik = fallback glif usvgr identik di kedua jalur).
//
// Data hidup di fontManifest.json (array mentah) - file ini hanya membungkus
// dengan tipe + derivasi (FONT_FAMILIES/FONT_PATHS/FONT_LOADS). Menambah
// font = edit fontManifest.json saja.

export interface FontSource {
  path: string;
  family?: string;
  weight?: number;
  /** Face miring (real italic, bukan sintesis browser). */
  style?: "italic";
}

import rawSources from "./fontManifest.json";

/** Fonts used twice: ingested into wasm (layout/measure) and declared as
 * @font-face (browser paints them). Both sides must see the exact same file. */
export const FONT_SOURCES: FontSource[] = rawSources as FontSource[];

/** Unique caption font families, for the Typography/Element panels. */
export const FONT_FAMILIES: string[] = [
  ...new Set(FONT_SOURCES.filter((f) => f.family).map((f) => f.family!)),
].sort((a, b) => a.localeCompare(b));

/** Paths only, for consumers that just need the bytes (svgr-wasm fontdb). */
export const FONT_PATHS: string[] = FONT_SOURCES.map((f) => f.path);

/** CSS `document.fonts.load` descriptors for the declared @font-face families. */
export const FONT_LOADS: string[] = [
  ...new Set(
    FONT_SOURCES.filter((f) => f.family && f.weight).map(
      (f) => `${f.style === "italic" ? "italic " : ""}${f.weight} 40px "${f.family}"`,
    ),
  ),
];
