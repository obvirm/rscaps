// Tes fungsional Style: untuk semua template × semua style control —
// terapkan nilai uji khas, verifikasi (1) nilai tersimpan di params,
// (2) SVG render berubah dari baseline, (3) tidak error. Revert per control.
import puppeteer from "puppeteer-core";
import fs from "node:fs";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT = process.env.RSCAPS_STYLE_SRT || "E:\\project\\rscaps\\rscaps-sample.srt";
const OUT = "E:\\temp\\opencode\\style_test.json";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(...a);

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  defaultViewport: { width: 1600, height: 1000 },
});

function testValueFor(c) {
  switch (c.type) {
    case "color":
      return "#ff0000";
    case "float":
    case "integer": {
      const min = c.min ?? 0;
      const max = c.max ?? (c.type === "integer" ? 10 : 1);
      const v = min + (max - min) * 0.75;
      return c.type === "integer" ? Math.round(v) : Number(v.toFixed(3));
    }
    case "toggle":
      return c.default === true ? false : true;
    case "select": {
      const opts = c.options ?? [];
      const def = String(c.default ?? opts[0]?.value ?? "");
      const other = opts.find((o) => String(o.value ?? o.label) !== def);
      return String(other?.value ?? other?.label ?? "x");
    }
    case "text":
    case "font":
      return `${c.default ?? "x"}Z`;
    default:
      return null;
  }
}

try {
  const page = await browser.newPage();
  page.on("pageerror", (e) => console.log("[pageerror]", e.message));
  page.setDefaultTimeout(20000);
  await page.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(500);
  await page.evaluate(() =>
    [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click(),
  );
  await page.waitForSelector('input[type="file"]');
  const vh = await page.evaluateHandle(() =>
    [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")),
  );
  await vh.asElement().uploadFile(VIDEO);
  await page.waitForSelector(".dialog");
  await sleep(400);
  await page.evaluate(() =>
    [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click(),
  );
  const sh = await page.evaluateHandle(() =>
    document.querySelector('input[type="file"][accept*="srt"]'),
  );
  await sh.asElement().uploadFile(SRT);
  await page.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(1500);

  const templates = await page.evaluate(() =>
    window.__rscaps.store.templates.map((t) => ({
      id: t.id,
      controls: (t.styleControls ?? []).map((c) => ({
        id: c.id,
        label: c.label,
        type: c.type,
        default: c.default,
        min: c.min,
        max: c.max,
        group: c.group,
        options: c.options?.map((o) => ({ value: o.value ?? o.label, label: o.label })),
      })),
    })),
  );
  log(`template: ${templates.length}`);

  const results = [];
  const ONLY = process.argv[2] ? process.argv[2].split(",") : null;
  const list = ONLY ? templates.filter((t) => ONLY.includes(t.id)) : templates;
  for (const t of list) {
    // baseline per template
    await page.evaluate(async (id) => {
      await window.__rscaps.store.applyTemplateFresh(id);
      window.__rscaps.store.seekTo(Math.round(1.2 * window.__rscaps.store.fps));
    }, t.id);
    await sleep(450);
    const baseline = await page.evaluate(() => window.__rscaps.store.svg);

    for (const c of t.controls) {
      if ((c.group ?? "style") === "motion") {
        results.push({ t: t.id, c: c.id, status: "skip-motion" });
        continue;
      }
      const value = testValueFor(c);
      if (value === null) {
        results.push({ t: t.id, c: c.id, status: "skip-unknown-type" });
        continue;
      }
      let row = { t: t.id, c: c.id, type: c.type, value, status: "", err: "" };
      try {
        const applied = await page.evaluate(
          (id, cid, val) => {
            const s = window.__rscaps.store;
            const ok = s.setStyleValue(cid, val);
            const stored = s.params?.style_values?.[cid];
            return { ok, stored };
          },
          t.id,
          c.id,
          value,
        );
        await sleep(280);
        const svg = await page.evaluate(() => window.__rscaps.store.svg);
        const persisted = applied.stored === value;
        const changed = svg !== baseline;
        const visualExpected = !String(c.id).includes("emoji"); // emoji tak ada di SRT uji
        if (!persisted) row.status = "FAIL-not-persisted";
        else if (!changed && visualExpected) row.status = "FAIL-no-visual-change";
        else if (!changed && !visualExpected) row.status = "ok-persisted-only";
        else row.status = "ok";
      } catch (e) {
        row.status = "FAIL-exception";
        row.err = String(e.message || e);
      }
      results.push(row);
      // revert bersih
      await page.evaluate(async (id) => {
        await window.__rscaps.store.applyTemplateFresh(id);
        window.__rscaps.store.seekTo(Math.round(1.2 * window.__rscaps.store.fps));
      }, t.id);
      await sleep(250);
    }
    const fails = results.filter((r) => r.t === t.id && r.status.startsWith("FAIL"));
    log(`✓ ${t.id} (${t.controls.length} control${fails.length ? `, ${fails.length} GAGAL` : ""})`);
  }

  fs.writeFileSync(OUT, JSON.stringify(results, null, 2));
  const by = {};
  for (const r of results) by[r.status] = (by[r.status] ?? 0) + 1;
  log("RINGKASAN:", JSON.stringify(by));
  const fails = results.filter((r) => r.status.startsWith("FAIL"));
  log("GAGAL:", fails.length);
  for (const f of fails) log(` ✗ ${f.t} / ${f.c} (${f.type}) → ${f.status} ${f.err}`);
} catch (e) {
  console.log("[FATAL]", String(e));
} finally {
  await browser.close();
}
process.exit(0);
