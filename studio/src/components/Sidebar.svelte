<script lang="ts">
  import { onMount } from "svelte";
  import {
    LayoutTemplate,
    Subtitles,
    Type,
    Palette,
    Move,
    Orbit,
    Sparkles,
    WrapText,
    MousePointer2,
    Code2,
  } from "@lucide/svelte";
  import { store } from "../lib/state.svelte";
  import TemplatesPanel from "./panels/TemplatesPanel.svelte";
  import CodePanel from "./panels/CodePanel.svelte";
  import ElementPanel from "./panels/ElementPanel.svelte";
  import TranscriptPanel from "./panels/TranscriptPanel.svelte";
  import TypographyPanel from "./panels/TypographyPanel.svelte";
  import StylePanel from "./panels/StylePanel.svelte";
  import PositionPanel from "./panels/PositionPanel.svelte";
  import MotionPanel from "./panels/MotionPanel.svelte";
  import EffectsPanel from "./panels/EffectsPanel.svelte";
  import LayoutPanel from "./panels/LayoutPanel.svelte";

  let { active = $bindable("templates") }: { active?: string } = $props();

  // Template catalog (style controls/defaults) comes from caption-core —
  // boot the bridge so every panel has data even on cold open.
  onMount(() => {
    void store.bootWasm();
  });

  const ICON_SIZE = 22;

  const rail = [
    { id: "templates", label: "Templates", icon: LayoutTemplate },
    { id: "transcript", label: "Transcript", icon: Subtitles },
    { id: "typography", label: "Typography", icon: Type },
    { id: "style", label: "Style", icon: Palette },
    { id: "position", label: "Position", icon: Move },
    { id: "motion", label: "Motion", icon: Orbit },
    { id: "effects", label: "Effects", icon: Sparkles },
    { id: "layout", label: "Layout", icon: WrapText },
    { id: "element", label: "Element", icon: MousePointer2 },
    { id: "code", label: "Code", icon: Code2 },
  ];

  const activeLabel = $derived(rail.find((r) => r.id === active)?.label ?? active);

  // Klik kata di preview → buka tab Element supaya seleksi langsung terlihat.
  $effect(() => {
    if (store.selectedWord != null) active = "element";
  });
</script>

<aside class="sidebar-card">
  <div class="content thin-scroll">
    <div class="tab-header">
      <h2 class="tab-title">{activeLabel}</h2>
    </div>

    <div class="tab-body">
      {#if active === "templates"}
        <TemplatesPanel />
      {:else if active === "transcript"}
        <TranscriptPanel />
      {:else if active === "typography"}
        <TypographyPanel />
      {:else if active === "style"}
        <StylePanel />
      {:else if active === "position"}
        <PositionPanel />
      {:else if active === "motion"}
        <MotionPanel />
      {:else if active === "effects"}
        <EffectsPanel />
      {:else if active === "layout"}
        <LayoutPanel />
      {:else if active === "element"}
        <ElementPanel />
      {:else if active === "code"}
        <CodePanel />
      {/if}
    </div>
  </div>

  <nav class="rail" aria-label="Editor sections">
    {#each rail as item (item.id)}
      {@const Icon = item.icon}
      <button
        class="rail-tab"
        class:active={active === item.id}
        aria-label={item.label}
        aria-current={active === item.id}
        onclick={() => (active = item.id)}
      >
        <Icon size={ICON_SIZE} />
        <span class="rail-label">{item.label}</span>
      </button>
    {/each}
  </nav>
</aside>

<style>
  .sidebar-card {
    min-width: 0;
    min-height: 0;
    height: 100%;
    width: 100%;
    flex: 1;
    display: flex;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-md);
    box-shadow: var(--shadow-sm);
    overflow: hidden;
  }
  .content {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 12px;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .tab-header {
    flex: none;
    padding-bottom: 10px;
    margin-bottom: 4px;
  }
  .tab-title {
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: rgb(var(--fg-0));
  }
  .tab-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .rail {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    border-left: 1px solid rgb(var(--border-subtle));
    background: rgb(var(--bg-0));
    overflow-y: auto;
    scrollbar-width: none;
  }
  .rail::-webkit-scrollbar {
    display: none;
  }
  .rail-tab {
    position: relative;
    flex: none;
    width: 64px;
    padding: 8px 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: var(--r-xs);
    color: rgb(var(--fg-2));
    transition:
      color var(--motion) var(--ease),
      background-color var(--motion) var(--ease);
  }
  .rail-tab:hover {
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-1));
  }
  .rail-tab.active {
    color: rgb(var(--accent));
    background: rgb(var(--bg-3));
    box-shadow: var(--shadow-raised);
  }
  .rail-label {
    font-size: 11px;
    font-weight: 500;
    line-height: 1;
    letter-spacing: -0.02em;
    white-space: nowrap;
  }
</style>
