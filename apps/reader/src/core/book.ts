/**
 * An opened `.darm` book: package access through the WASM core, the validated
 * IR, and lazily decoded resources.
 *
 * Trust boundary: everything in a package is untrusted. The container is
 * parsed, verified and (if needed) decrypted by `darm-package` compiled to
 * WebAssembly; the IR is validated by `darm-ir` (the same code the compiler
 * uses) before the reader looks at it; resources are only ever handed to the
 * browser's own decoders as blobs with the media type the IR declares.
 */
import type { Book, Resource } from "../ir";
import type { ImageProvider } from "../layout/types";
import init, { DarmBook, DarmPackage, type DarmEntry, type DarmManifest, type DarmProbe, type DarmSearchHit, headerLen, parseKeyfile, probe } from "../wasm-pkg/darm_wasm";
import wasmUrl from "../wasm-pkg/darm_wasm_bg.wasm?url";
import type { ByteSource } from "./source";

/** Reader-side limits, stricter than the format's, sized for phones. */
export const LIMITS = {
  /** Largest IR the reader parses. */
  irBytes: 64 * 1024 * 1024,
  /** Largest Argon2id memory the reader will allocate, KiB. */
  kdfMemoryKib: 256 * 1024,
  kdfPasses: 8,
  /** Largest image, caption file or font decoded into memory. */
  inlineBytes: 96 * 1024 * 1024,
  /** Largest decoded image, in pixels. */
  imagePixels: 64_000_000,
} as const;

export class DarmError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "DarmError";
  }
}

function wrap(e: unknown): DarmError {
  if (e instanceof DarmError) return e;
  const code = typeof (e as { code?: unknown })?.code === "string" ? (e as { code: string }).code : "E_READER";
  const message = e instanceof Error ? e.message.replace(/^E_[A-Z_]+: /, "") : String(e);
  return new DarmError(code, message);
}

let ready: Promise<unknown> | null = null;
/** Load the WASM core once. */
export function core(): Promise<unknown> {
  ready ??= init({ module_or_path: wasmUrl });
  return ready;
}

export interface Credentials {
  password?: string;
  /** Raw 32-byte key from a `.darmkey` file. */
  key?: Uint8Array;
}

export interface ProbeInfo extends DarmProbe {
  /** Size of the file actually supplied. */
  size: number;
}

/** Read the public part of a package: format, protection, public card. */
export async function probeSource(source: ByteSource): Promise<ProbeInfo> {
  await core();
  if (source.size < headerLen()) {
    // Tell "not a book at all" apart from "a book that was cut short".
    const MAGIC = [0x89, 0x44, 0x41, 0x52, 0x4d, 0x0d, 0x0a, 0x1a];
    const start = await source.read(0, Math.min(source.size, MAGIC.length));
    const isDarm = start.length === MAGIC.length && MAGIC.every((b, i) => start[i] === b);
    throw isDarm
      ? new DarmError("E_PKG_TRUNCATED", "the file ends before its header is complete")
      : new DarmError("E_PKG_BAD_MAGIC", "the file does not start with the DARM signature");
  }
  const head = await source.read(0, Math.min(source.size, 65_664));
  let info: DarmProbe;
  try {
    info = JSON.parse(probe(head)) as DarmProbe;
  } catch (e) {
    throw wrap(e);
  }
  if (info.totalLen !== source.size) {
    throw new DarmError(
      "E_PKG_TRUNCATED",
      `the file is ${source.size} bytes but says it should be ${info.totalLen}: the download or copy is incomplete`,
    );
  }
  return { ...info, size: source.size };
}

/**
 * Check every chunk of an unencrypted package against its SHA-256 before the
 * book is accepted into the library. (Encrypted packages are authenticated
 * chunk by chunk whenever they are decrypted, which needs the key.) This
 * matters because media in clear packages is later streamed straight from the
 * file without passing through the core again.
 */
export async function verifySource(source: ByteSource, onProgress?: (done: number, total: number) => void): Promise<void> {
  const info = await probeSource(source);
  if (info.encrypted) return;
  const head = await source.read(0, info.headLen);
  let pkg: DarmPackage;
  try {
    pkg = DarmPackage.open(head, null, null);
  } catch (e) {
    throw wrap(e);
  }
  try {
    const entries = pkg.entryCount();
    let done = 0;
    for (let e = 0; e < entries; e++) {
      const chunks = pkg.chunkCount(e);
      for (let c = 0; c < chunks; c++) {
        const stored = await source.read(pkg.chunkOffset(e, c), pkg.chunkLen(e, c));
        pkg.decodeChunk(e, c, stored); // throws E_PKG_INTEGRITY on any mismatch
        done += stored.length;
        onProgress?.(done, info.totalLen);
      }
    }
  } catch (e) {
    throw wrap(e);
  } finally {
    pkg.free();
  }
}

/** Parse the text of a `.darmkey` file into its 32-byte key. */
export async function readKeyfile(text: string): Promise<Uint8Array> {
  await core();
  try {
    return parseKeyfile(text);
  } catch (e) {
    throw wrap(e);
  }
}

interface LoadedImage {
  image: CanvasImageSource | null;
  promise: Promise<void> | null;
  failed: boolean;
}

export class OpenBook implements ImageProvider {
  readonly ir: Book;
  readonly uuid: string;
  readonly encrypted: boolean;
  private pkg: DarmPackage;
  private model: DarmBook;
  private entries = new Map<string, DarmEntry>();
  private resources = new Map<string, Resource>();
  private urls = new Map<string, Promise<string>>();
  private images = new Map<string, LoadedImage>();
  private closed = false;
  /** Called when an image finished decoding, so pages that use it can be repainted. */
  onImageReady: ((resource: string) => void) | null = null;

  private constructor(
    readonly source: ByteSource,
    pkg: DarmPackage,
    manifest: DarmManifest,
    model: DarmBook,
    ir: Book,
    uuid: string,
  ) {
    this.pkg = pkg;
    this.model = model;
    this.ir = ir;
    this.uuid = uuid;
    this.encrypted = pkg.isEncrypted();
    for (const e of manifest.entries) this.entries.set(e.name, e);
    for (const r of ir.resources) this.resources.set(r.name, r);
  }

  static async open(source: ByteSource, credentials: Credentials = {}): Promise<OpenBook> {
    const info = await probeSource(source);
    if (info.encrypted && info.kdf && credentials.password !== undefined) {
      if (info.kdf.m_kib > LIMITS.kdfMemoryKib || info.kdf.t > LIMITS.kdfPasses) {
        throw new DarmError(
          "E_READER_KDF",
          `this book asks for ${Math.round(info.kdf.m_kib / 1024)} MiB and ${info.kdf.t} passes to check a password, more than this reader allows`,
        );
      }
    }
    const head = await source.read(0, info.headLen);
    let pkg: DarmPackage;
    try {
      pkg = DarmPackage.open(head, credentials.password?.normalize("NFC") ?? null, credentials.key ?? null);
    } catch (e) {
      throw wrap(e);
    }
    try {
      const manifest = JSON.parse(pkg.manifestJson()) as DarmManifest;
      const irEntry = manifest.entries.find((e) => e.role === "ir");
      if (!irEntry) throw new DarmError("E_PKG_CORRUPT_MANIFEST", "the package has no book data");
      if (irEntry.size > LIMITS.irBytes) throw new DarmError("E_READER_LIMIT", "the book data is larger than this reader allows");
      const bytes = await OpenBook.decode(source, pkg, irEntry);
      const json = new TextDecoder("utf-8", { fatal: true }).decode(await new Blob(bytes).arrayBuffer());
      // Validate with the Rust core before trusting any field.
      const model = DarmBook.parse(json);
      const ir = JSON.parse(json) as Book;
      return new OpenBook(source, pkg, manifest, model, ir, info.uuid);
    } catch (e) {
      pkg.free();
      throw wrap(e);
    }
  }

  /** Decode every chunk of an entry. Each chunk is verified (hash or GCM tag) by the core. */
  private static async decode(source: ByteSource, pkg: DarmPackage, entry: DarmEntry): Promise<Uint8Array<ArrayBuffer>[]> {
    const parts: Uint8Array<ArrayBuffer>[] = [];
    const count = pkg.chunkCount(entry.index);
    for (let c = 0; c < count; c++) {
      const stored = await source.read(pkg.chunkOffset(entry.index, c), pkg.chunkLen(entry.index, c));
      parts.push(pkg.decodeChunk(entry.index, c, stored) as Uint8Array<ArrayBuffer>);
    }
    return parts;
  }

  private entryFor(name: string): { resource: Resource; entry: DarmEntry } {
    const resource = this.resources.get(name);
    if (!resource) throw new DarmError("E_READER_RESOURCE", `unknown resource ${name}`);
    if (resource.source.location !== "packaged") throw new DarmError("E_READER_RESOURCE", `${name} is not stored in the book`);
    const entry = this.entries.get(resource.source.entry);
    if (!entry) throw new DarmError("E_PKG_CORRUPT_MANIFEST", `the book is missing the file for ${name}`);
    return { resource, entry };
  }

  /**
   * The content of a packaged resource as a blob typed with the media type
   * the IR declares. Clear, uncompressed entries are zero-copy slices of the
   * file (so video streams and seeks straight from disk); anything else is
   * decoded chunk by chunk.
   */
  async blob(name: string): Promise<Blob> {
    if (this.closed) throw new DarmError("E_READER_CLOSED", "the book was closed");
    const { resource, entry } = this.entryFor(name);
    const direct = !this.encrypted && entry.compression === "none" && this.source.blob;
    if (direct) {
      const start = this.pkg.dataOffset() + entry.offset;
      return this.source.blob!.slice(start, start + entry.size, resource.media_type);
    }
    try {
      const parts = await OpenBook.decode(this.source, this.pkg, entry);
      return new Blob(parts, { type: resource.media_type });
    } catch (e) {
      throw wrap(e);
    }
  }

  /** Small resources (fonts, captions) as bytes. */
  async bytes(name: string): Promise<ArrayBuffer> {
    const { entry } = this.entryFor(name);
    if (entry.size > LIMITS.inlineBytes) throw new DarmError("E_READER_LIMIT", `${name} is too large to load into memory`);
    return (await this.blob(name)).arrayBuffer();
  }

  /** An object URL for a resource (cached; revoked when the book closes). */
  url(name: string): Promise<string> {
    let u = this.urls.get(name);
    if (!u) {
      u = this.blob(name).then((b) => URL.createObjectURL(b));
      this.urls.set(name, u);
      u.catch(() => this.urls.delete(name));
    }
    return u;
  }

  resource(name: string): Resource | undefined {
    return this.resources.get(name);
  }

  /** Remote URL of a resource, if it is remote and its host is on the book's allow-list. */
  remoteUrl(name: string): string | null {
    const r = this.resources.get(name);
    if (!r || r.source.location !== "remote" || this.ir.meta.offline) return null;
    try {
      const u = new URL(r.source.url);
      if (u.protocol !== "https:" || u.username || u.password) return null;
      return (this.ir.meta.network ?? []).includes(u.hostname.toLowerCase()) ? u.href : null;
    } catch {
      return null;
    }
  }

  // --- ImageProvider ----------------------------------------------------------

  size(resource: string): { width: number; height: number } | null {
    const r = this.resources.get(resource);
    return r?.width && r.height ? { width: r.width, height: r.height } : null;
  }

  get(resource: string): CanvasImageSource | null {
    let entry = this.images.get(resource);
    if (!entry) {
      entry = { image: null, promise: null, failed: false };
      this.images.set(resource, entry);
      entry.promise = this.loadImage(resource)
        .then((img) => {
          entry!.image = img;
        })
        .catch((e) => {
          entry!.failed = true;
          console.warn(`image ${resource} could not be decoded`, e);
        })
        .finally(() => {
          entry!.promise = null;
          if (!this.closed) this.onImageReady?.(resource);
        });
    }
    return entry.image;
  }

  /** True while the image is still being decoded (so a repaint is worth waiting for). */
  pending(resource: string): boolean {
    const e = this.images.get(resource);
    return !e || e.promise !== null;
  }

  /** Decode an image ahead of time. */
  async ensureImage(resource: string): Promise<void> {
    this.get(resource);
    await this.images.get(resource)?.promise;
  }

  private async loadImage(name: string): Promise<CanvasImageSource> {
    const r = this.resources.get(name);
    if (!r || r.kind !== "image") throw new DarmError("E_READER_RESOURCE", `${name} is not an image`);
    if (r.source.location !== "packaged") throw new DarmError("E_READER_RESOURCE", "remote images are not loaded");
    if (r.size > LIMITS.inlineBytes) throw new DarmError("E_READER_LIMIT", `${name} is too large to decode`);
    if (r.width && r.height && r.media_type !== "image/svg+xml" && r.width * r.height > LIMITS.imagePixels) {
      throw new DarmError("E_READER_LIMIT", `${name} has too many pixels to decode`);
    }
    // An <img> decodes every supported format, including SVG. Loaded this way
    // an SVG cannot run scripts or fetch anything, whatever it contains.
    const img = new Image();
    img.decoding = "async";
    img.src = await this.url(name);
    await img.decode();
    return img;
  }

  // --- search -----------------------------------------------------------------

  search(query: string, limit = 200): DarmSearchHit[] {
    if (this.closed) return [];
    // Byte offsets from the core are UTF-8; convert to UTF-16 indices for the UI.
    const hits = JSON.parse(this.model.search(query, limit)) as DarmSearchHit[];
    const enc = new TextEncoder();
    const dec = new TextDecoder();
    return hits.map((h) => {
      const bytes = enc.encode(h.snippet);
      const start = dec.decode(bytes.subarray(0, h.start)).length;
      const end = start + dec.decode(bytes.subarray(h.start, h.end)).length;
      return { ...h, start, end };
    });
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    for (const u of this.urls.values()) u.then((x) => URL.revokeObjectURL(x)).catch(() => undefined);
    this.urls.clear();
    this.images.clear();
    this.pkg.free();
    this.model.free();
  }
}
