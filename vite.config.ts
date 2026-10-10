import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";

const root = fileURLToPath(new URL("./studio/", import.meta.url));
const pkgRoot = fileURLToPath(new URL("./crates/caption-bridge/pkg/", import.meta.url));
const whisperPkgRoot = fileURLToPath(new URL("./crates/whisper-spike/pkg/", import.meta.url));
const nmRoot = fileURLToPath(new URL("./node_modules/", import.meta.url));
const modelsRoot = fileURLToPath(new URL("./models/", import.meta.url));
const tplFontsRoot = fileURLToPath(new URL("./studio/tplfonts/", import.meta.url));
const fontRoot = "C:\\Windows\\Fonts";

const MIME: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".ttf": "font/ttf",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
  ".css": "text/css; charset=utf-8",
  ".png": "image/png",
  ".tsx": "text/javascript; charset=utf-8",
  ".ts": "text/javascript; charset=utf-8",
};

function resolveOutsideRoot(urlPath: string): string | null {
  const clean = normalize(urlPath).replace(/^([/\\])+/, "");
  if (urlPath.startsWith("/pkg/")) return join(pkgRoot, clean.replace(/^pkg[/\\]/, ""));
  if (urlPath.startsWith("/wpkg/")) return join(whisperPkgRoot, clean.replace(/^wpkg[/\\]/, ""));
  if (urlPath.startsWith("/nm/")) return join(nmRoot, clean.replace(/^nm[/\\]/, ""));
  if (urlPath.startsWith("/models/")) return join(modelsRoot, clean.replace(/^models[/\\]/, ""));
  if (urlPath.startsWith("/tplfonts/")) return join(tplFontsRoot, clean.replace(/^tplfonts[/\\]/, ""));
  if (urlPath.startsWith("/fonts/")) return join(fontRoot, clean.replace(/^fonts[/\\]/, ""));
  return null;
}

// Serve asset dirs that live outside the Vite root (wasm pkg, system fonts,
// template fonts, node_modules for the @fframes/editor importmap). Mirrors the
// old server.mjs resolvePath logic so the browser sees identical URLs.
function staticDirs(): Plugin {
  return {
    name: "rscaps-static-dirs",
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const url = req.url || "";
        const pathname = url.split("?")[0];
        if (!pathname.startsWith("/pkg/") && !pathname.startsWith("/nm/") && !pathname.startsWith("/fonts/") && !pathname.startsWith("/tplfonts/") && !pathname.startsWith("/models/") && !pathname.startsWith("/wpkg/")) {
          return next();
        }
        const filePath = resolveOutsideRoot(pathname);
        if (!filePath) return next();
        try {
          const info = await stat(filePath);
          if (info.isDirectory()) return next();
          const body = await readFile(filePath);
          res.writeHead(200, {
            "content-type": MIME[extname(filePath)] ?? "application/octet-stream",
            "cache-control": "no-store",
          });
          res.end(body);
        } catch {
          return next();
        }
      });
    },
  };
}

export default defineConfig({
  root,
  plugins: [svelte(), staticDirs()],
  server: {
    port: 5177,
    strictPort: true,
    // NOTE: COOP/COEP (crossOriginIsolated) enables SharedArrayBuffer for
    // onnxruntime-web's threaded wasm, but the threaded build hangs during
    // multi-worker session init under the Vite dev server, and the detached-
    // ArrayBuffer issue is fixed in Rust by pre-growing the heap (see
    // whisper-spike transcribe). Kept off for simplicity.
    // headers: {
    //   "Cross-Origin-Opener-Policy": "same-origin",
    //   "Cross-Origin-Embedder-Policy": "require-corp",
    // },
  },
  build: {
    outDir: fileURLToPath(new URL("./studio/dist/", import.meta.url)),
    emptyOutDir: true,
  },
});
