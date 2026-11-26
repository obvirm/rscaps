import { env, pipeline } from "@huggingface/transformers";

type ModelKey = "tiny" | "base" | "small" | "medium" | "large" | "turbo";
type Backend = "webgpu" | "cpu";

type Request = {
  type: "transcribe" | "preload";
  id: number;
  samples?: Float32Array;
  model: ModelKey;
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
  medium: "onnx-community/whisper-large-v3-turbo_timestamped",
  large: "onnx-community/whisper-large-v3-turbo_timestamped",
  turbo: "onnx-community/whisper-large-v3-turbo_timestamped",
};

env.allowLocalModels = false;
env.useBrowserCache = true;

let cache: { key: string; pipe: Pipe } | null = null;

function repoFor(model: ModelKey): string {
  return REPOS[model];
}

async function loadPipe(req: Request): Promise<Pipe> {
  const repo = repoFor(req.model);
  if (req.backend === "webgpu" && !(navigator as Navigator & { gpu?: unknown }).gpu) {
    throw new Error("WebGPU tidak tersedia di browser ini; pilih mode CPU secara eksplisit.");
  }
  const key = `${repo}:${req.backend}`;
  if (cache?.key === key) return cache.pipe;

  let lastProgress = -1;
  const progress = (p: { status?: string; progress?: number; file?: string }) => {
    const fraction =
      typeof p.progress === "number" ? Math.max(0, Math.min(1, p.progress / 100)) : 0;
    if (fraction < 1 && fraction - lastProgress < 0.02) return;
    lastProgress = fraction;
    self.postMessage({ type: "progress", id: req.id, phase: p.file ?? p.status ?? "model", fraction });
  };

  const options =
    req.backend === "webgpu"
      ? ({
          device: "webgpu",
          dtype: { encoder_model: "fp32", decoder_model_merged: "fp16" },
          progress_callback: progress,
        } as const)
      : ({
          device: "wasm",
          dtype: { encoder_model: "q8", decoder_model_merged: "q8" },
          progress_callback: progress,
        } as const);

  const loaded = (await pipeline("automatic-speech-recognition", repo, options)) as unknown as Pipe;
  cache = { key, pipe: loaded };
  return loaded;
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
  const pipe = await loadPipe(req);
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
    language: req.language,
    task: "transcribe",
    return_timestamps: "word",
    chunk_length_s: 30,
    stride_length_s: 5,
    max_new_tokens: 448,
  });
  const chunks = output.chunks ?? [];
  const { words, segments } = segmentsFromWords(chunks);
  self.postMessage({
    type: "done",
    id: req.id,
    result: {
      text: output.text.trim(),
      segments,
      words,
      timings: { backend: req.backend, pure: req.backend === "webgpu" ? 1 : 0 },
    },
  });
}

self.onmessage = async (ev: MessageEvent<Request>) => {
  const req = ev.data;
  if (!req || (req.type !== "transcribe" && req.type !== "preload")) return;
  try {
    await run(req);
  } catch (e) {
    self.postMessage({ type: "error", id: req.id, error: String(e) });
  }
};
