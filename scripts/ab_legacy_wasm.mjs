// A/B paritas: webm_wasm (svgr-wasm) vs webm_legacy (canvas browser).
// Decode WebCodecs + template sama → selisih = raster caption saja.
// Ekstrak wajib -fps_mode passthrough (cfr = artefak dup frame).
import { execSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
const env = { ...process.env, PATH: "E:/temp/opencode/ffmpeg-9.0.2-full_build-shared/bin;" + process.env.PATH };
const dir = "E:/temp/opencode/ab/";
rmSync(dir, { recursive: true, force: true });
mkdirSync(dir, { recursive: true });
const TIMES = [1, 3, 5, 7, 9, 11];
const psnrOf = (a, b) => {
  const out = execSync(`ffmpeg -v info -i "${a}" -i "${b}" -lavfi psnr -f null - 2>&1`, {
    env,
    encoding: "utf8",
  });
  const m = (out + "").match(/average:([\d.]+)/);
  return m ? parseFloat(m[1]) : NaN;
};
let sum = 0;
for (const t of TIMES) {
  const a = `${dir}wasm_${t}.png`;
  const b = `${dir}legacy_${t}.png`;
  execSync(`ffmpeg -y -v error -i E:/temp/opencode/webm_wasm.webm -ss ${t} -fps_mode passthrough -frames:v 1 "${a}"`, { env });
  execSync(`ffmpeg -y -v error -i E:/temp/opencode/webm_legacy.webm -ss ${t} -fps_mode passthrough -frames:v 1 "${b}"`, { env });
  const p = psnrOf(a, b);
  sum += p;
  console.log(`t=${t}s PSNR wasm-vs-legacy: ${p.toFixed(2)} dB`);
}
console.log(`RATA-RATA: ${(sum / TIMES.length).toFixed(2)} dB`);
console.log(sum / TIMES.length >= 35 ? "PARITAS OK" : "PARITAS RAGU - lihat " + dir);
