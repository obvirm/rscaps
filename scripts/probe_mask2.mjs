import puppeteer from "puppeteer-core";
import fs from "node:fs";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const raw = fs.readFileSync("E:\\project\\rscaps\\crates\\caption-core\\src\\templates\\iris\\marker-stroke.b64", "utf8");
console.log("file chars:", raw.length, "head:", JSON.stringify(raw.slice(0, 20)));
const b64 = raw.trim();
console.log("trimmed:", b64.length, "mod4:", b64.length % 4);
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, defaultViewport: { width: 400, height: 300 } });
const p = await b.newPage();
const r = await p.evaluate((b64) => new Promise((resolve) => {
  const uri = "data:image/png;base64," + b64;
  const img = new Image();
  img.onload = () => resolve({ ok: true, w: img.naturalWidth, h: img.naturalHeight, uriLen: uri.length });
  img.onerror = (e) => resolve({ ok: false, uriLen: uri.length });
  img.src = uri;
  setTimeout(() => resolve({ ok: false, timeout: true, uriLen: uri.length }), 5000);
}), b64);
console.log(JSON.stringify(r));
await b.close();
