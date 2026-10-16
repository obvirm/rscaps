// Benchmark ekspor web (studio) vs CLI: buat project → pilih template →
// ukur waktu ekspor burn-in via tombol Export.
import puppeteer from "puppeteer-core";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const URL = "http://localhost:5177/";
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT = "E:\\project\\rscaps\\rscaps-sample.srt";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (m) => console.log(`[step] ${m}`);
const err = (m) => console.log(`[ERR] ${m}`);

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  args: ["--window-size=1920,1080", "--hide-scrollbars"],
  defaultViewport: { width: 1920, height: 1080 },
});
const page = await browser.newPage();
page.on("pageerror", (e) => err(`pageerror: ${e.message}`));
// Log SEMUA console (kecuali noise) untuk debug export.
page.on("console", (m) => {
  const t = m.text();
  if (!/favicon|Download the React DevTools/i.test(t)) {
    console.log(`[console.${m.type()}] ${t.slice(0, 400)}`);
  }
});

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

try {
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(600);
  step("new project");
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
  step("use subtitle file");
  await clickByText(".dialog button", "Use subtitle file instead");
  await page.waitForSelector('input[type="file"][accept*="srt"]', { timeout: 15000 });
  const srtHandle = await page.evaluateHandle(() =>
    document.querySelector('input[type="file"][accept*="srt"]'),
  );
  await srtHandle.asElement().uploadFile(SRT);

  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(2000);
  step("editor up");

  // Pilih template (cleo).
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
  await clickByText("button", "Export");
  await sleep(800);

  // Pilih format WebM (Opus) di Advanced options - AAC bawaan gagal di
  // video test (8000Hz mono → mp4a.40.5 tak support).
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

  // Ukur waktu ekspor.
  step("start export (mp4, keep resolution)");
  const t0 = Date.now();
  // Hook download blob: store.downloadBlob pakai <a download> + objectURL -
  // tidak lewat response event, jadi intercept di DOM.
  await page.evaluate(() => {
    const w = window;
    w.__downloads = [];
    const origClick = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function () {
      if (this.download) w.__downloads.push({ name: this.download, t: Date.now() });
      return origClick.apply(this, arguments);
    };
  });
  // Klik native (bukan evaluate) supaya Svelte onclick terpicu.
  const exportBtn = await page.$(".actions-right .btn.primary");
  if (!exportBtn) throw new Error("tombol Export tidak ditemukan");
  await exportBtn.click();
  step("clicked Export");

  // Tunggu: download blob / exported / dialog hilang / timeout.
  const deadline = t0 + 180_000;
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
      return { exporting, dialogOpen, status, downloads: downloads.length, dlName: downloads[0]?.name ?? "" };
    });
    if (info.downloads > 0) {
      console.log(`[info] download detected: "${info.dlName}" after ${((Date.now() - t0) / 1000).toFixed(1)}s`);
      done = true;
      break;
    }
    if (info.status && info.status !== lastStatus) {
      console.log(`[status] exporting=${info.exporting} dialog=${info.dialogOpen} :: ${info.status}`);
      seenStatuses.push(info.status);
      lastStatus = info.status;
    }
    if (/exported/i.test(info.status)) {
      done = true;
      break;
    }
    if (!info.exporting && !info.dialogOpen && seenStatuses.length > 0) {
      console.log(`[info] dialog closed after ${((Date.now() - t0) / 1000).toFixed(1)}s, statuses: ${JSON.stringify(seenStatuses)}`);
      done = seenStatuses.some((s) => /exported/i.test(s));
      break;
    }
    await sleep(150);
  }
  const elapsed = Date.now() - t0;
  if (done) {
    console.log(`[result] WEB export selesai dalam ${(elapsed / 1000).toFixed(1)}s`);
  } else {
    err(`export tidak selesai dalam 180s (elapsed ${(elapsed / 1000).toFixed(1)}s)`);
    await page.screenshot({ path: "E:\\temp\\opencode\\bench_web_export_stuck.png" });
    step("screenshot saved");
  }
} catch (e) {
  err(String(e));
} finally {
  await browser.close();
}
process.exit(0);
