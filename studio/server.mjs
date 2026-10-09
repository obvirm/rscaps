import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const studioRoot = fileURLToPath(new URL(".", import.meta.url));
const pkgRoot = fileURLToPath(new URL("../crates/caption-bridge/pkg/", import.meta.url));
const whisperPkgRoot = fileURLToPath(new URL("../crates/whisper-spike/pkg/", import.meta.url));
const nmRoot = fileURLToPath(new URL("../node_modules/", import.meta.url));
const modelsRoot = fileURLToPath(new URL("../models/", import.meta.url));
const tplFontsRoot = fileURLToPath(new URL("./tplfonts/", import.meta.url));
const fontRoot = "C:\\Windows\\Fonts";

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".ttf": "font/ttf",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
  ".css": "text/css; charset=utf-8",
};

function resolvePath(urlPath) {
  const clean = normalize(urlPath).replace(/^([/\\])+/, "");
  if (urlPath.startsWith("/pkg/")) return join(pkgRoot, clean.replace(/^pkg[/\\]/, ""));
  if (urlPath.startsWith("/wpkg/")) return join(whisperPkgRoot, clean.replace(/^wpkg[/\\]/, ""));
  if (urlPath.startsWith("/nm/")) return join(nmRoot, clean.replace(/^nm[/\\]/, ""));
  if (urlPath.startsWith("/models/")) return join(modelsRoot, clean.replace(/^models[/\\]/, ""));
  if (urlPath.startsWith("/tplfonts/")) return join(tplFontsRoot, clean.replace(/^tplfonts[/\\]/, ""));
  if (urlPath.startsWith("/fonts/")) return join(fontRoot, clean.replace(/^fonts[/\\]/, ""));
  if (clean === "" || clean === ".") return join(studioRoot, "index.html");
  return join(studioRoot, clean);
}

createServer(async (req, res) => {
  try {
    const urlPath = new URL(req.url, "http://localhost").pathname;
    const filePath = resolvePath(urlPath);
    const info = await stat(filePath);
    if (info.isDirectory()) throw new Error("directory");
    const body = await readFile(filePath);
    res.writeHead(200, {
      "content-type": MIME[extname(filePath)] ?? "application/octet-stream",
      "cache-control": "no-store",
      // Required for onnxruntime-web's threaded wasm (SharedArrayBuffer).
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
    });
    res.end(body);
  } catch {
    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  }
}).listen(5177, () => {
  console.log("studio at http://localhost:5177");
});
