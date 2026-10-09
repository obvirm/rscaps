import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// fframes' host-side proc-macro (fframes-media-dir-macro) links against
// FFmpeg 9 shared libs, and rustc needs av*-61.dll on PATH when it loads the
// proc-macro dylib. Inject it here so `pnpm build:wasm` works from a clean
// shell.
const FFMPEG_BIN = "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin";
const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const sep = process.platform === "win32" ? ";" : ":";

const result = spawnSync("wasm-pack", ["build", "--target", "web"], {
  cwd: repoRoot + "crates/caption-bridge",
  env: { ...process.env, PATH: `${FFMPEG_BIN}${sep}${process.env.PATH}` },
  stdio: "inherit",
  shell: process.platform === "win32",
});
process.exit(result.status ?? 1);
