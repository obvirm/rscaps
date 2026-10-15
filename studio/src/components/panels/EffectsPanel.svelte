<script lang="ts">
  import { store } from "../../lib/state.svelte";
  import Section from "../controls/Section.svelte";
  import Toggle from "../controls/Toggle.svelte";
  import Slider from "../controls/Slider.svelte";
  import SelectField from "../controls/SelectField.svelte";
  import { EFFECT_KEYS, effEffectFlag, effEmoji } from "../../lib/params";

  const params = $derived(store.params);
  const defaults = $derived(store.activeTemplate?.defaults);
  const emoji = $derived(effEmoji(params, defaults));

  const FLAGS = [
    { key: "gap_free", label: "Gap-free", legend: "Remove silence gaps between words." },
    {
      key: "remove_punctuation",
      label: "Remove punctuation",
      legend: "Strip punctuation from caption text.",
    },
    {
      key: "smart_punctuation",
      label: "Smart punctuation",
      legend: "Add curly quotes and ellipses automatically.",
    },
    { key: "carry_quotes", label: "Carry quotes", legend: "Keep quotation marks open across lines." },
    {
      key: "smart_lowercase",
      label: "Smart lowercase",
      legend: "Lowercase mid-sentence words that don't need caps.",
    },
  ] as const;

  const placementOptions = [
    { value: "segment-below", label: "Below the caption" },
    { value: "segment-above", label: "Above the caption" },
    { value: "word", label: "Inline with word" },
  ];
</script>

<div class="effects-panel">
  <Section>
    {#each FLAGS as f (f.key)}
      <Toggle
        label={f.label}
        legend={f.legend}
        checked={effEffectFlag(params, defaults, f.key)}
        onchange={(v) => store.setEffectsParams({ [f.key]: v })}
      />
    {/each}
  </Section>

  <Section title="Emojis">
    <SelectField
      label="Placement"
      value={emoji.placement}
      options={placementOptions}
      onchange={(v) => store.setEffectsParams({ emoji_placement: v })}
    />
    <Slider
      label="Size"
      value={emoji.size}
      min={0.5}
      max={3}
      step={0.05}
      onchange={(v) => store.setEffectsParams({ emoji_size: v })}
    />
    <Slider
      label="Gap"
      value={emoji.gap}
      min={-5}
      max={5}
      step={0.1}
      unit="em"
      onchange={(v) => store.setEffectsParams({ emoji_gap: v })}
    />
  </Section>
</div>

<style>
  .effects-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
