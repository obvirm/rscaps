<script lang="ts">
  import { Plus, Search, Trash2, LocateFixed } from "@lucide/svelte";
  import { store } from "../../lib/state.svelte";
  import { formatTime } from "../../lib/types";
  import type { WordInfo, Cue } from "../../lib/types";

  let query = $state("");
  let view = $state<"cue" | "word">("cue");
  let editingWord = $state<number | null>(null);

  const cues = $derived(store.cues);
  const words = $derived(store.words);
  const nowMs = $derived((store.frame / store.fps) * 1000);

  // Scene = jeda antar-cue > SCENE_GAP_MS. Tanpa analisis video - kelompok
  // narasi yang dipisah hening dianggap satu adegan.
  const SCENE_GAP_MS = 1500;

  interface WordCue {
    cueIndex: number;
    startMs: number;
    endMs: number;
    words: WordInfo[];
  }
  interface WordScene {
    startMs: number;
    endMs: number;
    cues: WordCue[];
  }

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return cues
      .map((cue, i) => ({ cue, i }))
      .filter(({ cue }) => q === "" || cue.text.toLowerCase().includes(q));
  });

  const activeIndex = $derived(cues.findIndex((c) => nowMs >= c.start_ms && nowMs < c.end_ms));
  const activeWordIndex = $derived(words.findIndex((w) => nowMs >= w.start_ms && nowMs < w.end_ms));

  // Kata dikelompokkan per cue, cue per scene (jeda > SCENE_GAP_MS).
  const wordGroups = $derived.by(() => {
    const byCue: WordCue[] = [];
    let cur: WordCue | null = null;
    for (const w of words) {
      const cueIdx = cues.findIndex(
        (c) => w.start_ms >= c.start_ms - 1 && w.start_ms < c.end_ms + 1,
      );
      if (!cur || cur.cueIndex !== cueIdx) {
        const cue: Cue | undefined = cues[cueIdx];
        cur = {
          cueIndex: cueIdx,
          startMs: cue ? cue.start_ms : w.start_ms,
          endMs: cue ? cue.end_ms : w.end_ms,
          words: [],
        };
        byCue.push(cur);
      }
      cur.words.push(w);
    }
    const scenes: WordScene[] = [];
    for (const g of byCue) {
      const last = scenes.at(-1);
      if (!last || g.startMs - last.endMs > SCENE_GAP_MS) {
        scenes.push({ startMs: g.startMs, endMs: g.endMs, cues: [g] });
      } else {
        last.endMs = g.endMs;
        last.cues.push(g);
      }
    }
    const q = query.trim().toLowerCase();
    if (q === "") return scenes;
    return scenes
      .map((s) => ({
        ...s,
        cues: s.cues
          .map((g) => ({ ...g, words: g.words.filter((w) => w.text.toLowerCase().includes(q)) }))
          .filter((g) => g.words.length > 0),
      }))
      .filter((s) => s.cues.length > 0);
  });

  $effect(() => {
    // Muat daftar kata (cache store invalidasi sendiri saat dokumen berubah).
    if (view === "word") void store.listWords();
  });

  function parseTime(text: string): number | null {
    const m = text.trim().match(/^(\d+):(\d{1,2})(?:\.(\d{1,3}))?$/);
    if (!m) {
      const n = Number(text);
      return Number.isFinite(n) ? Math.round(n * 1000) : null;
    }
    const min = Number(m[1]);
    const sec = Number(m[2]);
    const cs = m[3] ? Number(m[3].padEnd(3, "0")) : 0;
    return min * 60000 + sec * 1000 + cs;
  }

  function timeLabel(ms: number): string {
    return formatTime(ms / 1000).slice(0, 8); // mm:ss.cs
  }

  function onTimeChange(index: number, field: "start_ms" | "end_ms", text: string) {
    const ms = parseTime(text);
    if (ms === null) return;
    void store.editCue(index, { [field]: Math.max(0, ms) });
  }

  function onTextChange(index: number, text: string) {
    void store.editCue(index, { text });
  }

  function locate(index: number) {
    store.seekTo(Math.round((cues[index].start_ms / 1000) * store.fps));
  }

  function seekMs(ms: number) {
    store.seekTo(Math.round((ms / 1000) * store.fps));
  }

  // Edit satu kata = tulis balik token di teks cue sumber (kata didefinisikan
  // tokenize(cue.text)); kata duplikat dalam satu cue dicocokkan lewat
  // occurrence ke-N berdasarkan urutan index global.
  function commitWordEdit(w: WordInfo, text: string) {
    editingWord = null;
    const next = text.trim();
    const cueIdx = cues.findIndex(
      (c) => w.start_ms >= c.start_ms - 1 && w.start_ms < c.end_ms + 1,
    );
    if (cueIdx < 0 || next === w.text) return;
    const cue = cues[cueIdx];
    const sameCue = words.filter(
      (x) =>
        x.text === w.text &&
        x.start_ms >= cue.start_ms - 1 &&
        x.start_ms < cue.end_ms + 1,
    );
    const occurrence = sameCue.findIndex((x) => x.index === w.index);
    const tokens = cue.text.split(/(\s+)/); // pertahankan whitespace
    const tokenIdx: number[] = [];
    tokens.forEach((t, i) => {
      if (t.trim() !== "") tokenIdx.push(i);
    });
    let hit = -1;
    let n = 0;
    for (const ti of tokenIdx) {
      if (tokens[ti] === w.text) {
        if (n === occurrence) {
          hit = ti;
          break;
        }
        n++;
      }
    }
    if (hit < 0) return;
    tokens[hit] = next;
    void store.editCue(cueIdx, { text: tokens.join("") });
  }
</script>

<div class="transcript-panel">
  <div class="topbar">
    <div class="search">
      <Search size={13} />
      <input type="text" placeholder="Search." bind:value={query} aria-label="Search captions" />
    </div>
    <span class="count">{view === "cue" ? cues.length : words.length}</span>
    <div class="view-toggle" role="tablist" aria-label="Transcript view">
      <button
        class:selected={view === "cue"}
        onclick={() => (view = "cue")}
        role="tab"
        aria-selected={view === "cue"}
        title="Cue view"
      >Cue</button>
      <button
        class:selected={view === "word"}
        onclick={() => (view = "word")}
        role="tab"
        aria-selected={view === "word"}
        title="Word view - click a word to seek, double-click to edit"
      >Word</button>
    </div>
    <button class="add-btn" onclick={() => store.addCue(cues.length - 1)} title="Add caption">
      <Plus size={14} />
    </button>
  </div>

  {#if view === "cue"}
    <div class="list thin-scroll">
      {#each filtered as { cue, i } (i)}
        <div class="cue" class:active={i === activeIndex}>
          <div class="cue-head">
            <button class="locate" onclick={() => locate(i)} title="Seek to this caption" aria-label="Seek to caption {i + 1}">
              <LocateFixed size={12} />
            </button>
            <input
              class="time"
              value={timeLabel(cue.start_ms)}
              aria-label="Start time"
              onchange={(e) => onTimeChange(i, "start_ms", (e.currentTarget as HTMLInputElement).value)}
            />
            <span class="dash">-</span>
            <input
              class="time"
              value={timeLabel(cue.end_ms)}
              aria-label="End time"
              onchange={(e) => onTimeChange(i, "end_ms", (e.currentTarget as HTMLInputElement).value)}
            />
            <span class="spacer"></span>
            {#if store.sheets.length > 0}
              <select
                class="sheet-pick"
                value={cue.sheet ?? ""}
                onchange={(e) => store.assignCue(i, e.currentTarget.value)}
                title="Sheet cue ini"
                aria-label="Sheet cue {i + 1}"
              >
                <option value="">Default</option>
                {#each store.sheets as s (s.id)}
                  <option value={s.id}>{s.name}</option>
                {/each}
              </select>
            {/if}
            <button class="del" onclick={() => store.removeCue(i)} title="Delete caption" aria-label="Delete caption {i + 1}">
              <Trash2 size={12} />
            </button>
          </div>
          <textarea
            rows="2"
            value={cue.text}
            spellcheck="false"
            aria-label="Caption text"
            oninput={(e) => onTextChange(i, (e.currentTarget as HTMLTextAreaElement).value)}
          ></textarea>
        </div>
      {/each}
      {#if filtered.length === 0}
        <p class="empty">No captions {query ? `matching "${query}"` : "- import an .srt or transcribe"}</p>
      {/if}
    </div>
  {:else}
    <div class="list thin-scroll">
      {#each wordGroups as scene, si (si)}
        <div class="scene">
          <div class="scene-head">
            <span class="scene-name">Scene {si + 1}</span>
            <span class="scene-time">{timeLabel(scene.startMs)}</span>
            <button
              class="locate"
              onclick={() => seekMs(scene.startMs)}
              title="Seek to scene {si + 1}"
              aria-label="Seek to scene {si + 1}"
            >
              <LocateFixed size={12} />
            </button>
          </div>
          {#each scene.cues as group (group.cueIndex)}
            <div class="word-cue" class:active={group.cueIndex === activeIndex}>
              {#each group.words as w (w.index)}
                {#if editingWord === w.index}
                  <input
                    class="word-edit"
                    value={w.text}
                    spellcheck="false"
                    aria-label="Edit word"
                    onblur={(e) => commitWordEdit(w, (e.currentTarget as HTMLInputElement).value)}
                    onkeydown={(e) => {
                      if (e.key === "Enter") commitWordEdit(w, (e.currentTarget as HTMLInputElement).value);
                      if (e.key === "Escape") editingWord = null;
                    }}
                  />
                {:else}
                  <button
                    class="word"
                    class:active={w.index === activeWordIndex}
                    class:editing={editingWord === w.index}
                    onclick={() => seekMs(w.start_ms)}
                    ondblclick={() => (editingWord = w.index)}
                    title="{timeLabel(w.start_ms)} - click: seek, double-click: edit"
                  >{w.text}</button>
                {/if}
              {/each}
            </div>
          {/each}
        </div>
      {/each}
      {#if wordGroups.length === 0}
        <p class="empty">No words {query ? `matching "${query}"` : "- import an .srt or transcribe"}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .transcript-panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    gap: 10px;
  }
  .topbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .search {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 8px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-2));
  }
  .search:focus-within {
    border-color: rgb(var(--accent));
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    color: rgb(var(--fg-0));
    font-size: 13px;
  }
  .search input:focus {
    outline: none;
  }
  .count {
    font: 11px var(--font-mono);
    color: rgb(var(--fg-2));
  }
  .view-toggle {
    display: inline-flex;
    height: 28px;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    overflow: hidden;
    background: rgb(var(--bg-2));
  }
  .view-toggle button {
    padding: 0 10px;
    font-size: 11.5px;
    color: rgb(var(--fg-2));
    transition:
      background-color var(--motion) var(--ease),
      color var(--motion) var(--ease);
  }
  .view-toggle button:hover {
    color: rgb(var(--fg-0));
  }
  .view-toggle button.selected {
    background: rgb(var(--accent) / 0.16);
    color: rgb(var(--accent));
  }
  .add-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-1));
    transition:
      background-color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .add-btn:hover {
    background: rgb(var(--bg-3));
    color: rgb(var(--fg-0));
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-right: 4px;
  }
  .cue {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
    transition: border-color var(--motion) var(--ease);
  }
  .cue:hover {
    border-color: rgb(var(--border-strong));
  }
  .cue.active {
    border-color: rgb(var(--accent) / 0.6);
  }
  .cue-head {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .locate,
  .del {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: var(--r-xs);
    color: rgb(var(--fg-2));
    transition: color var(--motion) var(--ease);
  }
  .locate:hover {
    color: rgb(var(--accent));
  }
  .sheet-pick {
    max-width: 84px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-xs);
    color: rgb(var(--fg-1));
    font-size: 10px;
    padding: 1px 3px;
  }
  .del:hover {
    color: rgb(var(--error));
  }
  .time {
    width: 7ch;
    background: transparent;
    border: none;
    border-bottom: 1px solid transparent;
    font: 11px var(--font-mono);
    color: rgb(var(--fg-1));
    text-align: center;
    padding: 1px 0;
  }
  .time:hover {
    border-bottom-color: rgb(var(--fg-faint) / 0.5);
  }
  .time:focus {
    outline: none;
    border-bottom-color: rgb(var(--accent));
    color: rgb(var(--fg-0));
  }
  .dash {
    color: rgb(var(--fg-faint));
    font-size: 11px;
  }
  .spacer {
    flex: 1;
  }
  .cue textarea {
    width: 100%;
    resize: none;
    background: rgb(var(--bg-0));
    border: 1px solid transparent;
    border-radius: var(--r-xs);
    color: rgb(var(--fg-0));
    font-size: 13.5px;
    line-height: 1.45;
    padding: 6px 8px;
    overflow: hidden;
  }
  .cue textarea:focus {
    outline: none;
    border-color: rgb(var(--accent));
  }
  .empty {
    font-size: 12.5px;
    color: rgb(var(--fg-2));
    text-align: center;
    padding: 24px 8px;
    line-height: 1.6;
  }
  .scene {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    background: rgb(var(--bg-2));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-sm);
  }
  .scene-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .scene-name {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: rgb(var(--accent) / 0.9);
  }
  .scene-time {
    font: 11px var(--font-mono);
    color: rgb(var(--fg-2));
  }
  .word-cue {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    padding: 4px 2px;
    border-radius: var(--r-xs);
    border: 1px dashed transparent;
    transition: border-color var(--motion) var(--ease);
  }
  .word-cue.active {
    border-color: rgb(var(--accent) / 0.35);
  }
  .word {
    padding: 2px 7px;
    border-radius: 999px;
    background: rgb(var(--bg-0));
    border: 1px solid transparent;
    font-size: 12.5px;
    line-height: 1.5;
    color: rgb(var(--fg-1));
    transition:
      background-color var(--motion) var(--ease),
      color var(--motion) var(--ease),
      border-color var(--motion) var(--ease);
  }
  .word:hover {
    background: rgb(var(--bg-3));
    color: rgb(var(--fg-0));
  }
  .word.active {
    background: rgb(var(--accent) / 0.2);
    border-color: rgb(var(--accent) / 0.6);
    color: rgb(var(--fg-0));
  }
  .word-edit {
    width: 9ch;
    padding: 2px 7px;
    border-radius: 999px;
    background: rgb(var(--bg-0));
    border: 1px solid rgb(var(--accent));
    font-size: 12.5px;
    color: rgb(var(--fg-0));
  }
  .word-edit:focus {
    outline: none;
  }
</style>
