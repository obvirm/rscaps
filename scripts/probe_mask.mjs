import puppeteer from "puppeteer-core";
import fs from "node:fs";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const b64 = fs.readFileSync("E:\\project\\rscaps\\crates\\caption-core\\src\\templates\\iris\\marker-stroke.b64", "utf8").trim();
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, defaultViewport: { width: 600, height: 400 } });
const p = await b.newPage();
const r = await p.evaluate(async (b64) => {
  const uri = "data:image/png;base64," + b64;
  const img = new Image();
  let decode = "ok", size = "";
  try { await img.decode(); size = `${img.naturalWidth}x${img.naturalHeight}`; } catch (e) { decode = String(e); }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200" style="background:#333">
    <defs><mask id="m" mask-type="alpha" x="0" y="0" width="300" height="200" maskUnits="userSpaceOnUse">
    <image href="${uri}" x="0" y="0" width="300" height="200" preserveAspectRatio="none"/></mask></defs>
    <rect x="0" y="0" width="300" height="200" fill="#ffe066" mask="url(#m)"/></svg>`;
  document.body.innerHTML = svg;
  document.body.style.margin = "0";
  await new Promise((r) => setTimeout(r, 400));
  // sample pixel center
  const c = document.createElement("canvas"); c.width = 300; c.height = 200;
  return { decode, size, svgLen: svg.length };
}, b64);
console.log(JSON.stringify(r));
await p.screenshot({ path: "E:\\temp\\opencode\\mask_test.png" });
await b.close();
