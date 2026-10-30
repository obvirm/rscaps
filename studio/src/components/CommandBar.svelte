<script lang="ts">
  import { onMount } from "svelte";
  import { ArrowLeft, Undo2, Redo2, Sun, Moon, Monitor } from "@lucide/svelte";
  import { store } from "../lib/state.svelte";
  import { cycleTheme, getMode, type Theme } from "../lib/theme";
  import ExportDialog from "./ExportDialog.svelte";
  import type { ExportProjectRef } from "../lib/types";

  let { onBack }: { onBack?: () => void } = $props();

  let fileInput: HTMLInputElement;
  let showExport = $state(false);

  // Theme mode mirrors the store-less theme module via a local tick.
  let themeMode = $state<Theme>(getMode());

  const exportProject = $derived<ExportProjectRef | null>(
    store.currentId
      ? {
          id: store.currentId,
          name: store.projectName,
          width: store.videoWidth,
          height: store.videoHeight,
          fps: store.fps,
        }
      : null,
  );

  // Project name: local draft, commit on blur/Enter, Esc reverts (tscaps).
  let nameDraft = $state(store.projectName);
  let nameFocused = $state(false);
  let lastName = $state(store.projectName);
  $effect(() => {
    if (!nameFocused && store.projectName !== lastName) {
      lastName = store.projectName;
      nameDraft = store.projectName;
    }
  });
  function commitName() {
    if (nameDraft !== store.projectName) store.projectName = nameDraft.trim() || "Untitled";
  }

  // Save status pill (tscaps ProjectSaveStatusPill).
  const savePill = $derived.by(() => {
    if (store.saveState === "saving") return { label: "Saving", tone: "info" };
    if (store.saveState === "error") return { label: "Save failed", tone: "danger" };
    if (store.dirty) return { label: "Unsaved changes", tone: "warning" };
    return { label: "Saved", tone: "subtle" };
  });

  function onThemeCycle() {
    themeMode = cycleTheme();
  }
  const ThemeIcon = $derived(themeMode === "light" ? Sun : themeMode === "dark" ? Moon : Monitor);

  async function onFileChange(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      await store.importSubtitleFile(file);
    } catch (err) {
      store.setStatus(String(err), true);
    } finally {
      input.value = "";
    }
  }

  async function onSave() {
    try {
      await store.saveCurrent();
      await store.refreshProjects();
    } catch (err) {
      store.setStatus(String(err), true);
    }
  }

  function onBackClick() {
    onBack?.();
  }

  // Shortcut undo/redo global; bidang teks memakai undo native-nya.
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName;
      const type = (target as HTMLInputElement | null)?.type;
      const textish =
        tag === "TEXTAREA" ||
        (tag === "INPUT" &&
          ["text", "search", "number", "email", "password", "tel", "url"].includes(type ?? "text"));
      if (textish) return;
      if (e.key === "z" && !e.shiftKey) {
        e.preventDefault();
        store.undo();
      } else if (e.key === "y" || (e.key === "z" && e.shiftKey)) {
        e.preventDefault();
        store.redo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<header class="toolbar">
  <div class="cluster">
    <button class="icon-btn" onclick={onBackClick} title="Back to dashboard" aria-label="Back to dashboard">
      <ArrowLeft size={16} />
    </button>
    <input
      class="name-input"
      aria-label="Project name"
      placeholder="Untitled"
      bind:value={nameDraft}
      onfocus={() => (nameFocused = true)}
      onblur={() => {
        nameFocused = false;
        commitName();
      }}
      onkeydown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") {
          nameDraft = store.projectName;
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  </div>

  <div class="cluster">
    <button
      class="icon-btn"
      disabled={!store.canUndo}
      onclick={() => store.undo()}
      title="Undo (Ctrl+Z)"
      aria-label="Undo"
    >
      <Undo2 size={16} />
    </button>
    <button
      class="icon-btn"
      disabled={!store.canRedo}
      onclick={() => store.redo()}
      title="Redo (Ctrl+Y)"
      aria-label="Redo"
    >
      <Redo2 size={16} />
    </button>
  </div>

  <div class="spacer"></div>

  <div class="desktop-only pill-zone">
    <span class="pill {savePill.tone}">{savePill.label}</span>
  </div>

  <div class="desktop-only cluster">
    <button
      class="icon-btn"
      onclick={onThemeCycle}
      title="Theme: {themeMode} (click to change)"
      aria-label="Theme: {themeMode}. Click to change."
    >
      <ThemeIcon size={16} />
    </button>
  </div>

  <div class="cluster import-group desktop-only">
    <button
      class="btn"
      onclick={() => fileInput.click()}
      title="Import subtitles — supports .srt and .vtt">Import subtitle</button
    >
    <button
      class="btn"
      onclick={() => void store.transcribeCurrent()}
      disabled={store.transcribing}
      title="Transcribe the video again (replaces current captions)">Resub</button
    >
  </div>

  <button
    class="save-btn"
    onclick={onSave}
    disabled={!store.dirty || store.saveState === "saving"}
    aria-label="Save project"
  >
    {store.saveState === "saving" ? "Saving…" : "Save"}
  </button>

  <button
    class="btn accent"
    onclick={() => (showExport = true)}
    disabled={!store.currentId}
    title="Export video with burned-in captions">Export</button
  >

  <input
    bind:this={fileInput}
    type="file"
    accept=".srt,.vtt,application/x-subrip,text/vtt"
    hidden
    onchange={onFileChange}
  />
</header>

{#if showExport && exportProject}
  <ExportDialog project={exportProject} onclose={() => (showExport = false)} />
{/if}

<style>
  .toolbar {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 4px 10px;
    border-bottom: 1px solid rgb(var(--border-subtle));
    flex-shrink: 0;
    margin-bottom: 12px;
  }
  .cluster {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }
  .spacer {
    flex: 1;
  }
  .desktop-only {
    display: flex;
  }
  @media (max-width: 1023px) {
    .desktop-only {
      display: none;
    }
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: 1px solid transparent;
    border-radius: var(--r-xs);
    color: rgb(var(--fg-1));
    transition:
      color var(--motion) var(--ease),
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .icon-btn:not(:disabled):hover {
    background: rgb(var(--bg-2));
    border-color: rgb(var(--border));
    color: rgb(var(--fg-0));
  }
  .icon-btn:focus-visible {
    border-color: rgb(var(--accent));
  }
  .name-input {
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-xs);
    color: rgb(var(--fg-0));
    font-size: 15px;
    font-weight: 500;
    padding: 6px 8px;
    min-width: 120px;
    max-width: 320px;
    transition:
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .name-input:hover {
    background: rgb(var(--bg-2));
    border-color: rgb(var(--border));
  }
  .name-input:focus {
    background: rgb(var(--bg-2));
    border-color: rgb(var(--accent));
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 9px;
    border-radius: var(--r-pill);
    font-size: 11px;
    font-weight: 500;
    border: 1px solid rgb(var(--border));
    background: rgb(var(--bg-1));
    color: rgb(var(--fg-2));
    white-space: nowrap;
  }
  .pill::before {
    content: "";
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.85;
  }
  .pill.info {
    color: rgb(var(--accent-hi));
    border-color: rgb(var(--accent) / 0.4);
  }
  .pill.warning {
    color: rgb(var(--warning));
    border-color: rgb(var(--warning) / 0.4);
  }
  .pill.danger {
    color: rgb(var(--error));
    border-color: rgb(var(--error) / 0.4);
  }
  .pill.subtle {
    color: rgb(var(--fg-2));
  }
  .import-group {
    gap: 4px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 11px;
    font-size: 13px;
    font-weight: 500;
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    transition:
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .btn:not(:disabled):hover {
    background: rgb(var(--bg-2));
    border-color: rgb(var(--border-strong));
  }
  .btn.accent {
    background: rgb(var(--accent));
    border-color: rgb(var(--accent));
    color: rgb(var(--fg-on-accent));
  }
  .btn.accent:not(:disabled):hover {
    background: rgb(var(--accent-hi));
    border-color: rgb(var(--accent-hi));
  }
  .save-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 12px;
    border-radius: var(--r-xs);
    border: 1px solid rgb(var(--border));
    font-size: 13px;
    font-weight: 500;
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-2));
    transition:
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .save-btn:not(:disabled):hover {
    background: rgb(var(--bg-3));
    border-color: rgb(var(--accent));
  }
</style>
