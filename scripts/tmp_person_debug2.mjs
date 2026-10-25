// Ground truth: supply frame + masker EKSPLISIT lalu dump SVG + screenshot.
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
    const v = document.querySelector("video");
    v.currentTime = t;
    await new Promise((r2) => setTimeout(r2, 400));
    const bmp = await createImageBitmap(v);
    const okFrame = await s.supplyFrameImage(bmp);
    s.supplyPersonMask(t);
    const raw = s.wasm.bridge.render_frame(BigInt(f));
    return {
      state: s.personMaskState,
      samples: s.personMaskClient.entries.length,
      ranges: s.doc?.behind_ranges,
      okFrame,
      hasOccl: raw.includes("<mask id=\"pastor-person\""),
      hasGlass: raw.includes("rscaps-glass"),
      len: raw.length,
      head: raw.slice(0, 400),
      textSnips: (raw.match(/<text[^>]{0,300}>/g) || []).slice(0, 6),
      svg: raw,
    };
  });
  console.log("[emit]", JSON.stringify({
    okFrame: r.okFrame, hasOccl: r.hasOccl, hasGlass: r.hasGlass, len: r.len,
    textSnips: r.textSnips,
  }));
  writeFileSync("E:\\temp\\opencode\\pastor_raw.svg", r.svg);
  console.log("[saved] pastor_raw.svg");
  // Screenshot lewat pump (jalur preview normal) — mask+frame sudah tersedia.
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
