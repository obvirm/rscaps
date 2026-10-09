<script lang="ts">
  import { store } from "../../lib/state.svelte";
  import type { Direction } from "../../lib/types";

  let direction = $derived.by<Direction>(() => {
    try {
      const doc = JSON.parse(store.docText);
      return (doc.params?.direction as Direction) || "auto";
    } catch {
      return "auto";
    }
  });

  async function onApply() {
    try {
      await store.applyDocument();
    } catch (err) {
      store.setStatus(String(err), true);
    }
  }

  async function onDirection(e: Event) {
    const dir = (e.currentTarget as HTMLSelectElement).value as Direction;
    if (!store.setDirection(dir)) {
      store.setStatus("invalid document JSON", true);
      return;
    }
    try {
      await store.applyDocument();
    } catch (err) {
      store.setStatus(String(err), true);
    }
  }
</script>

<div class="code-panel">
  <label class="section-label" for="doc">document (JSON)</label>
  <textarea id="doc" spellcheck="false" bind:value={store.docText}></textarea>

  <div class="actions">
    <label class="dir-field">Direction
      <select value={direction} onchange={onDirection} title="Base text direction (RTL for Arabic/Hebrew content)">
        <option value="auto">Auto</option>
        <option value="ltr">LTR</option>
        <option value="rtl">RTL</option>
      </select>
    </label>
    <button class="btn accent" onclick={onApply}>Apply + prepare</button>
  </div>
</div>

<style>
  .code-panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    flex: 1;
  }
  .section-label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: rgb(var(--fg-2));
  }
  textarea {
    flex: 1;
    min-height: 120px;
    width: 100%;
    resize: none;
    background: rgb(var(--bg-0));
    color: rgb(var(--fg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
    font: 12px/1.55 var(--font-mono);
    padding: 10px;
    tab-size: 2;
  }
  textarea:focus {
    border-color: rgb(var(--accent));
  }
  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .dir-field {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: rgb(var(--fg-2));
  }
  .dir-field select {
    background: rgb(var(--bg-0));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-sm);
    padding: 5px 7px;
    font-size: 12px;
    color: rgb(var(--fg-0));
  }
  .dir-field select:focus {
    border-color: rgb(var(--accent));
  }
  .btn {
    padding: 6px 12px;
    font-size: 12.5px;
    font-weight: 500;
    border-radius: var(--r-sm);
    border: 1px solid transparent;
  }
  .btn.accent {
    background: rgb(var(--accent));
    color: rgb(var(--fg-on-accent));
  }
  .btn.accent:hover {
    background: rgb(var(--accent-hi));
  }
</style>
