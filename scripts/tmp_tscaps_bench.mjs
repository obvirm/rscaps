// Benchmark EXPORT tscaps (React) — beban sama: video 4-menit 1080p30 + 120 cue SRT.
// Ukur wall-time dari klik Export sampai anchor-download muncul, plus RAM/CPU/GPU.
import puppeteer from "puppeteer-core";
import { exec, spawn } from "child_process";
import { promisify } from "util";
import fs from "fs";
import path from "path";

const execP = promisify(exec);

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const VIDEO = "E:\\temp\\opencode\\web4min.mp4";
const SRT = "E:\\temp\\opencode\\web4min.srt";
const dlDir = "E:\\temp\\opencode\\tscaps_export_out";
const OUT_CSV = "E:\\temp\\opencode\\tscaps_usage.csv";
const OUT_JSON = "E:\\temp\\opencode\\tscaps_result.json";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (m) => console.log(`[step] ${m}`);
const err = (m) => console.log(`[ERR] ${m}`);

let monitorRunning = false;
let t0 = 0;
const samples = [];

async function sampleOnce() {
  try {
    const { stdout } = await execP(
      "powershell -NoProfile -Command \"$c=Get-CimInstance Win32_Processor; $os=Get-CimInstance Win32_OperatingSystem; $usedGB=[math]::Round(($os.TotalVisibleMemorySize-$os.FreePhysicalMemory)/1MB,2); $totalGB=[math]::Round($os.TotalVisibleMemorySize/1MB,2); $load=$c.LoadPercentage; Write-Output (\\\"RAM $usedGB / $totalGB CPU $load\\\")\"",
      { timeout: 8000 },
    );
    const ram = stdout.match(/RAM ([\d.]+) \/ ([\d.]+)/);
    const cpu = stdout.match(/CPU (\d+)/);
    samples.push({
      t: (Date.now() - t0) / 1000,
      ramUsedGB: ram ? +ram[1] : 0,
      ramTotalGB: ram ? +ram[2] : 0,
      cpuPct: cpu ? +cpu[1] : 0,
    });
    console.log(
      `[sample] t=${samples[samples.length - 1].t.toFixed(0)}s ram=${ram?.[1]}GB cpu=${cpu?.[1]}%`,
    );
  } catch (e) {
    err("sample gagal: " + String(e));
  }
}

async function monitorLoop() {
  while (monitorRunning) {
    await sampleOnce();
    await sleep(2000);
  }
}

async function main() {
  if (!fs.existsSync(dlDir)) fs.mkdirSync(dlDir, { recursive: true });
  const outFile = path.join(dlDir, "web4min.mp4");
  try { fs.rmSync(outFile, { force: true }); } catch { /* ignore */ }

  step("launch chrome headless");
  const browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: "new",
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
    if (/error|DEBUG|blobhook/i.test(t)) console.log(`[console.${m.type()}] ${t.slice(0, 200)}`);
  });

  await page.exposeFunction("__saveChunk", (b64) => {
    fs.appendFileSync(outFile, Buffer.from(b64, "base64"));
  });
  await page.exposeFunction("__beginSave", () => {
    fs.rmSync(outFile, { force: true });
  });
  await page.evaluateOnNewDocument(() => {
    // showSaveFilePicker di headless gantung (tanpa user interaction).
    // Hilangkan → factory jatuh ke OPFS/memory writer.
    Object.defineProperty(window, "showSaveFilePicker", { value: undefined, writable: false });
    const orig = URL.createObjectURL.bind(URL);
    URL.createObjectURL = (obj) => {
      if (obj instanceof Blob && obj.size > 1_000_000) {
        window.__lastBlob = obj;
        console.log(`[blobhook] capture blob size=${obj.size} type=${obj.type}`);
      }
      return orig(obj);
    };
  });

  try {
    await page.goto("http://localhost:5173/", { waitUntil: "networkidle2", timeout: 60000 });
    await sleep(2500);
    step("new project");
    await page.evaluate(() => {
      const els = [...document.querySelectorAll("button")];
      els.find((x) => /new project/i.test(x.textContent || ""))?.click();
    });
    await sleep(1200);
    const videoHandle = await page.evaluateHandle(
      () => [...document.querySelectorAll('input[type="file"]')][0],
    );
    await videoHandle.asElement().uploadFile(VIDEO);
    await sleep(4000);
    step("use subtitle file");
    await page.evaluate(() => {
      const els = [...document.querySelectorAll("button")];
      els.find((x) => /use subtitle file/i.test(x.textContent || ""))?.click();
    });
    await sleep(1500);
    const srtHandle = await page.evaluateHandle(
      () => document.querySelector('input[type="file"][accept*="srt"]') ??
        [...document.querySelectorAll('input[type="file"]')].pop(),
    );
    await srtHandle.asElement().uploadFile(SRT);
    await sleep(2500);
    step("start");
    await page.evaluate(() => {
      const els = [...document.querySelectorAll("button")];
      els.find((x) => x.textContent.trim() === "Start")?.click();
    });
    // Tunggu preprocessing editor selesai (re-encode video).
    for (let i = 0; i < 60; i++) {
      await sleep(2000);
      const s = await page.evaluate(() => ({
        preparing: /preparing|re-encoding/i.test(document.body.innerText),
        hasDialog: !!document.querySelector('[role=dialog]'),
      }));
      if (!s.preparing && !s.hasDialog) break;
    }
    step("editor up");

    // Hook download blob (anchor).
    await page.evaluate(() => {
      const w = window;
      w.__downloads = [];
      const origClick = HTMLAnchorElement.prototype.click;
      HTMLAnchorElement.prototype.click = function () {
        if (this.download) w.__downloads.push({ name: this.download, t: Date.now() });
        return origClick.apply(this, arguments);
      };
    });

    step("open export dialog");
    await page.evaluate(() => {
      const els = [...document.querySelectorAll("button")];
      els.find((x) => x.textContent.trim() === "Export")?.click();
    });
    await sleep(2000);

    // Mulai monitor sebelum klik Export.
    step("start system monitor");
    monitorRunning = true;
    t0 = Date.now();
    const monitorPromise = monitorLoop();

    // Klik Export.
    const exportBtn = await page.evaluateHandle(() => {
      const d = document.querySelector('[role=dialog]');
      if (!d) return null;
      const btns = [...d.querySelectorAll("button")];
      return btns.find((b) => b.textContent.trim() === "Export") ?? null;
    });
    const el = exportBtn.asElement();
    if (!el) throw new Error("tombol Export dalam dialog tidak ditemukan");
    await el.click();
    step("clicked Export — monitoring active");

    // Tunggu anchor-download (ekspor selesai).
    const deadline = t0 + 25 * 60 * 1000;
    let done = false;
    while (Date.now() < deadline) {
      const info = await page.evaluate(() => {
        const downloads = window.__downloads ?? [];
        const splash = /exporting/i.test(document.body.innerText);
        return { downloads: downloads.length, splash };
      });
      if (info.downloads > 0) {
        console.log(
          `[info] download detected after ${((Date.now() - t0) / 1000).toFixed(1)}s`,
        );
        done = true;
        break;
      }
      if ((Date.now() - t0) % 15000 < 2100) {
        console.log(`[progress @${((Date.now() - t0) / 1000).toFixed(0)}s] splash=${info.splash}`);
        const txt = await page.evaluate(() => {
          const d = document.querySelector('[role=dialog]');
          const el = d ?? document.body;
          return el.textContent.replace(/\s+/g, " ").slice(0, 400);
        });
        console.log(`[state @${((Date.now() - t0) / 1000).toFixed(0)}s] ${txt}`);
        await page.screenshot({
          path: `E:\\temp\\opencode\\tscaps_state_${((Date.now() - t0) / 1000).toFixed(0)}s.png`,
        });
      }
      await sleep(200);
    }

    monitorRunning = false;
    await monitorPromise;
    await sampleOnce(); // final sample

    // Salin blob hasil ekspor ke file.
    try {
      const saved = await page.evaluate(async () => {
        const blob = window.__lastBlob;
        if (!blob) return -1;
        await window.__beginSave();
        const CH = 1 << 20;
        for (let off = 0; off < blob.size; off += CH) {
          const bytes = new Uint8Array(
            await blob.slice(off, Math.min(off + CH, blob.size)).arrayBuffer(),
          );
          let s = "";
          for (let i = 0; i < bytes.length; i += 0x8000) {
            s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
          }
          await window.__saveChunk(btoa(s));
        }
        return blob.size;
      });
      console.log(`[info] blob tersimpan: ${saved} bytes → ${outFile}`);
    } catch (e) {
      err(`blob save gagal: ${String(e)}`);
    }

    const elapsedSec = (Date.now() - t0) / 1000;
    const peak = samples.reduce((m, s) => ({
      ram: Math.max(m.ram, s.ramUsedGB),
      cpu: Math.max(m.cpu, s.cpuPct),
    }), { ram: 0, cpu: 0 });
    const avg = {
      ram: +(samples.reduce((a, s) => a + s.ramUsedGB, 0) / Math.max(1, samples.length)).toFixed(2),
      cpu: +(samples.reduce((a, s) => a + s.cpuPct, 0) / Math.max(1, samples.length)).toFixed(1),
    };
    console.log("\n════════════════════════════════════════");
    console.log(done ? "✅ EXPORT SELESAI (tscaps)" : "❌ EXPORT TIMEOUT/GAGAL (tscaps)");
    console.log(`⏱  Durasi: ${(elapsedSec / 60).toFixed(2)} menit (${elapsedSec.toFixed(1)}s)`);
    console.log(`📊 Samples: ${samples.length} (interval 2s)`);
    console.log("── Peak ──");
    console.log(`RAM used:     ${peak.ram} GB`);
    console.log(`CPU:          ${peak.cpu}%`);
    console.log("── Average ──");
    console.log(`RAM used:     ${avg.ram} GB`);
    console.log(`CPU:          ${avg.cpu}%`);
    console.log("════════════════════════════════════════");
    console.log(`CSV:  ${OUT_CSV}`);
    console.log(`JSON: ${OUT_JSON}`);

    const result = {
      done,
      elapsedSec: +elapsedSec.toFixed(1),
      video: VIDEO,
      srt: SRT,
      samples: samples.length,
      peak,
      avg,
    };
    fs.writeFileSync(OUT_JSON, JSON.stringify(result, null, 1));
    const csv = ["t_sec,ramUsedGB,ramTotalGB,cpuPct"]
      .concat(samples.map((s) => `${s.t.toFixed(1)},${s.ramUsedGB},${s.ramTotalGB},${s.cpuPct}`))
      .join("\n");
    fs.writeFileSync(OUT_CSV, csv);
  } catch (e) {
    err(String(e));
    await page.screenshot({ path: "E:\\temp\\opencode\\tscaps_error.png" }).catch(() => {});
  } finally {
    monitorRunning = false;
    await browser.close();
  }
  process.exit(0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
