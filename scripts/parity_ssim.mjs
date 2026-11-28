// Parity Gate 41 template: SSIM tscaps (golden) vs rscaps.
// Pakai pasangan screenshot dari scripts/crosscheck.mjs:
//   <id>__tscaps.png  (golden)  vs  <id>__rscaps.png
// Ambang PRD: SSIM >= 0.95 per template. Hasil JSON + daftar gagal.
import { readdirSync, writeFileSync, existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const DIR = process.env.RSCAPS_COMPARE_DIR || "E:\\temp\\opencode\\compare";
const THRESHOLD = Number(process.env.RSCAPS_SSIM_MIN || "0.95");
const FFMPEG =
  process.env.FFMPEG ||
  "E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin\\ffmpeg.exe";
const FFPROBE =
  process.env.FFPROBE ||
  "E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin\\ffprobe.exe";

if (!existsSync(DIR)) {
  console.error(`folder compare tidak ada: ${DIR}`);
  process.exit(1);
}

const files = new Set(readdirSync(DIR));
const goldens = [...files].filter((f) => f.endsWith("__tscaps.png") && !f.includes("@")).sort();
const results = [];

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
  const out = `${r.stdout || ""}\n${r.stderr || ""}`;
  const m = out.match(/All:([0-9.]+)/);
  return m ? Number(m[1]) : null;
}

for (const g of goldens) {
  const id = g.slice(0, -"__tscaps.png".length);
  const c = `${id}__rscaps.png`;
  if (!files.has(c)) {
    results.push({ id, ssim: null, ok: false, reason: "rscaps screenshot missing" });
    continue;
  }
  const score = ssim(join(DIR, g), join(DIR, c));
  results.push({
    id,
    ssim: score,
    ok: score !== null && score >= THRESHOLD,
    reason: score === null ? "ffmpeg ssim parse failed" : undefined,
  });
}

const passed = results.filter((r) => r.ok).length;
const report = { dir: DIR, threshold: THRESHOLD, total: results.length, passed, results };
const outPath = join(DIR, "ssim_report.json");
writeFileSync(outPath, JSON.stringify(report, null, 2));

for (const r of results) {
  console.log(`${r.ok ? "OK  " : "FAIL"} ${r.id.padEnd(18)} ${r.ssim ?? "?"}`);
}
console.log(`\n${passed}/${results.length} lulus (SSIM >= ${THRESHOLD}) → ${outPath}`);
process.exit(passed === results.length ? 0 : 1);
