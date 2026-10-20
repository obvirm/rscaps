// Diagnostic + screenshot flow for rscaps studio. Step-logged, never hangs.
import puppeteer from "puppeteer-core";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const URL = "http://localhost:5177/";
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT = "E:\\project\\rscaps\\rscaps-sample.srt";

const out = process.argv[2] || "E:\\temp\\opencode\\f1_editor.png";
const tabsArg = process.argv.find((a) => a.startsWith("--tabs="));
const tabs = tabsArg ? tabsArg.slice(7).split(",") : [];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (msg) => console.log(`[step] ${msg}`);
const err = (msg) => console.log(`[ERR] ${msg}`);

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  args: ["--window-size=1920,1080", "--hide-scrollbars"],
  defaultViewport: { width: 1920, height: 1080 },
});
try {
  const page = await browser.newPage();
  page.on("console", (m) => console.log(`[console.${m.type()}]`, m.text()));
  page.on("pageerror", (e) => err(`pageerror: ${e.message}`));
  page.on("response", (r) => {
    if (r.status() >= 400) err(`HTTP ${r.status()} ${r.url()}`);
  });

  step("goto");
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });

  const clickByText = async (sel, text) => {
    const ok = await page.evaluate(
      (sel, text) => {
        const els = [...document.querySelectorAll(sel)];
        const el = els.find((e) => e.textContent.trim().includes(text));
        if (el) {
          el.click();
          return true;
        }
        return false;
      },
      sel,
      text,
    );
    if (!ok) throw new Error(`not found: ${sel} "${text}"`);
  };

  step("click New project");
  await sleep(600);
  await clickByText("button", "New project");

  step("upload video");
  await page.waitForSelector('input[type="file"]', { timeout: 15000 });
  await sleep(300);
  const videoHandle = await page.evaluateHandle(() => {
    const inputs = [...document.querySelectorAll('input[type="file"]')];
    const v = inputs.find((i) => !i.accept.includes("srt"));
    if (!v) throw new Error("video input not found");
    return v;
  });
  await videoHandle.asElement().uploadFile(VIDEO);

  step("wait dialog");
  await page.waitForSelector(".dialog", { timeout: 15000 });
  await sleep(500);

  step("click Use subtitle file");
  await clickByText(".dialog button", "Use subtitle file instead");
  await page.waitForSelector('input[type="file"][accept*="srt"]', { timeout: 15000 });
  const srtHandle = await page.evaluateHandle(() =>
    document.querySelector('input[type="file"][accept*="srt"]'),
  );
  step("upload srt");
  await srtHandle.asElement().uploadFile(SRT);

  step("wait editor shell (60s)");
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  step("editor shell up");
  await sleep(2000);

  const shot = async (path) => {
    await page.screenshot({ path });
    console.log("saved", path);
  };

  await shot(out);

  if (tabs.includes("transcript-word") || tabs.includes("words")) {
    step("open transcript word view");
    await page.evaluate(() => {
      const btns = Array.from(document.querySelectorAll(".view-toggle button"));
      const w = btns.find((b) => b.textContent?.trim() === "Word");
      if (w) w.click();
    });
    await new Promise((r) => setTimeout(r, 400));
  }
  for (const tab of tabs) {
    const ok = await page.evaluate((tab) => {
      const btns = [...document.querySelectorAll(".rail-tab")];
      const b = btns.find((x) => (x.getAttribute("aria-label") || "").toLowerCase() === tab);
      if (b) b.click();
      return !!b;
    }, tab.toLowerCase());
    if (tabs.includes("words")) {
      await sleep(400);
      await page.evaluate(() => {
        const btns = Array.from(document.querySelectorAll(".view-toggle button"));
        const w = btns.find((b) => b.textContent?.trim() === "Word");
        if (w) w.click();
      });
      await sleep(600);
      await shot(out.replace(/\.png$/, "_words.png"));
      continue;
    }
    if (!ok) {
      err(`tab not found: ${tab}`);
      continue;
    }
    await sleep(900);
    await shot(out.replace(/\.png$/, `_${tab}.png`));
  }
} catch (e) {
  err(String(e));
} finally {
  await browser.close();
}
process.exit(0);
