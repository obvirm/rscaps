// Worker GPU-composite: video pixels TIDAK pernah meninggalkan GPU.
//
// Frame video masuk sebagai VideoFrame (transferable, GPU-backed) → digambar
// ke OffscreenCanvas worker (GPU) → layer caption diraster di WASM (svgr-wasm,
// paritas glyph CLI) → digambar ke canvas yang sama via drawImage
// (source-over untuk base, difference untuk blend) → VideoFrame(canvas)
// ditransfer balik ke encoder. Tanpa getImageData / tanpa buffer 8.3MB
// GPU→CPU→GPU per frame.
import { rasterSvgWasm } from "./svgrwasm";

interface BurnRequest {
  idx: number;
  svg: string;
  blend: string;
  width: number;
  height: number;
  /** detik */
  t: number;
  /** detik */
  dur: number;
  /** Frame video GPU (jalur utama). */
  videoFrame?: VideoFrame | null;
  /** Fallback raster (rotation/flip tak tertangani canvas). */
  bitmap?: ImageBitmap | null;
  /** Fallback buffer mentah (jalur inline-ish). */
  buf?: ArrayBuffer;
  /** Mode preview: respons = ImageBitmap komposit (bukan VideoFrame). */
  preview?: boolean;
}

interface BurnResponse {
  idx: number;
  frame?: VideoFrame | null;
  buf: ArrayBuffer | null;
  /** Mode preview: frame komposit sebagai ImageBitmap (transferable). */
  bitmap?: ImageBitmap | null;
  error?: string;
}

// Kanvas per-worker.
let gWidth = 0;
let gHeight = 0;
let gCanvas: OffscreenCanvas | null = null;
let gCtx: OffscreenCanvasRenderingContext2D | null = null;

const ensureCanvas = (w: number, h: number) => {
  if (gCanvas && gWidth === w && gHeight === h) return gCtx;
  gWidth = w;
  gHeight = h;
  gCanvas = new OffscreenCanvas(w, h);
  gCtx = gCanvas.getContext("2d");
  return gCtx;
};

// Cache layer caption per SVG unik (cue statis = svg identik tiap frame).
// Key = string svg, value = ImageBitmap hasil unpremultiply.
// Cache layer caption per SVG unik. 48 = cukup untuk tail statis antar cue
// (terbukti aman saat pipeline inline; 2.4GB tekstur hanya berbahaya bila
// digabung antrean submit encoder burst — lihat encQueue di export.ts).
const LAYER_CACHE_MAX = 48;

/**
 * Apakah svg berpotensi menghasilkan piksel cat? Layer "kosong" (hanya
 * defs + grup tanpa teks/bentuk ber-fill — mis. skeleton base luna yang
 * berisi clipPath mati) melewati raster penuh (alloc 8.3MB + render ≈ 5-10ms)
 * padahal hasilnya pasti transparan. Kriteria: setelah tag penutup defs
 * (atau seluruh svg bila tanpa defs) harus ada elemen yang bisa mengecat.
 */
function svgHasPaint(svg: string): boolean {
  const idx = svg.indexOf("</defs>");
  const body = idx >= 0 ? svg.slice(idx + 7) : svg;
  return (
    body.includes("<text") ||
    body.includes("<path") ||
    body.includes("<rect") ||
    body.includes("<circle") ||
    body.includes("<image") ||
    body.includes("<use")
  );
}
const layerCache = new Map<string, ImageBitmap>();

// Profiler worker (dev): rata-rata ms per komponen, log tiap 600 frame.
const wp = { raster: 0, mkBmp: 0, draw: 0, vf: 0, n: 0, cacheHit: 0, cacheMiss: 0 };

async function layerBitmap(svg: string, width: number, height: number): Promise<ImageBitmap | null> {
  if (!svg || svg.trim().length === 0) return null;
  if (!svgHasPaint(svg)) return null; // layer kosong: tak perlu raster
  const cached = layerCache.get(svg);
  if (cached) {
    wp.cacheHit++;
    // LRU touch: pindahkan ke akhir.
    layerCache.delete(svg);
    layerCache.set(svg, cached);
    return cached;
  }
  wp.cacheMiss++;
  const r0 = performance.now();
  // raster_svg (Rust) sudah mengembalikan STRAIGHT alpha — tidak perlu
  // unpremultiply lagi di JS (hemat ~5ms per miss).
  const straight = await rasterSvgWasm(svg, width, height);
  const r1 = performance.now();
  // Salin ke buffer JS milik sendiri (view wasm bisa detached saat memory.grow).
  const bmp = await createImageBitmap(
    new ImageData(new Uint8ClampedArray(straight), width, height),
  );
  const r2 = performance.now();
  wp.raster += r1 - r0;
  wp.mkBmp += r2 - r1;
  if (layerCache.size >= LAYER_CACHE_MAX) {
    const oldest = layerCache.keys().next().value;
    if (oldest !== undefined) {
      layerCache.get(oldest)?.close();
      layerCache.delete(oldest);
    }
  }
  layerCache.set(svg, bmp);
  return bmp;
}

function wpLog(): void {
  if (wp.n === 0 || wp.n % 600 !== 0) return;
  const n = wp.n;
  console.log(
    `[burnWorker] n=${n} raster=${(wp.raster / n).toFixed(2)} mkBmp=${(wp.mkBmp / n).toFixed(2)} draw=${(wp.draw / n).toFixed(2)} vf=${(wp.vf / n).toFixed(2)} hit=${wp.cacheHit} miss=${wp.cacheMiss}`,
  );
}

self.addEventListener("error", (ev) => {
  console.error("[burnWorker] uncaught", ev.message, ev.filename, ev.lineno);
});
self.addEventListener("unhandledrejection", (ev) => {
  console.error("[burnWorker] unhandledrejection", String(ev.reason));
});

self.onmessage = async (e: MessageEvent<BurnRequest>) => {
  const { idx, svg, blend, width, height, t, dur, videoFrame, bitmap, buf, preview } = e.data;
  try {
    const ts = Math.round(t * 1e6);
    const durUs = Math.round(dur * 1e6) || undefined;

    // ── Jalur utama: VideoFrame GPU → canvas → VideoFrame GPU ────────────
    if (videoFrame) {
      const hasCaption = (svg && svg.trim().length > 0) || (blend && blend.trim().length > 0);
      if (!hasCaption) {
        // Cue kosong: lewat saja (restamp timestamp).
        const out = new VideoFrame(videoFrame, { timestamp: ts, duration: durUs });
        videoFrame.close();
        const res: BurnResponse = { idx, frame: out, buf: null };
        (self as unknown as Worker).postMessage(res, [out]);
        return;
      }
      const ctx = ensureCanvas(width, height);
      if (!ctx) throw new Error("worker 2D context gagal");
      const d0 = performance.now();
      ctx.globalCompositeOperation = "source-over";
      ctx.drawImage(videoFrame, 0, 0, width, height);
      videoFrame.close();
      const d1 = performance.now();
      wp.draw += d1 - d0;

      const base = await layerBitmap(svg, width, height);
      if (base) {
        ctx.globalCompositeOperation = "source-over";
        ctx.drawImage(base, 0, 0);
      }
      const blendBmp = await layerBitmap(blend, width, height);
      if (blendBmp) {
        ctx.globalCompositeOperation = "difference";
        ctx.drawImage(blendBmp, 0, 0);
        ctx.globalCompositeOperation = "source-over";
      }
      const v0 = performance.now();
      const out = new VideoFrame(ctx.canvas as OffscreenCanvas, {
        timestamp: ts,
        duration: durUs,
      });
      wp.vf += performance.now() - v0;
      wp.n++;
      wpLog();
      const res: BurnResponse = { idx, frame: out, buf: null };
      (self as unknown as Worker).postMessage(res, [out]);
      return;
    }

    // ── Fallback rotation/flip: bitmap → canvas GPU → composite sama ────
    if (bitmap) {
      const ctx = ensureCanvas(width, height);
      if (!ctx) throw new Error("worker 2D context gagal");
      ctx.globalCompositeOperation = "source-over";
      ctx.drawImage(bitmap, 0, 0, width, height);
      bitmap.close();

      const base = await layerBitmap(svg, width, height);
      if (base) {
        ctx.globalCompositeOperation = "source-over";
        ctx.drawImage(base, 0, 0);
      }
      const blendBmp = await layerBitmap(blend, width, height);
      if (blendBmp) {
        ctx.globalCompositeOperation = "difference";
        ctx.drawImage(blendBmp, 0, 0);
        ctx.globalCompositeOperation = "source-over";
      }
      // Mode preview: hasil komposit = ImageBitmap (transfer balik), bukan
      // VideoFrame. Jalur raster+komposit identik dengan export.
      if (preview) {
        const bmp = await createImageBitmap(ctx.canvas);
        const res: BurnResponse = { idx, frame: null, buf: null, bitmap: bmp };
        (self as unknown as Worker).postMessage(res, [bmp]);
        return;
      }
      const out = new VideoFrame(ctx.canvas as OffscreenCanvas, {
        timestamp: ts,
        duration: durUs,
      });
      const res: BurnResponse = { idx, frame: out, buf: null };
      (self as unknown as Worker).postMessage(res, [out]);
      return;
    }

    throw new Error("burn request tanpa videoFrame/bitmap");
  } catch (err) {
    const res: BurnResponse = { idx, buf: null, error: String(err) };
    (self as unknown as Worker).postMessage(res);
  }
};
