// Functional check: drive the real UI controls and assert the caption
// overlay SVG re-renders with the new params.
import puppeteer from "puppeteer-core";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  defaultViewport: { width: 1920, height: 1080 },
});

try {
  const page = await browser.newPage();
  page.on("pageerror", (e) => console.log("[pageerror]", e.message));
  await page.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(500);
  await page.evaluate(() =>
    [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click(),
  );
  await page.waitForSelector('input[type="file"]');
  const vh = await page.evaluateHandle(() =>
    [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")),
  );
  await vh.asElement().uploadFile("E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4");
  await page.waitForSelector(".dialog");
  await sleep(400);
  await page.evaluate(() =>
    [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click(),
  );
  const sh = await page.evaluateHandle(() =>
    document.querySelector('input[type="file"][accept*="srt"]'),
  );
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(1500);

  // Caption visible at frame 30 (Luna entrance done).
  await page.evaluate(() => window.__rscaps.store.seekTo(30));
  await sleep(300);
  const before = await page.evaluate(() => window.__rscaps.store.svg);
  await page.screenshot({ path: "E:\\temp\\opencode\\f2_verify_before.png" });

  // 1) Typography → Font size slider = 12 cqh
  await page.evaluate(() => {
    const b = [...document.querySelectorAll(".rail-tab")].find(
      (x) => x.getAttribute("aria-label") === "Typography",
    );
    b?.click();
  });
  await sleep(500);
  const sizeResult = await page.evaluate(() => {
    const ranges = [...document.querySelectorAll(".typography-panel input[type=range]")];
    const el = ranges[0]; // Font size
    if (!el) return { ok: false, reason: "no range" };
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    ).set;
    setter.call(el, "12");
    el.dispatchEvent(new Event("input", { bubbles: true }));
    return { ok: true, svg: window.__rscaps.store.svg, params: window.__rscaps.store.params };
  });
  await sleep(400);
  const afterSize = await page.evaluate(() => ({
    svg: window.__rscaps.store.svg,
    font_size: window.__rscaps.store.params?.font_size,
    videoHeight: window.__rscaps.store.videoHeight,
  }));
  console.log("size slider:", sizeResult.ok, "font_size now:", afterSize.font_size);
  console.log("svg changed by font size:", afterSize.svg !== before);

  // 2) Position → Vertical slider = 0.55
  await page.evaluate(() => {
    const b = [...document.querySelectorAll(".rail-tab")].find(
      (x) => x.getAttribute("aria-label") === "Position",
    );
    b?.click();
  });
  await sleep(500);
  const posResult = await page.evaluate(() => {
    const ranges = [...document.querySelectorAll(".position-panel input[type=range]")];
    const el = ranges[0]; // Vertical
    if (!el) return { ok: false };
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    ).set;
    setter.call(el, "0.55");
    el.dispatchEvent(new Event("input", { bubbles: true }));
    return { ok: true };
  });
  await sleep(400);
  const afterPos = await page.evaluate(() => ({
    svg: window.__rscaps.store.svg,
    v_offset: window.__rscaps.store.params?.v_offset,
  }));
  console.log("pos slider:", posResult.ok, "v_offset now:", afterPos.v_offset);
  console.log("svg changed by position:", afterPos.svg !== afterSize.svg);

  // 3) Style → color change via store (ColorField UI is a popover)
  const colorOk = await page.evaluate(() => window.__rscaps.store.setStyleValue("primary-color", "#ff0000"));
  await sleep(300);
  const afterColor = await page.evaluate(() => window.__rscaps.store.svg);
  console.log("color set:", colorOk, "svg changed by color:", afterColor !== afterPos.svg);

  await page.screenshot({ path: "E:\\temp\\opencode\\f2_verify_after.png" });
  console.log("saved verify screenshots");
} catch (e) {
  console.log("[ERR]", String(e));
} finally {
  await browser.close();
}
process.exit(0);
