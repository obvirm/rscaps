// Cross-check visual: tscaps asli (5173) vs rscaps (5177) — satu browser,
// dua tab, klik template satu per satu (search → klik kartu), screenshot
// area preview di keduanya ke folder compare/.
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT =
  process.env.RSCAPS_SRT || "E:\\project\\rscaps\\rscaps-sample.srt";
const OUT = "E:\\temp\\opencode\\compare";
const ONLY = process.argv[2] ? process.argv[2].split(",") : null;
// Waktu seek berlapis: satu pasang screenshot per template per waktu (untuk
// battery SRT multi-bahasa: setiap waktu = tengah satu cue).
const TIMES = (process.env.RSCAPS_TIMES || "1.2").split(",").map(Number);

fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(...a);

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  args: ["--hide-scrollbars"],
  defaultViewport: { width: 1600, height: 1000 },
});

async function clickByText(page, sel, text) {
  const ok = await page.evaluate(
    (sel, text) => {
      const el = [...document.querySelectorAll(sel)].find((e) =>
        e.textContent.trim().includes(text),
      );
      if (el) el.click();
      return !!el;
    },
    sel,
    text,
  );
  if (!ok) throw new Error(`tidak ketemu: ${sel} "${text}"`);
  return true;
}

async function upload(page, inputSel, file) {
  await page.waitForSelector(inputSel, { timeout: 15000 });
  const h = await page.evaluateHandle(
    (sel) => document.querySelector(sel),
    inputSel,
  );
  await h.asElement().uploadFile(file);
}

// ── Tab A: tscaps ──────────────────────────────────────────────────────────
async function setupTscaps(page) {
  log("[A] goto tscaps");
  await page.bringToFront();
  await page.goto("http://localhost:5173/", { waitUntil: "networkidle2", timeout: 60000 });
  await page.waitForFunction(
    () =>
      [...document.querySelectorAll("button")].some((b) =>
        b.textContent.includes("New project"),
      ),
    { timeout: 30000, polling: 200 },
  );
  await sleep(400);
  log("[A] New project");
  await clickByText(page, "button", "New project");
  await page.waitForSelector('input[type="file"][accept*="video"]', { timeout: 20000 });
  const inp = await page.evaluateHandle(
    () => document.querySelector('input[type="file"][accept*="video"]'),
  );
  await inp.asElement().uploadFile(VIDEO);
  log("[A] wait start dialog");
  // StartDialog: role=dialog muncul setelah probe video
  await page.waitForSelector('[role="dialog"]', { timeout: 30000 });
  await sleep(1200);
  log("[A] use subtitle file instead");
  await clickByText(page, '[role="dialog"] button, [role="dialog"] a, [role="dialog"] [role="button"]', "Use subtitle file");
  await upload(page, 'input[type="file"][accept*="srt"]', SRT);
  await page.waitForFunction(
    () =>
      (document.querySelector('[role="dialog"]')?.innerText || "").includes(
        "captions come from the file",
      ),
    { timeout: 20000, polling: 200 },
  );
  await sleep(600);
  log("[A] Start");
  await clickByText(page, '[role="dialog"] button', "Start");
  // tunggu editor + galeri
  try {
    await page.waitForSelector('[role="tab"][aria-label="Templates"]', {
      timeout: 90000,
      polling: 200,
    });
  } catch (e) {
    await page.screenshot({ path: `${OUT}/_debug_after_start.png` });
    const txt = await page
      .evaluate(() => document.body.innerText.slice(0, 600))
      .catch(() => "?");
    console.log("[A DEBUG body]", txt.replace(/\n+/g, " | "));
    throw e;
  }
  await sleep(2500);
  log("[A] editor siap");
}

const SEARCH_SEL = 'input[placeholder="Search templates"]';

async function setSearch(page, value) {
  const res = await page.evaluate(
    (sel, val) => {
      const input = document.querySelector(sel);
      if (!input) {
        return {
          ok: false,
          ph: [...document.querySelectorAll("input")].map(
            (i) => i.placeholder || i.type,
          ),
          body: document.body.innerText.slice(0, 220),
        };
      }
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      ).set;
      setter.call(input, val);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      return { ok: true };
    },
    SEARCH_SEL,
    value,
  );
  if (!res.ok) console.log("[setSearch miss]", JSON.stringify(res));
  return res.ok;
}

async function waitForTscapsReady(page, timeout = 300000) {
  // Tunggu tidak ada modal behind-actor dalam kondisi apa pun
  // ("Prepare video…" awal / "Preparing your video…" saat scan jalan).
  await page.waitForFunction(
    () => {
      const t = document.querySelector('[role="dialog"]')?.innerText || "";
      return (
        !t.includes("Prepare video for this template") &&
        !t.includes("Preparing your video")
      );
    },
    { timeout, polling: 500 },
  );
}

async function selectTscaps(page, id) {
  await waitForTscapsReady(page);
  if (!(await setSearch(page, id))) throw new Error("[A] search box tidak ada");
  await page.waitForFunction(
    (sel) => !!document.querySelector(sel),
    { timeout: 8000, polling: 150 },
    `button[aria-label="${id}"]`,
  );
  const card = await page.$(`button[aria-label="${id}"]`);
  await card.click({ timeout: 8000 });
  await sleep(800);
  const dialogText = await page.evaluate(
    () => document.querySelector('[role="dialog"]')?.innerText || "",
  );
  if (dialogText.includes("Prepare video for this template")) {
    console.log(`[A] behind-actor scan untuk ${id}…`);
    await page.evaluate(() => {
      const btn = [...document.querySelectorAll('[role="dialog"] button')].find((b) =>
        b.textContent.trim().includes("Continue"),
      );
      btn?.click();
    });
    await waitForTscapsReady(page);
    await sleep(600);
  }
}

async function shotTscaps(page, file) {
  const box = await page.$('div[style*="aspect-ratio"]');
  if (box) await box.screenshot({ path: file, timeout: 15000 }).catch(async () => {
    await page.screenshot({ path: file });
  });
  else await page.screenshot({ path: file });
}

// ── Tab B: rscaps ──────────────────────────────────────────────────────────
async function setupRscaps(page) {
  log("[B] goto rscaps");
  await page.goto("http://localhost:5177/", { waitUntil: "networkidle2", timeout: 60000 });
  await sleep(600);
  await clickByText(page, "button", "New project");
  await page.waitForSelector('input[type="file"]', { timeout: 15000 });
  const videoInp = await page.evaluateHandle(() =>
    [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")),
  );
  await videoInp.asElement().uploadFile(VIDEO);
  await page.waitForSelector(".dialog", { timeout: 15000 });
  await sleep(500);
  await clickByText(page, ".dialog button", "Use subtitle file instead");
  await upload(page, 'input[type="file"][accept*="srt"]', SRT);
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(2000);
  log("[B] editor siap");
}

async function selectRscaps(page, label) {
  if (!(await setSearch(page, label))) throw new Error("[B] search box tidak ada");
  await page.waitForFunction(
    (sel) => !!document.querySelector(sel),
    { timeout: 8000, polling: 150 },
    `button[aria-label="${label}"]`,
  );
  const card = await page.$(`button[aria-label="${label}"]`);
  await card.click({ timeout: 8000 });
  await sleep(800);
}

// Seek kedua app ke waktu yang SAMA (detik) supaya cue identik.
const SEEK_SEC = 1.2;

async function seekTscaps(page, sec = SEEK_SEC) {
  const info = await page.evaluate((sec) => {
    const slider = document.querySelector(
      "input.timeline-slider:not(.volume-slider)",
    );
    if (!slider) return { ok: false, reason: "no slider" };
    const setter = Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    ).set;
    setter.call(slider, String(sec));
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    slider.dispatchEvent(new Event("change", { bubbles: true }));
    // currentTime AKTUAL video (player bisa mendarat sedikit di bawah nilai
    // slider, mis. 1.2 -> 1.199) — jadi acuan frame yang sama di rscaps.
    const vid = document.querySelector("video");
    return { ok: true, actual: vid ? vid.currentTime : null, value: slider.value };
  }, sec);
  await sleep(600);
  if (!info.ok) console.log("[seekA miss]", JSON.stringify(info));
  else if (!seekLogged) {
    seekLogged = true;
    console.log("[seekA]", JSON.stringify(info));
  }
  return info;
}
let seekLogged = false;

async function seekRscaps(page, sec = SEEK_SEC) {
  await page.evaluate((sec) => {
    const s = window.__rscaps.store;
    s.seekTo(Math.round(sec * s.fps));
  }, sec);
  await sleep(400);
}

async function shotRscaps(page, file) {
  const box = await page.$(".video-box");
  if (box) await box.screenshot({ path: file });
  else await page.screenshot({ path: file });
}

// ── Main ───────────────────────────────────────────────────────────────────
try {
  const pageA = await browser.newPage(); // tscaps
  pageA.setDefaultTimeout(20000);
  const pageB = await browser.newPage(); // rscaps
  pageB.setDefaultTimeout(20000);
  pageA.on("pageerror", (e) => console.log("[A pageerror]", e.message));
  pageB.on("pageerror", (e) => console.log("[B pageerror]", e.message));

  await setupRscaps(pageB);
  await setupTscaps(pageA);

  // Daftar template dari engine rscaps (urutan katalog) + labelnya.
  const templates = await pageB.evaluate(() =>
    window.__rscaps.store.templates.map((t) => ({ id: t.id, label: t.label })),
  );
  log(`template engine: ${templates.length}`);

  // Kartu yang benar-benar ada di galeri tscaps.
  const tscapsIds = new Set(
    await pageA.evaluate(() =>
      [...document.querySelectorAll('button[aria-label]')]
        .map((b) => b.getAttribute("aria-label"))
        .filter((x) => x && /^[a-z0-9-]+$/.test(x)),
    ),
  );
  log(`kartu tscaps terdeteksi: ${tscapsIds.size}`);

  const results = [];
  const list = ONLY ? templates.filter((t) => ONLY.includes(t.id)) : templates;
  for (const t of list) {
    const row = { id: t.id, label: t.label, status: "", err: "" };
    try {
      // Halaman aktif bergantian supaya tidak kena throttle background page.
      await pageA.bringToFront();
      await selectTscaps(pageA, t.id);
      await pageB.bringToFront();
      await selectRscaps(pageB, t.label);
      for (const sec of TIMES) {
        const suffix = TIMES.length > 1 ? `@${sec}` : "";
        await pageA.bringToFront();
        const seek = await seekTscaps(pageA, sec);
        await shotTscaps(pageA, `${OUT}/${t.id}${suffix}__tscaps.png`);
        await pageB.bringToFront();
        // Pemetaan frame teruji: slider 1.2 -> tscaps melapor 1.199 tapi state
        // kontennya = t 1.2 -> rscaps frame 12 = round(1.2*fps). JANGAN pakai
        // floor(actual) — tscaps tampil 0.099s lebih awal (frame 11).
        await seekRscaps(pageB, sec);
        await shotRscaps(pageB, `${OUT}/${t.id}${suffix}__rscaps.png`);
      }
      row.status = "ok";
      log(`✓ ${t.id} (${TIMES.length}x)`);
    } catch (e) {
      row.status = "error";
      row.err = String(e.message || e);
      log(`✗ ${t.id}: ${row.err}`);
      // pulihkan search agar langkah berikutnya bersih
      for (const p of [pageA, pageB]) {
        try {
          await setSearch(p, "");
          await sleep(150);
        } catch {}
      }
    }
    results.push(row);
  }

  fs.writeFileSync(`${OUT}/_results.json`, JSON.stringify(results, null, 2));
  const ok = results.filter((r) => r.status === "ok").length;
  log(`SELESAI: ${ok} pasang ok, ${results.length - ok} lain`);
} catch (e) {
  console.log("[FATAL]", String(e));
} finally {
  await browser.close();
}
process.exit(0);
