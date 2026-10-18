// Worker burn_frame: komposit caption DI WASM pada thread terpisah sehingga
// burn frame N overlap dengan decode frame N+1 di main thread.
// Main menggambar frame hasil decode ke ImageBitmap (transferable) — seluruh
// pembacaan pixel (drawImage → getImageData), burn WASM, dan pembungkusan
// VideoFrame (WebCodecs) berjalan di sini, paralel antar worker; VideoFrame
// hasilnya ditransfer zero-copy ke main thread.
import { burnFrameWasm } from "./svgrwasm";

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
  bitmap: ImageBitmap | null;
  /** RGBA byte buffer — jalur inline-ish (bitmap transfer tidak tersedia). */
  buf?: ArrayBuffer;
}

interface BurnResponse {
  idx: number;
  buf: ArrayBuffer | null;
  frame?: VideoFrame | null;
  error?: string;
}

// Kanvas per-worker (dibuat sekali, dipakai ulang antar frame).
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

self.onmessage = async (e: MessageEvent<BurnRequest>) => {
  const { idx, svg, blend, width, height, t, dur, bitmap, buf } = e.data;
  try {
    let pixels: Uint8Array | null = null;
    if (bitmap) {
      const ctx = ensureCanvas(width, height);
      if (!ctx) throw new Error("worker 2D context gagal");
      ctx.drawImage(bitmap, 0, 0, width, height);
      bitmap.close();
      pixels = new Uint8Array(ctx.getImageData(0, 0, width, height).data);
    } else if (buf) {
      pixels = new Uint8Array(buf);
    } else {
      throw new Error("burn request tanpa bitmap/buf");
    }
    const out = await burnFrameWasm(pixels, svg, blend, width, height);
    try {
      const frame = new VideoFrame(out, {
        format: "RGBA",
        codedWidth: width,
        codedHeight: height,
        timestamp: Math.round(t * 1e6),
        duration: Math.round(dur * 1e6) || undefined,
      });
      const res: BurnResponse = { idx, buf: null, frame };
      (self as unknown as Worker).postMessage(res, [frame]);
      return;
    } catch {
      // WebCodecs tak tersedia di worker ini → kirim mentah (main menyalin).
      const outBuf = out.slice().buffer;
      const res: BurnResponse = { idx, buf: outBuf };
      (self as unknown as Worker).postMessage(res, [outBuf]);
    }
  } catch (err) {
    const res: BurnResponse = { idx, buf: null, error: String(err) };
    (self as unknown as Worker).postMessage(res);
  }
};
