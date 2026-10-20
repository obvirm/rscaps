export type Direction = "auto" | "ltr" | "rtl";

export interface Cue {
  start_ms: number;
  end_ms: number;
  text: string;
  /** Sheet id pemilik cue (absen/kosong = sheet default dokumen). */
  sheet?: string;
}

/** Sheet (track gaya) — mirror dari caption-core::Sheet. */
export interface Sheet {
  id: string;
  name: string;
  template?: string;
  params?: CaptionParams;
  /** Role semantik: "peak"/"hook" — kata di cue sheet ini ikut bertag. */
  role?: string;
}

/** Effects overrides — tri-state: absent = template default, else force. */
export interface EffectsParams {
  gap_free?: boolean;
  remove_punctuation?: boolean;
  smart_punctuation?: boolean;
  carry_quotes?: boolean;
  smart_lowercase?: boolean;
  emoji_placement?: string;
  emoji_size?: number;
  emoji_gap?: number;
}

/**
 * Satu override elemen kata (element inspector) — mirror `WordOverride` di
 * `crates/caption-core/src/lib.rs`. Field opsional: tak ada = inherit
 * template. `css` menang atas field terstruktur.
 */
export interface WordOverride {
  /** Index kata global (urutan dokumen, stream sebelum effects). */
  word: number;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
  font_family?: string;
  font_weight?: number;
  color?: string;
  /** Skala ukuran relatif base template (1.0 = tak berubah). */
  size_scale?: number;
  rotation_deg?: number;
  /** Anchor pusat kata: fraksi lebar/tinggi frame (absent = layout asli). */
  h_frac?: number;
  v_frac?: number;
  /** `"none"|"fade"|"rise"|"slide"|"scale"|"pop"|"snap"|"wobble"|"flash"`. */
  animation?: string;
  animation_duration_s?: number;
  css?: string;
}

/** Entry `list_words_json()` — daftar kata dokumen aktif. */
export interface WordInfo {
  index: number;
  text: string;
  start_ms: number;
  end_ms: number;
  /** Tag semantik resolved (quote/number otomatis, peak/hook role sheet,
   * manual dari doc.word_tags) — dari bridge listWords. */
  tags: string[];
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
  // Typography overrides (absent = template default)
  letter_spacing_em?: number;
  word_spacing_em?: number;
  line_spacing_em?: number;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
  text_case?: string;
  text_align?: string;
  // Position overrides
  h_align?: string;
  h_offset?: number;
  v_align?: string;
  v_offset?: number;
  angle_deg?: number;
  // Motion overrides
  motion_preset?: string;
  motion_duration_ms?: number;
  motion_distance_em?: number;
  // Effects (tri-state)
  effects?: EffectsParams;
  // Layout overrides
  max_lines?: number;
  max_width_frac?: number;
  max_chars?: number;
  // Data-driven style control values, keyed by control id
  style_values?: Record<string, string | number | boolean>;
  // Element inspector — per-word overrides (absent = no override)
  element_overrides?: WordOverride[];
}

export interface CaptionDocument {
  duration_frames: number;
  cues: Cue[];
  params: CaptionParams;
  template?: string;
  /** Sheet tambahan (track gaya); sheet default = dokumen sendiri. */
  sheets?: Sheet[];
  /** Tag manual per kata (key = index kata global dari listWords). */
  word_tags?: Record<string, string[]>;
}

/** Tag yang bisa ditempel manual pada kata (urutan tscaps WORD_TAG_NAMES).
 * quote/number juga terdeteksi otomatis; sisanya murni manual. */
export const WORD_TAG_OPTIONS: { id: string; label: string }[] = [
  { id: "number", label: "Number" },
  { id: "quote", label: "Quote" },
  { id: "emphasis", label: "Emphasis" },
  { id: "accent", label: "Accent" },
  { id: "entity", label: "Entity" },
];

// ── Template catalog (straight from caption-core) ─────────────────────────

export type StyleControlType =
  | "color"
  | "integer"
  | "float"
  | "select"
  | "toggle"
  | "text"
  | "image"
  | "font";

export interface StyleControl {
  id: string;
  label: string;
  type: StyleControlType;
  default?: string | number | boolean;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  group?: string;
  subgroup?: string;
  options?: Array<{ label: string; [key: string]: unknown }>;
  legend?: string;
  valueOn?: string | number;
  valueOff?: string | number;
}

export interface TemplateVariant {
  label: string;
  overrides: Record<string, string | number | boolean>;
}

/** template.json defaults (per bagian) — untuk nilai efektif panel. */
export interface TemplateDefaults {
  typography?: Record<string, unknown>;
  alignment?: {
    verticalAlign?: string;
    verticalOffset?: number;
    horizontalAlign?: string;
    horizontalOffset?: number;
  };
  rendering?: Record<string, unknown>;
  segmentSplitters?: Array<Record<string, unknown>>;
  lineSplitter?: Record<string, unknown>;
  features?: Record<string, unknown>;
  effects?: Array<Record<string, unknown>>;
}

/** One entry of the template catalog exported by caption-core. */
export interface TemplateEntry {
  id: string;
  label: string;
  category?: string;
  galleryOrder?: number;
  styleControls?: StyleControl[];
  variants?: TemplateVariant[];
  defaults?: TemplateDefaults;
}

export interface PrepareMeta {
  durationInFrames: number;
  fps: number;
}

export interface ParsedSubtitle {
  cues: Cue[];
  duration_frames: number;
}

export interface WasmBridge {
  render_frame(frame: bigint): string;
  prepare(fps: number): Promise<PrepareMeta>;
  ingest_font(bytes: Uint8Array): unknown;
  add_video_source_placeholder(
    file: string,
    url: string,
    width: number,
    height: number,
    duration: number,
  ): void;
}

// ── Project model ─────────────────────────────────────────────────────────

export type WhisperBackend = "cpu" | "webgpu";
export type WhisperModel = "tiny" | "base" | "small" | "medium" | "large";

export const WHISPER_MODELS: { id: WhisperModel; label: string }[] = [
  { id: "tiny", label: "Tiny — ~152 MB" },
  { id: "base", label: "Base — ~75 MB" },
  { id: "small", label: "Small — ~923 MB" },
  // Disabled: fp32 decoder+encoder 2.9 GB blows the wasm32 4 GB linear-memory
  // ceiling once sessions + KV caches are resident (ort-web loads from memory).
  { id: "medium", label: "Medium — ~2.9 GB" },
  // Disabled: Xenova ships large-v3 as external-data onnx (+5.9 GB) which
  // commit_from_memory cannot resolve, beyond the same wasm32 ceiling.
  { id: "large", label: "Large — ~5.9 GB" },
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
