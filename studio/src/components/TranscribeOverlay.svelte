<script lang="ts">
  import { store } from "../lib/state.svelte";
  import type { TranscribeStage } from "../lib/transcribe";

  const COPY: Record<TranscribeStage, { title: string; sub: string }> = {
    engine: { title: "Preparing the engine.", sub: "Loading the transcription engine." },
    model: { title: "Downloading the model.", sub: "First run only — the model is cached after this." },
    audio: { title: "Preparing your audio.", sub: "Decoding to 16 kHz mono." },
    transcribe: {
      title: "Transcribing your video in this browser.",
      sub: "Keep this tab open until it finishes.",
    },
    done: { title: "Finishing up.", sub: "Almost done…" },
  };

  const pct = $derived(Math.round(store.transcribeFraction * 100));
  const copy = $derived(COPY[store.transcribeStage] ?? COPY.engine);
</script>

{#if store.transcribing}
  <div class="overlay" role="status" aria-live="polite">
    <div class="panel">
      <div class="wordmark">rs<span>caps</span></div>
      <div class="pct">{pct}%</div>
      <div class="bar"><div class="fill" style={`width:${pct}%`}></div></div>
      <div class="title">{copy.title}</div>
      <div class="sub">{copy.sub}</div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgb(0 0 0 / 0.82);
    backdrop-filter: blur(6px);
  }
  .panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 18px;
    padding: 40px 48px;
    max-width: 460px;
    text-align: center;
  }
  .wordmark {
    font-size: 22px;
    font-weight: 800;
    letter-spacing: -0.03em;
    color: rgb(var(--fg-0));
  }
  .wordmark span {
    color: rgb(var(--accent));
  }
  .pct {
    font-family: var(--font-mono);
    font-size: 48px;
    font-weight: 700;
    color: rgb(var(--fg-0));
    line-height: 1;
  }
  .bar {
    width: 320px;
    max-width: 100%;
    height: 6px;
    border-radius: 999px;
    background: rgb(var(--bg-2));
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 999px;
    background: rgb(var(--accent));
    transition: width 200ms linear;
  }
  .title {
    font-size: 15px;
    font-weight: 600;
    color: rgb(var(--fg-0));
    margin-top: 6px;
  }
  .sub {
    font-size: 13px;
    color: rgb(var(--fg-2));
    line-height: 1.5;
  }
</style>
