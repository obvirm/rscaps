<script lang="ts">
  let { active = $bindable("templates") }: { active?: string } = $props();

  const items = [
    { id: "templates", label: "Templates" },
    { id: "transcript", label: "Transcript" },
    { id: "typography", label: "Type" },
    { id: "style", label: "Style" },
    { id: "position", label: "Position" },
    { id: "motion", label: "Motion" },
    { id: "effects", label: "Effects" },
    { id: "layout", label: "Layout" },
    { id: "code", label: "Code" },
  ];
</script>

<nav class="icon-rail" aria-label="Editor sections">
  {#each items as item (item.id)}
    <button
      class="rail-item"
      class:active={active === item.id}
      title={item.label}
      aria-label={item.label}
      aria-current={active === item.id}
      onclick={() => (active = item.id)}
    >
      <span class="tick tl"></span>
      <span class="tick tr"></span>
      <span class="tick bl"></span>
      <span class="tick br"></span>

      {#if item.id === "templates"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1" /><rect x="9" y="9" width="4.5" height="4.5" rx="1" /></svg>
      {:else if item.id === "transcript"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"><rect x="2" y="3.5" width="12" height="9" rx="2" /><path d="M4.5 8.5h3M9 8.5h2.5M4.5 10.7h5" /></svg>
      {:else if item.id === "typography"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"><path d="M3 4h10M8 4v8M6 12h4" /></svg>
      {:else if item.id === "style"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3"><circle cx="8" cy="8" r="5.5" /><circle cx="6" cy="6.5" r="1" fill="currentColor" stroke="none" /><circle cx="10" cy="6.5" r="1" fill="currentColor" stroke="none" /></svg>
      {:else if item.id === "position"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2v12M2 8h12M8 2l-2 2M8 2l2 2M8 14l-2-2M8 14l2-2M2 8l2-2M2 8l2 2M14 8l-2-2M14 8l-2 2" /></svg>
      {:else if item.id === "motion"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3"><circle cx="8" cy="8" r="2.2" /><ellipse cx="8" cy="8" rx="6" ry="2.8" transform="rotate(-24 8 8)" /></svg>
      {:else if item.id === "effects"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"><path d="M8 2.5l1.4 3.6L13 7.5l-3.6 1.4L8 12.5 6.6 8.9 3 7.5l3.6-1.4z" /></svg>
      {:else if item.id === "layout"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"><path d="M3 4h10M3 7h7M3 10h9M3 13h5" /></svg>
      {:else if item.id === "code"}
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"><path d="M5.5 4.5 2.5 8l3 3.5M10.5 4.5 13.5 8l-3 3.5" /></svg>
      {/if}
    </button>
  {/each}
</nav>

<style>
  .icon-rail {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-lg);
    align-self: start;
    position: sticky;
    top: 10px;
    max-height: calc(100dvh - 120px);
    overflow-y: auto;
    scrollbar-width: none;
  }
  .icon-rail::-webkit-scrollbar {
    display: none;
  }
  .rail-item {
    position: relative;
    width: 46px;
    height: 42px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--r-md);
    color: rgb(var(--fg-2));
    transition: color var(--motion) var(--ease), background-color var(--motion) var(--ease);
  }
  .rail-item:hover {
    background: rgb(var(--bg-2));
    color: rgb(var(--fg-1));
  }
  .rail-item.active {
    color: rgb(var(--accent));
    background: rgb(var(--bg-3));
  }
  /* corner ticks (crop marks) on active */
  .tick {
    position: absolute;
    width: 6px;
    height: 6px;
    opacity: 0;
    transition: opacity var(--motion) var(--ease);
  }
  .tick::before,
  .tick::after {
    content: "";
    position: absolute;
    background: rgb(var(--accent));
  }
  .tick::before {
    width: 6px;
    height: 1.5px;
  }
  .tick::after {
    width: 1.5px;
    height: 6px;
  }
  .tick.tl {
    top: 4px;
    left: 4px;
  }
  .tick.tr {
    top: 4px;
    right: 4px;
  }
  .tick.tr::before {
    right: 0;
  }
  .tick.tr::after {
    right: 0;
  }
  .tick.bl {
    bottom: 4px;
    left: 4px;
  }
  .tick.bl::before {
    bottom: 0;
  }
  .tick.bl::after {
    bottom: 0;
  }
  .tick.br {
    bottom: 4px;
    right: 4px;
  }
  .tick.br::before {
    bottom: 0;
    right: 0;
  }
  .tick.br::after {
    bottom: 0;
    right: 0;
  }
  .rail-item.active .tick {
    opacity: 1;
  }
</style>
