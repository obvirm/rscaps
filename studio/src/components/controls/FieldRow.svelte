<script lang="ts">
  let {
    label,
    legend = "",
    children,
  }: { label?: string; legend?: string; children?: import("svelte").Snippet } = $props();
</script>

<div class="field" class:has-label={label != null && label !== ""}>
  {#if label}
    <span class="label">{label}</span>
  {/if}
  <div class="control">
    {@render children?.()}
  </div>
  {#if legend}
    <p class="legend">{legend}</p>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field.has-label {
    display: grid;
    grid-template-columns: minmax(90px, auto) 1fr;
    align-items: center;
    column-gap: 10px;
    row-gap: 4px;
  }
  .label {
    font-size: 13px;
    color: rgb(var(--fg-1));
    line-height: 1.3;
  }
  .control {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .legend {
    grid-column: 1 / -1;
    font-size: 12px;
    line-height: 1.45;
    color: rgb(var(--fg-faint));
  }
</style>
