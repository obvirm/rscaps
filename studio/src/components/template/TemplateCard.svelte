<script lang="ts">
  import { Check, Star } from "@lucide/svelte";
  import type { TemplateEntry } from "../../lib/types";

  let {
    template,
    preview = "",
    selected = false,
    favorite = false,
    onselect,
    onfavorite,
  }: {
    template: TemplateEntry;
    preview?: string;
    selected?: boolean;
    favorite?: boolean;
    onselect: (t: TemplateEntry) => void;
    onfavorite: (id: string) => void;
  } = $props();

  const checker = `background-image:
    linear-gradient(45deg, rgb(var(--fg-0) / 0.07) 25%, transparent 25%),
    linear-gradient(-45deg, rgb(var(--fg-0) / 0.07) 25%, transparent 25%),
    linear-gradient(45deg, transparent 75%, rgb(var(--fg-0) / 0.07) 75%),
    linear-gradient(-45deg, transparent 75%, rgb(var(--fg-0) / 0.07) 75%);
  background-size: 14px 14px;
  background-position: 0 0, 0 7px, 7px -7px, -7px 0px;`;
</script>

<div class="card-wrap group/card">
  <button
    class="frame"
    class:selected
    aria-label={template.label}
    aria-pressed={selected}
    onclick={() => onselect(template)}
  >
    <span class="preview" style={checker}>
      {#if preview}
        {@html preview}
      {/if}
    </span>
  </button>

  {#if selected}
    <span class="active-badge" aria-hidden="true"><Check size={10} strokeWidth={3} /></span>
  {/if}

  <button
    class="star"
    class:is-fav={favorite}
    aria-label={favorite ? "Remove from favorites" : "Add to favorites"}
    onclick={(e) => {
      e.stopPropagation();
      onfavorite(template.id);
    }}
  >
    <Star size={14} strokeWidth={2.25} fill={favorite ? "currentColor" : "none"} />
  </button>

  <span class="label">{template.label}</span>
</div>

<style>
  .card-wrap {
    position: relative;
    content-visibility: auto;
    contain-intrinsic-size: auto 120px;
  }
  .frame {
    width: 100%;
    aspect-ratio: 4 / 2;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 0;
    background: transparent;
    border: 1.5px solid rgb(var(--border-subtle));
    border-radius: var(--r-md);
    cursor: pointer;
    transition:
      border-color var(--motion) var(--ease),
      box-shadow var(--motion) var(--ease);
  }
  .frame:hover {
    border-color: rgb(var(--border-strong));
  }
  .frame:focus-visible {
    outline: none;
    border-color: rgb(var(--accent));
    box-shadow: inset 0 0 0 2px rgb(var(--accent) / 0.4);
  }
  .frame.selected {
    border-color: rgb(var(--accent));
  }
  .preview {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    pointer-events: none;
  }
  .frame {
    position: relative;
  }
  .preview :global(svg) {
    width: 100%;
    height: 100%;
    display: block;
  }
  .active-badge {
    position: absolute;
    bottom: 4px;
    left: 4px;
    width: 16px;
    height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: rgb(var(--accent));
    color: rgb(var(--fg-on-accent));
    pointer-events: none;
  }
  .star {
    position: absolute;
    top: 4px;
    right: 4px;
    padding: 4px;
    border-radius: var(--r-xs);
    color: rgb(var(--fg-faint));
    background: rgb(var(--bg-1) / 0.7);
    backdrop-filter: blur(4px);
    border: 1px solid rgb(var(--border));
    opacity: 0;
    transition:
      opacity var(--motion) var(--ease),
      color var(--motion) var(--ease);
  }
  @media (hover: hover) {
    .card-wrap:hover .star {
      opacity: 1;
    }
  }
  .star:focus-visible {
    opacity: 1;
  }
  .star.is-fav {
    opacity: 1;
    color: rgb(var(--accent));
  }
  .label {
    display: block;
    margin-top: 3px;
    font-size: 11px;
    line-height: 1;
    color: rgb(var(--fg-2));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
