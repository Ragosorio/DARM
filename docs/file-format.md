# The `.darm` container format, version 1.0

`.darm` is the distribution container of DARM interactive books: one file that
holds the compiled book (the *IR*) and every resource it needs (images, video,
audio, fonts, captions), optionally encrypted.

This document is the normative description of format **1.0** exactly as
implemented by the `darm-package` crate (`crates/darm-package`), which is the
reference reader and writer. The WebAssembly reader (`crates/darm-wasm`) is a
thin binding over the same code.

Conventions used below:

* All fixed-width integers are **little-endian** unless a field says otherwise.
  Inside cryptographic inputs (nonces, associated data) integers are
  **big-endian**; each occurrence says so.
* `a || b` is byte concatenation. Quoted strings such as `"darm/1/chunk\0"` are
  ASCII bytes; `\0` is a single zero byte.
* "MUST", "SHOULD" and "MAY" are used in the RFC 2119 sense.
* KiB = 1024 bytes, MiB = 1024 KiB, GiB = 1024 MiB.

## 1. Design goals

* **A real container, not a renamed archive.** A fixed binary header, then
  clear metadata, then a manifest, then data. A reader needs the first
  `head_len` bytes (at most about 16 MiB, typically a few KiB) to know
  everything about the file, and can then fetch any chunk of any entry
  directly.
* **Random access.** Every entry is cut into fixed-size chunks that are
  verified, decrypted and decompressed independently, so a reader can seek in a
  multi-gigabyte video while holding one chunk in memory.
* **Zero-copy media.** In a clear package an uncompressed entry is stored
  verbatim and contiguously, so a web reader can hand a byte range of the file
  straight to the browser.
* **Hostile-input safe.** Every length is bounded by a published limit and
  checked against the real file size before anything is allocated.
* **Optional strong encryption** in which nothing but the header and the
  prelude is readable without the key.
* **Reproducible.** Building a clear package from the same inputs gives the
  same bytes.

## 2. File layout

```
offset 0          +--------------------------+
                  | fixed header (128 bytes) |
offset 128        +--------------------------+
                  | prelude                  |  clear JSON, prelude_len bytes
                  +--------------------------+
                  | manifest                 |  deflated JSON, sealed when
                  |                          |  encrypted; manifest_len bytes
offset head_len   +--------------------------+
                  | data area                |  the entries, back to back,
                  |                          |  each a run of chunks
offset total_len  +--------------------------+
```

`head_len = 128 + prelude_len + manifest_len`. There is no padding, no trailer
and no index at the end of the file: `total_len` MUST equal the length of the
file.

## 3. Fixed header

The header is exactly 128 bytes.

| Offset | Size | Field             | Description |
|-------:|-----:|-------------------|-------------|
| 0      | 8    | `magic`           | `89 44 41 52 4D 0D 0A 1A` — `"\x89DARM\r\n\x1a"`. |
| 8      | 2    | `format_major`    | u16, `1`. |
| 10     | 2    | `format_minor`    | u16, `0`. |
| 12     | 4    | `flags`           | u32 bit set, see below. |
| 16     | 16   | `uuid`            | Package UUID, in RFC 9562 byte order (the bytes as they appear in the text form). |
| 32     | 8    | `total_len`       | u64, length of the whole file in bytes. |
| 40     | 4    | `prelude_len`     | u32, length of the prelude in bytes. |
| 44     | 4    | `manifest_len`    | u32, length of the manifest *as stored* in bytes. |
| 48     | 32   | `prelude_sha256`  | SHA-256 of the prelude bytes. |
| 80     | 32   | `manifest_sha256` | SHA-256 of the stored manifest bytes (after compression and, if encrypted, after sealing). |
| 112    | 8    | `reserved`        | MUST be zero. |
| 120    | 8    | `header_check`    | First 8 bytes of SHA-256 over header bytes 0..120. |

The magic follows the PNG convention: a non-ASCII first byte (so the file is
never taken for text), a CR-LF pair and a Ctrl-Z (so line-ending conversion and
text-mode truncation damage the signature and are detected immediately).

The positions and meaning of `magic` and `format_major` are fixed for every
future version of the format; everything else in the header may change with the
major version.

### 3.1 Flags

| Bit | Name              | Meaning |
|----:|-------------------|---------|
| 0   | `ENCRYPTED`       | The manifest and every chunk are sealed with AES-256-GCM; the prelude has an `encryption` member. |
| 1   | `HAS_PUBLIC_CARD` | The prelude has a `card` member. |
| 2   | reserved          | Reserved for a future signature block (see §8.3). Never set in 1.0. |
| 3–31| reserved          | Never set in 1.0. |

A reader of major version 1 MUST reject a file in which any reserved bit is
set (`E_PKG_UNSUPPORTED_FEATURE`): a set bit means the file relies on a feature
the reader cannot honour.

### 3.2 Package UUID

* Clear packages: a version-8 (RFC 9562 "custom") UUID derived from the content,
  so identical inputs give identical files. It is the first 16 bytes of

  `SHA-256("darm/1/package-id\0" || u32-LE(prelude_len) || prelude || manifest_json)`

  where `manifest_json` is the manifest JSON *before* compression, with the
  version nibble set to 8 (`byte[6] = (byte[6] & 0x0f) | 0x80`) and the variant
  bits set to `10` (`byte[8] = (byte[8] & 0x3f) | 0x80`). This derivation is a
  writer convention; readers do not verify it.
* Encrypted packages: a random version-4 UUID. It doubles as the salt of every
  key derivation, so it MUST be freshly generated for each encrypted file.

### 3.3 Header validation

A reader MUST perform these checks, in this order, and stop at the first
failure:

1. The file starts with the magic (`E_PKG_BAD_MAGIC`); at least 128 bytes are
   available (`E_PKG_TRUNCATED`).
2. `format_major == 1` (`E_PKG_UNSUPPORTED_VERSION`). `format_minor` is not
   checked; see §12.
3. `header_check` matches (`E_PKG_INTEGRITY`).
4. No reserved flag bit is set (`E_PKG_UNSUPPORTED_FEATURE`); the reserved
   bytes are zero (`E_PKG_CORRUPT_HEADER`).
5. `2 <= prelude_len <= 65 536`; `1 <= manifest_len <= 16 777 216`
   (`17 <= manifest_len` when `ENCRYPTED`); `total_len <= 64 GiB`
   (`E_PKG_LIMIT`); `total_len >= head_len` (`E_PKG_CORRUPT_HEADER`).
6. When the real size of the file is known: it equals `total_len`. A shorter
   file is `E_PKG_TRUNCATED`; a longer one is `E_PKG_INTEGRITY`.

Only after these checks does the reader read the prelude and the manifest, and
it verifies `prelude_sha256` and `manifest_sha256` before parsing either.

## 4. Prelude

The prelude is a UTF-8 JSON object of at most 64 KiB that holds everything a
reader may know **without the key**. The writer emits it in canonical form:
compact (no insignificant whitespace), members in the order listed below.
Readers accept any member order and ignore members they do not know.

```json
{
  "generator": "darm-golden/1",
  "chunk_size": 65536,
  "card": {
    "title": "The Golden Fixture",
    "authors": ["DARM"],
    "language": "en",
    "cover_color": "#c9a227"
  },
  "encryption": {
    "cipher": "AES-256-GCM",
    "subkeys": "HKDF-SHA256",
    "key_commitment": "2O7VUcdcljlYJr2IDbuRmcAcmqB4EJiID4XRLZDhOJQ=",
    "slots": [
      {
        "kind": "password",
        "kdf": { "algorithm": "argon2id", "version": 19, "m_kib": 65536, "t": 3, "p": 1 },
        "salt": "07D/o0Fwc6gvFn5cN7PKOw==",
        "wrapped_key": "BYJUU+JGsyLkWCBL0Hc8Oz89r67fDHUST6ecm5Ss4vLcLyMpCSGWS6zAe8EQaH/J"
      },
      {
        "kind": "keyfile",
        "key_id": "aa9f45736cc9c55c",
        "wrapped_key": "bNDurNc9T4lMI2gahxUbUcB1V4fHirjku0x9TR+zyVfprhY74qlMbxP2RkWVdOuC"
      }
    ]
  }
}
```

| Member       | Type    | Required | Description |
|--------------|---------|----------|-------------|
| `generator`  | string  | yes | Free-form name and version of the writing tool; at most 256 bytes, no control characters. |
| `chunk_size` | integer | yes | Plaintext chunk size in bytes used by every entry; 65 536 to 4 194 304. |
| `card`       | object  | iff `HAS_PUBLIC_CARD` | The public card (§4.1). |
| `encryption` | object  | iff `ENCRYPTED` | Encryption parameters and key slots (§9). |

The presence of `card` and `encryption` MUST match the header flags
(`E_PKG_CORRUPT_PRELUDE` otherwise).

### 4.1 Public card

The card lets a library show a book it has not unlocked.

| Member        | Type             | Required | Description |
|---------------|------------------|----------|-------------|
| `title`       | string           | yes | Book title. |
| `authors`     | array of strings | no (default `[]`) | Author names in display order. |
| `language`    | string           | no | BCP 47 language tag. |
| `cover_color` | string           | no | Exactly `#` followed by six hexadecimal digits. |

In a clear package the writer stores the card whenever one is supplied. In an
encrypted package the card is stored **only if the author opts in**
(`expose_card`); otherwise the prelude has no `card` and the flag is clear, and
nothing about the book is visible without the key.

The card is public, unkeyed data. In a clear package it is protected against
accidental corruption only. In an encrypted package it is authenticated *once
the package has been unlocked* (§9.5); before that, a reader showing the card of
a locked book is showing unauthenticated text and SHOULD treat it as such.

## 5. Manifest

The manifest is a UTF-8 JSON object, stored as follows:

1. serialised compactly;
2. compressed as one raw DEFLATE stream (RFC 1951, no zlib or gzip wrapper);
3. when the package is encrypted, sealed with AES-256-GCM (§9.5): the stored
   bytes are the ciphertext followed by the 16-byte tag.

Limits: at most 16 MiB as stored and at most 64 MiB after inflation. A reader
MUST bound inflation at 64 MiB and reject a stream that would produce more
(`E_PKG_LIMIT`), and MUST reject a stream with bytes after its end.

```json
{
  "manifest_version": 1,
  "created": "2026-01-01T00:00:00Z",
  "book": { "chapters": 1, "format": "darm", "title": "The Golden Fixture" },
  "entries": [
    {
      "index": 0,
      "name": "book.ir.json",
      "media_type": "application/vnd.darm.ir+json",
      "role": "ir",
      "size": 1885,
      "sha256": "390de94575ac6de93dfbf38dcadafbf2aaf46717b3506b89a3db96b27c63795d",
      "compression": "deflate",
      "offset": 0,
      "stored_size": 239,
      "chunks": [
        { "len": 239, "sha256": "f3675ad4c083de732c690de7d07c8d41437b709c215adc805622dae2164c5043" }
      ]
    }
  ]
}
```

| Member             | Type    | Required | Description |
|--------------------|---------|----------|-------------|
| `manifest_version` | integer | yes | `1`. |
| `created`          | string  | no  | Creation time supplied by the author (RFC 3339 recommended); at most 256 bytes. Absent unless the author supplies it — the writer never reads the clock. |
| `book`             | any JSON value | yes | Opaque summary supplied by the tool that built the book; at most 1 MiB of JSON text. The writer stores it compactly with object members sorted by name. Default `{}`. |
| `entries`          | array   | yes | The entries in storage order; at most 16 384. |

Each entry:

| Member        | Type    | Description |
|---------------|---------|-------------|
| `index`       | integer | Position in `entries`, starting at 0. |
| `name`        | string  | Entry name (§7); unique within the package. |
| `media_type`  | string  | MIME type; 1 to 255 printable ASCII characters. |
| `role`        | string  | `"ir"` for the compiled book (at most one entry, at most 256 MiB) or `"resource"`. |
| `size`        | integer | Size in bytes of the original content; at most 16 GiB. |
| `sha256`      | string  | SHA-256 of the original content, 64 lower-case hex digits. |
| `compression` | string  | `"none"` or `"deflate"` (§6.2). |
| `offset`      | integer | Offset of the entry's first stored byte, **relative to the start of the data area** (absolute offset = `head_len + offset`). |
| `stored_size` | integer | Total stored bytes of the entry (sum of its chunk lengths). |
| `chunks`      | array   | One object per chunk, in order: `len` (stored bytes of the chunk) and `sha256` (SHA-256 of the stored bytes, 64 lower-case hex digits). `sha256` is present on every chunk of a clear package and absent in encrypted packages, where the GCM tag authenticates the chunk. |

The writer names the IR entry `book.ir.json`, gives it the media type
`application/vnd.darm.ir+json` and always stores it as entry 0; that name is
reserved for it.

Entry names, sizes, media types, hashes and the `book` summary all live in the
manifest, so in an encrypted package none of them is visible without the key.

### 5.1 Manifest validation

After parsing, a reader MUST verify all of the following before using any entry
(`E_PKG_CORRUPT_MANIFEST` unless another code is given):

* `manifest_version == 1`.
* At most 16 384 entries, and at most 262 144 chunks per entry
  (`E_PKG_LIMIT`); both are enforced while parsing, so an oversized list is
  never materialised. `book` is at most 1 MiB (`E_PKG_LIMIT`).
* For entry *i*: `index == i`; the name is valid (`E_PKG_BAD_NAME`) and not used
  by an earlier entry (`E_PKG_DUPLICATE_NAME`); the media type is valid;
  `size <= 16 GiB` (`E_PKG_LIMIT`); at most one entry has role `ir`, and it is
  at most 256 MiB.
* The number of chunks equals `ceil(size / chunk_size)` (zero for an empty
  entry).
* In a clear package every chunk has a `sha256`.
* With `plain_len(c)` = `chunk_size` for every chunk but the last and
  `size - chunk_size * (count - 1)` for the last, and `tag` = 16 if encrypted,
  else 0:
  * `compression: "none"`: `len == plain_len + tag`;
  * `compression: "deflate"`: `tag < len <= plain_len + plain_len/8 + 1024 + tag`
    (integer division).
* `stored_size` equals the sum of the chunk lengths.
* **The entries tile the data area exactly:** entry 0 has `offset == 0`, each
  following entry starts where the previous one ends, and the last one ends at
  `total_len - head_len`. Gaps, overlaps, reordering and unreferenced bytes
  are therefore impossible in a valid file.

Because of the last rule and the check of `total_len` against the real file
size, no declared size can exceed what the file actually contains, and the
largest buffer a reader ever needs for one chunk is `chunk_size * 9/8 + 1040`
bytes.

## 6. Data area

### 6.1 Chunking

Each entry is split into plaintext chunks of `chunk_size` bytes (the last one
may be shorter; an empty entry has no chunks). `chunk_size` is fixed for the
whole package: 64 KiB to 4 MiB, 1 MiB by default.

Each plaintext chunk is transformed independently into a *stored chunk*:

```
plaintext chunk --(deflate, if the entry is compressed)--> --(AES-256-GCM, if encrypted)--> stored chunk
```

The stored chunks of an entry are written back to back, in order, and the
entries follow each other in manifest order.

For an entry with `compression: "none"` every chunk except the last has the
same stored length (`chunk_size + tag`), so chunk *c* starts at
`head_len + offset + c * (chunk_size + tag)`. For a deflated entry the start of
chunk *c* is the sum of the preceding `len` values.

**Verbatim storage.** In a clear package, an entry with `compression: "none"`
occupies the byte range `head_len + offset .. head_len + offset + size` and
those bytes are exactly the original content. A reader MAY serve that range
directly (for example to a `<video>` element). Bytes read that way bypass
chunk verification; a reader that needs integrity MUST go through the chunks.

### 6.2 Compression

Compression is a per-entry choice between `none` and `deflate`. In a deflated
entry **every chunk is an independent raw DEFLATE stream** (RFC 1951, no
wrapper), so chunks can still be decoded in any order.

The reference writer deflates an entry only when all of these hold:

* the caller left compression on automatic (the default);
* the media type is text-like: `text/*`, `application/json`, `application/xml`,
  or any type ending in `+json` or `+xml` (which covers `image/svg+xml` and the
  IR); parameters are ignored and the comparison is case-insensitive;
* the deflated chunks are, in total, smaller than the original, and no chunk
  exceeds the stored-length bound of §5.1.

Everything else — images, video, audio, fonts, and text that does not shrink —
is stored verbatim. The writer uses `miniz_oxide` at level 6; the exact deflate
output is not normative and readers accept any valid stream.

When inflating a chunk a reader MUST allocate exactly `plain_len` bytes of
output, MUST fail if the stream would produce more or fewer bytes, and MUST
fail if stored bytes remain after the end of the stream (`E_PKG_INTEGRITY`). A
chunk can therefore never expand beyond its declared plaintext length.

### 6.3 Decoding a chunk

To decode chunk *c* of entry *e* from its stored bytes:

1. The stored length MUST equal `len` from the manifest.
2. Clear package: SHA-256 of the stored bytes MUST equal the chunk's `sha256`.
   Encrypted package: open the chunk with AES-256-GCM as described in §9.6.
3. If the entry is deflated, inflate into exactly `plain_len` bytes (§6.2).
4. The result MUST be `plain_len` bytes long.

Any failure is `E_PKG_INTEGRITY`. A reader that reads a whole entry (or
verifies a package) additionally checks the SHA-256 of the concatenated
plaintext against the entry's `sha256`.

## 7. Entry names

Entry names are relative paths with `/` as the separator. A name MUST satisfy
all of:

* 1 to 512 bytes of UTF-8;
* no leading `/`;
* no empty segment (so no `//` and no trailing `/`), no `.` segment and no `..`
  segment;
* no backslash, no `:`;
* no NUL and no other control character (Unicode category Cc: U+0000–U+001F,
  U+007F–U+009F).

Names are unique and compared byte for byte (no case folding, no Unicode
normalisation). The rules are enforced by the writer when an entry is added
and by the reader for every name found in a manifest (`E_PKG_BAD_NAME`,
`E_PKG_DUPLICATE_NAME`).

These rules make names safe to join under a directory on any platform as far
as traversal is concerned. A tool that extracts entries to disk is still
responsible for platform quirks the format does not forbid (reserved device
names, case-insensitive collisions).

## 8. Integrity model

### 8.1 What is covered

Every byte of a package is covered by a check:

| Bytes                 | Clear package | Encrypted package |
|-----------------------|---------------|-------------------|
| Header 0..120         | `header_check` (truncated SHA-256) | `header_check`; bytes 0..80 are also bound into the manifest's GCM tag |
| Reserved bytes, `header_check` | must be zero / must match | same |
| Prelude               | `prelude_sha256` in the header | `prelude_sha256`, which is bound into the manifest's GCM tag |
| Manifest              | `manifest_sha256` in the header | `manifest_sha256` and its own GCM tag |
| Each stored chunk     | `sha256` in the manifest | its own GCM tag, bound to package, entry, position and length |
| Each entry as a whole | `sha256` of the plaintext in the manifest | same |

Truncation is detected as soon as the file is opened because `total_len` must
equal the file size, and the manifest must tile the data area exactly.

### 8.2 Clear packages: corruption detection, not authenticity

The hashes of a clear package are **unkeyed**. They reliably detect accidental
damage — bit rot, truncated downloads, a bad copy — but they prove nothing
about who produced the file: anyone can modify a clear package and recompute
every hash, and the result is a valid package. A clear `.darm` file is exactly
as trustworthy as the channel it arrived through.

### 8.3 Signatures: reserved extension point

Format 1.0 defines **no signature scheme**. To leave room for one, header flag
bit 2 is reserved for a future signature block. A 1.0 writer never sets it and
a 1.0 reader rejects a file that has it set (§3.1), so a signed file can never
be mistaken for, or silently downgraded to, an unsigned one by an old reader.

### 8.4 Encrypted packages

In an encrypted package every byte is either authenticated under a key derived
from the content key or is a value derived from authenticated bytes. Without a
valid credential an attacker cannot change anything — header fields, the card,
the KDF parameters, the manifest, any chunk, or their order — without the
change being detected, at the latest when the affected part is used:

* changes to the header or the prelude (including the public card) are detected
  when the manifest is opened, i.e. immediately after unlocking;
* changes to a chunk, a swap of two chunks, a chunk moved to another entry or
  transplanted from another package, and a chunk passed off as the final one
  are detected when that chunk is decoded.

Authenticity here means "produced by someone who holds a credential of this
package". It does not identify an author: anyone who can unlock a package also
learns the content key and could build a different package with the same UUID
and key. Identifying the author needs a signature (§8.3).

## 9. Encryption

Encryption is optional and applies to the whole package: the manifest and
every chunk of every entry. Only the header and the prelude stay in the clear.

All primitives are standard and come from the RustCrypto project's
implementations, not from code written for DARM: AES-256-GCM (NIST SP 800-38D)
with 96-bit nonces and 128-bit tags, HKDF-SHA256 (RFC 5869), SHA-256, and
Argon2id version 1.3 (RFC 9106). Which of those crates have had an independent
audit, and which have not, is stated in [`security.md`](security.md#4-cryptography).

### 9.1 Key hierarchy

```
password ---- Argon2id(salt, m, t, p) ------------> KEK --+
                                                          +-- AES-256-GCM unwrap --> CEK
.darmkey key - HKDF-SHA256(salt = UUID, slot index) -> KEK --+                       |
                                                                                     |
                 HKDF-SHA256(salt = UUID, ikm = CEK, info = "darm/1/manifest-key")   --> manifest key
                 HKDF-SHA256(salt = UUID, ikm = CEK, info = "darm/1/data-key")       --> data key
                 HKDF-SHA256(salt = UUID, ikm = CEK, info = "darm/1/key-commitment") --> key commitment (public)
```

* The **content key** (CEK) is 32 bytes from the operating system's random
  source, generated anew for every build. It is never used directly as a
  cipher key.
* Each **key slot** (§9.3) stores the CEK wrapped under a key-encryption key
  (KEK) derived from one credential. Any one slot unlocks the package.
* The **manifest key** and the **data key** are 32-byte HKDF-SHA256 outputs
  with the package UUID as salt and distinct `info` strings.

The `encryption` object of the prelude:

| Member           | Type   | Description |
|------------------|--------|-------------|
| `cipher`         | string | `"AES-256-GCM"`. Any other value is `E_PKG_UNSUPPORTED_FEATURE`. |
| `subkeys`        | string | `"HKDF-SHA256"`. Any other value is `E_PKG_UNSUPPORTED_FEATURE`. |
| `key_commitment` | string | Base64 of the 32-byte key commitment (§9.4). |
| `slots`          | array  | 1 to 8 key slots. |

Base64 everywhere in this format is the standard alphabet with padding
(RFC 4648 §4).

### 9.2 Password key derivation and its bounds

A password slot derives its KEK with Argon2id, version 0x13, no secret and no
associated data, a 16-byte random salt and a 32-byte output. The password is
the UTF-8 encoding of the text the user typed, used as is; applications SHOULD
normalise passwords to Unicode NFC before writing and before reading. (The
WebAssembly binding retries a failed password once in NFC form.)

Parameters are stored per slot:

| Parameter | Default | Accepted range |
|-----------|---------|----------------|
| `m_kib` (memory, KiB) | 65 536 (64 MiB) | 8 192 (8 MiB) to 1 048 576 (1 GiB) |
| `t` (passes)          | 3  | 1 to 16 |
| `p` (lanes)           | 1  | 1 to 4  |

Writers refuse to write, and readers refuse to open (`E_PKG_LIMIT`, before any
key derivation runs), a package with a password slot outside these ranges. The
upper bounds stop a hostile file from making a reader spend unbounded memory
or time; the lower bounds refuse files whose password protection is too weak
to be worth the name. The check applies to every password slot of the file,
whichever credential is used to open it.

### 9.3 Key slots

A slot is a JSON object with a `kind`. Its index is its position in `slots`,
starting at 0.

**Password slot**

| Member        | Description |
|---------------|-------------|
| `kind`        | `"password"` |
| `kdf`         | `{ "algorithm": "argon2id", "version": 19, "m_kib": …, "t": …, "p": … }` |
| `salt`        | Base64 of 16 random bytes. |
| `wrapped_key` | Base64 of 48 bytes: the wrapped CEK. |

`KEK = Argon2id(password, salt, m_kib, t, p)`.

**Keyfile slot**

| Member        | Description |
|---------------|-------------|
| `kind`        | `"keyfile"` |
| `key_id`      | 16 lower-case hex digits identifying the key (§10). |
| `wrapped_key` | Base64 of 48 bytes: the wrapped CEK. |

`KEK = HKDF-SHA256(salt = UUID, ikm = keyfile key, info = "darm/1/keyfile-kek" || u32-BE(slot index))`.

**Wrapping.** `wrapped_key = AES-256-GCM(key = KEK, nonce = 12 zero bytes, plaintext = CEK, aad = slot_aad)`,
stored as the 32-byte ciphertext followed by the 16-byte tag. The fixed nonce is
safe because each KEK encrypts exactly one message: a password KEK depends on
that slot's random salt, a keyfile KEK on the package's random UUID and the
slot index. The writer also refuses to add the same keyfile key twice.

```
slot_aad (password) = "darm/1/slot\0" || UUID || u32-BE(slot index) || 0x01
                      || u32-BE(19) || u32-BE(m_kib) || u32-BE(t) || u32-BE(p) || salt
slot_aad (keyfile)  = "darm/1/slot\0" || UUID || u32-BE(slot index) || 0x02 || key_id (8 raw bytes)
```

**Unknown slots.** A slot whose `kind` is not known, or a password slot whose
`kdf.algorithm`/`version` is not `argon2id`/`19`, is skipped: it can never be
used to unlock, but it does not make the file invalid. This lets a later minor
version add slot kinds.

**Unlocking.** With a password, the reader tries every password slot in order;
with a keyfile key, every keyfile slot whose `key_id` matches the key. The
first slot whose GCM tag verifies yields the CEK. If none does the result is
`E_PKG_WRONG_CREDENTIALS`. Opening an encrypted package with no credential is
`E_PKG_CREDENTIALS_REQUIRED` (the error carries the public card when there is
one).

### 9.4 Key commitment

AES-GCM does not commit to its key: a ciphertext can be crafted to decrypt
successfully under two different keys. Without a countermeasure a malicious
author could build one file whose slots unwrap *different* content keys and
therefore show different books to different readers.

`key_commitment = HKDF-SHA256(salt = UUID, ikm = CEK, info = "darm/1/key-commitment")` (32 bytes)

After unwrapping a CEK the reader recomputes this value and compares it with
the prelude; a mismatch is `E_PKG_INTEGRITY`. Every slot of a file therefore
leads to the same content key. The commitment is an HKDF output under its own
label and reveals nothing about the CEK or the other sub-keys.

### 9.5 Sealing the manifest

```
stored manifest = AES-256-GCM(key = manifest key, nonce = 12 zero bytes,
                              plaintext = deflate(manifest JSON),
                              aad = "darm/1/manifest\0" || header bytes 0..80)
```

The manifest key encrypts only this one message. The associated data is the
first 80 bytes of the header — magic, version, flags, UUID, `total_len`,
`prelude_len`, `manifest_len` and `prelude_sha256` — so the header fields and,
through the hash, the whole prelude (card, chunk size, KDF parameters, slots)
are authenticated together with the manifest. `manifest_sha256` is not part of
the associated data: it is the hash of the ciphertext itself.

A failure to open the manifest after a slot has been unwrapped successfully
means the file was modified; it is reported as `E_PKG_INTEGRITY`, never as a
wrong password.

### 9.6 Sealing chunks

Every chunk is sealed separately with the data key:

```
stored chunk = AES-256-GCM(key = data key, nonce = chunk_nonce, plaintext = chunk bytes, aad = chunk_aad)

chunk_nonce  = u32-BE(entry index) || u64-BE(chunk index)                      (12 bytes)

chunk_aad    = "darm/1/chunk\0" || UUID || u32-BE(entry index) || u64-BE(chunk index)
               || final || u32-BE(plain_len) || compression                    (47 bytes)
```

* "chunk bytes" are the plaintext chunk, or its deflate stream if the entry is
  compressed; the stored chunk is the ciphertext followed by the 16-byte tag.
* `final` is `0x01` for the last chunk of its entry and `0x00` otherwise.
* `plain_len` is the length of the chunk's plaintext before compression.
* `compression` is `0x00` for `none` and `0x01` for `deflate`.

Nonces never repeat under a data key: the (entry, chunk) pair is unique within
a package and the data key is derived from a CEK that is fresh for every
build. A package has at most 16 384 entries of at most 262 144 chunks of at
most 4 MiB each — and in practice far fewer, since the whole file is limited
to 64 GiB — which stays well inside the usage limits of AES-GCM with unique
96-bit nonces.

This is the STREAM construction adapted to random access. The associated data
pins every chunk to its package, its entry, its position, whether it is the
last chunk, and its length, so reordering chunks, swapping chunks between
entries or packages, truncating an entry and extending one all fail
authentication. (The chunk count and sizes are also fixed by the authenticated
manifest; the two mechanisms are independent.)

### 9.7 What an observer without the key can learn

The header and prelude: the format version, the package UUID, the total size,
the chunk size, the generator string, the number and kinds of key slots with
their KDF parameters and key ids, and the card *if the author exposed it*. The
size of the manifest and the total size of the data are visible too. Entry
names, the number of entries, individual sizes, media types, the book summary
and all content are not.

### 9.8 Handling of secrets in the reference implementation

Every secret the library owns — the passwords and keyfile keys given to the
writer, KEKs, the CEK, derived sub-keys and parsed key files — lives in a
buffer that is zeroed when dropped, and the AES key schedules are zeroed on
drop as well. Credentials passed to the reader are borrowed and stay the
caller's responsibility. After opening, only the data and manifest ciphers are
kept; the CEK itself is discarded.

## 10. Key files (`.darmkey`)

A key file holds a 32-byte random key that is kept **outside** the book and
unlocks keyfile slots. It is UTF-8 JSON text of at most 4 096 bytes:

```json
{
  "darmkey": 1,
  "key_id": "aa9f45736cc9c55c",
  "key": "EBESExQVFhcYGRobHB0eH6ChoqOkpaanqKmqq6ytrq8=",
  "created": "2026-11-16T09:30:00Z"
}
```

| Member    | Type    | Required | Description |
|-----------|---------|----------|-------------|
| `darmkey` | integer | yes | Version of the key file format, `1`. |
| `key`     | string  | yes | Base64 (with padding) of exactly 32 bytes. |
| `key_id`  | string  | no  | `hex(SHA-256("darm/1/key-id\0" || key)[0..8])`, 16 lower-case hex digits. When present it MUST match the key, which catches a damaged file. |
| `created` | string  | no  | Creation time; the generator writes the current UTC time in RFC 3339 form. |

Unknown members are ignored. The `key_id` is a one-way function of a 256-bit
random key and reveals nothing useful about it; packages store it in their
keyfile slots so a reader can tell the user which key a book needs.

The example above is the test key used by the fixtures of this repository; it
protects nothing.

## 11. Limits

All limits are public constants of `darm-package` and are enforced by the
writer and by the reader.

| Quantity | Limit |
|----------|-------|
| Fixed header | exactly 128 bytes |
| Prelude | 2 bytes to 64 KiB |
| Manifest as stored | 1 byte to 16 MiB |
| Manifest after inflation | 64 MiB |
| `book` summary | 1 MiB of JSON text |
| Entries per package | 16 384 |
| Entry name | 1 to 512 bytes |
| Media type | 1 to 255 bytes |
| `generator`, `created` | 256 bytes |
| Chunk size | 64 KiB to 4 MiB (default 1 MiB) |
| Chunks per entry | 262 144 |
| Stored length of a deflated chunk | `plain_len + plain_len/8 + 1024` (+16 when encrypted) |
| Entry size (plaintext) | 16 GiB |
| IR entry size | 256 MiB |
| Package size | 64 GiB |
| Key slots | 1 to 8 |
| Argon2id memory | 8 MiB to 1 GiB |
| Argon2id passes | 1 to 16 |
| Argon2id lanes | 1 to 4 |
| Key file | 4 096 bytes |

Every size and offset in a package is below 2^53, so all of them are exactly
representable as JavaScript numbers.

## 12. Versioning and compatibility

The header carries `format_major.format_minor`; this document describes 1.0.

* **Unknown major version → reject.** A reader of major version 1 rejects any
  other major version with `E_PKG_UNSUPPORTED_VERSION` and a message telling
  the user to update the reader. A new major version may change anything
  except the position of the magic and of `format_major`.
* **Newer minor version → accept.** A minor version only ever *adds*: new
  optional JSON members in the prelude, the manifest, entries, chunks or slots,
  and new slot kinds. A 1.0 reader accepts a 1.x file, ignores the members it
  does not know and skips the slots it does not understand. Nothing a 1.0
  reader relies on may change meaning in a minor version.
* **Reserved flag bit set → reject** with `E_PKG_UNSUPPORTED_FEATURE`. Flag
  bits announce features a reader cannot safely ignore.
* Anything a 1.0 reader could not ignore — a new cipher or sub-key derivation,
  a new compression method, a new entry role, a different header layout, a new
  `manifest_version` — is not a minor change. A 1.0 reader rejects such files
  (`E_PKG_UNSUPPORTED_FEATURE` or `E_PKG_CORRUPT_MANIFEST`).
* Writers always write the lowest version that can represent the package; the
  reference writer writes 1.0.

The repository guards these promises with a committed fixture,
`crates/darm-package/tests/fixtures/golden-v1.darm`: a test checks that it
still opens and that rebuilding the same inputs reproduces it byte for byte.
Two encrypted fixtures guard the key derivation and AEAD framing the same way
on the reading side.

## 13. Migration policy

* A file written as 1.x stays a valid 1.x file forever; readers never rewrite a
  package in place.
* Readers keep support for older major versions for as long as files in that
  version may reasonably exist. A reader that understands format 2 is expected
  to still open format 1.
* Migration is explicit and offline, and the `darm` command-line tool is the
  migration tool. Moving a book from 1.x to 2.0 means reading the 1.x package
  with a 1.x-capable reader (unlocking it if it is encrypted) and writing a
  new 2.0 package from its entries, book summary and card. The original file
  is left untouched.
* Migrating an encrypted package requires a credential and produces a new
  content key and UUID, because key material is never reused across files.
  The same password or key file can protect the new file; key files
  themselves are independent of the container version and do not change.
* Because the migrated file is a new build, its hashes — and for a clear
  package its content-derived UUID — differ from the original's. Applications
  that track books by UUID should record the mapping at migration time.
* A format change that does not need a migration (a purely additive one) is
  released as a minor version instead, precisely so that no one has to convert
  anything.

## 14. Reading a package, step by step

This is the procedure the reference reader follows; it is also what a
streaming reader (a browser, an HTTP range client) does with `probe`,
`PackageHead::open` and `decode_chunk`.

1. Read the first 128 bytes and validate the header (§3.3). This yields
   `head_len` and `total_len`. Compare `total_len` with the real file size.
2. Read the first `head_len` bytes. Verify the prelude against
   `prelude_sha256`, parse it and validate it against the flags and limits.
   Reading the first 65 664 bytes (128 + 64 KiB) of a file is always enough to
   get the prelude, and therefore the public card, in one request.
3. Verify the stored manifest against `manifest_sha256`.
4. If `ENCRYPTED`: obtain a credential, unwrap the CEK from a slot (§9.3),
   check the key commitment (§9.4), derive the sub-keys and open the manifest
   (§9.5).
5. Inflate the manifest (bounded at 64 MiB), parse it and validate it (§5.1).
6. To read chunk *c* of entry *e*: compute its location (§6.1), read exactly
   `len` bytes there, and decode them (§6.3).

## 15. Worked example: the header of the golden fixture

`crates/darm-package/tests/fixtures/golden-v1.darm` is a clear package of
67 298 bytes with a card, an IR and three resources, built with 64 KiB chunks.
Its first 128 bytes:

```
00000000: 8944 4152 4d0d 0a1a 0100 0000 0200 0000  .DARM...........
00000010: 7dcd 6c88 abac 85ae 81d8 778e 6b42 7bdc  }.l.......w.kB{.
00000020: e206 0100 0000 0000 9100 0000 c002 0000  ................
00000030: 928c cbb5 2141 4c9e b6af bb6e 2143 ee84  ....!AL....n!C..
00000040: 775a 4171 f29a ca7f c897 0098 bf8a 03c0  wZAq............
00000050: 613d 5455 6917 1955 eee9 f133 e632 9b7f  a=TUi..U...3.2..
00000060: 9508 8c2c fecb 335e d1be ace5 a9a5 9397  ...,..3^........
00000070: 0000 0000 0000 0000 8758 9313 43ed 254e  .........X..C.%N
```

| Offset | Bytes | Field | Value |
|-------:|-------|-------|-------|
| 0x00 | `89 44 41 52 4d 0d 0a 1a` | `magic` | `"\x89DARM\r\n\x1a"` |
| 0x08 | `01 00` | `format_major` | 1 |
| 0x0a | `00 00` | `format_minor` | 0 |
| 0x0c | `02 00 00 00` | `flags` | `0x00000002`: `HAS_PUBLIC_CARD`, not encrypted |
| 0x10 | `7d cd 6c 88 ab ac 85 ae 81 d8 77 8e 6b 42 7b dc` | `uuid` | `7dcd6c88-abac-85ae-81d8-778e6b427bdc` (version 8, content-derived) |
| 0x20 | `e2 06 01 00 00 00 00 00` | `total_len` | `0x000106e2` = 67 298 |
| 0x28 | `91 00 00 00` | `prelude_len` | 145 |
| 0x2c | `c0 02 00 00` | `manifest_len` | 704 |
| 0x30 | `92 8c cb b5 … bf 8a 03 c0` | `prelude_sha256` | `928ccbb521414c9eb6afbb6e2143ee84775a4171f29aca7fc8970098bf8a03c0` |
| 0x50 | `61 3d 54 55 … a9 a5 93 97` | `manifest_sha256` | `613d545569171955eee9f133e6329b7f95088c2cfecb335ed1beace5a9a59397` |
| 0x70 | `00 00 00 00 00 00 00 00` | `reserved` | zero |
| 0x78 | `87 58 93 13 43 ed 25 4e` | `header_check` | first 8 bytes of SHA-256 over bytes 0x00..0x78 |

From these values: the prelude occupies bytes 128..273, the manifest bytes
273..977, so `head_len` = 977 and the data area is bytes 977..67 298
(66 321 bytes).

The prelude (145 bytes) is:

```json
{"generator":"darm-golden/1","chunk_size":65536,"card":{"title":"The Golden Fixture","authors":["DARM"],"language":"en","cover_color":"#c9a227"}}
```

The manifest inflates from 704 to 1 448 bytes and lists four entries:

| Index | Name | Role | Size | Compression | Offset | Stored size | Chunks (stored bytes) |
|------:|------|------|-----:|-------------|-------:|------------:|-----------------------|
| 0 | `book.ir.json`      | `ir`       | 1 885  | `deflate` | 0      | 239    | 239 |
| 1 | `media/noise.bin`   | `resource` | 66 000 | `none`    | 239    | 66 000 | 65 536, 464 |
| 2 | `text/captions.vtt` | `resource` | 3 000  | `deflate` | 66 239 | 82     | 82 |
| 3 | `empty`             | `resource` | 0      | `none`    | 66 321 | 0      | — |

The stored sizes add up to 66 321, the length of the data area. Entry 1 is
uncompressed, so its 66 000 original bytes sit verbatim at absolute offsets
977 + 239 = 1 216 to 67 216; its second chunk starts at 1 216 + 65 536 = 66 752.

## 16. Error codes

The reference implementation reports every failure with a stable code, exposed
as `PackageError::code()` in Rust and as the `code` property of thrown errors
in the WebAssembly binding.

| Code | Meaning |
|------|---------|
| `E_PKG_BAD_MAGIC` | Not a `.darm` file. |
| `E_PKG_TRUNCATED` | The file (or the supplied head) is shorter than required. |
| `E_PKG_UNSUPPORTED_VERSION` | Unknown major version; update the reader. |
| `E_PKG_UNSUPPORTED_FEATURE` | Reserved flag bit, unknown cipher or sub-key derivation. |
| `E_PKG_LIMIT` | A size or parameter is outside the ranges of §11. |
| `E_PKG_INTEGRITY` | A checksum, hash, key commitment or authentication tag does not match, or the file length differs from `total_len`. |
| `E_PKG_CORRUPT_HEADER` | The header is structurally invalid. |
| `E_PKG_CORRUPT_PRELUDE` | The prelude is not valid for this format. |
| `E_PKG_CORRUPT_MANIFEST` | The manifest is not valid for this format. |
| `E_PKG_WRONG_CREDENTIALS` | The password or key does not unlock the package. |
| `E_PKG_CREDENTIALS_REQUIRED` | The package is encrypted and no credential was given. |
| `E_PKG_BAD_NAME` | An entry name violates §7. |
| `E_PKG_DUPLICATE_NAME` | Two entries share a name. |
| `E_PKG_NOT_FOUND` | No such entry or chunk. |
| `E_PKG_BAD_KEYFILE` | A `.darmkey` file is malformed. |
| `E_PKG_BAD_OPTION` | Invalid writer or reader option. |
| `E_PKG_KDF` | Argon2id could not run (typically not enough memory). |
| `E_PKG_OUT_OF_MEMORY` | A buffer the format legitimately needs could not be allocated. |
| `E_PKG_SOURCE_CHANGED` | An input file changed while the package was being written. |
| `E_PKG_RANDOM` | The operating system's random source failed. |
| `E_PKG_IO` | An I/O error of the underlying file. |
