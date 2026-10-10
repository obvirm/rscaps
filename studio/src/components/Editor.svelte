<script lang="ts">
  import { onMount } from "svelte";
  import CommandBar from "./CommandBar.svelte";
  import IconRail from "./IconRail.svelte";
  import PreviewStage from "./PreviewStage.svelte";
  import Inspector from "./Inspector.svelte";
  import { store } from "../lib/state.svelte";
  import { router } from "../lib/router.svelte";

  let { projectId }: { projectId: string } = $props();

  let activeRail = $state("templates");
  let inspectorWidth = $state(320);
  let dragging = $state(false);
  let workspaceEl: HTMLElement;
  let loaded = $state(false);

  function onStartDrag(e: PointerEvent) {
    dragging = true;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onDrag(e: PointerEvent) {
    if (!dragging) return;
    const rect = workspaceEl.getBoundingClientRect();
    inspectorWidth = Math.min(Math.max(rect.right - e.clientX, 260), 600);
  }
  function onEndDrag() {
    dragging = false;
  }

  async function onBack() {
    await store.saveCurrent();
    await store.refreshProjects();
    router.goHome();
  }

  onMount(() => {
    let cancelled = false;
    store.openProject(projectId).then(() => {
      if (!cancelled) loaded = true;
    });

    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      const tag = el?.tagName?.toLowerCase();
      if (tag === "input" || tag === "textarea" || el?.isContentEditable) return;
      if (e.code === "Space") {
        e.preventDefault();
        store.togglePlay();
      } else if (e.code === "ArrowLeft") {
        e.preventDefault();
        store.stepFrame(-(e.shiftKey ? 10 : 1));
      } else if (e.code === "ArrowRight") {
        e.preventDefault();
        store.stepFrame(e.shiftKey ? 10 : 1);
      }
    };
    window.addEventListener("keydown", onKey);

    const onBeforeUnload = () => store.saveCurrent();
    window.addEventListener("beforeunload", onBeforeUnload);

    return () => {
      cancelled = true;
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("beforeunload", onBeforeUnload);
    };
  });

  // Autosave when the editable document text or project name changes.
  $effect(() => {
    void store.docText;
    void store.projectName;
    if (loaded) store.scheduleSave();
  });
</script>

<div class="app">
  <CommandBar {onBack} />

  {#if !loaded}
    <div class="loading">Opening project…</div>
  {:else}
    <div
      class="workspace"
      bind:this={workspaceEl}
      class:dragging
      style={`--inspector-w: ${inspectorWidth}px`}
    >
      <IconRail bind:active={activeRail} />

      <main class="center-col">
        <PreviewStage />
      </main>

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="resize-handle"
        class:dragging
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize inspector"
        onpointerdown={onStartDrag}
        onpointermove={onDrag}
        onpointerup={onEndDrag}
        onpointercancel={onEndDrag}
        title="Drag to resize"
      >
        <span></span>
      </div>

      <Inspector {activeRail} />
    </div>
  {/if}
</div>

<style>
  .app {
    height: 100dvh;
    display: flex;
    flex-direction: column;
  }
  .loading {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgb(var(--fg-2));
    font-family: var(--font-mono);
    font-size: 13px;
  }
  .workspace {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 56px minmax(0, 1fr) 12px var(--inspector-w, 320px);
    padding: 10px 12px 12px;
    gap: 0;
  }
  .center-col {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 12px 0 10px;
  }
  .resize-handle {
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: col-resize;
    touch-action: none;
  }
  .resize-handle span {
    width: 2px;
    height: 44px;
    border-radius: 999px;
    background: rgb(var(--border));
    transition: background-color 150ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .resize-handle:hover span,
  .resize-handle.dragging span {
    background: rgb(var(--accent));
  }
  @media (max-width: 900px) {
    .workspace {
      grid-template-columns: 48px minmax(0, 1fr);
    }
    .resize-handle,
    :global(aside.inspector) {
      display: none;
    }
  }
</style>
