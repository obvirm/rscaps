<script lang="ts">
  import type { Component } from "svelte";

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type IconComp = Component<any>;
  type Opt = { value: string; label?: string; icon?: IconComp };

  let {
    value = $bindable(),
    options,
    onchange,
    ariaLabel = "",
  }: {
    value?: string;
    options: Opt[];
    onchange?: (v: string) => void;
    ariaLabel?: string;
  } = $props();

  function pick(v: string) {
    value = v;
    onchange?.(v);
  }
</script>

<div class="segmented" role="radiogroup" aria-label={ariaLabel}>
  {#each options as opt (opt.value)}
    {@const Icon = opt.icon}
    <button
      class="seg"
      class:active={value === opt.value}
      role="radio"
      aria-checked={value === opt.value}
      aria-label={opt.label ?? opt.value}
      title={opt.label ?? opt.value}
      onclick={() => pick(opt.value)}
    >
      {#if Icon}
        <Icon size={14} />
      {:else}
        <span>{opt.label ?? opt.value}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
  }
  .seg {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    min-width: 28px;
    height: 22px;
    padding: 0 7px;
    border-radius: 3px;
    color: rgb(var(--fg-2));
    font-size: 12px;
    transition:
      color var(--motion) var(--ease),
      background-color var(--motion) var(--ease);
  }
  .seg:hover {
    color: rgb(var(--fg-0));
  }
  .seg.active {
    background: rgb(var(--bg-3));
    color: rgb(var(--accent));
    box-shadow: var(--shadow-raised);
  }
</style>
