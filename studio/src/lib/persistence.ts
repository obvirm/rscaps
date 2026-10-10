import type { ProjectMetadata, RscapsProject } from "./types";

const DB_NAME = "rscaps";
const DB_VERSION = 2;
const STORE = "projects";
const VIDEO_STORE = "videos";

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) {
        db.createObjectStore(STORE, { keyPath: "id" });
      }
      if (!db.objectStoreNames.contains(VIDEO_STORE)) {
        db.createObjectStore(VIDEO_STORE);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function tx<T>(
  storeName: string,
  mode: IDBTransactionMode,
  run: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  return openDb().then(
    (db) =>
      new Promise<T>((resolve, reject) => {
        const t = db.transaction(storeName, mode);
        const req = run(t.objectStore(storeName));
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
        t.oncomplete = () => db.close();
      }),
  );
}

export async function listProjects(): Promise<RscapsProject[]> {
  const all = await tx<RscapsProject[]>(STORE, "readonly", (s) => s.getAll());
  return all.sort((a, b) => (a.updatedAt < b.updatedAt ? 1 : -1));
}

export async function getProject(id: string): Promise<RscapsProject | undefined> {
  return tx<RscapsProject | undefined>(STORE, "readonly", (s) => s.get(id));
}

export async function saveProject(project: RscapsProject): Promise<void> {
  await tx(STORE, "readwrite", (s) => s.put(project));
}

export async function deleteProject(id: string): Promise<void> {
  await tx(STORE, "readwrite", (s) => s.delete(id));
  await tx(VIDEO_STORE, "readwrite", (s) => s.delete(id));
}

export async function saveVideo(id: string, blob: Blob): Promise<void> {
  await tx(VIDEO_STORE, "readwrite", (s) => s.put(blob, id));
}

export async function loadVideo(id: string): Promise<Blob | undefined> {
  return tx<Blob | undefined>(VIDEO_STORE, "readonly", (s) => s.get(id));
}

export function toMetadata(p: RscapsProject): ProjectMetadata {
  const fps = p.settings?.fps || 30;
  return {
    id: p.id,
    name: p.name,
    createdAt: p.createdAt,
    updatedAt: p.updatedAt,
    thumbnail: p.thumbnail,
    durationSec: (p.doc?.duration_frames || 0) / fps,
    fps,
    width: p.settings?.width || 1920,
    height: p.settings?.height || 1080,
    videoName: p.videoName,
  };
}
