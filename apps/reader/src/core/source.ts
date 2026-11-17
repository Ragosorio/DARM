/**
 * Random-access byte sources for `.darm` files.
 *
 * The package core (WASM) does no I/O: the reader hands it byte ranges read
 * through this small interface, so a multi-gigabyte book is never loaded into
 * memory as a whole. Implementations: a `Blob`/`File` picked by the user or
 * stored in the library, and an HTTP range reader for the `darm open` /
 * `darm dev` preview server.
 */
export interface ByteSource {
  readonly size: number;
  readonly name: string;
  read(offset: number, length: number): Promise<Uint8Array>;
  /** The backing blob, when there is one (enables zero-copy slices for media). */
  readonly blob?: Blob;
}

export function blobSource(blob: Blob, name: string): ByteSource {
  return {
    size: blob.size,
    name,
    blob,
    async read(offset, length) {
      if (offset < 0 || length < 0 || offset + length > blob.size) throw new RangeError("read beyond the end of the file");
      return new Uint8Array(await blob.slice(offset, offset + length).arrayBuffer());
    },
  };
}

/**
 * A same-origin URL read with HTTP range requests. Only relative URLs are
 * accepted: the reader never fetches a book from another origin on its own.
 */
export async function urlSource(url: string, name: string): Promise<ByteSource> {
  if (!/^\/(?!\/)/.test(url) && !/^\.\.?\//.test(url)) throw new Error("only same-origin book URLs can be opened");
  const first = await fetch(url, { headers: { Range: "bytes=0-65663" }, cache: "no-store" });
  if (!first.ok) throw new Error(`the book could not be loaded (HTTP ${first.status})`);
  if (first.status !== 206) {
    // No range support: fall back to one blob.
    return blobSource(await first.blob(), name);
  }
  const total = Number((first.headers.get("Content-Range") ?? "").split("/")[1]);
  if (!Number.isSafeInteger(total) || total <= 0) throw new Error("the server sent an invalid Content-Range");
  const head = new Uint8Array(await first.arrayBuffer());
  return {
    size: total,
    name,
    async read(offset, length) {
      if (offset < 0 || length < 0 || offset + length > total) throw new RangeError("read beyond the end of the file");
      if (length === 0) return new Uint8Array(0);
      if (offset + length <= head.length) return head.slice(offset, offset + length);
      const out = new Uint8Array(length);
      let done = 0;
      // The preview server caps one response; ask again until the range is complete.
      while (done < length) {
        const r = await fetch(url, { headers: { Range: `bytes=${offset + done}-${offset + length - 1}` }, cache: "no-store" });
        if (r.status !== 206 && r.status !== 200) throw new Error(`the book could not be loaded (HTTP ${r.status})`);
        const part = new Uint8Array(await r.arrayBuffer());
        if (part.length === 0) throw new Error("the server returned an empty range");
        out.set(part.subarray(0, length - done), done);
        done += Math.min(part.length, length - done);
      }
      return out;
    },
  };
}
