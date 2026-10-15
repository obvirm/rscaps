<script lang="ts">
  import { store } from "../../lib/state.svelte";
  import Section from "../controls/Section.svelte";
  import Slider from "../controls/Slider.svelte";
  import { effAnchorX, effAnchorY } from "../../lib/params";

  const params = $derived(store.params);
  const defaults = $derived(store.activeTemplate?.defaults);

  const ax = $derived(effAnchorX(params, defaults));
  const ay = $derived(effAnchorY(params, defaults, store.defaultParams));
  const angle = $derived(params?.angle_deg ?? 0);

  const cols = ["left", "center", "right"] as const;
  const rows = ["top", "center", "bottom"] as const;

  function cellOf(): { col: (typeof cols)[number]; row: (typeof rows)[number] } {
    const col =
      params?.h_align === "left" || params?.h_align === "center" || params?.h_align === "right"
        ? params.h_align
        : ax < 0.34
          ? "left"
          : ax > 0.66
            ? "right"
            : "center";
    const row =
      params?.v_align === "top" || params?.v_align === "center" || params?.v_align === "bottom"
        ? params.v_align
        : ay < 0.34
          ? "top"
          : ay > 0.66
            ? "bottom"
            : "center";
    return { col, row };
  }

  const cell = $derived(cellOf());

  function setV(v: number) {
    store.setParams({ v_align: undefined, v_offset: v });
  }
  function setH(v: number) {
    store.setParams({ h_align: undefined, h_offset: v });
  }
  function setAnchor(col: string, row: string) {
    // Pertahankan titik anchor absolut saat sisi blok berganti — engine
    // override: top→v_offset, bottom→1−v_offset, center→tanpa offset.
    // Tanpa ini, offset tak-terdefinisi =0 → blok melompat ke tepi ekstrem.
    const patch: Record<string, string | number | undefined> = {
      h_align: col,
      v_align: row,
    };
    patch.v_offset =
      row === "center" ? undefined : row === "bottom" ? 1 - ay : ay;
    patch.h_offset =
      col === "center" ? undefined : col === "right" ? 1 - ax : ax;
    store.setParams(patch);
  }
  function setAngle(v: number) {
    store.setParams({ angle_deg: v });
  }

  const readout = $derived(
    `The (${cell.col}, ${cell.row}) point of the block sits at ${Math.round(ax * 100)}% from left and ${Math.round(ay * 100)}% from top.`,
  );
</script>

<div class="position-panel">
  <Section>
    <Slider label="Vertical" value={ay} min={0} max={1} step={0.01} onchange={setV} />
    <Slider label="Horizontal" value={ax} min={0} max={1} step={0.01} onchange={setH} />
  </Section>

  <Section title="Anchor">
    <div class="anchor-wrap">
      <div class="grid" role="radiogroup" aria-label="Anchor">
        {#each rows as row (row)}
          {#each cols as col (col)}
            <button
              class="cell"
              class:active={cell.col === col && cell.row === row}
              role="radio"
              aria-checked={cell.col === col && cell.row === row}
              aria-label="{row} {col}"
              title="{row} {col}"
              onclick={() => setAnchor(col, row)}
            >
              <span class="dot"></span>
            </button>
          {/each}
        {/each}
      </div>

      <div class="mini">
        <span
          class="mini-block"
          style:left="{ax * 100}%"
          style:top="{ay * 100}%"
        ></span>
      </div>
    </div>

    <p class="readout">{readout}</p>
  </Section>

  <Section title="Rotation">
    <Slider label="Angle" value={angle} min={-180} max={180} step={1} unit="°" onchange={setAngle} />
  </Section>
</div>

<style>
  .position-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .anchor-wrap {
    display: flex;
    align-items: center;
    gap: 18px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(3, 18px);
    grid-template-rows: repeat(3, 18px);
    gap: 3px;
    flex: none;
  }
  .cell {
    width: 18px;
    height: 18px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid rgb(var(--border));
    border-radius: 3px;
    background: rgb(var(--bg-2));
    transition:
      border-color var(--motion) var(--ease),
      background-color var(--motion) var(--ease);
  }
  .cell:hover {
    border-color: rgb(var(--border-strong));
  }
  .cell.active {
    border-color: rgb(var(--accent));
    background: rgb(var(--accent) / 0.15);
  }
  .dot {
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: rgb(var(--fg-2));
  }
  .cell.active .dot {
    width: 5px;
    height: 5px;
    background: rgb(var(--accent));
  }
  .mini {
    position: relative;
    width: 100px;
    height: 58px;
    flex: none;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    background: rgb(var(--bg-0));
    overflow: hidden;
  }
  .mini-block {
    position: absolute;
    width: 56px;
    height: 14px;
    transform: translate(-50%, -50%);
    background: rgb(var(--accent) / 0.15);
    border: 1px solid rgb(var(--accent) / 0.45);
    border-radius: 2px;
    transition:
      left var(--motion-base) var(--ease),
      top var(--motion-base) var(--ease);
  }
  .readout {
    font-size: 12px;
    line-height: 1.5;
    color: rgb(var(--fg-faint));
  }
</style>
