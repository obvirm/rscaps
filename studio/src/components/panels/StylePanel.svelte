<script lang="ts">
  import { RotateCcw } from "@lucide/svelte";
  import { store } from "../../lib/state.svelte";
  import type { StyleControl } from "../../lib/types";
  import Section from "../controls/Section.svelte";
  import SelectField from "../controls/SelectField.svelte";
  import ColorField from "../controls/ColorField.svelte";
  import StyleControlRow from "../controls/StyleControlRow.svelte";

  const tpl = $derived(store.activeTemplate);
  const controls = $derived(
    (tpl?.styleControls ?? []).filter((c) => (c.group ?? "style") !== "motion"),
  );
  const variants = $derived(tpl?.variants ?? []);

  // Group by subgroup (tscaps FieldsSection), preserving first-seen order.
  const groups = $derived.by(() => {
    const map = new Map<string, StyleControl[]>();
    for (const c of controls) {
      const key = c.subgroup ?? "";
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(c);
    }
    return [...map.entries()];
  });

  const colorControls = $derived(controls.filter((c) => c.type === "color"));
  const nonColorControls = $derived(controls.filter((c) => c.type !== "color"));

  const titleOf = $derived((sub: string) =>
    sub === "" ? "" : sub.charAt(0).toUpperCase() + sub.slice(1).replace(/-/g, " "),
  );

  function sv(id: string, fallback: unknown): string | number | boolean | undefined {
    const v = store.params?.style_values?.[id];
    if (v !== undefined) return v;
    if (typeof fallback === "string" || typeof fallback === "number" || typeof fallback === "boolean") {
      return fallback;
    }
    return undefined;
  }

  // Variant (preset) selection: matches when every override equals current.
  const activeVariant = $derived.by(() => {
    const current = store.params?.style_values ?? {};
    const idx = variants.findIndex((v) =>
      Object.entries(v.overrides).every(([k, val]) => current[k] === val),
    );
    return idx >= 0 ? String(idx) : "";
  });

  function pickVariant(v: string) {
    if (v === "") return;
    const variant = variants[Number(v)];
    if (variant) store.applyVariantOverrides(variant.overrides);
  }

  const title = $derived(tpl?.label ?? "Default (v0)");
</script>

<div class="style-panel">
  <button
    class="restore-btn"
    onclick={() => store.restoreTemplateSettings()}
    title="Kembalikan Style/Typography/Position/Effects ke default template (bisa di-undo)"
  >
    <RotateCcw size={14} />
    Restore template defaults
  </button>
  {#if variants.length > 0}
    <Section title="Preset">
      <SelectField
        label="Variant"
        value={activeVariant}
        options={[
          { value: "", label: "Custom" },
          ...variants.map((v, i) => ({ value: String(i), label: v.label })),
        ]}
        onchange={pickVariant}
      />
    </Section>
  {/if}

  {#if controls.length === 0}
    <p class="empty">This template has no style controls.</p>
  {:else}
    {#if colorControls.length > 0}
      <Section title="Colors">
        <div class="color-grid">
          {#each colorControls as c (c.id)}
            <ColorField
              label={c.label}
              value={String(sv(c.id, c.default) ?? "#ffffff")}
              onchange={(v) => store.setStyleValue(c.id, v)}
            />
          {/each}
        </div>
      </Section>
    {/if}

    {#each groups as [sub, items] (sub)}
      {@const rest = items.filter((c) => c.type !== "color")}
      {#if rest.length > 0}
        <Section title={titleOf(sub) || (sub === "" ? title : sub)}>
          {#each rest as c (c.id)}
            <StyleControlRow
              control={c}
              value={store.params?.style_values?.[c.id]}
              onchange={(v) => store.setStyleValue(c.id, v)}
            />
          {/each}
        </Section>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .style-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .restore-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    margin: 8px 0 4px;
    padding: 7px 10px;
    border: 1px solid rgb(var(--border));
    border-radius: 8px;
    background: rgb(var(--bg-2));
    color: rgb(var(--fg));
    font-size: 12.5px;
    cursor: pointer;
  }
  .restore-btn:hover {
    background: rgb(var(--bg-3, var(--bg-2)));
    border-color: rgb(var(--accent));
  }
  .empty {
    font-size: 13px;
    color: rgb(var(--fg-2));
    padding: 12px 0;
  }
  .color-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 6px 12px;
  }
</style>
