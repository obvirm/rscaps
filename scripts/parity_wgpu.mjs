// Parity wgpu-lyon vs CPU tiny-skia untuk SEMUA template caption-core.
// Satu frame per template (default t=1.2s), bandingkan RGBA via ffmpeg SSIM.
// Hasil JSON + daftar gagal. Backend wgpu belum memetakan filter/mask/blend;
// gate ini yang menentukan template mana yang masih gagal.
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.cwd();
const BIN = join(ROOT, "target", "debug", process.platform === "win32" ? "rscaps.exe" : "rscaps");
const SRT = join(ROOT, "scripts", "battery.srt");
const OUT = "E:\\temp\\opencode\\wgpu_parity";
const FFMPEG =
  process.env.FFMPEG ||
  "E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin\\ffmpeg.exe";
const FFPROBE =
  process.env.FFPROBE ||
  "E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin\\ffprobe.exe";
const FRAME = process.env.RSCAPS_WGPU_FRAME || "36";
const THRESHOLD = Number(process.env.RSCAPS_SSIM_MIN || "0.95");
const env = {
  ...process.env,
  PATH: `E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin;${process.env.PATH}`,
};

const templates = spawnSync(BIN, ["templates"], { encoding: "utf8", env })
  .stdout.trim()
  .split(/\r?\n/)
  .map((line) => line.trim().split(/\s+/)[0])
  .filter((id) => id && !id.startsWith("("));

rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });

function run(template, cap) {
  const out = join(OUT, `${template}-${cap}`);
  mkdirSync(out, { recursive: true });
  const r = spawnSync(
    BIN,
    [
      "frames", "--srt", SRT, "--template", template, "--out", out,
      "--format", "png", "--from", FRAME, "--to", String(Number(FRAME) + 1), "--cap", cap,
    ],
    { encoding: "utf8", env },
  );
  if (r.status !== 0) throw new Error(`${template}/${cap}: ${r.stderr || r.stdout}`);
  return join(out, `frame_${String(FRAME).padStart(6, "0")}.png`);
}

function dimensions(file) {
  const r = spawnSync(
    FFPROBE,
    ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0", file],
    { encoding: "utf8" },
  );
  const [w, h] = (r.stdout || "").trim().split(",").map(Number);
  return w && h ? { w, h } : null;
}

function ssim(a, b) {
  const size = dimensions(a);
  if (!size) return null;
  const filter = `[1:v]scale=${size.w}:${size.h}:flags=bicubic[scaled];[0:v][scaled]ssim`;
  const r = spawnSync(
    FFMPEG,
    ["-hide_banner", "-i", a, "-i", b, "-filter_complex", filter, "-f", "null", "-"],
    { encoding: "utf8" },
  );
  const m = `${r.stdout || ""}\n${r.stderr || ""}`.match(/All:([0-9.]+)/);
  return m ? Number(m[1]) : null;
}

const results = [];
for (const id of templates) {
  try {
    const cpu = run(id, "tiny-skia");
    const gpu = run(id, "wgpu-lyon");
    const score = ssim(cpu, gpu);
    results.push({ id, ssim: score, ok: score !== null && score >= THRESHOLD });
    console.log(`${score !== null && score >= THRESHOLD ? "OK  " : "FAIL"} ${id.padEnd(10)} ${score ?? "?"}`);
  } catch (err) {
    results.push({ id, ssim: null, ok: false, error: String(err.message || err) });
    console.log(`ERR  ${id.padEnd(10)} ${String(err.message || err).slice(0, 120)}`);
  }
}

const passed = results.filter((r) => r.ok).length;
const report = { frame: Number(FRAME), threshold: THRESHOLD, total: results.length, passed, results };
writeFileSync(join(OUT, "report.json"), JSON.stringify(report, null, 2));
console.log(`\n${passed}/${results.length} lulus SSIM >= ${THRESHOLD} → ${join(OUT, "report.json")}`);
process.exit(passed === results.length ? 0 : 1);
