<script lang="ts">
  // Timeline (C6): strip di bawah preview — ruler waktu + lane cue.
  // Interaksi: klik/scrub untuk seek, drag badan blok = pindah cue,
  // drag tepi = trim, snap ke tepi cue tetangga & 0. Commit via
  // store.editCue (satu langkah undo per drag).
  import { store } from "../lib/state.svelte";

  let trackEl: HTMLElement;
  let collapsed = $state(false);
  let pxPerSec = $state(48);
  const ZOOM_STEPS = [8, 16, 32, 48, 96, 192, 384];

  const durationSec = $derived(Math.max(store.durationSec, 1));
  const totalPx = $derived(durationSec * pxPerSec);

  type Block = { i: number; start: number; end: number; label: string };
  const blocks = $derived.by<Block[]>(() => {
    return store.cues.map((c, i) => ({
      i,
      start: c.start_ms,
      end: c.end_ms,
      label: c.text.replace(/\s+/g, " ").trim() || "(kosong)",
    }));
  });

  type Drag =
    | { kind: "scrub" }
    | {
        kind: "move" | "trim-l" | "trim-r";
        index: number;
        originMs: number; // start (move/trim-l) atau end (trim-r)
        startX: number;
        snap: number[];
      };
  let drag: Drag | null = null;
  // Override visual saat drag (ms): index → {start, end}.
  let override = $state<{ index: number; start: number; end: number } | null>(null);

  const px = (ms: number) => (ms / 1000) * pxPerSec;
  const msAt = (clientX: number) => {
    const rect = trackEl.getBoundingClientRect();
    return Math.max(0, ((clientX - rect.left + trackEl.scrollLeft) / pxPerSec) * 1000);
  };

  // ── Ruler ticks (mirip tscaps: baseline hairline + mark + label mono) ────
  const ruler = $derived.by(() => {
    const targets = [0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300];
    const stepSec = targets.find((t) => t * pxPerSec >= 64) ?? 600;
    const marks: { ms: number; label: string }[] = [];
    for (let s = 0; s <= durationSec + 0.001; s += stepSec) {
      const m = Math.round(s * 1000);
      const label = s < 60 ? `${s}s` : `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, "0")}`;
      marks.push({ ms: m, label });
    }
    return marks;
  });

  function onTrackPointerDown(e: PointerEvent) {
    const target = e.target as HTMLElement;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    if (target.dataset.edge === "l" || target.dataset.edge === "r") {
      const index = Number(target.dataset.index);
      const cue = store.cues[index];
      if (!cue) return;
      drag = {
        kind: target.dataset.edge === "l" ? "trim-l" : "trim-r",
        index,
        originMs: target.dataset.edge === "l" ? cue.start_ms : cue.end_ms,
        startX: e.clientX,
        snap: snapTargets(index),
      };
    } else if (target.dataset.block) {
      const index = Number(target.dataset.block);
      const cue = store.cues[index];
      if (!cue) return;
      drag = { kind: "move", index, originMs: cue.start_ms, startX: e.clientX, snap: snapTargets(index) };
    } else {
      drag = { kind: "scrub" };
      scrubTo(e.clientX);
    }
  }

  function snapTargets(draggedIndex: number): number[] {
    const t = [0];
    for (const c of store.cues) {
      if (c.start_ms !== undefined) t.push(c.start_ms, c.end_ms);
    }
    void draggedIndex;
    return [...new Set(t)];
  }

  const SNAP_PX = 7;
  function applySnap(rawMs: number, snap: number[]): number {
    const tol = (SNAP_PX / pxPerSec) * 1000;
    for (const t of snap) {
      if (Math.abs(rawMs - t) <= tol) return t;
    }
    return rawMs;
  }

  function scrubTo(clientX: number) {
    store.stopPlay();
    const ms = msAt(clientX);
    store.seekTo(Math.min(Math.round((ms / 1000) * store.fps), store.scrubMax));
  }

  function onTrackPointerMove(e: PointerEvent) {
    if (!drag) return;
    if (drag.kind === "scrub") {
      scrubTo(e.clientX);
      return;
    }
    const cue = store.cues[drag.index];
    if (!cue) return;
    const deltaMs = ((e.clientX - drag.startX) / pxPerSec) * 1000;
    if (drag.kind === "move") {
      let start = Math.max(0, drag.originMs + deltaMs);
      start = applySnap(start, drag.snap);
      const dur = cue.end_ms - cue.start_ms;
      override = { index: drag.index, start, end: start + dur };
    } else if (drag.kind === "trim-l") {
      let start = Math.min(applySnap(drag.originMs + deltaMs, drag.snap), cue.end_ms - 100);
      override = { index: drag.index, start: Math.max(0, start), end: cue.end_ms };
    } else {
      let end = Math.max(applySnap(drag.originMs + deltaMs, drag.snap), cue.start_ms + 100);
      override = { index: drag.index, start: cue.start_ms, end };
    }
  }

  async function onTrackPointerUp() {
    const d = drag;
    const ov = override;
    drag = null;
    override = null;
    if (d && d.kind !== "scrub" && ov) {
      await store.editCue(d.index, { start_ms: Math.round(ov.start), end_ms: Math.round(ov.end) });
    }
  }

  // ── Playhead + follow ────────────────────────────────────────────────────
  const playheadPx = $derived((store.frame / store.fps) * pxPerSec);
  $effect(() => {
    void playheadPx;
    if (!trackEl) return;
    const x = playheadPx;
    if (x < trackEl.scrollLeft || x > trackEl.scrollLeft + trackEl.clientWidth - 24) {
      trackEl.scrollLeft = Math.max(0, x - trackEl.clientWidth * 0.25);
    }
  });

  function zoom(dir: 1 | -1) {
    const i = ZOOM_STEPS.indexOf(pxPerSec);
    const next = ZOOM_STEPS[Math.min(Math.max((i === -1 ? 3 : i) + dir, 0), ZOOM_STEPS.length - 1)];
    const anchorRatio = trackEl ? (trackEl.scrollLeft + trackEl.clientWidth / 2) / totalPx : 0;
    pxPerSec = next;
    queueMicrotask(() => {
      if (trackEl) trackEl.scrollLeft = Math.max(0, anchorRatio * durationSec * next - trackEl.clientWidth / 2);
    });
  }

  function zoomFit() {
    if (!trackEl) return;
    pxPerSec = Math.max(8, Math.min(384, trackEl.clientWidth / durationSec));
  }
</script>

<section class="timeline" class:collapsed>
  <header>
    <button class="fold" onclick={() => (collapsed = !collapsed)} title={collapsed ? "Expand" : "Collapse"}>
      <svg viewBox="0 0 24 24" width="12" height="12" class:rot={collapsed}>
        <path d="M6 9l6 6 6-6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
      </svg>
    </button>
    <span class="title">Timeline</span>
    <span class="spacer"></span>
    <button class="zoom" onclick={() => zoom(-1)} title="Zoom out">−</button>
    <button class="zoom" onclick={zoomFit} title="Fit">Fit</button>
    <button class="zoom" onclick={() => zoom(1)} title="Zoom in">+</button>
  </header>

  {#if !collapsed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="track"
      bind:this={trackEl}
      onpointerdown={onTrackPointerDown}
      onpointermove={onTrackPointerMove}
      onpointerup={onTrackPointerUp}
      onpointercancel={onTrackPointerUp}
    >
      <div class="content" style={`width: ${totalPx}px`}>
        <div class="ruler">
          {#each ruler as m}
            <span class="mark" style={`left: ${px(m.ms)}px`}></span>
            <span class="label" style={`left: ${px(m.ms)}px`}>{m.label}</span>
          {/each}
        </div>
        <div class="lane">
          {#each blocks as b (b.i)}
            {@const s = override?.index === b.i ? override.start : b.start}
            {@const e = override?.index === b.i ? override.end : b.end}
            <div
              class="block"
              class:dragging={override?.index === b.i}
              style={`left: ${px(s)}px; width: ${Math.max(px(e - s), 2)}px`}
              data-block={b.i}
              title={`#${b.i + 1} ${(s / 1000).toFixed(2)}s – ${(e / 1000).toFixed(2)}s\n${b.label}`}
            >
              <span class="edge" data-edge="l" data-index={b.i}></span>
              <span class="text" data-block={b.i}>{b.label}</span>
              <span class="edge" data-edge="r" data-index={b.i}></span>
            </div>
          {/each}
        </div>
        <div class="playhead" style={`left: ${playheadPx}px`}></div>
      </div>
    </div>
  {/if}
</section>

<style>
  .timeline {
    flex: none;
    display: flex;
    flex-direction: column;
    background: rgb(var(--bg-0));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-lg);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    user-select: none;
  }
  .title {
    font-size: 11px;
    font-family: var(--font-mono);
    color: rgb(var(--fg-2));
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .fold {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border: none;
    background: none;
    color: rgb(var(--fg-2));
    cursor: pointer;
    border-radius: var(--r-sm);
  }
  .fold:hover {
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-0));
  }
  .fold svg {
    transition: transform 150ms ease;
  }
  .fold svg.rot {
    transform: rotate(-90deg);
  }
  .spacer {
    flex: 1;
  }
  .zoom {
    font-family: var(--font-mono);
    font-size: 11px;
    color: rgb(var(--fg-2));
    background: none;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
    padding: 1px 8px;
    cursor: pointer;
    line-height: 18px;
  }
  .zoom:hover {
    color: rgb(var(--fg-0));
    border-color: rgb(var(--accent));
  }
  .track {
    height: 76px;
    overflow-x: auto;
    overflow-y: hidden;
    cursor: default;
    touch-action: none;
  }
  .content {
    position: relative;
    height: 100%;
  }
  .ruler {
    position: relative;
    height: 20px;
    border-bottom: 1px solid rgb(var(--border));
    cursor: ew-resize;
  }
  .mark {
    position: absolute;
    bottom: 0;
    width: 1px;
    height: 5px;
    background: rgb(var(--fg-faint));
  }
  .label {
    position: absolute;
    top: 2px;
    transform: translateX(3px);
    font-family: var(--font-mono);
    font-size: 9px;
    line-height: 1;
    color: rgb(var(--fg-faint));
    white-space: nowrap;
    pointer-events: none;
  }
  .lane {
    position: relative;
    height: 40px;
  }
  .block {
    position: absolute;
    top: 6px;
    height: 28px;
    display: flex;
    align-items: center;
    background: rgb(var(--accent) / 0.18);
    border: 1px solid rgb(var(--accent) / 0.55);
    border-radius: var(--r-sm);
    overflow: hidden;
    box-sizing: border-box;
  }
  .block:hover {
    background: rgb(var(--accent) / 0.28);
  }
  .block.dragging {
    background: rgb(var(--accent) / 0.4);
    z-index: 2;
  }
  .text {
    flex: 1;
    min-width: 0;
    padding: 0 4px;
    font-size: 10px;
    color: rgb(var(--fg-0));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }
  .edge {
    flex: none;
    width: 7px;
    height: 100%;
    cursor: ew-resize;
  }
  .edge:hover {
    background: rgb(var(--accent) / 0.5);
  }
  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1.5px;
    background: rgb(var(--fg-0) / 0.85);
    pointer-events: none;
  }
  .playhead::before {
    content: "";
    position: absolute;
    top: 0;
    left: -3px;
    border: 4px solid transparent;
    border-top-color: rgb(var(--fg-0) / 0.85);
  }
</style>
