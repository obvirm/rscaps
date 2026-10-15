<script lang="ts">
  let {
    label = "",
    checked = $bindable(false),
    onchange,
    legend = "",
  }: {
    label?: string;
    checked?: boolean;
    onchange?: (v: boolean) => void;
    legend?: string;
  } = $props();

  function toggle() {
    checked = !checked;
    onchange?.(checked);
  }
</script>

<div class="row">
  {#if label !== ""}
    <span class="label">{label}</span>
  {/if}
  <button
    class="switch"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    onclick={toggle}
  >
    <span class="thumb" class:on={checked}></span>
  </button>
  <span class="state">{checked ? "On" : "Off"}</span>
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
  .switch {
    position: relative;
    width: 32px;
    height: 18px;
    flex: none;
    border-radius: var(--r-pill);
    background: rgb(var(--bg-3));
    border: 1px solid rgb(var(--border));
    transition:
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .switch[aria-checked="true"] {
    background: rgb(var(--accent) / 0.25);
    border-color: rgb(var(--accent));
  }
  .thumb {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: rgb(var(--fg-1));
    transition: transform var(--motion) var(--ease);
  }
  .thumb.on {
    transform: translateX(14px);
    background: rgb(var(--accent));
  }
  .state {
    font-size: 11px;
    font-family: var(--font-mono);
    color: rgb(var(--fg-faint));
    min-width: 3ch;
  }
  .legend {
    font-size: 12px;
    line-height: 1.45;
    color: rgb(var(--fg-faint));
  }
</style>
