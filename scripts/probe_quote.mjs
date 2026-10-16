import puppeteer from "puppeteer-core";
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
  const out = await p.evaluate(async () => {
    const s = window.__rscaps.store;
    await s.applyTemplateFresh("cleo");
    s.seekTo(Math.round(1.2 * window.__rscaps.store.fps));
    await new Promise((r) => setTimeout(r, 300));
    const before = s.svg;
    const doc = JSON.parse(s.docText);
    const cue1 = doc.cues?.[0]?.text ?? null;
    const ok = s.setStyleValue("quote-color", "#ff0000");
    const stored = doc.params?.style_values;
    const sv = JSON.parse(s.docText).params?.style_values;
    await new Promise((r) => setTimeout(r, 300));
    const after = s.svg;
    return {
      cue1,
      frame: s.frame,
      ok, sv,
      hasQuoteInCue: /["“”']/.test(cue1 ?? ""),
      changed: before !== after,
      hasRed: after.includes("ff0000"),
      texts: [...after.matchAll(/>[^<>{}]+</g)].map(m=>m[0]).slice(0,30),
      fillSample: (after.match(/fill="[^"]+"/g) ?? []).slice(0, 8),
    };
  });
  console.log(JSON.stringify(out, null, 2));
} catch (e) { console.log("[ERR]", String(e)); }
finally { await b.close(); }
