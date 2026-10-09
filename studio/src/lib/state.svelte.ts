import { loadBridge, type LoadedBridge } from "./wasm";
import {
  formatTime,
  formatTimeShort,
  newProjectId,
  type CaptionDocument,
  type Cue,
  type Direction,
  type ProjectMetadata,
  type RscapsProject,
  type WhisperBackend,
  type WhisperModel,
} from "./types";
import {
  deleteProject as dbDelete,
  getProject,
  listProjects,
  loadVideo,
  saveProject as dbSave,
  saveVideo,
  toMetadata,
} from "./persistence";

export interface NewProjectOptions {
  name: string;
  template: string;
  direction: Direction;
  backend: WhisperBackend;
  model: WhisperModel;
  video: Blob;
  videoName: string;
  durationSec: number;
  fps: number;
  width: number;
  height: number;
}

const DEFAULT_PARAMS: CaptionDocument["params"] = {
  font_family: "Arial",
  font_size: 110,
  font_weight: 400,
  color: "#ffdd57",
  y_frac: 0.78,
  width_frac: 0.78,
  pop_in_ms: 250,
  travel_px: 48,
  direction: "auto",
};

function buildDoc(opts: NewProjectOptions): CaptionDocument {
  const durationMs = Math.max(1, Math.round(opts.durationSec * 1000));
  return {
    duration_frames: Math.max(1, Math.round(opts.durationSec * opts.fps)),
    cues: [{ start_ms: 0, end_ms: durationMs, text: opts.name || "New project" }],
    template: opts.template || undefined,
    params: { ...DEFAULT_PARAMS, direction: opts.direction },
  };
}

class StudioState {
  // ── Session / editor state ──────────────────────────────────────────────
  currentId = $state<string | null>(null);
  projectName = $state("Untitled");
  frame = $state(0);
  playing = $state(false);
  durationFrames = $state(300);
  fps = $state(30);
  videoWidth = $state(1920);
  videoHeight = $state(1080);
  videoUrl = $state<string | null>(null);
  videoName = $state<string | null>(null);
  backend = $state<WhisperBackend>("cpu");
  model = $state<WhisperModel>("base");
  status = $state("idle");
  statusIsError = $state(false);
  docText = $state("");
  svg = $state("");
  fontFaceCss = $state("");
  bootError = $state<string | null>(null);

  // ── Home state ──────────────────────────────────────────────────────────
  projects = $state<ProjectMetadata[]>([]);
  projectsLoading = $state(false);

  // ── Non-reactive wasm handles ───────────────────────────────────────────
  private wasm: LoadedBridge | null = null;
  private lastCanvas: HTMLCanvasElement | null = null;
  private playRaf = 0;
  private playStart = 0;
  private playStartFrame = 0;
  private saveTimer = 0;
  private videoEl: HTMLVideoElement | null = null;
  private objectUrl: string | null = null;

  // ── Derived ─────────────────────────────────────────────────────────────
  aspect = $derived(`${this.videoWidth} / ${this.videoHeight}`);
  cues = $derived.by<Cue[]>(() => {
    try {
      const doc = JSON.parse(this.docText) as CaptionDocument;
      return doc.cues || [];
    } catch {
      return [];
    }
  });
  durationSec = $derived(this.durationFrames / this.fps);
  timeText = $derived(formatTime(this.frame / this.fps));
  durationText = $derived(formatTime(this.durationSec));
  scrubMax = $derived(Math.max(this.durationFrames - 1, 0));
  playheadPct = $derived(this.durationFrames > 1 ? (this.frame / (this.durationFrames - 1)) * 100 : 0);

  activeCueIndex = $derived.by(() => {
    const t = (this.frame / this.fps) * 1000;
    return this.cues.findIndex((c) => t >= c.start_ms && t < c.end_ms);
  });

  rulerTicks = $derived.by(() => {
    const niceSteps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600];
    const raw = this.durationSec / 6;
    const step = niceSteps.find((s) => s >= raw) || 900;
    const ticks: { left: number; label: string }[] = [];
    for (let s = 0; s <= this.durationSec + 0.001; s += step) {
      ticks.push({ left: (s / this.durationSec) * 100, label: formatTimeShort(s) });
    }
    return ticks;
  });

  cueChips = $derived.by(() => {
    return this.cues.map((cue) => {
      const start = cue.start_ms / 1000;
      const end = cue.end_ms / 1000;
      return {
        text: cue.text,
        left: (start / this.durationSec) * 100,
        width: ((end - start) / this.durationSec) * 100,
        title: `${formatTime(start)} → ${formatTime(end)}`,
        seekFrame: Math.min(Math.round(start * this.fps), this.durationFrames - 1),
      };
    });
  });

  // ── Helpers ─────────────────────────────────────────────────────────────
  setStatus(message: string, isError = false): void {
    this.status = message;
    this.statusIsError = isError;
  }

  private currentDoc(): CaptionDocument | null {
    try {
      return JSON.parse(this.docText) as CaptionDocument;
    } catch {
      return null;
    }
  }

  private updateDoc(mutate: (doc: CaptionDocument) => void): boolean {
    const doc = this.currentDoc();
    if (!doc) return false;
    mutate(doc);
    this.docText = JSON.stringify(doc, null, 2);
    return true;
  }

  // ── Project lifecycle ───────────────────────────────────────────────────
  async bootWasm(): Promise<void> {
    if (this.wasm) return;
    try {
      this.setStatus("initializing wasm…");
      const loaded = await loadBridge();
      this.wasm = loaded;
      this.fontFaceCss = loaded.fontFaceCss;
      this.setStatus("ready");
    } catch (err) {
      console.error(err);
      this.bootError = String(err);
      this.setStatus(String(err), true);
    }
  }

  async refreshProjects(): Promise<void> {
    this.projectsLoading = true;
    try {
      const list = await listProjects();
      this.projects = list.map(toMetadata);
    } catch (err) {
      console.error(err);
      this.setStatus(String(err), true);
    } finally {
      this.projectsLoading = false;
    }
  }

  private applyVideoMeta(fps: number, width: number, height: number, videoName: string | null): void {
    this.fps = fps;
    this.videoWidth = width;
    this.videoHeight = height;
    this.videoName = videoName;
  }

  private setVideoBlob(blob: Blob): void {
    if (this.objectUrl) URL.revokeObjectURL(this.objectUrl);
    this.objectUrl = URL.createObjectURL(blob);
    this.videoUrl = this.objectUrl;
  }

  attachVideoElement(el: HTMLVideoElement | null): void {
    this.videoEl = el;
  }

  async createProject(opts: NewProjectOptions): Promise<string> {
    await this.bootWasm();
    const now = new Date().toISOString();
    const project: RscapsProject = {
      id: newProjectId(),
      name: opts.name || "Untitled",
      createdAt: now,
      updatedAt: now,
      doc: buildDoc(opts),
      settings: {
        fps: opts.fps,
        width: opts.width,
        height: opts.height,
        backend: opts.backend,
        model: opts.model,
      },
      videoName: opts.videoName,
    };
    this.currentId = project.id;
    this.projectName = project.name;
    this.applyVideoMeta(opts.fps, opts.width, opts.height, opts.videoName);
    this.backend = opts.backend;
    this.model = opts.model;
    this.setVideoBlob(opts.video);
    if (this.videoEl) {
      this.videoEl.src = this.videoUrl!;
      this.videoEl.load();
    }
    this.docText = JSON.stringify(project.doc, null, 2);
    await this.applyDocument();
    project.thumbnail = await this.makeThumbnail();
    await dbSave(project);
    await saveVideo(project.id, opts.video);
    await this.refreshProjects();
    return project.id;
  }

  async openProject(id: string): Promise<void> {
    await this.bootWasm();
    const project = await getProject(id);
    if (!project) {
      this.setStatus(`project ${id} not found`, true);
      return;
    }
    this.currentId = project.id;
    this.projectName = project.name;
    this.applyVideoMeta(
      project.settings.fps || 30,
      project.settings.width || 1920,
      project.settings.height || 1080,
      project.videoName || null,
    );
    this.backend = project.settings.backend || "cpu";
    this.model = project.settings.model || "base";
    this.docText = JSON.stringify(project.doc, null, 2);
    this.frame = 0;
    this.stopPlay();
    const blob = await loadVideo(id);
    if (blob) {
      this.setVideoBlob(blob);
      if (this.videoEl) {
        this.videoEl.src = this.videoUrl!;
        this.videoEl.load();
      }
    }
    await this.applyDocument();
    this.setStatus(`opened “${project.name}”`);
  }

  async saveCurrent(): Promise<void> {
    if (!this.currentId) return;
    const existing = await getProject(this.currentId);
    const doc = this.currentDoc();
    if (!doc) return;
    const now = new Date().toISOString();
    const project: RscapsProject = {
      id: this.currentId,
      name: this.projectName || "Untitled",
      createdAt: existing?.createdAt || now,
      updatedAt: now,
      doc,
      settings: {
        fps: this.fps,
        width: this.videoWidth,
        height: this.videoHeight,
        backend: this.backend,
        model: this.model,
      },
      videoName: this.videoName ?? undefined,
      thumbnail: await this.makeThumbnail(),
    };
    await dbSave(project);
  }

  scheduleSave(): void {
    if (!this.currentId) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => {
      this.saveCurrent()
        .then(() => this.refreshProjects())
        .catch((err) => console.error(err));
    }, 600);
  }

  async deleteProjectById(id: string): Promise<void> {
    await dbDelete(id);
    if (this.currentId === id) this.currentId = null;
    await this.refreshProjects();
  }

  // ── Rendering ───────────────────────────────────────────────────────────
  renderAt(f: number): void {
    if (!this.wasm) return;
    this.svg = this.wasm.bridge.render_frame(BigInt(f));
    this.frame = f;
    this.syncVideoToFrame(f);
  }

  // Render caption without touching the video element (used by playback loop).
  private renderCaption(f: number): void {
    if (!this.wasm) return;
    this.svg = this.wasm.bridge.render_frame(BigInt(f));
    this.frame = f;
  }

  private syncVideoToFrame(f: number): void {
    const v = this.videoEl;
    if (!v || this.playing) return;
    const target = f / this.fps;
    if (Number.isFinite(target) && Math.abs(v.currentTime - target) > 0.001) {
      v.currentTime = target;
    }
  }

  async applyDocument(): Promise<void> {
    if (!this.wasm) return;
    this.wasm.setDocumentJson(this.docText);
    const meta = await this.wasm.bridge.prepare(this.fps);
    this.durationFrames = meta.durationInFrames;
    this.setStatus(`prepared · ${this.durationFrames} frames · ${meta.fps} fps`);
    this.renderAt(Math.min(this.frame, Math.max(this.durationFrames - 1, 0)));
    if (document.fonts && document.fonts.ready) {
      document.fonts.ready.then(() => this.renderAt(this.frame));
    }
  }

  private async renderToCanvas(frame: number, width: number): Promise<HTMLCanvasElement | null> {
    if (!this.wasm) return null;
    const raw = this.wasm.bridge.render_frame(BigInt(frame));
    if (!raw) return null;
    const inlined = this.fontFaceCss
      ? raw.replace(/^<svg[^>]*>/, (m) => `${m}<style>${this.fontFaceCss}</style>`)
      : raw;
    const uri = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(inlined);
    const img = new Image();
    img.src = uri;
    await img.decode();
    const w = this.videoWidth || 1920;
    const h = this.videoHeight || 1080;
    const scale = width / w;
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = Math.round(h * scale);
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
    return canvas;
  }

  async renderRaster(): Promise<void> {
    const t0 = performance.now();
    const canvas = await this.renderToCanvas(this.frame, 1920);
    const ms = performance.now() - t0;
    if (!canvas) return;
    this.lastCanvas = canvas;
    this.setStatus(`raster frame ${this.frame} · ${ms.toFixed(1)} ms`);
  }

  async makeThumbnail(): Promise<string | undefined> {
    const target = Math.min(Math.max(Math.floor(this.fps / 2), 0), Math.max(this.durationFrames - 1, 0));
    const canvas = await this.renderToCanvas(target, 320);
    return canvas ? canvas.toDataURL("image/jpeg", 0.7) : undefined;
  }

  // ── Playback ────────────────────────────────────────────────────────────
  stopPlay(): void {
    this.playing = false;
    cancelAnimationFrame(this.playRaf);
    this.videoEl?.pause();
  }

  togglePlay(): void {
    if (this.playing) {
      this.stopPlay();
      return;
    }
    this.playing = true;
    const v = this.videoEl;
    if (v && this.videoUrl) {
      if (v.src !== this.videoUrl) v.src = this.videoUrl;
      if (v.currentTime >= this.durationSec - 0.05) v.currentTime = 0;
      void v.play().catch(() => {});
      const tick = () => {
        if (!this.playing) return;
        const f = Math.min(
          Math.round(v.currentTime * this.fps),
          Math.max(this.durationFrames - 1, 0),
        );
        this.renderCaption(f);
        this.playRaf = requestAnimationFrame(tick);
      };
      this.playRaf = requestAnimationFrame(tick);
    } else {
      this.playStart = performance.now();
      this.playStartFrame = this.frame % this.durationFrames;
      const tick = (now: number) => {
        if (!this.playing) return;
        const f =
          (this.playStartFrame + Math.floor(((now - this.playStart) / 1000) * this.fps)) %
          this.durationFrames;
        this.renderCaption(f);
        this.playRaf = requestAnimationFrame(tick);
      };
      this.playRaf = requestAnimationFrame(tick);
    }
  }

  stepFrame(delta: number): void {
    this.stopPlay();
    this.renderAt(Math.min(Math.max(this.frame + delta, 0), this.durationFrames - 1));
  }

  seekTo(f: number): void {
    this.stopPlay();
    this.renderAt(Math.min(Math.max(f, 0), this.durationFrames - 1));
  }

  // ── Document mutations ──────────────────────────────────────────────────
  setTemplate(id: string): boolean {
    return this.updateDoc((doc) => {
      doc.template = id;
    });
  }

  setDirection(dir: Direction): boolean {
    return this.updateDoc((doc) => {
      doc.params = doc.params || ({ ...DEFAULT_PARAMS } as CaptionDocument["params"]);
      doc.params.direction = dir;
    });
  }

  // ── Import / export ─────────────────────────────────────────────────────
  async importSubtitleFile(file: File): Promise<void> {
    if (!this.wasm) return;
    const text = await file.text();
    const format = file.name.toLowerCase().endsWith(".vtt") ? "vtt" : "srt";
    const parsed = JSON.parse(this.wasm.parseSubtitle(text, format)) as {
      cues: Cue[];
      duration_frames: number;
    };
    const doc =
      this.currentDoc() ||
      ({ cues: [], duration_frames: 0, params: { ...DEFAULT_PARAMS } } as CaptionDocument);
    doc.cues = parsed.cues;
    doc.duration_frames = Math.max(doc.duration_frames || 0, parsed.duration_frames);
    this.docText = JSON.stringify(doc, null, 2);
    this.frame = 0;
    await this.applyDocument();
    this.scheduleSave();
    this.setStatus(`imported ${parsed.cues.length} cues from ${file.name}`);
  }

  exportSubtitle(format: "srt" | "vtt"): void {
    if (!this.wasm) return;
    const doc = this.currentDoc();
    if (!doc) return;
    const text = this.wasm.formatSubtitle(JSON.stringify(doc.cues), format);
    this.downloadBlob(
      new Blob([text], { type: "text/plain;charset=utf-8" }),
      `${this.projectName || "captions"}.${format}`,
    );
    this.setStatus(`exported ${doc.cues.length} cues as .${format}`);
  }

  async exportPng(): Promise<void> {
    const canvas = await this.renderToCanvas(this.frame, 1920);
    if (!canvas) return;
    this.lastCanvas = canvas;
    canvas.toBlob((blob) => {
      if (!blob) return;
      this.downloadBlob(blob, `frame-${this.frame}.png`);
      this.setStatus(`exported frame-${this.frame}.png`);
    }, "image/png");
  }

  private downloadBlob(blob: Blob, filename: string): void {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = filename;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 5000);
  }
}

export const store = new StudioState();
