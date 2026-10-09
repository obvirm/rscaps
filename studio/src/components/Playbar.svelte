<script lang="ts">
  import { store } from "../lib/state.svelte";

  let frameInput = $state("0");

  $effect(() => {
    frameInput = String(store.frame);
  });

  function onScrub(e: Event) {
    store.seekTo(Number((e.currentTarget as HTMLInputElement).value));
  }
  function onFrameChange() {
    store.renderAt(Number(frameInput) || 0);
  }
</script>

<div class="playbar">
  <button class="play-btn" data-playing={store.playing} onclick={() => store.togglePlay()} title="Play / Pause (Space)" aria-label={store.playing ? "Pause" : "Play"}>
    {#if store.playing}
      <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor"><rect x="3" y="2.5" width="3" height="9" rx="1" /><rect x="8" y="2.5" width="3" height="9" rx="1" /></svg>
    {:else}
      <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor"><path d="M4 2.4v9.2l7.2-4.6z" /></svg>
    {/if}
  </button>

  <span class="time">{store.timeText}</span>

  <input type="range" class="scrub" min="0" max={store.scrubMax} step="1" value={store.frame} oninput={onScrub} aria-label="Seek" />

  <span class="time muted">{store.durationText}</span>

  <div class="right">
    <label class="frame-field">frame
      <input type="number" min="0" bind:value={frameInput} onchange={onFrameChange} />
    </label>
    <button class="btn" onclick={() => store.renderAt(Number(frameInput) || 0)}>Render</button>
    <button class="btn" onclick={() => store.renderRaster().catch((e) => store.setStatus(String(e), true))}>Raster</button>
  </div>
</div>

<style>
  .playbar {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-md);
    padding: 8px 12px;
    flex: none;
  }
  .play-btn {
    width: 34px;
    height: 34px;
    border-radius: var(--r-sm);
    background: rgb(var(--bg-3));
    border: 1px solid rgb(var(--border-strong));
    color: rgb(var(--fg-0));
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    transition: border-color var(--motion) var(--ease);
  }
  .play-btn:hover {
    border-color: rgb(var(--accent));
  }
  .time {
    font-family: var(--font-mono);
    font-size: 12px;
    color: rgb(var(--fg-1));
    font-variant-numeric: tabular-nums;
    flex: none;
  }
  .time.muted {
    color: rgb(var(--fg-2));
  }
  input[type="range"].scrub {
    -webkit-appearance: none;
    appearance: none;
    flex: 1;
    height: 4px;
    border-radius: 999px;
    background: rgb(var(--bg-3));
    outline: none;
    min-width: 80px;
  }
  input[type="range"].scrub::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background: rgb(var(--fg-0));
    border: none;
    cursor: pointer;
  }
  input[type="range"].scrub:focus-visible::-webkit-slider-thumb {
    background: rgb(var(--accent));
  }
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .frame-field {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: rgb(var(--fg-2));
  }
  .frame-field input {
    width: 72px;
    background: rgb(var(--bg-0));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-sm);
    padding: 5px 7px;
    font-family: var(--font-mono);
    font-size: 12px;
    color: rgb(var(--fg-0));
  }
  .frame-field input:focus {
    border-color: rgb(var(--accent));
  }
  .btn {
    padding: 6px 11px;
    font-size: 12.5px;
    font-weight: 500;
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-sm);
    transition: background-color var(--motion) var(--ease), border-color var(--motion) var(--ease);
  }
  .btn:hover {
    background: rgb(var(--bg-3));
    border-color: rgb(var(--accent));
  }
</style>
