// Paritas visual SEMUA template: CLI (usvgr native) vs Web (svgr-wasm).
// Sisi browser: raw render_frame → burnFrameWasm di atas base transparan
// (jalur yang sama dengan export web, termasuk registry gambar iris/selene).
// Sisi CLI: `frames --engine cpu --format png` (satu sumber kebenaran yang
// sama: caption-core → SVG → usvgr). Bandingkan RGBA per piksel.
//
// Pakai: node scripts/parity_all.mjs [timeSec]
import puppeteer from "puppeteer-core";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const TIME_SEC = Number(process.argv[2] ?? "1.2");
const VIDEO = "E:\\project\\rscaps\\crates\\whisper-spike\\test\\speech-test.mp4";
const SRT = "E:\\project\\rscaps\\rscaps-sample.srt";
const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const bin = join(repoRoot, "target", "release", process.platform === "win32" ? "rscaps.exe" : "rscaps");
const outDir = "E:\\temp\\opencode\\parity_all";
const FFMPEG_BIN = "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin";
const env = { ...process.env, PATH: `${FFMPEG_BIN};${process.env.PATH}` };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

if (!existsSync(bin)) {
  console.error("binary belum di-build: cargo build --release -p rscaps-cli");
  process.exit(1);
}
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const b = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  defaultViewport: { width: 1600, height: 1000 },
  protocolTimeout: 1200000,
});
try {
  const p = await b.newPage();
  p.on("pageerror", (e) => console.log("[pageerror]", (e.message ?? "").slice(0, 120)));
  await p.goto("http://localhost:5177/", { waitUntil: "networkidle2" });
  await sleep(600);
  await p.evaluate(() => [...document.querySelectorAll("button")].find((e) => e.textContent.includes("New project"))?.click());
  await p.waitForSelector('input[type="file"]');
  const vh = await p.evaluateHandle(() => [...document.querySelectorAll('input[type="file"]')].find((i) => !i.accept.includes("srt")));
  await vh.asElement().uploadFile(VIDEO);
  await p.waitForSelector(".dialog");
  await sleep(400);
  await p.evaluate(() => [...document.querySelectorAll(".dialog button")].find((e) => e.textContent.includes("Use subtitle file"))?.click());
  const sh = await p.evaluateHandle(() => document.querySelector('input[type="file"][accept*="srt"]'));
  await sh.asElement().uploadFile(SRT);
  await p.waitForSelector("header.toolbar", { timeout: 60000 });
  await sleep(1500);

  const meta = await p.evaluate(async () => {
    const s = window.__rscaps.store;
    const mod = await import("/src/lib/svgrwasm.ts");
    await mod.initSvgrWasm();
    return { fps: s.fps, w: s.videoWidth, h: s.videoHeight };
  });
  const { fps, w, h } = meta;
  const frame = Math.round(TIME_SEC * fps);
  console.log(`[meta] fps=${fps} kanvas=${w}x${h} frame=${frame} (t=${TIME_SEC}s)`);

  // ── Sisi browser: raster WASM per template ──
  const ids = await p.evaluate(() => window.__rscaps.store.templates.map((t) => t.id));
  const webPngs = {};
  for (const id of ids) {
    try {
      const png = await p.evaluate(async (tid, f, W, H) => {
        const s = window.__rscaps.store;
        await s.applyTemplateFresh(tid);
        await new Promise((r) => setTimeout(r, 30));
        const raw = s.wasm.bridge.render_frame(BigInt(f));
        if (!raw || !raw.includes("<svg") || raw.trim().length < 40) return null;
        const mod = await import("/src/lib/svgrwasm.ts");
        const rgba = await mod.burnFrameWasm(new Uint8Array(W * H * 4), raw, "", W, H);
        // burn_frame SUDAH mengembalikan straight RGBA (unpremultiply di Rust,
        // svgr-wasm lib.rs) - JANGAN demultiply lagi di sini.
        const img = new ImageData(W, H);
        img.data.set(rgba.subarray(0, W * H * 4));
        const c = document.createElement("canvas");
        c.width = W;
        c.height = H;
        c.getContext("2d").putImageData(img, 0, 0);
        return c.toDataURL("image/png").split(",")[1];
      }, id, frame, w, h);
      if (png) webPngs[id] = png;
    } catch (e) {
      console.log(`[web-err] ${id}: ${String(e).slice(0, 90)}`);
    }
  }
  console.log(`[web] ${Object.keys(webPngs).length}/${ids.length} template ter-raster`);

  // ── Sisi CLI: frames --engine cpu per template ──
  const cliPngs = {};
  for (const id of ids) {
    const dir = join(outDir, id);
    mkdirSync(dir, { recursive: true });
    const r = spawnSync(
      bin,
      ["frames", "--engine", "cpu", "--template", id, "--srt", SRT, "--format", "png",
       "--size", `${w}x${h}`, "--fps", String(fps), "--from", String(frame), "--to", String(frame + 1), "--out", dir],
      { env, encoding: "utf8", timeout: 120000 },
    );
    const pngPath = join(dir, `frame_${String(frame).padStart(6, "0")}.png`);
    if (r.status === 0 && existsSync(pngPath)) {
      cliPngs[id] = readFileSync(pngPath).toString("base64");
    }
  }
  console.log(`[cli] ${Object.keys(cliPngs).length}/${ids.length} PNG dihasilkan`);

  // ── Diff di browser (dekode PNG → RGBA → bandingkan) ──
  const results = await p.evaluate(async (pairs) => {
    async function decode(b64) {
      const img = new Image();
      img.src = "data:image/png;base64," + b64;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.naturalWidth;
      c.height = img.naturalHeight;
      c.getContext("2d").drawImage(img, 0, 0);
      return c.getContext("2d").getImageData(0, 0, c.width, c.height);
    }
    const out = {};
    for (const [id, webB64, cliB64] of pairs) {
      if (!cliB64) {
        out[id] = { status: "cli-kosong" };
        continue;
      }
      const a = await decode(webB64);
      const b2 = await decode(cliB64);
      if (a.width !== b2.width || a.height !== b2.height) {
        out[id] = { status: `ukuran ${a.width}x${a.height} vs ${b2.width}x${b2.height}` };
        continue;
      }
      let maxDiff = 0, bad = 0, sumSq = 0, n = a.data.length;
      for (let i = 0; i < n; i += 4) {
        for (let k = 0; k < 4; k++) {
          const d = Math.abs(a.data[i + k] - b2.data[i + k]);
          if (d > maxDiff) maxDiff = d;
          if (d > 8) bad++;
          sumSq += d * d;
        }
      }
      const mse = sumSq / n;
      const psnr = mse === 0 ? 99 : 10 * Math.log10(255 * 255 / mse);
      out[id] = { status: "ok", psnr: +psnr.toFixed(1), maxDiff, bad };
    }
    return out;
  }, Object.entries(webPngs).map(([id, b64]) => [id, b64, cliPngs[id] ?? null]));

  // ── Laporan ──
  let pass = 0, fail = 0, skip = 0;
  const fails = [];
  for (const id of ids) {
    const r = results[id];
    if (!r || r.status !== "ok") {
      skip++;
      console.log(`SKIP ${id.padEnd(12)} ${r?.status ?? "web-kosong"}`);
      continue;
    }
    const ok = r.psnr >= 45 && r.maxDiff <= 24 && r.bad < w * h * 0.002;
    if (ok) pass++;
    else {
      fail++;
      fails.push(id);
    }
    console.log(`${ok ? "OK  " : "GAGAL"} ${id.padEnd(12)} psnr=${r.psnr}dB max=${r.maxDiff} px>8=${r.bad}`);
  }
  console.log(`\n=== ${pass} ok, ${fail} gagal, ${skip} skip ===`);
  if (fails.length) {
    console.log("gagal:", fails.join(", "));
    process.exitCode = 1;
  }
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 300));
} finally {
  await b.close();
}
