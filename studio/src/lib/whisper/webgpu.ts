import { decodeAudioTo16kMono } from "../audio";
import type { WhisperBackend, WhisperModel, WhisperQuant } from "../types";
import type { ProgressFn, TranscribeResult, TranscribeSegment } from "../transcribe";

type Word = { text: string; start_s: number; end_s: number };

type Reply =
  | { type: "progress"; id: number; phase: string; fraction: number }
  | { type: "done"; id: number; result: { text: string; segments: TranscribeSegment[]; words: Word[]; timings?: Record<string, number> } }
  | { type: "error"; id: number; error: string };

let worker: Worker | null = null;
let nextId = 1;
let queue: Promise<unknown> = Promise.resolve();
const pending = new Map<
  number,
  {
    onProgress: (fraction: number, phase: string) => void;
    resolve: (r: Extract<Reply, { type: "done" }>["result"]) => void;
    reject: (e: Error) => void;
  }
>();

function getWorker() {
  if (worker) return worker;
  worker = new Worker(new URL("./webgpu.worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (ev: MessageEvent<Reply>) => {
    const msg = ev.data;
    const p = pending.get(msg.id);
    if (!p) return;
    if (msg.type === "progress") p.onProgress(msg.fraction, msg.phase);
    else if (msg.type === "done") {
      pending.delete(msg.id);
      p.resolve(msg.result);
    } else {
      pending.delete(msg.id);
      p.reject(new Error(msg.error));
    }
  };
  worker.onerror = (ev) => {
    for (const p of pending.values()) p.reject(new Error(ev.message));
    pending.clear();
    worker = null;
  };
  return worker;
}

function enqueue<T>(fn: () => Promise<T>): Promise<T> {
  const next = queue.then(fn, fn);
  queue = next.then(
    () => undefined,
    () => undefined,
  );
  return next;
}

function send(
  req: { type: "transcribe" | "preload"; samples?: Float32Array; model: WhisperModel; backend: WhisperBackend; language: string },
  onProgress: (fraction: number, phase: string) => void,
): Promise<Extract<Reply, { type: "done" }>["result"]> {
  return enqueue(
    () =>
      new Promise((resolve, reject) => {
        const id = nextId++;
        pending.set(id, { onProgress, resolve, reject });
        const w = getWorker();
        w.postMessage({ ...req, id }, req.samples ? [req.samples.buffer] : []);
      }),
  );
}

export async function preloadWebGpu(
  model: WhisperModel,
  backend: WhisperBackend,
  language: string,
  onProgress: ProgressFn = () => {},
): Promise<void> {
  await send(
    { type: "preload", model, backend, language },
    (fraction) => onProgress(fraction, "model"),
  );
}

export async function transcribeWebGpu(
  blob: Blob,
  model: WhisperModel,
  _quant: WhisperQuant,
  language: string,
  backend: WhisperBackend,
  onProgress: ProgressFn = () => {},
): Promise<TranscribeResult & { words: (Word & { start_ms: number; end_ms: number })[]; timings?: Record<string, number> }> {
  onProgress(0.02, "audio");
  const audio = await decodeAudioTo16kMono(blob);
  const result = await send(
    { type: "transcribe", samples: audio.samples, model, backend, language },
    (fraction) => onProgress(0.05 + fraction * 0.9, "transcribe"),
  );
  onProgress(1, "done");
  return {
    text: result.text,
    segments: result.segments,
    words: result.words.map((w) => ({
      ...w,
      start_ms: Math.round(w.start_s * 1000),
      end_ms: Math.round(w.end_s * 1000),
    })),
    timings: result.timings,
  };
}
