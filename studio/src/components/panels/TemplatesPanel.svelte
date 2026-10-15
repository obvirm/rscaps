<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "../../lib/state.svelte";

  // Catalog comes from caption-core via the wasm bridge (store.templates) —
  // boot it here so the list is ready even if the editor didn't open a
  // project through the normal path.
  onMount(() => {
    void store.bootWasm();
  });

  let current = $derived.by(() => {
    try {
      const doc = JSON.parse(store.docText);
      return doc.template || "";
    } catch {
      return "";
    }
  });

  async function onChange(e: Event) {
    const id = (e.currentTarget as HTMLSelectElement).value;
    try {
      await store.applyTemplate(id);
    } catch (err) {
      store.setStatus(String(err), true);
    }
  }
</script>

<div class="templates-panel">
  <label class="section-label" for="template-select">Template</label>
  <div class="picker">
    <select id="template-select" value={current} onchange={onChange}>
      <option value="">Default (v0)</option>
      {#each store.templates as { id, label } (id)}
        <option value={id}>{label}</option>
      {/each}
    </select>
  </div>
  <p class="note">
    Native templates render 1:1 through fframes — same engine in preview and export.
  </p>
</div>

<style>
  .templates-panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }
  .section-label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: rgb(var(--fg-2));
  }
  .picker {
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-md);
    padding: 10px;
  }
  .picker select {
    width: 100%;
    background: rgb(var(--bg-0));
    color: rgb(var(--fg-0));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-sm);
    font-size: 13px;
    padding: 8px 10px;
  }
  .picker select:focus {
    border-color: rgb(var(--accent));
  }
  .note {
    font-size: 12px;
    line-height: 1.6;
    color: rgb(var(--fg-2));
    background: rgb(var(--bg-2));
    border: 1px dashed rgb(var(--border-strong));
    border-radius: var(--r-md);
    padding: 14px;
  }
</style>
