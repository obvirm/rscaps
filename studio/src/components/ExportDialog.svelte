<script lang="ts">
  import { store } from "../lib/state.svelte";
  import { computeOutputSize, type ExportFormat, type ExportQuality, type ExportResolution } from "../lib/export";
  import type { ExportProjectRef } from "../lib/types";

  let { project, onclose }: { project: ExportProjectRef; onclose: () => void } = $props();

  let resolution = $state<ExportResolution>("keep");
  let format = $state<ExportFormat>("mp4");
  let quality = $state<ExportQuality>("high");
  let advancedOpen = $state(false);
  let subtitlesOpen = $state(false);

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

  async function onExport() {
    if (store.exporting) return;
    await store.exportProjectBurnIn(project.id, { format, quality, resolution });
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

    <div class="field">
      <label class="label" for="ex-resolution">Resolution</label>
      <select id="ex-resolution" class="select" bind:value={resolution} disabled={store.exporting}>
        {#each resolutions as r (r.id)}
          <option value={r.id}>{r.label}</option>
        {/each}
      </select>
      <p class="hint">Output keeps your video's frame rate.</p>
    </div>

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

    <p class="summary">
      Output {outputSize.width} × {outputSize.height} · {project.fps} fps · {format.toUpperCase()}
    </p>

    {#if store.exporting}
      <p class="exporting">{store.status}</p>
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
    max-width: 420px;
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
