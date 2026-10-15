<script lang="ts">
  import { Plus, Search, Trash2, LocateFixed } from "@lucide/svelte";
  import { store } from "../../lib/state.svelte";
  import { formatTime } from "../../lib/types";

  let query = $state("");

  const cues = $derived(store.cues);
  const nowMs = $derived((store.frame / store.fps) * 1000);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return cues
      .map((cue, i) => ({ cue, i }))
      .filter(({ cue }) => q === "" || cue.text.toLowerCase().includes(q));
  });

  const activeIndex = $derived(cues.findIndex((c) => nowMs >= c.start_ms && nowMs < c.end_ms));

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
</script>

<div class="transcript-panel">
  <div class="topbar">
    <div class="search">
      <Search size={13} />
      <input type="text" placeholder="Search…" bind:value={query} aria-label="Search captions" />
    </div>
    <span class="count">{cues.length}</span>
    <button class="add-btn" onclick={() => store.addCue(cues.length - 1)} title="Add caption">
      <Plus size={14} />
    </button>
  </div>

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
          <span class="dash">—</span>
          <input
            class="time"
            value={timeLabel(cue.end_ms)}
            aria-label="End time"
            onchange={(e) => onTimeChange(i, "end_ms", (e.currentTarget as HTMLInputElement).value)}
          />
          <span class="spacer"></span>
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
      <p class="empty">No captions {query ? `matching “${query}”` : "— import an .srt or transcribe"}</p>
    {/if}
  </div>
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
</style>
