import { decodeAudioTo16kMono } from "../audio";
import { langToTokenId, type WhisperModel, type WhisperQuant } from "../types";
import type { ProgressFn, TranscribeResult, TranscribeSegment } from "../transcribe";

export type HybridWord = {
  text: string;
  start_ms: number;
  end_ms: number;
};

export type HybridResult = TranscribeResult & {
  words: HybridWord[];
  timings?: Record<string, number>;
};

type WorkerReply =
  | { type: "boot" }
  | { type: "progress"; phase: string; fraction: number }
  | {
      type: "done";
      result: {
        text: string;
        segments: TranscribeSegment[];
        words: { text: string; start_s: number; end_s: number }[];
        timings?: Record<string, number>;
      };
    }
  | { type: "error"; error: string };

type HybridRequest = {
  samples: Float32Array;
  model: WhisperModel;
  quant: WhisperQuant;
  langId: number;
  duration: number;
  remoteModels: boolean;
};

let worker: Worker | null = null;
let queue: Promise<unknown> = Promise.resolve();
let active: {
  resolve: (reply: Extract<WorkerReply, { type: "done" }>) => void;
  reject: (err: Error) => void;
  onProgress: ProgressFn;
} | null = null;

function resetWorker(): void {
  worker?.terminate();
  worker = null;
  active = null;
}

function ensureWorker(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./hybrid.worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (ev: MessageEvent<WorkerReply>) => {
    const msg = ev.data;
    if (!active) return;
    if (msg.type === "progress") {
      active.onProgress(0.05 + msg.fraction * 0.9, "transcribe");
    } else if (msg.type === "done") {
      const done = active;
      active = null;
      done.resolve(msg);
    } else if (msg.type === "error") {
      const failed = active;
      active = null;
      failed.reject(new Error(msg.error));
    }
  };
  worker.onerror = (ev) => {
    if (!active) return;
    const failed = active;
    active = null;
    failed.reject(new Error(ev.message));
  };
  return worker;
}

function requestHybrid(
  req: HybridRequest,
  onProgress: ProgressFn,
): Promise<Extract<WorkerReply, { type: "done" }>> {
  const run = () =>
    new Promise<Extract<WorkerReply, { type: "done" }>>((resolve, reject) => {
      active = { resolve, reject, onProgress };
      ensureWorker().postMessage({ type: "transcribe", ...req }, [req.samples.buffer]);
    });
  const result = queue.then(run, run);
  queue = result.catch(() => {});
  return result;
}

export async function preloadHybrid(
  model: WhisperModel,
  quant: WhisperQuant,
  language: string,
  onProgress: ProgressFn = () => {},
): Promise<void> {
  const samples = new Float32Array(16000);
  await requestHybrid(
    {
      samples,
      model,
      quant,
      langId: langToTokenId(language),
      duration: 1,
      remoteModels: (globalThis as { __rscapsRemoteModels?: boolean }).__rscapsRemoteModels === true,
    },
    onProgress,
  );
}

/**
 * Hybrid browser path: JS owns ORT WebGPU sessions, Rust WASM owns mel,
 * logits processors, tokenizer, and DTW. One persistent worker keeps the
 * sessions warm across preload and transcription.
 */
export async function transcribeHybrid(
  blob: Blob,
  model: WhisperModel,
  quant: WhisperQuant,
  language: string,
  onProgress: ProgressFn = () => {},
): Promise<HybridResult> {
  onProgress(0.02, "audio");
  const tAudio = performance.now();
  const audio = await decodeAudioTo16kMono(blob);
  console.log(`[hybrid] audio ${Math.round(performance.now() - tAudio)}ms`);
  const duration = audio.samples.length / audio.sampleRate;
  const langId = langToTokenId(language);
  const reply = await requestHybrid(
    {
      samples: audio.samples,
      model,
      quant,
      langId,
      duration,
      remoteModels: (globalThis as { __rscapsRemoteModels?: boolean }).__rscapsRemoteModels === true,
    },
    onProgress,
  );
  onProgress(1, "done");
  const result: HybridResult = {
    text: reply.result.text,
    segments: reply.result.segments,
    words: reply.result.words.map((w) => ({
      text: w.text,
      start_ms: Math.round(w.start_s * 1000),
      end_ms: Math.round(w.end_s * 1000),
    })),
    timings: reply.result.timings,
  };
  const t = result.text.trim().toLowerCase();
  const sparseWords = result.words.length < Math.max(2, duration * 1.5);
  const sparseHallucination =
    model !== "base" &&
    (sparseWords ||
      (result.text.trim().length < 64 &&
        (/^(terima kasih|sampai jumpa|thank(s| you)|please subscribe)/.test(t) ||
          result.words.length === 0)));
  if (sparseHallucination) {
    resetWorker();
    const fallback = await transcribeHybrid(blob, "base", "q8", language, (f, stage) =>
      onProgress(0.9 + f * 0.09, stage),
    );
    fallback.timings = { ...fallback.timings, fallback: 1 };
    return fallback;
  }
  return result;
}
