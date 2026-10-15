<script lang="ts">
  let {
    label = "",
    value = $bindable(),
    min = 0,
    max = 1,
    step = 0.01,
    unit = "",
    onchange,
  }: {
    label?: string;
    value: number;
    min?: number;
    max?: number;
    step?: number;
    unit?: string;
    onchange?: (v: number) => void;
  } = $props();

  let text = $state("");
  let editing = $state(false);

  const decimals = $derived(Math.max(0, String(step).split(".")[1]?.length ?? 0));
  const display = $derived(editing ? text : format(value));

  function format(v: number): string {
    const n = Number(v.toFixed(decimals));
    return String(n);
  }

  function clampSnap(v: number): number {
    const clamped = Math.min(Math.max(v, min), max);
    const snapped = Number((Math.round(clamped / step) * step).toFixed(decimals + 4));
    return Number(snapped.toFixed(decimals));
  }

  function commit(v: number) {
    const next = clampSnap(v);
    value = next;
    onchange?.(next);
  }

  function onRange(e: Event) {
    commit(Number((e.currentTarget as HTMLInputElement).value));
  }

  function onTextKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      (e.currentTarget as HTMLInputElement).blur();
    } else if (e.key === "Escape") {
      editing = false;
      text = format(value);
      (e.currentTarget as HTMLInputElement).blur();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const dir = e.key === "ArrowUp" ? 1 : -1;
      const base = Number(text) || value;
      commit(base + dir * step * (e.shiftKey ? 10 : 1));
      text = format(value);
    }
  }
</script>

<div class="slider" class:has-label={label !== ""}>
  {#if label !== ""}
    <span class="label">{label}</span>
  {/if}
  <input
    class="range"
    type="range"
    {min}
    {max}
    {step}
    {value}
    aria-label={label}
    oninput={onRange}
  />
  <input
    class="chip"
    type="text"
    inputmode="decimal"
    value={display}
    aria-label="{label} value"
    onfocus={() => {
      editing = true;
      text = format(value);
    }}
    onblur={() => {
      editing = false;
      const n = Number(text);
      if (Number.isFinite(n)) commit(n);
    }}
    onkeydown={onTextKey}
  />
  {#if unit}
    <span class="unit">{unit}</span>
  {/if}
</div>

<style>
  .slider {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    width: 100%;
  }
  .label {
    font-size: 13px;
    color: rgb(var(--fg-1));
    min-width: 90px;
    flex: none;
  }
  .range {
    flex: 1;
    min-width: 0;
    height: 3px;
    appearance: none;
    background: rgb(var(--bg-3));
    border-radius: 999px;
    cursor: pointer;
  }
  .range::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: rgb(var(--fg-0));
    border: none;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.4);
    transition: background-color var(--motion) var(--ease);
  }
  .range:hover::-webkit-slider-thumb {
    background: rgb(var(--accent));
  }
  .range::-moz-range-thumb {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: rgb(var(--fg-0));
    border: none;
  }
  .chip {
    width: 6ch;
    flex: none;
    background: transparent;
    border: none;
    border-bottom: 1px solid transparent;
    font: 12px var(--font-mono);
    color: rgb(var(--fg-0));
    text-align: right;
    padding: 2px 0;
    transition:
      border-color var(--motion) var(--ease),
      color var(--motion) var(--ease);
  }
  .chip:hover {
    border-bottom-color: rgb(var(--fg-faint) / 0.5);
  }
  .chip:focus {
    outline: none;
    border-bottom-color: rgb(var(--accent));
  }
  .unit {
    font-size: 11px;
    color: rgb(var(--fg-faint));
    flex: none;
    min-width: 2ch;
  }
</style>
