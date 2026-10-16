// Probe element inspector end-to-end: project → pilih template → seek → klik kata → panel.
import puppeteer from "puppeteer-core";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const URL = "http://localhost:5177/";
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT = "E:\\project\\rscaps\\rscaps-sample.srt";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = (m) => console.log(`[step] ${m}`);
const err = (m) => console.log(`[ERR] ${m}`);

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  args: ["--window-size=1920,1080", "--hide-scrollbars"],
  defaultViewport: { width: 1920, height: 1080 },
});
const page = await browser.newPage();
page.on("pageerror", (e) => err(e.message));

const clickByText = async (sel, text) => {
  const ok = await page.evaluate(
    (sel, text) => {
      const els = [...document.querySelectorAll(sel)];
      const el = els.find((e) => e.textContent.trim().includes(text));
      if (el) el.click();
      return !!el;
    },
    sel,
    text,
  );
  if (!ok) throw new Error(`not found: ${sel} "${text}"`);
};

try {
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
  await sleep(600);
  step("new project");
  await clickByText("button", "New project");

  await page.waitForSelector('input[type="file"]', { timeout: 15000 });
  await sleep(300);
  const videoHandle = await page.evaluateHandle(() => {
    const inputs = [...document.querySelectorAll('input[type="file"]')];
    return inputs.find((i) => !i.accept.includes("srt"));
  });
  await videoHandle.asElement().uploadFile(VIDEO);

  await page.waitForSelector(".dialog", { timeout: 15000 });
  await sleep(500);
  step("use subtitle file");
  await clickByText(".dialog button", "Use subtitle file instead");
  await page.waitForSelector('input[type="file"][accept*="srt"]', { timeout: 15000 });
  const srtHandle = await page.evaluateHandle(() =>
    document.querySelector('input[type="file"][accept*="srt"]'),
  );
  await srtHandle.asElement().uploadFile(SRT);

  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(2000);
  step("editor up");

  step("open Templates tab + pilih template pertama");
  await page.evaluate(() => {
    const btns = [...document.querySelectorAll(".rail-tab")];
    const b = btns.find((x) => (x.getAttribute("aria-label") || "").toLowerCase() === "templates");
    b?.click();
  });
  await sleep(800);
  const picked = await page.evaluate(() => {
    const cards = [...document.querySelectorAll(".tpl-card, .template-card, [class*=card]")];
    const c = cards.find((x) => x.textContent.trim().length > 0 && x.offsetWidth > 40);
    c?.click();
    return c ? c.textContent.trim().slice(0, 30) : null;
  });
  step(`template picked: ${picked}`);
  await sleep(1500);

  step("seek frame 15 + Render");
  await page.evaluate(() => {
    const input = document.querySelector(".frame-field input");
    if (!input) throw new Error("frame input not found");
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    setter.call(input, "15");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await sleep(200);
  await clickByText("button", "Render");
  await sleep(1200);

  // Verifikasi caption tampil: ada <text> di svg preview?
  const textInfo = await page.evaluate(() => {
    const svg = document.querySelector(".video-box svg") || document.querySelector("svg[preserveAspectRatio]");
    const texts = svg ? [...svg.querySelectorAll("text")] : [];
    return {
      count: texts.length,
      sample: texts.slice(0, 8).map((t) => {
        const r = t.getBoundingClientRect();
        return { s: t.textContent, x: r.left, y: r.top, w: r.width, h: r.height };
      }),
      overlay: document.querySelector(".frame-chip, .video-meta")?.textContent ?? null,
    };
  });
  step(`svg texts: ${textInfo.count} sample=${JSON.stringify(textInfo.sample.slice(0, 4))}`);

  // Buka tab Element
  await page.evaluate(() => {
    const btns = [...document.querySelectorAll(".rail-tab")];
    btns.find((x) => (x.getAttribute("aria-label") || "").toLowerCase() === "element")?.click();
  });
  await sleep(400);

  // Klik tepat di tengah bbox kata pertama
  let opened = false;
  if (textInfo.sample.length > 0) {
    const t0 = textInfo.sample.find((s) => s.w > 0) ?? textInfo.sample[0];
    const x = t0.x + t0.w / 2;
    const y = t0.y + t0.h / 2;
    step(`click word "${t0.s}" at ${x},${y} (w=${t0.w})`);
    await page.mouse.click(x, y);
    await sleep(800);
    opened = await page.evaluate(() => document.body.innerText.includes("ANIMATION"));
    step(`panel opened: ${opened}`);
    // Verifikasi bingkai seleksi kuning ada di DOM
    const selBox = await page.evaluate(() => {
      const el = document.querySelector(".word-sel");
      if (!el) return null;
      const cs = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      return { border: cs.borderTopColor, w: Math.round(r.width), h: Math.round(r.height), handles: el.querySelectorAll(".h").length };
    });
    step(`word-sel overlay: ${JSON.stringify(selBox)}`);
  }

  await page.screenshot({ path: "E:\\temp\\opencode\\element_word_open.png" });
  step("saved element_word_open.png");

  if (opened) {
    // Klik "Pop in" → cek CSS box berisi animation
    const pop = await page.evaluate(() => {
      const els = [...document.querySelectorAll("button, [class*=card], [role=button]")];
      const el = els.find((e) => e.textContent.trim() === "Pop in");
      if (el) el.click();
      return !!el;
    });
    step(`click Pop in: ${pop}`);
    await sleep(1000);
    const cssVal = await page.evaluate(() => {
      const ta = document.querySelector("textarea");
      return ta ? ta.value : null;
    });
    step(`css after pop: ${JSON.stringify(cssVal)}`);
    await page.screenshot({ path: "E:\\temp\\opencode\\element_word_pop.png" });
    step("saved element_word_pop.png");
  }
} catch (e) {
  err(String(e));
} finally {
  await browser.close();
}
process.exit(0);
