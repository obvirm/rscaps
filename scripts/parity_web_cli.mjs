// Paritas visual: export web (svgr-wasm) vs render CLI --engine cpu (svgr).
// Keduanya usvgr+tiny-skia → PSNR tinggi = paritas terbukti.
//
// Pakai: node scripts/parity_web_cli.mjs
import puppeteer from "puppeteer-core";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, existsSync, readdirSync, copyFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const STUDIO_URL = "http://localhost:5177/";
// --legacy: export via jalur lama (canvas browser, ?exportmode=legacy) untuk
// A/B paritas. File hasil diberi awalan webm_legacy (tak menimpa webm_wasm).
const LEGACY = process.argv.includes("--legacy");
const PAGE_URL = LEGACY ? STUDIO_URL + "?exportmode=legacy" : STUDIO_URL;
const SKIP_COMPARE = LEGACY; // run legacy cukup export saja
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT = "E:\\project\\rscaps\\rscaps-sample.srt";
const TEMPLATE = "cleo";
const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const bin = join(repoRoot, "target", "debug", process.platform === "win32" ? "rscaps.exe" : "rscaps");
const outDir = "E:\\temp\\opencode\\parity";
const FFMPEG_BIN = "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin";
const env = { ...process.env, PATH: `${FFMPEG_BIN};${process.env.PATH}` };
const TIMES = [1, 3, 5, 7, 9, 11]; // detik

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (m) => console.log(`[step] ${m}`);
const run = (args) => {
  const r = spawnSync(args[0], args.slice(1), { env, encoding: "utf8" });
  if (r.status !== 0) {
    console.error(r.stdout, r.stderr);
    throw new Error(`gagal: ${args.join(" ")}`);
  }
  return r;
};

if (!LEGACY) rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

// ── 1. Export web (Chrome headless, download ke outDir) ────────────────────
step("export web via Chrome");
const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  args: ["--window-size=1920,1080", "--hide-scrollbars"],
  defaultViewport: { width: 1920, height: 1080 },
});
const page = await browser.newPage();
page.on("pageerror", (e) => console.log(`[pageerror] ${e.message}`));
page.on("console", (m) => {
  if (/\[dbg/.test(m.text())) console.log(m.text());
});
const client = await page.createCDPSession();
await client.send("Page.setDownloadBehavior", { behavior: "allow", downloadPath: outDir });

const clickByText = async (sel, text) => {
  const ok = await page.evaluate(
    (sel, text) => {
      const els = [...document.querySelectorAll(sel)];
      const el = els.find((e) => e.textContent.trim().includes(text));
      if (el) el.click();
      return !!el;
    },
    sel,
    text,
  );
  if (!ok) throw new Error(`not found: ${sel} "${text}"`);
};

let webm = null;
try {
  await page.goto(PAGE_URL, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(600);
  await clickByText("button", "New project");
  await page.waitForSelector('input[type="file"]', { timeout: 15000 });
  await sleep(300);
  const videoHandle = await page.evaluateHandle(() => {
    const inputs = [...document.querySelectorAll('input[type="file"]')];
    return inputs.find((i) => !i.accept.includes("srt"));
  });
  await videoHandle.asElement().uploadFile(VIDEO);
  await page.waitForSelector(".dialog", { timeout: 15000 });
  await sleep(500);
  // Dialog lengkap (dengan #np-template) baru muncul SETELAH video dipilih.
  // Default dropdown = "luna" — set ke TEMPLATE sebelum create project.
  await page.waitForFunction(
    (tpl) => {
      const sel = document.querySelector("#np-template");
      return sel && [...sel.options].some((o) => o.value === tpl);
    },
    { timeout: 30000 },
    TEMPLATE,
  );
  await page.select("#np-template", TEMPLATE);
  console.log(`[info] template dialog → ${TEMPLATE}`);
  step("use subtitle file");
  await clickByText(".dialog button", "Use subtitle file instead");
  await page.waitForSelector('input[type="file"][accept*="srt"]', { timeout: 15000 });
  const srtHandle = await page.evaluateHandle(() =>
    document.querySelector('input[type="file"][accept*="srt"]'),
  );
  await srtHandle.asElement().uploadFile(SRT);
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(2000);

  await page.evaluate(() => {
    const btns = [...document.querySelectorAll(".rail-tab")];
    btns.find((x) => (x.getAttribute("aria-label") || "").toLowerCase() === "templates")?.click();
  });
  await sleep(800);
  await page.evaluate((tpl) => {
    const cards = [...document.querySelectorAll("[class*=card], button")];
    cards.find((x) => new RegExp(tpl, "i").test(x.textContent ?? ""))?.click();
  }, TEMPLATE);
  await sleep(1500);

  // Ground truth posisi caption: screenshot preview studio (frame 0).
  const previewEl = await page.$(".preview-region");
  if (previewEl) {
    await previewEl.screenshot({ path: join(outDir, "preview_0.png") });
    console.log("[info] preview screenshot: preview_0.png");
  }

  await clickByText("button", "Export");
  await sleep(800);
  // WebM: AAC bawaan gagal di video test (8000Hz mono).
  const adv = await page.$(".advanced-toggle, summary");
  if (adv) {
    await adv.click();
    await sleep(400);
  }
  const formatSel = await page.$("#ex-format");
  if (formatSel) {
    await formatSel.select("webm");
    await sleep(200);
  }
  const exportBtn = await page.$(".actions-right .btn.primary");
  if (!exportBtn) throw new Error("tombol Export tidak ditemukan");

  const t0 = Date.now();
  await exportBtn.click();
  // Tunggu file webm muncul di download dir + dialog tutup.
  const deadline = t0 + 180_000;
  while (Date.now() < deadline) {
    const files = readdirSync(outDir).filter((f) => f.endsWith(".webm"));
    const dialogOpen = await page.evaluate(() => !!document.querySelector(".backdrop"));
    if (files.length > 0 && !dialogOpen) {
      // Tunggu Chrome selesai commit download (ukuran stabil) — kalau
      // browser.close() terlalu cepat, Chrome bisa membatalkan file.
      const path = join(outDir, files[0]);
      let last = -1;
      for (let i = 0; i < 10; i++) {
        const size = existsSync(path) ? statSync(path).size : -1;
        if (size === last && size > 0) break;
        last = size;
        await sleep(300);
      }
      webm = path;
      break;
    }
    await sleep(300);
  }
  if (!webm) throw new Error("webm tidak ter-download dalam 180s");
  console.log(`[info] webm: ${webm} (${((Date.now() - t0) / 1000).toFixed(1)}s)`);
  await sleep(500);
  // Simpan dengan nama mode (wasm/legacy) untuk A/B.
  const named = join(outDir, LEGACY ? "webm_legacy.webm" : "webm_wasm.webm");
  copyFileSync(webm, named);
  console.log(`[info] saved: ${named}`);
} finally {
  await browser.close();
}

// ── 2. Render CLI --engine cpu (jalur svgr lama) ───────────────────────────
if (SKIP_COMPARE) {
  console.log("[done] mode legacy: export saja (A/B jalankan mode wasm untuk membandingkan)");
  process.exit(0);
}
step("render CLI --engine cpu (crf 0)");
const cliMp4 = join(outDir, "cli_cpu.mp4");
run([
  bin, "render", "--engine", "cpu",
  "--video", VIDEO, "--srt", SRT, "--template", TEMPLATE,
  "--out", cliMp4, "--crf", "0",
]);

// ── 3. Ekstrak frame di Times yang sama + PSNR per frame ───────────────────
// PENTING: -fps_mode passthrough — default cfr MENDUPLIKASI frame dan
// menggeser konten (false-positive "lag 1 frame" saat investigasi paritas).
step("ekstrak frame + PSNR");
const results = [];
for (const t of TIMES) {
  const a = join(outDir, `web_${t}.png`);
  const b = join(outDir, `cli_${t}.png`);
  run(["ffmpeg", "-y", "-v", "error", "-ss", String(t), "-i", webm, "-fps_mode", "passthrough", "-frames:v", "1", a]);
  run(["ffmpeg", "-y", "-v", "error", "-ss", String(t), "-i", cliMp4, "-fps_mode", "passthrough", "-frames:v", "1", b]);
  const r = spawnSync(
    "ffmpeg",
    ["-v", "info", "-i", a, "-i", b, "-lavfi", "psnr", "-f", "null", "-"],
    { env, encoding: "utf8" },
  );
  const m = (r.stderr || "").match(/average:([\d.]+)/);
  results.push({ t, psnr: m ? parseFloat(m[1]) : NaN });
  console.log(`[psnr] t=${t}s: ${m ? m[1] : "?"} dB`);
}

// ── 4. Contact sheet side-by-side (web | cli) per waktu; tanpa drawtext ───
// (drawtext butuh fontconfig - tak tersedia di mesin ini. Urutan: baris per
// waktu [1,3,5,7,9,11]s, kolom kiri WEB kanan CLI.)
step("contact sheet");
const inputs = TIMES.flatMap((t) => ["-i", join(outDir, `web_${t}.png`), "-i", join(outDir, `cli_${t}.png`)]);
const n = TIMES.length * 2;
const filt = {
  filter_complex: `${Array.from({ length: n }, (_, i) => `[${i}:v]scale=320:240[v${i}]`).join(";")};${Array.from({ length: n }, (_, i) => `[v${i}]`).join("")}xstack=inputs=${n}:layout=${Array.from({ length: n }, (_, i) => `${(i % 2) * 320}_${Math.floor(i / 2) * 240}`).join("|")}[out]`,
};
run([
  "ffmpeg", "-y", "-v", "error", ...inputs,
  "-filter_complex", filt.filter_complex,
  "-map", "[out]", join(outDir, "contact_sheet.png"),
]);

const avg = results.reduce((s, r) => s + (Number.isFinite(r.psnr) ? r.psnr : 0), 0) / results.length;
console.log(`\n[result] PSNR rata-rata: ${avg.toFixed(2)} dB (≥40 = paritas visual)`);
console.log(`[result] file di ${outDir}`);
console.log(avg >= 40 ? "[result] PARITAS OK" : "[result] PARITAS RAGU — lihat contact_sheet.png");

