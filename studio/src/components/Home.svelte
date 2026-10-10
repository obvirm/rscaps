<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "../lib/state.svelte";
  import { router } from "../lib/router.svelte";
  import { toggleTheme } from "../lib/theme";
  import { formatTimeShort, type ProjectMetadata } from "../lib/types";
  import { probeVideo, type VideoProbe } from "../lib/video";
  import NewProjectDialog from "./NewProjectDialog.svelte";
  import ExportDialog from "./ExportDialog.svelte";

  let showDialog = $state(false);
  let fileInput: HTMLInputElement;
  let pendingVideo = $state<File | null>(null);
  let pendingProbe = $state<VideoProbe | null>(null);
  let probing = $state(false);
  let probeError = $state<string | null>(null);
  let openMenuId = $state<string | null>(null);
  let exportProject = $state<ProjectMetadata | null>(null);

  onMount(() => {
    store.refreshProjects();
  });

  function closeMenu() {
    openMenuId = null;
  }

  function toggleMenu(e: MouseEvent, id: string) {
    e.stopPropagation();
    openMenuId = openMenuId === id ? null : id;
  }

  function onExport(id: string) {
    closeMenu();
    const project = store.projects.find((p) => p.id === id);
    if (project) exportProject = project;
  }

  function pickVideo() {
    fileInput?.click();
  }

  async function onFileChosen(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    probing = true;
    probeError = null;
    try {
      const probe = await probeVideo(file);
      pendingVideo = file;
      pendingProbe = probe;
      showDialog = true;
    } catch (err) {
      probeError = `Could not read video — ${err}`;
      store.setStatus(probeError, true);
    } finally {
      probing = false;
    }
  }

  function relativeDate(iso: string): string {
    const then = new Date(iso).getTime();
    const diff = Date.now() - then;
    const m = Math.floor(diff / 60000);
    if (m < 1) return "just now";
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    const d = Math.floor(h / 24);
    if (d < 7) return `${d}d ago`;
    return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
  }

  async function onDelete(e: MouseEvent, id: string, name: string) {
    e.stopPropagation();
    closeMenu();
    if (!confirm(`Delete “${name}”? This cannot be undone.`)) return;
    await store.deleteProjectById(id);
  }

  function open(id: string) {
    router.openProject(id);
  }
</script>

<svelte:window onclick={closeMenu} />

<div class="home">
  <header class="chrome">
    <div class="chrome-inner">
      <span class="wordmark"><span class="r">rs</span><span class="c">caps</span><span class="dot">.</span></span>
      <div class="utility">
        <button class="icon-btn" onclick={toggleTheme} title="Toggle theme" aria-label="Toggle theme">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><circle cx="8" cy="8" r="5.5" /><path d="M8 2.5a5.5 5.5 0 0 1 0 11z" fill="currentColor" stroke="none" /></svg>
        </button>
      </div>
    </div>
  </header>

  <div class="shell">
    <main class="main">
      <div class="page-header">
        <div>
          <h1 class="page-title">Local projects</h1>
          {#if store.projects.length > 0}
            <p class="page-sub">
              {store.projects.length === 1
                ? "1 project · stored in this browser"
                : `${store.projects.length} projects · stored in this browser`}
            </p>
          {/if}
        </div>
        <div class="header-actions">
          <button class="btn primary" onclick={pickVideo} disabled={probing}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M7 2.5v9M2.5 7h9" /></svg>
            <span>{probing ? "Reading video…" : "New project"}</span>
          </button>
        </div>
      </div>

      {#if probeError}
        <div class="notice error">{probeError}</div>
      {/if}

      {#if store.projectsLoading}
        <div class="empty">
          <p class="empty-sub">Loading projects…</p>
        </div>
      {:else if store.projects.length > 0}
        <div class="grid">
          {#each store.projects as p (p.id)}
            <div
              class="card"
              role="button"
              tabindex="0"
              onclick={() => open(p.id)}
              onkeydown={(e) => e.key === "Enter" && open(p.id)}
            >
              <div class="thumb">
                {#if p.thumbnail}
                  <img src={p.thumbnail} alt="" />
                {:else}
                  <span class="no-preview">No preview</span>
                {/if}
                {#if p.durationSec > 0}
                  <span class="duration">{formatTimeShort(p.durationSec)}</span>
                {/if}
              </div>
              <div class="card-body">
                <div class="card-name" title={p.name}>{p.name}</div>
                <div class="card-meta">
                  <span class="card-date">{relativeDate(p.updatedAt)}</span>
                  <div class="menu-anchor">
                    <button
                      class="dots"
                      title="Project menu"
                      aria-label="Project menu"
                      aria-haspopup="menu"
                      aria-expanded={openMenuId === p.id}
                      onclick={(e) => toggleMenu(e, p.id)}
                    >
                      <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor"><circle cx="7" cy="3" r="1.3" /><circle cx="7" cy="7" r="1.3" /><circle cx="7" cy="11" r="1.3" /></svg>
                    </button>
                    {#if openMenuId === p.id}
                      <!-- svelte-ignore a11y_no_static_element_interactions -->
                      <div class="menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()}>
                        <button class="menu-item" role="menuitem" onclick={() => onExport(p.id)}>
                          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M7 1.5v7M4 5.5 7 8.5l3-3" /><path d="M2 9.5v2a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1v-2" /></svg>
                          <span>Export</span>
                        </button>
                        <button class="menu-item danger" role="menuitem" onclick={(e) => onDelete(e, p.id, p.name)}>
                          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 3.5h9M5.5 3.5V2.5a1 1 0 0 1 1-1h1a1 1 0 0 1 1 1v1M4 3.5l.5 7a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1l.5-7" /></svg>
                          <span>Delete</span>
                        </button>
                      </div>
                    {/if}
                  </div>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="empty">
          <h2 class="empty-title">Make your first project.</h2>
          <p class="empty-sub">
            Create a project to design captions with fframes. Everything stays in this browser.
          </p>
          <button class="btn primary big" onclick={pickVideo} disabled={probing}>
            <svg width="15" height="15" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M7 2.5v9M2.5 7h9" /></svg>
            <span>{probing ? "Reading video…" : "New project"}</span>
          </button>
        </div>
      {/if}
    </main>
  </div>
</div>

<input
  class="visually-hidden"
  type="file"
  accept="video/*"
  bind:this={fileInput}
  onchange={onFileChosen}
/>

{#if showDialog && pendingVideo && pendingProbe}
  <NewProjectDialog video={pendingVideo} probe={pendingProbe} onclose={() => (showDialog = false)} />
{/if}

{#if exportProject}
  <ExportDialog project={exportProject} onclose={() => (exportProject = null)} />
{/if}

<style>
  .home {
    height: 100dvh;
    display: flex;
    flex-direction: column;
    background: rgb(var(--bg-0));
    overflow-y: auto;
  }
  .chrome {
    position: sticky;
    top: 0;
    z-index: 30;
    background: rgb(var(--bg-0) / 0.85);
    backdrop-filter: blur(8px);
    border-bottom: 1px solid rgb(var(--border));
    flex: none;
  }
  .chrome-inner {
    max-width: 1120px;
    margin: 0 auto;
    width: 100%;
    height: 60px;
    padding: 0 20px;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .wordmark {
    font-weight: 700;
    font-size: 18px;
    letter-spacing: -0.02em;
    user-select: none;
  }
  .wordmark .r {
    color: rgb(var(--fg-0));
  }
  .wordmark .c {
    color: rgb(var(--accent));
  }
  .wordmark .dot {
    color: rgb(var(--accent));
  }
  .utility {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: var(--r-sm);
    color: rgb(var(--fg-1));
    transition: color var(--motion) var(--ease), background-color var(--motion) var(--ease);
  }
  .icon-btn:hover {
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-0));
  }
  .shell {
    flex: 1;
    max-width: 1120px;
    margin: 0 auto;
    width: 100%;
    padding: 40px 20px 56px;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 28px;
  }
  .page-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }
  .page-title {
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.022em;
    color: rgb(var(--fg-0));
  }
  .page-sub {
    font-size: 13px;
    color: rgb(var(--fg-1));
    margin-top: 4px;
  }
  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 9px 15px;
    font-size: 13.5px;
    font-weight: 500;
    border-radius: var(--r-sm);
    border: 1px solid transparent;
    transition: background-color var(--motion) var(--ease), border-color var(--motion) var(--ease);
  }
  .btn.primary {
    color: rgb(var(--fg-on-accent));
    background: rgb(var(--accent));
    border-color: rgb(var(--accent));
  }
  .btn.primary:hover {
    background: rgb(var(--accent-hi));
    border-color: rgb(var(--accent-hi));
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .notice {
    padding: 9px 12px;
    font-size: 12.5px;
    line-height: 1.5;
    border-radius: var(--r-sm);
    border: 1px solid rgb(var(--border-strong));
  }
  .notice.error {
    color: rgb(var(--error));
    border-color: rgb(var(--error) / 0.5);
    background: rgb(var(--error) / 0.08);
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
  .btn.big {
    padding: 11px 20px;
    font-size: 14px;
    margin-top: 8px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 16px;
  }
  .card {
    display: flex;
    flex-direction: column;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-md);
    overflow: hidden;
    cursor: pointer;
    transition: border-color var(--motion) var(--ease), transform var(--motion) var(--ease);
  }
  .card:hover {
    border-color: rgb(var(--border-strong));
  }
  .thumb {
    position: relative;
    aspect-ratio: 16 / 9;
    background: #000;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .no-preview {
    font-family: var(--font-mono);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: rgb(var(--fg-2));
  }
  .duration {
    position: absolute;
    bottom: 6px;
    right: 6px;
    font-family: var(--font-mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: #fff;
    background: rgb(0 0 0 / 0.7);
    backdrop-filter: blur(4px);
    padding: 2px 6px;
    border-radius: 4px;
  }
  .card-body {
    padding: 10px 12px 11px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .card-name {
    font-size: 13.5px;
    font-weight: 500;
    color: rgb(var(--fg-0));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .card-meta {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .card-date {
    flex: 1;
    font-size: 11px;
    color: rgb(var(--fg-2));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .menu-anchor {
    position: relative;
    display: inline-flex;
  }
  .dots {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    color: rgb(var(--fg-2));
    opacity: 0;
    transition: opacity var(--motion) var(--ease), color var(--motion) var(--ease), background-color var(--motion) var(--ease);
  }
  .card:hover .dots,
  .dots[aria-expanded="true"] {
    opacity: 1;
  }
  .dots:hover {
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-2));
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 20;
    min-width: 148px;
    padding: 4px;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border-strong));
    border-radius: var(--r-md);
    box-shadow: var(--shadow-md);
    display: flex;
    flex-direction: column;
  }
  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 9px;
    font-size: 12.5px;
    color: rgb(var(--fg-0));
    border-radius: var(--r-sm);
    text-align: left;
    transition: background-color var(--motion) var(--ease);
  }
  .menu-item:hover {
    background: rgb(var(--bg-2));
  }
  .menu-item.danger {
    color: rgb(var(--error));
  }
  .menu-item.danger:hover {
    background: rgb(var(--error) / 0.12);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 80px 0 96px;
    gap: 12px;
  }
  .empty-title {
    font-size: 26px;
    font-weight: 600;
    letter-spacing: -0.022em;
    color: rgb(var(--fg-0));
  }
  .empty-sub {
    font-size: 13.5px;
    color: rgb(var(--fg-1));
    max-width: 42ch;
    line-height: 1.6;
  }
</style>
