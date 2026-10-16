import puppeteer from "puppeteer-core";
const b = await puppeteer.launch({ executablePath: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", headless: true, defaultViewport: { width: 1600, height: 1000 } });
const p = await b.newPage();
p.on("pageerror", (e) => console.log("[pageerror]", e.message));
p.on("console", (m) => { if (m.type() === "error") console.log("[console]", m.text().slice(0, 200)); });
await p.goto("http://localhost:5173/", { waitUntil: "networkidle2", timeout: 60000 });
await new Promise((r) => setTimeout(r, 4000));
await p.screenshot({ path: "E:\\temp\\opencode\\cc_tscaps_home.png" });
const info = await p.evaluate(() => ({
  title: document.title,
  buttons: [...document.querySelectorAll("button, a[role=button], [role=button]")].map((x) => (x.textContent || "").trim().replace(/\s+/g, " ").slice(0, 50)).filter(Boolean),
  bodyHead: (document.body.innerText || "").slice(0, 500),
}));
console.log(JSON.stringify(info, null, 1));
await b.close();
