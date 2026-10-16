import puppeteer from "puppeteer-core";
import fs from "node:fs";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT = "E:\\project\\rscaps\\scripts\\battery.srt";
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
  await vh.asElement().uploadFile(VIDEO);
  await p.waitForSelector(".dialog"); await sleep(400);
  await p.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await p.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile(SRT);
  await p.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(1500);
  // seek ke 8.2 (big word RSCAPS tampil) lalu set milo + tanam blend di anchor texts
  const info = await p.evaluate(async () => {
    const s = window.__rscaps.store;
    await s.applyTemplateFresh("milo");
    s.seekTo(Math.round(8.2 * s.fps));
    await new Promise((r) => setTimeout(r, 500));
    const before = s.svg;
    const blended = before.replace(/<text /g, '<text style="mix-blend-mode: difference" ');
    s.svg = blended;
    await new Promise((r) => setTimeout(r, 300));
    const bigMatch = blended.match(/<text[^>]*>([^<]{4,})<\/text>/g) || [];
    return { texts: bigMatch.slice(0, 6), applied: blended !== before };
  });
  console.log(JSON.stringify(info, null, 2));
  const box = await p.$(".video-box");
  if (box) await box.screenshot({ path: "E:\\temp\\opencode\\compare\\blend_milo.png" });
  else await p.screenshot({ path: "E:\\temp\\opencode\\compare\\blend_milo.png" });
  console.log("shot saved");
} catch (e) { console.log("[ERR]", String(e)); }
finally { await b.close(); }
