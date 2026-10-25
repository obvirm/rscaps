// Cari sampel dengan overlap person∩band-teks terbesar (band atas ~30%),
// lalu screenshot frame itu — bukti occlusion memotong teks.
import puppeteer from "puppeteer-core";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const b = await puppeteer.launch({
  executablePath: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  headless: true,
  defaultViewport: { width: 1600, height: 1000 },
  protocolTimeout: 600000,
});
try {
  const p = await b.newPage();
  await p.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(600);
  await p.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await p.waitForSelector('input[type="file"]');
  const vh = await p.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile("E:\\project\\movie2short\\data\\output\\jobs\\1791884396610-359dd5\\filter_test.mp4");
  await p.waitForSelector(".dialog");
  await sleep(400);
  await p.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await p.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile("E:\\project\\rscaps\\rscaps-sample.srt");
  await p.waitForSelector("header.toolbar", { timeout: 180000 });
  await sleep(1200);
  const r = await p.evaluate(async () => {
    const s = window.__rscaps.store;
    await s.applyTemplateFresh("pastor");
    for (let i = 0; i < 480 && s.personMaskState !== "done" && s.personMaskState !== "error"; i++) {
      await new Promise((r2) => setTimeout(r2, 500));
    }
    if (s.personMaskState !== "done") return { state: s.personMaskState };
    // Skor overlap person∩band-atas per sampel (band = y < 30% tinggi).
    const entries = s.personMaskClient.entries;
    const scored = entries.map((m) => {
      const bandH = Math.max(1, Math.floor(m.height * 0.3));
      let inBand = 0;
      for (let y = 0; y < bandH; y++) {
        const row = y * m.width;
        for (let x = 0; x < m.width; x++) if (m.alpha[row + x] > 128) inBand++;
      }
      return { t: m.t, frac: inBand / (bandH * m.width) };
    });
    scored.sort((a, b2) => b2.frac - a.frac);
    return { state: s.personMaskState, ranges: s.doc?.behind_ranges, top: scored.slice(0, 6) };
  });
  console.log("[scan]", JSON.stringify(r));
  const top = r.top ?? [];
  if (top.length > 0 && top[0].frac > 0.01) {
    // Screenshot frame terbaik + runner-up.
    for (let i = 0; i < Math.min(3, top.length); i++) {
      const t = top[i].t;
      await p.evaluate((tt) => {
        const s = window.__rscaps.store;
        const v = document.querySelector("video");
        if (v) v.currentTime = tt;
        s.renderAt(Math.round(tt * s.fps));
      }, t);
      await sleep(2500);
      const el = await p.$("canvas");
      if (el) await el.screenshot({ path: `E:\\temp\\opencode\\pastor_best${i}.png` });
      console.log(`[shot] pastor_best${i} t=${t.toFixed(2)} frac=${(top[i].frac * 100).toFixed(1)}%`);
    }
  } else {
    console.log("[info] tak ada sampel dengan person di band teks (frac tertinggi < 1%)");
  }
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 260));
} finally {
  await b.close();
}
