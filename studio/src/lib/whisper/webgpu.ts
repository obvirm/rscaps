import { decodeAudioTo16kMono } from "../audio";
import type { WhisperModel, WhisperQuant } from "../types";
import type { ProgressFn, TranscribeResult } from "../transcribe";

type Backend = "webgpu" | "cpu";

type Reply =
  | { type: "progress"; id: number; phase: string; fraction: number }
  | {
      type: "done";
      id: number;
      result: TranscribeResult & { timings?: Record<string, unknown> };
    }
  | { type: "error"; id: number; error: string };

type Pending = {
  resolve: (r: Extract<Reply, { type: "done" }>) => void;
  reject: (e: Error) => void;
  onProgress: ProgressFn;
};

let worker: Worker | null = null;
let nextId = 0;
const pending = new Map<number, Pending>();

function getWorker(): Worker {
  if (worker) return worker;
  worker = new Worker(new URL("./webgpu.worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (ev: MessageEvent<Reply>) => {
    const msg = ev.data;
    const p = pending.get(msg.id);
    if (!p) return;
    if (msg.type === "progress") {
      p.onProgress(0.05 + msg.fraction * 0.9, "transcribe");
    } else if (msg.type === "done") {
      pending.delete(msg.id);
      p.resolve(msg);
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

function send(
  payload: {
    type: "transcribe" | "preload";
    samples: Float32Array;
    model: WhisperModel;
    quant: WhisperQuant;
    backend: Backend;
    language: string;
  },
  onProgress: ProgressFn,
): Promise<Extract<Reply, { type: "done" }>> {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject, onProgress });
    getWorker().postMessage({ ...payload, id }, [payload.samples.buffer]);
  });
}

export async function preloadWebGpu(
  model: WhisperModel,
  quant: WhisperQuant,
  backend: Backend,
  language: string,
  onProgress: ProgressFn = () => {},
): Promise<void> {
  await send(
    { type: "preload", samples: new Float32Array(16000), model, quant, backend, language },
    onProgress,
  );
}

export async function transcribeWebGpu(
  blob: Blob,
  model: WhisperModel,
  quant: WhisperQuant,
  language: string,
  backend: Backend = "webgpu",
  onProgress: ProgressFn = () => {},
): Promise<TranscribeResult> {
  onProgress(0.02, "audio");
  const audio = await decodeAudioTo16kMono(blob);
  const reply = await send(
    { type: "transcribe", samples: audio.samples, model, quant, backend, language },
    onProgress,
  );
  onProgress(1, "done");
  return {
    text: reply.result.text,
    segments: reply.result.segments,
    words: reply.result.words ?? [],
  };
}
