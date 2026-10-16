import puppeteer from "puppeteer-core";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await puppeteer.launch({ executablePath: CHROME, headless: true, defaultViewport: { width: 1920, height: 1080 } });
try {
  const page = await browser.newPage();
  page.on("pageerror", (e) => console.log("[pageerror]", e.message));
  await page.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(500);
  await page.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await page.waitForSelector('input[type="file"]');
  const vh = await page.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile("E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4");
  await page.waitForSelector(".dialog");
  await sleep(400);
  await page.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await page.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(2500);
  const info = await page.evaluate(() => {
    const s = window.__rscaps?.store;
    return {
      svgLen: (s?.svg || "").length,
      svgHead: (s?.svg || "").slice(0, 300),
      frame: s?.frame,
      duration: s?.durationFrames,
      cues: s?.cues?.length,
      docHead: (s?.docText || "").slice(0, 400),
      status: s?.status,
      templates: (s?.templates || []).length,
    };
  });
  console.log(JSON.stringify(info, null, 2));
} catch (e) { console.log("[ERR]", String(e)); }
finally { await browser.close(); }
process.exit(0);
