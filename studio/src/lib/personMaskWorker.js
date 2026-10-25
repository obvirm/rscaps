/// Worker segmentasi orang (text-behind-actor pastor) — CLASSIC worker
/// (bukan module): bundel MediaPipe CJS dimuat via fetch + Function
/// (importScripts lintas-origin rusak di chrome 156; fetch dari worker
/// normal). File ini JS polos dan di-fetch sebagai TEKS (`?url` → aset
/// verbatim di build produksi — TS akan ter-emit mentah dan mati di
/// worker). URL bundel CJS di-vendor lewat pesan `init` dari main thread
/// (`?url` import — path dev /src/... tak ada di dist), runtime WASM dari
/// CDN jsdelivr, model dari CDN Google (pola tscaps). Input ImageBitmap
/// per sampel, output alpha Uint8 (confidence × 255) ter-downsample ke
/// sisi maksimum 256.
const WASM_BASE =
  "https://cdn.jsdelivr.net/npm/@mediapipe/tasks-vision@0.10.14/wasm";
const MODEL_URL =
  "https://storage.googleapis.com/mediapipe-models/image_segmenter/selfie_segmenter/float16/latest/selfie_segmenter.tflite";
const MASK_MAX_SIDE = 256;

let bundleUrl = "";
let mp = null;
let segmenter = null;
let initPromise = null;
let sourceCanvas = null;
let targetCanvas = null;

function post(msg, transfer) {
  self.postMessage(msg, transfer ?? []);
}

async function loadBundle() {
  if (mp) return mp;
  const src = await fetch(bundleUrl).then((r) => {
    if (!r.ok) throw new Error(`bundel mediapipe: ${r.status}`);
    return r.text();
  });
  const exports = {};
  const moduleShim = { exports };
  new Function("exports", "module", src)(exports, moduleShim);
  mp = moduleShim.exports ?? exports;
  return mp;
}

async function ensureSegmenter() {
  if (segmenter) return;
  if (!initPromise) {
    initPromise = (async () => {
      const api = await loadBundle();
      const vision = await api.FilesetResolver.forVisionTasks(WASM_BASE);
      const make = (delegate) =>
        api.ImageSegmenter.createFromOptions(vision, {
          baseOptions: { modelAssetPath: MODEL_URL, delegate },
          runningMode: "VIDEO",
          outputCategoryMask: false,
          outputConfidenceMasks: true,
        });
      // GPU dulu (cepat); gagal → CPU (WebView/driver lawas).
      try {
        segmenter = await make("GPU");
      } catch {
        segmenter = await make("CPU");
      }
    })();
  }
  await initPromise;
}

function downsample(confidence, width, height, t) {
  const scale = Math.min(1, MASK_MAX_SIDE / Math.max(width, height));
  const tw = Math.max(1, Math.round(width * scale));
  const th = Math.max(1, Math.round(height * scale));
  if (!sourceCanvas) sourceCanvas = new OffscreenCanvas(1, 1);
  if (!targetCanvas) targetCanvas = new OffscreenCanvas(1, 1);
  sourceCanvas.width = width;
  sourceCanvas.height = height;
  targetCanvas.width = tw;
  targetCanvas.height = th;
  const sctx = sourceCanvas.getContext("2d", { willReadFrequently: true });
  const image = sctx.createImageData(width, height);
  for (let i = 0; i < confidence.length; i++) {
    const o = i * 4;
    image.data[o] = 255;
    image.data[o + 1] = 255;
    image.data[o + 2] = 255;
    image.data[o + 3] = Math.min(255, (confidence[i] ?? 0) * 255) | 0;
  }
  sctx.putImageData(image, 0, 0);
  const tctx = targetCanvas.getContext("2d", { willReadFrequently: true });
  tctx.clearRect(0, 0, tw, th);
  tctx.drawImage(sourceCanvas, 0, 0, tw, th);
  const out = tctx.getImageData(0, 0, tw, th);
  const alpha = new Uint8Array(tw * th);
  for (let i = 0; i < alpha.length; i++) alpha[i] = out.data[i * 4 + 3];
  return { alpha, width: tw, height: th, t };
}

self.onmessage = async (event) => {
  const msg = event.data;
  if (msg.type === "init") {
    // FIFO: init selalu diproses sebelum segment pertama.
    bundleUrl = msg.bundleUrl;
    try {
      await ensureSegmenter();
      post({ type: "ready" });
    } catch (err) {
      post({ type: "error", id: -1, message: String(err) });
    }
    return;
  }
  if (msg.type === "segment" && msg.bitmap) {
    try {
      await ensureSegmenter();
      const result = segmenter.segmentForVideo(msg.bitmap, msg.timestampMs);
      const mask = result.confidenceMasks?.[0];
      if (!mask) {
        msg.bitmap.close();
        post({ type: "error", id: msg.id, message: "no confidence mask" });
        return;
      }
      const confidence = mask.getAsFloat32Array();
      const sw = mask.width;
      const sh = mask.height;
      mask.close();
      msg.bitmap.close();
      const out = downsample(confidence, sw, sh, msg.t);
      post({ type: "mask", id: msg.id, ...out }, [out.alpha.buffer]);
    } catch (err) {
      msg.bitmap.close();
      post({ type: "error", id: msg.id, message: String(err) });
    }
  }
};
