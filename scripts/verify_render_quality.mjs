// Verifikasi KUALITAS hasil render setelah optimasi.
// 4 varian, semua engine=cpu (band crop path),同一 SRT & video pendek:
//   ref_full   : band crop OFF (full-frame) + x264 CRF18 medium  → GROUND TRUTH
//   band128    : band crop ON margin 128 + x264 CRF18 medium     → default
//   band96     : band crop ON margin 96  + x264 CRF18 medium     → uji margin kecil
//   fast       : band crop ON margin 128 + x264 ultrafast CRF25  → setelan cepat
//
// Banding PSNR/SSIM via ffmpeg (band pita caption + full frame).
// Run: node scripts/verify_render_quality.mjs [--sec=30]
import { spawnSync, execFileSync } from "child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "fs";

const FFMPEG = "E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin\\ffmpeg.exe";
const FFPROBE = "E:\\temp\\opencode\\ffmpeg-9.0.2-full_build-shared\\bin\\ffprobe.exe";
const RSCAPS = "E:\\project\\rscaps\\target\\release\\rscaps.exe";
const TMP = "E:\\temp\\opencode\\quality_check";
const SRC_VIDEO = "E:\\temp\\opencode\\web4min.mp4";
const SRC_SRT = "E:\\temp\\opencode\\web4min.srt";

const secArg = process.argv.find((a) => a.startsWith("--sec="));
const SEC = secArg ? Number(secArg.split("=")[1]) : 30;

const step = (m) => console.log(`[step] ${m}`);

function sh(bin, args, opts = {}) {
  const r = spawnSync(bin, args, { encoding: "utf8", ...opts });
  if (r.status !== 0) {
    throw new Error(`${bin} exit ${r.status}: ${(r.stderr || "").slice(-800)}`);
  }
  return r.stdout || "";
}

// ── siapkan clip pendek + SRT terpotong ───────────────────────────────────
function prepareInputs() {
  mkdirSync(TMP, { recursive: true });
  const video = `${TMP}\\clip_${SEC}s.mp4`;
  const srt = `${TMP}\\clip_${SEC}s.srt`;
  if (!existsSync(video)) {
    step(`buat clip uji ${SEC}s (testsrc2 1920x1080@30)`);
    sh(FFMPEG, [
      "-y", "-v", "error",
      "-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30",
      "-t", String(SEC), "-pix_fmt", "yuv420p", video,
    ]);
  }
  if (!existsSync(srt)) {
    step("potong SRT ke durasi clip");
    const raw = readFileSync(SRC_SRT, "utf8");
    const blocks = raw.split(/\r?\n\r?\n/).filter((b) => b.trim());
    const keep = [];
    for (const b of blocks) {
      const m = b.match(/(\d+):(\d+):(\d+),(\d+)\s*-->\s*(\d+):(\d+):(\d+),(\d+)/);
      if (!m) continue;
      const start = +m[1] * 3600 + +m[2] * 60 + +m[3];
      if (start < SEC) keep.push(b.trim());
    }
    writeFileSync(srt, keep.join("\n\n") + "\n", "utf8");
  }
  return { video, srt };
}

// ── render satu varian ───────────────────────────────────────────────────
function renderVariant(id, { video, srt }, { bandCrop, margin, preset, crf, hwEncode }) {
  const out = `${TMP}\\${id}.mp4`;
  const args = [
    "render",
    "--video", video,
    "--srt", srt,
    "--template", "cleo",
    "--out", out,
    "--engine", "cpu",
    "--preset", preset,
    "--crf", String(crf),
  ];
  if (hwEncode) args.push("--hw-encode");
  const env = {
    ...process.env,
    RSCAPS_BAND_CROP: bandCrop ? "1" : "0",
    RSCAPS_BAND_MARGIN: String(margin),
    RSCAPS_SLICES: "3",
    RSCAPS_FFMPEG_THREADS: "16",
  };
  step(`render ${id} (band=${bandCrop ? `on/${margin}px` : "off"} preset=${preset} crf=${crf})`);
  const t0 = Date.now();
  const r = spawnSync(RSCAPS, args, { env, encoding: "utf8" });
  const wall = ((Date.now() - t0) / 1000).toFixed(1);
  if (r.status !== 0) {
    throw new Error(`render ${id} gagal: ${(r.stderr || "").slice(-1500)}`);
  }
  const band = (r.stderr || "").match(/band crop aktif: y=(\d+) h=(\d+)/);
  return { out, wall, band: band ? `y=${band[1]} h=${band[2]}` : "full-frame" };
}

// ── banding PSNR/SSIM antar 2 file (ffmpeg filter) ───────────────────────
function compare(a, b, region) {
  const filters = [];
  if (region) filters.push(`[0:v]${region}[r0];[1:v]${region}[r1]`);
  else filters.push(`[0:v]null[r0];[1:v]null[r1]`);
  const args = [
    "-hide_banner", "-i", a, "-i", b,
    "-filter_complex", `${filters.join(";")};[r0][r1]psnr[p];[r0][r1]ssim[ss]`,
    "-f", "null", "-",
  ];
  const r = spawnSync(FFMPEG, args, { encoding: "utf8" });
  const out = r.stderr || "";
  const psnr = out.match(/PSNR y:([\d.]+|inf)\s+u:([\d.]+|inf)\s+v:([\d.]+|inf)\s+average:([\d.]+|inf)/);
  const ssim = out.match(/SSIM Y:([\d.]+).*?All:([\d.]+)/);
  const bytes = (out.match(/total size: (\d+) bytes/) || [])[1];
  return {
    psnr: psnr ? psnr[4] : "?",
    ssim: ssim ? ssim[2] : "?",
    bytes: bytes ? (Number(bytes) / 1e6).toFixed(2) + "MB" : "?",
  };
}

function main() {
  const inputs = prepareInputs();
  const variants = [
    { id: "ref_full", bandCrop: false, margin: 128, preset: "medium", crf: 18, hwEncode: false },
    { id: "band128", bandCrop: true, margin: 128, preset: "medium", crf: 18, hwEncode: false },
    { id: "band96", bandCrop: true, margin: 96, preset: "medium", crf: 18, hwEncode: false },
    { id: "fast", bandCrop: true, margin: 128, preset: "ultrafast", crf: 25, hwEncode: false },
    { id: "fast_nvenc", bandCrop: true, margin: 128, preset: "medium", crf: 23, hwEncode: true },
  ];
  const res = {};
  for (const v of variants) {
    res[v.id] = renderVariant(v.id, inputs, v);
    res[v.id].size = sh(FFPROBE, ["-v", "error", "-show_entries", "format=size", "-of", "csv=p=0", res[v.id].out]).trim();
  }
  const ref = res.ref_full.out;
  // pita caption dari log band128
  const bandM = res.band128.band.match(/y=(\d+) h=(\d+)/);
  const bandRegion = bandM ? `crop=1920:${bandM[2]}:0:${bandM[1]}` : null;

  console.log("\n=== HASIL (vs ref_full = full-frame, x264 CRF18 medium) ===");
  console.log("varian        | band        | preset/crf | wall  | sizeMB | PSNR full | PSNR pita | SSIM pita");
  for (const v of variants) {
    const r = res[v.id];
    if (v.id === "ref_full") {
      console.log(`${v.id.padEnd(13)}| ${r.band.padEnd(12)}| ${v.preset}/${v.crf}   | ${r.wall}s | ${(Number(r.size) / 1e6).toFixed(1)} | (reference)`);
      continue;
    }
    const cFull = compare(ref, r.out, null);
    const cBand = bandRegion ? compare(ref, r.out, bandRegion) : { psnr: "?", ssim: "?" };
    console.log(
      `${v.id.padEnd(13)}| ${r.band.padEnd(12)}| ${(v.preset + "/" + v.crf).padEnd(10)} | ${(r.wall + "s").padEnd(5)} | ${(Number(r.size) / 1e6).toFixed(1).padEnd(6)} | ${cFull.psnr.padEnd(9)} | ${cBand.psnr.padEnd(9)} | ${cBand.ssim}`,
    );
  }
  console.log(`\nBand pita caption: ${bandRegion || "(tidak ada band)"}`);
  console.log(`File: ${TMP}`);
}

main();
