<script lang="ts">
  import { store } from "../../lib/state.svelte";
  import Section from "../controls/Section.svelte";
  import Slider from "../controls/Slider.svelte";
  import { effLayout } from "../../lib/params";

  const params = $derived(store.params);
  const defaults = $derived(store.activeTemplate?.defaults);
  const layout = $derived(effLayout(params, defaults));
</script>

<div class="layout-panel">
  <Section title="Scenes">
    <Slider
      label="Max chars"
      value={layout.maxChars}
      min={Math.max(layout.minChars + 1, 4)}
      max={80}
      step={1}
      onchange={(v) => store.setParams({ max_chars: v })}
    />
  </Section>

  <Section title="Lines">
    <Slider
      label="Max lines"
      value={layout.maxLines}
      min={1}
      max={6}
      step={1}
      onchange={(v) => store.setParams({ max_lines: v })}
    />
    <Slider
      label="Max width"
      value={Number(layout.maxWidthFrac.toFixed(2))}
      min={0.1}
      max={1}
      step={0.01}
      onchange={(v) => store.setParams({ max_width_frac: v })}
    />
  </Section>
</div>

<style>
  .layout-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
