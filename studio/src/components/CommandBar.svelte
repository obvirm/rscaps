<script lang="ts">
  import { store } from "../lib/state.svelte";
  import { toggleTheme } from "../lib/theme";

  let { onBack }: { onBack?: () => void } = $props();

  let fileInput: HTMLInputElement;

  async function onFileChange(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      await store.importSubtitleFile(file);
    } catch (err) {
      store.setStatus(String(err), true);
    } finally {
      input.value = "";
    }
  }
</script>

<header class="command-bar">
  <button class="icon-btn" onclick={onBack} title="Back to projects" aria-label="Back">
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M10 3 5 8l5 5" /></svg>
  </button>

  <span class="wordmark">rscaps</span>

  <input class="name-input" aria-label="Project name" bind:value={store.projectName} />

  <div class="spacer"></div>

  <span class="status" class:error={store.statusIsError}>{store.status}</span>

  <div class="group">
    <button class="btn" onclick={() => fileInput.click()} title="Import captions from a .srt or .vtt file">Import</button>
    <button class="btn" onclick={() => store.exportSubtitle("srt")} title="Download captions as SubRip (.srt)">.srt</button>
    <button class="btn" onclick={() => store.exportSubtitle("vtt")} title="Download captions as WebVTT (.vtt)">.vtt</button>
  </div>

  <button class="icon-btn" onclick={toggleTheme} title="Toggle theme" aria-label="Toggle theme">
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><circle cx="8" cy="8" r="5.5" /><path d="M8 2.5a5.5 5.5 0 0 1 0 11z" fill="currentColor" stroke="none" /></svg>
  </button>

  <button class="btn" disabled>Save</button>
  <button class="btn accent" onclick={() => store.exportPng()} title="Download current frame as PNG">Export</button>

  <input
    bind:this={fileInput}
    type="file"
    accept=".srt,.vtt,application/x-subrip,text/vtt"
    hidden
    onchange={onFileChange}
  />
</header>

<style>
  .command-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-bottom: 1px solid rgb(var(--border));
    flex-shrink: 0;
  }
  .wordmark {
    font-weight: 700;
    font-size: 15px;
    letter-spacing: -0.02em;
    color: rgb(var(--fg-0));
    padding-right: 4px;
    border-right: 1px solid rgb(var(--border));
    margin-right: 2px;
    user-select: none;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: var(--r-sm);
    color: rgb(var(--fg-1));
    transition: color var(--motion) var(--ease), background-color var(--motion) var(--ease);
  }
  .icon-btn:not(:disabled):hover {
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-0));
  }
  .name-input {
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    color: rgb(var(--fg-0));
    font-size: 14px;
    font-weight: 500;
    padding: 5px 8px;
    min-width: 120px;
    max-width: 300px;
    transition: background-color var(--motion) var(--ease), border-color var(--motion) var(--ease);
  }
  .name-input:hover {
    background: rgb(var(--bg-2));
    border-color: rgb(var(--border));
  }
  .name-input:focus {
    background: rgb(var(--bg-2));
    border-color: rgb(var(--accent));
  }
  .spacer {
    flex: 1;
  }
  .group {
    display: flex;
    gap: 4px;
  }
  .status {
    font-family: var(--font-mono);
    font-size: 11px;
    color: rgb(var(--fg-2));
    max-width: 360px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 3px 8px;
    border-radius: var(--r-sm);
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
  }
  .status.error {
    color: rgb(var(--error));
    border-color: rgb(var(--error) / 0.4);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 11px;
    font-size: 12.5px;
    font-weight: 500;
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
    transition: background-color var(--motion) var(--ease), border-color var(--motion) var(--ease);
  }
  .btn:not(:disabled):hover {
    background: rgb(var(--bg-2));
    border-color: rgb(var(--border-strong));
  }
  .btn.accent {
    background: rgb(var(--accent));
    border-color: rgb(var(--accent));
    color: rgb(var(--fg-on-accent));
  }
  .btn.accent:not(:disabled):hover {
    background: rgb(var(--accent-hi));
    border-color: rgb(var(--accent-hi));
  }
</style>
