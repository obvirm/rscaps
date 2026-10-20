// Preview = export: raster + komposit caption dijalankan lewat jalur
// burnWorker yang sama dengan export (svgr-wasm + blend difference terhadap
// piksel video), satu worker khusus, latest-wins.
//
// Main thread: createImageBitmap(<video>) (GPU copy ~1-3ms) → transfer ke
// worker → worker menggambar bitmap + layer caption (cache LRU) → ImageBitmap
// komposit transfer balik → drawImage ke kanvas preview.

export interface PreviewBurnRequest {
  idx: number;
  /** SVG layer base (render_frame, kosong saat gap). */
  base: string;
  /** SVG layer blend (mix-blend-mode) atau null. */
  blend: string | null;
  width: number;
  height: number;
  /** detik, hanya untuk identitas respons */
  t: number;
  bitmap: ImageBitmap;
}

export interface PreviewBurnResult {
  idx: number;
  frame: ImageBitmap | null;
  error?: string;
}

type Pending = { resolve: (r: PreviewBurnResult) => void };

export class PreviewBurner {
  private worker: Worker | null = null;
  private seq = 0;
  private failed = false;
  private pending = new Map<number, Pending>();

  get available(): boolean {
    return !this.failed && typeof Worker !== "undefined" && typeof OffscreenCanvas !== "undefined";
  }

  private ensure(): Worker | null {
    if (this.failed || !this.available) return null;
    if (this.worker) return this.worker;
    try {
      this.worker = new Worker(new URL("./burnWorker.ts", import.meta.url), {
        name: "preview-burn",
        type: "module",
      });
      this.worker.onmessage = (e: MessageEvent) => {
        const d = e.data as { idx: number; bitmap?: ImageBitmap | null; error?: string };
        const p = this.pending.get(d.idx);
        if (!p) {
          d.bitmap?.close();
          return;
        }
        this.pending.delete(d.idx);
        p.resolve({ idx: d.idx, frame: d.bitmap ?? null, error: d.error });
      };
      this.worker.onerror = (ev) => {
        console.error("[previewBurn] worker error", ev.message);
        this.failed = true;
        for (const [, p] of this.pending) p.resolve({ idx: -1, frame: null, error: "worker error" });
        this.pending.clear();
      };
      return this.worker;
    } catch (err) {
      console.error("[previewBurn] gagal buat worker", err);
      this.failed = true;
      return null;
    }
  }

  /**
   * Komposit satu frame. Bitmap video DITRANSFER (berpindah kepemilikan) —
   * pemanggil tidak boleh memakainya lagi setelah memanggil ini.
   * Latest-wins: hanya respons dengan idx terakhir yang dianggap relevan.
   */
  request(req: Omit<PreviewBurnRequest, "idx">): Promise<PreviewBurnResult> {
    const worker = this.ensure();
    if (!worker) return Promise.resolve({ idx: -1, frame: null, error: "unavailable" });
    const idx = ++this.seq;
    return new Promise((resolve) => {
      this.pending.set(idx, { resolve });
      const payload = {
        idx,
        svg: req.base,
        blend: req.blend ?? "",
        width: req.width,
        height: req.height,
        t: req.t,
        dur: 0,
        bitmap: req.bitmap,
        preview: true,
      };
      worker.postMessage(payload, [req.bitmap]);
    });
  }

  /** Abaikan respons yang masih menggantung (scrub cepat). */
  cancelStale(): void {
    for (const [idx, p] of this.pending) {
      if (idx !== this.seq) {
        this.pending.delete(idx);
        p.resolve({ idx, frame: null });
      }
    }
  }

  dispose(): void {
    this.worker?.terminate();
    this.worker = null;
    this.pending.clear();
  }
}
