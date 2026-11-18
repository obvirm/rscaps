import { decodeAudioTo16kMono } from "./audio";
import type { WhisperModel, WhisperQuant } from "./types";
import { langToTokenId, DEFAULT_QUANT } from "./types";
import { invoke, isTauri } from "./tauri";

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

// Encoder: A/B cross-load (large) membuktikan encoder kuant (_quantized/uint8)
// membuat decoder memilih tail-hallucination, sementara decoder kuant relatif
// baik; encoder int8 stabil di semua ukuran. Encoder SELALU diambil varian
// int8 — decoder tetap mengikuti quant yang dipilih user. (Fast-path folder
// lokal bisa beda asal file-nya int8/fp16 non-kuant — sama-sama stabil.)
const ENCODER_SUFFIX = "_int8";

export function isModelSupported(model: WhisperModel): boolean {
  return SUPPORTED_MODELS.has(model);
}

const SAMPLE_RATE = 16000;

// Speech-activity check (VAD-lite): peak RMS over 100 ms frames; active
// speech/music sits far above this floor, room tone and digital silence below.
function hasSpeechEnergy(samples: Float32Array): boolean {
  const frame = Math.round(SAMPLE_RATE * 0.1);
  let peak = 0;
  for (let i = 0; i + frame <= samples.length; i += frame) {
    let sum = 0;
    for (let j = i; j < i + frame; j++) sum += samples[j] * samples[j];
    const rms = Math.sqrt(sum / frame);
    if (rms > peak) peak = rms;
  }
  return peak > 0.01;
}
// whisper consumes a fixed 30 s window (480 000 samples -> 80x3000 mel).
const CHUNK_SAMPLES = 30 * SAMPLE_RATE;

interface WhisperModule {
  default: (wasmUrl?: string) => Promise<unknown>;
  init: (distUrl?: string) => Promise<void>;
  init_webgpu?: (distUrl?: string) => Promise<void>;
  set_arch: (
    layers: number,
    decLayers: number,
    heads: number,
    headDim: number,
    dModel: number,
    vocab: number,
    nMels: number,
    classic: boolean,
  ) => void;
  set_opt_level: (level: number) => void;
  set_logprob_gate: (v: number) => void;
  set_min_new_tokens: (v: number) => void;
  load_encoder: (bytes: Uint8Array | null) => Promise<void>;
  load_decoder: (bytes: Uint8Array | null) => Promise<void>;
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

// Vocab variant per repo (ground truth generation_config.json, 2026-11):
// repo *_timestamped tiny/base/small/medium = klasik (transcribe=50359,
// notimestamps=50363, ts_begin=50364); large-v3-ONNX & turbo_timestamped =
// v3 (transcribe=50360, notimestamps=50364, ts_begin=50365). Bahasa &
// decoder_start sama di keduanya.
const MODEL_CLASSIC: Partial<Record<WhisperModel, boolean>> = {
  tiny: true,
  base: true,
  small: true,
  medium: true,
  large: false,
  turbo: false,
};
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

// Fetch a file while reporting byte progress (0→1 within this file). The
// output buffer is preallocated from Content-Length — concatenating streamed
// chunks (parts + final copy) doubled the JS peak and made 1.7 GB+ models hit
// "Array buffer allocation failed" (RangeError) in Chrome's heap.
async function readWithProgress(res: Response, onFrac: (frac: number) => void): Promise<Uint8Array> {
  const total = Number(res.headers.get("content-length")) || 0;
  if (!res.body) return new Uint8Array(await res.arrayBuffer());
  const reader = res.body.getReader();
  let out: Uint8Array;
  let loaded = 0;
  if (total > 0) {
    out = new Uint8Array(total);
  } else {
    // Unknown length: accumulate (rare — our sources always send it).
    const parts: Uint8Array[] = [];
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      parts.push(value);
      loaded += value.length;
      onFrac(0);
    }
    out = new Uint8Array(loaded);
    let off = 0;
    for (const p of parts) {
      out.set(p, off);
      off += p.length;
    }
    return out;
  }
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (loaded + value.length > total) break; // server lied about length
    out.set(value, loaded);
    loaded += value.length;
    onFrac(loaded / total);
  }
  if (loaded < total) throw new Error(`short read: ${loaded}/${total}`);
  return out;
}

// Varian external-data (model ≥1,5 GB): folder lokal dev dapat membawa
// <file>.ext.onnx + <file>.ext.data (pisah via scripts/tmp_split_onnx.mjs).
// ort.min.js memuat data lewat mountExternalData dan melepasnya setelah
// create — transient build turun dari ~3× bobot ke ~1× sehingga medium/int8,
// medium/uint8, large/fp16, dan turbo/fp16 muat di plafon wasm32 4 GB.
async function fetchModelExt(
  model: WhisperModel,
  quant: WhisperQuant,
  base: string,
  onFrac: (frac: number) => void,
): Promise<{ pb: Uint8Array; data: Uint8Array } | null> {
  const dir = `/models/whisper-${model}-${quant}`;
  try {
    const res = await fetch(`${dir}/${base}.ext.onnx`);
    if (!res.ok || !(res.headers.get("content-type") || "").includes("octet-stream")) return null;
    const pb = await readWithProgress(res, onFrac);
    const dres = await fetch(`${dir}/${base}.ext.data`);
    if (!dres.ok) return null;
    const data = await readWithProgress(dres, onFrac);
    return { pb, data };
  } catch {
    return null;
  }
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
  // CacheStorage stores a full copy of the bytes; above ~1.6 GB the extra
  // Response-body copy trips Chrome's ArrayBuffer allocation again, so huge
  // models are NOT cached (dev fast-path folders cover them).
  if (bytes.length < 1_600_000_000) {
    await cache.put(
      url,
      new Response(bytes.buffer as ArrayBuffer, {
        headers: { "content-length": String(bytes.length) },
      }),
    );
  }
  return bytes;
}

async function loadWhisper(
  model: WhisperModel,
  quant: WhisperQuant,
  onProgress: ProgressFn,
  backend: "cpu" | "webgpu" = "cpu",
): Promise<WhisperModule> {
  const key = `${model}:${quant}:${backend}`;
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
    if (backend === "webgpu" && mod.init_webgpu) {
      await mod.init_webgpu(`${location.origin}/nm/onnxruntime-web/dist/`);
    } else {
      await mod.init(`${location.origin}/nm/onnxruntime-web/dist/`);
    }    // Thread pool: the old hard-coded numThreads=1 ("threaded build hangs")
    // left all cores idle but one; the detached-ArrayBuffer hang behind it is
    // fixed in Rust by pre-growing the heap. Use cores-1, with __rscapsThreads
    // as the A/B escape hatch.
    const g = globalThis as { ort?: { env?: { wasm?: { numThreads?: number } } }; __rscapsThreads?: number };
    if (g.ort?.env?.wasm) {
      const dbgThreads = g.__rscapsThreads;
      g.ort.env.wasm.numThreads =
        typeof dbgThreads === "number" && dbgThreads > 0
          ? Math.floor(dbgThreads)
          : Math.max(1, (navigator.hardwareConcurrency || 4) - 1);
    }

    // Arch BEFORE the ONNX bytes: the decoder graph shape and mel size depend
    // on it. head_dim is 64 and logits vocab 51865 across every size (large-v3
    // carries a 51866th tokenizer entry but its logits stay 51865).
    const a = MODEL_ARCH[model];
    if (!a) throw new Error(`model "${model}" has no arch config`);
    mod.set_arch(a.layers, a.decLayers, a.heads, 64, a.dModel, 51865, a.nMels, MODEL_CLASSIC[model] ?? false);

    onProgress(0.05, "model");
    // fp16-weight models (fp16, q4f16) hit ORT regression #26717
    // (SimplifiedLayerNormFusion crash during session init). Basic level skips
    // the extended fusions that carry the bug; the patched ort-web now
    // actually applies this level. medium/q8 (QDQ) also breaks at All —
    // instant EOS, empty output — but works at Basic (A/B:
    // scripts/tmp_runs/ab-medium-q8-opt1.json). Others stay at All.
    // Escape hatch harness (sama semangatnya dgn env RSCAPS_*): override
    // opt-level dari console agar A/B per level bisa dijalankan tanpa edit.
    const dbgLevel = (globalThis as { __rscapsDebugOptLevel?: number }).__rscapsDebugOptLevel;
    const suffix = QUANT_SUFFIX[quant];
    // Fetch sequentially with byte progress so the bar moves smoothly during a
    // first-run download. External-data variants (if the local folder carries
    // the split) are tried first and skip the full-file fetch entirely.
    let decExt = await fetchModelExt(model, quant, "decoder_model_merged", (f) =>
      onProgress(0.05 + f * 0.12, "model"),
    );
    let encExt: { pb: Uint8Array; data: Uint8Array } | null = null;
    if (decExt) {
      encExt = await fetchModelExt(model, quant, `encoder_model${ENCODER_SUFFIX}`, (f) => onProgress(0.17 + f * 0.11, "model"));
    }
    let enc: Uint8Array | null = encExt ? null : await fetchModelFile(model, quant, `encoder_model${ENCODER_SUFFIX}.onnx`, (f) =>
      onProgress(0.05 + f * 0.13, "model"),
    );
    let dec: Uint8Array | null = decExt ? null : await fetchModelFile(model, quant, `decoder_model_merged${suffix}.onnx`, (f) =>
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
    // Opt level: external-data builds run at Disable — graph transforms copy
    // the (already huge) weight set once more and bust the 4 GB ceiling
    // (medium/int8: level 4 = bad_alloc, level 0 = works). Non-ext keeps the
    // old mapping (fp16/q4f16/medium-q8 → Basic for ORT #26717 & QDQ). QDQ
    // `_quantized` decoders break at All on small too (A/B scripts/tmp_runs/
    // small-q8-opt1.json: All = early-stop "Selamat tinggal", Basic = full id).
    const q8Basic = (model === "medium" || model === "small") && quant === "q8";
    const optLevel = dbgLevel ?? (decExt || encExt ? 0 : quant === "fp16" || quant === "q4f16" || q8Basic ? 1 : 4);
    mod.set_opt_level(optLevel);
    // Escape hatch harness: override the HF avg-logprob quality gate per run.
    const dbgGate = (globalThis as { __rscapsLogprobGate?: number }).__rscapsLogprobGate;
    if (typeof dbgGate === "number" && Number.isFinite(dbgGate)) {
      mod.set_logprob_gate(dbgGate);
    }
    const dbgMinNew = (globalThis as { __rscapsMinNewTokens?: number }).__rscapsMinNewTokens;
    if (typeof dbgMinNew === "number" && Number.isFinite(dbgMinNew)) {
      mod.set_min_new_tokens(dbgMinNew);
    }
    // Load decoder FIRST, encoder last: peak ort-wasm heap = model buffer +
    // built weights in one shot. Encoder session is the smaller of the two, so
    // building it last (on top of the decoder session) keeps every peak below
    // the wasm32 4 GB ceiling — enc-first hit 4.7 GB on large/fp16 (bad_alloc
    // during decoder build). JS refs are dropped right after each load so the
    // byte copies can be GC'd while the next session builds.
    const gExt = globalThis as { __rscapsExternalData?: { path: string; data: Uint8Array }[] };
    if (decExt) {
      // Path harus persis = location yang dibake ke pb oleh tmp_split_onnx.mjs.
      gExt.__rscapsExternalData = [{ path: "decoder_model_merged.weights.data", data: decExt.data }];
      try {
        await mod.load_decoder(decExt.pb);
      } finally {
        gExt.__rscapsExternalData = undefined;
      }
      decExt = null;
    } else {
      await mod.load_decoder(dec);
      dec = null;
    }
    if (encExt) {
      gExt.__rscapsExternalData = [{ path: "encoder_model.weights.data", data: encExt.data }];
      try {
        await mod.load_encoder(encExt.pb);
      } finally {
        gExt.__rscapsExternalData = undefined;
      }
      encExt = null;
    } else {
      await mod.load_encoder(enc);
      enc = null;
    }
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
 * Jalur shell desktop: engine NATIVE (Rust, ORT penuh). Blob media ditulis
 * chunked ke file temp (pattern save_export), lalu command `transcribe_native`
 * men-decode audio (ffmpeg) + transkrip (window 30s + rescue) — progress di-
 * poll dari state Rust tiap 500 ms.
 */
async function transcribeNative(
  blob: Blob,
  model: WhisperModel,
  quant: WhisperQuant,
  language: string,
  onProgress: ProgressFn,
): Promise<TranscribeResult> {
  onProgress(0.02, "audio");
  const tmpDir = await invoke<string>("native_get_temp_dir");
  const ext = (blob.name?.match(/\.([a-z0-9]{1,5})$/i)?.[1] ?? "mp4").toLowerCase();
  const path = `${tmpDir}\\rscaps-transcribe-${Date.now()}.${ext}`;
  const buf = new Uint8Array(await blob.arrayBuffer());
  const CHUNK = 4 * 1024 * 1024;
  for (let off = 0; off < buf.length; off += CHUNK) {
    const slice = buf.subarray(off, Math.min(off + CHUNK, buf.length));
    if (off === 0) await invoke("native_write_file", { path, data: Array.from(slice) });
    else await invoke("save_export_append", { path, data: Array.from(slice) });
  }

  const runId = `t${Date.now()}`;
  const poll = setInterval(() => {
    invoke<[number, string]>("transcribe_native_progress", { runId })
      .then(([f, stage]) => {
        if (f > 0) onProgress(Math.min(f, 0.99) as number, stage as TranscribeStage);
      })
      .catch(() => {});
  }, 500);

  try {
    onProgress(0.05, "audio");
    const result = await invoke<{ text: string; segments: { start: number; end: number; text: string }[] }>(
      "transcribe_native",
      { path, model, quant, lang: language, runId, ffmpegDir: null },
    );
    onProgress(1, "done");
    const segments: TranscribeSegment[] = result.segments
      .map((s) => ({ start: s.start, end: s.end, text: s.text.trim() }))
      .filter((s) => s.text);
    return { segments, text: result.text.trim() };
  } finally {
    clearInterval(poll);
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
  // Shell desktop (Tauri): transkrip = engine NATIVE (ORT penuh) di Rust,
  // BUKAN wasm. Audio ditulis ke temp file → command transcribe_native
  // (ffmpeg decode + window 30s + rescue di Rust) → JSON sama skema.
  if (isTauri()) {
    return transcribeNative(blob, model, quant, language, onProgress);
  }
  const backend = ((globalThis as { __rscapsBackend?: string }).__rscapsBackend === "webgpu"
    ? "webgpu"
    : "cpu") as "cpu" | "webgpu";
  const mod = await loadWhisper(model, quant, onProgress, backend);

  onProgress(0.32, "audio");
  const audio = await decodeAudioTo16kMono(blob);
  const samples = audio.samples;
  const totalSamples = samples.length;
  if (totalSamples === 0) {
    onProgress(1, "done");
    return { segments: [], text: "" };
  }

  const langId = BigInt(langToTokenId(language));
  // <|transcribe|> per vocab variant (klasik 50359, v3 50360).
  const taskToken = BigInt(MODEL_CLASSIC[model] ?? false ? 50359 : 50360);
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

    let winSegments: TranscribeSegment[] = [];
    let winText = "";

    const hidden = await mod.encode_audio(chunk);
    const raw = await mod.transcribe(hidden, langId, taskToken, true, MAX_TOKENS, windowDur);
    let parsed = JSON.parse(raw) as {
      text: string;
      segments: { start: number; end: number; text: string }[];
      quality?: { silent: boolean; avg_logprob: number; no_speech: number };
    };
    if (parsed.quality) console.log(`quality ${JSON.stringify(parsed.quality)}`);

    // Sparse-decode rescue: a confident end-of-audio prior ("Terima kasih
    // telah menonton!") on hard audio yields a few words across many seconds
    // of dense speech (fragile quant exports, verbatim in transformers.js).
    // Re-decode the window in short slices — short windows escape the
    // attractor — and keep the result only when it is meaningfully richer.
    const wordCount = parsed.text.trim() ? parsed.text.trim().split(/\s+/).length : 0;
    // Degeneracy signal: one word dominating the decode ("I, I, I, I") is a
    // repetition artifact even when the sparse-word test passes.
    const wordHist = new Map<string, number>();
    for (const w of parsed.text.toLowerCase().split(/\s+/)) {
      if (!w) continue;
      wordHist.set(w, (wordHist.get(w) ?? 0) + 1);
    }
    const topWordFrac = wordCount ? Math.max(...wordHist.values()) / wordCount : 0;
    if (
      !parsed.quality?.silent &&
      windowDur > 8 &&
      (wordCount < Math.max(2, windowDur * 0.4) || topWordFrac > 0.3) &&
      hasSpeechEnergy(chunk)
    ) {
      const sliceTexts: string[] = [];
      let sliceWords = 0;
      let sliceSegments: TranscribeSegment[] = [];
      const SLICE_SAMPLES = 4 * SAMPLE_RATE;
      const nSlices = Math.ceil(chunk.length / SLICE_SAMPLES);
      for (let s = 0; s < nSlices; s++) {
        const slice = chunk.subarray(
          s * SLICE_SAMPLES,
          Math.min((s + 1) * SLICE_SAMPLES, chunk.length),
        );
        if (slice.length < SAMPLE_RATE / 2) break;
        const sliceStartSec = (s * SLICE_SAMPLES) / SAMPLE_RATE;
        const sliceDur = slice.length / SAMPLE_RATE;
        const hidden2 = await mod.encode_audio(slice);
        const raw2 = await mod.transcribe(hidden2, langId, taskToken, true, MAX_TOKENS, sliceDur);
        const parsed2 = JSON.parse(raw2) as typeof parsed;
        if (parsed2.quality) console.log(`quality ${JSON.stringify(parsed2.quality)}`);
        for (const seg of parsed2.segments) {
          const text = seg.text.trim();
          if (!text) continue;
          sliceSegments.push({ start: seg.start + sliceStartSec, end: seg.end + sliceStartSec, text });
        }
        if (parsed2.text.trim()) {
          sliceTexts.push(parsed2.text.trim());
          sliceWords += parsed2.text.trim().split(/\s+/).length;
        }
      }
      if (sliceWords > wordCount) {
        parsed = { text: sliceTexts.join(" "), segments: sliceSegments };
      }
    }

    for (const seg of parsed.segments) {
      const text = seg.text.trim();
      if (!text) continue;
      winSegments.push({ start: seg.start + offsetSec, end: seg.end + offsetSec, text });
    }
    if (parsed.text.trim()) winText = parsed.text.trim();
    segments.push(...winSegments);
    if (winText) texts.push(winText);
  }

  onProgress(1, "done");
  return { segments, text: texts.join(" ").trim() };
}
