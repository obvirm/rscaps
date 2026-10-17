// Benchmark export tscaps (web): New project → upload video + SRT → export → ukur waktu.
import puppeteer from "puppeteer-core";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const URL = "http://localhost:5173/";
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
page.on("pageerror", (e) => err(`pageerror: ${e.message}`));
page.on("console", (m) => {
  if (m.type() === "error") err(`console: ${m.text().slice(0, 200)}`);
});
// Intercept download.
const downloads = [];
page.on("response", (r) => {
  const cd = r.headers()["content-disposition"] ?? "";
  if (cd.includes("attachment") || r.url().includes(".mp4") || r.url().includes(".webm")) {
    downloads.push({ url: r.url().slice(0, 100), cd: cd.slice(0, 80) });
  }
});
// Override showSaveFilePicker SEBELUM page scripts load → tscaps fallback
// ke OpfsExportWriter (native dialog tidak bisa diinteraksi headless).
await page.evaluateOnNewDocument(() => {
  Object.defineProperty(window, "showSaveFilePicker", {
    value: undefined,
    writable: false,
    configurable: true,
  });
});

const clickByText = async (text) => {
  const ok = await page.evaluate((text) => {
    const els = [...document.querySelectorAll("button, [role=button]")];
    const el = els.find((e) => e.textContent?.trim() === text);
    if (el) el.click();
    return !!el;
  }, text);
  if (!ok) throw new Error(`button "${text}" not found`);
};

try {
  step("goto tscaps");
  await page.goto(URL, { waitUntil: "networkidle2", timeout: 30000 });
  await page.waitForFunction(
    () => [...document.querySelectorAll("button")].some((b) => (b.textContent ?? "").includes("New project")),
    { timeout: 30000 },
  );
  await sleep(1000);

  step("New project");
  await clickByText("New project");
  await sleep(800);

  step("upload video");
  const videoInput = await page.$$('input[type="file"]');
  await videoInput[0].uploadFile(VIDEO);

  step("wait for start dialog");
  await page.waitForFunction(
    () => document.body.innerText.includes("Use subtitle file instead"),
    { timeout: 30000 },
  );
  await sleep(500);

  step("use subtitle file");
  await clickByText("Use subtitle file instead");
  await sleep(500);

  step("upload SRT");
  const srtInputs = await page.$$('input[type="file"]');
  for (const inp of srtInputs) {
    const accept = await inp.evaluate((e) => e.accept);
    if (!accept.includes("video")) {
      await inp.uploadFile(SRT);
      break;
    }
  }
  await sleep(500);

  step("click Start");
  await clickByText("Start");
  await page.waitForFunction(
    () => document.body.innerText.includes("Export") && document.body.innerText.includes("Templates"),
    { timeout: 30000 },
  );
  // Tunggu video fully loaded di editor (preview siap).
  await sleep(5000);
  step("editor ready (waited 5s)");

  // Debug: cek state editor sebelum export.
  const editorState = await page.evaluate(() => {
    // Cari store di window (kalau di-expose) atau cek DOM.
    const t = document.body.innerText;
    return {
      hasCaptions: /caption|transcript/i.test(t),
      hasVideo: /0:\d{2}\.\d{3}/.test(t), // timeline duration visible
      previewVisible: !!document.querySelector("canvas, video, [class*=preview]"),
      bodyLen: t.length,
    };
  });
  console.log("[editor-state]", JSON.stringify(editorState));

  step("open Export dialog");
  await clickByText("Export");
  await sleep(1000);

  // Verifikasi dialog terbuka.
  const dialogVisible = await page.evaluate(() =>
    document.body.innerText.includes("Export video"),
  );
  if (!dialogVisible) throw new Error("Export dialog not open");
  step("Export dialog open");

  // Klik tombol Export DI DALAM dialog pakai elementHandle.click().
  step("click Export in dialog (elementHandle)");
  const t0 = Date.now();
  const btnHandles = await page.$$("button");
  let clicked = false;
  for (const h of btnHandles) {
    const meta = await h.evaluate((el) => ({
      text: el.textContent?.trim() ?? "",
      disabled: !!el.disabled,
      visible: el.offsetWidth > 0 && el.offsetHeight > 0,
    }));
    // Ambil tombol Export yang terlihat, bukan yang di header (header sudah
    // tertutup backdrop sehingga offsetWidth=0).
    if (meta.text === "Export" && !meta.disabled && meta.visible) {
      await h.click();
      clicked = true;
      step("clicked Export via elementHandle.click");
      break;
    }
  }
  if (!clicked) throw new Error("no visible Export button found");

  // Poll: tunggu dialog tertutup ATAU status berubah.
  const deadline = t0 + 300_000;
  let done = false;
  let lastSnap = "";
  while (Date.now() < deadline) {
    const snap = await page.evaluate(() => {
      const t = document.body.innerText;
      const dialogOpen = t.includes("Export video");
      const progress = t.match(/(\d{1,3})%/)?.[1] ?? "";
      const status = /export|render|encod|complet|done|download/i.test(t)
        ? (t.match(/(?:Exporting|Rendering|Encoding|Finalizing|exported|Download)[^\n]{0,60}/i)?.[0] ?? "")
        : "";
      return { dialogOpen, progress, status };
    });
    const key = `${snap.dialogOpen}|${snap.progress}|${snap.status}`;
    if (key !== lastSnap) {
      console.log(`[snap] dialog=${snap.dialogOpen} progress=${snap.progress}% status="${snap.status}"`);
      lastSnap = key;
    }
    if (!snap.dialogOpen) {
      done = true;
      break;
    }
    await sleep(500);
  }

  const elapsed = ((Date.now() - t0) / 1000).toFixed(1);
  await page.screenshot({ path: "E:\\temp\\opencode\\tscaps_export_final.png" });
  console.log(`[downloads] ${JSON.stringify(downloads)}`);
  if (done && downloads.length > 0) {
    console.log(`[result] TSCAPS export selesai dalam ${elapsed}s (${downloads.length} download)`);
  } else if (done) {
    console.log(`[result] dialog tertutup dalam ${elapsed}s tapi TIDAK ada download - kemungkinan export gagal`);
  } else {
    err(`export tidak selesai dalam 300s (elapsed ${elapsed}s)`);
  }
} catch (e) {
  err(String(e));
  await page.screenshot({ path: "E:\\temp\\opencode\\tscaps_export_err.png" }).catch(() => {});
} finally {
  await browser.close();
}
process.exit(0);
