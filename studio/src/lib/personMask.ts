/// Klien segmentasi orang + cache masker (mirror MaskCache tscaps): nearest
/// dalam toleransi 0.15s. Analisis = seek video tersembunyi per sampel
/// (maskFps), segment di worker, hasil dipakai preview/export pastor.
const MASK_FPS = 10;
const MASK_TOLERANCE_SEC = 0.15;
/** Fraksi piksel "person" (alpha>128) minimal agar frame dianggap berisi orang. */
const PERSON_PIXEL_FRACTION = 0.03;
/** Cue aktif behind bila ≥85% sampelnya berisi orang. */
const CUE_PERSON_RATIO = 0.85;

export interface PersonMask {
  t: number;
  alpha: Uint8Array;
  width: number;
  height: number;
}

type WorkerOutbound =
  | { type: "ready" }
  | { type: "error"; id: number; message: string }
  | { type: "mask"; id: number; t: number; alpha: Uint8Array; width: number; height: number };

export class PersonMaskClient {
  private worker: Worker | null = null;
  private pending = new Map<number, {
    resolve: (m: PersonMask) => void;
    reject: (e: Error) => void;
  }>();
  private nextId = 1;
  /** Progress analisis 0..1; -1 = tidak berjalan; 1 = selesai (poll manual). */
  progress = -1;
  /** Sampel masker terurut waktu (dari analisis terakhir). */
  entries: PersonMask[] = [];
  /** Progress analisis 0..1; -1 = tidak berjalan; 1 = selesai (poll manual). */

  private workerUrl: string | null = null;

  private async ensureWorker(): Promise<Worker> {
    if (!this.worker) {
      // CLASSIC worker via Blob: runtime MediaPipe memanggil importScripts,
      // dilarang di module worker — dan vite memaksa type:module untuk
      // pola `new Worker(new URL(...))`. Scriptnya di-fetch (sudah
      // di-transform TS→JS oleh vite, tanpa import/export), dibungkus
      // Blob, lalu dibangun sebagai worker klasik.
      if (!this.workerUrl) {
        const src = await fetch(
          new URL("./personMaskWorker.ts", import.meta.url),
        ).then((r) => {
          if (!r.ok) throw new Error(`worker fetch gagal: ${r.status}`);
          return r.text();
        });
        this.workerUrl = URL.createObjectURL(
          new Blob([src], { type: "text/javascript" }),
        );
      }
      this.worker = new Worker(this.workerUrl);
      this.worker.onmessage = (event: MessageEvent<WorkerOutbound>) => {
        const msg = event.data;
        if (msg.type === "mask") {
          const p = this.pending.get(msg.id);
          if (p) {
            this.pending.delete(msg.id);
            p.resolve({ t: msg.t, alpha: msg.alpha, width: msg.width, height: msg.height });
          }
        } else if (msg.type === "error") {
          const p = this.pending.get(msg.id);
          if (p) {
            this.pending.delete(msg.id);
            p.reject(new Error(msg.message));
          }
        }
      };
      this.worker.onerror = (e) => {
        // Gagalkan semua pending (init/model/wasm rusak) — host fallback quiet.
        for (const [, p] of this.pending) p.reject(new Error(e.message || "mask worker error"));
        this.pending.clear();
      };
    }
    return this.worker;
  }

  async segment(bitmap: ImageBitmap, timestampMs: number, t: number): Promise<PersonMask> {
    const w = await this.ensureWorker();
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      w.postMessage({ type: "segment", id, bitmap, timestampMs, t }, [bitmap]);
    });
  }

  /** Sampel masker di seluruh durasi via video tersembunyi (tanpa ganggu preview). */
  async analyze(videoUrl: string, durationSec: number): Promise<PersonMask[]> {
    this.progress = 0;
    this.entries = [];
    const timestamps: number[] = [];
    const step = 1 / MASK_FPS;
    for (let t = 0; t < durationSec; t += step) timestamps.push(t);
    if (timestamps.length === 0) {
      this.progress = 1;
      return this.entries;
    }
    // Video tersembunyi (tscaps HiddenVideoLoader): tak menyentuh element preview.
    const video = document.createElement("video");
    video.muted = true;
    video.preload = "auto";
    video.src = videoUrl;
    await new Promise<void>((resolve, reject) => {
      video.onloadeddata = () => resolve();
      video.onerror = () => reject(new Error("video load gagal (mask analysis)"));
    });
    try {
      let captured = 0;
      for (const t of timestamps) {
        const bmp = await seekFrame(video, t);
        try {
          const mask = await this.segment(bmp, Math.round(t * 1000), t);
          this.entries.push(mask);
        } catch {
          // Sampel gagal (worker error) — lewati; pasokan per-frame fallback quiet.
        }
        captured++;
        this.progress = captured / timestamps.length;
      }
    } finally {
      video.removeAttribute("src");
      video.load();
    }
    this.progress = 1;
    return this.entries;
  }

  /** Masker terdekat dalam toleransi (nearest dalam daftar terurut). */
  nearest(t: number): PersonMask | null {
    const idx = this.nearestIndex(t);
    return idx >= 0 ? this.entries[idx] : null;
  }

  /** Index masker terdekat dalam toleransi; -1 bila tak ada. */
  nearestIndex(t: number): number {
    const entries = this.entries;
    if (entries.length === 0) return -1;
    let lo = 0;
    let hi = entries.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (entries[mid]!.t < t) lo = mid + 1;
      else hi = mid;
    }
    const candA = lo;
    const candB = Math.max(0, lo - 1);
    const best =
      Math.abs(entries[candA]!.t - t) <= Math.abs(entries[candB]!.t - t) ? candA : candB;
    const chosen = entries[best]!;
    return Math.abs(chosen.t - t) <= MASK_TOLERANCE_SEC ? best : -1;
  }

  /**
   * Rentang behind (ms) per cue: cue aktif bila ≥CUE_PERSON_RATIO sampel
   * di dalamnya berisi orang. Pola tscaps: rentang cue utuh (register tak
   * berbalik mid-segment).
   */
  behindRanges(
    cues: ReadonlyArray<{ start_ms: number; end_ms: number }>,
  ): Array<[number, number]> {
    const isPerson = (m: PersonMask): boolean => {
      let count = 0;
      for (let i = 0; i < m.alpha.length; i++) if (m.alpha[i]! > 128) count++;
      return count / m.alpha.length >= PERSON_PIXEL_FRACTION;
    };
    const out: Array<[number, number]> = [];
    for (const cue of cues) {
      const start = cue.start_ms / 1000;
      const end = cue.end_ms / 1000;
      const samples = this.entries.filter((m) => m.t >= start && m.t < end);
      if (samples.length === 0) continue;
      const withPerson = samples.filter(isPerson).length;
      if (withPerson / samples.length >= CUE_PERSON_RATIO) {
        out.push([Math.round(cue.start_ms), Math.round(cue.end_ms)]);
      }
    }
    return out;
  }

  dispose(): void {
    this.worker?.terminate();
    this.worker = null;
    this.pending.clear();
    this.entries = [];
    this.progress = -1;
  }
}

function seekFrame(video: HTMLVideoElement, t: number): Promise<ImageBitmap> {
  return new Promise((resolve, reject) => {
    const onSeek = () => {
      video.removeEventListener("seeked", onSeek);
      createImageBitmap(video)
        .then(resolve)
        .catch(reject);
    };
    video.addEventListener("seeked", onSeek);
    video.currentTime = t;
  });
}
