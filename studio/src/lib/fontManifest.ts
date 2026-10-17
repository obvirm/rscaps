// Single source of the caption font manifest for the studio.
//
// Every consumer (caption-bridge ingest + @font-face, svgr-wasm fontdb, the
// Typography/Element panels) MUST read this list instead of keeping its own
// copy — a second list is a guarantee of drift (preview/export would then
// raster with different fonts).

export interface FontSource {
  path: string;
  family?: string;
  weight?: number;
}

/** Fonts used twice: ingested into wasm (layout/measure) and declared as
 * @font-face (browser paints them). Both sides must see the exact same file. */
export const FONT_SOURCES: FontSource[] = [
  { path: "/fonts/ariblk.ttf" },
  { path: "/fonts/arial.ttf" },
  { path: "/tplfonts/ttf/poppins-latin-400-normal.ttf", family: "Poppins", weight: 400 },
  { path: "/tplfonts/ttf/poppins-latin-700-normal.ttf", family: "Poppins", weight: 700 },
  { path: "/tplfonts/ttf/cairo-arabic-700-normal.ttf", family: "Cairo", weight: 700 },
  { path: "/tplfonts/ttf/assistant-hebrew-700-normal.ttf", family: "Assistant", weight: 700 },
  { path: "/tplfonts/ttf/vazirmatn-arabic-700-normal.ttf", family: "Vazirmatn", weight: 700 },
  { path: "/tplfonts/ttf/heebo-hebrew-700-normal.ttf", family: "Heebo", weight: 700 },
  { path: "/tplfonts/ttf/lalezar-arabic-400-normal.ttf", family: "Lalezar", weight: 400 },
  { path: "/tplfonts/ttf/amiri-arabic-700-normal.ttf", family: "Amiri", weight: 700 },
  { path: "/tplfonts/ttf/bebas-neue-latin-400-normal.ttf", family: "Bebas Neue", weight: 400 },
  { path: "/tplfonts/ttf/inter-latin-400-normal.ttf", family: "Inter", weight: 400 },
  { path: "/tplfonts/ttf/inter-latin-600-normal.ttf", family: "Inter", weight: 600 },
  { path: "/tplfonts/ttf/inter-latin-700-normal.ttf", family: "Inter", weight: 700 },
  { path: "/tplfonts/ttf/inter-latin-900-normal.ttf", family: "Inter", weight: 900 },
  { path: "/tplfonts/ttf/anton-latin-400-normal.ttf", family: "Anton", weight: 400 },
  { path: "/tplfonts/ttf/bungee-latin-400-normal.ttf", family: "Bungee", weight: 400 },
  { path: "/tplfonts/ttf/poppins-latin-900-normal.ttf", family: "Poppins", weight: 900 },
  { path: "/tplfonts/ttf/playfair-display-latin-700-normal.ttf", family: "Playfair Display", weight: 700 },
  { path: "/tplfonts/ttf/playfair-display-latin-800-normal.ttf", family: "Playfair Display", weight: 800 },
  { path: "/tplfonts/ttf/lora-latin-500-normal.ttf", family: "Lora", weight: 500 },
  { path: "/tplfonts/ttf/gabarito-latin-600-normal.ttf", family: "Gabarito", weight: 600 },
  { path: "/tplfonts/ttf/gabarito-latin-900-normal.ttf", family: "Gabarito", weight: 900 },
  { path: "/tplfonts/ttf/bricolage-grotesque-latin-800-normal.ttf", family: "Bricolage Grotesque", weight: 800 },
  { path: "/tplfonts/ttf/jetbrains-mono-latin-400-normal.ttf", family: "JetBrains Mono", weight: 400 },
  { path: "/tplfonts/ttf/caveat-latin-600-normal.ttf", family: "Caveat", weight: 600 },
  { path: "/tplfonts/ttf/caveat-latin-700-normal.ttf", family: "Caveat", weight: 700 },
  { path: "/tplfonts/ttf/montserrat-latin-300-normal.ttf", family: "Montserrat", weight: 300 },
  { path: "/tplfonts/ttf/montserrat-latin-600-normal.ttf", family: "Montserrat", weight: 600 },
  { path: "/tplfonts/ttf/montserrat-latin-700-normal.ttf", family: "Montserrat", weight: 700 },
  { path: "/tplfonts/ttf/manrope-latin-500-normal.ttf", family: "Manrope", weight: 500 },
  { path: "/tplfonts/ttf/righteous-latin-400-normal.ttf", family: "Righteous", weight: 400 },
  { path: "/tplfonts/ttf/vt323-latin-400-normal.ttf", family: "VT323", weight: 400 },
  { path: "/tplfonts/ttf/komika-axis-latin-400-normal.ttf", family: "Komika Axis", weight: 400 },
  { path: "/tplfonts/ttf/im-fell-english-latin-400-normal.ttf", family: "IM Fell English", weight: 400 },
  { path: "/tplfonts/ttf/lobster-latin-400-normal.ttf", family: "Lobster", weight: 400 },
];

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
      (f) => `${f.weight} 40px "${f.family}"`,
    ),
  ),
];
