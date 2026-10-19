// Loader svgr-wasm: raster caption di WASM (bukan canvas browser).
//
// Font diambil dari fontManifest.ts (satu sumber) — wajib identik dengan
// caption-bridge supaya ukuran teks WASM-raster = caption-core (paritas).
import { FONT_PATHS } from "./fontManifest";
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

/**
 * Raster SVG caption SAJA di WASM → pixels RGBA (premultiplied, tiny-skia).
 * Dipakai jalur GPU-composite: raster caption di WASM (paritas glyph CLI),
 * komposit source-over/difference di GPU canvas — piksel video TIDAK
 * readback ke CPU.
 */
export async function rasterSvgWasm(
  svg: string,
  width: number,
  height: number,
): Promise<Uint8Array> {
  const wasm = await initSvgrWasm();
  return wasm.raster_svg(svg, width, height);
}
