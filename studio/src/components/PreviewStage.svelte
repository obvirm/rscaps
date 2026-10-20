<script lang="ts">
  import { onMount } from "svelte";
  import Playbar from "./Playbar.svelte";
  import { store } from "../lib/state.svelte";
  import { PreviewBurner } from "../lib/previewBurn";

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

  // ── Preview = export ──────────────────────────────────────────────────────
  // Saat video ada & engine WASM siap, caption TIDAK dilukis browser: frame
  // video + layer caption diraster/komposit lewat jalur burnWorker yang sama
  // dengan export (svgr-wasm + blend difference), lalu hasilnya digambar ke
  // kanvas ini. SVG tetap ter-mount sebagai layer geometri hit-test (opacity
  // 0 - getBoundingClientRect tetap bekerja untuk element inspector).
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  const burner = new PreviewBurner();
  const burnActive = $derived(
    store.videoUrl !== null && store.videoWidth > 0 && burner.available && store.wasmReady,
  );

  let lastKey = "";
  let inFlight = false;

  $effect(() => {
    if (!burnActive) return;
    let raf = 0;
    const pump = () => {
      raf = requestAnimationFrame(pump);
      const v = videoEl;
      const canvas = canvasEl;
      if (!v || !canvas || inFlight || v.readyState < 2) return;
      if (canvas.width !== store.videoWidth || canvas.height !== store.videoHeight) {
        canvas.width = store.videoWidth;
        canvas.height = store.videoHeight;
        lastKey = "";
      }
      const f = Math.round(v.currentTime * store.fps);
      const key = `${f}|${store.svg}`;
      if (key === lastKey) return;
      lastKey = key;
      inFlight = true;
      const idx = f;
      const w = store.videoWidth;
      const h = store.videoHeight;
      createImageBitmap(v)
        .then(async (bmp) => {
          // Layer kaca (selene): frame JPEG disetel SEBELUM render supaya
          // SVG caption membawa frame video ter-blur ter-embed.
          await store.supplyFrameImage(bmp);
          const layers = store.renderFrameSvgLayers(f);
          return burner.request({
            base: layers?.base ?? "",
            blend: layers?.blend ?? null,
            width: w,
            height: h,
            t: v.currentTime,
            bitmap: bmp,
          });
        })
        .then((res) => {
          inFlight = false;
          if (!res.frame) return;
          const ctx = canvas.getContext("2d");
          if (ctx) {
            ctx.globalCompositeOperation = "source-over";
            ctx.drawImage(res.frame, 0, 0);
          }
          res.frame.close();
        })
        .catch((err) => {
          inFlight = false;
          console.error("[previewBurn]", err);
        });
    };
    raf = requestAnimationFrame(pump);
    return () => cancelAnimationFrame(raf);
  });

  // Element inspector: klik pada preview — hit-test bounding box <text> —
  // pilih kata (null = klik kosong — lepas seleksi). SVG engine tak punya
  // index kata, jadi index di-alineasikan sebagai sub-kurun ke stream kata
  // (lihat komentar jujur di store.hitTestWord).
  function onStageClick(e: MouseEvent) {
    const box = e.currentTarget as HTMLElement;
    void store.hitTestWord(e.clientX, e.clientY, box).then((i) => store.selectWord(i));
  }

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
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="video-box" onclick={onStageClick}>
    {#if store.videoUrl}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video class="bg-video" bind:this={videoEl} src={store.videoUrl} preload="auto" playsinline
      ></video>
    {/if}
    {#if burnActive}
      <canvas class="burn-canvas" bind:this={canvasEl}></canvas>
    {/if}
    <div class="stage" class:ghost={burnActive}>
      {@html store.svg}
    </div>
    {#if store.wordBox && store.selectedWord != null}
      {@const b = store.wordBox}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="word-sel"
        style={`left:${b.x}px; top:${b.y}px; width:${b.w}px; height:${b.h}px`}
      >
        <span class="h tl"></span><span class="h tc"></span><span class="h tr"></span>
        <span class="h ml"></span><span class="h mr"></span>
        <span class="h bl"></span><span class="h bc"></span><span class="h br"></span>
      </div>
    {/if}
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
  /* Mode preview=export: SVG tersembunyi (layer geometri hit-test saja) -
  opacity 0, bukan display none, agar getBoundingClientRect tetap valid. */
  .stage.ghost {
    opacity: 0;
    pointer-events: none;
  }
  .burn-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
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
  /* Bingkai seleksi kata (mirip tscaps): outline kuning + 8 handle. */
  .word-sel {
    position: absolute;
    border: 1.5px solid #f5c518;
    pointer-events: none;
    z-index: 3;
    box-sizing: border-box;
  }
  .word-sel .h {
    position: absolute;
    width: 7px;
    height: 7px;
    background: #fff;
    border: 1.5px solid #f5c518;
    box-sizing: border-box;
  }
  .word-sel .tl { left: -4px; top: -4px; }
  .word-sel .tc { left: 50%; top: -4px; transform: translateX(-50%); }
  .word-sel .tr { right: -4px; top: -4px; }
  .word-sel .ml { left: -4px; top: 50%; transform: translateY(-50%); }
  .word-sel .mr { right: -4px; top: 50%; transform: translateY(-50%); }
  .word-sel .bl { left: -4px; bottom: -4px; }
  .word-sel .bc { left: 50%; bottom: -4px; transform: translateX(-50%); }
  .word-sel .br { right: -4px; bottom: -4px; }
  .controls {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: none;
  }
</style>
