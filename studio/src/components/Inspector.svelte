<script lang="ts">
  import TemplatesPanel from "./panels/TemplatesPanel.svelte";
  import CodePanel from "./panels/CodePanel.svelte";
  import PlaceholderPanel from "./panels/PlaceholderPanel.svelte";

  let { activeRail }: { activeRail: string } = $props();

  const labels: Record<string, string> = {
    templates: "Templates",
    transcript: "Transcript",
    typography: "Typography",
    style: "Style",
    position: "Position",
    motion: "Motion",
    effects: "Effects",
    layout: "Layout",
    code: "Code",
  };
</script>

<aside class="inspector">
  <div class="inspector-head">
    <span class="dot"></span>
    <span class="title">{labels[activeRail] || activeRail}</span>
  </div>

  <div class="inspector-body">
    {#if activeRail === "templates"}
      <TemplatesPanel />
    {:else if activeRail === "code"}
      <CodePanel />
    {:else}
      <PlaceholderPanel label={labels[activeRail] || activeRail} />
    {/if}
  </div>
</aside>

<style>
  .inspector {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: rgb(var(--bg-1));
    border: 1px solid rgb(var(--border));
    border-radius: var(--r-lg);
    overflow: hidden;
  }
  .inspector-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-bottom: 1px solid rgb(var(--border));
    flex: none;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 2px;
    background: rgb(var(--accent));
    flex: none;
  }
  .title {
    font-size: 13px;
    font-weight: 600;
    color: rgb(var(--fg-0));
    letter-spacing: -0.01em;
  }
  .inspector-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 12px;
    overflow: hidden;
  }
</style>
