<script lang="ts">
  let { title = "", children }: { title?: string; children?: import("svelte").Snippet } = $props();
</script>

<section class="section">
  {#if title}
    <h3 class="section-title">{title}</h3>
  {/if}
  <div class="rows">
    {@render children?.()}
  </div>
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 18px 0;
    border-top: 1px solid rgb(var(--border-subtle));
  }
  .section:first-child {
    border-top: none;
    padding-top: 4px;
  }
  .section-title {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: rgb(var(--fg-2));
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
</style>
