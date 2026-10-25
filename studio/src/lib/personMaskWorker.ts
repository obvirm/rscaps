/// Worker segmentasi orang (text-behind-actor pastor) — CLASSIC worker
/// (bukan module): bundel MediaPipe CJS dimuat via fetch + Function
/// (importScripts lintas-origin rusak di chrome 156; fetch dari worker
/// normal). Bundel di-vendor same-origin (studio/src/lib/vendor/),
/// runtime WASM dari CDN jsdelivr, model dari CDN Google (pola tscaps).
/// Input ImageBitmap per sampel, output alpha Uint8 (confidence × 255)
/// ter-downsample ke sisi maksimum 256.
const BUNDLE_URL =
  self.location.origin + "/src/lib/vendor/mediapipe_vision_bundle.cjs";
const WASM_BASE =
  "https://cdn.jsdelivr.net/npm/@mediapipe/tasks-vision@0.10.14/wasm";
const MODEL_URL =
  "https://storage.googleapis.com/mediapipe-models/image_segmenter/selfie_segmenter/float16/latest/selfie_segmenter.tflite";
const MASK_MAX_SIDE = 256;

interface SegmenterLike {
  segmentForVideo(
    image: ImageBitmap | HTMLVideoElement | HTMLCanvasElement,
    timestampMs: number,
  ): {
    confidenceMasks?: Array<{
      getAsFloat32Array(): Float32Array;
      width: number;
      height: number;
      close(): void;
    }>;
  };
}

interface MpApi {
  FilesetResolver: {
    forVisionTasks: (base: string) => Promise<unknown>;
  };
  ImageSegmenter: {
    createFromOptions: (
      vision: unknown,
      opts: Record<string, unknown>,
    ) => Promise<unknown>;
  };
}

let mp: MpApi | null = null;
let segmenter: SegmenterLike | null = null;
let initPromise: Promise<void> | null = null;
let sourceCanvas: OffscreenCanvas | null = null;
let targetCanvas: OffscreenCanvas | null = null;

async function loadBundle(): Promise<MpApi> {
  if (mp) return mp;
  const src = await fetch(BUNDLE_URL).then((r) => {
    if (!r.ok) throw new Error(`bundel mediapipe: ${r.status}`);
    return r.text();
  });
  const exports = {} as Record<string, unknown>;
  const moduleShim = { exports };
  new Function("exports", "module", src)(exports, moduleShim);
  mp = (moduleShim.exports ?? exports) as unknown as MpApi;
  return mp;
}

async function ensureSegmenter(): Promise<void> {
  if (segmenter) return;
  if (!initPromise) {
    initPromise = (async () => {
      const api = await loadBundle();
      const vision = await api.FilesetResolver.forVisionTasks(WASM_BASE);
      const make = (delegate: string) =>
        api.ImageSegmenter.createFromOptions(vision, {
          baseOptions: { modelAssetPath: MODEL_URL, delegate },
          runningMode: "VIDEO",
          outputCategoryMask: false,
          outputConfidenceMasks: true,
        });
      // GPU dulu (cepat); gagal → CPU (WebView/driver lawas).
      try {
        segmenter = (await make("GPU")) as SegmenterLike;
      } catch {
        segmenter = (await make("CPU")) as SegmenterLike;
      }
    })();
  }
  await initPromise;
}

function downsample(
  confidence: Float32Array,
  width: number,
  height: number,
  t: number,
): { alpha: Uint8Array; width: number; height: number; t: number } {
  const scale = Math.min(1, MASK_MAX_SIDE / Math.max(width, height));
  const tw = Math.max(1, Math.round(width * scale));
  const th = Math.max(1, Math.round(height * scale));
  if (!sourceCanvas) sourceCanvas = new OffscreenCanvas(1, 1);
  if (!targetCanvas) targetCanvas = new OffscreenCanvas(1, 1);
  sourceCanvas.width = width;
  sourceCanvas.height = height;
  targetCanvas.width = tw;
  targetCanvas.height = th;
  const sctx = sourceCanvas.getContext("2d", { willReadFrequently: true })!;
  const image = sctx.createImageData(width, height);
  for (let i = 0; i < confidence.length; i++) {
    const o = i * 4;
    image.data[o] = 255;
    image.data[o + 1] = 255;
    image.data[o + 2] = 255;
    image.data[o + 3] = Math.min(255, (confidence[i] ?? 0) * 255) | 0;
  }
  sctx.putImageData(image, 0, 0);
  const tctx = targetCanvas.getContext("2d", { willReadFrequently: true })!;
  tctx.clearRect(0, 0, tw, th);
  tctx.drawImage(sourceCanvas, 0, 0, tw, th);
  const out = tctx.getImageData(0, 0, tw, th);
  const alpha = new Uint8Array(tw * th);
  for (let i = 0; i < alpha.length; i++) alpha[i] = out.data[i * 4 + 3]!;
  return { alpha, width: tw, height: th, t };
}

self.onmessage = async (event: MessageEvent) => {
  const msg = event.data as {
    type: string;
    id: number;
    bitmap?: ImageBitmap;
    timestampMs?: number;
    t?: number;
  };
  if (msg.type === "init") {
    try {
      await ensureSegmenter();
      (self as unknown as Worker).postMessage({ type: "ready" });
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "error",
        id: -1,
        message: String(err),
      });
    }
    return;
  }
  if (msg.type === "segment" && msg.bitmap) {
    try {
      await ensureSegmenter();
      const result = segmenter!.segmentForVideo(msg.bitmap, msg.timestampMs!);
      const mask = result.confidenceMasks?.[0];
      if (!mask) {
        msg.bitmap.close();
        (self as unknown as Worker).postMessage({
          type: "error",
          id: msg.id,
          message: "no confidence mask",
        });
        return;
      }
      const confidence = mask.getAsFloat32Array();
      const sw = mask.width;
      const sh = mask.height;
      mask.close();
      msg.bitmap.close();
      const out = downsample(confidence, sw, sh, msg.t!);
      (self as unknown as Worker).postMessage(
        { type: "mask", id: msg.id, ...out },
        [out.alpha.buffer],
      );
    } catch (err) {
      msg.bitmap.close();
      (self as unknown as Worker).postMessage({
        type: "error",
        id: msg.id,
        message: String(err),
      });
    }
  }
};
