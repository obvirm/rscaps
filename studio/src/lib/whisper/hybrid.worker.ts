import * as ortImport from "onnxruntime-web/webgpu";
import whisperModels from "../../../../config/whisper-models.json";

type OrtTensor = {
  data?: Float32Array | BigInt64Array | Uint8Array;
  dims: readonly number[];
  location?: string;
  gpuBuffer?: unknown;
  getData?: (release?: boolean) => Promise<Float32Array | BigInt64Array | Uint8Array>;
};
type OrtSession = {
  inputNames: string[];
  outputNames: string[];
  run(
    feeds: Record<string, OrtTensor>,
    fetches?: string[] | Record<string, null>,
  ): Promise<Record<string, OrtTensor>>;
  release?: () => void;
};
type OrtApi = {
  env: { wasm: { wasmPaths: string; numThreads: number } };
  Tensor: new (type: string, data: unknown, dims: number[]) => OrtTensor;
  InferenceSession: {
    create(bytes: Uint8Array, opts: unknown): Promise<OrtSession>;
  };
};
const ort = ortImport as unknown as OrtApi;

type Core = {
  default: (wasmUrl: string) => Promise<unknown>;
  mel_from_audio: (samples: Float32Array, nMels: number) => Float32Array;
  set_arch: (...args: unknown[]) => void;
  process_logits: (
    logits: Float32Array,
    generated: Int32Array,
    timestamps: boolean,
    duration: number,
  ) => Float32Array;
  ends_in_repetition: (generated: Int32Array) => boolean;
  dtw_token_times: (
    weights: Float32Array,
    nTokens: number,
    nHeads: number,
    nFrames: number,
  ) => Float32Array;
  words_json: (
    tokenizerJson: string,
    ids: Int32Array,
    starts: Float32Array,
    ends: Float32Array,
  ) => string;
  segments_json: (tokenizerJson: string, ids: Int32Array, timestamps: boolean) => string;
};

type ModelConfig = {
  layers: number;
  decLayers: number;
  heads: number;
  dModel: number;
  nMels: number;
  classic: boolean;
  splitDecoder?: boolean;
  repo: string;
  alignmentHeads?: number[][];
};

type Request = {
  type: "transcribe";
  samples: Float32Array;
  model: keyof typeof whisperModels.models;
  quant: string;
  langId: number;
  duration: number;
  remoteModels?: boolean;
};

let remoteModels = false;

type SessionCache = {
  key: string;
  encoder: { session: OrtSession; backend: string };
  pre: { session: OrtSession; backend: string } | null;
  past: { session: OrtSession; backend: string } | null;
  tokenizerJson: string;
};
let sessionCache: SessionCache | null = null;


type Segment = { start: number; end: number; text: string };
type Word = { text: string; start_s: number; end_s: number };

const SAMPLE_RATE = 16000;
const WINDOW_SAMPLES = 30 * SAMPLE_RATE;

let core: Core | null = null;
self.postMessage({ type: "boot" });

async function loadCore(): Promise<Core> {
  if (core) return core;
  const pkgUrl = "/wcorepkg/whisper_wasm.js";
  const mod = (await import(/* @vite-ignore */ pkgUrl)) as Core;
  await mod.default("/wcorepkg/whisper_wasm_bg.wasm");
  core = mod;
  return mod;
}

const modelCache = "rscaps-whisper-models";

async function fetchBytes(
  url: string,
  onProgress?: (f: number) => void,
  useCache = true,
): Promise<Uint8Array> {
  const cache = useCache ? await caches.open(modelCache) : null;
  const hit = cache ? await cache.match(url) : undefined;
  if (hit) return new Uint8Array(await hit.arrayBuffer());
  const res = await fetch(url);
  if (!res.ok) throw new Error(`fetch ${res.status}: ${url}`);
  const total = Number(res.headers.get("content-length")) || 0;
  if (!res.body) {
    const bytes = new Uint8Array(await res.arrayBuffer());
    onProgress?.(1);
    return bytes;
  }
  const reader = res.body.getReader();
  if (total > 0) {
    const out = new Uint8Array(total);
    let offset = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (offset + value.length > total) break;
      out.set(value, offset);
      offset += value.length;
      onProgress?.(offset / total);
    }
    if (offset < total) {
      const short = out.subarray(0, offset);
      return short;
    }
    if (cache) {
      try {
        await cache.put(
          url,
          new Response(out, { headers: { "content-type": "application/octet-stream" } }),
        );
      } catch {
        /* cache is an optimization */
      }
    }
    return out;
  }
  const parts: Uint8Array[] = [];
  let loaded = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    loaded += value.length;
  }
  const out = new Uint8Array(loaded);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function localDir(model: string, quant: string): string {
  return `/models/whisper-${model}-${quant}`;
}

function hfUrl(repo: string, file: string, root = false): string {
  return `https://huggingface.co/${repo}/resolve/main/${root ? "" : "onnx/"}${file}`;
}

async function fetchModelExt(
  model: string,
  quant: string,
  base: string,
  onProgress?: (f: number) => void,
): Promise<{ pb: Uint8Array; data: Uint8Array } | null> {
  const dir = localDir(model, quant);
  try {
    const res = await fetch(`${dir}/${base}.ext.onnx`);
    if (!res.ok) return null;
    const type = res.headers.get("content-type") || "";
    if (!type.includes("octet-stream") && !type.includes("onnx")) return null;
    const pb = await readResponse(res, (f) => onProgress?.(f * 0.1));
    const dres = await fetch(`${dir}/${base}.ext.data`);
    if (!dres.ok) return null;
    const data = await readResponse(dres, (f) => onProgress?.(0.1 + f * 0.9));
    return { pb, data };
  } catch {
    return null;
  }
}

async function fetchModelFile(
  model: string,
  quant: string,
  cfg: ModelConfig,
  file: string,
  onProgress?: (f: number) => void,
): Promise<Uint8Array> {
  const local = `${localDir(model, quant)}/${file}`;
  if (!remoteModels) {
    try {
      const res = await fetch(local);
      if (res.ok) {
        const type = res.headers.get("content-type") || "";
        if (type.includes("octet-stream") || type.includes("onnx")) {
          return await readResponse(res, onProgress);
        }
      }
    } catch {
      /* fall through to HuggingFace */
    }
  }
  return fetchBytes(hfUrl(cfg.repo, file), onProgress, true);
}

async function readResponse(res: Response, onProgress?: (f: number) => void): Promise<Uint8Array> {
  const total = Number(res.headers.get("content-length")) || 0;
  if (!res.body) return new Uint8Array(await res.arrayBuffer());
  const reader = res.body.getReader();
  if (total > 0) {
    const out = new Uint8Array(total);
    let offset = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (offset + value.length > total) break;
      out.set(value, offset);
      offset += value.length;
      onProgress?.(offset / total);
    }
    return offset === total ? out : out.subarray(0, offset);
  }
  const parts: Uint8Array[] = [];
  let loaded = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    loaded += value.length;
  }
  const out = new Uint8Array(loaded);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

async function fetchTokenizer(
  model: string,
  quant: string,
  cfg: ModelConfig,
): Promise<string> {
  const local = `${localDir(model, "q8")}/tokenizer.json`;
  if (!remoteModels) {
    try {
      const res = await fetch(local);
      if (res.ok) return await res.text();
    } catch {
      /* fall through */
    }
  }
  const res = await fetch(hfUrl(cfg.repo, "tokenizer.json", true));
  if (!res.ok) throw new Error(`tokenizer ${res.status}`);
  return await res.text();
}

function makeTensor(
  type: "float32" | "int64" | "bool",
  data: Float32Array | BigInt64Array | Uint8Array,
  dims: number[],
): OrtTensor {
  return new ort.Tensor(type, data, dims);
}

function kv(out: Record<string, OrtTensor>, name: string): OrtTensor {
  return out[name];
}

function graphOptLevel(model: string, quant: string): string {
  // fp16/q4f16 hit ORT #26717 (SimplifiedLayerNormFusion crash), so they use
  // Basic. small QDQ keeps Basic; other QDQ sizes run the fused graph.
  if (quant === "fp16" || quant === "q4f16") return "basic";
  if (quant === "q8" && model === "small") return "basic";
  return "all";
}

async function createSession(
  bytes: Uint8Array,
  opts: Record<string, unknown>,
): Promise<{ session: OrtSession; backend: string }> {
  const requested = (opts.executionProviders as string[] | undefined) ?? ["webgpu", "wasm"];
  if (!requested.includes("webgpu")) {
    return { session: await ort.InferenceSession.create(bytes, opts), backend: "wasm" };
  }
  const gpu = {
    ...opts,
    executionProviders: requested,
    preferredOutputLocation: "gpu-buffer",
  };
  try {
    return { session: await ort.InferenceSession.create(bytes, gpu), backend: requested.join(",") };
  } catch (e) {
    // A graph/kernel WebGPU failure must not strand the run on a dead EP.
    console.warn("[hybrid] webgpu session failed, falling back to wasm:", String(e).slice(0, 200));
    const { preferredOutputLocation: _ignored, ...rest } = opts as Record<string, unknown>;
    const cpu = { ...rest, executionProviders: ["wasm"] };
    return { session: await ort.InferenceSession.create(bytes, cpu), backend: "wasm" };
  }
}

function trimRepeatedWords(words: Word[]): number {
  for (let i = 0; i < words.length; i++) {
    if (/(.)\1{5,}/u.test(words[i].text)) return i;
  }
  const norm = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
  const n = words.map((w) => norm(w.text));
  for (let start = 0; start < n.length; start++) {
    for (let period = 1; period <= 16; period++) {
      if (start + period * 3 > n.length) continue;
      let repeats = 1;
      let pos = start;
      while (pos + period * 2 <= n.length) {
        let same = true;
        for (let k = 0; k < period; k++) {
          if (n[pos + k] !== n[pos + period + k]) {
            same = false;
            break;
          }
        }
        if (!same) break;
        repeats++;
        pos += period;
      }
      if (repeats >= 3 && pos + period >= n.length - 2) {
        return start === 0 ? period : start;
      }
    }
  }
  return words.length;
}

type WindowResult = {
  segments: Segment[];
  words: Word[];
  timings: { encode: number; decode: number; dtw: number; align: number; steps: number };
};

async function tensorData(
  t: OrtTensor,
): Promise<Float32Array | BigInt64Array | Uint8Array | undefined> {
  if (t.location === "gpu-buffer" || t.location === "ml-tensor") {
    return t.getData ? await t.getData() : undefined;
  }
  return t.data;
}

function wordsToSegments(words: Word[]): Segment[] {
  const out: Segment[] = [];
  let cur: Word[] = [];
  const flush = () => {
    if (!cur.length) return;
    out.push({
      start: cur[0].start_s,
      end: cur[cur.length - 1].end_s,
      text: cur.map((w) => w.text).join(" ").trim(),
    });
    cur = [];
  };
  for (const w of words) {
    if (
      cur.length &&
      (w.start_s - cur[cur.length - 1].end_s > 0.8 ||
        cur.length >= 9 ||
        w.end_s - cur[0].start_s > 7)
    ) {
      flush();
    }
    cur.push(w);
  }
  flush();
  return out.filter((s) => s.text.length > 0);
}

async function collectCrossAttention(
  session: OrtSession,
  encTensor: OrtTensor,
  prompt: number[],
  generated: number[],
  textIds: number[],
  alignment: number[][],
  nFrames: number,
  cfg: ModelConfig,
  split: boolean,
): Promise<Float32Array | null> {
  if (!alignment.length || !textIds.length) return null;
  const inputIds = [...prompt, ...generated];
  const textPositions: number[] = [];
  for (let i = 0; i < generated.length; i++) {
    if (generated[i] < 50257) textPositions.push(prompt.length + i);
  }
  if (textPositions.length < textIds.length) return null;
  const empty = (): OrtTensor =>
    makeTensor("float32", new Float32Array(0), [1, cfg.heads, 0, whisperModels.headDim]);
  const feeds: Record<string, OrtTensor> = {
    input_ids: makeTensor(
      "int64",
      BigInt64Array.from(inputIds.map((v) => BigInt(v))),
      [1, inputIds.length],
    ),
    encoder_hidden_states: encTensor,
  };
  if (!split) {
    feeds.use_cache_branch = makeTensor("bool", new Uint8Array([0]), [1]);
    for (let i = 0; i < cfg.decLayers; i++) {
      feeds[`past_key_values.${i}.decoder.key`] = empty();
      feeds[`past_key_values.${i}.decoder.value`] = empty();
      feeds[`past_key_values.${i}.encoder.key`] = empty();
      feeds[`past_key_values.${i}.encoder.value`] = empty();
    }
  }
  const out = await session.run(
    feeds,
    alignment.map(([layer]) => `cross_attentions.${layer}`),
  );
  const layerData = new Map<number, Float32Array>();
  const weights = new Float32Array(textIds.length * alignment.length * nFrames);
  let write = 0;
  for (let t = 0; t < textIds.length; t++) {
    const pos = textPositions[t];
    for (const [layer, head] of alignment) {
      let src = layerData.get(layer);
      if (!src) {
        const attn = out[`cross_attentions.${layer}`] as OrtTensor | undefined;
        if (!attn) return null;
        const data = (await tensorData(attn)) as Float32Array | undefined;
        if (!data) return null;
        src = data;
        layerData.set(layer, src);
      }
      const attn = out[`cross_attentions.${layer}`] as OrtTensor;
      const ad = attn.dims as number[];
      const frames = ad[3];
      const base = (head * ad[2] + pos) * frames;
      const take = Math.min(nFrames, frames);
      for (let f = 0; f < take; f++) weights[write++] = src[base + f];
      for (let f = take; f < nFrames; f++) weights[write++] = 0;
    }
  }
  return weights;
}

function hasSpeechEnergy(samples: Float32Array): boolean {
  const frame = 1600;
  let loud = 0;
  for (let off = 0; off + frame <= samples.length; off += frame) {
    let sum = 0;
    for (let i = off; i < off + frame; i++) sum += samples[i] * samples[i];
    if (Math.sqrt(sum / frame) > 0.01) loud++;
  }
  return loud >= Math.max(2, Math.floor(samples.length / frame / 40));
}

function isSparseHallucination(text: string): boolean {
  const t = text.trim().toLowerCase();
  if (!t) return true;
  return (
    /^(terima kasih|sampai jumpa|thank(s| you)|please subscribe)/.test(t) ||
    /sub(title)? (indo|indonesia|by)|sub by|diterjemahkan oleh/.test(t) ||
    /^by\s+[a-z0-9_.-]+[:.]?$/i.test(t)
  );
}

async function rescueSparseWindow(
  win: Float32Array,
  initial: WindowResult,
  cfg: ModelConfig,
  c: Core,
  encoder: OrtSession,
  decoder: OrtSession,
  decoderPast: OrtSession | null,
  tokenizerJson: string,
  langId: number,
  decoderGpu: boolean,
): Promise<WindowResult> {
  const sliceSamples = Math.round(SAMPLE_RATE * 4);
  const segments: Segment[] = [];
  const words: Word[] = [];
  const timings = { encode: 0, decode: 0, dtw: 0, align: 0, steps: 0 };
  for (let off = 0; off < win.length; off += sliceSamples) {
    const slice = win.subarray(off, Math.min(off + sliceSamples, win.length));
    if (slice.length < SAMPLE_RATE / 2) break;
    const sec = off / SAMPLE_RATE;
    const part = await runWindow(
      slice,
      cfg,
      c,
      encoder,
      decoder,
      decoderPast,
      tokenizerJson,
      langId,
      slice.length / SAMPLE_RATE,
      decoderGpu,
    );
    for (const seg of part.segments) {
      if (isSparseHallucination(seg.text)) continue;
      segments.push({ start: seg.start + sec, end: seg.end + sec, text: seg.text });
    }
    for (const word of part.words) {
      words.push({ text: word.text, start_s: word.start_s + sec, end_s: word.end_s + sec });
    }
    timings.encode += part.timings.encode;
    timings.decode += part.timings.decode;
    timings.dtw += part.timings.dtw;
    timings.align += part.timings.align;
    timings.steps += part.timings.steps;
  }
  const keptWords = words.filter((w) => {
    const mid = (w.start_s + w.end_s) / 2;
    return segments.some((seg) => mid >= seg.start - 0.05 && mid <= seg.end + 0.05);
  });
  const better =
    keptWords.length > initial.words.length ||
    segments.length > initial.segments.length;
  return better ? { segments, words: keptWords, timings } : initial;
}

async function runWindow(
  win: Float32Array,
  cfg: ModelConfig,
  c: Core,
  encoder: OrtSession,
  decoder: OrtSession | null,
  decoderPast: OrtSession | null,
  tokenizerJson: string,
  langId: number,
  windowDur: number,
  decoderGpu: boolean,
): Promise<WindowResult> {
  const tEnc = performance.now();
  const mel = c.mel_from_audio(win, cfg.nMels);
  const melName = encoder.inputNames[0];
  const encOut = await encoder.run({
    [melName]: makeTensor("float32", mel, [1, cfg.nMels, 3000]),
  });
  const hidden = encOut[encoder.outputNames[0]] as OrtTensor;
  let encTensor: OrtTensor;
  if (hidden.location === "gpu-buffer" && decoderGpu) {
    encTensor = hidden;
  } else {
    const hiddenData = await tensorData(hidden);
    if (!hiddenData) throw new Error("encoder output has no data");
    encTensor = makeTensor("float32", hiddenData as Float32Array, [...hidden.dims]);
  }
  const encMs = performance.now() - tEnc;

  const task = cfg.classic ? 50359 : 50360;
  const tsBegin = cfg.classic ? 50364 : 50365;
  const eos = 50257;
  const vocab = whisperModels.vocab;
  const heads = cfg.heads;
  const headDim = whisperModels.headDim;
  const alignment = cfg.alignmentHeads ?? [];
  const prompt = [50258, langId, task];
  const generated: number[] = [];
  const textIds: number[] = [];
  const nFrames = Math.min(1500, Math.max(1, Math.ceil(windowDur / 0.02)));
  const empty = (): OrtTensor =>
    makeTensor("float32", new Float32Array(0), [1, heads, 0, headDim]);
  let pastDecK: OrtTensor[] = Array.from({ length: cfg.decLayers }, empty);
  let pastDecV: OrtTensor[] = Array.from({ length: cfg.decLayers }, empty);
  let pastEncK: OrtTensor[] = Array.from({ length: cfg.decLayers }, empty);
  let pastEncV: OrtTensor[] = Array.from({ length: cfg.decLayers }, empty);
  let ids = prompt;
  let useCache = false;
  let lastTsSec = 0;
  const tDec = performance.now();
  const maxSteps = Math.min(whisperModels.maxTokens, Math.max(32, Math.ceil(windowDur * 8) + 8));

  for (let step = 0; step < maxSteps; step++) {
    let out: Record<string, OrtTensor>;
    if (decoderPast && decoder) {
      const splitFeeds: Record<string, OrtTensor> = {
        input_ids: makeTensor(
          "int64",
          BigInt64Array.from(ids.map((v) => BigInt(v))),
          [1, ids.length],
        ),
      };
      if (!useCache) {
        splitFeeds.encoder_hidden_states = encTensor;
        out = await decoder.run(splitFeeds, [
          "logits",
          ...Array.from({ length: cfg.decLayers }, (_, i) => [
            `present.${i}.decoder.key`,
            `present.${i}.decoder.value`,
            `present.${i}.encoder.key`,
            `present.${i}.encoder.value`,
          ]).flat(),
        ]);
      } else {
        for (let i = 0; i < cfg.decLayers; i++) {
          splitFeeds[`past_key_values.${i}.decoder.key`] = pastDecK[i];
          splitFeeds[`past_key_values.${i}.decoder.value`] = pastDecV[i];
          splitFeeds[`past_key_values.${i}.encoder.key`] = pastEncK[i];
          splitFeeds[`past_key_values.${i}.encoder.value`] = pastEncV[i];
        }
        out = await decoderPast.run(splitFeeds, [
          "logits",
          ...Array.from({ length: cfg.decLayers }, (_, i) => [
            `present.${i}.decoder.key`,
            `present.${i}.decoder.value`,
          ]).flat(),
        ]);
      }
    } else {
      if (!decoder) throw new Error("merged decoder session missing");
      const feeds: Record<string, OrtTensor> = {
        input_ids: makeTensor(
          "int64",
          BigInt64Array.from(ids.map((v) => BigInt(v))),
          [1, ids.length],
        ),
        encoder_hidden_states: encTensor,
        use_cache_branch: makeTensor("bool", new Uint8Array([useCache ? 1 : 0]), [1]),
      };
      for (let i = 0; i < cfg.decLayers; i++) {
        feeds[`past_key_values.${i}.decoder.key`] = pastDecK[i];
        feeds[`past_key_values.${i}.decoder.value`] = pastDecV[i];
        feeds[`past_key_values.${i}.encoder.key`] = pastEncK[i];
        feeds[`past_key_values.${i}.encoder.value`] = pastEncV[i];
      }
      out = await decoder.run(feeds, [
        "logits",
        ...Array.from({ length: cfg.decLayers }, (_, i) => [
          `present.${i}.decoder.key`,
          `present.${i}.decoder.value`,
          ...(useCache
            ? []
            : [`present.${i}.encoder.key`, `present.${i}.encoder.value`]),
        ]).flat(),
      ]);
    }
    const logits = out.logits as OrtTensor;
    const logitsData = (await tensorData(logits)) as Float32Array | undefined;
    if (!logitsData) throw new Error("logits output has no data");
    const dims = logits.dims as number[];
    const seq = dims[1];
    const row = logitsData.slice((seq - 1) * vocab, seq * vocab);
    const processed = c.process_logits(row, Int32Array.from(generated), true, windowDur);
    let best = 0;
    let bestV = -Infinity;
    for (let i = 0; i < vocab; i++) {
      const v = processed[i];
      if (v > bestV) {
        bestV = v;
        best = i;
      }
    }

    if (best < eos) {
      textIds.push(best);
    }

    if (best === eos) break;
    generated.push(best);
    if (generated.length >= 8 && c.ends_in_repetition(Int32Array.from(generated))) break;
    if (best >= tsBegin) lastTsSec = Math.max(lastTsSec, (best - tsBegin) * 0.02);
    if (
      best >= tsBegin &&
      generated.length >= 2 &&
      generated[generated.length - 2] >= tsBegin &&
      (best - tsBegin) * 0.02 >= windowDur
    ) {
      break;
    }
    for (let i = 0; i < cfg.decLayers; i++) {
      if (out[`present.${i}.decoder.key`]) {
        pastDecK[i] = kv(out, `present.${i}.decoder.key`);
        pastDecV[i] = kv(out, `present.${i}.decoder.value`);
      }
      if (!useCache && out[`present.${i}.encoder.key`]) {
        pastEncK[i] = kv(out, `present.${i}.encoder.key`);
        pastEncV[i] = kv(out, `present.${i}.encoder.value`);
      }
    }
    ids = [best];
    useCache = true;
  }
  const decMs = performance.now() - tDec;

  const outGenerated = generated;
  const outTextIds = textIds;
  const trimmedLastTs = lastTsSec;
  const tAlign = performance.now();
  let outWeights: Float32Array | null = null;
  try {
    outWeights = await collectCrossAttention(
      decoder ?? decoderPast!,
      encTensor,
      prompt,
      generated,
      textIds,
      alignment,
      nFrames,
      cfg,
      !!decoderPast,
    );
  } catch (e) {
    console.warn("[hybrid] cross-attention alignment failed:", String(e).slice(0, 200));
  }
  const alignMs = performance.now() - tAlign;

  const tDtw = performance.now();
  const ids32 = Int32Array.from(outGenerated);
  const seg = JSON.parse(c.segments_json(tokenizerJson, ids32, true)) as {
    text: string;
    segments: Segment[];
  };
  let segments = seg.segments.map((s) => ({
    start: Math.max(0, Math.min(windowDur, s.start)),
    end: Math.max(0, Math.min(windowDur, s.end)),
    text: s.text,
  }));
  let words: Word[] = [];
  const usedFrames = Math.min(
    nFrames,
    Math.max(1, Math.ceil((trimmedLastTs > 0.1 ? trimmedLastTs : windowDur) / 0.02)),
  );
  if (
    outTextIds.length &&
    alignment.length &&
    outWeights &&
    outWeights.length === outTextIds.length * alignment.length * nFrames
  ) {
    let compact: Float32Array | number[] = outWeights;
    if (usedFrames !== nFrames) {
      compact = [];
      for (let t = 0; t < outTextIds.length; t++) {
        for (let h = 0; h < alignment.length; h++) {
          const base = (t * alignment.length + h) * nFrames;
          for (let f = 0; f < usedFrames; f++) compact.push(outWeights[base + f]);
        }
      }
    }
    const times = c.dtw_token_times(
      Float32Array.from(compact),
      outTextIds.length,
      alignment.length,
      usedFrames,
    );
    const starts = new Float32Array(outTextIds.length);
    const ends = new Float32Array(outTextIds.length);
    for (let i = 0; i < outTextIds.length; i++) {
      starts[i] = times[i * 2];
      ends[i] = times[i * 2 + 1];
    }
    words = JSON.parse(c.words_json(tokenizerJson, Int32Array.from(outTextIds), starts, ends));
    for (let i = 0; i < words.length; i++) {
      words[i].start_s = Math.max(0, Math.min(windowDur, words[i].start_s));
      words[i].end_s = Math.max(0, Math.min(windowDur, words[i].end_s));
      if (i + 1 < words.length) {
        words[i].end_s = Math.min(words[i].end_s, words[i + 1].start_s);
        words[i].end_s = Math.max(words[i].start_s, words[i].end_s);
      }
    }
    const keepWords = trimRepeatedWords(words);
    if (keepWords < words.length) {
      words = words.slice(0, keepWords);
      const rebuilt: Segment[] = [];
      for (const seg of segments) {
        const inSeg = words.filter((w) => {
          const mid = (w.start_s + w.end_s) / 2;
          return mid >= seg.start - 0.05 && mid <= seg.end + 0.05;
        });
        if (!inSeg.length) continue;
        rebuilt.push({
          start: inSeg[0].start_s,
          end: inSeg[inSeg.length - 1].end_s,
          text: inSeg.map((w) => w.text).join(" ").trim(),
        });
      }
      if (keepWords === 0) {
        segments = [];
      } else if (rebuilt.length) {
        segments = rebuilt;
      }
    }
    if (words.length) segments = wordsToSegments(words);
  }
  const dtwMs = performance.now() - tDtw;
  return {
    segments,
    words,
    timings: {
      encode: Math.round(encMs),
      decode: Math.round(decMs),
      dtw: Math.round(dtwMs),
      align: Math.round(alignMs),
      steps: outGenerated.length,
    },
  };
}

async function transcribe(req: Request) {
  const cfg = whisperModels.models[req.model] as ModelConfig;
  if (!cfg) throw new Error(`unknown model ${req.model}`);
  const alignment = cfg.alignmentHeads ?? [];
  if (!alignment.length) throw new Error(`alignmentHeads missing for ${req.model}`);

  const t0 = performance.now();
  const c = await loadCore();
  c.set_arch(cfg.layers, cfg.decLayers, cfg.heads, whisperModels.headDim, cfg.dModel, whisperModels.vocab, cfg.nMels, cfg.classic);
  const coreMs = performance.now() - t0;

  self.postMessage({ type: "progress", phase: "model", fraction: 0.02 });
  const computeQuant = req.quant === "q8" ? "q4" : req.quant;
  const split = cfg.splitDecoder === true && computeQuant !== "q4" &&
    (req.quant === "q8" || req.quant === "int8");
  const cacheKey = `${req.model}:${computeQuant}:${split}:${remoteModels}`;
  let fetchMs = 0;
  let sessMs = 0;
  let encSession: { session: OrtSession; backend: string };
  let preSession: { session: OrtSession; backend: string } | null;
  let pastSession: { session: OrtSession; backend: string } | null;
  let tokenizerJson: string;
  if (sessionCache?.key === cacheKey) {
    encSession = sessionCache.encoder;
    preSession = sessionCache.pre;
    pastSession = sessionCache.past;
    tokenizerJson = sessionCache.tokenizerJson;
  } else {
    sessionCache?.encoder.session.release?.();
    sessionCache?.pre?.session.release?.();
    sessionCache?.past?.session.release?.();
    sessionCache = null;
    const tFetch = performance.now();
    const encFile = remoteModels
      ? `encoder_model${whisperModels.encoderSuffix}.onnx`
      : "encoder_model.onnx";
    const encBytes = await fetchModelFile(req.model, "q8", cfg, encFile, (f) =>
      self.postMessage({ type: "progress", phase: "encoder", fraction: 0.02 + f * 0.16 }),
    );
    const suffix = (whisperModels.quantSuffix as Record<string, string>)[req.quant] ?? "";
    let decBytes: Uint8Array | null = null;
    let pastBytes: Uint8Array | null = null;
    let decExt: { pb: Uint8Array; data: Uint8Array } | null = null;
    if (split) {
      decBytes = await fetchModelFile(
        req.model,
        req.quant,
        cfg,
        `decoder_model${suffix}.onnx`,
        (f) => self.postMessage({ type: "progress", phase: "decoder", fraction: 0.18 + f * 0.18 }),
      );
      pastBytes = await fetchModelFile(
        req.model,
        req.quant,
        cfg,
        `decoder_with_past_model${suffix}.onnx`,
        (f) => self.postMessage({ type: "progress", phase: "decoder", fraction: 0.36 + f * 0.18 }),
      );
    } else {
      decExt = await fetchModelExt(req.model, req.quant, "decoder_model_merged", (f) =>
        self.postMessage({ type: "progress", phase: "decoder", fraction: 0.18 + f * 0.32 }),
      );
      decBytes =
        decExt?.pb ??
        (await fetchModelFile(
          req.model,
          req.quant,
          cfg,
          "decoder_model_merged.onnx",
          (f) => self.postMessage({ type: "progress", phase: "decoder", fraction: 0.18 + f * 0.32 }),
        ));
    }
    tokenizerJson = await fetchTokenizer(req.model, req.quant, cfg);
    fetchMs = performance.now() - tFetch;

    const opts: Record<string, unknown> = {
      graphOptimizationLevel: graphOptLevel(req.model, computeQuant),
      executionProviders: ["webgpu"],
      enableMemPattern: false,
      enableCpuMemArena: false,
    };
    const decoderOpts: Record<string, unknown> = {
      ...opts,
      ...(decExt
        ? {
            graphOptimizationLevel: "disabled",
            externalData: [
              { path: "decoder_model_merged.weights.data", data: decExt.data },
            ],
          }
        : {}),
    };
    const tSess = performance.now();
    encSession = await createSession(encBytes, opts);
    try {
      preSession = decBytes ? await createSession(decBytes, decoderOpts) : null;
      pastSession = pastBytes ? await createSession(pastBytes, decoderOpts) : null;
    } catch (e) {
      throw new Error(`decoder session: ${String(e)}`);
    }
    sessMs = performance.now() - tSess;
    sessionCache = {
      key: cacheKey,
      encoder: encSession,
      pre: preSession,
      past: pastSession,
      tokenizerJson,
    };
  }
  const backends = new Set<string>([encSession.backend]);
  if (preSession) backends.add(preSession.backend);
  if (pastSession) backends.add(pastSession.backend);
  if (split) {
    console.warn(
      "[hybrid] split cross-attn outputs",
      preSession?.session.outputNames.filter((n) => n.startsWith("cross_attentions")).length ?? 0,
      pastSession?.session.outputNames.filter((n) => n.startsWith("cross_attentions")).length ?? 0,
    );
  }
  let decSession = split ? null : preSession;

  const totalChunks = Math.max(1, Math.ceil(req.samples.length / WINDOW_SAMPLES));
  const segments: Segment[] = [];
  const words: Word[] = [];
  const timings: {
    core: number;
    fetch: number;
    sessions: number;
    encode: number;
    decode: number;
    dtw: number;
    align: number;
    steps: number;
    windows: number;
    backend: string;
    computeQuant: string;
  } = {
    core: Math.round(coreMs),
    fetch: Math.round(fetchMs),
    sessions: Math.round(sessMs),
    encode: 0,
    decode: 0,
    dtw: 0,
    align: 0,
    steps: 0,
    windows: totalChunks,
    backend: "",
    computeQuant,
  };
  const tRun = performance.now();
  for (let i = 0; i < totalChunks; i++) {
    const start = i * WINDOW_SAMPLES;
    const end = Math.min(start + WINDOW_SAMPLES, req.samples.length);
    const win = req.samples.subarray(start, end);
    const windowDur = Math.max(win.length / SAMPLE_RATE, 0.001);
    const offsetSec = start / SAMPLE_RATE;
    self.postMessage({
      type: "progress",
      phase: "transcribe",
      fraction: 0.52 + (i / totalChunks) * 0.45,
    });
    let out: WindowResult;
    try {
      out = await runWindow(
        win,
        cfg,
        c,
        encSession.session,
        split ? preSession!.session : decSession!.session,
        split ? pastSession!.session : null,
        tokenizerJson,
        req.langId,
        windowDur,
        (preSession?.backend ?? "").includes("webgpu"),
      );
    } catch (e) {
      throw new Error(`window ${i} decode: ${String(e)}`);
    }
    if (
      out.words.length < Math.max(2, windowDur * 0.4) &&
      hasSpeechEnergy(win)
    ) {
      const rescued = await rescueSparseWindow(
        win,
        out,
        cfg,
        c,
        encSession.session,
        split ? preSession!.session : decSession!.session,
        split ? pastSession!.session : null,
        tokenizerJson,
        req.langId,
        (preSession?.backend ?? "").includes("webgpu"),
      );
      out = rescued;
    }
    for (const seg of out.segments) {
      if (seg.text.trim()) {
        segments.push({ start: seg.start + offsetSec, end: seg.end + offsetSec, text: seg.text });
      }
    }
    for (const word of out.words) {
      if (word.text.trim()) {
        words.push({ text: word.text, start_s: word.start_s + offsetSec, end_s: word.end_s + offsetSec });
      }
    }
    timings.encode += out.timings.encode;
    timings.decode += out.timings.decode;
    timings.dtw += out.timings.dtw;
    timings.align += out.timings.align;
    timings.steps += out.timings.steps;
  }
  timings.encode = Math.round(timings.encode);
  timings.decode = Math.round(timings.decode);
  timings.dtw = Math.round(timings.dtw);
  timings.align = Math.round(timings.align);
  timings.sessions = Math.round(timings.sessions);
  timings.backend = [...backends].join(";");
  self.postMessage({ type: "progress", phase: "transcribe", fraction: 0.98 });
  const text = segments.map((s) => s.text).join(" ").trim();
  return { text, segments, words, timings: { ...timings, run: Math.round(performance.now() - tRun) } };
}

self.onmessage = async (ev: MessageEvent<Request>) => {
  if (ev.data?.type !== "transcribe") return;
  try {
    ort.env.wasm.wasmPaths = `${self.location.origin}/nm/onnxruntime-web/dist/`;
    ort.env.wasm.numThreads = Math.max(1, (navigator.hardwareConcurrency || 4) - 1);
    remoteModels = ev.data.remoteModels === true;
    const result = await transcribe(ev.data);
    self.postMessage({ type: "done", result });
  } catch (e) {
    self.postMessage({ type: "error", error: String(e) });
  }
};
