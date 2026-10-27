// Paritas piksel preview baru (occlusion strip + komposit GPU) vs raster
// usvgr murni (svg asli ber-mask) — dijalankan di dev server (kode pipeline
// identik dengan build produksi).
import puppeteer from "puppeteer-core";
const browser = await puppeteer.launch({
  executablePath: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  headless: false,
  defaultViewport: { width: 1500, height: 950 },
  protocolTimeout: 900000,
  args: ["--autoplay-policy=no-user-gesture-required"],
});
const page = await browser.newPage();
await page.goto("http://localhost:5177/", { waitUntil: "domcontentloaded", timeout: 60000 });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
try {
  await sleep(800);
  await page.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await page.waitForSelector('input[type="file"]', { timeout: 30000 });
  const vh = await page.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile("E:\\project\\movie2short\\data\\output\\jobs\\1791884396610-359dd5\\filter_test.mp4");
  await page.waitForSelector(".dialog", { timeout: 60000 });
  await sleep(400);
  await page.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await page.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await page.waitForSelector("header.toolbar", { timeout: 120000 });
  await sleep(800);
  const setup = await page.evaluate(async () => {
    const s = window.__rscaps.store;
    await s.applyTemplateFresh("pastor");
    for (let i = 0; i < 480 && s.personMaskState !== "done" && s.personMaskState !== "error"; i++) {
      await new Promise((r) => setTimeout(r, 500));
    }
    return { state: s.personMaskState, ranges: (s.doc.behind_ranges ?? []).length };
  });
  console.log("[setup]", JSON.stringify(setup));

  const r = await page.evaluate(async () => {
    const s = window.__rscaps.store;
    const t = 5.585416666666668;
    const f = Math.round(t * s.fps);
    s.seekTo(f);
    await new Promise((res) => setTimeout(res, 1200));

    const canvas = document.querySelector("canvas.burn-canvas");
    if (!canvas) return { err: "no burn-canvas" };
    const pctx = canvas.getContext("2d");
    const prev = pctx.getImageData(0, 0, canvas.width, canvas.height);

    const v = document.querySelector("video");
    const bmp = await createImageBitmap(v);
    const c = new OffscreenCanvas(bmp.width, bmp.height);
    const cc = c.getContext("2d");
    cc.drawImage(bmp, 0, 0);
    const frameRgba = new Uint8Array(cc.getImageData(0, 0, bmp.width, bmp.height).data.buffer);

    const blob = await c.convertToBlob({ type: "image/jpeg", quality: 0.75 });
    s.wasm.setFrameImage(new Uint8Array(await blob.arrayBuffer()));
    s.supplyPersonMask(t);
    const raw = s.renderFrameSvgLayers(f)?.base ?? "";

    const { burnFrameWasm } = await import("/src/lib/svgrwasm.ts");
    const ref = await burnFrameWasm(frameRgba, raw, "", bmp.width, bmp.height);

    let se = 0, bad = 0, maxD = 0;
    const n = prev.data.length;
    for (let i = 0; i < n; i += 4) {
      for (let k = 0; k < 3; k++) {
        const d = prev.data[i + k] - ref[i + k];
        se += d * d;
        if (Math.abs(d) > maxD) maxD = Math.abs(d);
      }
      if (Math.abs(prev.data[i] - ref[i]) > 24) bad++;
    }
    const mse = se / (n / 4 * 3);
    const psnr = mse > 0 ? 10 * Math.log10(255 * 255 / mse) : 99;
    return {
      w: canvas.width, h: canvas.height,
      psnr: +psnr.toFixed(2), maxD, badPx: bad,
      svgHasMask: raw.includes("pastor-person"),
      svgLen: raw.length,
    };
  });
  console.log("[parity]", JSON.stringify(r, null, 1));
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 250));
} finally {
  await browser.close();
}
