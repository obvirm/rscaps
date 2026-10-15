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
  await vh.asElement().uploadFile("E:\\project\\rscaps\\crates\\whisper-spike\\test\\portrait-test.mp4");
  await page.waitForSelector(".dialog");
  await sleep(400);
  await page.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await page.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(1500);
  // caption visible mid-cue, anchor bottom 0.75 (absolute)
  await page.evaluate(() => {
    const s = window.__rscaps.store;
    s.seekTo(30);
    s.setParams({ v_align: undefined, v_offset: 0.75, h_align: undefined, h_offset: 0.5 });
  });
  await sleep(500);
  const geo = await page.evaluate(() => {
    const box = document.querySelector(".video-box").getBoundingClientRect();
    const svg = document.querySelector(".stage svg");
    const sr = svg.getBoundingClientRect();
    return { box: {x: box.x, y: box.y, w: box.width, h: box.height}, svg: {x: sr.x, y: sr.y, w: sr.width, h: sr.height}, aspect: window.__rscaps.store.aspect };
  });
  console.log(JSON.stringify(geo));
  console.log("svg fills box:", Math.abs(geo.svg.w - geo.box.w) < 2 && Math.abs(geo.svg.h - geo.box.h) < 2);
  await page.screenshot({ path: "E:\\temp\\opencode\\f2_portrait.png" });
  console.log("saved f2_portrait.png");
} catch (e) { console.log("[ERR]", String(e)); }
finally { await browser.close(); }
process.exit(0);
