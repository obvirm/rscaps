<script lang="ts">
  import { Copy, Pipette } from "@lucide/svelte";

  let {
    label = "",
    value = $bindable("#ffffff"),
    onchange,
  }: {
    label?: string;
    value?: string;
    onchange?: (v: string) => void;
  } = $props();

  let open = $state(false);
  let text = $state(value);
  let anchor: HTMLElement;
  let popX = $state(0);
  let popY = $state(0);

  function toggleOpen() {
    if (!open && anchor) {
      const r = anchor.getBoundingClientRect();
      popX = Math.min(r.left, window.innerWidth - 196);
      popY = r.bottom + 6;
    }
    open = !open;
  }

  $effect(() => {
    if (!open) text = value;
  });

  function normalize(v: string): string | null {
    let s = v.trim().replace(/^#/, "");
    if (/^[0-9a-fA-F]{3}$/.test(s)) s = s.split("").map((c) => c + c).join("");
    if (/^[0-9a-fA-F]{6}$/.test(s)) return `#${s.toLowerCase()}`;
    if (/^[0-9a-fA-F]{8}$/.test(s)) return `#${s.toLowerCase()}`;
    return null;
  }

  const rgbHex = $derived((normalize(value) ?? "#ffffff").slice(0, 7));

  function commit(v: string) {
    const n = normalize(v);
    if (!n) {
      text = value;
      return;
    }
    text = n;
    value = n;
    onchange?.(n);
  }

  function onNative(e: Event) {
    const hex = (e.currentTarget as HTMLInputElement).value;
    const alpha = (normalize(value) ?? "#ffffffff").slice(7) || "ff";
    commit(hex + alpha);
  }
</script>

<div class="row">
  {#if label !== ""}
    <span class="label">{label}</span>
  {/if}
  <div class="swatch-wrap" bind:this={anchor}>
    <button
      class="swatch"
      aria-label="{label} color"
      style:background="linear-gradient({value}, {value}), conic-gradient(#ccc 25%, #fff 0 50%, #ccc 0 75%, #fff 0)"
      style:background-blend-mode="multiply"
      onclick={toggleOpen}
    ></button>
    <span class="hex">{value}</span>
    <button class="mini" title="Copy" aria-label="Copy color" onclick={() => navigator.clipboard?.writeText(value)}>
      <Copy size={12} />
    </button>
  </div>
</div>

{#if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="popover"
    role="dialog"
    aria-label="{label} picker"
    style:left="{popX}px"
    style:top="{popY}px"
    onclick={(e) => e.stopPropagation()}
  >
    <input class="native" type="color" value={rgbHex} oninput={onNative} />
    <div class="hex-row">
      <Pipette size={12} />
      <input
        class="hex-input"
        type="text"
        bind:value={text}
        spellcheck="false"
        onkeydown={(e) => {
          if (e.key === "Enter") {
            commit(text);
            (e.currentTarget as HTMLInputElement).blur();
          }
          if (e.key === "Escape") open = false;
        }}
        onblur={() => commit(text)}
      />
    </div>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={() => (open = false)}></div>
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
  .swatch-wrap {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .swatch {
    width: 26px;
    height: 26px;
    border-radius: var(--r-xs);
    border: 1px solid rgb(var(--border));
    cursor: pointer;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.2);
  }
  .swatch:hover {
    border-color: rgb(var(--border-strong));
  }
  .hex {
    font: 12px var(--font-mono);
    color: rgb(var(--fg-1));
    letter-spacing: 0.02em;
  }
  .mini {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: var(--r-xs);
    color: rgb(var(--fg-2));
    opacity: 0;
    transition: opacity var(--motion) var(--ease);
  }
  .swatch-wrap:hover .mini {
    opacity: 1;
  }
  .mini:hover {
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-0));
  }
  .popover {
    position: fixed;
    z-index: 60;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border-subtle));
    border-radius: var(--r-sm);
    box-shadow: var(--shadow-md);
    width: 180px;
  }
  .native {
    width: 100%;
    height: 72px;
    padding: 0;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    background: none;
    cursor: pointer;
  }
  .hex-row {
    display: flex;
    align-items: center;
    gap: 6px;
    color: rgb(var(--fg-2));
  }
  .hex-input {
    flex: 1;
    min-width: 0;
    background: rgb(var(--bg-0));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-0));
    font: 12px var(--font-mono);
    padding: 5px 7px;
  }
  .hex-input:focus {
    border-color: rgb(var(--accent));
    outline: none;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
  }
</style>
