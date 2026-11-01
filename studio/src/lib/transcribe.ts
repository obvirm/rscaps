import { decodeAudioTo16kMono } from "./audio";
import type { WhisperModel, WhisperQuant } from "./types";
import { langToTokenId, DEFAULT_QUANT } from "./types";

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

// The spike engine serves every model size through a runtime arch config that
// JS passes via set_arch() before the ONNX bytes are loaded (layers/heads/
// d_model/n_mels differ per size; encoder len 1500, head_dim 64 and the token
// layout are shared). Keep in sync with each Xenova/whisper-* config.json.
interface ArchConfig {
  layers: number; // encoder layer count
  decLayers: number; // decoder layer count (≠ layers hanya pada turbo: 32/4)
  heads: number;
  dModel: number;
  nMels: number;
}
const MODEL_ARCH: Partial<Record<WhisperModel, ArchConfig>> = {
  // onnx-community exports use a uniform head_dim of 64, so heads = d_model / 64
  // (tiny's original config is 4 heads x 96, but the graph is 6 heads x 64).
  tiny: { layers: 4, decLayers: 4, heads: 6, dModel: 384, nMels: 80 },
  base: { layers: 6, decLayers: 6, heads: 8, dModel: 512, nMels: 80 },
  small: { layers: 12, decLayers: 12, heads: 12, dModel: 768, nMels: 80 },
  medium: { layers: 24, decLayers: 24, heads: 16, dModel: 1024, nMels: 80 },
  // n_mels 128 khas v3 (bukan 80).
  large: { layers: 32, decLayers: 32, heads: 20, dModel: 1280, nMels: 128 },
  // turbo: encoder 32 layer, decoder hanya 4 layer (distilasi large-v3).
  turbo: { layers: 32, decLayers: 4, heads: 20, dModel: 1280, nMels: 128 },
};

const SUPPORTED_MODELS: ReadonlySet<WhisperModel> = new Set(
  Object.keys(MODEL_ARCH) as WhisperModel[],
);

// onnx-community repos per model (sama dengan mapping tscaps). File diunduh
// langsung dari HuggingFace on-demand dan di-cache di browser (CacheStorage);
// folder lokal /models/whisper-<model>-<quant>/ tetap jadi fast-path dev.
const MODEL_REPO: Record<WhisperModel, string> = {
  tiny: "onnx-community/whisper-tiny_timestamped",
  base: "onnx-community/whisper-base_timestamped",
  small: "onnx-community/whisper-small_timestamped",
  medium: "onnx-community/whisper-medium_timestamped",
  large: "onnx-community/whisper-large-v3-ONNX",
  turbo: "onnx-community/whisper-large-v3-turbo_timestamped",
};

// Suffix nama file ONNX per varian kuantisasi di repo onnx-community
// ("" = fp32 polos). q8 memakai nama *_quantized.
const QUANT_SUFFIX: Record<WhisperQuant, string> = {
  fp32: "",
  fp16: "_fp16",
  int8: "_int8",
  uint8: "_uint8",
  q8: "_quantized",
  q4: "_q4",
  q4f16: "_q4f16",
  bnb4: "_bnb4",
};

export function isModelSupported(model: WhisperModel): boolean {
  return SUPPORTED_MODELS.has(model);
}

const SAMPLE_RATE = 16000;
// whisper consumes a fixed 30 s window (480 000 samples -> 80x3000 mel).
const CHUNK_SAMPLES = 30 * SAMPLE_RATE;

interface WhisperModule {
  default: (wasmUrl?: string) => Promise<unknown>;
  init: (distUrl?: string) => Promise<void>;
  set_arch: (
    layers: number,
    decLayers: number,
    heads: number,
    headDim: number,
    dModel: number,
    vocab: number,
    nMels: number,
  ) => void;
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

// One module instance per model: the wasm holds that model's encoder/decoder
// sessions and arch, so switching size in the UI must reload (set_arch +
// re-fetch the ONNX files for the new size).
const cached = new Map<string, WhisperModule>();
const loading = new Map<string, Promise<WhisperModule>>();

function ensureOk(res: Response, url: string): Response {
  if (!res.ok) throw new Error(`fetch failed ${res.status}: ${url}`);
  return res;
}

// Fetch a file while reporting byte progress (0→1 within this file). Falls back
// to an indeterminate `total = 0` when the server omits Content-Length.
async function readWithProgress(res: Response, onFrac: (frac: number) => void): Promise<Uint8Array> {
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

async function fetchModelFile(
  model: WhisperModel,
  quant: WhisperQuant,
  file: string,
  onFrac: (frac: number) => void,
): Promise<Uint8Array> {
  // Fast-path dev: folder lokal /models/whisper-<model>-<quant>/ (nama file
  // selalu polos encoder_model.onnx / decoder_model_merged.onnx).
  const local = `/models/whisper-${model}-${quant}/${file.replace(/_quantized|_q4f16|_q4|_fp16|_int8|_uint8|_bnb4/, "")}`;
  try {
    const res = await fetch(local);
    if (res.ok && (res.headers.get("content-type") || "").includes("octet-stream")) {
      return await readWithProgress(res, onFrac);
    }
  } catch {
    /* fall through to HuggingFace */
  }
  // On-demand dari HuggingFace; hasil di-cache di CacheStorage supaya unduhan
  // pertama hanya sekali per kombinasi model×kuantisasi (gaya tscaps).
  const url = `https://huggingface.co/${MODEL_REPO[model]}/resolve/main/onnx/${file}`;
  const cache = await caches.open("rscaps-whisper-models");
  const hit = await cache.match(url);
  if (hit) {
    onFrac(1);
    return new Uint8Array(await hit.arrayBuffer());
  }
  const res = ensureOk(await fetch(url), url);
  const bytes = await readWithProgress(res, onFrac);
  await cache.put(
    url,
    new Response(bytes.slice().buffer, {
      headers: { "content-length": String(bytes.length) },
    }),
  );
  return bytes;
}

async function loadWhisper(
  model: WhisperModel,
  quant: WhisperQuant,
  onProgress: ProgressFn,
): Promise<WhisperModule> {
  const key = `${model}:${quant}`;
  const hit = cached.get(key);
  if (hit) return hit;
  const inflight = loading.get(key);
  if (inflight) return inflight;
  const task = (async () => {
    onProgress(0.02, "engine");
    const pkgUrl = "/wpkg/whisper_spike.js";
    const mod = (await import(/* @vite-ignore */ pkgUrl)) as WhisperModule;
    await mod.default("/wpkg/whisper_spike_bg.wasm");
    // ort-web dist self-host; jangan pakai default Rust (hardcoded port dev
    // lama) — ikuti origin halaman supaya port dev berganti pun tetap jalan.
    await mod.init(`${location.origin}/nm/onnxruntime-web/dist/`);
    // The threaded ort build can hang during session init; stay single-threaded.
    const g = globalThis as { ort?: { env?: { wasm?: { numThreads?: number } } } };
    if (g.ort?.env?.wasm) g.ort.env.wasm.numThreads = 1;

    // Arch BEFORE the ONNX bytes: the decoder graph shape and mel size depend
    // on it. head_dim is 64 and logits vocab 51865 across every size (large-v3
    // carries a 51866th tokenizer entry but its logits stay 51865).
    const a = MODEL_ARCH[model];
    if (!a) throw new Error(`model "${model}" has no arch config`);
    mod.set_arch(a.layers, a.decLayers, a.heads, 64, a.dModel, 51865, a.nMels);

    onProgress(0.05, "model");
    const suffix = QUANT_SUFFIX[quant];
    // Fetch sequentially with byte progress so the bar moves smoothly during a
    // first-run download.
    const enc = await fetchModelFile(model, quant, `encoder_model${suffix}.onnx`, (f) =>
      onProgress(0.05 + f * 0.13, "model"),
    );
    const dec = await fetchModelFile(model, quant, `decoder_model_merged${suffix}.onnx`, (f) =>
      onProgress(0.18 + f * 0.1, "model"),
    );
    let tok: string;
    if (quant === DEFAULT_QUANT[model]) {
      // fast-path lokal sudah menyediakan tokenizer di folder default
      const tokRes = await fetch(`/models/whisper-${model}-${quant}/tokenizer.json`);
      if (tokRes.ok) {
        tok = await tokRes.text();
      } else {
        tok = await (await ensureOk(
          await fetch(`https://huggingface.co/${MODEL_REPO[model]}/resolve/main/tokenizer.json`),
          "tokenizer.json",
        )).text();
      }
    } else {
      tok = await (await ensureOk(
        await fetch(`https://huggingface.co/${MODEL_REPO[model]}/resolve/main/tokenizer.json`),
        "tokenizer.json",
      )).text();
    }
    onProgress(0.29, "model");
    await mod.load_encoder(enc);
    await mod.load_decoder(dec);
    mod.load_tokenizer(tok);
    cached.set(key, mod);
    return mod;
  })();
  loading.set(key, task);
  try {
    return await task;
  } finally {
    loading.delete(key);
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
  quant: WhisperQuant,
  language: string,
  onProgress: ProgressFn = () => {},
): Promise<TranscribeResult> {
  if (!isModelSupported(model)) {
    throw new Error(
      `model "${model}" is not supported by this engine build yet (supported: ${[...SUPPORTED_MODELS].join(", ")}).`,
    );
  }
  const mod = await loadWhisper(model, quant, onProgress);

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
