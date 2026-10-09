<script lang="ts">
  import { store } from "../lib/state.svelte";
  import { router } from "../lib/router.svelte";
  import { TEMPLATES } from "../lib/templates";
  import { formatTimeShort, type Direction } from "../lib/types";
  import type { VideoProbe } from "../lib/video";

  let {
    video,
    probe,
    onclose,
  }: { video: File; probe: VideoProbe; onclose: () => void } = $props();

  const initialName = video.name.replace(/\.[^.]+$/, "") || "Untitled";
  let name = $state(initialName);
  let template = $state("luna");
  let direction = $state<Direction>("auto");
  let advancedOpen = $state(false);
  let creating = $state(false);
  let nameInput: HTMLInputElement;

  $effect(() => {
    nameInput?.focus();
    nameInput?.select();
  });

  async function onCreate() {
    if (creating) return;
    creating = true;
    try {
      const id = await store.createProject({
        name: name.trim() || "Untitled",
        template,
        direction,
        video,
        videoName: video.name,
        durationSec: probe.durationSec,
        fps: probe.fps,
        width: probe.width,
        height: probe.height,
      });
      onclose();
      router.openProject(id);
    } catch (err) {
      store.setStatus(String(err), true);
    } finally {
      creating = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" onclick={onclose}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="New project"
    onclick={(e) => e.stopPropagation()}
  >
    <h2 class="title">New project</h2>
    <p class="description">
      Duration, frame rate, and canvas are derived from your video. Everything runs in your browser.
    </p>

    <div class="video-summary">
      <span class="video-name" title={video.name}>{video.name}</span>
      <span class="video-meta"
        >{probe.width}×{probe.height} · {probe.fps} fps · {formatTimeShort(probe.durationSec)}</span
      >
    </div>

    <div class="field">
      <label class="label" for="np-name">Name</label>
      <input id="np-name" class="input" type="text" bind:this={nameInput} bind:value={name} />
    </div>

    <div class="field">
      <label class="label" for="np-template">Template</label>
      <select id="np-template" class="select" bind:value={template}>
        {#each TEMPLATES as [id, label] (id)}
          <option value={id}>{label}</option>
        {/each}
      </select>
      <p class="hint">Native templates render 1:1 through fframes — same engine in preview and export.</p>
    </div>

    <button class="advanced-toggle" type="button" onclick={() => (advancedOpen = !advancedOpen)} aria-expanded={advancedOpen}>
      <svg class:open={advancedOpen} width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 2.5 8 6l-4 3.5" /></svg>
      Advanced
    </button>

    {#if advancedOpen}
      <div class="advanced">
        <div class="field">
          <label class="label" for="np-direction">Direction</label>
          <select id="np-direction" class="select" bind:value={direction}>
            <option value="auto">Auto</option>
            <option value="ltr">Left to right</option>
            <option value="rtl">Right to left</option>
          </select>
          <p class="hint">Auto detects per-line script direction. Override for mixed or bidi text.</p>
        </div>
      </div>
    {/if}

    <div class="actions">
      <button class="btn secondary" type="button" onclick={onclose}>Cancel</button>
      <button class="btn primary" type="button" onclick={onCreate} disabled={creating}>
        {creating ? "Creating…" : "Create"}
      </button>
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
    max-width: 440px;
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
  }
  .description {
    font-size: 13px;
    line-height: 1.5;
    color: rgb(var(--fg-1));
    margin-top: -6px;
  }
  .video-summary {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 10px 12px;
    background: rgb(var(--bg-0));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
  }
  .video-name {
    font-size: 13px;
    font-weight: 500;
    color: rgb(var(--fg-0));
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .video-meta {
    font-size: 11.5px;
    font-family: var(--font-mono);
    color: rgb(var(--fg-2));
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
  .input,
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
  .input:focus,
  .select:focus {
    border-color: rgb(var(--accent));
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
  .advanced-toggle:hover {
    color: rgb(var(--fg-0));
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
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 6px;
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
  .btn.secondary:hover {
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
</style>
