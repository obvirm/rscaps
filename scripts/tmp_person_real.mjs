// Verifikasi occlusion dengan video berisi ORANG sungguhan: analisis real,
// behind_ranges otomatis, teks terpotong di belakang orang.
import puppeteer from "puppeteer-core";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const b = await puppeteer.launch({
  executablePath: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  headless: true,
  defaultViewport: { width: 1600, height: 1000 },
  protocolTimeout: 600000,
});
try {
  const p = await b.newPage();
  await p.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(600);
  await p.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await p.waitForSelector('input[type="file"]');
  const vh = await p.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile("E:\\project\\movie2short\\data\\output\\jobs\\1791884396610-359dd5\\filter_test.mp4");
  await p.waitForSelector(".dialog");
  await sleep(400);
  await p.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await p.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await p.waitForSelector("header.toolbar", { timeout: 180000 });
  await sleep(1200);
  let lastLog = "";
  const logProbe = await p.evaluateHandle(() => null);
  const r = await p.evaluate(async () => {
    const s = window.__rscaps.store;
    await s.applyTemplateFresh("pastor");
    window.__progress = [];
    const iv = setInterval(() => {
      window.__progress.push(`${s.personMaskState}:${Math.round(s.personMaskProgress * 100)}%`);
    }, 5000);
    for (let i = 0; i < 480 && s.personMaskState !== "done" && s.personMaskState !== "error"; i++) {
      await new Promise((r2) => setTimeout(r2, 500));
    }
    clearInterval(iv);
    const ranges = s.doc?.behind_ranges ?? [];
    // Frame uji: tengah rentang behind pertama; fallback tengah video.
    let t;
    if (ranges.length > 0) t = (ranges[0][0] + ranges[0][1]) / 2000;
    else t = Math.max(0.1, s.durationFrames / s.fps / 2);
    const f = Math.round(t * s.fps);
    const v = document.querySelector("video");
    if (v) v.currentTime = t;
    await new Promise((r2) => setTimeout(r2, 400));
    s.renderAt(f);
    return {
      state: s.personMaskState,
      samples: s.personMaskClient.entries.length,
      ranges,
      durationSec: Math.round((s.durationFrames / s.fps) * 10) / 10,
      testT: Math.round(t * 100) / 100,
      frame: f,
    };
  });
  console.log("[real]", JSON.stringify(r), "progress:", JSON.stringify(await p.evaluate(() => window.__progress)));
  await sleep(2500);
  const el = await p.$("canvas");
  if (el) await el.screenshot({ path: "E:\\temp\\opencode\\pastor_real.png" });
  console.log("[shot behind]");
  // Kontras: frame di LUAR rentang (register biasa, teks utuh).
  const r2 = await p.evaluate(() => {
    const s = window.__rscaps.store;
    const ranges = s.doc?.behind_ranges ?? [];
    const fps = s.fps;
    // Cari t di antara rentang: setelah rentang terakhir + margin 0.1s.
    let t = 0.05;
    if (ranges.length > 0 && ranges[ranges.length - 1][1] > 0) {
      const after = ranges[ranges.length - 1][1] / 1000 + 0.1;
      t = Math.min(after, s.durationFrames / fps - 0.05);
    }
    const v = document.querySelector("video");
    if (v) v.currentTime = t;
    s.renderAt(Math.round(t * fps));
    return t;
  });
  await sleep(2500);
  if (el) await el.screenshot({ path: "E:\\temp\\opencode\\pastor_real_off.png" });
  console.log("[shot outside]", r2.toFixed(2));
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 260));
} finally {
  await b.close();
}
