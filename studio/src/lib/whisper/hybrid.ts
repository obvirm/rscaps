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

/**
 * Hybrid browser path: JS owns ORT WebGPU sessions, Rust WASM owns mel,
 * logits processors, tokenizer, and DTW. Kept separate from the legacy
 * whisper-spike WASM path until parity is proven.
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

  const worker = new Worker(new URL("./hybrid.worker.ts", import.meta.url), { type: "module" });
  try {
    const reply = await new Promise<Extract<WorkerReply, { type: "done" }>>((resolve, reject) => {
      worker.onmessage = (ev: MessageEvent<WorkerReply>) => {
        const msg = ev.data;
        if (msg.type === "boot") {
          console.log(`[hybrid] worker boot ${Math.round(performance.now() - tAudio)}ms`);
        } else if (msg.type === "progress") {
          onProgress(0.05 + msg.fraction * 0.9, "transcribe");
        } else if (msg.type === "done") {
          resolve(msg);
        } else {
          reject(new Error(msg.error));
        }
      };
      worker.onerror = (ev) => reject(new Error(ev.message));
      worker.postMessage(
        {
          type: "transcribe",
          samples: audio.samples,
          model,
          quant,
          langId,
          duration,
          remoteModels: (globalThis as { __rscapsRemoteModels?: boolean }).__rscapsRemoteModels === true,
        },
        [audio.samples.buffer],
      );
    });
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
    const sparseWords = result.words.length < Math.max(2, duration * 0.75);
    const sparseHallucination =
      model !== "base" &&
      (sparseWords ||
        (result.text.trim().length < 64 &&
          (/^(terima kasih|sampai jumpa|thank(s| you)|please subscribe)/.test(t) ||
            result.words.length === 0)));
    if (sparseHallucination) {
      const fallback = await transcribeHybrid(blob, "base", "q8", language, (f, stage) =>
        onProgress(0.9 + f * 0.09, stage),
      );
      fallback.timings = { ...fallback.timings, fallback: 1 };
      return fallback;
    }
    return result;
  } finally {
    worker.terminate();
  }
}
