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
  register_image: (href: string, rgba: Uint8Array, width: number, height: number) => void;
  unregister_image: (href: string) => void;
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
        register_image: (href: string, rgba: Uint8Array, width: number, height: number) => void;
        unregister_image: (href: string) => void;
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
      // Load font: FETCH paralel (kecepatan) tapi load_font BERURUTAN sesuai
      // fontManifest — urutan fontdb menentukan pemilihan fallback glif usvgr
      // (face PERTAMA yang mencakup karakter), jadi preview/export web harus
      // urutannya sama dengan CLI (fonts.rs load_manifest).
      const datas = await Promise.all(
        FONT_PATHS.map(async (path) => {
          try {
            const res = await fetch(path);
            if (!res.ok) return null;
            return new Uint8Array(await res.arrayBuffer());
          } catch {
            // Font gagal dimuat - lanjut (font sistem mungkin menggantikan).
            return null;
          }
        }),
      );
      for (const bytes of datas) {
        if (bytes) mod.load_font(bytes);
      }
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
  // Registry gambar jalur inline (main thread): kaca selene + stroke iris.
  await ensureInlineImages(svg, baseRgba, width, height);
  return wasm.burn_frame(baseRgba, svg, blendSvg, width, height);
}

// Registry gambar main-thread (jalur inline) — cermin burnWorker. Kaca
// selene ditandai param `rscaps-glass`; gambar statis lain (stroke iris,
// upload user) decode-once.
let inlineGlassKey = "";
const inlineStaticKeys = new Set<string>();

async function ensureInlineImages(
  svg: string,
  baseRgba: Uint8Array,
  width: number,
  height: number,
): Promise<void> {
  if (!svg.includes("data:image/")) return;
  const glass = svg.match(/href="(data:image\/jpeg;rscaps-glass;base64,[^"]+)"/);
  if (glass && glass[1] !== inlineGlassKey) {
    if (inlineGlassKey) await unregisterImageWasm(inlineGlassKey);
    await registerImageWasm(glass[1], baseRgba, width, height);
    inlineGlassKey = glass[1];
  }
  for (const m of svg.matchAll(/href="(data:image\/[a-z0-9.+-]+(?:;[a-z0-9-]+)*;base64,[^"]+)"/g)) {
    const href = m[1];
    if (href === inlineGlassKey || inlineStaticKeys.has(href)) continue;
    const decoded = await decodeDataUrlRgba(href);
    if (decoded) {
      await registerImageWasm(href, decoded.rgba, decoded.w, decoded.h);
      inlineStaticKeys.add(href);
    }
  }
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

/**
 * Daftarkan piksel RGBA untuk `<image href>` — usvgr TIDAK men-decode data
 * URL; kunci harus PERSIS string href di SVG. Kaca selene: kunci berganti
 * tiap frame → pemanggil wajib unregister kunci lama (lihat burnWorker).
 */
export async function registerImageWasm(
  href: string,
  rgba: Uint8Array,
  width: number,
  height: number,
): Promise<void> {
  const wasm = await initSvgrWasm();
  wasm.register_image(href, rgba, width, height);
}

export async function unregisterImageWasm(href: string): Promise<void> {
  const wasm = await initSvgrWasm();
  wasm.unregister_image(href);
}

/** Decode data URL gambar → RGBA (untuk gambar statis seperti stroke iris). */
export async function decodeDataUrlRgba(
  href: string,
): Promise<{ rgba: Uint8Array; w: number; h: number } | null> {
  try {
    // Parametrized data URL (masker pastor `;rscaps-pmask`) — buang param
    // sebelum fetch supaya browser decode normal sebagai mime dasarnya.
    const clean = href.replace(/;rscaps-[a-z-]+(?=;base64,)/, "");
    const blob = await (await fetch(clean)).blob();
    const bmp = await createImageBitmap(blob);
    const c = new OffscreenCanvas(bmp.width, bmp.height);
    const cctx = c.getContext("2d");
    if (!cctx) return null;
    cctx.drawImage(bmp, 0, 0);
    const d = cctx.getImageData(0, 0, bmp.width, bmp.height);
    return { rgba: new Uint8Array(d.data.buffer), w: bmp.width, h: bmp.height };
  } catch {
    return null;
  }
}
