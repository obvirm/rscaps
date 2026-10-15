<script lang="ts">
  import { onMount } from "svelte";
  import type { GallerySection as Section } from "../../lib/templateGallery";
  import type { TemplateEntry } from "../../lib/types";
  import TemplateCard from "./TemplateCard.svelte";

  let {
    section,
    previews,
    selectedId = "",
    favorites,
    expanded = false,
    onselect,
    onfavorite,
    onopen,
  }: {
    section: Section;
    previews: Record<string, string>;
    selectedId?: string;
    favorites: ReadonlySet<string>;
    expanded?: boolean;
    onselect: (t: TemplateEntry) => void;
    onfavorite: (id: string) => void;
    onopen: (id: string) => void;
  } = $props();

  let gridEl: HTMLElement;
  let cols = $state(1);

  onMount(() => {
    const update = () => {
      if (!gridEl) return;
      const tracks = getComputedStyle(gridEl)
        .gridTemplateColumns.split(" ")
        .filter((t) => t !== "").length;
      cols = Math.max(1, tracks);
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(gridEl);
    return () => ro.disconnect();
  });

  const shown = $derived(
    expanded
      ? section.templates
      : section.templates.slice(0, section.previewRows * cols),
  );
  const hasMore = $derived(shown.length < section.templates.length);
</script>

<section class="gallery-section">
  <header class="section-head">
    <h3 class="section-title">{section.label}</h3>
    {#if hasMore}
      <button class="view-all" onclick={() => onopen(section.id)}>
        View all {section.templates.length}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6" /></svg>
      </button>
    {/if}
  </header>

  <div class="grid" bind:this={gridEl}>
    {#each shown as t (t.id)}
      <TemplateCard
        template={t}
        preview={previews[t.id] ?? ""}
        selected={selectedId === t.id}
        favorite={favorites.has(t.id)}
        {onselect}
        {onfavorite}
      />
    {/each}
  </div>
</section>

<style>
  .gallery-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .section-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  .section-title {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: rgb(var(--fg-2));
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .view-all {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: rgb(var(--fg-faint));
    transition: color var(--motion) var(--ease);
  }
  .view-all:hover {
    color: rgb(var(--fg-1));
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(110px, 100%), 1fr));
    gap: 8px;
    align-items: start;
  }
</style>
