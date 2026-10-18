import {
  Input,
  BlobSource,
  ALL_FORMATS,
  Output,
  Mp4OutputFormat,
  WebMOutputFormat,
  BufferTarget,
  CanvasSource,
  VideoSampleSource,
  VideoSample,
  AudioSampleSource,
  VideoSampleSink,
  AudioSampleSink,
  getFirstEncodableVideoCodec,
  getFirstEncodableAudioCodec,
  Quality,
  type VideoCodec,
  type AudioCodec,
  type Rotation,
} from "mediabunny";
import { burnFrameWasm, initSvgrWasm } from "./svgrwasm";

export type ExportFormat = "mp4" | "webm";
export type ExportQuality = "low" | "medium" | "high";
// "keep" preserves the source size; a number is the target height in pixels.
export type ExportResolution = "keep" | 1080 | 720 | 480;

/** Caption frame untuk burn-in: layer dasar + (opsional) layer blend
 * `mix-blend-mode: difference`. Di WASM path berupa string SVG. */
export interface CaptionLayers {
  base: CanvasImageSource;
  blend?: CanvasImageSource | null;
}

/** String SVG caption (WASM path) - diraster & dikomposit di svgr-wasm. */
export interface CaptionSvgLayers {
  base: string;
  blend?: string | null;
}

export interface BurnInOptions {
  videoBlob: Blob;
  sourceWidth: number;
  sourceHeight: number;
  fps: number;
  format: ExportFormat;
  quality: ExportQuality;
  resolution: ExportResolution;
  /** Renders one caption frame as SVG strings (base + optional blend).
   *  Diraster & dikomposit di svgr-wasm - TIDAK di-canvas browser. */
  renderCaption: (frame: number) => Promise<CaptionSvgLayers | null>;
  onProgress?: (fraction: number, stage: string) => void;
  /** A/B paritas: raster caption via canvas browser (jalur lama, pre-svgr-wasm).
   *  Diaktifkan lewat URL ?exportmode=legacy. */
  legacyCanvas?: boolean;
  /** @font-face CSS untuk inline ke SVG (dibutuhkan jalur legacy). */
  fontFaceCss?: string;
}

const QUALITY: Record<ExportQuality, Quality> = {
  low: new Quality("low"),
  medium: new Quality("medium"),
  high: new Quality("high"),
};

/** Replikasi svgToCanvas jalur lama (state pre-447109c): inline @font-face,
 *  data URI  → <img> → canvas berukuran output. */
async function svgToCanvasLegacy(
  svg: string,
  fontFaceCss: string | undefined,
  outWidth: number,
  outHeight: number,
): Promise<HTMLCanvasElement> {
  const inlined = fontFaceCss
    ? svg.replace(/^<svg[^>]*>/, (m) => `${m}<style>${fontFaceCss}</style>`)
    : svg;
  const uri = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(inlined);
  const img = new Image();
  img.src = uri;
  await img.decode();
  const canvas = document.createElement("canvas");
  canvas.width = outWidth;
  canvas.height = outHeight;
  const c2d = canvas.getContext("2d")!;
  c2d.drawImage(img, 0, 0, outWidth, outHeight);
  return canvas;
}

function even(n: number): number {
  return Math.max(2, Math.round(n / 2) * 2);
}

/** Compute the output size for a resolution choice, never upscaling past source. */
export function computeOutputSize(
  sourceWidth: number,
  sourceHeight: number,
  resolution: ExportResolution,
): { width: number; height: number } {
  if (resolution === "keep" || sourceHeight <= 0) {
    return { width: even(sourceWidth), height: even(sourceHeight) };
  }
  // Target height, capped at the source so we never upscale.
  const targetH = Math.min(resolution, sourceHeight);
  const scale = targetH / sourceHeight;
  return { width: even(sourceWidth * scale), height: even(targetH) };
}

/**
 * Burn captions into the video and return an encoded mp4/webm Blob.
 *
 * Decodes each source frame with mediabunny, composites the caption overlay on
 * top, re-encodes through a CanvasSource, and copies the audio track through an
 * AudioSampleSource. Output size follows the chosen resolution.
 */
export async function burnInExport(opts: BurnInOptions): Promise<Blob> {
  const { width, height } = computeOutputSize(
    opts.sourceWidth,
    opts.sourceHeight,
    opts.resolution,
  );
  const progress = opts.onProgress ?? (() => {});

  const input = new Input({ source: new BlobSource(opts.videoBlob), formats: ALL_FORMATS });
  const vTrack = await input.getPrimaryVideoTrack();
  if (!vTrack) throw new Error("no video track found");
  const aTrack = await input.getPrimaryAudioTrack();

  const videoCodecs: VideoCodec[] = opts.format === "webm" ? ["vp9", "vp8"] : ["avc"];
  const vCodec = await getFirstEncodableVideoCodec(videoCodecs, { width, height });
  if (!vCodec) {
    throw new Error(`no encodable ${opts.format.toUpperCase()} video codec on this device`);
  }
  const audioCodecs: AudioCodec[] = opts.format === "webm" ? ["opus"] : ["aac"];
  const aCodec = aTrack ? await getFirstEncodableAudioCodec(audioCodecs) : null;

  const target = new BufferTarget();
  const format =
    opts.format === "webm" ? new WebMOutputFormat() : new Mp4OutputFormat({ fastStart: "in-memory" });
  const output = new Output({ format, target });

  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("could not create 2D canvas context");
  // Jalur normal memakai VideoSampleSource: hasil burn di worker dibungkus
  // VideoFrame (WebCodecs) → VideoSample → encoder tanpa salin pixel di main
  // thread (CanvasSource memaksa draw canvas + konversi per frame).
  const encodingConfig = {
    codec: vCodec,
    quality: QUALITY[opts.quality],
    // Hanya untuk mp4/avc: hardware encoder (Media Foundation/NVENC) bila
    // tersedia; fallback VP9 software tanpa hint agar aman di semua tempat.
    ...(opts.format === "mp4" && vCodec === "avc" ? { hardwareAcceleration: "prefer-hardware" as const } : {}),
  };
  const videoSource = opts.legacyCanvas
    ? new CanvasSource(canvas, encodingConfig)
    : new VideoSampleSource(encodingConfig);
  output.addVideoTrack(videoSource, { frameRate: opts.fps });

  let audioSource: AudioSampleSource | null = null;
  if (aTrack && aCodec) {
    audioSource = new AudioSampleSource({ codec: aCodec, quality: QUALITY.high });
    output.addAudioTrack(audioSource);
  }

  await output.start();

  // ── Worker pool burn (WASM di thread terpisah) ──────────────────────────
  // Sebelumnya burn_frame + readback + blit berjalan serial di main thread
  // (~19ms/frame). Dengan worker: burn frame N overlap dengan decode+read
  // frame N+1; main thread tinggal putImageData + add.
  interface BurnResponse {
    idx: number;
    buf: ArrayBuffer | null;
    frame?: VideoFrame | null;
    error?: string;
  }
  interface WorkerEntry {
    worker: Worker;
    pending: Map<number, (r: BurnResponse) => void>;
  }
  const burnWorkers: WorkerEntry[] = [];
  let workerOk = !opts.legacyCanvas && typeof Worker !== "undefined";
  if (workerOk) {
    const count = Math.min(6, Math.max(1, (navigator.hardwareConcurrency ?? 8) - 4));
    for (let i = 0; i < count; i++) {
      try {
        const worker = new Worker(new URL("./burnWorker.ts", import.meta.url), {
          type: "module",
        });
        const entry: WorkerEntry = { worker, pending: new Map() };
        worker.onmessage = (e: MessageEvent<BurnResponse>) => {
          const cb = entry.pending.get(e.data.idx);
          if (cb) {
            entry.pending.delete(e.data.idx);
            cb(e.data);
          }
        };
        worker.onerror = () => {
          // Gagal → putuskan semua pending dengan error agar drainOne tidak hang.
          for (const [, cb] of entry.pending) cb({ idx: -1, buf: null, error: "worker error" });
          entry.pending.clear();
          workerOk = false;
        };
        burnWorkers.push(entry);
      } catch {
        workerOk = false;
      }
    }
    if (burnWorkers.length === 0) workerOk = false;
    if (!workerOk) console.warn("[export] worker burn tidak tersedia - pakai jalur inline");
  }
  if (!workerOk) {
    // Jalur inline butuh WASM di main thread.
    await initSvgrWasm();
  }
  let burnSeq = 0;
  const burnViaWorker = (
    bitmap: ImageBitmap | null,
    svg: string,
    blend: string,
    t: number,
    dur: number,
  ): Promise<BurnResponse> => {
    const entry = burnWorkers[burnSeq % burnWorkers.length];
    const idx = burnSeq++;
    return new Promise<BurnResponse>((resolve) => {
      entry.pending.set(idx, resolve);
      entry.worker.postMessage(
        {
          idx,
          bitmap,
          svg,
          blend,
          width,
          height,
          t,
          dur,
        },
        [bitmap as unknown as Transferable],
      );
    });
  };

  // ── Video: composite caption over each decoded frame ────────────────────
  const durationSec = (await input.computeDuration()) || 0;
  const vSink = new VideoSampleSink(vTrack);
  // Canvas baca terpisah dari canvas encode supaya draw/read frame N+1 tidak
  // menimpa konten canvas encode saat pipeline menunggu hasil burn N.
  const decodeCanvas = new OffscreenCanvas(width, height);
  const dctx = decodeCanvas.getContext("2d");
  if (!dctx) throw new Error("could not create 2D decode canvas context");

  type Inflight = {
    t: number;
    dur: number;
    rotation: number;
    flip: boolean;
    vidBuf: Uint8Array | null;
    result: Promise<BurnResponse> | null;
  };
  const WINDOW = 8;
  const inflight: Inflight[] = [];

  const drainOne = async () => {
    const f = inflight.shift();
    if (!f) return;
    if (opts.legacyCanvas) {
      // JALUR LAMA: canvas encode diisi hasil burn lalu di-capture per frame.
      let outBuf: ArrayBuffer | null = null;
      if (f.result) {
        const r = await f.result;
        outBuf = r.buf;
        if (!outBuf) console.warn("[export] worker burn gagal, pakai frame asli:", r.error);
      }
      if (!outBuf && f.vidBuf) outBuf = f.vidBuf.buffer as ArrayBuffer;
      if (outBuf) {
        ctx.putImageData(
          new ImageData(new Uint8ClampedArray(outBuf), width, height),
          0,
          0,
        );
      }
      await (videoSource as CanvasSource).add(f.t, f.dur);
    } else {
      // JALUR VideoSampleSource: sample dibangun dari VideoFrame hasil burn
      // (zero-copy) atau buffer fallback.
      let frame: VideoFrame | null = null;
      let outBuf: ArrayBuffer | null = null;
      if (f.result) {
        const r = await f.result;
        frame = r.frame ?? null;
        outBuf = r.buf;
        if (!frame && !outBuf) console.warn("[export] worker burn gagal, pakai frame asli:", r.error);
      }
      if (!frame && !outBuf && f.vidBuf) outBuf = f.vidBuf.buffer as ArrayBuffer;
      if (frame ?? outBuf) {
        let vf = frame;
        if (!vf && outBuf) {
          vf = new VideoFrame(new Uint8Array(outBuf), {
            format: "RGBA",
            codedWidth: width,
            codedHeight: height,
            timestamp: Math.round(f.t * 1e6),
            duration: Math.round(f.dur * 1e6) || undefined,
          });
        }
        const sample = new VideoSample(vf as VideoFrame, {
          rotation: f.rotation as Rotation,
          flip: f.flip,
        });
        await (videoSource as VideoSampleSource).add(sample);
        sample.close();
      }
    }
    if (durationSec > 0) progress(Math.min(1, f.t / durationSec), "rendering video");
  };

  let frameIndex = 0;
  for await (const sample of vSink.samples()) {
    const t = sample.timestamp;
    const dur = sample.duration ?? 1 / opts.fps;
    // draw menggambar penuh (stretch ke w�h) - tak perlu clearRect.
    sample.draw(dctx, 0, 0, width, height);
    const rotation = sample.rotation;
    const flip = sample.flip;
    sample.close();

    const cap = await opts.renderCaption(Math.max(0, Math.round(t * opts.fps)));
    if (cap && opts.legacyCanvas) {
      // JALUR LAMA (pre-svgr-wasm): raster SVG via <img> canvas browser,
      // blend layer dikomposit dengan globalCompositeOperation difference.
      const base = await svgToCanvasLegacy(cap.base, opts.fontFaceCss, width, height);
      ctx.drawImage(base, 0, 0, width, height);
      if (cap.blend) {
        const blend = await svgToCanvasLegacy(cap.blend, opts.fontFaceCss, width, height);
        ctx.save();
        ctx.globalCompositeOperation = "difference";
        ctx.drawImage(blend, 0, 0, width, height);
        ctx.restore();
      }
      await (videoSource as CanvasSource).add(t, dur);
      if (durationSec > 0) progress(Math.min(1, t / durationSec), "rendering video");
      frameIndex++;
      continue;
    }

    let result: Promise<BurnResponse> | null = null;
    if (workerOk) {
      // Snapshot frame → ImageBitmap (snapshot saat pemanggilan), transfer ke
      // worker: seluruh pembacaan pixel + burn + VideoFrame jalan paralel di
      // thread worker, main thread tinggal menggambar + snapshot.
      const bmp = await createImageBitmap(decodeCanvas);
      result = burnViaWorker(bmp, cap ? cap.base : "", cap?.blend ?? "", t, dur);
      inflight.push({ t, dur, rotation, flip, vidBuf: null, result });
    } else {
      // Jalur inline (worker mati): readback + burn di main thread.
      const imgData = dctx.getImageData(0, 0, width, height);
      const vidBuf = new Uint8Array(
        imgData.data.buffer,
        imgData.data.byteOffset,
        imgData.data.byteLength,
      );
      if (cap) {
        const out = await burnFrameWasm(vidBuf, cap.base, cap.blend ?? "", width, height);
        result = Promise.resolve({
          idx: -1,
          buf: out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength) as ArrayBuffer,
        });
      }
      inflight.push({ t, dur, rotation, flip, vidBuf, result });
    }
    if (inflight.length >= WINDOW) await drainOne();
    frameIndex++;
  }
  while (inflight.length > 0) await drainOne();

  // ── Audio: re-encode the source audio through ───────────────────────────
  if (audioSource && aTrack) {
    progress(0.99, "encoding audio");
    const aSink = new AudioSampleSink(aTrack);
    for await (const asample of aSink.samples()) {
      await audioSource.add(asample);
      asample.close();
    }
  }

  progress(1, "finalizing");
  await output.finalize();
  const buffer = target.buffer;
  if (!buffer || buffer.byteLength === 0) throw new Error("export produced no data");
  return new Blob([buffer], {
    type: opts.format === "webm" ? "video/webm" : "video/mp4",
  });
}
