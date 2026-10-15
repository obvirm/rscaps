<script lang="ts">
  import { AlignCenter, AlignLeft, AlignRight, Italic, Strikethrough, Underline } from "@lucide/svelte";
  import { store } from "../../lib/state.svelte";
  import { FONT_FAMILIES } from "../../lib/wasm";
  import Section from "../controls/Section.svelte";
  import FieldRow from "../controls/FieldRow.svelte";
  import Slider from "../controls/Slider.svelte";
  import SelectField from "../controls/SelectField.svelte";
  import Segmented from "../controls/Segmented.svelte";
  import {
    effFontFamily,
    effFontSizeCqh,
    effFontWeight,
    effNumber,
    effTextAlign,
    effTextCase,
  } from "../../lib/params";

  const params = $derived(store.params);
  const def = $derived(store.defaultParams);
  const defaults = $derived(store.activeTemplate?.defaults);

  const family = $derived(effFontFamily(params, def, defaults));
  const sizeCqh = $derived(effFontSizeCqh(params, def, defaults, store.videoHeight));
  const weight = $derived(effFontWeight(params, def, defaults));
  const textCase = $derived(effTextCase(params, defaults));
  const textAlign = $derived(effTextAlign(params, defaults));
  const italic = $derived(params?.italic === true);
  const underline = $derived(params?.underline === true);
  const strikethrough = $derived(params?.strikethrough === true);

  const letter = $derived(
    effNumber(params?.letter_spacing_em, typo("letterSpacing"), 0),
  );
  const word = $derived(effNumber(params?.word_spacing_em, typo("wordSpacing"), 0));
  const line = $derived(effNumber(params?.line_spacing_em, typo("lineSpacing"), 0));

  function typo(key: string): unknown {
    return defaults?.typography?.[key];
  }

  const familyOptions = $derived(
    FONT_FAMILIES.includes(family)
      ? FONT_FAMILIES.map((f) => ({ value: f, label: f }))
      : [{ value: family, label: family }, ...FONT_FAMILIES.map((f) => ({ value: f, label: f }))],
  );

  const directionOptions = [
    { value: "auto", label: "Auto" },
    { value: "ltr", label: "Left to right" },
    { value: "rtl", label: "Right to left" },
  ];

  function set(k: string, v: unknown) {
    store.setParams({ [k]: v } as never);
  }

  function setSize(cqh: number) {
    store.setParams({ font_size: (cqh / 100) * store.videoHeight });
  }

  function toggleStyle(key: "italic" | "underline" | "strikethrough") {
    store.setParams({ [key]: !(params?.[key] === true) });
  }

  const styleBtns = $derived([
    { key: "italic" as const, icon: Italic, on: italic, title: "Italic" },
    { key: "underline" as const, icon: Underline, on: underline, title: "Underline" },
    { key: "strikethrough" as const, icon: Strikethrough, on: strikethrough, title: "Strikethrough" },
  ]);
</script>

<div class="typography-panel">
  <Section>
    <FieldRow label="Font">
      <SelectField
        value={family}
        options={familyOptions}
        onchange={(v) => set("font_family", v)}
      />
    </FieldRow>

    <Slider label="Font size" value={sizeCqh} min={1} max={25} step={0.1} unit="cqh" onchange={setSize} />
    <Slider label="Weight" value={weight} min={100} max={900} step={100} onchange={(v) => set("font_weight", v)} />

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
            onclick={() => toggleStyle(b.key)}
          >
            <Icon size={14} />
          </button>
        {/each}
      </div>
    </FieldRow>

    <FieldRow label="Align">
      <Segmented
        value={textAlign}
        ariaLabel="Text align"
        options={[
          { value: "left", icon: AlignLeft, label: "Left" },
          { value: "center", icon: AlignCenter, label: "Center" },
          { value: "right", icon: AlignRight, label: "Right" },
        ]}
        onchange={(v) => set("text_align", v)}
      />
    </FieldRow>

    <FieldRow label="Case">
      <Segmented
        value={textCase}
        ariaLabel="Text case"
        options={[
          { value: "none", label: "aA" },
          { value: "uppercase", label: "AA" },
          { value: "lowercase", label: "aa" },
        ]}
        onchange={(v) => set("text_case", v)}
      />
    </FieldRow>

    <Slider label="Letter spacing" value={letter} min={-0.08} max={0.2} step={0.005} unit="em" onchange={(v) => set("letter_spacing_em", v)} />
    <Slider label="Word spacing" value={word} min={-0.5} max={0.5} step={0.01} unit="em" onchange={(v) => set("word_spacing_em", v)} />
    <Slider label="Line spacing" value={line} min={-0.5} max={1.5} step={0.01} unit="em" onchange={(v) => set("line_spacing_em", v)} />
  </Section>

  <Section title="Direction">
    <SelectField
      label="Direction"
      value={params?.direction ?? "auto"}
      options={directionOptions}
      onchange={(v) => store.setDirection(v as "auto" | "ltr" | "rtl")}
      legend="Auto detects per-line script direction. Override for mixed or bidi text."
    />
  </Section>
</div>

<style>
  .typography-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
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
</style>
