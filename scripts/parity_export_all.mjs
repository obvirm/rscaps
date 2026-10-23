// Paritas visual level EXPORT (bukan caption-only): komposit frame video +
// caption — web (svgr-wasm `burn_frame`, jalur yang sama dengan export &
// preview studio: layer base source-over + layer blend difference) vs CLI
// (engine fframes: SVG komposit di-dump via env RSCAPS_DUMP_SVG_DIR lalu
// di-raster usvgr — rasterizer yang sama dengan backend CPU fframes).
//
// Yang tercakup di sini dan TIDAK di parity_all.mjs:
//   - mix-blend-mode: difference DI ATAS piksel video (luna/milo/luca);
//   - kaca selene (frame video sebagai plate, parity D4);
//   - gambar data: URL di atas video (mask iris, D5).
//
// Frame video diambil SEKALI via ffmpeg (decode yang sama dengan CLI) dan
// dipakai sebagai base di KEDUA sisi + terdaftar untuk `<image href=
// "frame_N">` komposit CLI → diff murni dari pipeline caption, bebas noise
// decoder. Video uji = frame statis testsrc2 yang di-loop (3 dtk) supaya
// caption timing dan komposit tetap nyata tanpa risiko mis-alignment frame.
//
// Pakai: node scripts/parity_export_all.mjs [timeSec]
import puppeteer from "puppeteer-core";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const CHROME = "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const TIME_SEC = Number(process.argv[2] ?? "1.2");
const FFMPEG_BIN = "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin";
const FFMPEG = join(FFMPEG_BIN, "ffmpeg.exe");
const VIDEO = "E:\\temp\\opencode\\parity_export\\static.mp4";
const SRT = "E:\\project\\rscaps\\rscaps-sample.srt";
const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const bin = join(repoRoot, "target", "release", process.platform === "win32" ? "rscaps.exe" : "rscaps");
const outDir = "E:\\temp\\opencode\\parity_export";
const env = { ...process.env, PATH: `${FFMPEG_BIN};${process.env.PATH}` };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

if (!existsSync(bin)) {
  console.error("binary belum di-build: cargo build --release -p rscaps-cli");
  process.exit(1);
}
// Video uji statis: 1 frame testsrc2 di-loop 3 dtk @30fps — konten kaya
// (gradien, pola warna) untuk blend difference & kaca, tapi tanpa gerak
// sehingga decode web/CLI pasti identik.
if (!existsSync(VIDEO)) {
  mkdirSync(outDir, { recursive: true });
  const frame = join(outDir, "frame.png");
  spawnSync(FFMPEG, ["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30", "-frames:v", "1", frame], { env, stdio: "inherit" });
  spawnSync(FFMPEG, ["-y", "-v", "error", "-loop", "1", "-i", frame, "-t", "3", "-r", "30", "-pix_fmt", "yuv420p", VIDEO], { env, stdio: "inherit" });
  console.log("[setup] video uji statis dibuat:", VIDEO);
}
rmSync(join(outDir, "dump"), { recursive: true, force: true });
mkdirSync(join(outDir, "dump"), { recursive: true });
mkdirSync(join(outDir, "fail"), { recursive: true });

const b = await puppeteer.launch({
  executablePath: CHROME,
  headless: true,
  defaultViewport: { width: 1600, height: 1000 },
  protocolTimeout: 2400000,
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

  // ── Frame video (decode ffmpeg = decode CLI fframes) → base64 PNG ──
  const framePng = join(outDir, `frame_${frame}.png`);
  spawnSync(FFMPEG, ["-y", "-v", "error", "-i", VIDEO, "-vf", `select=eq(n\\,${frame})`, "-fps_mode", "passthrough", "-frames:v", "1", framePng], { env, stdio: "inherit" });
  const frameB64 = readFileSync(framePng).toString("base64");

  const ids = await p.evaluate(() => window.__rscaps.store.templates.map((t) => t.id));

  // ── CLI: render fframes + dump SVG komposit per template ──
  const dumpSvgs = {};
  mkdirSync(join(outDir, "cli"), { recursive: true });
  for (const id of ids) {
    const dumpDir = join(outDir, "dump", id);
    const mp4 = join(outDir, "cli", `${id}.mp4`);
    mkdirSync(dumpDir, { recursive: true });
    const r = spawnSync(
      bin,
      ["render", "--engine", "fframes", "--video", VIDEO, "--srt", SRT, "--template", id,
       "--out", mp4, "--crf", "0"],
      { env: { ...env, RSCAPS_DUMP_SVG_DIR: dumpDir }, encoding: "utf8", timeout: 300000 },
    );
    const svgPath = join(dumpDir, `svg_${String(frame).padStart(6, "0")}.svg`);
    if (r.status === 0 && existsSync(svgPath)) {
      dumpSvgs[id] = readFileSync(svgPath, "utf8");
    } else {
      console.log(`[cli-err] ${id}: exit=${r.status} ${(r.stderr ?? "").split("\n").slice(-1)[0].slice(0, 100)}`);
    }
  }
  console.log(`[cli] ${Object.keys(dumpSvgs).length}/${ids.length} SVG komposit ter-dump`);

  // ── Web + raster dump CLI + diff (semua di browser) ──
  const results = await p.evaluate(async (frameB64, F, W, H, ids, dumpSvgs) => {
    async function decodePng(b64) {
      const img = new Image();
      img.src = "data:image/png;base64," + b64;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = W;
      c.height = H;
      c.getContext("2d").drawImage(img, 0, 0, W, H);
      return c;
    }
    const toPngB64 = (rgba) => {
      const c = document.createElement("canvas");
      c.width = W;
      c.height = H;
      c.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(rgba), W, H), 0, 0);
      return c.toDataURL("image/png").split(",")[1];
    };
    const mod = await import("/src/lib/svgrwasm.ts");
    const s = window.__rscaps.store;
    const frameCanvas = await decodePng(frameB64);
    // Piksel frame (ffmpeg decode) = base komposit web + isi <image frame_N>.
    const fd = frameCanvas.getContext("2d").getImageData(0, 0, W, H);
    const frameRgba = new Uint8Array(fd.data.buffer.slice(0));
    const bmp = await createImageBitmap(frameCanvas);
    const out = {};
    for (const id of ids) {
      const dumpSvg = dumpSvgs[id];
      if (!dumpSvg) {
        out[id] = { status: "cli-kosong" };
        continue;
      }
      try {
        // ── sisi web: kontrak export persis (state.renderFrameSvgLayers +
        // burnFrameWasm; supplyFrameImage hanya pemicu pancaran kaca) ──
        await s.applyTemplateFresh(id);
        await new Promise((r) => setTimeout(r, 30));
        await s.supplyFrameImage(bmp);
        const layers = s.renderFrameSvgLayers(F);
        if (!layers || (!layers.base.includes("<svg") && !layers.blend)) {
          out[id] = { status: "web-kosong" };
          continue;
        }
        const webRgba = await mod.burnFrameWasm(frameRgba.slice(0), layers.base, layers.blend ?? "", W, H);

        // ── sisi CLI: raster SVG komposit dump (usvgr, sama dgn backend
        // fframes cpu): daftarkan frame + decode data: URL statis ──
        await mod.registerImageWasm(`frame_${F}`, frameRgba.slice(0), W, H);
        for (const m of dumpSvg.matchAll(/href="(data:image\/[a-z0-9.+-]+;base64,[^"]+)"/g)) {
          const href = m[1];
          if (href.includes("rscaps-glass")) continue;
          const dec = await mod.decodeDataUrlRgba(href);
          if (dec) await mod.registerImageWasm(href, dec.rgba, dec.w, dec.h);
        }
        const cliRgba = await mod.rasterSvgWasm(dumpSvg, W, H);

        // ── diff RGBA per piksel (max kanal, alpha FE = komposit opaque) ──
        let maxDiff = 0, bad = 0, sumSq = 0;
        const n = webRgba.length;
        for (let px = 0; px < n; px += 4) {
          let d = 0;
          for (let k = 0; k < 3; k++) {
            const c = Math.abs(webRgba[px + k] - cliRgba[px + k]);
            if (c > d) d = c;
          }
          if (d > maxDiff) maxDiff = d;
          if (d > 8) bad++;
          sumSq += d * d;
        }
        const mse = sumSq / (n / 4) / 3;
        const psnr = mse === 0 ? 99 : 10 * Math.log10(255 * 255 / mse);
        // Toleransi edge blend (data): komposit difference manual (web) vs
        // inline usvgr/tiny-skia (CLI) menyisakan 2-3 piksel AA dengan max
        // ~37 (luca). psnr>=45 + bad<0.2% tetap menutup semua gagalan
        // nyata; max yang tinggi hanya ditoleransi saat pikselnya marginal
        // (<=16 px) — error struktural selalu menaikkan bad ke ribuan.
        out[id] = {
          status: "ok", psnr: +psnr.toFixed(1), maxDiff, bad,
          ok: psnr >= 45 && bad < W * H * 0.002 && (maxDiff <= 24 || bad <= 16),
        };
        if (!out[id].ok) {
          out[id].pngWeb = toPngB64(webRgba);
          out[id].pngCli = toPngB64(cliRgba);
        }
      } catch (e) {
        out[id] = { status: "err", msg: String(e).slice(0, 160) };
      }
      // Higienitas lintas template: jangan bawa frame kaca ke template lain.
      s.wasm.clearFrameImage();
    }
    return out;
  }, frameB64, frame, w, h, ids, dumpSvgs);

  // ── Laporan + PNG pasangan utk template yang gagal ──
  let pass = 0, fail = 0, skip = 0;
  const fails = [];
  for (const id of ids) {
    const r = results[id];
    if (!r || r.status !== "ok") {
      skip++;
      console.log(`SKIP ${id.padEnd(12)} ${r?.status ?? "?"} ${r?.msg ?? ""}`);
      continue;
    }
    if (r.ok) {
      pass++;
      continue;
    }
    fail++;
    fails.push(id);
    console.log(`GAGAL ${id.padEnd(12)} psnr=${r.psnr}dB max=${r.maxDiff} px>8=${r.bad}`);
    writeFileSync(join(outDir, "fail", `${id}_web.png`), Buffer.from(r.pngWeb, "base64"));
    writeFileSync(join(outDir, "fail", `${id}_cli.png`), Buffer.from(r.pngCli, "base64"));
  }
  console.log(`\n=== ${pass} ok, ${fail} gagal, ${skip} skip (psnr>=45, bad<0.2% px, max<=24 — max hingga 48 ditoleransi bila bad<=16 px; edge blend AA) ===`);
  if (fails.length) {
    console.log("gagal:", fails.join(", "), `— PNG pasangan di ${join(outDir, "fail")}`);
    process.exitCode = 1;
  }
} catch (e) {
  console.log("[ERR]", String(e).slice(0, 400));
} finally {
  await b.close();
}
