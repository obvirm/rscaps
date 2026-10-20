<script lang="ts">
  // Sheets (C7): daftar track gaya. Sheet default = dokumen; sheet tambahan
  // punya template + params sendiri. Klik item = jadikan target edit panel
  // (Style/Typography/Position/Motion/Effects/Layout menulis ke sheet aktif).
  import { Plus, Trash2, RotateCcw } from "@lucide/svelte";
  import { store } from "../../lib/state.svelte";

  let renamingId = $state<string | null>(null);
  let renameDraft = $state("");

  const startRename = (id: string, current: string) => {
    renamingId = id;
    renameDraft = current;
  };
  const commitRename = () => {
    if (renamingId && renameDraft.trim()) {
      store.renameSheet(renamingId, renameDraft.trim());
    }
    renamingId = null;
  };
</script>

<div class="sheets">
  <button class="add" onclick={() => store.addSheet()}>
    <Plus size={14} />
    <span>New sheet</span>
  </button>

  <div class="list">
    <!-- Sheet default (dokumen) -->
    <button
      class="sheet"
      class:active={store.activeSheetId === ""}
      onclick={() => store.setActiveSheet("")}
    >
      <span class="dot default"></span>
      <span class="name">Default</span>
      <span class="meta">{store.doc?.template || "v0"}</span>
      <span class="count">{store.cues.filter((c) => !c.sheet).length}</span>
    </button>

    {#each store.sheets as sheet (sheet.id)}
      <div class="sheet" class:active={store.activeSheetId === sheet.id}>
        <span class="dot"></span>
        {#if renamingId === sheet.id}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="rename"
            bind:value={renameDraft}
            autofocus
            onkeydown={(e) => e.key === "Enter" && commitRename()}
            onblur={commitRename}
          />
        {:else}
          <button class="name pick" onclick={() => store.setActiveSheet(sheet.id)} title="Edit sheet ini">
            {sheet.name}
          </button>
          <button
            class="icon-btn"
            onclick={() => startRename(sheet.id, sheet.name)}
            title="Rename"
          >
            <span class="edit-hint">✎</span>
          </button>
        {/if}
        <select
          class="tpl"
          value={sheet.template ?? ""}
          onchange={(e) => store.setSheetTemplate(sheet.id, e.currentTarget.value)}
          title="Template sheet"
        >
          <option value="">(v0)</option>
          {#each store.templates as t (t.id)}
            <option value={t.id}>{t.label}</option>
          {/each}
        </select>
        <select
          class="role"
          value={sheet.role ?? ""}
          onchange={(e) => store.setSheetRole(sheet.id, e.currentTarget.value)}
          title="Role sheet — kata di cue sheet ini ikut bertag (kel peak, dll)"
          aria-label="Sheet role"
        >
          <option value="">- role -</option>
          <option value="peak">Peak</option>
          <option value="hook">Hook</option>
        </select>
        <span class="count">{store.cues.filter((c) => c.sheet === sheet.id).length}</span>
        <button class="icon-btn" onclick={() => store.resetSheet(sheet.id)} title="Reset ke inheritance">
          <RotateCcw size={13} />
        </button>
        <button class="icon-btn danger" onclick={() => store.removeSheet(sheet.id)} title="Hapus sheet">
          <Trash2 size={13} />
        </button>
      </div>
    {/each}
  </div>

  <p class="hint">
    {#if store.activeSheet}
      Panel Style/Typography/Position/Motion/Effects/Layout sedang mengedit
      <strong>{store.activeSheet.name}</strong>. Cue ditempel ke sheet dari panel
      Transcript (kolom Sheet).
    {:else}
      Panel sedang mengedit sheet <strong>Default</strong> (dokumen). Tambah sheet
      untuk track gaya kedua — cue ditempel lewat panel Transcript.
    {/if}
  </p>
</div>

<style>
  .sheets {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .add {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 7px;
    border: 1px dashed rgb(var(--border));
    border-radius: var(--r-sm);
    background: none;
    color: rgb(var(--fg-2));
    font-size: 12px;
    cursor: pointer;
  }
  .add:hover {
    color: rgb(var(--accent));
    border-color: rgb(var(--accent));
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .sheet {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
    background: rgb(var(--bg-0));
    color: rgb(var(--fg-0));
    font-size: 12px;
    min-height: 32px;
    box-sizing: border-box;
  }
  button.sheet {
    cursor: pointer;
    width: 100%;
    text-align: left;
  }
  .sheet:hover {
    border-color: rgb(var(--accent) / 0.5);
  }
  .sheet.active {
    border-color: rgb(var(--accent));
    background: rgb(var(--accent) / 0.08);
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: rgb(var(--accent));
  }
  .dot.default {
    background: rgb(var(--fg-2));
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .pick {
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
    text-align: left;
  }
  .pick:hover {
    color: rgb(var(--accent));
  }
  .rename {
    flex: 1;
    min-width: 0;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--accent));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-0));
    font-size: 12px;
    padding: 2px 5px;
  }
  .meta {
    flex: 1;
    color: rgb(var(--fg-2));
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .tpl {
    flex: none;
    max-width: 110px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-1));
    font-size: 11px;
    padding: 2px 4px;
  }
  .role {
    flex: none;
    max-width: 74px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-1));
    font-size: 11px;
    padding: 2px 4px;
  }
  .count {
    flex: none;
    min-width: 18px;
    text-align: center;
    color: rgb(var(--fg-2));
    font-family: var(--font-mono);
    font-size: 10px;
    background: rgb(var(--bg-2));
    border-radius: 999px;
    padding: 1px 5px;
  }
  .icon-btn {
    flex: none;
    display: flex;
    align-items: center;
    background: none;
    border: none;
    color: rgb(var(--fg-2));
    cursor: pointer;
    padding: 2px;
    border-radius: var(--r-xs);
  }
  .icon-btn:hover {
    color: rgb(var(--fg-0));
    background: rgb(var(--bg-2));
  }
  .icon-btn.danger:hover {
    color: #e5484d;
  }
  .edit-hint {
    font-size: 12px;
    line-height: 1;
  }
  .hint {
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    color: rgb(var(--fg-2));
  }
  .hint strong {
    color: rgb(var(--fg-0));
  }
</style>
