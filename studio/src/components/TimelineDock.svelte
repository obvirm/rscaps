<script lang="ts">
  import { store } from "../lib/state.svelte";
</script>

<div class="timeline-dock">
  <div class="ruler">
    {#each store.rulerTicks as tick (tick.label + tick.left)}
      <span class="tick" style={`left: ${tick.left}%`}>{tick.label}</span>
    {/each}
  </div>
  <div class="lane">
    <div class="playhead" style={`left: ${store.playheadPct}%`}></div>
    {#each store.cueChips as chip (chip.title + chip.left)}
      <button
        class="cue-chip"
        style={`left: ${chip.left}%; width: calc(${chip.width.toFixed(3)}% - 4px)`}
        title={chip.title}
        onclick={() => store.seekTo(chip.seekFrame)}
      >
        {chip.text}
      </button>
    {/each}
    {#each store.cueChips as chip, i (chip.title + chip.left)}
      {#if i === store.activeCueIndex}
        <div
          class="active-overlay"
          style={`left: ${chip.left}%; width: calc(${chip.width.toFixed(3)}% - 4px)`}
        ></div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .timeline-dock {
    height: 118px;
    flex: none;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-md);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .ruler {
    position: relative;
    height: 24px;
    border-bottom: 1px solid rgb(var(--border));
    flex: none;
    overflow: hidden;
  }
  .tick {
    position: absolute;
    top: 0;
    bottom: 0;
    border-left: 1px solid rgb(var(--border));
    font-size: 10px;
    font-family: var(--font-mono);
    color: rgb(var(--fg-2));
    padding: 5px 0 0 4px;
    user-select: none;
  }
  .lane {
    position: relative;
    flex: 1;
    margin: 10px;
  }
  .cue-chip {
    position: absolute;
    top: 8px;
    height: 42px;
    border-radius: var(--r-sm);
    border: 1px solid rgb(var(--border-strong));
    font-size: 11px;
    color: rgb(var(--fg-1));
    overflow: hidden;
    padding: 5px 7px;
    cursor: pointer;
    white-space: nowrap;
    text-overflow: ellipsis;
    text-align: left;
    direction: auto;
    transition: filter var(--motion) var(--ease);
  }
  .cue-chip:nth-child(3n + 2) {
    background: rgb(var(--scene-1) / 0.2);
  }
  .cue-chip:nth-child(3n + 3) {
    background: rgb(var(--scene-2) / 0.2);
  }
  .cue-chip:nth-child(3n + 4) {
    background: rgb(var(--scene-3) / 0.2);
  }
  .cue-chip:hover {
    filter: brightness(1.15);
  }
  .active-overlay {
    position: absolute;
    top: 8px;
    height: 42px;
    border-radius: var(--r-sm);
    border: 1px solid rgb(var(--accent));
    box-shadow: 0 0 0 1px rgb(var(--accent));
    pointer-events: none;
  }
  .playhead {
    position: absolute;
    top: -10px;
    bottom: -10px;
    width: 1px;
    background: rgb(var(--accent));
    pointer-events: none;
    z-index: 2;
  }
  .playhead::before {
    content: "";
    position: absolute;
    top: 0;
    left: -3.5px;
    width: 8px;
    height: 8px;
    background: rgb(var(--accent));
    border-radius: 2px;
  }
</style>
