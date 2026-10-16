import {
  Input,
  BlobSource,
  ALL_FORMATS,
  Output,
  Mp4OutputFormat,
  WebMOutputFormat,
  BufferTarget,
  CanvasSource,
  AudioSampleSource,
  VideoSampleSink,
  AudioSampleSink,
  getFirstEncodableVideoCodec,
  getFirstEncodableAudioCodec,
  Quality,
  type VideoCodec,
  type AudioCodec,
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
}

const QUALITY: Record<ExportQuality, Quality> = {
  low: new Quality("low"),
  medium: new Quality("medium"),
  high: new Quality("high"),
};

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
  const canvasSource = new CanvasSource(canvas, { codec: vCodec, quality: QUALITY[opts.quality] });
  output.addVideoTrack(canvasSource, { frameRate: opts.fps });

  let audioSource: AudioSampleSource | null = null;
  if (aTrack && aCodec) {
    audioSource = new AudioSampleSource({ codec: aCodec, quality: QUALITY.high });
    output.addAudioTrack(audioSource);
  }

  await output.start();

  // Init svgr-wasm + load font (sekali sebelum loop) - raster di WASM.
  await initSvgrWasm();

  // ── Video: composite caption over each decoded frame ────────────────────
  const durationSec = (await input.computeDuration()) || 0;
  const vSink = new VideoSampleSink(vTrack);
  let frameIndex = 0;
  for await (const sample of vSink.samples()) {
    const t = sample.timestamp;
    ctx.clearRect(0, 0, width, height);
    sample.draw(ctx, 0, 0, width, height);

    const cap = await opts.renderCaption(Math.max(0, Math.round(t * opts.fps)));
    if (cap) {
      // Baca piksel video dari canvas (decode WebCodecs → RGBA).
      const imgData = ctx.getImageData(0, 0, width, height);
      // Raster + komposit caption DI WASM (svgr-wasm) - bukan canvas browser.
      const out = await burnFrameWasm(
        new Uint8Array(imgData.data.buffer, imgData.data.byteOffset, imgData.data.byteLength),
        cap.base,
        cap.blend ?? "",
        width,
        height,
      );
      // Tulis hasil komposit kembali (putImageData = blit murah, bukan raster).
      ctx.putImageData(new ImageData(new Uint8ClampedArray(out.buffer, out.byteOffset, out.byteLength), width, height), 0, 0);
    }
    await canvasSource.add(t, sample.duration ?? 1 / opts.fps);
    sample.close();
    frameIndex++;
    if (durationSec > 0) progress(Math.min(1, t / durationSec), "rendering video");
  }

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
