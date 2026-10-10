import { decodeAudioTo16kMono } from "./audio";
import type { WhisperModel } from "./types";
import { langToTokenId } from "./types";

export interface TranscribeSegment {
  start: number; // seconds
  end: number; // seconds
  text: string;
}

export interface TranscribeResult {
  segments: TranscribeSegment[];
  text: string;
}

export type ProgressFn = (fraction: number, stage: TranscribeStage) => void;

// Coarse phases of a transcription run, used by the loading overlay to pick
// its title/subtext. `fraction` is the overall 0→1 progress across all phases.
export type TranscribeStage = "engine" | "model" | "audio" | "transcribe" | "done";

// The spike engine is compiled for the whisper-base architecture only (6 enc/dec
// layers, d_model 512, 8 heads, vocab 51865). Other sizes need their own
// constants baked into the wasm, so we refuse them loudly instead of producing
// garbage.
const SUPPORTED_MODELS: ReadonlySet<WhisperModel> = new Set(["base"]);

export function isModelSupported(model: WhisperModel): boolean {
  return SUPPORTED_MODELS.has(model);
}

const SAMPLE_RATE = 16000;
// whisper consumes a fixed 30 s window (480 000 samples -> 80x3000 mel).
const CHUNK_SAMPLES = 30 * SAMPLE_RATE;

interface WhisperModule {
  default: (wasmUrl?: string) => Promise<unknown>;
  init: () => Promise<void>;
  load_encoder: (bytes: Uint8Array) => Promise<void>;
  load_decoder: (bytes: Uint8Array) => Promise<void>;
  load_tokenizer: (json: string) => void;
  encode_audio: (samples: Float32Array) => Promise<Float32Array>;
  transcribe: (
    hidden: Float32Array,
    langId: bigint,
    task: bigint,
    returnTimestamps: boolean,
    maxTokens: number,
    audioDuration: number,
  ) => Promise<string>;
}

const TRANSCRIBE_TASK = 50359n; // <|transcribe|>
const MAX_TOKENS = 448;

let cached: WhisperModule | null = null;
let loading: Promise<WhisperModule> | null = null;

function ensureOk(res: Response, url: string): Response {
  if (!res.ok) throw new Error(`fetch failed ${res.status}: ${url}`);
  return res;
}

// Fetch a file while reporting byte progress (0→1 within this file). Falls back
// to an indeterminate `total = 0` when the server omits Content-Length.
async function fetchBytes(
  url: string,
  onFrac: (frac: number) => void,
): Promise<Uint8Array> {
  const res = ensureOk(await fetch(url), url);
  const total = Number(res.headers.get("content-length")) || 0;
  if (!res.body) return new Uint8Array(await res.arrayBuffer());
  const reader = res.body.getReader();
  const parts: Uint8Array[] = [];
  let loaded = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    loaded += value.length;
    if (total > 0) onFrac(loaded / total);
  }
  const out = new Uint8Array(loaded);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

async function loadWhisper(model: WhisperModel, onProgress: ProgressFn): Promise<WhisperModule> {
  if (cached) return cached;
  if (loading) return loading;
  loading = (async () => {
    onProgress(0.02, "engine");
    const pkgUrl = "/wpkg/whisper_spike.js";
    const mod = (await import(/* @vite-ignore */ pkgUrl)) as WhisperModule;
    await mod.default("/wpkg/whisper_spike_bg.wasm");
    await mod.init();
    // The threaded ort build can hang during session init; stay single-threaded.
    const g = globalThis as { ort?: { env?: { wasm?: { numThreads?: number } } } };
    if (g.ort?.env?.wasm) g.ort.env.wasm.numThreads = 1;

    onProgress(0.05, "model");
    const base = `/models/whisper-${model}/`;
    // Fetch sequentially with byte progress so the bar moves smoothly during a
    // first-run download (encoder ~23 MB, decoder ~54 MB).
    const enc = await fetchBytes(base + "encoder_model.onnx", (f) =>
      onProgress(0.05 + f * 0.13, "model"),
    );
    const dec = await fetchBytes(base + "decoder_model_merged.onnx", (f) =>
      onProgress(0.18 + f * 0.1, "model"),
    );
    const tokRes = ensureOk(await fetch(base + "tokenizer.json"), base + "tokenizer.json");
    const tok = await tokRes.text();
    onProgress(0.29, "model");
    await mod.load_encoder(enc);
    await mod.load_decoder(dec);
    mod.load_tokenizer(tok);
    cached = mod;
    return mod;
  })();
  try {
    return await loading;
  } catch (err) {
    loading = null;
    throw err;
  }
}

/**
 * Transcribe a media blob (audio or video container). Decodes to mono 16 kHz,
 * then runs the whisper engine in 30 s windows, offsetting each window's
 * timestamps back to the absolute timeline and concatenating the results.
 */
export async function transcribeBlob(
  blob: Blob,
  model: WhisperModel,
  language: string,
  onProgress: ProgressFn = () => {},
): Promise<TranscribeResult> {
  if (!isModelSupported(model)) {
    throw new Error(
      `model "${model}" is not supported by this engine build yet (only: ${[...SUPPORTED_MODELS].join(", ")}). Pick Base in Advanced.`,
    );
  }
  const mod = await loadWhisper(model, onProgress);

  onProgress(0.32, "audio");
  const audio = await decodeAudioTo16kMono(blob);
  const samples = audio.samples;
  const totalSamples = samples.length;
  if (totalSamples === 0) {
    onProgress(1, "done");
    return { segments: [], text: "" };
  }

  const langId = BigInt(langToTokenId(language));
  const segments: TranscribeSegment[] = [];
  const texts: string[] = [];
  const totalChunks = Math.max(1, Math.ceil(totalSamples / CHUNK_SAMPLES));

  for (let i = 0; i < totalChunks; i++) {
    const start = i * CHUNK_SAMPLES;
    const end = Math.min(start + CHUNK_SAMPLES, totalSamples);
    const window = samples.subarray(start, end);
    // Pad the final short window to at least 1 sample so the mel has signal.
    const chunk = window.length < 1 ? new Float32Array(1) : window;

    onProgress(0.4 + (i / totalChunks) * 0.58, "transcribe");
    const offsetSec = start / SAMPLE_RATE;
    const windowDur = chunk.length / SAMPLE_RATE;

    const hidden = await mod.encode_audio(chunk);
    const raw = await mod.transcribe(hidden, langId, TRANSCRIBE_TASK, true, MAX_TOKENS, windowDur);
    const parsed = JSON.parse(raw) as {
      text: string;
      segments: { start: number; end: number; text: string }[];
    };

    for (const seg of parsed.segments) {
      const text = seg.text.trim();
      if (!text) continue;
      segments.push({
        start: seg.start + offsetSec,
        end: seg.end + offsetSec,
        text,
      });
    }
    if (parsed.text.trim()) texts.push(parsed.text.trim());
  }

  onProgress(1, "done");
  return { segments, text: texts.join(" ").trim() };
}
