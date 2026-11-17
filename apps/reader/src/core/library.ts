/**
 * The personal library: imported `.darm` files, their covers, and per-book
 * reading state, stored in IndexedDB on the device. Nothing leaves the
 * device; there is no account and no server.
 *
 * Stores:
 *   books  id → BookRecord (metadata, cover thumbnail, progress summary)
 *   files  id → Blob (the `.darm` file itself, kept verbatim — encrypted
 *                     books stay encrypted at rest; passwords are never stored)
 *   state  id → ReadingState (position, variables, quiz/choice records, bookmarks)
 */
import type { State } from "../ir";
import type { PageAnchor, UiRecords } from "../layout/types";

export interface BookRecord {
  /** Package UUID. */
  id: string;
  title: string;
  authors: string[];
  language?: string;
  coverColor?: string;
  encrypted: boolean;
  /** False for protected books that hide their title until unlocked. */
  hasCard: boolean;
  fileName: string;
  size: number;
  addedAt: number;
  openedAt: number;
  /** 0 … 1, for the shelf. */
  progress: number;
  finished: boolean;
  /** Small PNG of the front cover, once the book has been opened. */
  cover?: Blob;
}

export interface Bookmark {
  anchor: PageAnchor;
  label: string;
  createdAt: number;
}

export interface ReadingState {
  /** Where the reader was: on the front cover, at a page, or on the back cover. */
  at: "front" | "page" | "back";
  anchor: PageAnchor | null;
  vars: State;
  ui: UiRecords;
  bookmarks: Bookmark[];
  /** Hosts the user allowed this book to load media from. */
  allowedHosts: string[];
  updatedAt: number;
}

export function emptyReadingState(): ReadingState {
  return { at: "front", anchor: null, vars: {}, ui: { quiz: {}, choice: {} }, bookmarks: [], allowedHosts: [], updatedAt: 0 };
}

const DB_NAME = "darm-library";
const DB_VERSION = 1;

function request<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error ?? new Error("storage error"));
  });
}

function done(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error ?? new Error("storage transaction aborted"));
    tx.onerror = () => reject(tx.error ?? new Error("storage error"));
  });
}

/** How a blob is kept in IndexedDB: as itself, or as plain bytes where the engine cannot store Blobs. */
type StoredBlob = Blob | { bytes: ArrayBuffer; type: string };

function unpack(stored: StoredBlob | undefined): Blob | undefined {
  if (!stored) return undefined;
  return stored instanceof Blob ? stored : new Blob([stored.bytes], { type: stored.type });
}

/** Largest file kept as plain bytes (it passes through memory once). */
const MAX_BYTES_FALLBACK = 512 * 1024 * 1024;

let blobSupport: Promise<boolean> | null = null;
/**
 * Can this engine store Blobs in IndexedDB? WebKit cannot in private or
 * ephemeral sessions, and there the write neither succeeds nor fails — it
 * just never finishes — so the answer is found with a throwaway database and
 * a deadline, once per session.
 */
function canStoreBlobs(): Promise<boolean> {
  blobSupport ??= new Promise<boolean>((resolve) => {
    const name = "darm-blob-probe";
    const finish = (ok: boolean) => {
      resolve(ok);
      try {
        indexedDB.deleteDatabase(name);
      } catch {
        /* nothing to clean up */
      }
    };
    const timer = setTimeout(() => finish(false), 1500);
    try {
      const open = indexedDB.open(name, 1);
      open.onupgradeneeded = () => open.result.createObjectStore("t");
      open.onerror = () => {
        clearTimeout(timer);
        finish(false);
      };
      open.onsuccess = () => {
        const db = open.result;
        try {
          const tx = db.transaction("t", "readwrite");
          tx.objectStore("t").put(new Blob(["probe"]), "k");
          tx.oncomplete = () => {
            clearTimeout(timer);
            db.close();
            finish(true);
          };
          tx.onabort = tx.onerror = () => {
            clearTimeout(timer);
            db.close();
            finish(false);
          };
        } catch {
          clearTimeout(timer);
          db.close();
          finish(false);
        }
      };
    } catch {
      clearTimeout(timer);
      finish(false);
    }
  });
  return blobSupport;
}

async function pack(blob: Blob): Promise<StoredBlob> {
  if (await canStoreBlobs()) return blob;
  if (blob.size > MAX_BYTES_FALLBACK) {
    throw new Error("This browser session cannot keep a book this large in its library (private browsing limits storage). Open it in a normal window.");
  }
  return { bytes: await blob.arrayBuffer(), type: blob.type };
}

export class Library {
  private db: Promise<IDBDatabase> | null = null;

  private open(): Promise<IDBDatabase> {
    this.db ??= new Promise((resolve, reject) => {
      const r = indexedDB.open(DB_NAME, DB_VERSION);
      r.onupgradeneeded = () => {
        const db = r.result;
        if (!db.objectStoreNames.contains("books")) db.createObjectStore("books", { keyPath: "id" });
        if (!db.objectStoreNames.contains("files")) db.createObjectStore("files");
        if (!db.objectStoreNames.contains("state")) db.createObjectStore("state");
      };
      r.onsuccess = () => resolve(r.result);
      r.onerror = () => reject(r.error ?? new Error("the library could not be opened"));
      r.onblocked = () => reject(new Error("the library is in use by another tab"));
    });
    return this.db;
  }

  /** Whether the browser offers persistent storage at all (private windows may not). */
  static get available(): boolean {
    return typeof indexedDB !== "undefined";
  }

  private revive(r: BookRecord | undefined): BookRecord | undefined {
    if (r?.cover) r.cover = unpack(r.cover as StoredBlob);
    return r;
  }

  async list(): Promise<BookRecord[]> {
    const db = await this.open();
    const all = await request(db.transaction("books").objectStore("books").getAll() as IDBRequest<BookRecord[]>);
    return all.map((r) => this.revive(r)!).sort((a, b) => Math.max(b.openedAt, b.addedAt) - Math.max(a.openedAt, a.addedAt));
  }

  async get(id: string): Promise<BookRecord | undefined> {
    const db = await this.open();
    return this.revive(await request(db.transaction("books").objectStore("books").get(id) as IDBRequest<BookRecord | undefined>));
  }

  async file(id: string): Promise<Blob | undefined> {
    const db = await this.open();
    return unpack(await request(db.transaction("files").objectStore("files").get(id) as IDBRequest<StoredBlob | undefined>));
  }

  /** Add (or replace) a book and its file in one transaction. */
  async add(record: BookRecord, file: Blob): Promise<void> {
    const db = await this.open();
    // Convert before the transaction opens: awaiting inside one would let it auto-commit.
    const storedFile = await pack(file);
    const storedRecord = { ...record, cover: record.cover ? await pack(record.cover) : undefined };
    const tx = db.transaction(["books", "files"], "readwrite");
    tx.objectStore("books").put(storedRecord);
    tx.objectStore("files").put(storedFile, record.id);
    try {
      await done(tx);
    } catch (e) {
      if ((e as DOMException)?.name === "QuotaExceededError") {
        throw new Error("There is not enough storage on this device to keep the book in the library.");
      }
      throw new Error(`The book could not be saved to the library on this device (${(e as Error)?.message ?? "storage error"}).`);
    }
    // Ask the browser not to evict the library under storage pressure.
    void navigator.storage?.persist?.().catch(() => false);
  }

  async update(id: string, patch: Partial<BookRecord>): Promise<void> {
    const db = await this.open();
    const stored: Record<string, unknown> = { ...patch };
    if (patch.cover) stored.cover = await pack(patch.cover);
    const tx = db.transaction("books", "readwrite");
    const store = tx.objectStore("books");
    const get = store.get(id);
    get.onsuccess = () => {
      if (get.result) store.put({ ...get.result, ...stored, id });
    };
    await done(tx);
  }

  async remove(id: string): Promise<void> {
    const db = await this.open();
    const tx = db.transaction(["books", "files", "state"], "readwrite");
    tx.objectStore("books").delete(id);
    tx.objectStore("files").delete(id);
    tx.objectStore("state").delete(id);
    await done(tx);
  }

  async state(id: string): Promise<ReadingState> {
    const db = await this.open();
    const s = await request(db.transaction("state").objectStore("state").get(id) as IDBRequest<Partial<ReadingState> | undefined>);
    // Stored data is treated as untrusted: missing or malformed parts fall back to defaults.
    const base = emptyReadingState();
    if (!s || typeof s !== "object") return base;
    return {
      at: s.at === "page" || s.at === "back" ? s.at : "front",
      anchor: s.anchor && typeof s.anchor.chapterId === "string" ? { chapterId: s.anchor.chapterId, block: typeof s.anchor.block === "string" ? s.anchor.block : null, line: Number(s.anchor.line) || 0 } : null,
      vars: s.vars && typeof s.vars === "object" ? s.vars : {},
      ui: {
        quiz: s.ui?.quiz && typeof s.ui.quiz === "object" ? s.ui.quiz : {},
        choice: s.ui?.choice && typeof s.ui.choice === "object" ? s.ui.choice : {},
      },
      bookmarks: Array.isArray(s.bookmarks) ? s.bookmarks.filter((b) => b && typeof b.label === "string" && b.anchor && typeof b.anchor.chapterId === "string").slice(0, 500) : [],
      allowedHosts: Array.isArray(s.allowedHosts) ? s.allowedHosts.filter((h) => typeof h === "string").slice(0, 64) : [],
      updatedAt: Number(s.updatedAt) || 0,
    };
  }

  async saveState(id: string, state: ReadingState): Promise<void> {
    const db = await this.open();
    const tx = db.transaction("state", "readwrite");
    tx.objectStore("state").put({ ...state, updatedAt: Date.now() }, id);
    await done(tx);
  }
}

/** In-memory stand-in used when IndexedDB is unavailable (some private windows) and for ephemeral previews. */
export class MemoryLibrary extends Library {
  private books = new Map<string, BookRecord>();
  private blobs = new Map<string, Blob>();
  private states = new Map<string, ReadingState>();
  override async list(): Promise<BookRecord[]> {
    return [...this.books.values()].sort((a, b) => Math.max(b.openedAt, b.addedAt) - Math.max(a.openedAt, a.addedAt));
  }
  override async get(id: string): Promise<BookRecord | undefined> {
    return this.books.get(id);
  }
  override async file(id: string): Promise<Blob | undefined> {
    return this.blobs.get(id);
  }
  override async add(record: BookRecord, file: Blob): Promise<void> {
    this.books.set(record.id, record);
    this.blobs.set(record.id, file);
  }
  override async update(id: string, patch: Partial<BookRecord>): Promise<void> {
    const c = this.books.get(id);
    if (c) this.books.set(id, { ...c, ...patch, id });
  }
  override async remove(id: string): Promise<void> {
    this.books.delete(id);
    this.blobs.delete(id);
    this.states.delete(id);
  }
  override async state(id: string): Promise<ReadingState> {
    return structuredClone(this.states.get(id) ?? emptyReadingState());
  }
  override async saveState(id: string, state: ReadingState): Promise<void> {
    this.states.set(id, structuredClone({ ...state, updatedAt: Date.now() }));
  }
}
