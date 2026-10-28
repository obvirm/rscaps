// Matrix benchmark CLI rscaps — video 4 menit penuh, 8 konfigurasi.
// Resume-safe: config yang sudah punya hasil (results.json + file output) di-skip.
// Run: node scripts/bench_cli_matrix.mjs [--only=id1,id2]
import { spawn, exec } from "child_process";
import { promisify } from "util";
import { existsSync, mkdirSync, writeFileSync, appendFileSync, readFileSync } from "fs";

const execP = promisify(exec);

const RSCAPS = "E:\\project\\rscaps\\target\\release\\rscaps.exe";
const FFMPEG_BIN = "E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin";
const VIDEO = "E:\\temp\\opencode\\web4min.mp4";
const SRT = "E:\\temp\\opencode\\web4min.srt";
const OUT_DIR = "E:\\temp\\opencode\\cli_matrix";
const RESULTS = `${OUT_DIR}\\results.json`;
const SAMPLE_PS1 = "E:\\temp\\opencode\\sample_usage_cli.ps1";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (m) => console.log(`[step] ${m}`);

// ── Konfigurasi matrix ──────────────────────────────────────────────────────
// engine: cpu | skia | fframes (mode streaming: caption via machinery Video
// fframes → raster pita → pipe rawvideo → ffmpeg overlay+encode)
// slices: jumlah proses ffmpeg paralel (CPU→3, GPU→2); threads = 12/slices
// (total thread ≈ 12 core logis; 16 thread/slice → oversubscribe → +8%
//   dan CPU 100% kontensi). CRF x264 = 28 (ultrafast): vs NVENC cq23
//   PSNR 46.3 dB / SSIM 0.995 (ambang paritas 45 dB); CRF 23-25 bikin
//   varian x264 >40s (encode 13.3s + raster 34s tak Impas).
const CONFIGS = [
  { id: "fframes_x264",        engine: "fframes", hwEncode: false, hwaccel: null,    slices: 2, threads: 16, preset: "ultrafast", crf: 28, note: "fframes streaming + libx264 ultrafast crf28, decode CPU" },
  { id: "fframes_nvenc_cuda",  engine: "fframes", hwEncode: true,  hwaccel: "cuda",  slices: 2, threads: 16, preset: "p1",        crf: 23, note: "fframes streaming + nvenc p1 + decode CUDA (full GPU)" },
  { id: "skia_x264",           engine: "skia",    hwEncode: false, hwaccel: null,    slices: 2, threads: 16, preset: "ultrafast", crf: 28, note: "skia(Vulkan raster) + libx264 ultrafast crf28" },
  { id: "skia_nvenc",          engine: "skia",    hwEncode: true,  hwaccel: null,    slices: 2, threads: 16, preset: "medium",    crf: 23, note: "skia + nvenc medium cq23" },
  { id: "cpu_x264",            engine: "cpu",     hwEncode: false, hwaccel: null,    slices: 3, threads: 16, preset: "ultrafast", crf: 28, note: "cpu(usvgr raster) + libx264 ultrafast crf28" },
  { id: "cpu_x264_cuda",       engine: "cpu",     hwEncode: false, hwaccel: "cuda",  slices: 3, threads: 16, preset: "ultrafast", crf: 28, note: "cpu + libx264 ultrafast crf28 + decode CUDA" },
  { id: "cpu_nvenc",           engine: "cpu",     hwEncode: true,  hwaccel: null,    slices: 3, threads: 16, preset: "medium",    crf: 23, note: "cpu + nvenc medium cq23" },
  { id: "cpu_nvenc_cuda",      engine: "cpu",     hwEncode: true,  hwaccel: "cuda",  slices: 3, threads: 16, preset: "medium",    crf: 23, note: "cpu + nvenc medium cq23 + decode CUDA" },
];

// ── Usage sampling (RAM/CPU/GPU proses render) ─────────────────────────────
async function sampleOnce() {
  try {
    const { stdout } = await execP(
      `powershell -NoProfile -ExecutionPolicy Bypass -File "${SAMPLE_PS1}"`,
      { timeout: 8000, windowsHide: true },
    );
    const line = stdout.trim().split("\n").pop() || "";
    const p = line.split(",").map((x) => parseFloat(x.trim()));
    if (p.length >= 8 && !p.some(Number.isNaN)) {
      return {
        ramTotalGB: p[0], ramFreeGB: p[1], cpuPct: p[2], procRamGB: p[3],
        gpuUtilPct: p[4], gpuMemUsedMB: p[5], gpuMemTotalMB: p[6], gpuTempC: p[7],
      };
    }
  } catch {}
  return null;
}

// ── ffprobe ringkas output ─────────────────────────────────────────────────
async function probe(file) {
  try {
    const { stdout } = await execP(
      `ffprobe -v error -show_entries format=duration,size,bit_rate -show_entries stream=codec_type,codec_name,profile,width,height,r_frame_rate,nb_frames -of json "${file}"`,
      { timeout: 20000 },
    );
    return JSON.parse(stdout);
  } catch (e) {
    return { error: String(e.message || e).slice(0, 300) };
  }
}

// ── Jalankan satu config ───────────────────────────────────────────────────
async function runAttempt(cfg, attempt = 1) {
  const outFile = `${OUT_DIR}\\${cfg.id}.mp4`;
  const usageCsv = `${OUT_DIR}\\${cfg.id}.${attempt}.usage.csv`;
  const logFile = `${OUT_DIR}\\${cfg.id}.${attempt}.log`;

  if (existsSync(outFile) && existsSync(`${OUT_DIR}\\${cfg.id}.json`)) {
    step(`SKIP ${cfg.id} (sudah ada hasil)`);
    return JSON.parse(readFileSync(`${OUT_DIR}\\${cfg.id}.json`, "utf8"));
  }

  const args = [
    "render",
    "--video", VIDEO,
    "--srt", SRT,
    "--template", "cleo",
    "--out", outFile,
    "--engine", cfg.engine,
  ];
  if (cfg.hwEncode) args.push("--hw-encode");
  if (cfg.hwaccel) args.push("--hwaccel", cfg.hwaccel);
  if (cfg.preset) args.push("--preset", cfg.preset);
  if (cfg.crf) args.push("--crf", String(cfg.crf));

  const env = {
    ...process.env,
    PATH: `${FFMPEG_BIN};${process.env.PATH}`,
    RSCAPS_FFMPEG_THREADS: String(cfg.threads ?? 16), // thread ffmpeg per slice
    // (12 core logis; thread tinggi per slice lebih cepat: rantai
    // decode+overlay+konversi RGBA→YUV di CPU — encode NVENC pun butuh
    // thread CPU untuk feed. 6 thread/slice bikin varian nvenc +4s.)
    RSCAPS_WINDOW: "32", // double-buffer 32 frame/slice → RAM ≈ slice×32×3.3MB
    RSCAPS_FFRAMES_CHUNK: "150", // HANYA untuk engine fframes-pipeline (chunk 150
    // ≈ 1.25GB predecode; 300 → 12.5GB in-flight → libav -12 "Cannot allocate
    // memory" saat commit sistem tinggal ~8GB).
    RSCAPS_FFRAMES_DECODE_JOBS: "2",
    RSCAPS_FFRAMES_RENDER_JOBS: "1", // raster fframes = GPU → 2 worker kontensi
    RSCAPS_SLICES: String(cfg.slices),
  };

  step(`RUN ${cfg.id} — ${cfg.note}`);
  writeFileSync(logFile, `# ${cfg.id}: rscaps ${args.join(" ")}\n`, "utf8");
  writeFileSync(usageCsv, "t,ramTotalGB,ramFreeGB,cpuPct,procRamGB,gpuUtilPct,gpuMemUsedMB,gpuMemTotalMB,gpuTempC\n");

  const samples = [];
  let sampling = true;
  const t0 = Date.now();
  const sampler = (async () => {
    while (sampling) {
      const s = await sampleOnce();
      if (s) {
        s.t = ((Date.now() - t0) / 1000).toFixed(1);
        samples.push(s);
        appendFileSync(
          usageCsv,
          `${s.t},${s.ramTotalGB},${s.ramFreeGB},${s.cpuPct},${s.procRamGB},${s.gpuUtilPct},${s.gpuMemUsedMB},${s.gpuMemTotalMB},${s.gpuTempC}\n`,
        );
      }
      await sleep(2000);
    }
  })();

  // Spawn rscaps — log stderr ke file, tampilkan progress % kasar.
  const wall = await new Promise((resolve) => {
    const child = spawn(RSCAPS, args, { env });
    let lastPct = "";
    const onData = (d) => {
      const txt = d.toString();
      appendFileSync(logFile, txt);
      const m = txt.match(/(\d+)%/);
      if (m && m[0] !== lastPct) {
        lastPct = m[0];
        process.stdout.write(`  [${cfg.id}] ${m[0]}\n`);
      }
    };
    child.stdout.on("data", onData);
    child.stderr.on("data", onData);
    child.on("close", (code) => resolve({ code, ms: Date.now() - t0 }));
    child.on("error", (e) => { appendFileSync(logFile, `\nSPAWN ERROR: ${e}\n`); resolve({ code: -1, ms: Date.now() - t0 }); });
  });

  sampling = false;
  await sampler;

  const wallSec = +(wall.ms / 1000).toFixed(1);
  const ok = wall.code === 0 && existsSync(outFile);
  const logTail = ok ? "" : readFileSync(logFile, "utf8").slice(-1500);

  const fs = await import("fs");
  const sizeMB = ok ? +(fs.statSync(outFile).size / 1048576).toFixed(1) : null;
  const ff = ok ? await probe(outFile) : null;

  const peak = (k) => (samples.length ? Math.max(...samples.map((s) => s[k])) : null);
  const avg = (k) =>
    samples.length ? +(samples.reduce((a, s) => a + s[k], 0) / samples.length).toFixed(1) : null;

  const rec = {
    id: cfg.id,
    note: cfg.note,
    engine: cfg.engine,
    hwEncode: cfg.hwEncode ? "h264_nvenc" : "libx264",
    hwaccel: cfg.hwaccel ?? "cpu",
    ok,
    wallSec,
    realtimeFactor: +(240 / wallSec).toFixed(2), // kecepatan vs durasi video 240s
    sizeMB,
    ffprobe: ff,
    usage: {
      samples: samples.length,
      peak: { cpuPct: peak("cpuPct"), procRamGB: peak("procRamGB"), gpuUtilPct: peak("gpuUtilPct"), gpuMemUsedMB: peak("gpuMemUsedMB"), gpuTempC: peak("gpuTempC") },
      avg: { cpuPct: avg("cpuPct"), procRamGB: avg("procRamGB"), gpuUtilPct: avg("gpuUtilPct"), gpuMemUsedMB: avg("gpuMemUsedMB") },
    },
    error: ok ? null : logTail,
  };

  writeFileSync(`${OUT_DIR}\\${cfg.id}.${attempt}.json`, JSON.stringify(rec, null, 2), "utf8");
  step(
    `${ok ? "DONE" : "FAIL"} ${cfg.id}#${attempt}: ${wallSec}s (×${rec.realtimeFactor}) size=${sizeMB ?? "?"}MB peakCPU=${rec.usage.peak.cpuPct}% peakGPU=${rec.usage.peak.gpuUtilPct}%`,
  );
  return rec;
}

/// Wrapper best-of-N: mesin ini bisa punya beban luar (ComfyUI/Docker) →
/// satu run bisa +30% karena commit RAM sistem hampir penuh (alokasi dipaksa
/// pagefile). Ambil run TERCEPAT sebagai angka Cleanup (performansi
/// ter-minimal, bukan rata-rata baseline).
async function runOne(cfg, repeat = 1) {
  const outFile = `${OUT_DIR}\\${cfg.id}.mp4`;
  const resFile = `${OUT_DIR}\\${cfg.id}.json`;
  if (existsSync(outFile) && existsSync(resFile)) {
    step(`SKIP ${cfg.id} (sudah ada hasil)`);
    return JSON.parse(readFileSync(resFile, "utf8"));
  }
  let best = null;
  const attempts = [];
  for (let i = 1; i <= repeat; i++) {
    const rec = await runAttempt(cfg, i);
    attempts.push({ attempt: i, ok: rec.ok, wallSec: rec.wallSec });
    if (!best || (rec.ok && !best.ok) || (rec.ok && best.ok && rec.wallSec < best.wallSec)) {
      best = rec;
    }
  }
  best.attempts = attempts;
  writeFileSync(resFile, JSON.stringify(best, null, 2), "utf8");
  return best;
}

// ── Main ───────────────────────────────────────────────────────────────────
mkdirSync(OUT_DIR, { recursive: true });
const onlyArg = process.argv.find((a) => a.startsWith("--only="));
const only = onlyArg ? onlyArg.slice(7).split(",") : null;
const repeatArg = process.argv.find((a) => a.startsWith("--repeat="));
const repeat = repeatArg ? Math.max(1, parseInt(repeatArg.slice(9), 10) || 1) : 1;
const list = only ? CONFIGS.filter((c) => only.includes(c.id)) : CONFIGS;

const all = existsSync(RESULTS) ? JSON.parse(readFileSync(RESULTS, "utf8")) : {};
for (const cfg of list) {
  try {
    all[cfg.id] = await runOne(cfg, repeat);
  } catch (e) {
    console.error(`[ERR] ${cfg.id}: ${e}`);
    all[cfg.id] = { id: cfg.id, ok: false, error: String(e).slice(0, 1000) };
  }
  writeFileSync(RESULTS, JSON.stringify(all, null, 2), "utf8");
}

// ── Tabel ringkas ──────────────────────────────────────────────────────────
console.log("\n══════════ MATRIX CLI — video 4 menit (1920×1080@30, 7200 frame, cleo) ══════════");
const hdr = ["config", "engine", "encode", "decode", "waktu", "×realtime", "sizeMB", "CPU%", "RAM GB", "GPU%", "GPUmem"];
console.log(hdr.map((h) => h.padEnd(11)).join("|"));
for (const cfg of CONFIGS) {
  const r = all[cfg.id];
  if (!r) continue;
  if (!r.ok) { console.log(`${cfg.id.padEnd(11)}| GAGAL — lihat ${cfg.id}.log`); continue; }
  const row = [
    cfg.id,
    r.engine,
    r.hwEncode,
    r.hwaccel,
    `${r.wallSec}s`,
    `×${r.realtimeFactor}`,
    r.sizeMB,
    `${r.usage.peak.cpuPct}`,
    `${r.usage.peak.procRamGB}`,
    `${r.usage.peak.gpuUtilPct}`,
    `${r.usage.peak.gpuMemUsedMB}`,
  ];
  console.log(row.map((x) => String(x ?? "-").padEnd(11)).join("|"));
}
console.log(`\nDetail: ${RESULTS}`);
console.log(`Usage CSV per config: ${OUT_DIR}\\<id>.usage.csv`);

