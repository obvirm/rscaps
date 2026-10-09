import initWasm, { init as ortBackendInit, load_model, encode } from "/wpkg/whisper_spike.js";

const logEl = document.getElementById("log");
function log(line) {
  logEl.textContent += line + "\n";
  console.log(line);
}
function fail(err) {
  logEl.classList.add("error");
  log("ERROR: " + (err && err.message ? err.message : err));
  console.error(err);
}

const N_MELS = 80;
const N_FRAMES = 3000;

async function main() {
  logEl.textContent = "";
  log(`crossOriginIsolated=${crossOriginIsolated}`);

  const t = (label) => {
    const t0 = performance.now();
    return () => `${label}: ${(performance.now() - t0).toFixed(0)} ms`;
  };

  let done = t("wasm module load");
  await initWasm({ module_or_path: "/wpkg/whisper_spike_bg.wasm" });
  log(done());

  done = t("ort-web backend init (fetch onnxruntime-web dist)");
  await ortBackendInit(location.origin + "/nm/onnxruntime-web/dist/");
  log(done());

  done = t("model fetch /models/encoder_model.onnx");
  const bytes = await fetch("/models/encoder_model.onnx").then((r) => {
    if (!r.ok) throw new Error(`model fetch failed: ${r.status}`);
    return r.arrayBuffer();
  });
  log(`${done()} (${(bytes.byteLength / 1e6).toFixed(1)} MB)`);

  done = t("session create (commit_from_memory)");
  await load_model(new Uint8Array(bytes));
  log(done());

  const mel = new Float32Array(N_MELS * N_FRAMES);
  for (let i = 0; i < mel.length; i++) mel[i] = Math.sin(i * 0.017) * 0.5 - 1.6;

  log(`encoding pseudo log-mel (${N_MELS}x${N_FRAMES}) …`);
  done = t("encoder run");
  const hidden = await encode(mel);
  log(done());

  let sum = 0;
  let absmax = 0;
  for (let i = 0; i < hidden.length; i++) {
    sum += hidden[i];
    absmax = Math.max(absmax, Math.abs(hidden[i]));
  }
  log(
    `output length=${hidden.length} (expect ${1500 * 384}) mean=${(sum / hidden.length).toFixed(4)} absmax=${absmax.toFixed(3)}`
  );
  log(
    `first8: ${Array.from(hidden.slice(0, 8)).map((v) => v.toFixed(3)).join(", ")}`
  );
  log("done ✓");
}

main().catch(fail);
