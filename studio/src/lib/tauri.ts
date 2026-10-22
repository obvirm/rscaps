// Jembatan shell desktop (Tauri v2). Bila studio berjalan di dalam webview
// Tauri, file export disimpan lewat dialog native + penulisan chunked
// (command `save_export_pick`/`save_export_append`) — tanpa blob download
// yang menahan RAM 2x, tanpa ketergantungan File System Access API webview.
//
// Tanpa `@tauri-apps/api` (pnpm terblokir config workspace): IPC lewat
// `__TAURI_INTERNALS__.invoke` — jalur internal yang dipakai binding resmi.

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

interface TauriInternals {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
}

function internals(): TauriInternals | null {
  if (!isTauri()) return null;
  return (window as unknown as { __TAURI_INTERNALS__: TauriInternals })
    .__TAURI_INTERNALS__;
}

const CHUNK_BYTES = 4 * 1024 * 1024;

/** Simpan bytes lewat dialog Save native. `null` = user membatalkan atau
 * bukan shell Tauri (pemanggil jatuh ke jalur browser biasa). */
export async function saveFileNative(
  data: Uint8Array,
  fileName: string,
  filterName?: string,
  filterExt?: string[],
): Promise<string | null> {
  const t = internals();
  if (!t) return null;
  let path: string;
  try {
    path = (await t.invoke("save_export_pick", {
      fileName,
      filterName: filterName ?? null,
      filterExt: filterExt ?? null,
    })) as string;
  } catch (e) {
    if (String(e).includes("cancel")) return null;
    throw e;
  }
  for (let off = 0; off < data.length; off += CHUNK_BYTES) {
    await t.invoke("save_export_append", {
      path,
      data: Array.from(data.subarray(off, off + CHUNK_BYTES)),
    });
  }
  return path;
}
