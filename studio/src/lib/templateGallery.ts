// Galeri template — layout & state UI (clean-room dari perilaku tscaps).
// Data template sendiri (id/label/category/styleControls/…) datang dari
// caption-core lewat bridge; tabel kategori di sini = domain UI (label,
// urutan section, anggaran baris) persis pola tscaps.

import type { TemplateEntry } from "./types";

export type CategoryId = "modern" | "key-moments" | "viral" | "classic" | "lab";

export interface CategoryDef {
  label: string;
  /** Baris default sebelum "View all". */
  previewRows: number;
}

/** Urutan key = urutan tampil section. */
export const TEMPLATE_CATEGORIES: Record<CategoryId, CategoryDef> = {
  modern: { label: "Modern", previewRows: 3 },
  "key-moments": { label: "Key moments", previewRows: 1 },
  viral: { label: "Viral", previewRows: 2 },
  classic: { label: "Classic", previewRows: 2 },
  lab: { label: "Lab", previewRows: 2 },
};

export interface GallerySection {
  id: string; // "favorites" | kategori
  label: string;
  templates: TemplateEntry[];
  previewRows: number;
}

export function isCategoryId(value: string): value is CategoryId {
  return Object.prototype.hasOwnProperty.call(TEMPLATE_CATEGORIES, value);
}

/**
 * Susunan section: Favorites dulu (kalau tidak kosong), lalu per-kategori
 * sesuai urutan tabel. Favorit mengambang ke atas di keluarganya sendiri;
 * section kosong dibuang.
 */
export function buildSections(
  templates: TemplateEntry[],
  favoriteIds: ReadonlySet<string>,
): GallerySection[] {
  const sections: GallerySection[] = [];

  const favorites = favoritesFirst(templates, favoriteIds).filter((t) =>
    favoriteIds.has(t.id),
  );
  if (favorites.length > 0) {
    sections.push({
      id: "favorites",
      label: "Favorites",
      templates: favorites,
      previewRows: 2,
    });
  }

  for (const [id, def] of Object.entries(TEMPLATE_CATEGORIES)) {
    const family = favoritesFirst(
      templates.filter((t) => (t.category ?? "lab") === id),
      favoriteIds,
    );
    if (family.length > 0) {
      sections.push({
        id,
        label: def.label,
        templates: family,
        previewRows: def.previewRows,
      });
    }
  }
  return sections;
}

/** Pencarian substring sederhana (polа tscaps: favorit dulu di dalam hasil). */
export function searchTemplates(
  templates: TemplateEntry[],
  favoriteIds: ReadonlySet<string>,
  query: string,
): TemplateEntry[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [];
  return favoritesFirst(
    templates.filter((t) =>
      `${t.id} ${t.label}`.toLowerCase().includes(needle),
    ),
    favoriteIds,
  );
}

function favoritesFirst(
  templates: TemplateEntry[],
  favoriteIds: ReadonlySet<string>,
): TemplateEntry[] {
  return [...templates].sort(
    (a, b) =>
      Number(favoriteIds.has(b.id)) - Number(favoriteIds.has(a.id)),
  );
}

// ── Favorit (localStorage — pola repo rscaps, bukan IndexedDB) ─────────────

const FAVORITES_KEY = "rscaps:template-favorites";
export function loadFavorites(): Set<string> {
  try {
    const raw = localStorage.getItem(FAVORITES_KEY);
    if (!raw) return new Set();
    const list = JSON.parse(raw) as unknown;
    if (Array.isArray(list)) {
      return new Set(list.filter((x): x is string => typeof x === "string"));
    }
  } catch {
    /* ignore */
  }
  return new Set();
}

export function saveFavorites(ids: ReadonlySet<string>): void {
  try {
    localStorage.setItem(FAVORITES_KEY, JSON.stringify([...ids]));
  } catch {
    /* ignore */
  }
}

// ── Crop preview ke sekitar teks caption ───────────────────────────────────

/**
 * SVG frame penuh → SVG yang di-crop ke bounding box teks (dipaksa ke rasio
 * kartu), supaya caption terbaca di kartu ~110px. Heuristik: kumpulkan posisi
 * `<text>` (x,y) + perkiraan tinggi font dari selisih baseline; tanpa teks →
 * kembalikan string asli (kartu menampilkan checker saja bila kosong).
 */
export function cropPreviewSvg(svg: string, aspect = 2): string {
  if (!svg || !svg.includes("<text")) return svg;
  const vb = /viewBox="(-?[\d.]+) (-?[\d.]+) ([\d.]+) ([\d.]+)"/.exec(svg);
  if (!vb) return svg;
  const fw = parseFloat(vb[3]);
  const fh = parseFloat(vb[4]);

  const xs: number[] = [];
  const ys: number[] = [];
  const lens: number[] = [];
  for (const m of svg.matchAll(/<text[^>]*\bx="(-?[\d.]+)"[^>]*\by="(-?[\d.]+)"[^>]*>([^<]*)</g)) {
    xs.push(parseFloat(m[1]));
    ys.push(parseFloat(m[2]));
    lens.push(m[3].length);
  }
  if (xs.length === 0) return svg;

  // Tinggi font: jarak antar baseline bila >1 baris, fallback 6.5% tinggi kanvas.
  let font = fh * 0.065;
  if (ys.length > 1) {
    const sorted = [...new Set(ys)].sort((a, b) => a - b);
    if (sorted.length > 1) {
      const gaps = sorted.slice(1).map((v, i) => v - sorted[i]).filter((g) => g > 1);
      if (gaps.length > 0) font = Math.min(...gaps);
    }
  }

  let minX = Infinity;
  let maxX = -Infinity;
  for (let i = 0; i < xs.length; i++) {
    minX = Math.min(minX, xs[i]);
    maxX = Math.max(maxX, xs[i] + lens[i] * font * 0.62);
  }
  const minY = Math.min(...ys) - font * 0.85;
  const maxY = Math.max(...ys) + font * 0.3;
  const pad = font * 0.45;

  let cx = (minX + maxX) / 2;
  let cy = (minY + maxY) / 2;
  let cw = maxX - minX + pad * 2;
  let ch = maxY - minY + pad * 2;
  // Paksa rasio kartu (lubangi sisi yang kurang).
  if (cw / ch < aspect) cw = ch * aspect;
  else ch = cw / aspect;
  // Jaga tetap di dalam kanvas bila mungkin.
  cw = Math.min(cw, fw * 1.5);
  ch = Math.min(ch, fh * 1.5);
  cx = Math.min(Math.max(cx, cw / 2 - fw * 0.25), fw - cw / 2 + fw * 0.25);
  cy = Math.min(Math.max(cy, ch / 2 - fh * 0.25), fh - ch / 2 + fh * 0.25);

  const crop = `${(cx - cw / 2).toFixed(1)} ${(cy - ch / 2).toFixed(1)} ${cw.toFixed(1)} ${ch.toFixed(1)}`;
  return svg.replace(/viewBox="[^"]*"/, `viewBox="${crop}"`);
}
