import init, { create_wasm_bridge, set_document_json } from "/pkg/caption_bridge.js";

const FPS = 30;

const DEFAULT_DOC = {
  duration_frames: 300,
  cues: [
    { start_ms: 0, end_ms: 2500, text: "Welcome to rscaps" },
    { start_ms: 2500, end_ms: 5500, text: "مرحبا بالعالم من rscaps — هذا نص عربي طويل لاختبار تقسيم الأسطر" },
    { start_ms: 5500, end_ms: 8000, text: "שלום עולם — זהו טקסט עברי לבדיקת פריסה" },
    { start_ms: 8000, end_ms: 10000, text: "Captions rendered by fframes in Rust" },
  ],
  params: {
    font_family: "Arial",
    font_size: 110,
    font_weight: 400,
    color: "#ffdd57",
    y_frac: 0.78,
    width_frac: 0.78,
    pop_in_ms: 250,
    travel_px: 48,
  },
};

const $ = (id) => document.getElementById(id);
const els = {
  doc: $("doc"),
  apply: $("apply"),
  play: $("play"),
  frame: $("frame"),
  render: $("render"),
  status: $("status"),
  stage: $("stage"),
  stageCaption: $("stage-caption"),
  raster: $("raster"),
  rasterStage: $("raster-stage"),
  rasterCaption: $("raster-caption"),
  thumbs: $("thumbs"),
  scrub: $("scrub"),
  timeDisplay: $("time-display"),
  durationDisplay: $("duration-display"),
  themeToggle: $("theme-toggle"),
  exportBtn: $("export"),
  editor: $("editor"),
  handle: $("resize-handle"),
  previewCol: $("preview-col"),
  timeline: $("timeline"),
  timelineRuler: $("timeline-ruler"),
  timelineLane: $("timeline-lane"),
  playhead: $("playhead"),
  placeholderNote: $("placeholder-note"),
  templatePicker: $("template-picker"),
  templateSelect: $("template-select"),
};

let bridge;
let durationFrames = DEFAULT_DOC.duration_frames;
let playing = false;
let playRaf = 0;
let playStart = 0;
let playStartFrame = 0;
let lastCanvas = null;
let fontFaceCss = "";

function setStatus(message, isError = false) {
  els.status.textContent = message;
  els.status.classList.toggle("error", isError);
  els.status.classList.toggle("ready", !isError);
}

function formatTime(seconds) {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const cs = Math.floor((seconds % 1) * 100);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(cs).padStart(2, "0")}`;
}

function formatTimeShort(seconds) {
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function activeCueIndex(frame) {
  const t = (frame / FPS) * 1000;
  let idx = -1;
  try {
    const doc = JSON.parse(els.doc.value);
    idx = doc.cues.findIndex((c) => t >= c.start_ms && t < c.end_ms);
  } catch {}
  return idx;
}

function renderAt(frame) {
  const svg = bridge.render_frame(BigInt(frame));
  els.stage.innerHTML = svg;
  els.stageCaption.textContent = `frame ${frame} · t=${(frame / FPS).toFixed(2)}s`;
  els.frame.value = String(frame);
  els.scrub.value = String(frame);
  els.timeDisplay.textContent = formatTime(frame / FPS);

  // Timeline: playhead + active cue highlight
  const pct = durationFrames > 1 ? (frame / (durationFrames - 1)) * 100 : 0;
  els.playhead.style.left = `${pct}%`;
  const active = activeCueIndex(frame);
  els.timelineLane.querySelectorAll(".cue-chip").forEach((chip, i) => {
    chip.classList.toggle("active", i === active);
  });
  return svg;
}

function renderThumbs() {
  const frames = [10, 65, 150];
  els.thumbs.innerHTML = "";
  for (const frame of frames) {
    const fig = document.createElement("figure");
    const stage = document.createElement("div");
    stage.className = "stage";
    stage.innerHTML = bridge.render_frame(BigInt(frame));
    const caption = document.createElement("figcaption");
    caption.textContent = `frame ${frame}`;
    fig.append(stage, caption);
    els.thumbs.append(fig);
  }
}

function renderTimeline() {
  const durationSec = durationFrames / FPS;
  els.durationDisplay.textContent = formatTime(durationSec);
  els.scrub.max = String(Math.max(durationFrames - 1, 0));

  // Ruler ticks — adaptive step so labels never collide in the sidebar
  const niceSteps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600];
  const raw = durationSec / 6;
  const step = niceSteps.find((s) => s >= raw) || 900;
  els.timelineRuler.innerHTML = "";
  for (let s = 0; s <= durationSec + 0.001; s += step) {
    const tick = document.createElement("span");
    tick.className = "timeline-tick";
    tick.style.left = `${(s / durationSec) * 100}%`;
    tick.textContent = formatTimeShort(s);
    els.timelineRuler.append(tick);
  }

  // Cue chips
  els.timelineLane.querySelectorAll(".cue-chip").forEach((c) => c.remove());
  let doc;
  try {
    doc = JSON.parse(els.doc.value);
  } catch {
    return;
  }
  for (const cue of doc.cues || []) {
    const chip = document.createElement("button");
    chip.className = "cue-chip";
    const start = cue.start_ms / 1000;
    const end = cue.end_ms / 1000;
    chip.style.left = `${(start / durationSec) * 100}%`;
    chip.style.width = `calc(${(((end - start) / durationSec) * 100).toFixed(3)}% - 4px)`;
    chip.textContent = cue.text;
    chip.title = `${formatTime(start)} → ${formatTime(end)}`;
    chip.addEventListener("click", () => {
      stopPlay();
      renderAt(Math.min(Math.round(start * FPS), durationFrames - 1));
    });
    els.timelineLane.append(chip);
  }
}

async function renderRaster() {
  const raw = bridge.render_frame(BigInt(els.frame.value || 0));
  if (!raw) {
    els.rasterStage.innerHTML = "";
    els.rasterCaption.textContent = "raster — empty frame";
    return;
  }
  // SVG-as-image can't see document @font-face: inline it (same base64
  // data URLs), mirroring what fframes' export does.
  const svg = fontFaceCss
    ? raw.replace(/^<svg[^>]*>/, (m) => `${m}<style>${fontFaceCss}</style>`)
    : raw;
  const t0 = performance.now();
  const uri = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
  const img = new Image();
  img.src = uri;
  await img.decode();
  const canvas = document.createElement("canvas");
  canvas.width = 1920;
  canvas.height = 1080;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(img, 0, 0, 1920, 1080);
  const ms = performance.now() - t0;
  els.rasterStage.innerHTML = "";
  els.rasterStage.append(canvas);
  canvas.style.width = "100%";
  canvas.style.height = "100%";
  lastCanvas = canvas;
  els.rasterCaption.textContent = `raster frame ${els.frame.value} · ${ms.toFixed(1)} ms (decode+draw)`;
}

async function applyDocument() {
  set_document_json(els.doc.value);
  const meta = await bridge.prepare(FPS);
  durationFrames = meta.durationInFrames;
  setStatus(`prepared · duration=${durationFrames} frames · fps=${meta.fps}`);
  renderAt(Number(els.frame.value) || 0);
  renderThumbs();
  renderTimeline();
  await renderRaster();
  if (document.fonts && document.fonts.ready) {
    // Fonts can finish loading after the first paint → repaint once ready.
    document.fonts.ready.then(() => {
      renderAt(Number(els.frame.value) || 0);
      renderThumbs();
    });
  }
}

function stopPlay() {
  playing = false;
  cancelAnimationFrame(playRaf);
  els.play.dataset.playing = "false";
  els.play.setAttribute("aria-label", "Play");
}

function togglePlay() {
  if (playing) {
    stopPlay();
    return;
  }
  playing = true;
  els.play.dataset.playing = "true";
  els.play.setAttribute("aria-label", "Pause");
  playStart = performance.now();
  playStartFrame = Number(els.frame.value) % durationFrames;
  const tick = (now) => {
    if (!playing) return;
    const frame =
      (playStartFrame + Math.floor(((now - playStart) / 1000) * FPS)) % durationFrames;
    renderAt(frame);
    playRaf = requestAnimationFrame(tick);
  };
  playRaf = requestAnimationFrame(tick);
}

// ── Wiring ────────────────────────────────────────────────────────────

els.apply.addEventListener("click", () => {
  applyDocument().catch((err) => setStatus(String(err), true));
});
els.render.addEventListener("click", () => {
  renderAt(Number(els.frame.value) || 0);
});
els.play.addEventListener("click", togglePlay);
els.raster.addEventListener("click", () => {
  renderRaster().catch((err) => setStatus(String(err), true));
});
els.frame.addEventListener("change", () => renderAt(Number(els.frame.value) || 0));

els.scrub.addEventListener("input", () => {
  stopPlay();
  renderAt(Number(els.scrub.value) || 0);
});

// Theme toggle
els.themeToggle.addEventListener("click", () => {
  const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem("rscaps:theme", next);
  } catch {}
});

// Export current frame as PNG
els.exportBtn.addEventListener("click", async () => {
  try {
    await renderRaster();
    if (!lastCanvas) return;
    lastCanvas.toBlob((blob) => {
      if (!blob) return;
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = `frame-${els.frame.value}.png`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 5000);
      setStatus(`exported frame-${els.frame.value}.png`);
    }, "image/png");
  } catch (err) {
    setStatus(String(err), true);
  }
});

// Mode strip: Captions | divider | Timeline
document.querySelectorAll(".mode-tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".mode-tab").forEach((t) => t.classList.remove("active"));
    tab.classList.add("active");
    const mode = tab.dataset.mode;
    document.querySelectorAll(".mode-panel").forEach((panel) => {
      panel.hidden = panel.dataset.panel !== mode;
    });
    if (mode === "timeline") renderTimeline();
  });
});

// Captions sub-tabs
const SUB_TAB_LABELS = {
  templates: "Templates",
  transcript: "Transcript",
  typography: "Typography",
  style: "Style",
  position: "Position",
  motion: "Motion",
  effects: "Effects",
  layout: "Layout",
};

document.querySelectorAll(".subtab").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".subtab").forEach((t) => t.classList.remove("active"));
    tab.classList.add("active");
    const sub = tab.dataset.sub;
    const codePanel = document.querySelector('[data-subpanel="code"]');
    const placeholder = document.querySelector('[data-subpanel="placeholder"]');
    if (sub === "code") {
      codePanel.hidden = false;
      placeholder.hidden = true;
    } else {
      codePanel.hidden = true;
      placeholder.hidden = false;
      els.templatePicker.hidden = sub !== "templates";
      els.placeholderNote.textContent =
        sub === "templates"
          ? "Native templates render 1:1 through fframes — same engine in preview and export."
          : `${SUB_TAB_LABELS[sub] || sub} — coming in Fase 1`;
    }
  });
});

// Template picker: write the id into the document JSON and re-prepare.
els.templateSelect.addEventListener("change", () => {
  try {
    const doc = JSON.parse(els.doc.value);
    doc.template = els.templateSelect.value;
    els.doc.value = JSON.stringify(doc, null, 2);
  } catch (err) {
    setStatus(String(err), true);
    return;
  }
  applyDocument().catch((err) => setStatus(String(err), true));
});

// Keyboard shortcuts (tscaps-style): Space = play/pause, ←/→ = frame step
window.addEventListener("keydown", (e) => {
  const tag = (e.target.tagName || "").toLowerCase();
  if (tag === "input" || tag === "textarea" || e.target.isContentEditable) return;
  if (e.code === "Space") {
    e.preventDefault();
    togglePlay();
  } else if (e.code === "ArrowLeft") {
    e.preventDefault();
    stopPlay();
    renderAt(Math.max((Number(els.frame.value) || 0) - (e.shiftKey ? 10 : 1), 0));
  } else if (e.code === "ArrowRight") {
    e.preventDefault();
    stopPlay();
    renderAt(Math.min((Number(els.frame.value) || 0) + (e.shiftKey ? 10 : 1), durationFrames - 1));
  }
});

// Sidebar resize handle (left/right split)
{
  let dragging = false;
  els.handle.addEventListener("pointerdown", (e) => {
    dragging = true;
    els.handle.classList.add("dragging");
    els.handle.setPointerCapture(e.pointerId);
  });
  els.handle.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const rect = els.editor.getBoundingClientRect();
    const left = Math.min(Math.max(e.clientX - rect.left, rect.width * 0.45), rect.width - 300);
    els.editor.style.gridTemplateColumns = `${left}px 12px minmax(280px, 1fr)`;
    window.dispatchEvent(new Event("resize"));
  });
  const end = () => {
    dragging = false;
    els.handle.classList.remove("dragging");
  };
  els.handle.addEventListener("pointerup", end);
  els.handle.addEventListener("pointercancel", end);
}

// Keep video box within the preview column: measure controls height
{
  const playbar = document.querySelector(".playbar");
  const strip = document.querySelector(".compare-strip");
  const measure = () => {
    const h = playbar.offsetHeight + strip.offsetHeight + 36; // gaps + padding
    els.previewCol.style.setProperty("--controls-h", `${h}px`);
  };
  const ro = new ResizeObserver(measure);
  ro.observe(playbar);
  ro.observe(strip);
  measure();
}

try {
  setStatus("initializing wasm…");
  await init();

  bridge = create_wasm_bridge();

  // Fonts are used twice: ingested into wasm (fframes measures/uploads them)
  // and declared as @font-face (the browser paints them) — both sides must
  // see the exact same file or preview and export diverge.
  const FONT_SOURCES = [
    // Arial Black (weight 900, Latin only) + Arial regular (weight 400, has
    // Arabic/Hebrew) — both report family name "Arial"; face selection is by
    // weight. Already provided by the OS in the browser, so no @font-face.
    { path: "/fonts/ariblk.ttf" },
    { path: "/fonts/arial.ttf" },
    { path: "/tplfonts/ttf/poppins-latin-400-normal.ttf", family: "Poppins", weight: 400 },
    { path: "/tplfonts/ttf/poppins-latin-700-normal.ttf", family: "Poppins", weight: 700 },
    { path: "/tplfonts/ttf/cairo-arabic-700-normal.ttf", family: "Cairo", weight: 700 },
    { path: "/tplfonts/ttf/assistant-hebrew-700-normal.ttf", family: "Assistant", weight: 700 },
    { path: "/tplfonts/ttf/vazirmatn-arabic-700-normal.ttf", family: "Vazirmatn", weight: 700 },
    { path: "/tplfonts/ttf/heebo-hebrew-700-normal.ttf", family: "Heebo", weight: 700 },
    { path: "/tplfonts/ttf/lalezar-arabic-400-normal.ttf", family: "Lalezar", weight: 400 },
    { path: "/tplfonts/ttf/amiri-arabic-700-normal.ttf", family: "Amiri", weight: 700 },
    { path: "/tplfonts/ttf/bebas-neue-latin-400-normal.ttf", family: "Bebas Neue", weight: 400 },
    { path: "/tplfonts/ttf/inter-latin-400-normal.ttf", family: "Inter", weight: 400 },
    { path: "/tplfonts/ttf/inter-latin-600-normal.ttf", family: "Inter", weight: 600 },
    { path: "/tplfonts/ttf/inter-latin-700-normal.ttf", family: "Inter", weight: 700 },
    { path: "/tplfonts/ttf/inter-latin-900-normal.ttf", family: "Inter", weight: 900 },
    { path: "/tplfonts/ttf/anton-latin-400-normal.ttf", family: "Anton", weight: 400 },
    { path: "/tplfonts/ttf/bungee-latin-400-normal.ttf", family: "Bungee", weight: 400 },
    { path: "/tplfonts/ttf/poppins-latin-900-normal.ttf", family: "Poppins", weight: 900 },
    { path: "/tplfonts/ttf/playfair-display-latin-700-normal.ttf", family: "Playfair Display", weight: 700 },
    { path: "/tplfonts/ttf/playfair-display-latin-800-normal.ttf", family: "Playfair Display", weight: 800 },
    { path: "/tplfonts/ttf/lora-latin-500-normal.ttf", family: "Lora", weight: 500 },
    { path: "/tplfonts/ttf/gabarito-latin-600-normal.ttf", family: "Gabarito", weight: 600 },
    { path: "/tplfonts/ttf/gabarito-latin-900-normal.ttf", family: "Gabarito", weight: 900 },
    { path: "/tplfonts/ttf/bricolage-grotesque-latin-800-normal.ttf", family: "Bricolage Grotesque", weight: 800 },
    { path: "/tplfonts/ttf/jetbrains-mono-latin-400-normal.ttf", family: "JetBrains Mono", weight: 400 },
    { path: "/tplfonts/ttf/caveat-latin-600-normal.ttf", family: "Caveat", weight: 600 },
    { path: "/tplfonts/ttf/caveat-latin-700-normal.ttf", family: "Caveat", weight: 700 },
    { path: "/tplfonts/ttf/montserrat-latin-300-normal.ttf", family: "Montserrat", weight: 300 },
    { path: "/tplfonts/ttf/montserrat-latin-600-normal.ttf", family: "Montserrat", weight: 600 },
    { path: "/tplfonts/ttf/montserrat-latin-700-normal.ttf", family: "Montserrat", weight: 700 },
    { path: "/tplfonts/ttf/manrope-latin-500-normal.ttf", family: "Manrope", weight: 500 },
    { path: "/tplfonts/ttf/righteous-latin-400-normal.ttf", family: "Righteous", weight: 400 },
    { path: "/tplfonts/ttf/vt323-latin-400-normal.ttf", family: "VT323", weight: 400 },
    { path: "/tplfonts/ttf/komika-axis-latin-400-normal.ttf", family: "Komika Axis", weight: 400 },
    { path: "/tplfonts/ttf/im-fell-english-latin-400-normal.ttf", family: "IM Fell English", weight: 400 },
    { path: "/tplfonts/ttf/lobster-latin-400-normal.ttf", family: "Lobster", weight: 400 },
  ];

  let faceCss = "";
  const bytesToBase64 = (bytes) => {
    let bin = "";
    for (let i = 0; i < bytes.length; i += 0x8000) {
      bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    return btoa(bin);
  };

  for (const src of FONT_SOURCES) {
    const fontBytes = await fetch(src.path).then((res) => {
      if (!res.ok) throw new Error(`font fetch failed: ${src.path} ${res.status}`);
      return res.arrayBuffer();
    });
    const face = bridge.ingest_font(new Uint8Array(fontBytes));
    console.log("ingested font", src.path, face);
    if (src.family) {
      faceCss +=
        `@font-face{font-family:"${src.family}";font-style:normal;` +
        `font-weight:${src.weight};font-display:block;` +
        `src:url("data:font/ttf;base64,${bytesToBase64(new Uint8Array(fontBytes))}") format("truetype");}\n`;
    }
  }

  if (faceCss) {
    fontFaceCss = faceCss;
    const style = document.createElement("style");
    style.id = "tpl-fonts";
    style.textContent = faceCss;
    document.head.append(style);
    await Promise.all([
      document.fonts.load('400 40px "Poppins"'),
      document.fonts.load('700 40px "Poppins"'),
      document.fonts.load('700 40px "Cairo"'),
      document.fonts.load('700 40px "Assistant"'),
      document.fonts.load('700 40px "Vazirmatn"'),
      document.fonts.load('700 40px "Heebo"'),
      document.fonts.load('400 40px "Lalezar"'),
      document.fonts.load('700 40px "Amiri"'),
      document.fonts.load('400 40px "Bebas Neue"'),
      document.fonts.load('400 40px "Inter"'),
      document.fonts.load('600 40px "Inter"'),
      document.fonts.load('700 40px "Inter"'),
      document.fonts.load('900 40px "Inter"'),
      document.fonts.load('400 40px "Anton"'),
      document.fonts.load('400 40px "Bungee"'),
      document.fonts.load('900 40px "Poppins"'),
      document.fonts.load('700 40px "Playfair Display"'),
      document.fonts.load('800 40px "Playfair Display"'),
      document.fonts.load('500 40px "Lora"'),
      document.fonts.load('600 40px "Gabarito"'),
      document.fonts.load('900 40px "Gabarito"'),
      document.fonts.load('800 40px "Bricolage Grotesque"'),
      document.fonts.load('400 40px "JetBrains Mono"'),
      document.fonts.load('600 40px "Caveat"'),
      document.fonts.load('700 40px "Caveat"'),
      document.fonts.load('300 40px "Montserrat"'),
      document.fonts.load('600 40px "Montserrat"'),
      document.fonts.load('700 40px "Montserrat"'),
      document.fonts.load('500 40px "Manrope"'),
      document.fonts.load('400 40px "Righteous"'),
      document.fonts.load('400 40px "VT323"'),
      document.fonts.load('400 40px "Komika Axis"'),
      document.fonts.load('400 40px "IM Fell English"'),
      document.fonts.load('400 40px "Lobster"'),
    ]);
  }

  els.doc.value = JSON.stringify(DEFAULT_DOC, null, 2);
  try {
    els.templateSelect.value = JSON.parse(els.doc.value).template || "";
  } catch {}
  await applyDocument();
} catch (err) {
  console.error(err);
  setStatus(String(err), true);
}
