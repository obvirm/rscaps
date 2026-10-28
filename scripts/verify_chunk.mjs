// Verifikasi batas chunk: render `--engine fframes` chunked vs single-pass lalu
// bandingkan hash per-frame hasil decode (framemd5). Menutup risiko off-by-one
// di batas potongan.
//
// Catatan: JANGAN pakai filter psnr — ffmpeg concat menggeser PTS, sehingga
// psnr membandingkan pasangan frame yang salah di batas dan memberi skor palsu.
// framemd5 membandingkan KONTEN frame, bukan timestamp.
//
// Pakai: node scripts/verify_chunk.mjs [video] [srt] [template]
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const bin = join(repoRoot, "target", "debug", process.platform === "win32" ? "rscaps.exe" : "rscaps");
const video = process.argv[2] ?? join(repoRoot, "models", "rscaps-test.mp4");
const srt = process.argv[3] ?? join(repoRoot, "rscaps-sample.srt");
const template = process.argv[4] ?? "milo";
const chunk = 30;
const outDir = join(repoRoot, "out", "verify_chunk");

const FFMPEG_BIN = "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin";
const sep = process.platform === "win32" ? ";" : ":";
const env = { ...process.env, PATH: `${FFMPEG_BIN}${sep}${process.env.PATH}` };

function run(args, extraEnv = {}) {
  const r = spawnSync(bin, args, { env: { ...env, ...extraEnv }, encoding: "utf8" });
  if (r.status !== 0) {
    console.error(`gagal: rscaps ${args.join(" ")}\n${r.stdout}\n${r.stderr}`);
    process.exit(1);
  }
}
/** Hash konten tiap frame (kolom terakhir framemd5), tanpa PTS. */
function frameHashes(file) {
  const r = spawnSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-i", file, "-map", "0:v", "-f", "framemd5", "-"], {
    env,
    encoding: "utf8",
  });
  return r.stdout
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => l.split(", ").at(-1));
}

if (!existsSync(bin)) {
  console.error(`binary belum di-build: ${bin} (cargo build -p rscaps-cli)`);
  process.exit(1);
}
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const single = join(outDir, "single.mp4");
const chunked = join(outDir, "chunked.mp4");

console.log("render single-pass (crf 0)...");
run(["render", "--engine", "fframes-pipeline", "--video", video, "--srt", srt, "--template", template, "--out", single, "--crf", "0"]);
console.log(`render chunked (RSCAPS_FFRAMES_CHUNK=${chunk}, crf 0)...`);
run(["render", "--engine", "fframes-pipeline", "--video", video, "--srt", srt, "--template", template, "--out", chunked, "--crf", "0"], {
  RSCAPS_FFRAMES_CHUNK: String(chunk),
});

const a = frameHashes(single);
const b = frameHashes(chunked);
const n = Math.max(a.length, b.length);
const bad = [];
for (let i = 0; i < n; i++) if (a[i] !== b[i]) bad.push(i);

console.log(`frame: single=${a.length} chunked=${b.length}`);
if (bad.length === 0) {
  console.log(`OK: chunked identik dengan single (${a.length} frame)`);
  process.exit(0);
}
console.log(`GAGAL: ${bad.length} frame beda → index ${bad.slice(0, 20).join(", ")}`);
process.exit(1);
