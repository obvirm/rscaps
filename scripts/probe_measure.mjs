import puppeteer from "puppeteer-core";
import fs from "node:fs";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, defaultViewport: { width: 1600, height: 1000 } });
try {
  const p = await b.newPage();
  p.on("pageerror", (e) => console.log("[pageerror]", e.message));
  await p.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(500);
  await p.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await p.waitForSelector('input[type="file"]');
  const vh = await p.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile("E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4");
  await p.waitForSelector(".dialog"); await sleep(400);
  await p.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await p.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await p.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(2000);
  const ids = ["loki", "iris", "cleo", "luca"];
  for (const id of ids) {
    await p.evaluate(async (id) => {
      const s = window.__rscaps.store;
      await s.applyTemplateFresh(id);
      s.seekTo(12);
    }, id);
    await sleep(600);
    const svg = await p.evaluate(() => window.__rscaps.store.svg);
    fs.writeFileSync(`E:\\temp\\opencode\\svg_${id}.svg`, svg);
    console.log(id, "len", svg.length);
  }
} catch (e) { console.log("[ERR]", String(e)); }
finally { await b.close(); }
