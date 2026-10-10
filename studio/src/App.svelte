<script lang="ts">
  import { router } from "./lib/router.svelte";
  import Home from "./components/Home.svelte";
  import Editor from "./components/Editor.svelte";
  import TranscribeOverlay from "./components/TranscribeOverlay.svelte";
</script>

{#if router.route.screen === "home"}
  <Home />
{:else}
  <Editor projectId={router.route.projectId} />
{/if}

<TranscribeOverlay />
