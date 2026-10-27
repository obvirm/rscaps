import puppeteer from "puppeteer-core";
import { writeFileSync } from "node:fs";
const ver = await (await fetch("http://127.0.0.1:9333/json/version")).json();
const browser = await puppeteer.connect({ browserWSEndpoint: ver.webSocketDebuggerUrl });
const page = (await browser.pages())[0];
await page.bringToFront();
const svg = await page.evaluate(async () => {
  const s = window.__rscaps.store;
  s.seekTo(60);
  await new Promise((res) => setTimeout(res, 800));
  const v = document.querySelector("video");
  const bmp = await createImageBitmap(v);
  await s.supplyFrameImage(bmp);
  s.supplyPersonMask(v.currentTime);
  return s.renderFrameSvgLayers(Math.round(v.currentTime * s.fps))?.base ?? "";
});
writeFileSync("E:\\temp\\opencode\\occ_svg.txt", svg);
console.log("svgLen", svg.length);

// Uji regex burnWorker persis.
const OCCL_RE = /<clipPath id="pastor-occl"><rect x="([\d.-]+)" y="([\d.-]+)" width="([\d.-]+)" height="([\d.-]+)"\/><\/clipPath><mask id="pastor-person"[^>]*x="[^"]*"[^>]*>(?:[\s\S]*?)<\/mask><image[^>]*rscaps-glass[^>]*\/>/;
const m = svg.match(OCCL_RE);
console.log("OCCL_RE match:", !!m, m ? `rect=${m[1]},${m[2]},${m[3]},${m[4]}` : "");
if (!m) {
  const i = svg.indexOf('<clipPath id="pastor-occl"');
  console.log("chunk @", i, ":", JSON.stringify(svg.slice(i, i + 300)));
  const j = svg.indexOf("</mask>");
  console.log("has </mask>:", j >= 0, "glass idx:", svg.indexOf("rscaps-glass"));
}
browser.disconnect();
