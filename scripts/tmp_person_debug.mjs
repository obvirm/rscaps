// Debug: visualkan masker person pada frame uji + statistik band + emit.
import puppeteer from "puppeteer-core";
import { writeFileSync } from "node:fs";
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
  const r = await p.evaluate(async () => {
    const s = window.__rscaps.store;
    await s.applyTemplateFresh("pastor");
    for (let i = 0; i < 480 && s.personMaskState !== "done" && s.personMaskState !== "error"; i++) {
      await new Promise((r2) => setTimeout(r2, 500));
    }
    const t = 5.585;
    const f = Math.round(t * s.fps);
    const idx = s.personMaskClient.nearestIndex(t);
    if (idx < 0) return { err: "no mask" };
    const png = s.personMaskPngs[idx];
    const bmp = await createImageBitmap(new Blob([png], { type: "image/png" }));
    // Statistik band di masker asli (alpha>128): top 30%, tengah, bawah 25%.
    const m = s.personMaskClient.entries[idx];
    const frac = (y0, y1) => {
      const a = Math.floor(m.height * y0), z = Math.floor(m.height * y1);
      let n = 0;
      for (let y = a; y < z; y++) for (let x = 0; x < m.width; x++) if (m.alpha[y * m.width + x] > 128) n++;
      return n / ((z - a) * m.width);
    };
    const c = new OffscreenCanvas(bmp.width, bmp.height);
    c.getContext("2d").drawImage(bmp, 0, 0);
    const blob = await c.convertToBlob({ type: "image/png" });
    const b64 = await new Promise((res) => {
      const fr = new FileReader();
      fr.onload = () => res(fr.result);
      fr.readAsDataURL(blob);
    });
    const raw = s.wasm.bridge.render_frame(BigInt(f));
    return {
      t,
      frame: f,
      maskSize: [bmp.width, bmp.height],
      top30: frac(0, 0.3),
      mid: frac(0.3, 0.75),
      bottom25: frac(0.75, 1),
      hasOccl: raw.includes("<mask id=\"pastor-person\""),
      hasGlass: raw.includes("rscaps-glass"),
      maskPng: b64,
      cuesNear: (s.doc?.cues ?? []).filter((c2) => c2.end_ms > t * 1000 - 1500 && c2.start_ms < t * 1000 + 1500).map((c2) => [c2.start_ms, c2.end_ms]),
      ranges: s.doc?.behind_ranges,
    };
  });
  console.log("[dbg]", JSON.stringify({ ...r, maskPng: (r.maskPng || "").slice(0, 40) + "..." }));
  writeFileSync("E:\\temp\\opencode\\pastor_mask.png", Buffer.from(r.maskPng.split(",")[1], "base64"));
  console.log("[saved] pastor_mask.png");
  // Render + screenshot frame uji lewat jalur preview normal.
  await p.evaluate((tt) => {
    const s = window.__rscaps.store;
    const v = document.querySelector("video");
    if (v) v.currentTime = tt;
    s.renderAt(Math.round(tt * s.fps));
  }, 5.585);
  await sleep(2500);
  const el = await p.$("canvas");
  if (el) await el.screenshot({ path: "E:\\temp\\opencode\\pastor_dbg.png" });
  console.log("[shot] pastor_dbg.png");
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 260));
} finally {
  await b.close();
}
