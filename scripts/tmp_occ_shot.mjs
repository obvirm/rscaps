import puppeteer from "puppeteer-core";
const ver = await (await fetch("http://127.0.0.1:9333/json/version")).json();
const browser = await puppeteer.connect({ browserWSEndpoint: ver.webSocketDebuggerUrl });
const page = (await browser.pages())[0];
await page.bringToFront();
const r = await page.evaluate(async () => {
  const s = window.__rscaps.store;
  const entries = s.personMaskClient.entries ?? [];
  let best = -1, bestA = -1;
  for (let i = 0; i < entries.length; i++) {
    const m = entries[i];
    const y0 = Math.floor(m.height * 0.08), y1 = Math.floor(m.height * 0.3);
    let sum = 0;
    for (let y = y0; y < y1; y += 2) for (let x = 0; x < m.width; x += 2) sum += m.alpha[y * m.width + x];
    if (sum > bestA) { bestA = sum; best = i; }
  }
  return { t: entries[best]?.t ?? 0, n: entries.length };
});
const f = Math.round(r.t * 12);
await page.evaluate((ff) => window.__rscaps.store.seekTo(ff), f);
await new Promise((res) => setTimeout(res, 2500));
const box = await page.$(".video-box");
await box.screenshot({ path: "E:\\temp\\opencode\\gpu_occ_tauri.png" });
console.log(JSON.stringify({ ...r, f, shot: "E:\\temp\\opencode\\gpu_occ_tauri.png" }));
browser.disconnect();
