import puppeteer from "puppeteer-core";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await puppeteer.launch({ executablePath: CHROME, headless: true, defaultViewport: { width: 1920, height: 1080 } });
try {
  const page = await browser.newPage();
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
  await sleep(2000);
  const dump = await page.evaluate(async () => {
    const s = window.__rscaps.store;
    const out = {};
    for (const f of [0, 15, 30, 60]) {
      s.seekTo(f);
      await new Promise((r) => setTimeout(r, 120));
      out[f] = s.svg;
    }
    return out;
  });
  for (const [f, svg] of Object.entries(dump)) {
    const clips = [...svg.matchAll(/<rect[^>]*>/g)].map((m) => m[0]).join(" ");
    const texts = [...svg.matchAll(/<text[^>]*y="([\d.]+)"[^>]*>([^<]*)/g)].map((m) => m[2]).join("|");
    console.log(`frame ${f}: rects=[${clips}] text=${texts}`);
  }
  // screenshot at frame 30
  await page.evaluate(() => window.__rscaps.store.seekTo(30));
  await sleep(400);
  await page.screenshot({ path: "E:\\temp\\opencode\\f1_frame30.png" });
  console.log("saved f1_frame30.png");
} catch (e) { console.log("[ERR]", String(e)); }
finally { await browser.close(); }
process.exit(0);
