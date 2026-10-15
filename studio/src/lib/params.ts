import { FONT_FAMILIES } from "./wasm";
import type { CaptionParams, EffectsParams, TemplateDefaults } from "./types";

const eps = 1e-4;

/** template.json fontStack id ("komika-axis") → display family ("Komika Axis"). */
export function stackToFamily(stack: string | undefined | null): string | null {
  if (!stack) return null;
  const norm = stack.toLowerCase().replace(/-/g, " ");
  const hit = FONT_FAMILIES.find((f) => f.toLowerCase() === norm);
  if (hit) return hit;
  const partial = FONT_FAMILIES.find(
    (f) => f.toLowerCase().includes(norm) || norm.includes(f.toLowerCase()),
  );
  return partial ?? null;
}

function typo(defaults?: TemplateDefaults): Record<string, unknown> {
  return defaults?.typography ?? {};
}

export function effFontFamily(
  params: CaptionParams | null,
  def: CaptionParams | null,
  defaults?: TemplateDefaults,
): string {
  if (params && def && params.font_family !== def.font_family) return params.font_family;
  return stackToFamily(typo(defaults).fontStack as string | undefined) ?? params?.font_family ?? "";
}

export function effFontSizeCqh(
  params: CaptionParams | null,
  def: CaptionParams | null,
  defaults: TemplateDefaults | undefined,
  videoH: number,
): number {
  if (params && def && Math.abs(params.font_size - def.font_size) > eps) {
    return Number(((params.font_size / Math.max(videoH, 1)) * 100).toFixed(2));
  }
  const v = typo(defaults).fontSize;
  return typeof v === "number" ? v : 4.5;
}

export function effFontWeight(
  params: CaptionParams | null,
  def: CaptionParams | null,
  defaults?: TemplateDefaults,
): number {
  if (params && def && params.font_weight !== def.font_weight) return params.font_weight;
  const v = typo(defaults).fontWeight;
  return typeof v === "number" ? v : (params?.font_weight ?? 400);
}

export function effTextCase(params: CaptionParams | null, defaults?: TemplateDefaults): string {
  if (params?.text_case) return params.text_case;
  const v = typo(defaults).textCase;
  return typeof v === "string" ? v : "none";
}

export function effTextAlign(params: CaptionParams | null, defaults?: TemplateDefaults): string {
  if (params?.text_align) return params.text_align;
  const v = typo(defaults).textAlign;
  return typeof v === "string" ? v : "center";
}

export function effNumber(
  override: number | undefined,
  fallback: unknown,
  fallbackDefault: number,
): number {
  if (override !== undefined) return override;
  return typeof fallback === "number" ? fallback : fallbackDefault;
}

/** Absolute anchor (0..1, from left/top) — mirrors overrides::resolve. */
export function effAnchorX(params: CaptionParams | null, defaults?: TemplateDefaults): number {
  const align = defaults?.alignment;
  const defX = resolveDefX(align);
  if (params?.h_align === "left") return clamp01(params.h_offset ?? 0);
  if (params?.h_align === "right") return 1 - clamp01(params.h_offset ?? 0);
  if (params?.h_align === "center") return 0.5;
  if (params?.h_offset !== undefined) return clamp01(params.h_offset);
  return defX;
}

export function effAnchorY(
  params: CaptionParams | null,
  defaults?: TemplateDefaults,
  def?: CaptionParams | null,
): number {
  const align = defaults?.alignment;
  const defY = resolveDefY(align);
  if (params?.v_align === "top") return clamp01(params.v_offset ?? 0);
  if (params?.v_align === "bottom") return 1 - clamp01(params.v_offset ?? 0);
  if (params?.v_align === "center") return 0.5;
  if (params?.v_offset !== undefined) return clamp01(params.v_offset);
  const sentinel = def?.y_frac ?? 0.78;
  if (params && Math.abs(params.y_frac - sentinel) > eps) return clamp01(params.y_frac);
  return defY;
}

function clamp01(v: number): number {
  return Math.min(Math.max(v, 0), 1);
}

function resolveDefX(align: TemplateDefaults["alignment"] | undefined): number {
  // `horizontalOffset` = titik anchor absolut (mengukur dari kiri; `center`
  // selalu 0.5, `right` = 1-offset) — SEMANTIK tscaps (xPx = resolver(...)),
  // sama dengan anchor_x konst per-template di engine. JANGAN ubah jadi
  // "align menentukan sisi" — itu hanya menentu sisi BLOK, bukan titik.
  if (!align) return 0.5;
  const off = align.horizontalOffset ?? 0.5;
  if (align.horizontalAlign === "right") return 1 - clamp01(off);
  if (align.horizontalAlign === "center") return 0.5;
  return clamp01(off); // left / tak ada align
}

function resolveDefY(align: TemplateDefaults["alignment"] | undefined): number {
  // `verticalOffset` = titik anchor absolut dari ATAS (tscaps: yPx =
  // verticalOffset×height; verticalAlign hanya menempelkan sisi blok di
  // titik itu). Engine menyimpan anchor_y konst = nilai ini mentah.
  if (!align) return 0.75;
  return clamp01(align.verticalOffset ?? 0.75);
}

// ── Effects ───────────────────────────────────────────────────────────────

const EFFECT_KEYS = [
  "gap_free",
  "remove_punctuation",
  "smart_punctuation",
  "carry_quotes",
  "smart_lowercase",
] as const;

function templateEffect(
  defaults: TemplateDefaults | undefined,
  type: string,
): Record<string, unknown> | undefined {
  return defaults?.effects?.find((e) => e.type === type);
}

export function effEffectFlag(
  params: CaptionParams | null,
  defaults: TemplateDefaults | undefined,
  key: (typeof EFFECT_KEYS)[number],
): boolean {
  const override = params?.effects?.[key];
  if (typeof override === "boolean") return override;
  const tpl = templateEffect(defaults, key);
  return tpl?.enabled === true;
}

export function effEmoji(
  params: CaptionParams | null,
  defaults: TemplateDefaults | undefined,
): { placement: string; size: number; gap: number } {
  const tpl = templateEffect(defaults, "emoji") ?? {};
  const p: EffectsParams | undefined = params?.effects;
  return {
    placement: p?.emoji_placement ?? (typeof tpl.placement === "string" ? tpl.placement : "segment-below"),
    size: p?.emoji_size ?? (typeof tpl.size === "number" ? tpl.size : 1.8),
    gap: p?.emoji_gap ?? (typeof tpl.gap === "number" ? tpl.gap : 0.1),
  };
}

export { EFFECT_KEYS };

// ── Layout ────────────────────────────────────────────────────────────────

export function effLayout(
  params: CaptionParams | null,
  defaults: TemplateDefaults | undefined,
): { maxChars: number; minChars: number; maxLines: number; maxWidthFrac: number } {
  const splitter =
    defaults?.segmentSplitters?.find(
      (s) => s.type === "boundary_score_limit_by_chars" || s.type === "limit_by_chars",
    ) ?? {};
  const line = defaults?.lineSplitter ?? {};
  return {
    maxChars: params?.max_chars ?? (typeof splitter.maxChars === "number" ? splitter.maxChars : 22),
    minChars: typeof splitter.minChars === "number" ? splitter.minChars : 0,
    maxLines: params?.max_lines ?? (typeof line.maxLines === "number" ? line.maxLines : 2),
    maxWidthFrac:
      params?.max_width_frac ??
      (typeof line.maxWidthRatio === "number" ? line.maxWidthRatio : 0.9),
  };
}
