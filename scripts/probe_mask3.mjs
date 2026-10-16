import puppeteer from "puppeteer-core";
import fs from "node:fs";
const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const b64 = fs.readFileSync("E:\\project\\rscaps\\crates\\caption-core\\src\\templates\\iris\\marker-stroke.b64", "utf8").trim();
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, defaultViewport: { width: 600, height: 400 } });
const p = await b.newPage();
await p.evaluate((b64) => {
  const uri = "data:image/png;base64," + b64;
  document.body.style.margin = "0";
  document.body.style.background = "#222";
  document.body.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="600" height="400" viewBox="0 0 600 400" style="display:block">
    <defs><mask id="m" mask-type="alpha" x="50" y="150" width="500" height="100" maskUnits="userSpaceOnUse">
    <image href="${uri}" x="50" y="150" width="500" height="100" preserveAspectRatio="none"/></mask></defs>
    <rect x="50" y="150" width="500" height="100" fill="#ffe066" mask="url(#m)"/>
    <text x="120" y="215" font-size="40" fill="#000">Welcome to</text></svg>`;
}, b64);
await new Promise((r) => setTimeout(r, 800));
await p.screenshot({ path: "E:\\temp\\opencode\\mask_test2.png" });
console.log("ok");
await b.close();
