// Repro: buka editor di beberapa lebar jendela + audit preview kosong.
import puppeteer from "puppeteer-core";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const widths = [
  { w: 1000, h: 700 },
  { w: 760, h: 900 },
  { w: 900, h: 700 },
];

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  args: ["--hide-scrollbars"],
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

  // Audit preview: semua template harus punya SVG tak-kosong.
  const audit = await page.evaluate(() => {
    const s = window.__rscaps.store;
    const empties = [];
    for (const t of s.templates) {
      const svg = s.previews[t.id];
      if (!svg || svg.length < 100 || !svg.includes("<text")) empties.push(t.id);
    }
    return { total: s.templates.length, built: Object.keys(s.previews).length, empties };
  });
  console.log("AUDIT", JSON.stringify(audit));

  for (const size of widths) {
    await page.setViewport({ width: size.w, height: size.h });
    await sleep(600);
    const geo = await page.evaluate(() => {
      const ws = document.querySelector(".workspace");
      const card = document.querySelector(".sidebar-card");
      const center = document.querySelector(".center-col");
      const grid = ws ? getComputedStyle(ws).gridTemplateColumns : null;
      const r = (el) => (el ? Math.round(el.getBoundingClientRect().width) : -1);
      return {
        win: innerWidth,
        grid,
        center: r(center),
        card: r(card),
        sidebarCol: r(document.querySelector(".sidebar-col")),
      };
    });
    console.log(`W=${size.w} → ${JSON.stringify(geo)}`);
    await page.screenshot({ path: `E:\\temp\\opencode\\f6_w${size.w}.png` });
  }
} catch (e) {
  console.log("[ERR]", String(e));
} finally {
  await browser.close();
}
process.exit(0);
