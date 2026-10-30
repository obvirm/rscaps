// Worker GPU-composite: video pixels TIDAK pernah meninggalkan GPU.
//
// Frame video masuk sebagai VideoFrame (transferable, GPU-backed) → digambar
// ke OffscreenCanvas worker (GPU) → layer caption diraster di WASM (svgr-wasm,
// paritas glyph CLI) → digambar ke canvas yang sama via drawImage
// (source-over untuk base, difference untuk blend) → VideoFrame(canvas)
// ditransfer balik ke encoder. Tanpa getImageData / tanpa buffer 8.3MB
// GPU→CPU→GPU per frame.
import { rasterSvgWasm, registerImageWasm, unregisterImageWasm, decodeDataUrlRgba } from "./svgrwasm";

interface BurnRequest {
  idx: number;
  svg: string;
  blend: string;
  width: number;
  height: number;
  sourceWidth?: number;
  sourceHeight?: number;
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

function drawCover(
  ctx: OffscreenCanvasRenderingContext2D,
  bitmap: ImageBitmap,
  width: number,
  height: number,
): void {
  const sourceAspect = bitmap.width / bitmap.height;
  const targetAspect = width / height;
  let sx = 0;
  let sy = 0;
  let sw = bitmap.width;
  let sh = bitmap.height;
  if (sourceAspect > targetAspect) {
    sw = bitmap.height * targetAspect;
    sx = (bitmap.width - sw) / 2;
  } else {
    sh = bitmap.width / targetAspect;
    sy = (bitmap.height - sh) / 2;
  }
  ctx.drawImage(bitmap, sx, sy, sw, sh, 0, 0, width, height);
}

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

// Registry gambar worker-side: kaca selene (kunci jpeg berganti per frame —
// piksel mentah frame diambil dari kanvas komposit; penanda param
// `rscaps-glass`) + gambar statis lain (stroke iris, upload user) — decode
// sekali per kunci, tak pernah di-unregister.
let lastGlassKey = "";
const staticKeys = new Set<string>();

// ── Occlusion pastor: strip dari SVG, komposit GPU per frame ────────────────
// Layer occlusion (mask luminance putih + glass frame) di-raster usvgr = 4
// dari 5 dinding biaya preview (resample mask + luminance + apply + parse
// string besar; svg berubah TIAP frame karena glass embed frame → cache
// layer tak pernah kena). Kontrak host (dipakai preview + export web):
// elemen occlusion DIHAPUS dari string sebelum raster; komposit dilakukan
// di canvas GPU: frame × alpha-masker (destination-in) = mask luminance
// putih (eksak: L(putih)×A = A; Δ≤1/255 pembulatan). String jadi stabil →
// cache layer hidup → usvgr hanya saat konten caption berubah.
interface OcclLayer {
  /** svg tanpa elemen occlusion (kunci cache). */
  svg: string;
  /** rect region (video coords) — null = full frame. */
  rect: [number, number, number, number] | null;
  /** href data URL masker asli (kunci decode). */
  maskHref: string;
  /** dimensi masker (px) untuk pemetaan region→source-rect. */
  maskW: number;
  maskH: number;
}

const OCCL_RE = /<clipPath id="pastor-occl"><rect x="([\d.-]+)" y="([\d.-]+)" width="([\d.-]+)" height="([\d.-]+)"\/><\/clipPath><mask id="pastor-person"[^>]*x="[^"]*"[^>]*>(?:[\s\S]*?)<\/mask><image[^>]*rscaps-glass[^>]*\/>/;
const MASK_HREF_RE = /href="(data:image\/png;rscaps-pmask;base64,[^"]+)"/;
const MASK_DIM_RE = /<image href="data:image\/png;rscaps-pmask;base64,[^"]*" x="0" y="0" width="(\d+)" height="(\d+)"/;

/**
 * Ekstrak layer occlusion pastor dari svg (kalau ada) → {svg tanpa
 * occlusion, rect, masker}. Kompositnya dikerjakan `drawOcclusion` di
 * canvas GPU. Kalau tak ada occlusion → svg kembali apa adanya.
 */
function extractOcclusion(svg: string): OcclLayer | null {
  const m = svg.match(OCCL_RE);
  if (!m) return null;
  const maskHref = svg.match(MASK_HREF_RE)?.[1] ?? "";
  const dims = svg.match(MASK_DIM_RE);
  const rect: [number, number, number, number] = [+m[1], +m[2], +m[3], +m[4]];
  return {
    svg: svg.replace(OCCL_RE, ""),
    rect: rect[2] > 0 && rect[3] > 0 ? rect : null,
    maskHref,
    maskW: dims ? +dims[1] : 0,
    maskH: dims ? +dims[2] : 0,
  };
}

/** Bitmap masker (RGBA, putih + alpha=confidence) per href — cache kecil. */
const maskBitmaps = new Map<string, ImageBitmap>();

async function occlMaskBitmap(maskHref: string): Promise<ImageBitmap | null> {
  if (!maskHref) return null;
  const hit = maskBitmaps.get(maskHref);
  if (hit) return hit;
  const decoded = await decodeDataUrlRgba(maskHref);
  if (!decoded) return null;
  const bmp = await createImageBitmap(
    new ImageData(new Uint8ClampedArray(decoded.rgba), decoded.w, decoded.h),
  );
  if (maskBitmaps.size >= 8) {
    const oldest = maskBitmaps.keys().next().value;
    if (oldest !== undefined && oldest !== maskHref) {
      maskBitmaps.get(oldest)?.close();
      maskBitmaps.delete(oldest);
    }
  }
  maskBitmaps.set(maskHref, bmp);
  return bmp;
}

/** Kanvas temp untuk komposit frame×masker (ukuran region, GPU). */
let occTemp: OffscreenCanvas | null = null;
let occTempCtx: OffscreenCanvasRenderingContext2D | null = null;

/**
 * Komposit occlusion di ATAS layer caption (canvas GPU): piksel frame video
 * ASLI (bukan kanvas ber-teks!) dipotong siluet orang (destination-in
 * masker) lalu digambar ke kanvas utama pada region bbox. Setara mask
 * luminance SVG (masker putih: L×A = A) — Δ≤1/255 pembulatan 8-bit.
 * `src` = sumber piksel video (VideoFrame / ImageBitmap) + dimensi aslinya.
 */
interface OcclSource {
  source: CanvasImageSource;
  sw: number;
  sh: number;
}

async function drawOcclusion(
  occ: OcclLayer,
  ctx: OffscreenCanvasRenderingContext2D,
  width: number,
  height: number,
  src: OcclSource,
  sourceWidth = width,
  sourceHeight = height,
): Promise<void> {
  const mask = await occlMaskBitmap(occ.maskHref);
  if (!mask) return;
  const [x, y, w, h] = occ.rect ?? [0, 0, sourceWidth, sourceHeight];
  const sx = width / sourceWidth;
  const sy = height / sourceHeight;
  const dx = x * sx;
  const dy = y * sy;
  const dw = w * sx;
  const dh = h * sy;
  if (!occTemp || occTemp.width !== dw || occTemp.height !== dh) {
    occTemp = new OffscreenCanvas(Math.max(1, Math.round(dw)), Math.max(1, Math.round(dh)));
    occTempCtx = occTemp.getContext("2d");
  }
  const tctx = occTempCtx;
  if (!tctx) return;
  tctx.globalCompositeOperation = "source-over";
  tctx.clearRect(0, 0, occTemp.width, occTemp.height);
  // Frame video ASLI (area region) — teks di bawah TIDAK ikut: occlusion
  // menimpa teks dengan piksel orang, semantik mask SVG.
  const fx = src.sw > 0 ? src.sw / sourceWidth : 1;
  const fy = src.sh > 0 ? src.sh / sourceHeight : 1;
  tctx.drawImage(
    src.source,
    x * fx, y * fy, w * fx, h * fy,
    0, 0, occTemp.width, occTemp.height,
  );
  // × siluet orang: destination-in memakai alpha sumber (masker putih +
  // alpha=confidence — identik luminance putih). Masker digambar full-frame
  // preserveAspectRatio=none → sub-rect masker = region × skala masker.
  tctx.globalCompositeOperation = "destination-in";
  tctx.drawImage(
    mask,
    (x / sourceWidth) * mask.width, (y / sourceHeight) * mask.height,
    (w / sourceWidth) * mask.width, (h / sourceHeight) * mask.height,
    0, 0, occTemp.width, occTemp.height,
  );
  tctx.globalCompositeOperation = "source-over";
  ctx.drawImage(occTemp, 0, 0, occTemp.width, occTemp.height, dx, dy, dw, dh);
}

async function ensureSvgImages(
  svg: string,
  ctx: OffscreenCanvasRenderingContext2D,
  width: number,
  height: number,
): Promise<void> {
  if (!svg.includes("data:image/")) return;
  // Kaca selene: piksel mentah frame (sudah di kanvas) di bawah kunci
  // data URL jpeg-nya; blur/saturate dikerjakan filter usvgr (region kecil).
  const glass = svg.match(/href="(data:image\/jpeg;rscaps-glass;base64,[^"]+)"/);
  if (glass && glass[1] !== lastGlassKey) {
    if (lastGlassKey) unregisterImageWasm(lastGlassKey);
    const data = ctx.getImageData(0, 0, width, height);
    await registerImageWasm(glass[1], new Uint8Array(data.data.buffer), width, height);
    lastGlassKey = glass[1];
  }
  // Statis (stroke iris, upload user, masker pastor `;rscaps-pmask`):
  // decode sekali, kunci data URL apa pun yang BUKAN kaca.
  for (const m of svg.matchAll(/href="(data:image\/[a-z0-9.+-]+(?:;[a-z0-9-]+)*;base64,[^"]+)"/g)) {
    const href = m[1];
    if (href === lastGlassKey || staticKeys.has(href)) continue;
    const decoded = await decodeDataUrlRgba(href);
    if (decoded) {
      await registerImageWasm(href, decoded.rgba, decoded.w, decoded.h);
      staticKeys.add(href);
    }
  }
}

// Profiler worker (dev): rata-rata ms per komponen, log tiap 600 frame.
const wp = { raster: 0, mkBmp: 0, draw: 0, vf: 0, n: 0, cacheHit: 0, cacheMiss: 0 };

async function layerBitmap(
  svg: string,
  width: number,
  height: number,
  ctx: OffscreenCanvasRenderingContext2D,
  supersample = 1,
): Promise<ImageBitmap | null> {
  if (!svg || svg.trim().length === 0) return null;
  if (!svgHasPaint(svg)) return null; // layer kosong: tak perlu raster
  await ensureSvgImages(svg, ctx, width, height);
  const cacheKey = `${width}x${height}@${supersample}:${svg}`;
  const cached = layerCache.get(cacheKey);
  if (cached) {
    wp.cacheHit++;
    // LRU touch: pindahkan ke akhir.
    layerCache.delete(cacheKey);
    layerCache.set(cacheKey, cached);
    return cached;
  }
  wp.cacheMiss++;
  const r0 = performance.now();
  // raster_svg (Rust) sudah mengembalikan STRAIGHT alpha — tidak perlu
  // unpremultiply lagi di JS (hemat ~5ms per miss).
  const rasterWidth = width * supersample;
  const rasterHeight = height * supersample;
  const straight = await rasterSvgWasm(svg, rasterWidth, rasterHeight);
  const r1 = performance.now();
  // Salin ke canvas milik sendiri (view wasm bisa detached saat memory.grow).
  const rasterCanvas = new OffscreenCanvas(rasterWidth, rasterHeight);
  const rasterCtx = rasterCanvas.getContext("2d");
  if (!rasterCtx) throw new Error("layer raster canvas context gagal");
  rasterCtx.putImageData(
    new ImageData(new Uint8ClampedArray(straight), rasterWidth, rasterHeight),
    0,
    0,
  );
  let bmp: ImageBitmap;
  if (supersample > 1) {
    const downsampleCanvas = new OffscreenCanvas(width, height);
    const downsampleCtx = downsampleCanvas.getContext("2d");
    if (!downsampleCtx) throw new Error("layer downsample context gagal");
    downsampleCtx.imageSmoothingEnabled = true;
    downsampleCtx.imageSmoothingQuality = "high";
    downsampleCtx.drawImage(rasterCanvas, 0, 0, width, height);
    bmp = await createImageBitmap(downsampleCanvas);
  } else {
    bmp = await createImageBitmap(rasterCanvas);
  }
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
  layerCache.set(cacheKey, bmp);
  return bmp;
}

const WP_LOG_EVERY = 24;
function wpLog(): void {
  if (!(wp.n <= 30 || wp.n % WP_LOG_EVERY === 0)) return;
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

self.onmessage = async (e: MessageEvent<BurnRequest & { prefetch?: boolean }>) => {
  const { idx, svg, blend, width, height, sourceWidth, sourceHeight, t, dur, videoFrame, bitmap, buf, preview } = e.data;
  const logicalWidth = sourceWidth || width;
  const logicalHeight = sourceHeight || height;
  // Keep preview supersampling bounded on large 4K+ canvases.
  const previewSupersample = preview && width * height <= 2_500_000 ? 2 : 1;
  try {
    const ts = Math.round(t * 1e6);
    const durUs = Math.round(dur * 1e6) || undefined;

    // ── Prefetch: hangatkan cache layer saja (tanpa komposit/respons) ─────
    if (e.data.prefetch) {
      const ctx = ensureCanvas(width, height);
      if (ctx) await layerBitmap(svg, width, height, ctx);
      return;
    }

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
      const d1 = performance.now();
      wp.draw += d1 - d0;

      // Occlusion pastor: strip dari svg → komposit GPU (string stabil,
      // cache layer hidup). Order: base → occlusion → blend (= order svg).
      // VideoFrame ditutup SETELAH occlusion (butuh piksel asli).
      const occ = extractOcclusion(svg);
      const baseSvg = occ ? occ.svg : svg;

      const base = await layerBitmap(baseSvg, width, height, ctx, previewSupersample);
      if (base) {
        ctx.globalCompositeOperation = "source-over";
        ctx.drawImage(base, 0, 0);
      }
      if (occ) {
        await drawOcclusion(occ, ctx, width, height, {
          source: videoFrame,
          sw: videoFrame.displayWidth || videoFrame.codedWidth || width,
          sh: videoFrame.displayHeight || videoFrame.codedHeight || height,
        }, logicalWidth, logicalHeight);
      }
      videoFrame.close();
      const blendBmp = await layerBitmap(blend, width, height, ctx, previewSupersample);
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
      const occ = extractOcclusion(svg);
      if (preview && !occ) drawCover(ctx, bitmap, width, height);
      else ctx.drawImage(bitmap, 0, 0, width, height);

      // Occlusion pastor: strip + komposit GPU (sama dengan jalur utama).
      // Bitmap ditutup SETELAH occlusion (butuh piksel asli).
      const baseSvg = occ ? occ.svg : svg;

      const base = await layerBitmap(baseSvg, width, height, ctx, previewSupersample);
      if (base) {
        ctx.globalCompositeOperation = "source-over";
        ctx.drawImage(base, 0, 0);
      }
      if (occ) {
        await drawOcclusion(occ, ctx, width, height, {
          source: bitmap,
          sw: bitmap.width,
          sh: bitmap.height,
        }, logicalWidth, logicalHeight);
      }
      bitmap.close();
      const blendBmp = await layerBitmap(blend, width, height, ctx, previewSupersample);
      if (blendBmp) {
        ctx.globalCompositeOperation = "difference";
        ctx.drawImage(blendBmp, 0, 0);
        ctx.globalCompositeOperation = "source-over";
      }
      // Mode preview: hasil komposit = ImageBitmap (transfer balik), bukan
      // VideoFrame. Jalur raster+komposit identik dengan export.
      if (preview) {
        const bmp = await createImageBitmap(ctx.canvas);
        wp.n++;
        wpLog();
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
