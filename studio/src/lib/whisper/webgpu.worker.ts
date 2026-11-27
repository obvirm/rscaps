import { env, pipeline, LogitsProcessor, LogitsProcessorList } from "@huggingface/transformers";
import type { Tensor } from "@huggingface/transformers";
import type { WhisperQuant } from "../types";

type ModelKey = "tiny" | "base" | "small" | "medium" | "large" | "turbo";
type Backend = "webgpu" | "cpu";

type Request = {
  type: "transcribe" | "preload";
  id: number;
  samples?: Float32Array;
  model: ModelKey;
  quant: WhisperQuant;
  backend: Backend;
  language: string;
};

type Pipe = (
  audio: Float32Array,
  options: Record<string, unknown>,
) => Promise<{ text: string; chunks?: { text: string; timestamp: [number, number] }[] }>;

const REPOS: Record<ModelKey, string> = {
  tiny: "onnx-community/whisper-tiny_timestamped",
  base: "onnx-community/whisper-base_timestamped",
  small: "onnx-community/whisper-small_timestamped",
  medium: "onnx-community/whisper-medium_timestamped",
  large: "onnx-community/whisper-large-v3-turbo_timestamped",
  turbo: "onnx-community/whisper-large-v3-turbo_timestamped",
};

const MAX_NEW_TOKENS = 444;

type TokenTimestamps = { dims: number[]; data: Float32Array };
type Extractor = (
  outputs: unknown,
  alignmentHeads: unknown,
  numFrames: unknown,
  timePrecision: number,
  inputIds: number,
) => TokenTimestamps;

const alignedModels = new WeakSet<object>();

function installTimestampAligner(model: { _extract_token_timestamps?: Extractor } | null): void {
  if (!model || typeof model._extract_token_timestamps !== "function" || alignedModels.has(model)) return;
  alignedModels.add(model);
  const original = model._extract_token_timestamps.bind(model);
  model._extract_token_timestamps = (outputs, heads, numFrames, precision, inputIds = 0) => {
    const frames = typeof numFrames === "number" ? Math.floor(numFrames / 2) : numFrames;
    const reading = original(outputs, heads, frames, precision, inputIds);
    if (inputIds < 1) return reading;
    const predicting = original(outputs, heads, frames, precision, inputIds - 1);
    const [batchSize = 1, length = 0] = reading.dims;
    if (length < 2) return reading;
    for (let batch = 0; batch < batchSize; batch++) {
      const read = reading.data.subarray(batch * length, (batch + 1) * length);
      const pred = predicting.data.subarray(batch * length, (batch + 1) * length);
      for (let i = inputIds; i < length - 1; i++) {
        const t = pred[i - 1]!;
        if (t !== 0) read[i] = (read[i]! + t) / 2;
      }
      read[length - 1] = pred[length - 2]!;
    }
    return reading;
  };
}

const MIN_FULL_PERIODS = 5;
const MIN_REPETITION_TOKENS = 50;

class LoopGuard {
  private fired = false;
  constructor(private readonly eos: number) {}
  apply(inputIds: bigint[][], logits: Tensor): Tensor {
    for (let b = 0; b < inputIds.length; b++) {
      if (!this.looping(inputIds[b] ?? [])) continue;
      this.fired = true;
      const row = (logits as unknown as { data: Float32Array }[])[b];
      if (row) {
        row.data.fill(-Infinity);
        row.data[this.eos] = 0;
      }
    }
    return logits;
  }
  consumeFired(): boolean {
    const v = this.fired;
    this.fired = false;
    return v;
  }
  private looping(seq: readonly bigint[]): boolean {
    const maxPeriod = Math.floor(seq.length / MIN_FULL_PERIODS);
    for (let period = 1; period <= maxPeriod; period++) {
      const window = Math.max(
        period * MIN_FULL_PERIODS,
        period * Math.ceil(MIN_REPETITION_TOKENS / period),
      );
      if (window > seq.length) continue;
      let periodic = true;
      for (let i = seq.length - window + period; i < seq.length; i++) {
        if (seq[i] !== seq[i - period]) {
          periodic = false;
          break;
        }
      }
      if (periodic) return true;
    }
    return false;
  }
}

class ReplacingList extends LogitsProcessorList {
  override push(item: LogitsProcessor): void {
    const existing = this.processors.findIndex((held) => held.constructor === item.constructor);
    if (existing >= 0) this.processors.splice(existing, 1);
    super.push(item);
  }
}

class GuardProcessor extends LogitsProcessor {
  constructor(private readonly guard: LoopGuard) {
    super();
  }
  _call(inputIds: bigint[][], logits: Tensor): Tensor {
    return this.guard.apply(inputIds, logits);
  }
}

function eosOf(pipe: unknown): number | null {
  const model = (pipe as { model?: { generation_config?: { eos_token_id?: unknown }; config?: { eos_token_id?: unknown } } }).model;
  const raw = model?.generation_config?.eos_token_id ?? model?.config?.eos_token_id;
  const id = Array.isArray(raw) ? raw[0] : raw;
  return typeof id === "number" ? id : null;
}

env.allowLocalModels = false;
env.useBrowserCache = true;

let cache: { key: string; pipe: Pipe; processors: LogitsProcessorList | null } | null = null;

async function loadPipe(req: Request): Promise<{ pipe: Pipe; processors: LogitsProcessorList | null }> {
  if (req.backend === "webgpu" && !(navigator as Navigator & { gpu?: unknown }).gpu) {
    throw new Error("WebGPU tidak tersedia di browser ini. Pilih mode CPU secara eksplisit.");
  }
  if (req.backend === "webgpu") {
    env.backends.onnx.wasm!.numThreads = 1;
  }
  const key = `${REPOS[req.model]}:${req.quant}:${req.backend}`;
  if (cache?.key === key) return { pipe: cache.pipe, processors: cache.processors };

  let lastProgress = -1;
  const progress = (p: { status?: string; progress?: number; file?: string }) => {
    const fraction =
      typeof p.progress === "number" ? Math.max(0, Math.min(1, p.progress / 100)) : 0;
    if (fraction < 1 && fraction - lastProgress < 0.02) return;
    lastProgress = fraction;
    self.postMessage({ type: "progress", id: req.id, phase: p.file ?? p.status ?? "model", fraction });
  };

  const loaded = (await pipeline("automatic-speech-recognition", REPOS[req.model], {
    device: req.backend === "webgpu" ? "webgpu" : "wasm",
    dtype:
      req.backend === "webgpu"
        ? { encoder_model: "q4", decoder_model_merged: "q4" }
        : "q8",
    session_options: { graphOptimizationLevel: "basic" },
    progress_callback: progress,
  } as const)) as unknown as Pipe;
  const eos = eosOf(loaded);
  const processors = eos === null ? null : new ReplacingList();
  if (processors && eos !== null) processors.push(new GuardProcessor(new LoopGuard(eos)));
  cache = { key, pipe: loaded, processors };
  return { pipe: loaded, processors };
}

function segmentsFromWords(chunks: { text: string; timestamp: [number, number] }[]) {
  const words = chunks
    .map((c) => ({
      text: c.text.trim(),
      start_s: c.timestamp?.[0] ?? 0,
      end_s: c.timestamp?.[1] ?? c.timestamp?.[0] ?? 0,
    }))
    .filter((w) => w.text && w.end_s >= w.start_s);
  const segments: { start: number; end: number; text: string }[] = [];
  let cur: typeof words = [];
  const flush = () => {
    if (!cur.length) return;
    segments.push({
      start: cur[0].start_s,
      end: cur[cur.length - 1].end_s,
      text: cur.map((w) => w.text).join(" ").trim(),
    });
    cur = [];
  };
  for (const w of words) {
    const prev = cur[cur.length - 1];
    if (prev && (w.start_s - prev.end_s > 0.8 || cur.length >= 9 || w.end_s - cur[0].start_s > 7)) {
      flush();
    }
    cur.push(w);
  }
  flush();
  return { words, segments };
}

async function run(req: Request) {
  const { pipe, processors } = await loadPipe(req);
  if (req.type === "preload") {
    await pipe(new Float32Array(16000), {
      language: req.language,
      task: "transcribe",
      max_new_tokens: 1,
      return_timestamps: false,
    });
    self.postMessage({ type: "done", id: req.id, result: { text: "", segments: [], words: [] } });
    return;
  }
  if (!req.samples) throw new Error("samples missing");
  self.postMessage({ type: "progress", id: req.id, phase: "transcribe", fraction: 0.55 });
  const output = await pipe(req.samples, {
    sampling_rate: 16000,
    language: req.language || null,
    task: "transcribe",
    return_timestamps: "word",
    chunk_length_s: 30,
    stride_length_s: 5,
    max_new_tokens: MAX_NEW_TOKENS,
  });
  const { words, segments } = segmentsFromWords(output.chunks ?? []);
  self.postMessage({
    type: "done",
    id: req.id,
    result: {
      text: output.text.trim(),
      segments,
      words,
      timings: { backend: req.backend, pure: 1 },
    },
  });
}

self.onmessage = async (ev: MessageEvent<Request>) => {
  const req = ev.data;
  if (!req || (req.type !== "transcribe" && req.type !== "preload")) return;
  try {
    await run(req);
  } catch (e) {
    self.postMessage({ type: "error", id: req.id, error: String((e as Error)?.stack ?? e) });
  }
};
