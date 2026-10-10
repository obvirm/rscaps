import { decodeAudioTo16kMono } from "/src/lib/audio.ts";

// Capture all console.log output (the wasm logs encoder shape via console.log).
window.__logs = [];
const _origLog = console.log.bind(console);
console.log = (...a) => {
  window.__logs.push(a.map(String).join(" "));
  _origLog(...a);
};
const _origErr = console.error.bind(console);
console.error = (...a) => {
  window.__logs.push("ERR: " + a.map(String).join(" "));
  _origErr(...a);
};
window.addEventListener("error", (e) => {
  window.__logs.push("WINDOW_ERROR: " + (e.message || e.error) + " @ " + (e.filename || "") + ":" + (e.lineno || ""));
});
window.addEventListener("unhandledrejection", (e) => {
  window.__logs.push("UNHANDLED: " + String(e.reason && e.reason.stack ? e.reason.stack : e.reason));
});

const outEl = document.getElementById("out");
const lines = [];
function log(msg, cls) {
  lines.push(cls ? `<span class="${cls}">${msg}</span>` : msg);
  outEl.innerHTML = lines.join("\n");
  console.log(msg);
}

async function main() {
  try {
    const pkgUrl = "/wpkg/whisper_spike.js?cb=" + Date.now();
    const wasm = await import(/* @vite-ignore */ pkgUrl);
    await wasm.default("/wpkg/whisper_spike_bg.wasm?cb=" + Date.now());
    log("wasm module loaded");
    await wasm.init();
    log("ort-web backend init ok");

    // Under crossOriginIsolated (COEP), onnxruntime-web's threaded build uses
    // SharedArrayBuffer but spawns navigator.hardwareConcurrency workers, which
    // can hang during session init under the Vite dev server. Force 1 thread.
    try {
      if (window.ort && window.ort.env && window.ort.env.wasm) {
        window.ort.env.wasm.numThreads = 1;
        log("set ort.env.wasm.numThreads = 1");
      } else {
        log("ort.env.wasm not reachable (keys: " + (window.ort ? Object.keys(window.ort).join(",") : "no ort") + ")");
      }
    } catch (e) {
      log("numThreads set failed: " + e);
    }

    // Load models.
    const base = "/models/whisper-base/";
    const [encBytes, decBytes, tokJson] = await Promise.all([
      fetch(base + "encoder_model.onnx").then((r) => r.arrayBuffer()),
      fetch(base + "decoder_model_merged.onnx").then((r) => r.arrayBuffer()),
      fetch(base + "tokenizer.json").then((r) => r.text()),
    ]);
    log(`encoder ${(encBytes.byteLength / 1e6).toFixed(1)}MB  decoder ${(decBytes.byteLength / 1e6).toFixed(1)}MB  tokenizer ${(tokJson.length / 1e6).toFixed(2)}MB`);
    await wasm.load_encoder(new Uint8Array(encBytes));
    await wasm.load_decoder(new Uint8Array(decBytes));
    wasm.load_tokenizer(tokJson);
    log("encoder + decoder + tokenizer loaded");

    // Decode JFK speech.
    const blob = await (await fetch("/models/jfk.flac")).blob();
    const audio = await decodeAudioTo16kMono(blob);
    log(`jfk audio: ${audio.samples.length} samples (${audio.durationSec.toFixed(2)}s)`);

    // Encode.
    const t0 = performance.now();
    const hidden = await wasm.encode_audio(audio.samples);
    const tEnc = performance.now() - t0;
    log(`encoder: ${hidden.length} floats in ${tEnc.toFixed(0)}ms`);

    // Debug: inspect one prefill's input/output shapes.
    try {
      const dbg = await wasm.debug_prefill(hidden);
      log("DEBUG_PREFILL: " + dbg);
    } catch (e) {
      log("DEBUG_PREFILL_ERR: " + e);
    }

    // Transcribe WITHOUT timestamps (simplest correctness check).
    const EN = 50259n, TRANSCRIBE = 50359n;
    const dur = audio.durationSec;
    const t1 = performance.now();
    const rawNoTs = await wasm.transcribe(hidden, EN, TRANSCRIBE, false, 128, dur);
    const tNoTs = performance.now() - t1;
    const noTs = JSON.parse(rawNoTs);
    log(`--- no-timestamps (${tNoTs.toFixed(0)}ms) ---`);
    log(JSON.stringify(noTs));
    log(`TEXT: "${noTs.text}"`);

    // Transcribe WITH timestamps.
    const t2 = performance.now();
    const rawTs = await wasm.transcribe(hidden, EN, TRANSCRIBE, true, 128, dur);
    const tTs = performance.now() - t2;
    const ts = JSON.parse(rawTs);
    log(`--- timestamps (${tTs.toFixed(0)}ms) ---`);
    log(`TEXT: "${ts.text}"`);
    log(`TOKENS: ${JSON.stringify(ts.tokens)}`);
    log(`SEGMENTS (${ts.segments.length}):`);
    for (const s of ts.segments) log(`  [${s.start.toFixed(2)}→${s.end.toFixed(2)}] ${s.text}`);

    log("DONE");
    window.__encResult = { ok: true, noTs, ts, tEnc, tNoTs, tTs };
  } catch (e) {
    log("ERROR: " + (e && e.stack ? e.stack : e), "bad");
    window.__encResult = { ok: false, error: String(e) };
  }
}

main();
