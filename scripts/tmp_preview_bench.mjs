// Bench preview burn pipeline: chrome (dev 5177) atau tauri (WebView2 CDP 9333).
// Pemakaian: node scripts/tmp_preview_bench.mjs chrome|tauri
import puppeteer from "puppeteer-core";

const MODE = process.argv[2] ?? "chrome";
const VIDEO = "E:\\project\\movie2short\\data\\output\\jobs\\1791884396610-359dd5\\filter_test.mp4";
const SRT = "E:\\project\\rscaps\\rscaps-sample.srt";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let browser;
let page;
if (MODE === "chrome") {
  browser = await puppeteer.launch({
    executablePath: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    headless: false,
    defaultViewport: { width: 1500, height: 950 },
    protocolTimeout: 900000,
    args: ["--autoplay-policy=no-user-gesture-required"],
  });
  page = await browser.newPage();
  await page.goto("http://localhost:5177/", { waitUntil: "domcontentloaded", timeout: 60000 });
} else {
  const ver = await (await fetch("http://127.0.0.1:9333/json/version")).json();
  browser = await puppeteer.connect({ browserWSEndpoint: ver.webSocketDebuggerUrl });
  const pages = await browser.pages();
  page = pages.find((p) => /tauri\.localhost|localhost/.test(p.url())) ?? pages[0];
  await page.bringToFront();
}

const errors = [];
const workerLogs = [];
page.on("pageerror", (e) => errors.push(String(e).slice(0, 160)));
page.on("console", (m) => {
  const t = m.text();
  if (t.includes("[burnWorker]")) workerLogs.push(t.slice(0, 200));
  if (m.type() === "error") errors.push("[console] " + t.slice(0, 160));
});

try {
  await sleep(MODE === "tauri" ? 2500 : 800);
  await page.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await page.waitForSelector('input[type="file"]', { timeout: 30000 });
  const vh = await page.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile(VIDEO);
  await page.waitForSelector(".dialog", { timeout: 60000 });
  await sleep(400);
  await page.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await page.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile(SRT);
  await page.waitForSelector("header.toolbar", { timeout: 120000 });
  await sleep(1000);

  const setup = await page.evaluate(async () => {
    const s = window.__rscaps.store;
    await s.applyTemplateFresh("pastor");
    for (let i = 0; i < 480 && s.personMaskState !== "done" && s.personMaskState !== "error"; i++) {
      await new Promise((r) => setTimeout(r, 500));
    }
    // Instrumen rate pump: renderFrameSvgLayers dipanggil sekali per pass.
    window.__pumpN = 0;
    const orig = s.renderFrameSvgLayers.bind(s);
    s.renderFrameSvgLayers = (f) => { window.__pumpN++; return orig(f); };
    return { state: s.personMaskState, ranges: (s.doc.behind_ranges ?? []).length };
  });
  console.log("[setup]", JSON.stringify(setup));

  // Putar 12 detik dari AWAL video (range behind pertama ada di awal).
  await page.evaluate(() => {
    const s = window.__rscaps.store;
    s.seekTo(0);
    s.togglePlay();
  });
  const f0 = await page.evaluate(() => ({ f: window.__rscaps.store.frame, t: performance.now() }));
  await sleep(6000);
  await page.screenshot({ path: `E:\\temp\\opencode\\bench_${MODE}_mid.png` });
  await sleep(6000);
  const res = await page.evaluate((f0) => {
    const s = window.__rscaps.store;
    const v = document.querySelector("video");
    const q = v?.getVideoPlaybackQuality?.();
    s.stopPlay();
    return {
      framesAdvanced: s.frame - f0.f,
      dt: +((performance.now() - f0.t) / 1000).toFixed(1),
      videoDropped: q?.droppedVideoFrames ?? -1,
      svgLen: s.svg.length,
      pumpPasses: window.__pumpN,
    };
  }, f0);
  console.log("[bench " + MODE + "]", JSON.stringify(res));
  if (workerLogs.length) console.log("[worker]", JSON.stringify(workerLogs.slice(-14), null, 1));
  if (errors.length) console.log("[errors]", JSON.stringify([...new Set(errors)].slice(0, 5)));
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 300));
  if (errors.length) console.log("[errors]", JSON.stringify([...new Set(errors)].slice(0, 5)));
} finally {
  if (MODE === "chrome") await browser.close();
  else browser.disconnect();
}
