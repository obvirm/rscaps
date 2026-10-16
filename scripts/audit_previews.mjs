import puppeteer from "puppeteer-core";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await puppeteer.launch({ executablePath: CHROME, headless: true, defaultViewport: { width: 1400, height: 900 } });
try {
  const page = await browser.newPage();
  page.on("pageerror", (e) => console.log("[pageerror]", e.message));
  await page.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(500);
  await page.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await page.waitForSelector('input[type="file"]');
  const vh = await page.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile("E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4");
  await page.waitForSelector(".dialog"); await sleep(400);
  await page.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await page.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(2000);
  const audit = await page.evaluate(() => {
    const s = window.__rscaps.store;
    const bad = [];
    const wrongWord = [];
    for (const t of s.templates) {
      const svg = s.previews[t.id] || "";
      if (!svg.includes("<text")) bad.push(t.id);
      if (svg && !svg.toLowerCase().includes("rscaps")) wrongWord.push(t.id);
    }
    const card = document.querySelector(".sidebar-card");
    const col = document.querySelector(".sidebar-col");
    return {
      total: s.templates.length,
      noText: bad,
      wrongWord,
      cardW: Math.round(card?.getBoundingClientRect().width ?? -1),
      colW: Math.round(col?.getBoundingClientRect().width ?? -1),
      win: innerWidth,
    };
  });
  console.log(JSON.stringify(audit, null, 1));
  await page.screenshot({ path: "E:\\temp\\opencode\\f6_audit.png" });
} catch (e) { console.log("[ERR]", String(e)); }
finally { await browser.close(); }
process.exit(0);
