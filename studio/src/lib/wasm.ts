import type { WasmBridge, CaptionDocument, TemplateEntry } from "./types";

interface FontSource {
  path: string;
  family?: string;
  weight?: number;
}

// Fonts are used twice: ingested into wasm (fframes measures/uploads them) and
// declared as @font-face (the browser paints them) — both sides must see the
// exact same file or preview and export diverge.
const FONT_SOURCES: FontSource[] = [
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

const FONT_LOADS = [
  '400 40px "Poppins"',
  '700 40px "Poppins"',
  '700 40px "Cairo"',
  '700 40px "Assistant"',
  '700 40px "Vazirmatn"',
  '700 40px "Heebo"',
  '400 40px "Lalezar"',
  '700 40px "Amiri"',
  '400 40px "Bebas Neue"',
  '400 40px "Inter"',
  '600 40px "Inter"',
  '700 40px "Inter"',
  '900 40px "Inter"',
  '400 40px "Anton"',
  '400 40px "Bungee"',
  '900 40px "Poppins"',
  '700 40px "Playfair Display"',
  '800 40px "Playfair Display"',
  '500 40px "Lora"',
  '600 40px "Gabarito"',
  '900 40px "Gabarito"',
  '800 40px "Bricolage Grotesque"',
  '400 40px "JetBrains Mono"',
  '600 40px "Caveat"',
  '700 40px "Caveat"',
  '300 40px "Montserrat"',
  '600 40px "Montserrat"',
  '700 40px "Montserrat"',
  '500 40px "Manrope"',
  '400 40px "Righteous"',
  '400 40px "VT323"',
  '400 40px "Komika Axis"',
  '400 40px "IM Fell English"',
  '400 40px "Lobster"',
];

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin);
}

export interface LoadedBridge {
  bridge: WasmBridge;
  setDocumentJson: (json: string) => void;
  parseSubtitle: (text: string, format: string) => string;
  formatSubtitle: (cuesJson: string, format: string) => string;
  /** Template catalog straight from caption-core — never hardcode a copy. */
  templateCatalog: TemplateEntry[];
  /** Canonical default document (params) straight from caption-core. */
  defaultDocument: CaptionDocument;
  fontFaceCss: string;
}

export async function loadBridge(): Promise<LoadedBridge> {
  // @fframes/editor is resolved via the importmap in index.html; the wasm glue
  // is served at /pkg/ by the Vite static-dirs middleware (not bundled). The
  // path is a variable so Vite treats the import as fully dynamic.
  const pkgUrl = "/pkg/caption_bridge.js";
  const mod = (await import(/* @vite-ignore */ pkgUrl)) as {
    default: (opts?: { module_or_path?: unknown }) => Promise<unknown>;
    create_wasm_bridge: () => WasmBridge;
    set_document_json: (json: string) => void;
    parse_subtitle: (text: string, format: string) => string;
    format_subtitle: (cuesJson: string, format: string) => string;
    template_catalog_json: () => string;
    default_document_json: () => string;
  };

  await mod.default();
  const bridge = mod.create_wasm_bridge();

  let faceCss = "";
  for (const src of FONT_SOURCES) {
    const res = await fetch(src.path);
    if (!res.ok) throw new Error(`font fetch failed: ${src.path} ${res.status}`);
    const bytes = new Uint8Array(await res.arrayBuffer());
    bridge.ingest_font(bytes);
    if (src.family && src.weight) {
      faceCss +=
        `@font-face{font-family:"${src.family}";font-style:normal;` +
        `font-weight:${src.weight};font-display:block;` +
        `src:url("data:font/ttf;base64,${bytesToBase64(bytes)}") format("truetype");}\n`;
    }
  }

  if (faceCss) {
    const style = document.createElement("style");
    style.id = "tpl-fonts";
    style.textContent = faceCss;
    document.head.append(style);
    await Promise.all(FONT_LOADS.map((f) => document.fonts.load(f)));
  }

  return {
    bridge,
    setDocumentJson: mod.set_document_json,
    parseSubtitle: mod.parse_subtitle,
    formatSubtitle: mod.format_subtitle,
    templateCatalog: JSON.parse(mod.template_catalog_json()) as TemplateEntry[],
    defaultDocument: JSON.parse(mod.default_document_json()) as CaptionDocument,
    fontFaceCss: faceCss,
  };
}
