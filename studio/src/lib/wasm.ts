import type { WasmBridge, CaptionDocument, TemplateEntry } from "./types";
import { FONT_SOURCES, FONT_LOADS, FONT_FAMILIES } from "./fontManifest";

// Re-exported so existing panel imports (`./wasm`) keep working — the list
// itself lives once, in fontManifest.ts.
export { FONT_FAMILIES };

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
  /** Frame video (JPEG penuh) untuk layer kaca selene — per frame. */
  setFrameImage: (bytes: Uint8Array) => void;
  clearFrameImage: () => void;
  /** Daftar kata dokumen aktif (JSON `[WordInfo, .]`) - element inspector. */
  listWords: () => string;
  parseSubtitle: (text: string, format: string) => string;
  formatSubtitle: (cuesJson: string, format: string) => string;
  /** Params JSON → CSS subset (panel Code settings). */
  paramsToCss: (paramsJson: string) => string;
  /** CSS subset → params JSON (merge ke base). */
  paramsFromCss: (baseParamsJson: string, css: string) => string;
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
    set_frame_image: (bytes: Uint8Array) => void;
    clear_frame_image: () => void;
    list_words_json: () => string;
    parse_subtitle: (text: string, format: string) => string;
    format_subtitle: (cuesJson: string, format: string) => string;
    params_to_css: (paramsJson: string) => string;
    params_from_css: (baseParamsJson: string, css: string) => string;
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
    setFrameImage: mod.set_frame_image,
    clearFrameImage: mod.clear_frame_image,
    listWords: mod.list_words_json,
    parseSubtitle: mod.parse_subtitle,
    formatSubtitle: mod.format_subtitle,
    paramsToCss: mod.params_to_css,
    paramsFromCss: mod.params_from_css,
    templateCatalog: JSON.parse(mod.template_catalog_json()) as TemplateEntry[],
    defaultDocument: JSON.parse(mod.default_document_json()) as CaptionDocument,
    fontFaceCss: faceCss,
  };
}
