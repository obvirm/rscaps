<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "../lib/state.svelte";
  import { computeOutputSize, type ExportFormat, type ExportQuality, type ExportResolution } from "../lib/export";
  import type { ExportProjectRef } from "../lib/types";
  import { invoke } from "../lib/tauri";

  let { project, onclose }: { project: ExportProjectRef; onclose: () => void } = $props();

  // Export mode: "web" (mediabunny WebCodecs) | "native" (CLI engines: cpu/skia/fframes + ffmpeg)
  let exportMode = $state<"web" | "native">("web");

  // Common options
  let resolution = $state<ExportResolution>("keep");
  let format = $state<ExportFormat>("mp4");
  let quality = $state<ExportQuality>("high");

  // Native-only options
  let nativeEngine = $state<"cpu" | "skia" | "fframes">("fframes");
  let nativeHwEncode = $state(true);
  let nativeHwEncoder = $state("h264_nvenc");
  let nativeHwaccel = $state<"auto" | "cuda" | "d3d11va" | "vaapi" | "qsv" | "">("auto");
  let nativeCrf = $state(18);
  let nativePreset = $state("medium");
  let nativeFfmpegDir = $state("");

  // Advanced toggles
  let advancedOpen = $state(false);
  let subtitlesOpen = $state(false);
  let nativeAdvancedOpen = $state(false);
  let hwEncoderAvailable = $state(false);

  const resolutions: { id: ExportResolution; label: string }[] = $derived([
    { id: "keep", label: `Keep original (${project.width} × ${project.height})` },
    { id: 1080, label: labelFor(1080) },
    { id: 720, label: labelFor(720) },
    { id: 480, label: labelFor(480) },
  ]);

  function labelFor(h: ExportResolution): string {
    const { width, height } = computeOutputSize(project.width, project.height, h);
    return `${width} × ${height}`;
  }

  const outputSize = $derived(computeOutputSize(project.width, project.height, resolution));

  const hwEncoders = [
    { value: "h264_nvenc", label: "NVIDIA NVENC (h264_nvenc)" },
    { value: "h264_qsv", label: "Intel QuickSync (h264_qsv)" },
    { value: "h264_vaapi", label: "VAAPI (h264_vaapi)" },
    { value: "h264_videotoolbox", label: "Apple VideoToolbox (h264_videotoolbox)" },
    { value: "h264_amf", label: "AMD AMF (h264_amf)" },
  ];

  const hwaccelOptions = [
    { value: "", label: "None (CPU decode)" },
    { value: "auto", label: "Auto" },
    { value: "cuda", label: "CUDA (NVIDIA)" },
    { value: "d3d11va", label: "D3D11VA (Windows)" },
    { value: "vaapi", label: "VAAPI (Linux)" },
    { value: "qsv", label: "QuickSync (Intel)" },
  ];

  async function checkHwEncoder() {
    if (!store.isTauri) return;
    try {
      const avail = await invoke<boolean>("native_check_hw_encoder", {
        ffmpegDir: nativeFfmpegDir || null,
        encoder: nativeHwEncoder,
      });
      hwEncoderAvailable = avail;
    } catch {
      hwEncoderAvailable = false;
    }
  }

  async function onExport() {
    if (store.exporting) return;

    if (exportMode === "web") {
      // Web export (mediabunny WebCodecs)
      await store.exportProjectBurnIn(project.id, { format, quality, resolution });
    } else {
      // Native export (CLI engines via Tauri)
      await store.exportProjectNative(project.id, {
        engine: nativeEngine,
        hwEncode: nativeHwEncode,
        hwEncoder: nativeHwEncoder,
        hwaccel: nativeHwaccel || undefined,
        format,
        quality,
        resolution,
        crf: nativeCrf,
        preset: nativePreset,
        ffmpegDir: nativeFfmpegDir || undefined,
      });
    }
    onclose();
  }

  function onExportSubtitles(fmt: "srt" | "vtt") {
    subtitlesOpen = false;
    void store.exportProjectSubtitle(project.id, fmt);
    onclose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !store.exporting) onclose();
  }

  onMount(() => {
    if (store.isTauri) {
      checkHwEncoder();
    }
  });

  // Re-check when hw_encoder changes
  $effect(() => {
    if (store.isTauri && exportMode === "native" && nativeHwEncode) {
      checkHwEncoder();
    }
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" onclick={() => !store.exporting && onclose()}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Export video"
    onclick={(e) => e.stopPropagation()}
  >
    <h2 class="title">Export video</h2>

    <!-- Export Mode Selector -->
    <div class="field">
      <label class="label">Export Mode</label>
      <div class="mode-tabs" role="tablist">
        <button
          role="tab"
          class="mode-tab {exportMode === 'web' ? 'active' : ''}"
          aria-selected={exportMode === 'web'}
          onclick={() => exportMode = 'web'}
          disabled={store.exporting}
        >
          <span class="tab-icon">🌐</span>
          <span>Web Export</span>
          <span class="tab-desc">mediabunny WebCodecs (browser)</span>
        </button>
        <button
          role="tab"
          class="mode-tab {exportMode === 'native' ? 'active' : ''}"
          aria-selected={exportMode === 'native'}
          onclick={() => exportMode = 'native'}
          disabled={store.exporting || !store.isTauri}
        >
          <span class="tab-icon">⚙️</span>
          <span>Native Export</span>
          <span class="tab-desc">CLI engines (cpu/skia/fframes + ffmpeg)</span>
        </button>
      </div>
      {#if exportMode === "native" && !store.isTauri}
        <p class="hint warning">Native export hanya tersedia di aplikasi desktop (Tauri).</p>
      {/if}
    </div>

    <!-- Common: Resolution -->
    <div class="field">
      <label class="label" for="ex-resolution">Resolution</label>
      <select id="ex-resolution" class="select" bind:value={resolution} disabled={store.exporting}>
        {#each resolutions as r (r.id)}
          <option value={r.id}>{r.label}</option>
        {/each}
      </select>
      <p class="hint">Output keeps your video's frame rate.</p>
    </div>

    <!-- Web Export Advanced Options -->
    {#if exportMode === "web"}
      <button class="advanced-toggle" type="button" onclick={() => (advancedOpen = !advancedOpen)} aria-expanded={advancedOpen} disabled={store.exporting}>
        <svg class:open={advancedOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 2.5 8 6l-4 3.5" /></svg>
        Advanced options
      </button>

      {#if advancedOpen}
        <div class="advanced">
          <div class="field">
            <label class="label" for="ex-format">Format</label>
            <select id="ex-format" class="select" bind:value={format} disabled={store.exporting}>
              <option value="mp4">MP4</option>
              <option value="webm">WebM</option>
            </select>
            <p class="hint">
              {format === "mp4"
                ? "Universal compatibility. Recommended for most uses."
                : "Smaller files. Best for web playback."}
            </p>
          </div>

          <div class="field">
            <label class="label" for="ex-quality">Quality</label>
            <select id="ex-quality" class="select" bind:value={quality} disabled={store.exporting}>
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
            <p class="hint">
              {quality === "high"
                ? "Recommended for most uploads."
                : quality === "medium"
                  ? "Balanced size and quality."
                  : "Smallest file size."}
            </p>
          </div>
        </div>
      {/if}
    {/if}

    <!-- Native Export Options -->
    {#if exportMode === "native" && store.isTauri}
      <div class="native-options">
        <div class="field">
          <label class="label" for="ex-native-engine">Render Engine</label>
          <select id="ex-native-engine" class="select" bind:value={nativeEngine} disabled={store.exporting}>
            <option value="fframes">FFrames (full GPU pipeline: decode→composite→encode)</option>
            <option value="cpu">CPU (usvgr + tiny-skia, deterministik, paritas web)</option>
            <option value="skia">Skia GPU (butuh build --features gpu-skia)</option>
          </select>
          <p class="hint">
            {nativeEngine === "fframes"
              ? "Terbaik: decode+composite+encode GPU, pipelined chunked, tanpa RAM cap."
              : nativeEngine === "cpu"
                ? "Deterministik, cocok CI/server, paritas pixel dengan web export."
                : "GPU Vulkan, butuh rebuild: cargo build -p rscaps-cli --features gpu-skia"}
          </p>
        </div>

        <div class="field">
          <label class="label">Hardware Encode</label>
          <div class="checkbox-row">
            <input type="checkbox" id="ex-hw-encode" bind:checked={nativeHwEncode} disabled={store.exporting} />
            <label for="ex-hw-encode">Enable hardware encoder (fallback ke libx264 kalau tidak tersedia)</label>
          </div>
        </div>

        {#if nativeHwEncode}
          <div class="field">
            <label class="label" for="ex-hw-encoder">Hardware Encoder</label>
            <select id="ex-hw-encoder" class="select" bind:value={nativeHwEncoder} disabled={store.exporting} onchange={checkHwEncoder}>
              {#each hwEncoders as enc}
                <option value={enc.value}>{enc.label} {hwEncoderAvailable && nativeHwEncoder === enc.value ? "✓" : ""}</option>
              {/each}
            </select>
            <p class="hint">
              {hwEncoderAvailable ? "Encoder tersedia di ffmpeg ini ✓" : "Encoder TIDAK tersedia → fallback libx264"}
            </p>
          </div>

          <div class="field">
            <label class="label" for="ex-hwaccel">Hardware Decode (hwaccel)</label>
            <select id="ex-hwaccel" class="select" bind:value={nativeHwaccel} disabled={store.exporting}>
              {#each hwaccelOptions as ha}
                <option value={ha.value}>{ha.label}</option>
              {/each}
            </select>
            <p class="hint">Decode video di GPU sebelum composite. "Auto" = ffmpeg pilih.</p>
          </div>
        {/if}

        <button class="advanced-toggle" type="button" onclick={() => (nativeAdvancedOpen = !nativeAdvancedOpen)} aria-expanded={nativeAdvancedOpen} disabled={store.exporting}>
          <svg class:open={nativeAdvancedOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 2.5 8 6l-4 3.5" /></svg>
          Advanced native options
        </button>

        {#if nativeAdvancedOpen}
          <div class="advanced">
            <div class="field">
              <label class="label" for="ex-crf">CRF (x264 / nvenc cq)</label>
              <input id="ex-crf" type="number" class="select" bind:value={nativeCrf} min={0} max={51} step={1} disabled={store.exporting} />
              <p class="hint">Lebih kecil = kualitas lebih bagus (default 18, range 17–23).</p>
            </div>

            <div class="field">
              <label class="label" for="ex-preset">Preset</label>
              <select id="ex-preset" class="select" bind:value={nativePreset} disabled={store.exporting}>
                <option value="ultrafast">ultrafast</option>
                <option value="superfast">superfast</option>
                <option value="veryfast">veryfast</option>
                <option value="faster">faster</option>
                <option value="fast">fast</option>
                <option value="medium">medium</option>
                <option value="slow">slow</option>
                <option value="slower">slower</option>
                <option value="veryslow">veryslow</option>
              </select>
              <p class="hint">x264 preset. Hardware encoder pakai preset sendiri (nvenc: preset, qsv: preset, dll).</p>
            </div>

            <div class="field">
              <label class="label" for="ex-ffmpeg-dir">FFmpeg Directory (opsional)</label>
              <input id="ex-ffmpeg-dir" type="text" class="select" bind:value={nativeFfmpegDir} placeholder="C:\\ffmpeg\\bin atau /usr/bin" disabled={store.exporting} />
              <p class="hint">Folder berisi ffmpeg/ffprobe. Bisa juga via env RSCAPS_FFMPEG_DIR atau PATH.</p>
            </div>
          </div>
        {/if}
      </div>
    {/if}

    <p class="summary">
      Output {outputSize.width} × {outputSize.height} · {project.fps} fps · {format.toUpperCase()}
      {#if exportMode === "native"} · Engine: {nativeEngine.toUpperCase()} {nativeHwEncode ? "· HW Encode" : ""}{/if}
    </p>

    {#if store.exporting}
      <p class="exporting">{store.status}</p>
      {#if exportMode === "native" && store.nativeExportProgress}
        <div class="progress-bar">
          <div class="progress-fill" style="width: {(store.nativeExportProgress.current / store.nativeExportProgress.total) * 100}%"></div>
        </div>
        <p class="progress-text">{store.nativeExportProgress.current} / {store.nativeExportProgress.total} frames ({store.nativeExportProgress.stage})</p>
      {/if}
    {/if}

    <div class="actions">
      <div class="subs">
        <button class="link" type="button" onclick={() => (subtitlesOpen = !subtitlesOpen)} disabled={store.exporting}>
          Export subtitles instead
        </button>
        {#if subtitlesOpen}
          <div class="subs-menu" role="menu">
            <button class="menu-item" role="menuitem" onclick={() => onExportSubtitles("srt")}>SRT</button>
            <button class="menu-item" role="menuitem" onclick={() => onExportSubtitles("vtt")}>VTT</button>
          </div>
        {/if}
      </div>
      <div class="actions-right">
        <button class="btn secondary" type="button" onclick={onclose} disabled={store.exporting}>Cancel</button>
        <button class="btn primary" type="button" onclick={onExport} disabled={store.exporting}>
          {store.exporting ? "Exporting…" : "Export"}
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgb(var(--bg-0) / 0.7);
    backdrop-filter: blur(4px);
  }
  .dialog {
    width: 100%;
    max-width: 520px;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-md);
    padding: 22px 22px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-height: 90vh;
    overflow-y: auto;
  }
  .title {
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.02em;
    color: rgb(var(--fg-0));
    margin: 0;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-size: 12px;
    font-weight: 500;
    color: rgb(var(--fg-1));
  }
  .select {
    width: 100%;
    background: rgb(var(--bg-0));
    color: rgb(var(--fg-0));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-sm);
    padding: 9px 11px;
    font-size: 13px;
    transition: border-color var(--motion) var(--ease);
  }
  .select:focus {
    border-color: rgb(var(--accent));
  }
  .select:disabled {
    opacity: 0.6;
  }
  .hint {
    font-size: 11.5px;
    line-height: 1.5;
    color: rgb(var(--fg-2));
  }
  .hint.warning {
    color: rgb(var(--warning));
  }
  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: rgb(var(--fg-1));
  }
  .checkbox-row input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: rgb(var(--accent));
  }

  /* Mode Tabs */
  .mode-tabs {
    display: flex;
    gap: 4px;
    background: rgb(var(--bg-0));
    border-radius: var(--r-sm);
    padding: 4px;
  }
  .mode-tab {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 10px 14px;
    border-radius: var(--r-xs);
    border: none;
    background: transparent;
    color: rgb(var(--fg-1));
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color var(--motion) var(--ease), color var(--motion) var(--ease);
    flex: 1;
    text-align: left;
  }
  .mode-tab:hover:not(:disabled) {
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-0));
  }
  .mode-tab.active {
    background: rgb(var(--accent));
    color: rgb(var(--fg-on-accent));
  }
  .mode-tab:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .tab-icon {
    font-size: 16px;
  }
  .tab-desc {
    font-size: 11px;
    font-weight: 400;
    opacity: 0.85;
  }

  /* Native Options Section */
  .native-options {
    border-top: 1px solid rgb(var(--border));
    padding-top: 8px;
    margin-top: 4px;
  }

  .advanced-toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    font-size: 12.5px;
    font-weight: 500;
    color: rgb(var(--fg-1));
    padding: 4px 0;
  }
  .advanced-toggle:hover:not(:disabled) {
    color: rgb(var(--fg-0));
  }
  .advanced-toggle:disabled {
    opacity: 0.6;
  }
  .advanced-toggle svg {
    transition: transform var(--motion) var(--ease);
  }
  .advanced-toggle svg.open {
    transform: rotate(90deg);
  }
  .advanced {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-left: 12px;
    border-left: 2px solid rgb(var(--border));
    margin-left: 4px;
  }

  .summary {
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: rgb(var(--fg-2));
    padding-top: 2px;
    border-top: 1px solid rgb(var(--border));
    margin: 0;
  }
  .exporting {
    font-size: 12px;
    color: rgb(var(--accent));
    margin: 0;
  }
  .progress-bar {
    width: 100%;
    height: 6px;
    background: rgb(var(--bg-0));
    border-radius: 3px;
    overflow: hidden;
    margin-top: 8px;
  }
  .progress-fill {
    height: 100%;
    background: rgb(var(--accent));
    transition: width 150ms linear;
  }
  .progress-text {
    font-size: 11.5px;
    color: rgb(var(--fg-2));
    margin: 4px 0 0;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 6px;
  }
  .subs {
    position: relative;
    display: inline-flex;
  }
  .subs-menu {
    position: absolute;
    left: 0;
    bottom: calc(100% + 4px);
    z-index: 5;
    min-width: 90px;
    padding: 4px;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-md);
    box-shadow: var(--shadow-md);
    display: flex;
    flex-direction: column;
  }
  .menu-item {
    padding: 7px 9px;
    font-size: 12.5px;
    color: rgb(var(--fg-0));
    border-radius: var(--r-sm);
    text-align: left;
    transition: background-color var(--motion) var(--ease);
  }
  .menu-item:hover {
    background: rgb(var(--bg-2));
  }
  .actions-right {
    display: flex;
    gap: 8px;
  }
  .link {
    font-size: 12.5px;
    color: rgb(var(--fg-1));
    text-decoration: underline;
    text-underline-offset: 2px;
    padding: 4px 0;
    transition: color var(--motion) var(--ease);
  }
  .link:hover:not(:disabled) {
    color: rgb(var(--fg-0));
  }
  .link:disabled {
    opacity: 0.5;
  }
  .btn {
    padding: 8px 14px;
    font-size: 13px;
    font-weight: 500;
    border-radius: var(--r-sm);
    border: 1px solid transparent;
    transition: background-color var(--motion) var(--ease), border-color var(--motion) var(--ease);
  }
  .btn.secondary {
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-2));
    border-color: rgb(var(--border-strong));
  }
  .btn.secondary:hover:not(:disabled) {
    background: rgb(var(--bg-3));
  }
  .btn.primary {
    color: rgb(var(--fg-on-accent));
    background: rgb(var(--accent));
    border-color: rgb(var(--accent));
  }
  .btn.primary:not(:disabled):hover {
    background: rgb(var(--accent-hi));
    border-color: rgb(var(--accent-hi));
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>