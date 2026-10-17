import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// fframes' host-side proc-macro (fframes-media-dir-macro) links against
// FFmpeg 9 shared libs, and rustc needs av*-61.dll on PATH when it loads the
// proc-macro dylib. Inject it here so `pnpm build:wasm` works from a clean
// shell.
const FFMPEG_BIN = "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin";
const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const sep = process.platform === "win32" ? ";" : ":";

// Each wasm crate is served by the Vite static-dirs plugin under its own
// alias: caption-bridge -> /pkg/, svgr-wasm -> /svgrpkg/.
const CRATES = [
  { dir: "crates/caption-bridge", ffmpeg: true },
  { dir: "crates/svgr-wasm", ffmpeg: false },
];

for (const crate of CRATES) {
  const env = { ...process.env };
  if (crate.ffmpeg) env.PATH = `${FFMPEG_BIN}${sep}${process.env.PATH}`;
  console.log(`\n>> wasm-pack build ${crate.dir}`);
  const result = spawnSync("wasm-pack", ["build", "--target", "web"], {
    cwd: repoRoot + crate.dir,
    env,
    stdio: "inherit",
    shell: process.platform === "win32",
  });
  if ((result.status ?? 1) !== 0) {
    console.error(`wasm-pack gagal untuk ${crate.dir}`);
    process.exit(result.status ?? 1);
  }
}
