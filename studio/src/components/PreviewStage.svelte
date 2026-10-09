<script lang="ts">
  import { onMount } from "svelte";
  import Playbar from "./Playbar.svelte";
  import { store } from "../lib/state.svelte";

  let regionEl: HTMLElement;
  let controlsEl: HTMLElement;
  let videoEl = $state<HTMLVideoElement | null>(null);
  let controlsH = $state(210);
  let aspect = $derived(store.aspect);
  let ratio = $derived(store.videoWidth / store.videoHeight);

  $effect(() => {
    store.attachVideoElement(videoEl);
    return () => store.attachVideoElement(null);
  });

  onMount(() => {
    const measure = () => {
      controlsH = controlsEl.offsetHeight + 36;
    };
    const ro = new ResizeObserver(measure);
    ro.observe(controlsEl);
    measure();
    return () => ro.disconnect();
  });
</script>

<section
  class="preview-region"
  bind:this={regionEl}
  style={`--controls-h: ${controlsH}px; --aspect: ${aspect}; --ratio: ${ratio}`}
>
  <div class="video-box">
    {#if store.videoUrl}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video class="bg-video" bind:this={videoEl} src={store.videoUrl} preload="auto" playsinline
      ></video>
    {/if}
    <div class="stage">
      {@html store.svg}
    </div>
    <span class="stage-chip"
      >frame {store.frame} · {(store.frame / store.fps).toFixed(2)}s · {store.videoWidth}×{store
        .videoHeight}</span
    >
  </div>

  <div class="controls" bind:this={controlsEl}>
    <Playbar />
  </div>
</section>

<style>
  .preview-region {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    container-type: size;
  }
  .video-box {
    position: relative;
    width: 100%;
    aspect-ratio: var(--aspect, 16 / 9);
    max-height: calc(100cqh - var(--controls-h, 210px));
    max-width: calc((100cqh - var(--controls-h, 210px)) * var(--ratio, 1.7778));
    background: rgb(var(--bg-0));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-lg);
    overflow: hidden;
    box-shadow: var(--shadow-md);
    flex: none;
  }
  .bg-video {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .stage {
    position: absolute;
    inset: 0;
    user-select: none;
    -webkit-user-select: none;
  }
  .stage :global(svg) {
    width: 100%;
    height: 100%;
    display: block;
    pointer-events: none;
  }
  .stage-chip {
    position: absolute;
    left: 10px;
    bottom: 8px;
    font-size: 11px;
    font-family: var(--font-mono);
    color: rgb(var(--fg-2));
    background: rgb(var(--bg-0) / 0.55);
    padding: 2px 7px;
    border-radius: 999px;
    pointer-events: none;
  }
  .controls {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: none;
  }
</style>
