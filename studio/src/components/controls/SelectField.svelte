<script lang="ts">
  import { ChevronDown } from "@lucide/svelte";

  let {
    label = "",
    value = $bindable(),
    options,
    onchange,
    legend = "",
  }: {
    label?: string;
    value: string;
    options: Array<{ value: string; label: string }>;
    onchange?: (v: string) => void;
    legend?: string;
  } = $props();

  function onInput(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    value = v;
    onchange?.(v);
  }
</script>

<div class="row">
  {#if label !== ""}
    <span class="label">{label}</span>
  {/if}
  <div class="select-wrap">
    <select {value} aria-label={label} onchange={onInput}>
      {#each options as opt (opt.value)}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
    <ChevronDown class="chev" size={14} />
  </div>
</div>
{#if legend}
  <p class="legend">{legend}</p>
{/if}

<style>
  .row {
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
  .select-wrap {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
  }
  select {
    width: 100%;
    height: 26px;
    padding: 0 26px 0 8px;
    font-size: 13px;
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    appearance: none;
    cursor: pointer;
    transition: border-color var(--motion) var(--ease);
    text-overflow: ellipsis;
  }
  select:hover {
    border-color: rgb(var(--border-strong));
  }
  select:focus {
    border-color: rgb(var(--accent));
    outline: none;
  }
  :global(.chev) {
    position: absolute;
    right: 7px;
    top: 50%;
    transform: translateY(-50%);
    color: rgb(var(--fg-2));
    pointer-events: none;
  }
  .legend {
    font-size: 12px;
    line-height: 1.45;
    color: rgb(var(--fg-faint));
  }
</style>
