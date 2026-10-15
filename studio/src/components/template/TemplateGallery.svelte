<script lang="ts">
  import { onMount } from "svelte";
  import { ChevronLeft, Search } from "@lucide/svelte";
  import { store } from "../../lib/state.svelte";
  import {
    buildSections,
    loadFavorites,
    saveFavorites,
    searchTemplates,
    isCategoryId,
    type GallerySection,
  } from "../../lib/templateGallery";
  import { cropPreviewSvg } from "../../lib/templateGallery";
  import type { TemplateEntry } from "../../lib/types";
  import GallerySectionView from "./GallerySection.svelte";

  let query = $state("");
  let openedId = $state<string | null>(null);
  let favorites = $state<Set<string>>(new Set());

  onMount(() => {
    favorites = loadFavorites();
    void store.bootWasm().then(() => store.buildTemplatePreviews());
    return () => store.buildTemplatePreviews(); // no-op aman; restore di dalam
  });

  const cropped = $derived.by(() => {
    const map: Record<string, string> = {};
    for (const [id, svg] of Object.entries(store.previews)) {
      map[id] = cropPreviewSvg(svg, 2);
    }
    return map;
  });

  const selectedId = $derived(store.doc?.template ?? "");
  const sections = $derived(buildSections(store.templates, favorites));
  const results = $derived(
    query.trim() ? searchTemplates(store.templates, favorites, query) : [],
  );
  const openedSection = $derived<GallerySection | null>(
    openedId && !query.trim()
      ? (sections.find((s) => s.id === openedId) ?? null)
      : null,
  );

  function toggleFavorite(id: string) {
    const next = new Set(favorites);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    favorites = next;
    saveFavorites(next);
  }

  async function select(t: TemplateEntry) {
    // Reset style values ke default template baru (perilaku tscaps).
    await store.applyTemplateFresh(t.id);
  }

  function openSection(id: string) {
    openedId = id;
  }
  function closeSection() {
    openedId = null;
  }
</script>

<div class="gallery">
  <div class="search">
    <Search size={13} />
    <input
      type="text"
      placeholder="Search templates"
      bind:value={query}
      aria-label="Search templates"
    />
  </div>

  <div class="body thin-scroll">
    {#if query.trim()}
      {#if results.length === 0}
        <p class="empty">No templates match.</p>
      {:else}
        <GallerySectionView
          section={{
            id: "results",
            label: "Results",
            templates: results,
            previewRows: 999,
          }}
          previews={cropped}
          {selectedId}
          {favorites}
          expanded
          onselect={select}
          onfavorite={toggleFavorite}
          onopen={openSection}
        />
      {/if}
    {:else if openedSection}
      <button class="back" onclick={closeSection}>
        <ChevronLeft size={12} strokeWidth={2.5} />
        <span>All templates</span>
      </button>
      <GallerySectionView
        section={openedSection}
        previews={cropped}
        {selectedId}
        {favorites}
        expanded
        onselect={select}
        onfavorite={toggleFavorite}
        onopen={openSection}
      />
    {:else}
      {#each sections as section, i (section.id)}
        <div class="section-slot" class:divided={i > 0}>
          <GallerySectionView
            {section}
            previews={cropped}
            {selectedId}
            {favorites}
            onselect={select}
            onfavorite={toggleFavorite}
            onopen={openSection}
          />
        </div>
      {/each}
      {#if sections.length === 0}
        <p class="empty">Loading templates…</p>
      {/if}
    {/if}
  </div>
</div>

<style>
  .gallery {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-height: 0;
    flex: 1;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 10px;
    flex: none;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-faint));
    transition:
      border-color var(--motion) var(--ease),
      background-color var(--motion) var(--ease);
  }
  .search:focus-within {
    border-color: rgb(var(--accent));
    background: rgb(var(--bg-1));
    box-shadow: 0 0 0 2px rgb(var(--accent) / 0.3);
    color: rgb(var(--fg-2));
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    color: rgb(var(--fg-1));
    font-size: 13px;
  }
  .search input::placeholder {
    color: rgb(var(--fg-faint));
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-right: 4px;
  }
  .section-slot + .section-slot {
    border-top: 1px solid rgb(var(--border-subtle));
    padding-top: 12px;
  }
  .section-slot:not(:last-child) {
    padding-bottom: 12px;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    align-self: flex-start;
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
  .back:hover {
    color: rgb(var(--fg-0));
  }
  .empty {
    padding: 24px 8px;
    text-align: center;
    font-size: 13px;
    color: rgb(var(--fg-2));
  }
</style>
