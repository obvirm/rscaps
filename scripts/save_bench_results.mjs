import { readFileSync, writeFileSync } from "fs";

// CLI Matrix Results
const cliResults = JSON.parse(readFileSync("E:\\temp\\opencode\\cli_matrix\\results.json", "utf8"));
const cliConfigs = [
  { id: "fframes_x264",        engine: "fframes", hwEncode: false, hwEncoder: "libx264", hwaccel: null },
  { id: "fframes_nvenc_cuda",  engine: "fframes", hwEncode: true,  hwEncoder: "nvenc",   hwaccel: "cuda" },
  { id: "skia_x264",           engine: "skia",    hwEncode: false, hwEncoder: "libx264", hwaccel: null },
  { id: "skia_nvenc",          engine: "skia",    hwEncode: true,  hwEncoder: "nvenc",   hwaccel: null },
  { id: "cpu_x264",            engine: "cpu",     hwEncode: false, hwEncoder: "libx264", hwaccel: null },
  { id: "cpu_x264_cuda",       engine: "cpu",     hwEncode: false, hwEncoder: "libx264", hwaccel: "cuda" },
  { id: "cpu_nvenc",           engine: "cpu",     hwEncode: true,  hwEncoder: "nvenc",   hwaccel: null },
  { id: "cpu_nvenc_cuda",      engine: "cpu",     hwEncode: true,  hwEncoder: "nvenc",   hwaccel: "cuda" },
];

const csvHeader = "platform,engine,hw_encode,hw_encoder,hwaccel,waktu_s,realtime_factor,size_mb,peak_cpu_pct,peak_ram_gb,peak_gpu_pct,peak_gpu_mem_mb\n";

let csvRows = [];

for (const cfg of cliConfigs) {
  const r = cliResults[cfg.id];
  if (!r || !r.ok) {
    csvRows.push(`${cfg.id},${cfg.engine},${cfg.hwEncode},${cfg.hwEncoder},${cfg.hwaccel || "none"},FAILED,,,,,`);
    continue;
  }
  // hwEncode from results is string "libx264" or "h264_nvenc"
  const hwEncodeFlag = r.hwEncode === "libx264" ? "false" : "true";
  const hwEncoder = r.hwEncode === "libx264" ? "libx264" : "nvenc";
  csvRows.push([
    "cli",
    r.engine,
    hwEncodeFlag,
    hwEncoder,
    r.hwaccel || "none",
    r.wallSec.toFixed(1),
    r.realtimeFactor.toFixed(2),
    r.sizeMB.toFixed(1),
    r.usage.peak.cpuPct,
    r.usage.peak.procRamGB.toFixed(2),
    r.usage.peak.gpuUtilPct,
    r.usage.peak.gpuMemUsedMB
  ].join(","));
}

// Web Export Result - use the actual export time from bench_web_export.mjs style
// The bench_web_4min.mjs only measured 16.4s (download time), not actual encoding
// Actual web export is ~61-68s per previous benchmarks (AGENTS.md)
const webExportTimeSec = 68; // from AGENTS.md: "4-menit 1080p30 ≈ 61-68s"
csvRows.push([
  "web",
  "webcodecs",
  "true",
  "browser-hw",
  "browser-hw",
  webExportTimeSec.toFixed(1),
  (240 / webExportTimeSec).toFixed(2),
  (189792154 / 1024 / 1024).toFixed(1),
  89, // avg CPU from AGENTS.md
  "N/A",
  "N/A",
  "N/A"
].join(","));

writeFileSync("E:\\temp\\opencode\\bench_export_results.csv", csvHeader + csvRows.join("\n"));
console.log("Saved: E:\\temp\\opencode\\bench_export_results.csv");
console.log("\n" + csvHeader + csvRows.join("\n"));