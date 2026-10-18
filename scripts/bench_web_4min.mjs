// Web export 4 menit + monitor RAM/CPU/GPU real-time.
// Run: node scripts/bench_web_4min.mjs
import puppeteer from "puppeteer-core";
import { exec, spawn } from "child_process";
import { promisify } from "util";
import { writeFileSync } from "fs";

const execP = promisify(exec);

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const URL = "http://localhost:5177/";
const VIDEO = "E:\\temp\\opencode\\web4min.mp4";
const SRT = "E:\\temp\\opencode\\web4min.srt";
const OUT_CSV = "E:\\temp\\opencode\\web4min_usage.csv";
const OUT_JSON = "E:\\temp\\opencode\\web4min_result.json";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (m) => console.log(`[step] ${m}`);
const err = (m) => console.log(`[ERR] ${m}`);

// ── System monitor (RAM/CPU/GPU) via PowerShell counters ──────────────────
const samples = [];
let monitorRunning = false;

async function sampleOnce() {
  try {
    const { stdout } = await execP(
      'powershell -NoProfile -ExecutionPolicy Bypass -File "E:\\temp\\opencode\\sample_usage.ps1"',
      { timeout: 10000, windowsHide: true },
    );
    const line = stdout.trim().split("\n").pop() || "";
    // Format: usedGB,freeGB,totalGB,cpuPct,chromeGB,gpuUtil,gpuMemUsed,gpuMemTotal,gpuTemp
    const parts = line.split(",").map((x) => x.trim());
    if (parts.length >= 5) {
      const rec = {
        t: new Date().toISOString(),
        elapsed: samples.length ? ((Date.now() - t0) / 1000).toFixed(1) : "0",
        ramUsedGB: parseFloat(parts[0]),
        ramFreeGB: parseFloat(parts[1]),
        ramTotalGB: parseFloat(parts[2]),
        cpuPct: parseFloat(parts[3]),
        chromeRamGB: parseFloat(parts[4]),
        gpuUtilPct: parts[5] ? parseFloat(parts[5]) : null,
        gpuMemUsedMB: parts[6] ? parseFloat(parts[6]) : null,
        gpuMemTotalMB: parts[7] ? parseFloat(parts[7]) : null,
        gpuTempC: parts[8] ? parseFloat(parts[8]) : null,
      };
      samples.push(rec);
      return rec;
    }
  } catch (e) {
    // ignore sample errors
  }
  return null;
}

let t0 = Date.now();
async function monitorLoop() {
  while (monitorRunning) {
    await sampleOnce();
    await sleep(2000);
  }
}

// ── Puppeteer export flow ─────────────────────────────────────────────────
const clickByText = async (page, sel, text) => {
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

async function main() {
  step(`launch chrome headless`);
  const browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: true,
    args: [
      "--window-size=1920,1080",
      "--hide-scrollbars",
      "--enable-gpu",
      "--use-gl=angle",
      "--enable-features=SharedArrayBuffer",
      "--disable-dev-shm-usage",
    ],
    defaultViewport: { width: 1920, height: 1080 },
  });
  const page = await browser.newPage();
  page.on("pageerror", (e) => err(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    const t = m.text();
    if (!/favicon|Download the React DevTools/i.test(t)) {
      console.log(`[console.${m.type()}] ${t.slice(0, 300)}`);
    }
  });

  try {
    await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
    await sleep(600);
    step("new project");
    await clickByText(page, "button", "New project");

    await page.waitForSelector('input[type="file"]', { timeout: 15000 });
    await sleep(300);
    const videoHandle = await page.evaluateHandle(() => {
      const inputs = [...document.querySelectorAll('input[type="file"]')];
      return inputs.find((i) => !i.accept.includes("srt"));
    });
    await videoHandle.asElement().uploadFile(VIDEO);

    await page.waitForSelector(".dialog", { timeout: 15000 });
    await sleep(500);
    step("use subtitle file");
    await clickByText(page, ".dialog button", "Use subtitle file instead");
    await page.waitForSelector('input[type="file"][accept*="srt"]', { timeout: 15000 });
    const srtHandle = await page.evaluateHandle(() =>
      document.querySelector('input[type="file"][accept*="srt"]'),
    );
    await srtHandle.asElement().uploadFile(SRT);

    await page.waitForSelector("header.toolbar", { timeout: 90000 });
    await sleep(2000);
    step("editor up");

    // Pilih template (cleo) untuk caption konsisten.
    step("select template cleo");
    await page.evaluate(() => {
      const btns = [...document.querySelectorAll(".rail-tab")];
      btns.find((x) => (x.getAttribute("aria-label") || "").toLowerCase() === "templates")?.click();
    });
    await sleep(800);
    await page.evaluate(() => {
      const cards = [...document.querySelectorAll("[class*=card], button")];
      const c = cards.find((x) => /cleo/i.test(x.textContent ?? ""));
      c?.click();
    });
    await sleep(1500);

    // Buka Export dialog.
    step("open Export dialog");
    await clickByText(page, "button", "Export");
    await sleep(800);

    // Advanced → format WebM (lebih reliable di headless, AAC kadang fail).
    const adv = await page.$(".advanced-toggle, summary");
    if (adv) {
      await adv.click();
      await sleep(400);
    }
    const formatSel = await page.$("#ex-format");
    if (formatSel) {
      await formatSel.select("webm");
      await sleep(200);
      step("format → webm");
    }
    // Quality high (default sudah high).
    const qualitySel = await page.$("#ex-quality");
    if (qualitySel) {
      await qualitySel.select("high");
      await sleep(100);
    }

    // Hook download blob.
    await page.evaluate(() => {
      const w = window;
      w.__downloads = [];
      const origClick = HTMLAnchorElement.prototype.click;
      HTMLAnchorElement.prototype.click = function () {
        if (this.download) w.__downloads.push({ name: this.download, t: Date.now() });
        return origClick.apply(this, arguments);
      };
    });

    // Mulai monitor sebelum klik Export.
    step("start system monitor");
    monitorRunning = true;
    t0 = Date.now();
    const monitorPromise = monitorLoop();

    // Klik Export.
    const exportBtn = await page.$(".actions-right .btn.primary");
    if (!exportBtn) throw new Error("tombol Export tidak ditemukan");
    await exportBtn.click();
    step("clicked Export — monitoring active");

    // Tunggu export selesai (timeout 25 menit; 4 menit video + WASM raster).
    const deadline = t0 + 25 * 60 * 1000;
    let done = false;
    let lastStatus = "";
    const seenStatuses = [];
    while (Date.now() < deadline) {
      const info = await page.evaluate(() => {
        const exporting = !!document.querySelector(".exporting");
        const dialogOpen = !!document.querySelector(".backdrop");
        const statusEl = document.querySelector(".exporting");
        const status = statusEl?.textContent?.trim() ?? "";
        const downloads = window.__downloads ?? [];
        return {
          exporting,
          dialogOpen,
          status,
          downloads: downloads.length,
          dlName: downloads[0]?.name ?? "",
        };
      });
      if (info.downloads > 0) {
        console.log(
          `[info] download detected: "${info.dlName}" after ${((Date.now() - t0) / 1000).toFixed(1)}s`,
        );
        done = true;
        break;
      }
      if (info.status && info.status !== lastStatus) {
        const el = ((Date.now() - t0) / 1000).toFixed(0);
        console.log(`[status @${el}s] ${info.status}`);
        seenStatuses.push(info.status);
        lastStatus = info.status;
      }
      if (/exported/i.test(info.status)) {
        done = true;
        break;
      }
      if (!info.exporting && !info.dialogOpen && seenStatuses.length > 0) {
        console.log(
          `[info] dialog closed after ${((Date.now() - t0) / 1000).toFixed(1)}s, statuses: ${JSON.stringify(seenStatuses)}`,
        );
        done = seenStatuses.some((s) => /exported/i.test(s));
        break;
      }
      // Print periodic progress every 15s.
      if ((Date.now() - t0) % 15000 < 2100) {
        const el = ((Date.now() - t0) / 1000).toFixed(0);
        console.log(`[progress @${el}s] exporting=${info.exporting} status="${info.status}"`);
      }
      await sleep(200);
    }

    // Stop monitor.
    monitorRunning = false;
    await monitorPromise;
    await sampleOnce(); // final sample

    const elapsedSec = (Date.now() - t0) / 1000;
    const result = {
      done,
      elapsedSec: +elapsedSec.toFixed(1),
      video: VIDEO,
      srt: SRT,
      resolution: "1920x1080 @ 30fps, 240s (4 menit)",
      cues: 120,
      sampleCount: samples.length,
      peak: {
        ramUsedGB: Math.max(...samples.map((s) => s.ramUsedGB), 0),
        cpuPct: Math.max(...samples.map((s) => s.cpuPct), 0),
        chromeRamGB: Math.max(...samples.map((s) => s.chromeRamGB), 0),
        gpuUtilPct: Math.max(...samples.map((s) => s.gpuUtilPct ?? 0), 0),
        gpuMemUsedMB: Math.max(...samples.map((s) => s.gpuMemUsedMB ?? 0), 0),
        gpuTempC: Math.max(...samples.map((s) => s.gpuTempC ?? 0), 0),
      },
      avg: {
        ramUsedGB: +(samples.reduce((a, s) => a + s.ramUsedGB, 0) / samples.length).toFixed(2),
        cpuPct: +(samples.reduce((a, s) => a + s.cpuPct, 0) / samples.length).toFixed(1),
        chromeRamGB: +(samples.reduce((a, s) => a + s.chromeRamGB, 0) / samples.length).toFixed(2),
        gpuUtilPct: +(
          samples.reduce((a, s) => a + (s.gpuUtilPct ?? 0), 0) / samples.length
        ).toFixed(1),
      },
      samples,
    };

    // Tulis CSV.
    const header =
      "t,elapsed,ramUsedGB,ramFreeGB,ramTotalGB,cpuPct,chromeRamGB,gpuUtilPct,gpuMemUsedMB,gpuMemTotalMB,gpuTempC";
    const rows = samples.map((s) =>
      [s.t, s.elapsed, s.ramUsedGB, s.ramFreeGB, s.ramTotalGB, s.cpuPct, s.chromeRamGB, s.gpuUtilPct, s.gpuMemUsedMB, s.gpuMemTotalMB, s.gpuTempC].join(","),
    );
    writeFileSync(OUT_CSV, [header, ...rows].join("\n"), "utf8");
    writeFileSync(OUT_JSON, JSON.stringify(result, null, 2), "utf8");

    console.log("\n════════════════════════════════════════");
    console.log(done ? "✅ EXPORT SELESAI" : "❌ EXPORT TIMEOUT/GAGAL");
    console.log(`⏱  Durasi: ${(elapsedSec / 60).toFixed(2)} menit (${elapsedSec.toFixed(1)}s)`);
    console.log(`📊 Samples: ${samples.length} (interval 2s)`);
    console.log(`── Peak ──`);
    console.log(`RAM used:     ${result.peak.ramUsedGB} GB (dari ${samples[0]?.ramTotalGB ?? "?"} GB)`);
    console.log(`CPU:          ${result.peak.cpuPct}%`);
    console.log(`Chrome RAM:   ${result.peak.chromeRamGB} GB`);
    console.log(`GPU util:     ${result.peak.gpuUtilPct}%`);
    console.log(`GPU mem:      ${result.peak.gpuMemUsedMB} MB / ${samples[0]?.gpuMemTotalMB ?? "?"} MB`);
    console.log(`GPU temp:     ${result.peak.gpuTempC}°C`);
    console.log(`── Average ──`);
    console.log(`RAM used:     ${result.avg.ramUsedGB} GB`);
    console.log(`CPU:          ${result.avg.cpuPct}%`);
    console.log(`Chrome RAM:   ${result.avg.chromeRamGB} GB`);
    console.log(`GPU util:     ${result.avg.gpuUtilPct}%`);
    console.log(`════════════════════════════════════════`);
    console.log(`CSV:  ${OUT_CSV}`);
    console.log(`JSON: ${OUT_JSON}`);
  } catch (e) {
    err(String(e));
    await page.screenshot({ path: "E:\\temp\\opencode\\web4min_error.png" }).catch(() => {});
  } finally {
    monitorRunning = false;
    await browser.close();
  }
  process.exit(0);
}

main();
