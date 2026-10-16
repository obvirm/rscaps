<script lang="ts">
  import type { Component } from "svelte";
  import {
    Ban,
    Eye,
    GitMerge,
    Italic,
    Magnet,
    Maximize,
    MousePointer2,
    MoveRight,
    MoveUp,
    Sparkle,
    Strikethrough,
    Underline,
    Vibrate,
    X,
    Zap,
  } from "@lucide/svelte";
  import { store } from "../../lib/state.svelte";
  import { FONT_FAMILIES } from "../../lib/wasm";
  import type { WordOverride } from "../../lib/types";
  import Section from "../controls/Section.svelte";
  import FieldRow from "../controls/FieldRow.svelte";
  import Slider from "../controls/Slider.svelte";
  import ColorField from "../controls/ColorField.svelte";
  import { effAnchorX, effAnchorY, effFontFamily, effFontWeight } from "../../lib/params";

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type IconComp = Component<any>;

  const sel = $derived(store.selectedWord);
  const ov = $derived(
    sel == null ? null : (store.params?.element_overrides?.find((o) => o.word === sel) ?? null),
  );
  const word = $derived(sel == null ? null : (store.words.find((w) => w.index === sel) ?? null));
  const wordText = $derived(word?.text ?? (sel != null ? `#${sel}` : ""));

  const params = $derived(store.params);
  const def = $derived(store.defaultParams);
  const defaults = $derived(store.activeTemplate?.defaults);

  // Lazy refresh: panel dibuka / seleksi berubah → sinkronkan cache kata.
  $effect(() => {
    void store.selectedWord;
    void store.listWords();
  });

  function patch(p: Partial<WordOverride>) {
    if (sel == null) return;
    store.setWordOverride(sel, p);
  }

  function clear() {
    if (sel == null) return;
    store.clearWordOverride(sel);
  }

  // ── TEXT ────────────────────────────────────────────────────────────────
  const italic = $derived(ov?.italic === true);
  const underline = $derived(ov?.underline === true);
  const strikethrough = $derived(ov?.strikethrough === true);

  const styleBtns = $derived([
    { key: "italic" as const, icon: Italic, on: italic, title: "Italic" },
    { key: "underline" as const, icon: Underline, on: underline, title: "Underline" },
    { key: "strikethrough" as const, icon: Strikethrough, on: strikethrough, title: "Strikethrough" },
  ]);

  function setFlag(key: "italic" | "underline" | "strikethrough", v: boolean | undefined) {
    if (sel == null) return;
    const p: Partial<WordOverride> =
      key === "italic" ? { italic: v } : key === "underline" ? { underline: v } : { strikethrough: v };
    store.setWordOverride(sel, p);
  }

  const fontFamily = $derived(ov?.font_family ?? effFontFamily(params, def, defaults));
  const weight = $derived(ov?.font_weight ?? effFontWeight(params, def, defaults));
  const color = $derived(ov?.color ?? params?.color ?? "#ffffff");

  function onFont(e: Event) {
    const v = (e.currentTarget as HTMLInputElement).value.trim();
    patch({ font_family: v === "" ? undefined : v });
  }

  // ── LAYOUT / POSITION ───────────────────────────────────────────────────
  const sizePct = $derived(Math.round((ov?.size_scale ?? 1) * 100));
  const rotation = $derived(ov?.rotation_deg ?? 0);
  const ax = $derived(effAnchorX(params, defaults));
  const ay = $derived(effAnchorY(params, defaults, def));
  const hFrac = $derived(ov?.h_frac ?? ax);
  const vFrac = $derived(ov?.v_frac ?? ay);

  // ── ANIMATION ───────────────────────────────────────────────────────────
  const ANIMS: Array<{ id: string; label: string; icon: IconComp; value: string | undefined }> = [
    { id: "inherited", label: "Inherited", icon: GitMerge, value: undefined },
    { id: "none", label: "None", icon: Ban, value: "none" },
    { id: "fade", label: "Fade in", icon: Eye, value: "fade" },
    { id: "rise", label: "Rise in", icon: MoveUp, value: "rise" },
    { id: "slide", label: "Slide in", icon: MoveRight, value: "slide" },
    { id: "scale", label: "Scale in", icon: Maximize, value: "scale" },
    { id: "pop", label: "Pop in", icon: Sparkle, value: "pop" },
    { id: "snap", label: "Snap in", icon: Magnet, value: "snap" },
    { id: "wobble", label: "Wobble in", icon: Vibrate, value: "wobble" },
    { id: "flash", label: "Flash in", icon: Zap, value: "flash" },
  ];

  const animRaw = $derived((ov?.animation ?? "").trim().toLowerCase());
  const activeAnim = $derived(
    animRaw === "" || animRaw === "inherited" ? "inherited" : animRaw.replace(/-in$/, ""),
  );
  const animDuration = $derived(ov?.animation_duration_s ?? 0.35);

  // ── CSS ─────────────────────────────────────────────────────────────────
  const css = $derived(ov?.css ?? "");

  function onCss(e: Event) {
    patch({ css: (e.currentTarget as HTMLTextAreaElement).value });
  }
</script>

<div class="element-panel">
  {#if sel == null}
    <div class="empty">
      <MousePointer2 size={22} />
      <p>Click a word in the preview to inspect it.</p>
      <p class="hint">Overrides address a word by its global index and win over the template.</p>
    </div>
  {:else}
    <div class="head">
      <div class="head-word">
        <span class="quote">“{wordText}”</span>
        <span class="idx">word {sel}</span>
      </div>
      <button class="clear" disabled={!ov} onclick={clear} title="Remove every override on this word">
        <X size={13} />
        Clear
      </button>
    </div>

    <Section title="Text">
      <FieldRow label="Style">
        <div class="style-strip">
          {#each styleBtns as b (b.key)}
            {@const Icon = b.icon}
            <button
              class="style-btn"
              class:on={b.on}
              title={b.title}
              aria-label={b.title}
              aria-pressed={b.on}
              onclick={() => setFlag(b.key, b.on ? undefined : true)}
            >
              <Icon size={14} />
            </button>
          {/each}
        </div>
      </FieldRow>

      <FieldRow label="Font">
        <input
          class="text-input"
          type="text"
          list="element-font-families"
          value={fontFamily}
          spellcheck="false"
          placeholder="Template default"
          onchange={onFont}
        />
        <datalist id="element-font-families">
          {#each FONT_FAMILIES as f (f)}
            <option value={f}></option>
          {/each}
        </datalist>
      </FieldRow>

      <Slider label="Weight" value={weight} min={100} max={900} step={100} onchange={(v) => patch({ font_weight: v })} />
      <ColorField label="Color" value={color} onchange={(v) => patch({ color: v })} />
    </Section>

    <Section title="Layout">
      <Slider
        label="Size"
        value={sizePct}
        min={20}
        max={300}
        step={5}
        unit="%"
        onchange={(v) => patch({ size_scale: v / 100 })}
      />
      <Slider
        label="Rotation"
        value={rotation}
        min={-180}
        max={180}
        step={1}
        unit="°"
        onchange={(v) => patch({ rotation_deg: v })}
      />
    </Section>

    <Section title="Position">
      <Slider label="Vertical" value={vFrac} min={0} max={1} step={0.01} onchange={(v) => patch({ v_frac: v })} />
      <Slider label="Horizontal" value={hFrac} min={0} max={1} step={0.01} onchange={(v) => patch({ h_frac: v })} />
      <p class="legend">Centre of the word as a fraction of the frame. Inherited keeps the layout position.</p>
    </Section>

    <Section title="Animation">
      <div class="anim-grid" role="radiogroup" aria-label="Word animation">
        {#each ANIMS as a (a.id)}
          {@const Icon = a.icon}
          <button
            class="anim-card"
            class:active={activeAnim === a.id}
            role="radio"
            aria-checked={activeAnim === a.id}
            aria-label={a.label}
            title={a.label}
            onclick={() => patch({ animation: a.value })}
          >
            <Icon size={16} />
            <span>{a.label}</span>
          </button>
        {/each}
      </div>
      <Slider
        label="Duration"
        value={animDuration}
        min={0.1}
        max={1.5}
        step={0.05}
        unit="s"
        onchange={(v) => patch({ animation_duration_s: v })}
      />
    </Section>

    <Section title="CSS">
      <textarea
        class="css"
        spellcheck="false"
        value={css}
        placeholder="color: gold; font-weight: 700;"
        onchange={onCss}
      ></textarea>
      <p class="legend">
        Wins over the template. Animations need var(--on-word-being-narrated-starts) as their delay.
      </p>
    </Section>
  {/if}
</div>

<style>
  .element-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 40px 16px;
    text-align: center;
    color: rgb(var(--fg-2));
  }
  .empty p {
    font-size: 13px;
    line-height: 1.5;
  }
  .empty .hint {
    font-size: 12px;
    color: rgb(var(--fg-faint));
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 6px 0 2px;
  }
  .head-word {
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .quote {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 15px;
    font-weight: 600;
    color: rgb(var(--fg-0));
  }
  .idx {
    flex: none;
    font: 11px var(--font-mono);
    color: rgb(var(--fg-faint));
  }
  .clear {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 5px 9px;
    border: 1px solid rgb(var(--border));
    border-radius: 8px;
    background: rgb(var(--bg-2));
    color: rgb(var(--fg));
    font-size: 12px;
    cursor: pointer;
    transition:
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .clear:hover:enabled {
    background: rgb(var(--bg-3));
    border-color: rgb(var(--accent));
  }
  .clear:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .style-strip {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
  }
  .style-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 22px;
    border-radius: 3px;
    color: rgb(var(--fg-2));
    transition:
      color var(--motion) var(--ease),
      background-color var(--motion) var(--ease);
  }
  .style-btn:hover {
    color: rgb(var(--fg-0));
  }
  .style-btn.on {
    background: rgb(var(--bg-3));
    color: rgb(var(--accent));
    box-shadow: var(--shadow-raised);
  }
  .text-input {
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 8px;
    font-size: 13px;
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    transition: border-color var(--motion) var(--ease);
  }
  .text-input:hover {
    border-color: rgb(var(--border-strong));
  }
  .text-input:focus {
    border-color: rgb(var(--accent));
    outline: none;
  }
  .anim-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 6px;
  }
  .anim-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 62px;
    padding: 8px 4px;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-2));
    font-size: 11px;
    line-height: 1.2;
    text-align: center;
    cursor: pointer;
    transition:
      color var(--motion) var(--ease),
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .anim-card:hover {
    color: rgb(var(--fg-0));
    border-color: rgb(var(--border-strong));
  }
  .anim-card.active {
    color: rgb(var(--accent));
    border-color: rgb(var(--accent));
    background: rgb(var(--accent) / 0.15);
  }
  .css {
    width: 100%;
    min-height: 120px;
    resize: vertical;
    background: rgb(var(--bg-0));
    color: rgb(var(--fg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
    font: 12px/1.55 var(--font-mono);
    padding: 10px;
    tab-size: 2;
  }
  .css:focus {
    border-color: rgb(var(--accent));
    outline: none;
  }
  .legend {
    font-size: 12px;
    line-height: 1.45;
    color: rgb(var(--fg-faint));
  }
</style>
