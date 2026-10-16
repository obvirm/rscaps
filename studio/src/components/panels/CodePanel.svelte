<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "../../lib/state.svelte";
  import type { Direction } from "../../lib/types";

  let srtText = $state("");
  let cssText = $state("");
  let applying = $state(false);

  let direction = $derived.by<Direction>(() => {
    const m = cssText.match(/^\s*direction\s*:\s*(auto|ltr|rtl)\s*$/m);
    return (m?.[1] as Direction) || "auto";
  });

  onMount(() => {
    srtText = store.codeSrtText();
    cssText = store.codeParamsCss();
  });

  async function onApply() {
    if (applying) return;
    applying = true;
    try {
      await store.applyCodePanel(srtText, cssText);
    } catch (err) {
      store.setStatus(String(err), true);
    } finally {
      applying = false;
    }
  }

  function onDirection(e: Event) {
    const dir = (e.currentTarget as HTMLSelectElement).value as Direction;
    if (/^\s*direction\s*:/m.test(cssText)) {
      cssText = cssText.replace(/^(\s*direction\s*:\s*)\S+\s*;?$/m, `$1${dir};`);
    } else {
      cssText = `${cssText.trimEnd()}\ndirection: ${dir};\n`;
    }
  }
</script>

<div class="code-panel">
  <label class="section-label" for="srt">captions (SRT)</label>
  <textarea id="srt" class="srt" spellcheck="false" bind:value={srtText}></textarea>

  <label class="section-label" for="css">settings (CSS)</label>
  <textarea id="css" class="css" spellcheck="false" bind:value={cssText}></textarea>

  <div class="actions">
    <label class="dir-field">Direction
      <select value={direction} onchange={onDirection} title="Base text direction (RTL for Arabic/Hebrew content)">
        <option value="auto">Auto</option>
        <option value="ltr">LTR</option>
        <option value="rtl">RTL</option>
      </select>
    </label>
    <button class="btn accent" onclick={onApply} disabled={applying}>
      {applying ? "Applying…" : "Apply + prepare"}
    </button>
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
  .srt {
    flex: 1.2;
    min-height: 100px;
  }
  .css {
    flex: 1;
    min-height: 80px;
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
  .btn.accent:hover:not(:disabled) {
    background: rgb(var(--accent-hi));
  }
  .btn:disabled {
    opacity: 0.6;
  }
</style>
