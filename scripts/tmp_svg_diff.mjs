import puppeteer from "puppeteer-core";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ver = await (await fetch("http://127.0.0.1:9333/json/version")).json();
const browser = await puppeteer.connect({ browserWSEndpoint: ver.webSocketDebuggerUrl });
const page = (await browser.pages())[0];
await page.bringToFront();
try {
  await sleep(2000);
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
    const grab = async (f) => {
      s.seekTo(f);
      await new Promise((res) => setTimeout(res, 250));
      const v = document.querySelector("video");
      const bmp = await createImageBitmap(v);
      await s.supplyFrameImage(bmp);
      s.supplyPersonMask(f / s.fps);
      return s.renderFrameSvgLayers(f)?.base ?? "";
    };
    const a = await grab(45);
    const b = await grab(50);
    const c = await grab(55);
    const OCCL_RE = /<clipPath id="pastor-occl"><rect x="([\d.-]+)" y="([\d.-]+)" width="([\d.-]+)" height="([\d.-]+)"\/><\/clipPath><mask id="pastor-person"[^>]*x="[^"]*"[^>]*>(?:[\s\S]*?)<\/mask><image[^>]*rscaps-glass[^>]*\/>/;
    const sa = a.replace(OCCL_RE, "");
    const sb = b.replace(OCCL_RE, "");
    const sc = c.replace(OCCL_RE, "");
    let diffAt = -1;
    for (let i = 0; i < Math.min(sa.length, sb.length); i++) {
      if (sa[i] !== sb[i]) { diffAt = i; break; }
    }
    return {
      strippedLenA: sa.length, strippedLenB: sb.length, strippedLenC: sc.length,
      strippedSameAB: sa === sb,
      strippedSameBC: sb === sc,
      diffAt,
      ctxA: diffAt >= 0 ? sa.slice(Math.max(0, diffAt - 80), diffAt + 80) : "",
      ctxB: diffAt >= 0 ? sb.slice(Math.max(0, diffAt - 80), diffAt + 80) : "",
    };
  });
  console.log("[diff]", JSON.stringify(r, null, 1));
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 250));
} finally {
  browser.disconnect();
}
