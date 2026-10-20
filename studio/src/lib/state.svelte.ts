import { loadBridge, type LoadedBridge } from "./wasm";
import { splitSvgLayers } from "./svgLayers";
import { transcribeBlob, type TranscribeStage } from "./transcribe";
import { burnInExport, computeOutputSize, type ExportFormat, type ExportQuality, type ExportResolution } from "./export";
import {
  formatTime,
  newProjectId,
  type CaptionDocument,
  type CaptionParams,
  type Cue,
  type Direction,
  type EffectsParams,
  type ProjectMetadata,
  type RscapsProject,
  type TemplateEntry,
  type WhisperBackend,
  type WhisperModel,
  type WordInfo,
  type WordOverride,
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

// Short status-bar text per transcription phase (the full-screen overlay shows
// richer copy; this is the one-line command-bar summary).
const STAGE_STATUS: Record<TranscribeStage, string> = {
  engine: "loading engine…",
  model: "loading model…",
  audio: "decoding audio…",
  transcribe: "transcribing…",
  done: "done",
};

// Caption defaults and the template catalog come straight from caption-core
// through the wasm bridge (one source of truth). Never hardcode them here.

function buildDoc(opts: NewProjectOptions, base: CaptionDocument): CaptionDocument {
  const durationMs = Math.max(1, Math.round(opts.durationSec * 1000));
  return {
    ...base,
    duration_frames: Math.max(1, Math.round(opts.durationSec * opts.fps)),
    cues: [{ start_ms: 0, end_ms: durationMs, text: opts.name || "New project" }],
    template: opts.template || undefined,
    params: { ...base.params, direction: opts.direction },
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
  // Profiler dev: akumulasi ms render_frame bridge (lihat exportprof).
  rfAcc = 0;
  rfN = 0;
  videoWidth = $state(1920);
  videoHeight = $state(1080);
  videoUrl = $state<string | null>(null);
  videoName = $state<string | null>(null);
  backend = $state<WhisperBackend>("cpu");
  model = $state<WhisperModel>("base");
  language = $state("en");
  transcribing = $state(false);
  transcribeFraction = $state(0);
  transcribeStage = $state<TranscribeStage>("engine");
  exporting = $state(false);
  status = $state("idle");
  statusIsError = $state(false);
  /** Save button status (tscaps SaveButtonStatus). */
  saveState = $state<"idle" | "saving" | "saved" | "error">("saved");
  dirty = $state(false);
  docText = $state("");
  svg = $state("");
  fontFaceCss = $state("");
  bootError = $state<string | null>(null);
  /** Engine WASM siap (bootWasm selesai) - gerbang mode preview=export. */
  wasmReady = $state(false);
  /** Template catalog from caption-core (populated on wasm boot). */
  templates = $state<TemplateEntry[]>([]);
  /** Canonical default params (sentinels for template-wins resolution). */
  defaultParams = $state<CaptionParams | null>(null);
  /** Preview SVG per template (dokumen mock) untuk galeri. */
  previews = $state<Record<string, string>>({});
  private previewsBuilt = false;

  // ── Element inspector ───────────────────────────────────────────────────
  /** Index kata global yang terpilih di preview (null = tak ada). */
  selectedWord = $state<number | null>(null);
  /** Sheet aktif yang menerima edit panel ("" = sheet default dokumen). */
  activeSheetId = $state("");
  /**
   * Bingkai layar (px relatif `.video-box`) kata terpilih — dipakai overlay
   * seleksi di PreviewStage. Diukur ulang tiap render (kata bisa bergerak
   * karena animasi).
   */
  wordBox = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  /** Cache daftar kata dokumen aktif (bridge `list_words_json`). */
  words = $state<WordInfo[]>([]);
  /** docText terakhir yang di-sinkronkan ke `words` (kunci invalidasi). */
  private wordsKey = "";

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
  // Raw blob of the currently loaded video, kept in memory so export and
  // transcription don't depend on a fresh IndexedDB read succeeding.
  private videoBlob: Blob | null = null;
  private objectUrl: string | null = null;

  // ── Derived ─────────────────────────────────────────────────────────────
  aspect = $derived(`${this.videoWidth} / ${this.videoHeight}`);
  doc = $derived.by<CaptionDocument | null>(() => {
    try {
      return JSON.parse(this.docText) as CaptionDocument;
    } catch {
      return null;
    }
  });
  params = $derived.by<CaptionParams | null>(() => {
    // Panel menampilkan params target edit: sheet aktif (mewarisi doc saat
    // belum punya params sendiri) atau params dokumen.
    if (this.activeSheet) return this.activeSheet.params ?? this.doc?.params ?? null;
    return this.doc?.params ?? null;
  });
  activeTemplate = $derived.by(() => {
    // Template target edit: sheet aktif punya template sendiri (kosong = v0),
    // sheet default memakai doc.template.
    const id = this.activeSheet ? (this.activeSheet.template ?? "") : (this.doc?.template ?? "");
    return this.templates.find((t) => t.id === id) ?? null;
  });
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
  /** Sheet tambahan dokumen (tanpa sheet default). */
  sheets = $derived(this.doc?.sheets ?? []);
  /** Sheet aktif (null = sheet default dokumen). */
  activeSheet = $derived(this.sheets.find((s) => s.id === this.activeSheetId) ?? null);

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

  /** Canonical default document from caption-core (fresh copy each call). */
  private defaultDocument(): CaptionDocument {
    if (!this.wasm) throw new Error("wasm not ready");
    return structuredClone(this.wasm.defaultDocument);
  }

  private updateDoc(mutate: (doc: CaptionDocument) => void, historyKey?: string): boolean {
    const doc = this.currentDoc();
    if (!doc) return false;
    this.pushHistory(historyKey);
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
      this.wasmReady = true;
      this.fontFaceCss = loaded.fontFaceCss;
      this.templates = loaded.templateCatalog;
      this.defaultParams = loaded.defaultDocument.params;
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
    this.videoBlob = blob;
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
      doc: buildDoc(opts, this.defaultDocument()),
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
    // Register real video dimensions with the engine so templates lay out on
    // the project's own pixel grid (viewBox = width × height).
    if (this.wasm && this.objectUrl) {
      this.wasm.bridge.add_video_source_placeholder(
        opts.videoName || "video",
        this.objectUrl,
        opts.width,
        opts.height,
        opts.durationSec,
      );
    }
    if (this.videoEl) {
      this.videoEl.src = this.videoUrl!;
      this.videoEl.load();
    }
    this.docText = JSON.stringify(project.doc, null, 2);
    this.clearHistory();
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
    this.clearHistory();
    this.frame = 0;
    this.stopPlay();
    const blob = await loadVideo(id);
    if (blob) {
      this.setVideoBlob(blob);
      // Real video dimensions for the engine (see createProject).
      if (this.wasm && this.objectUrl) {
        this.wasm.bridge.add_video_source_placeholder(
          project.videoName || "video",
          this.objectUrl,
          this.videoWidth,
          this.videoHeight,
          this.durationFrames / this.fps,
        );
      }
      if (this.videoEl) {
        this.videoEl.src = this.videoUrl!;
        this.videoEl.load();
      }
    }
    await this.applyDocument();
    this.setStatus(
      blob ? `opened “${project.name}”` : "source video missing — re-import the video file",
      !blob,
    );
  }

  // ── Transcription ───────────────────────────────────────────────────────
  async transcribeCurrent(): Promise<void> {
    if (!this.currentId || this.transcribing) return;
    this.transcribing = true;
    this.transcribeFraction = 0;
    this.transcribeStage = "engine";
    try {
      const blob = this.videoBlob ?? (await loadVideo(this.currentId)) ?? null;
      if (!blob) {
        this.setStatus("no video found to transcribe", true);
        return;
      }
      const result = await transcribeBlob(blob, this.model, this.language, (fraction, stage) => {
        this.transcribeFraction = fraction;
        this.transcribeStage = stage;
        this.setStatus(STAGE_STATUS[stage] ?? stage);
      });
      const cues: Cue[] = result.segments
        .map((s) => ({
          start_ms: Math.max(0, Math.round(s.start * 1000)),
          end_ms: Math.max(0, Math.round(s.end * 1000)),
          text: s.text,
        }))
        .filter(
          (c) =>
            c.end_ms > c.start_ms &&
            // Drop degenerate cues that are only punctuation/whitespace (e.g. a
            // lone "." that whisper emits over near-silence).
            c.text.replace(/[\p{P}\p{S}\s]/gu, "").length > 0,
        );
      if (cues.length === 0) {
        this.setStatus("no speech detected", true);
        return;
      }
      this.updateDoc((doc) => {
        doc.cues = cues;
      });
      this.frame = 0;
      await this.applyDocument();
      await this.saveCurrent();
      await this.refreshProjects();
      this.setStatus(`transcribed ${cues.length} cue${cues.length === 1 ? "" : "s"}`);
    } catch (err) {
      console.error(err);
      this.setStatus(String(err), true);
    } finally {
      this.transcribing = false;
    }
  }

  async saveCurrent(): Promise<void> {
    if (!this.currentId) return;
    this.saveState = "saving";
    try {
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
      this.saveState = "saved";
      this.dirty = false;
    } catch (err) {
      this.saveState = "error";
      throw err;
    }
  }

  scheduleSave(): void {
    if (!this.currentId) return;
    this.dirty = true;
    clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => {
      this.saveCurrent()
        .then(() => this.refreshProjects())
        .catch((err) => console.error(err));
    }, 600);
  }

  // ── Undo / redo (snapshot `docText`) ────────────────────────────────────
  // Dua tumpukan + coalescing waktu: edit serupa beruntun <500ms (tarikan
  // slider, ketikan cue) menyatu jadi satu langkah undo; edit lain
  // menyegel entri sebelumnya. Pola yang sama dengan tscaps UndoRedoStack.
  private undoStack: Array<{ doc: string; key?: string; t: number }> = [];
  private redoStack: string[] = [];
  canUndo = $state(false);
  canRedo = $state(false);
  private static readonly HISTORY_LIMIT = 100;
  private static readonly COALESCE_MS = 500;

  /** Simpan snapshot SEBELUM mutasi. `historyKey` menggabungkan edit
   * beruntun sejenis jadi satu entri. */
  private pushHistory(historyKey?: string): void {
    if (!this.docText) return;
    const now = Date.now();
    const last = this.undoStack.at(-1);
    if (
      historyKey !== undefined &&
      last?.key === historyKey &&
      now - last.t < StudioState.COALESCE_MS
    ) {
      last.t = now;
    } else {
      this.undoStack.push({ doc: this.docText, key: historyKey, t: now });
      if (this.undoStack.length > StudioState.HISTORY_LIMIT) this.undoStack.shift();
    }
    this.redoStack = [];
    this.syncHistoryFlags();
  }

  private clearHistory(): void {
    this.undoStack = [];
    this.redoStack = [];
    this.syncHistoryFlags();
  }

  private syncHistoryFlags(): void {
    this.canUndo = this.undoStack.length > 0;
    this.canRedo = this.redoStack.length > 0;
  }

  undo(): void {
    const entry = this.undoStack.pop();
    if (!entry) return;
    this.redoStack.push(this.docText);
    // Segel entri teratas: edit berikutnya tidak boleh menyatu lintas
    // batas navigasi undo/redo.
    const top = this.undoStack.at(-1);
    if (top) top.key = undefined;
    this.syncHistoryFlags();
    void this.applyHistoryDoc(entry.doc);
  }

  redo(): void {
    const next = this.redoStack.pop();
    if (next === undefined) return;
    this.undoStack.push({ doc: this.docText, t: Date.now() });
    this.syncHistoryFlags();
    void this.applyHistoryDoc(next);
  }

  private async applyHistoryDoc(docText: string): Promise<void> {
    this.docText = docText;
    this.stopPlay();
    await this.applyDocument();
    this.scheduleSave();
  }

  /** Kembalikan Style/Typography/Position/Layout/Effects/Motion ke default
   * template aktif: `params` dikosongkan sehingga render jatuh ke default
   * template.json (tscaps SetTemplateAction semantics). Cue & template tidak
   * disentuh; langkah ini bisa di-undo. */
  async restoreTemplateSettings(): Promise<void> {
    const ok = this.updateDoc((doc) => {
      doc.params = JSON.parse(JSON.stringify(this.defaultParams ?? {})) as CaptionParams;
    });
    if (!ok) return;
    this.applyParams();
    this.scheduleSave();
    this.setStatus("template settings restored");
  }

  async deleteProjectById(id: string): Promise<void> {
    await dbDelete(id);
    if (this.currentId === id) this.currentId = null;
    await this.refreshProjects();
  }

  // ── Rendering ───────────────────────────────────────────────────────────
  /**
   * The engine renders in a fixed 1920×1080 viewBox space (fframes
   * `current_video_size` is hardwired there). The preview must map that box
   * onto the video rect EXACTLY the way export does (`drawImage` stretch),
   * otherwise letterboxing shifts anchors on non-16:9 videos. Inject
   * `preserveAspectRatio="none"` so the SVG fills the box like the export.
   */
  private toPreviewSvg(raw: string): string {
    if (!raw || raw.includes("preserveAspectRatio")) return raw;
    return raw.replace("<svg ", '<svg preserveAspectRatio="none" ');
  }

  renderAt(f: number): void {
    if (!this.wasm) return;
    this.svg = this.toPreviewSvg(this.wasm.bridge.render_frame(BigInt(f)));
    this.frame = f;
    this.syncVideoToFrame(f);
    this.scheduleWordBox();
  }

  // Render caption without touching the video element (used by playback loop).
  private renderCaption(f: number): void {
    if (!this.wasm) return;
    this.svg = this.toPreviewSvg(this.wasm.bridge.render_frame(BigInt(f)));
    this.frame = f;
    this.scheduleWordBox();
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

  // Render one SVG string onto a transparent canvas at an explicit output size.
  private async svgToCanvas(svg: string, outWidth: number, outHeight: number): Promise<HTMLCanvasElement> {
    const inlined = this.fontFaceCss
      ? svg.replace(/^<svg[^>]*>/, (m) => `${m}<style>${this.fontFaceCss}</style>`)
      : svg;
    const uri = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(inlined);
    const img = new Image();
    img.src = uri;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = outWidth;
    canvas.height = outHeight;
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(img, 0, 0, outWidth, outHeight);
    return canvas;
  }

  // Caption frame untuk burn-in: layer dasar + (opsional) layer blend
  // `mix-blend-mode: difference` — dikomposit terhadap video di export.ts.
  private async renderFrameLayers(
    frame: number,
    outWidth: number,
    outHeight: number,
  ): Promise<{ base: HTMLCanvasElement; blend?: HTMLCanvasElement | null } | null> {
    if (!this.wasm) return null;
    const raw = this.wasm.bridge.render_frame(BigInt(frame));
    if (!raw) return null;
    const BLEND_RE = /<text[^>]*style="mix-blend-mode: difference"[^>]*>[\s\S]*?<\/text>/g;
    if (!raw.includes("mix-blend-mode: difference")) {
      return { base: await this.svgToCanvas(raw, outWidth, outHeight) };
    }
    const blendParts = raw.match(BLEND_RE) ?? [];
    const baseSvg = raw.replace(BLEND_RE, "");
    const root = raw.match(/^<svg[^>]*>/)?.[0] ?? "<svg>";
    const blendSvg = `${root}${blendParts.join("")}</svg>`;
    return {
      base: await this.svgToCanvas(baseSvg, outWidth, outHeight),
      blend: await this.svgToCanvas(blendSvg, outWidth, outHeight),
    };
  }

  /**
   * Caption frame sebagai STRING SVG (untuk burn-in via svgr-wasm).
   * Tidak me-raster ke canvas - raster & komposit terjadi di WASM.
   */
  renderFrameSvgLayers(frame: number): { base: string; blend?: string | null } | null {
    if (!this.wasm) return null;
    const rf0 = performance.now();
    const raw = this.wasm.bridge.render_frame(BigInt(frame));
    this.rfAcc += performance.now() - rf0;
    this.rfN += 1;
    return splitSvgLayers(raw);
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
      if (this.activeSheetId) {
        const sheet = doc.sheets?.find((s) => s.id === this.activeSheetId);
        if (sheet) {
          sheet.template = id;
          return;
        }
      }
      doc.template = id;
    });
  }

  setDirection(dir: Direction): boolean {
    return this.updateDoc((doc) => {
      this.editTargetParams(doc).direction = dir;
    });
  }

  // ── Param overrides (panel wiring) ──────────────────────────────────────
  /** Re-render overlay without preparing (cue timing unchanged) or seeking. */
  private applyParams(): void {
    if (!this.wasm) return;
    this.wasm.setDocumentJson(this.docText);
    const f = Math.min(this.frame, Math.max(this.durationFrames - 1, 0));
    this.svg = this.toPreviewSvg(this.wasm.bridge.render_frame(BigInt(f)));
    this.frame = f;
  }

  /**
   * Params object yang sedang diedit panel: sheet aktif (dibuat on-demand,
   * mewarisi doc.params saat pertama disentuh) atau params dokumen.
   */
  private editTargetParams(doc: CaptionDocument): CaptionParams {
    if (this.activeSheetId) {
      doc.sheets = doc.sheets ?? [];
      const sheet = doc.sheets.find((s) => s.id === this.activeSheetId);
      if (sheet) {
        sheet.params =
          sheet.params ??
          (JSON.parse(JSON.stringify(doc.params)) as CaptionParams);
        return sheet.params;
      }
    }
    return doc.params;
  }

  /** Merge a params patch, re-render the overlay, mark the project dirty. */
  setParams(patch: Partial<CaptionParams>): boolean {
    const ok = this.updateDoc((doc) => {
      Object.assign(this.editTargetParams(doc), patch);
    }, "params");
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  /** Merge into `params.effects` (tri-state overrides). */
  setEffectsParams(patch: Partial<EffectsParams>): boolean {
    const ok = this.updateDoc((doc) => {
      const p = this.editTargetParams(doc);
      p.effects = { ...(p.effects ?? {}), ...patch };
    }, "params");
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  /** Set one data-driven style control value (`params.style_values`). */
  setStyleValue(id: string, value: string | number | boolean): boolean {
    const ok = this.updateDoc((doc) => {
      const p = this.editTargetParams(doc);
      p.style_values = { ...(p.style_values ?? {}), [id]: value };
    }, "params");
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  /** Apply a template variant (preset) by merging its overrides. */
  applyVariantOverrides(overrides: Record<string, string | number | boolean>): boolean {
    const ok = this.updateDoc((doc) => {
      const p = this.editTargetParams(doc);
      p.style_values = { ...(p.style_values ?? {}), ...overrides };
    }, "params");
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  // ── Sheets (C7: multi-track gaya) ────────────────────────────────────────
  /** Sheet baru (tanpa template — v0) dan jadikan aktif. */
  addSheet(): boolean {
    const ok = this.updateDoc((doc) => {
      doc.sheets = doc.sheets ?? [];
      const id = `sh-${Date.now().toString(36)}-${Math.floor(Math.random() * 46656).toString(36)}`;
      doc.sheets.push({ id, name: `Sheet ${doc.sheets.length + 1}` });
      this.activeSheetId = id;
    });
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  /** Hapus sheet; cue-nya kembali ke sheet default. */
  removeSheet(id: string): boolean {
    const ok = this.updateDoc((doc) => {
      doc.sheets = (doc.sheets ?? []).filter((s) => s.id !== id);
      for (const cue of doc.cues) {
        if (cue.sheet === id) delete cue.sheet;
      }
    });
    if (ok) {
      if (this.activeSheetId === id) this.activeSheetId = "";
      this.applyDocument().then(() => this.scheduleSave());
    }
    return ok;
  }

  renameSheet(id: string, name: string): boolean {
    const ok = this.updateDoc((doc) => {
      const sheet = doc.sheets?.find((s) => s.id === id);
      if (sheet) sheet.name = name;
    });
    if (ok) {
      this.scheduleSave();
    }
    return ok;
  }

  /** Ganti template sheet ("" = renderer v0). */
  setSheetTemplate(id: string, template: string): boolean {
    const ok = this.updateDoc((doc) => {
      const sheet = doc.sheets?.find((s) => s.id === id);
      if (sheet) sheet.template = template;
    });
    if (ok) {
      this.applyDocument().then(() => this.scheduleSave());
    }
    return ok;
  }

  /** Reset sheet ke inheritance penuh (buang params + template sendiri). */
  resetSheet(id: string): boolean {
    const ok = this.updateDoc((doc) => {
      const sheet = doc.sheets?.find((s) => s.id === id);
      if (sheet) {
        delete sheet.params;
        sheet.template = "";
      }
    });
    if (ok) {
      this.applyDocument().then(() => this.scheduleSave());
    }
    return ok;
  }

  /** Setel sheet aktif yang menerima edit panel ("" = default). */
  setActiveSheet(id: string): void {
    this.activeSheetId = id;
  }

  /** Assign satu cue ke sheet ("" = default). Re-render, timing tetap. */
  assignCue(index: number, sheetId: string): boolean {
    const ok = this.updateDoc((doc) => {
      const cue = doc.cues[index];
      if (!cue) return;
      if (sheetId) cue.sheet = sheetId;
      else delete cue.sheet;
    });
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  /** Setel role sheet ("peak"/"hook"/"" = tanpa role) → kata di cue-nya ikut bertag. */
  setSheetRole(id: string, role: string): boolean {
    const ok = this.updateDoc((doc) => {
      const sheet = doc.sheets?.find((s) => s.id === id);
      if (!sheet) return;
      if (role) sheet.role = role;
      else delete sheet.role;
    });
    if (ok) {
      this.applyDocument().then(() => this.scheduleSave());
    }
    return ok;
  }

  /** Toggle tag manual pada satu kata (index global = listWords). */
  toggleWordTag(wordIndex: number, tag: string): boolean {
    const ok = this.updateDoc((doc) => {
      const key = String(wordIndex);
      doc.word_tags ??= {};
      const list = doc.word_tags[key] ?? [];
      const at = list.indexOf(tag);
      if (at >= 0) list.splice(at, 1);
      else list.push(tag);
      if (list.length > 0) doc.word_tags[key] = list;
      else delete doc.word_tags[key];
      if (Object.keys(doc.word_tags).length === 0) delete doc.word_tags;
    });
    if (ok) {
      this.applyParams();
      this.scheduleSave();
      void this.listWords();
    }
    return ok;
  }

  // ── Element inspector (per-word overrides) ──────────────────────────────
  /** Pilih (atau lepas, `null`) kata di preview — membuka panel Element. */
  selectWord(i: number | null): void {
    this.selectedWord = i;
    if (i === null) {
      this.wordBox = null;
      return;
    }
    // Alignment hitbox butuh `words` — tunggu dulu, baru ukur bingkai.
    void this.listWords().then(() => this.scheduleWordBox());
  }

  /**
   * Ukur ulang bingkai layar kata terpilih dari bbox `<text>` di SVG preview
   * (sama seperti hitTestWord: koordinat layar, di-offset relatif `.video-box`).
   * Dipanggil tiap render + saat memilih; DOM sudah sinkron dengan `this.svg`.
   */
  private scheduleWordBox(): void {
    if (this.selectedWord == null) {
      this.wordBox = null;
      return;
    }
    // SVG di-inject via {@html} — tunggu microtask supaya DOM sudah ada.
    queueMicrotask(() => this.measureWordBox());
  }

  private measureWordBox(): void {
    if (this.selectedWord == null) {
      this.wordBox = null;
      return;
    }
    const scope = document.querySelector(".video-box");
    const svg = scope?.querySelector("svg");
    if (!scope || !svg) {
      this.wordBox = null;
      return;
    }
    const texts = Array.from(svg.querySelectorAll("text"));
    if (texts.length === 0) {
      this.wordBox = null;
      return;
    }
    // Alinea yang sama dengan hitTestWord: label <text> → index stream kata.
    const words = this.words;
    if (words.length === 0) {
      this.wordBox = null;
      return;
    }
    const norm = (s: string) => s.trim().toLowerCase();
    const labels = texts.map((t) => norm(t.textContent ?? ""));
    const tMs = (this.frame / this.fps) * 1000;
    const near = words.findIndex((w) => w.end_ms >= tMs - 2000);
    const from = Math.max(0, (near < 0 ? 0 : near) - 20);
    const align = (start: number): number[] => {
      const out = new Array<number>(labels.length).fill(-1);
      let w = Math.max(0, Math.min(start, words.length));
      for (let i = 0; i < labels.length; i++) {
        if (labels[i] === "") continue;
        let j = -1;
        for (let k = w; k < words.length; k++) {
          if (norm(words[k].text) === labels[i]) {
            j = k;
            break;
          }
        }
        if (j < 0) {
          for (let k = 0; k < words.length; k++) {
            if (norm(words[k].text) === labels[i]) {
              j = k;
              break;
            }
          }
        }
        if (j < 0) continue;
        out[i] = j;
        w = j + 1;
      }
      return out;
    };
    const matched = (a: number[]) => a.reduce((n, v) => (v >= 0 ? n + 1 : n), 0);
    const anchored = align(from);
    const fromStart = align(0);
    const picked = matched(anchored) >= matched(fromStart) ? anchored : fromStart;

    const scopeRect = scope.getBoundingClientRect();
    let box: { x: number; y: number; w: number; h: number } | null = null;
    texts.forEach((t, i) => {
      if (picked[i] !== this.selectedWord) return;
      const r = t.getBoundingClientRect();
      if (r.width <= 0 || r.height <= 0) return;
      box = {
        x: r.left - scopeRect.left,
        y: r.top - scopeRect.top,
        w: r.width,
        h: r.height,
      };
    });
    this.wordBox = box;
  }

  /**
   * Daftar kata dokumen aktif, di-cache per `docText` — refresh otomatis
   * saat dokumen berubah (cue edit/load), dipanggil lazy: saat panel Element
   * dibuka dan saat sebuah kata dipilih.
   */
  async listWords(): Promise<WordInfo[]> {
    await this.bootWasm();
    if (!this.wasm) return this.words;
    if (this.wordsKey === this.docText) return this.words;
    try {
      // Bridge membaca dokumen globalnya — pastikan menunjuk dokumen proyek
      // (bisa saja masih menunjuk dokumen mock galeri template).
      this.wasm.setDocumentJson(this.docText);
      this.words = JSON.parse(this.wasm.listWords()) as WordInfo[];
      this.wordsKey = this.docText;
    } catch (err) {
      console.error(err);
    }
    return this.words;
  }

  /**
   * Merge patch per-kata ke `params.element_overrides` (entry dengan
   * `word == index`, dibuat bila belum ada). Field yang di-set
   * `undefined` / `null` / `""` DIHAPUS (kembali inherit template); bila
   * tak ada field tersisa, entry ikut dibuang.
   */
  setWordOverride(index: number, patch: Partial<WordOverride>): boolean {
    const ok = this.updateDoc((doc) => {
      doc.params = { ...(doc.params ?? this.defaultDocument().params) };
      const list = (doc.params.element_overrides ?? []).map((o) => ({ ...o }));
      const at = list.findIndex((o) => o.word === index);
      const merged: Record<string, unknown> = { ...(at >= 0 ? list[at] : { word: index }) };
      for (const [k, v] of Object.entries(patch)) {
        if (k === "word") continue;
        if (v === undefined || v === null || v === "") delete merged[k];
        else merged[k] = v;
      }
      const filled = Object.entries(merged).some(
        ([k, v]) => k !== "word" && v !== undefined && v !== null && v !== "",
      );
      if (filled) {
        const entry = merged as unknown as WordOverride;
        if (at >= 0) list[at] = entry;
        else list.push(entry);
      } else if (at >= 0) {
        list.splice(at, 1);
      }
      if (list.length > 0) doc.params.element_overrides = list;
      else delete doc.params.element_overrides;
    }, "params");
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  /** Buang semua override satu kata (kembali penuh ke template). */
  clearWordOverride(index: number): boolean {
    const ok = this.updateDoc((doc) => {
      doc.params = { ...(doc.params ?? this.defaultDocument().params) };
      const list = (doc.params.element_overrides ?? []).filter((o) => o.word !== index);
      if (list.length > 0) doc.params.element_overrides = list;
      else delete doc.params.element_overrides;
    }, "params");
    if (ok) {
      this.applyParams();
      this.scheduleSave();
    }
    return ok;
  }

  /**
   * Klik pada preview → index kata terpilih.
   *
   * JUJUR soal mapping: SVG dari engine TIDAK membawa index kata, dan yang
   * dirender bisa SUBSET stream (segmen lintas-cue + limit baris — mis.
   * frame 0 pada sample: 3 kata cue-1 + 3 kata pertama cue-2), jadi “`<text>`
   * ke-n ↔ kata aktif ke-n” salah bila daftar kata aktif dipotong di cue.
   *
   * Yang dipakai: ALINEA SEKUENS — daftar label `<text>` (lowercase) dicocokkan
   * sebagai sub-kurun terurut ke dalam stream `list_words()` (asumsi jujur:
   * urutan DOM `<text>` = urutan stream, karena engine menggambar per baris
   * dalam urutan kata). Start-point alignment di-anchoring ke jendela narasi
   * dekat `t` (dikurangi slack kata) supaya token berulang tidak salah
   * menempel ke cue lain; bila alinea itu kalah cocok, dipakai alinea dari 0.
   * Kata yang labelnya tak terbaca (efek teks mengubah isi) → null.
   */
  async hitTestWord(
    clientX: number,
    clientY: number,
    root?: Element | null,
  ): Promise<number | null> {
    const scope = root ?? document.querySelector(".video-box");
    const svg = scope?.querySelector("svg");
    if (!svg) return null;

    // Hit-test bounding box tiap <text> dalam koordinat layar (SVG di-scale
    // oleh CSS, jadi rect layar sudah memadai — tanpa konversi viewBox).
    let hit = -1;
    let bestArea = Infinity;
    const texts = Array.from(svg.querySelectorAll("text"));
    texts.forEach((t, i) => {
      const r = t.getBoundingClientRect();
      if (r.width <= 0 && r.height <= 0) return;
      if (clientX < r.left || clientX > r.right || clientY < r.top || clientY > r.bottom) return;
      const area = r.width * r.height;
      if (area < bestArea) {
        bestArea = area;
        hit = i;
      }
    });
    if (hit < 0) return null;

    const words = await this.listWords();
    if (words.length === 0) return null;
    const norm = (s: string) => s.trim().toLowerCase();
    const labels = texts.map((t) => norm(t.textContent ?? ""));
    if (labels.every((l) => l === "")) return null;

    const tMs = (this.frame / this.fps) * 1000;
    // Anchor: kata pertama yang jendela narasinya masih dekat `t` (−2s),
    // dilonggarkan 20 kata ke belakang untuk baris yang masih tampil.
    const near = words.findIndex((w) => w.end_ms >= tMs - 2000);
    const from = Math.max(0, (near < 0 ? 0 : near) - 20);

    const align = (start: number): number[] => {
      const out = new Array<number>(labels.length).fill(-1);
      let w = Math.max(0, Math.min(start, words.length));
      for (let i = 0; i < labels.length; i++) {
        if (labels[i] === "") continue;
        let j = -1;
        for (let k = w; k < words.length; k++) {
          if (norm(words[k].text) === labels[i]) {
            j = k;
            break;
          }
        }
        if (j < 0) {
          for (let k = 0; k < words.length; k++) {
            if (norm(words[k].text) === labels[i]) {
              j = k;
              break;
            }
          }
        }
        if (j < 0) continue; // label tak terbaca → -1, posisi w tak maju
        out[i] = j;
        w = j + 1;
      }
      return out;
    };
    const matched = (a: number[]) => a.reduce((n, v) => (v >= 0 ? n + 1 : n), 0);

    const anchored = align(from);
    const fromStart = align(0);
    const picked = matched(anchored) >= matched(fromStart) ? anchored : fromStart;

    let at = picked[hit];
    if (at < 0) {
      // Label tak terbaca → perkirakan dari tetangga terdekat yang ter-map
      // (urutan DOM = urutan stream, jadi jarak indeks ikut berlaku).
      let prev = -1;
      for (let i = hit - 1; i >= 0 && prev < 0; i--) if (picked[i] >= 0) prev = i;
      let next = -1;
      for (let i = hit + 1; i < picked.length && next < 0; i++) if (picked[i] >= 0) next = i;
      if (prev >= 0) at = picked[prev] + (hit - prev);
      else if (next >= 0) at = picked[next] - (next - hit);
      else return null;
    }
    at = Math.min(Math.max(at, 0), words.length - 1);
    return words[at].index;
  }

  /** Switch template: full re-prepare (new splitters/animation timing). */
  async applyTemplate(id: string): Promise<void> {
    if (!this.setTemplate(id)) return;
    await this.applyDocument();
    this.scheduleSave();
  }

  /**
   * Pilih template + RESET style values ke default template baru
   * (perilaku tscaps SetTemplateAction: tak ada carry-over edit manual).
   */
  async applyTemplateFresh(id: string): Promise<void> {
    const ok = this.updateDoc((doc) => {
      doc.template = id;
      doc.params = { ...(doc.params ?? this.defaultDocument().params) };
      delete doc.params.style_values;
    });
    if (!ok) return;
    await this.applyDocument();
    this.scheduleSave();
  }

  /**
   * Render preview statis untuk semua template (galeri): pasang dokumen mock
   * satu kata, render tiap template di puncak narasi, lalu RESTORE dokumen
   * proyek (engine menyimpan satu dokumen global — restore wajib).
   */
  buildTemplatePreviews(): void {
    if (this.previewsBuilt || !this.wasm || this.templates.length === 0) return;
    this.previewsBuilt = true;
    try {
      const mock = this.defaultDocument();
      mock.cues = [{ start_ms: 0, end_ms: 4000, text: "rscaps" }];
      mock.duration_frames = Math.max(this.durationFrames, Math.round(this.fps * 4));
      // Puncak narasi kata: 450ms — entrance selesai, highlight aktif.
      const frame = Math.max(1, Math.round(this.fps * 0.45));
      const out: Record<string, string> = {};
      for (const t of this.templates) {
        mock.template = t.id;
        try {
          this.wasm.setDocumentJson(JSON.stringify(mock));
          out[t.id] = this.wasm.bridge.render_frame(BigInt(frame));
        } catch {
          out[t.id] = "";
        }
      }
      this.previews = out;
    } finally {
      // Restore dokumen + ukuran video proyek (ukuran tidak berubah — hanya
      // dokumen yang ditukar), lalu gambar ulang overlay saat ini.
      this.applyParams();
    }
  }

  // ── Cue editing (Transcript panel) ──────────────────────────────────────
  /** Patch one cue. Timing changes re-prepare (duration), text-only re-renders. */
  async editCue(index: number, patch: Partial<Cue>): Promise<void> {
    const ok = this.updateDoc((doc) => {
      const cue = doc.cues[index];
      if (!cue) return;
      Object.assign(cue, patch);
      if (cue.end_ms < cue.start_ms) cue.end_ms = cue.start_ms;
      doc.cues.sort((a, b) => a.start_ms - b.start_ms || a.end_ms - b.end_ms);
    }, "cue");
    if (!ok) return;
    const timing = patch.start_ms !== undefined || patch.end_ms !== undefined;
    if (timing) {
      await this.applyDocument();
    } else {
      this.applyParams();
    }
    this.scheduleSave();
  }

  async addCue(afterIndex?: number): Promise<void> {
    const ok = this.updateDoc((doc) => {
      const prev = afterIndex !== undefined ? doc.cues[afterIndex] : undefined;
      const start = prev ? prev.end_ms : (doc.cues.at(-1)?.end_ms ?? 0);
      const cue: Cue = { start_ms: start, end_ms: start + 2000, text: "New caption" };
      doc.cues.splice((afterIndex ?? doc.cues.length - 1) + 1, 0, cue);
      doc.duration_frames = Math.max(
        doc.duration_frames,
        Math.ceil((cue.end_ms / 1000) * this.fps),
      );
    });
    if (!ok) return;
    await this.applyDocument();
    this.scheduleSave();
  }

  async removeCue(index: number): Promise<void> {
    const ok = this.updateDoc((doc) => {
      if (doc.cues.length <= 1) {
        doc.cues[0] = { start_ms: 0, end_ms: 1000, text: "" };
        return;
      }
      doc.cues.splice(index, 1);
    });
    if (!ok) return;
    await this.applyDocument();
    this.scheduleSave();
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
      (() => {
        const base = this.defaultDocument();
        base.cues = [];
        return base;
      })();
    doc.cues = parsed.cues;
    doc.duration_frames = Math.max(doc.duration_frames || 0, parsed.duration_frames);
    this.pushHistory();
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

  /** SRT teks untuk panel Code (cues dokumen aktif). */
  codeSrtText(): string {
    if (!this.wasm) return "";
    const doc = this.currentDoc();
    if (!doc) return "";
    try {
      return this.wasm.formatSubtitle(JSON.stringify(doc.cues), "srt");
    } catch {
      return "";
    }
  }

  /** CSS settings untuk panel Code (params → CSS subset). */
  codeParamsCss(): string {
    const doc = this.currentDoc();
    if (!doc || !this.wasm) return "";
    try {
      return this.wasm.paramsToCss(JSON.stringify(doc.params ?? {}));
    } catch {
      return "";
    }
  }

  /**
   * Apply panel Code: SRT (cues) + CSS settings digabung ke dokumen lalu
   * prepare ulang. Captions diedit sebagai SRT; settings sebagai CSS subset
   * yang ditafsir engine (bukan JSON mentah).
   */
  async applyCodePanel(srtText: string, paramsCss: string): Promise<void> {
    await this.bootWasm();
    if (!this.wasm) throw new Error("wasm not ready");
    let cues: Cue[];
    try {
      const parsed = JSON.parse(this.wasm.parseSubtitle(srtText, "srt")) as {
        cues: Cue[];
        duration_frames: number;
      };
      cues = parsed.cues;
    } catch (err) {
      throw new Error(`invalid SRT: ${err}`);
    }
    const base = this.currentDoc();
    const baseParams = JSON.stringify(base?.params ?? this.defaultDocument().params);
    let paramsJson: string;
    try {
      paramsJson = this.wasm.paramsFromCss(baseParams, paramsCss);
    } catch (err) {
      throw new Error(`invalid settings CSS: ${err}`);
    }
    const params = JSON.parse(paramsJson) as CaptionParams;
    const ok = this.updateDoc((doc) => {
      doc.cues = cues;
      doc.params = params;
    }, "code");
    if (!ok) throw new Error("invalid document");
    await this.applyDocument();
    this.scheduleSave();
    this.setStatus(`applied ${cues.length} cues + CSS settings`);
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

  // ── Project export (works from Home without opening the project) ────────
  async exportProjectBurnIn(
    projectId: string,
    opts: { format: ExportFormat; quality: ExportQuality; resolution: ExportResolution },
  ): Promise<void> {
    if (this.exporting) return;
    this.exporting = true;
    try {
      await this.bootWasm();
      if (!this.wasm) throw new Error("wasm not ready");
      const project = await getProject(projectId);
      if (!project) throw new Error("project not found");

      // When exporting the project open in the editor, prefer the in-memory
      // video blob (already loaded on screen) over a fresh IndexedDB read, and
      // use the LIVE (possibly unsaved) document so the export matches what's
      // on screen. Otherwise fall back to the persisted doc + stored video.
      const isCurrent = projectId === this.currentId;
      let blob: Blob | null = isCurrent ? this.videoBlob : null;
      if (!blob) blob = (await loadVideo(projectId)) ?? null;
      if (!blob) {
        throw new Error(
          "source video missing for this project — re-import the video file to export",
        );
      }

      const docJson = isCurrent ? this.docText : JSON.stringify(project.doc);
      const fps = (isCurrent ? this.fps : project.settings.fps) || 30;
      const srcW = (isCurrent ? this.videoWidth : project.settings.width) || 1920;
      const srcH = (isCurrent ? this.videoHeight : project.settings.height) || 1080;

      // prepare() membangun kembali timeline & MERESSET dokumen bridge, jadi
      // setDocumentJson HARUS dijalankan SETELAH placeholder + prepare -
      // kalau tidak, render_frame mengembali svg path kosong (regresi 2026-10).
      const durationSec = ((isCurrent ? this.durationFrames : project.doc.duration_frames) || 1) / fps;
      this.wasm.bridge.add_video_source_placeholder(
        project.videoName || "video",
        "",
        srcW,
        srcH,
        durationSec,
      );
      await this.wasm.bridge.prepare(fps);
      this.wasm.setDocumentJson(docJson);

      const { width, height } = computeOutputSize(srcW, srcH, opts.resolution);
      const out = await burnInExport({
        videoBlob: blob,
        sourceWidth: srcW,
        sourceHeight: srcH,
        fps,
        format: opts.format,
        quality: opts.quality,
        resolution: opts.resolution,
        renderCaption: async (frame) => this.renderFrameSvgLayers(frame),
        onProgress: (frac, stage) =>
          this.setStatus(`${stage} ${(frac * 100).toFixed(0)}%`),
        // A/B paritas: ?exportmode=legacy → raster canvas browser (jalur lama).
        legacyCanvas: new URLSearchParams(location.search).get("exportmode") === "legacy",
        fontFaceCss: this.fontFaceCss,
      });
      const ext = opts.format === "webm" ? "webm" : "mp4";
      if (import.meta.env.DEV && this.rfN > 0) {
        console.log(
          `[exportprof] render_frame=${(this.rfAcc / this.rfN).toFixed(3)}ms x${this.rfN}`,
        );
      }
      console.log("[exportstage] downloadBlob start");
      this.downloadBlob(out, `${project.name || "export"}.${ext}`);
      console.log("[exportstage] downloadBlob end");
      this.setStatus(`exported ${project.name}.${ext}`);
    } catch (err) {
      console.error(err);
      this.setStatus(String(err), true);
    } finally {
      this.exporting = false;
    }
  }

  async exportProjectSubtitle(projectId: string, format: "srt" | "vtt"): Promise<void> {
    try {
      await this.bootWasm();
      if (!this.wasm) throw new Error("wasm not ready");
      const project = await getProject(projectId);
      if (!project) throw new Error("project not found");
      const text = this.wasm.formatSubtitle(JSON.stringify(project.doc.cues), format);
      this.downloadBlob(
        new Blob([text], { type: "text/plain;charset=utf-8" }),
        `${project.name || "captions"}.${format}`,
      );
      this.setStatus(`exported ${project.doc.cues.length} cues as .${format}`);
    } catch (err) {
      console.error(err);
      this.setStatus(String(err), true);
    }
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
