/**
 * Glue between the reader and its native shell (Tauri 2, `apps/desktop`).
 *
 * The shell never gives the webview a path. When the operating system hands
 * the app a `.darm` file (double-click, "Open with", a second launch, the
 * native File → Open… dialog) the shell registers it under a random token and
 * tells us `{ token, name, size }`. We pull the bytes through
 * `darm_read_chunk` in 4 MiB pieces, build a `File` out of the pieces (so a
 * multi-gigabyte book never has to sit in one `ArrayBuffer`), give it to the
 * reader, and release the token.
 *
 * In a browser this module does nothing: `initNativeShell` resolves to `false`.
 *
 * It talks to the global Tauri API (`app.withGlobalTauri` in
 * `tauri.conf.json`), so the reader needs no `@tauri-apps/api` dependency.
 */

/** Event the shell emits when files arrive while the app is running. */
export const NATIVE_FILES_EVENT = "darm://files";

/** Bytes requested per `darm_read_chunk` call. The shell refuses more than 8 MiB. */
export const NATIVE_CHUNK_BYTES = 4 * 1024 * 1024;

const DARM_MIME = "application/vnd.darm";

/** What the shell tells us about a file it holds. There is deliberately no path. */
interface NativeFile {
  token: string;
  name: string;
  size: number;
}

/** The subset of `window.__TAURI__` this module uses. */
interface TauriGlobal {
  core: {
    invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  };
  event: {
    listen(event: string, handler: (event: unknown) => void): Promise<() => void>;
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNativeFile(value: unknown): value is NativeFile {
  return (
    isRecord(value) &&
    typeof value.token === "string" &&
    value.token.length > 0 &&
    typeof value.name === "string" &&
    typeof value.size === "number" &&
    Number.isSafeInteger(value.size) &&
    value.size >= 0
  );
}

/** The Tauri global, or `null` in a browser (or if the shell does not expose it). */
function tauriGlobal(): TauriGlobal | null {
  if (typeof window === "undefined") return null;
  const w = window as unknown as { __TAURI_INTERNALS__?: unknown; __TAURI__?: unknown };
  if (w.__TAURI_INTERNALS__ === undefined) return null;
  const api = w.__TAURI__;
  if (
    isRecord(api) &&
    isRecord(api.core) &&
    typeof api.core.invoke === "function" &&
    isRecord(api.event) &&
    typeof api.event.listen === "function"
  ) {
    return api as unknown as TauriGlobal;
  }
  console.error("DARM shell: running inside Tauri, but window.__TAURI__ is missing (app.withGlobalTauri must be true)");
  return null;
}

/**
 * A binary IPC response as bytes. Tauri delivers raw responses as an
 * `ArrayBuffer`; the fallbacks cover its `postMessage` transport, which some
 * platforms use and which can deliver a typed array or a plain array of numbers.
 */
function toBytes(raw: unknown): Uint8Array<ArrayBuffer> {
  if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
  if (ArrayBuffer.isView(raw)) {
    const copy = new Uint8Array(raw.byteLength);
    copy.set(new Uint8Array(raw.buffer, raw.byteOffset, raw.byteLength));
    return copy;
  }
  if (Array.isArray(raw)) return Uint8Array.from(raw as number[]);
  throw new Error("the shell returned something that is not binary data");
}

/** Pulls a registered file out of the shell, chunk by chunk. */
async function readNativeFile(tauri: TauriGlobal, info: NativeFile): Promise<File> {
  const parts: Uint8Array<ArrayBuffer>[] = [];
  let offset = 0;
  while (offset < info.size) {
    const len = Math.min(NATIVE_CHUNK_BYTES, info.size - offset);
    const chunk = toBytes(await tauri.core.invoke("darm_read_chunk", { token: info.token, offset, len }));
    if (chunk.byteLength !== len) {
      throw new Error(`short read at byte ${offset}: wanted ${len} bytes, got ${chunk.byteLength}`);
    }
    parts.push(chunk);
    offset += len;
  }
  return new File(parts, info.name, { type: DARM_MIME });
}

/** Takes everything the shell has queued and opens it, one file at a time. */
async function drainPending(tauri: TauriGlobal, open: (file: File) => Promise<void>): Promise<void> {
  const pending = await tauri.core.invoke("darm_take_pending");
  if (!Array.isArray(pending)) throw new Error("darm_take_pending did not return a list");
  for (const info of pending as unknown[]) {
    if (!isNativeFile(info)) {
      console.error("DARM shell: ignoring a malformed file entry");
      continue;
    }
    try {
      await open(await readNativeFile(tauri, info));
    } catch (e) {
      console.error(`DARM shell: could not open "${info.name}"`, e);
    } finally {
      // Always let the shell close the file, even if the book turned out to be unreadable.
      await tauri.core.invoke("darm_release", { token: info.token }).catch((e: unknown) => {
        console.error("DARM shell: could not release a file", e);
      });
    }
  }
}

/**
 * Connects the reader to its native shell.
 *
 * Resolves to `false` straight away when the page is not running inside the
 * DARM shell (an ordinary browser, the PWA). Otherwise it opens the files the
 * shell is already holding (the ones the app was launched with), keeps
 * listening for files that arrive later, and resolves to `true`.
 *
 * `open` is called once per file, never concurrently, in the order the files
 * arrived. Errors are reported with `console.error` and never thrown.
 */
export async function initNativeShell(open: (file: File) => Promise<void>): Promise<boolean> {
  const tauri = tauriGlobal();
  if (!tauri) return false;

  // One drain at a time: an event that arrives while a book is still being
  // read from disk waits for its turn instead of interleaving.
  let queue: Promise<void> = Promise.resolve();
  const drain = (): Promise<void> => {
    queue = queue
      .then(() => drainPending(tauri, open))
      .catch((e: unknown) => {
        console.error("DARM shell: could not receive files", e);
      });
    return queue;
  };

  // Listen first, then drain: a file that arrives in between is either in the
  // queue we are about to drain or announced by an event we already hear.
  try {
    await tauri.event.listen(NATIVE_FILES_EVENT, () => {
      void drain();
    });
  } catch (e) {
    console.error("DARM shell: could not listen for incoming files", e);
  }
  await drain();
  return true;
}
