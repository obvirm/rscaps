export type Direction = "auto" | "ltr" | "rtl";

export interface Cue {
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface CaptionParams {
  font_family: string;
  font_size: number;
  font_weight: number;
  color: string;
  y_frac: number;
  width_frac: number;
  pop_in_ms: number;
  travel_px: number;
  direction: Direction;
}

export interface CaptionDocument {
  duration_frames: number;
  cues: Cue[];
  params: CaptionParams;
  template?: string;
}

export interface PrepareMeta {
  durationInFrames: number;
  fps: number;
}

export interface ParsedSubtitle {
  cues: Cue[];
  duration_frames: number;
}

/** One entry of the template catalog exported by caption-core. */
export interface TemplateEntry {
  id: string;
  label: string;
}

export interface WasmBridge {
  render_frame(frame: bigint): string;
  prepare(fps: number): Promise<PrepareMeta>;
  ingest_font(bytes: Uint8Array): unknown;
}

// ── Project model ─────────────────────────────────────────────────────────

export type WhisperBackend = "cpu" | "webgpu";
export type WhisperModel = "tiny" | "base" | "small" | "medium" | "large";

export const WHISPER_MODELS: { id: WhisperModel; label: string }[] = [
  { id: "tiny", label: "Tiny — ~39 MB" },
  { id: "base", label: "Base — ~75 MB" },
  { id: "small", label: "Small — ~244 MB" },
  { id: "medium", label: "Medium — ~769 MB" },
  { id: "large", label: "Large — ~1.5 GB" },
];

// Whisper language token ids run in a fixed alphabetical order starting at
// <|en|> = 50259. This is that order (index 0 = en). We expose a curated
// subset in the UI but map by index so any code resolves correctly.
const WHISPER_LANG_ORDER =
  "en zh de es ru ko fr ja pt tr pl ca nl ar sv it id hi fi vi he uk el ms cs ro da hu ta no th ur hr bg lt la mi ml cy sk te fa lv bn sr az sl kn et mk br eu hy ne mn bs kk sq sw gl mr pa si km sn yo so af oc ka be tg sd gu am yi lo uz fo ht ps tk nn mt sa lb my bo tl mg as tt haw ln ha ba jw su yue".split(
    " ",
  );

export const WHISPER_LANGUAGES: { id: string; label: string }[] = [
  { id: "en", label: "English" },
  { id: "id", label: "Indonesian" },
  { id: "ms", label: "Malay" },
  { id: "ar", label: "Arabic" },
  { id: "zh", label: "Chinese" },
  { id: "ja", label: "Japanese" },
  { id: "ko", label: "Korean" },
  { id: "hi", label: "Hindi" },
  { id: "ur", label: "Urdu" },
  { id: "fa", label: "Persian" },
  { id: "he", label: "Hebrew" },
  { id: "tr", label: "Turkish" },
  { id: "ru", label: "Russian" },
  { id: "es", label: "Spanish" },
  { id: "fr", label: "French" },
  { id: "de", label: "German" },
  { id: "pt", label: "Portuguese" },
  { id: "it", label: "Italian" },
  { id: "nl", label: "Dutch" },
  { id: "pl", label: "Polish" },
  { id: "vi", label: "Vietnamese" },
  { id: "th", label: "Thai" },
];

export function langToTokenId(code: string): number {
  const idx = WHISPER_LANG_ORDER.indexOf(code);
  return idx >= 0 ? 50259 + idx : 50259; // default <|en|>
}

export interface ProjectSettings {
  fps: number;
  width: number;
  height: number;
  backend: WhisperBackend;
  model: WhisperModel;
}

export interface RscapsProject {
  id: string;
  name: string;
  createdAt: string; // ISO
  updatedAt: string; // ISO
  doc: CaptionDocument;
  settings: ProjectSettings;
  videoName?: string;
  thumbnail?: string; // dataURL of a rendered preview frame
}

export interface ProjectMetadata {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  thumbnail?: string;
  durationSec: number;
  fps: number;
  width: number;
  height: number;
  videoName?: string;
}

// Minimal reference the export dialog needs; ProjectMetadata satisfies it.
export interface ExportProjectRef {
  id: string;
  name: string;
  width: number;
  height: number;
  fps: number;
}

export function newProjectId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `p_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

export function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const cs = Math.floor((seconds % 1) * 100);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(cs).padStart(2, "0")}`;
}

export function formatTimeShort(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}
