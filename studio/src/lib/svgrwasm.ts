// Loader svgr-wasm: raster caption di WASM (bukan canvas browser).
//
// Inisialisasi sekali: init WASM + load semua font. Setelah itu export
// memanggil burn_frame() yang mengkomposit frame video + caption SVG
// sepenuhnya di WASM - browser hanya memindahkan bytes (putImageData).

let wasmReady: Promise<SvgrWasm> | null = null;

interface SvgrWasm {
  load_font: (data: Uint8Array) => void;
  font_count: () => number;
  burn_frame: (
    baseRgba: Uint8Array,
    svg: string,
    blendSvg: string,
    width: number,
    height: number,
  ) => Uint8Array;
  raster_svg: (svg: string, width: number, height: number) => Uint8Array;
}

/** Font yang sama dengan wasm.ts FONT_SOURCES - wajib sinkron. */
const FONT_PATHS = [
  "/fonts/ariblk.ttf",
  "/fonts/arial.ttf",
  "/tplfonts/ttf/poppins-latin-400-normal.ttf",
  "/tplfonts/ttf/poppins-latin-700-normal.ttf",
  "/tplfonts/ttf/cairo-arabic-700-normal.ttf",
  "/tplfonts/ttf/assistant-hebrew-700-normal.ttf",
  "/tplfonts/ttf/vazirmatn-arabic-700-normal.ttf",
  "/tplfonts/ttf/heebo-hebrew-700-normal.ttf",
  "/tplfonts/ttf/lalezar-arabic-400-normal.ttf",
  "/tplfonts/ttf/amiri-arabic-700-normal.ttf",
  "/tplfonts/ttf/bebas-neue-latin-400-normal.ttf",
  "/tplfonts/ttf/inter-latin-400-normal.ttf",
  "/tplfonts/ttf/inter-latin-600-normal.ttf",
  "/tplfonts/ttf/inter-latin-700-normal.ttf",
  "/tplfonts/ttf/inter-latin-900-normal.ttf",
  "/tplfonts/ttf/anton-latin-400-normal.ttf",
  "/tplfonts/ttf/bungee-latin-400-normal.ttf",
  "/tplfonts/ttf/poppins-latin-900-normal.ttf",
  "/tplfonts/ttf/playfair-display-latin-700-normal.ttf",
  "/tplfonts/ttf/playfair-display-latin-800-normal.ttf",
  "/tplfonts/ttf/lora-latin-500-normal.ttf",
  "/tplfonts/ttf/gabarito-latin-600-normal.ttf",
  "/tplfonts/ttf/gabarito-latin-900-normal.ttf",
  "/tplfonts/ttf/bricolage-grotesque-latin-800-normal.ttf",
  "/tplfonts/ttf/jetbrains-mono-latin-400-normal.ttf",
  "/tplfonts/ttf/caveat-latin-600-normal.ttf",
  "/tplfonts/ttf/caveat-latin-700-normal.ttf",
  "/tplfonts/ttf/montserrat-latin-300-normal.ttf",
  "/tplfonts/ttf/montserrat-latin-600-normal.ttf",
  "/tplfonts/ttf/montserrat-latin-700-normal.ttf",
  "/tplfonts/ttf/manrope-latin-500-normal.ttf",
  "/tplfonts/ttf/righteous-latin-400-normal.ttf",
  "/tplfonts/ttf/vt323-latin-400-normal.ttf",
  "/tplfonts/ttf/komika-axis-latin-400-normal.ttf",
  "/tplfonts/ttf/im-fell-english-latin-400-normal.ttf",
  "/tplfonts/ttf/lobster-latin-400-normal.ttf",
];

/** Init WASM + load semua font. Idempoten (cached promise). */
export function initSvgrWasm(): Promise<SvgrWasm> {
  if (!wasmReady) {
    wasmReady = (async () => {
      // Path dibuat dinamis supaya Vite tidak mencoba resolve/meng-bundle
      // file WASM ini (di-serve oleh plugin static-dirs di /svgrpkg/).
      const url = "/svgrpkg/" + "svgr_wasm.js";
      const mod = (await import(/* @vite-ignore */ url)) as {
        default: (opts?: { module_or_path?: unknown }) => Promise<unknown>;
        load_font: (data: Uint8Array) => void;
        font_count: () => number;
        burn_frame: (
          baseRgba: Uint8Array,
          svg: string,
          blendSvg: string,
          width: number,
          height: number,
        ) => Uint8Array;
        raster_svg: (svg: string, width: number, height: number) => Uint8Array;
      };
      await mod.default();
      // Load font paralel - font identik dengan caption-bridge supaya
      // ukuran teks WASM-raster = ukuran teks caption-core (paritas).
      await Promise.all(
        FONT_PATHS.map(async (path) => {
          try {
            const res = await fetch(path);
            if (!res.ok) return;
            const bytes = new Uint8Array(await res.arrayBuffer());
            mod.load_font(bytes);
          } catch {
            // Font gagal dimuat - lanjut (font sistem mungkin menggantikan).
          }
        }),
      );
      return mod as unknown as SvgrWasm;
    })();
  }
  return wasmReady;
}

/**
 * Komposit frame video + caption di WASM.
 *
 * @param baseRgba - pixels frame video RGBA (width*height*4) dari WebCodecs
 * @param svg - string SVG caption (layer normal)
 * @param blendSvg - layer mix-blend-mode:difference ("" = kosong)
 * @returns pixels RGBA hasil komposit
 */
export async function burnFrameWasm(
  baseRgba: Uint8Array,
  svg: string,
  blendSvg: string,
  width: number,
  height: number,
): Promise<Uint8Array> {
  const wasm = await initSvgrWasm();
  return wasm.burn_frame(baseRgba, svg, blendSvg, width, height);
}
