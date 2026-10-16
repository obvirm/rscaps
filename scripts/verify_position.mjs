import puppeteer from "puppeteer-core";
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
  await sleep(1500);
  // buka tab Position
  await p.evaluate(() => [...document.querySelectorAll(".rail-tab")].find((x) => x.getAttribute("aria-label") === "Position")?.click());
  await sleep(400);
  const check = async (tpl) => {
    await p.evaluate(async (tpl) => { await window.__rscaps.store.applyTemplateFresh(tpl); }, tpl);
    await sleep(600);
    return await p.evaluate(() => {
      const ranges = [...document.querySelectorAll(".position-panel input[type=range]")];
      const v = ranges[0]?.value, h = ranges[1]?.value;
      return { vertical: v, horizontal: h, anchorY: window.__rscaps.store.params?.v_offset ?? null };
    });
  };
  for (const tpl of ["loki", "mira", "kai", "luna"]) {
    console.log(tpl, JSON.stringify(await check(tpl)));
  }
  // grid preserve: luna (anchor 0.2 top) → klik "bottom"
  await p.evaluate(async () => { await window.__rscaps.store.applyTemplateFresh("luna"); });
  await sleep(500);
  const before = await p.evaluate(() => window.__rscaps.store.svg);
  await p.evaluate(() => {
    const cells = [...document.querySelectorAll(".position-panel .cell")];
    cells.find((c) => c.getAttribute("aria-label") === "bottom left")?.click();
  });
  await sleep(600);
  const after = await p.evaluate(() => ({ v_align: window.__rscaps.store.params?.v_align, v_offset: window.__rscaps.store.params?.v_offset }));
  console.log("grid bottom-left on luna:", JSON.stringify(after), "svgChanged:", before !== (await p.evaluate(() => window.__rscaps.store.svg)));
} catch (e) { console.log("[ERR]", String(e)); }
finally { await b.close(); }
