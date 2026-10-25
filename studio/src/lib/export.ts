import {
  Input,
  BlobSource,
  ALL_FORMATS,
  Output,
  Mp4OutputFormat,
  WebMOutputFormat,
  BufferTarget,
  StreamTarget,
  type StreamTargetChunk,
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
  type AudioSample,
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
  /** Layer kaca (selene) & cutout pastor: bila wanted(t) untuk frame itu,
   * frame di-gambar ke kanvas grab lalu JPEG-nya disuplai SEBELUM
   * renderCaption. wanted dipanggil per frame (t detik) — pastor butuh
   * hanya pada rentang behind. */
  frameImage?: {
    wanted: boolean | ((t: number) => boolean);
    supply: (jpeg: Uint8Array) => void;
  };
  /** Masker orang behind-actor (pastor): bila wanted(t), pemanggil suplai
   * PNG masker untuk t SEBELUM renderCaption (dipasangkan dengan frame). */
  personMask?: {
    wanted: (t: number) => boolean;
    supply: (t: number) => void;
  };
  /** Streaming: tulis mux langsung ke WritableStream (mis.
   * FileSystemWritableFileStream dari showSaveFilePicker) — file besar
   * tidak ditahan di RAM 2× (BufferTarget + fastStart "in-memory").
   * Bila diisi, nilai kembalian adalah Blob KOSONG (sentinel) — file
   * sudah ditulis ke tujuan; pemanggil tak perlu downloadBlob. */
  streamTo?: WritableStream<StreamTargetChunk> | null;
}

const QUALITY: Record<ExportQuality, Quality> = {
  low: new Quality("low"),
  medium: new Quality("medium"),
  high: new Quality("high"),
};

/** Bungkus `FileSystemWritableFileStream` (File System Access API) menjadi
 * `WritableStream<StreamTargetChunk>` untuk `StreamTarget`. mediabunny
 * mengunci (getWriter) stream yang diberikan dan menutupnya saat
 * `output.finalize()` — wrapper menjaga FSA writable TETAP tidak terkunci:
 * sukses → writer.close() → close() wrapper → file persist; gagal →
 * `abort()` bisa dipanggil langsung ke FSA (partial dibuang, lock lepas). */
export function wrapFileSystemWritable(fw: {
  write: (p: unknown) => Promise<void>;
  close: () => Promise<void>;
  abort?: (r?: unknown) => Promise<void>;
}): WritableStream<StreamTargetChunk> {
  return new WritableStream<StreamTargetChunk>({
    write: (chunk) => fw.write(chunk),
    close: () => fw.close(),
    abort: (reason) => fw.abort?.(reason) ?? Promise.resolve(),
  });
}

// Bitrate video eksplisit (menggantikan preset QUALITY mediabunny yang
// terlalu pelit: high = 6.1 Mbps @1080p). Tabel dasar mengikuti rekomendasi
// upload YouTube + headroom caption burn-in, lalu dinaikkan DI ATAS tabel
// tscaps (6/12/18/40) — kualitas high kita sengaja lebih mahal dari referensi.
const VIDEO_BASELINE_BITRATE: Array<{ px: number; bps: number }> = [
  { px: 1280 * 720, bps: 7_000_000 },
  { px: 1920 * 1080, bps: 14_000_000 },
  { px: 2560 * 1440, bps: 21_000_000 },
  { px: Number.POSITIVE_INFINITY, bps: 46_000_000 },
];
// Faktor preset kualitas (mirip tscaps: low 0.5, medium 0.75, high 1.0).
const BITRATE_QUALITY_FACTOR: Record<ExportQuality, number> = { low: 0.5, medium: 0.75, high: 1 };
function videoBitrate(width: number, height: number, fps: number, quality: ExportQuality): number {
  const px = width * height;
  const base = VIDEO_BASELINE_BITRATE.find((t) => px <= t.px)!.bps;
  // >30fps: skala ~1.5x di 60fps (rasio tabel YouTube).
  const fpsFactor = Number.isFinite(fps) && fps > 30 ? 1 + (fps - 30) / 60 : 1;
  return Math.round((base * BITRATE_QUALITY_FACTOR[quality] * fpsFactor) / 1000) * 1000;
}

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

  // mp4: rantai fallback codec — avc dulu (tercepat untuk konten nyata di
  // pengukuran run61-69: avc 4.7ms/frame vs hevc 5.25 vs av1 8.8; kompatibilitas
  // pemutar terluas), hevc & av1 menyusul bila avc tak tersedia di perangkat.
  // webm: vp9/vp8 seperti sebelumnya.
  const videoCodecs: VideoCodec[] =
    opts.format === "webm" ? ["vp9", "vp8"] : ["avc", "hevc", "av1"];
  const vCodec = await getFirstEncodableVideoCodec(videoCodecs, { width, height });
  if (!vCodec) {
    throw new Error(`no encodable ${opts.format.toUpperCase()} video codec on this device`);
  }
  const audioCodecs: AudioCodec[] = opts.format === "webm" ? ["opus"] : ["aac"];
  const aCodec = aTrack ? await getFirstEncodableAudioCodec(audioCodecs) : null;

  const streaming = opts.streamTo != null;
  // Streaming: mux menulis langsung ke WritableStream (backpressure
  // mengalir ke encoder) — RAM konstan. Fallback: BufferTarget (RAM ≈ 2×
  // ukuran file: buffer + penyusunan moov "in-memory").
  const bufTarget = streaming ? null : new BufferTarget();
  const target = bufTarget ?? new StreamTarget(opts.streamTo!, { chunked: true });
  const format =
    opts.format === "webm"
      ? new WebMOutputFormat()
      : new Mp4OutputFormat({ fastStart: streaming ? false : "in-memory" });
  const output = new Output({ format, target });

  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("could not create 2D canvas context");
  // Jalur normal memakai VideoSampleSource: hasil burn di worker dibungkus
  // VideoFrame (WebCodecs) → VideoSample → encoder tanpa salin pixel di main
  // thread (CanvasSource memaksa draw canvas + konversi per frame).
  const encodingConfig = {
    codec: vCodec,
    bitrate: videoBitrate(width, height, opts.fps, opts.quality),
    bitrateMode: "variable" as const,
    // Caption teks kontras-tinggi → encoder diprioritaskan ke tepi teks
    // (hint WebCodecs; browser yang tak mengenalinya mengabaikan).
    contentHint: "text",
    // Hanya untuk mp4: hardware encoder bila tersedia (avc/hevc/av1 semuanya
    // hw-capable di GPU modern; probe tmp_codec_probe.mjs). webm (vp9/vp8)
    // tanpa hint agar aman software di semua tempat.
    // latencyMode 'realtime' diuji (run57): lebih lambat (38.6s vs 36.3s) —
    // jangan dipakai.
    ...(opts.format === "mp4" ? { hardwareAcceleration: "prefer-hardware" as const } : {}),
    // Emit AVCC (bukan Annex B default Chrome): tanpa ini mediabunny
    // menulis-ulang NAL unit setiap paket di jalur mux (run59: wall mentok
    // ~4.8ms/frame padahal encoder mentah 270fps). onEncoderConfig dipanggil
    // sebelum isConfigSupported → mutasi ikut tervalidasi.
    onEncoderConfig: (cfg: VideoEncoderConfig) => {
      if (opts.format === "mp4" && vCodec === "avc") cfg.avc = { format: "avc" };
    },
    // Diagnostik dev: pace kedatangan paket encoder (throughput output).
    onEncodedPacket: () => {
      const n = performance.now();
      const last = encPackets[encPackets.length - 1];
      if (last !== undefined) {
        encDeltaSum += n - last;
        encDeltaCount++;
      }
      encPackets.push(n);
    },
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
        worker.onerror = (ev: ErrorEvent) => {
          // Gagal → putuskan semua pending dengan error agar drainOne tidak hang.
          console.warn("[export] worker onerror:", ev.message || "(no message)", ev.filename, ev.lineno);
          for (const [, cb] of entry.pending) cb({ idx: -1, buf: null, error: ev.message || "worker error" });
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
    frame: VideoFrame | null,
    bmp: ImageBitmap | null,
    svg: string,
    blend: string,
    t: number,
    dur: number,
  ): Promise<BurnResponse> => {
    const entry = burnWorkers[burnSeq % burnWorkers.length];
    const idx = burnSeq++;
    const transfer: Transferable[] = [];
    if (frame) transfer.push(frame);
    if (bmp) transfer.push(bmp);
    return new Promise<BurnResponse>((resolve) => {
      entry.pending.set(idx, resolve);
      entry.worker.postMessage(
        {
          idx,
          videoFrame: frame,
          bitmap: bmp,
          svg,
          blend,
          width,
          height,
          t,
          dur,
        },
        transfer,
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
  // Kanvas grab terpisah untuk layer kaca (JPEG per frame) — tak mengganggu
  // kanvas decode/encode.
  const grabCanvas = new OffscreenCanvas(width, height);
  const grabCtx = grabCanvas.getContext("2d", { willReadFrequently: true });
  if (!grabCtx) throw new Error("could not create 2d grab canvas context");

  type Inflight = {
    t: number;
    dur: number;
    rotation: number;
    flip: boolean;
    vidBuf: Uint8Array | null;
    result: Promise<BurnResponse> | null;
  };
  // Window 8 = batas aman: 16 dead-lock pool VideoFrame Chrome (run46), dan
  // 10 JUGA dead-lock (run56, stuck + GPU spin 77%) → plafon keras pool ≈8.
  // Jangan naikkan tanpa mengurangi live frame di jalur lain.
  const WINDOW = 8;
  const inflight: Inflight[] = [];
  // Profiler drain: tunggu hasil worker vs backpressure encoder.
  const dProf = { wait: 0, add: 0 };
  // Antrean submit encoder: add() TIDAK di-await per frame (lockstep dengan
  // encoder membuat main jadi pacer ~5.5ms/frame padahal encoder cuma ~75%
  // utilized). Submit beruntun → backpressure diambil tiap ENC_FLUSH_AT
  // promise; urutan add() tetap terjaga (panggilan sinkron berurutan).
  // ANGGARAN LIVE VideoFrame Chrome ≈ ≤16 (window8 + encQ + encoder queue);
  // encQ=8 deadlock tanpa error (stuck + GPU spin), encQ=3 (pre-svgHasPaint)
  // kehabisan GPU texture. encQ=1 dan encQ=2 setara (36.3s vs 36.6s, run54/55)
  // → pakai 1 (paling aman, maks 2 submit outstanding).
  const encQueue: Promise<void>[] = [];
  const ENC_FLUSH_AT = 1;
  // Diagnostik dev: jumlah paket & selisih antar-paket encoder.
  const encPackets: number[] = [];
  let encDeltaSum = 0;
  let encDeltaCount = 0;

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
        const w0 = performance.now();
        const r = await f.result;
        dProf.wait += performance.now() - w0;
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
        const a0 = performance.now();
        const p = (videoSource as VideoSampleSource)
          .add(sample)
          .then(() => {
            sample.close();
          });
        encQueue.push(p);
        if (encQueue.length >= ENC_FLUSH_AT) {
          const w0 = performance.now();
          await encQueue.shift();
          dProf.add += performance.now() - w0;
        }
        void a0;
      }
    }
    if (durationSec > 0) progress(Math.min(1, f.t / durationSec), "rendering video");
    return f;
  };

  // ── Audio interleave (pola pumpUntil): sampel audio di-encode SELAMA
  // drain video berjalan — tersembunyi di dalam tunggu encoder — bukan
  // fase terpisah setelah video selesai. Sampel timestamp negatif
  // (offset container) dibuang; muxer menolaknya.
  let aIter: AsyncIterator<AudioSample> | null = null;
  let aNext: AudioSample | null = null;
  if (audioSource && aTrack) {
    aIter = new AudioSampleSink(aTrack).samples()[Symbol.asyncIterator]();
    const r0 = await aIter.next();
    aNext = r0.done || !r0.value ? null : r0.value;
  }
  const pumpAudio = async (untilSec: number) => {
    while (aNext && aNext.timestamp <= untilSec) {
      const s = aNext;
      await advanceAudio();
      try {
        if (s.timestamp >= 0 && audioSource) await audioSource.add(s);
      } finally {
        s.close();
      }
    }
  };
  async function advanceAudio() {
    if (!aIter) return;
    const r = await aIter.next();
    aNext = r.done || !r.value ? null : r.value;
  }

  let frameIndex = 0;
  // Profiler sementara (dev): akumulasi waktu per bagian. decode/caption
  // diukur di producer, drain di consumer — keduanya kini berjalan paralel,
  // jadi total TIDAK lagi sama dengan wall time.
  const prof = { decode: 0, caption: 0, push: 0, drain: 0, other: 0 };
  let profLast = performance.now();
  // Pipelining producer→consumer: sebelumnya loop decode meng-await drainOne
  // (worker+encoder) di dalam badan iterasi → decode N+1 tak boleh mulai
  // sebelum encoder N selesai → semua tahap ter-serialisasi di main thread
  // (run54-58: total ≈ 4.8ms/frame = JUMLAH tunggu semua tahap). Kini
  // decode+caption berjalan paralel dengan drain; producer berhenti lewat
  // kondisi inflight >= WINDOW sehingga budget live VideoFrame tetap sama
  // (window hasil worker + encQ + antrean internal encoder ≈ ≤16, plafon
  // Chrome — window >8 terbukti dead-lock, run46/run56).
  let producerDone = false;
  let producerError: unknown = null;
  let pipelineError: unknown = null;
  let slotWaiter: (() => void) | null = null;
  let itemWaiter: (() => void) | null = null;
  const waitSlot = () => new Promise<void>((r) => (slotWaiter = r));
  const waitItem = () => new Promise<void>((r) => (itemWaiter = r));
  const notifySlot = () => {
    const w = slotWaiter;
    slotWaiter = null;
    w?.();
  };
  const notifyItem = () => {
    const w = itemWaiter;
    itemWaiter = null;
    w?.();
  };

  const producer = (async () => {
    try {
      for await (const sample of vSink.samples()) {
        const nowT = performance.now();
        prof.decode += nowT - profLast;
        const t = sample.timestamp;
        const dur = sample.duration ?? 1 / opts.fps;
        const rotation = sample.rotation;
        const flip = sample.flip;

        const frameImageWanted =
          typeof opts.frameImage?.wanted === "function"
            ? opts.frameImage.wanted(t)
            : (opts.frameImage?.wanted ?? false);
        if (opts.frameImage && frameImageWanted) {
          sample.draw(grabCtx, 0, 0, width, height);
          const grabBlob = await grabCanvas.convertToBlob({ type: "image/jpeg", quality: 0.75 });
          opts.frameImage.supply(new Uint8Array(await grabBlob.arrayBuffer()));
        }
        if (opts.personMask && opts.personMask.wanted(t)) {
          // Pemanggil memegang cache PNG per sampel — suplai untuk t ini.
          opts.personMask.supply(t);
        }

        const capStart = performance.now();
        const cap = await opts.renderCaption(Math.max(0, Math.round(t * opts.fps)));
        prof.caption += performance.now() - capStart;
        if (cap && opts.legacyCanvas) {
          // JALUR LAMA (pre-svgr-wasm): raster SVG via <img> canvas browser,
          // blend layer dikomposit dengan globalCompositeOperation difference.
          sample.draw(ctx, 0, 0, width, height);
          sample.close();
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
          profLast = performance.now();
          continue;
        }

        let result: Promise<BurnResponse> | null = null;
        if (workerOk && rotation === 0 && !flip) {
          // Jalur GPU: VideoFrame (GPU-backed) transfer ke worker, digambar di
          // canvas worker + layer caption di-composite di GPU. Piksel video tidak
          // pernah readback ke CPU.
          const vf = sample.toVideoFrame();
          sample.close();
          result = burnViaWorker(vf, null, cap ? cap.base : "", cap?.blend ?? "", t, dur);
          inflight.push({ t, dur, rotation, flip, vidBuf: null, result });
        } else if (workerOk) {
          // Fallback rotation/flip: draw dulu (rotasi diterapkan) → bitmap → worker.
          sample.draw(dctx, 0, 0, width, height);
          sample.close();
          const bmp = await createImageBitmap(decodeCanvas);
          result = burnViaWorker(null, bmp, cap ? cap.base : "", cap?.blend ?? "", t, dur);
          inflight.push({ t, dur, rotation, flip, vidBuf: null, result });
        } else {
          // Jalur inline (worker mati): readback + burn di main thread.
          sample.draw(dctx, 0, 0, width, height);
          sample.close();
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
        notifyItem();
        if (inflight.length >= WINDOW) {
          // Backpressure: tunggu consumer mengosongkan slot. Race satu-siklus
          // (notif datang sebelum waiter terdaftar) aman — drain berikutnya
          // menotifikasi lagi.
          while (inflight.length >= WINDOW && !pipelineError) await waitSlot();
          if (pipelineError) return;
        }
        frameIndex++;
        profLast = performance.now();
      }
    } catch (e) {
      producerError = e;
    } finally {
      producerDone = true;
      notifyItem();
    }
  })();

  const consumer = (async () => {
    try {
      while (true) {
        if (inflight.length > 0) {
          const d0 = performance.now();
          const f = await drainOne();
          prof.drain += performance.now() - d0;
          notifySlot();
          // Audio di-encode di sela tunggu encoder (interleave).
          if (f) await pumpAudio(f.t);
        } else if (!producerDone) {
          await waitItem();
        } else {
          break;
        }
      }
      // Sisa audio setelah frame video terakhir + flush antrean encoder:
      // semua submit harus settle sebelum finalize.
      await pumpAudio(Number.POSITIVE_INFINITY);
      const f0 = performance.now();
      while (encQueue.length > 0) await encQueue.shift();
      dProf.add += performance.now() - f0;
    } catch (e) {
      pipelineError = e;
      notifySlot(); // lepaskan producer bila tertahan backpressure
      throw e;
    }
  })();

  await producer;
  try {
    await consumer;
  } finally {
    notifySlot();
  }
  if (producerError) throw producerError;
  if (import.meta.env.DEV) {
    const n = Math.max(1, frameIndex);
    const avgDelta = encDeltaCount > 0 ? encDeltaSum / encDeltaCount : 0;
    const tailSum = encPackets.length > 1000 ? encPackets[encPackets.length - 1] - encPackets[encPackets.length - 1001] : 0;
    const tailAvg = tailSum > 0 ? tailSum / 1000 : 0;
    console.log(
      `[exportprof] frames=${frameIndex} decode=${(prof.decode / n).toFixed(2)}ms caption=${(prof.caption / n).toFixed(2)}ms drain=${(prof.drain / n).toFixed(2)}ms (waitWorker=${(dProf.wait / n).toFixed(2)}ms encAdd=${(dProf.add / n).toFixed(2)}ms) total=${((prof.decode + prof.caption + prof.drain) / n).toFixed(2)}ms encOut=${avgDelta.toFixed(2)}ms encOutTail1000=${tailAvg.toFixed(2)}ms packets=${encPackets.length}`,
    );
  }

  progress(1, "finalizing");
  console.log("[exportstage] before finalize");
  await output.finalize();
  console.log("[exportstage] after finalize");
  if (!bufTarget) {
    // Jalur streaming: data sudah tertulis ke WritableStream selama encode.
    return new Blob([], {
      type: opts.format === "webm" ? "video/webm" : "video/mp4",
    });
  }
  const buffer = bufTarget.buffer;
  if (!buffer || buffer.byteLength === 0) throw new Error("export produced no data");
  console.log("[exportstage] blob build", buffer.byteLength);
  return new Blob([buffer], {
    type: opts.format === "webm" ? "video/webm" : "video/mp4",
  });
}
