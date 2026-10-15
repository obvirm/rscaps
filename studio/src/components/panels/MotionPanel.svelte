<script lang="ts">
  import { store } from "../../lib/state.svelte";
  import Section from "../controls/Section.svelte";
  import Slider from "../controls/Slider.svelte";
  import SelectField from "../controls/SelectField.svelte";
  import StyleControlRow from "../controls/StyleControlRow.svelte";

  const params = $derived(store.params);
  // Template-declared motion controls (group: "motion") — tscaps shows these
  // under the Motion tab, not Style.
  const motionControls = $derived(
    (store.activeTemplate?.styleControls ?? []).filter((c) => c.group === "motion"),
  );

  const presetOptions = [
    { value: "", label: "Template default" },
    { value: "static", label: "None (static)" },
  ];

  const preset = $derived(params?.motion_preset ?? "");
  const duration = $derived(params?.motion_duration_ms ?? 250);
  const distance = $derived(params?.motion_distance_em ?? 0.15);
</script>

<div class="motion-panel">
  <Section title="Entrance">
    <SelectField
      label="Animation"
      value={preset}
      options={presetOptions}
      onchange={(v) => store.setParams({ motion_preset: v === "" ? undefined : v })}
      legend="Template default uses the entrance baked into the template. None renders captions without motion."
    />
    <Slider
      label="Duration"
      value={duration}
      min={50}
      max={2000}
      step={10}
      unit="ms"
      onchange={(v) => store.setParams({ motion_duration_ms: v })}
    />
    <Slider
      label="Distance"
      value={distance}
      min={0}
      max={2}
      step={0.01}
      unit="em"
      onchange={(v) => store.setParams({ motion_distance_em: v })}
    />
  </Section>

  {#if motionControls.length > 0}
    <Section title="Template motion">
      {#each motionControls as c (c.id)}
        <StyleControlRow
          control={c}
          value={store.params?.style_values?.[c.id]}
          onchange={(v) => store.setStyleValue(c.id, v)}
        />
      {/each}
    </Section>
  {/if}
</div>

<style>
  .motion-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
