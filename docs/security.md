# Security

What DARM protects, what it does not, and where each defence lives in the
code. The byte-level description of the container and its cryptography is in
[`file-format.md`](file-format.md); this document is the threat model and the
map.

## 1. Summary

* A `.darm` file is **data, never a program**. Nothing in a book can run code,
  open a file on the device or contact a server on its own.
* Every file a reader opens is treated as **hostile input** and is parsed,
  verified and decrypted by one Rust implementation (compiled natively for the
  CLI and to WebAssembly for the reader).
* **Protected books** are encrypted with AES-256-GCM under a key derived from a
  password (Argon2id) or held in a separate key file. Without the key, the
  content — text, media, even the list of files — cannot be read from the
  package, by a person, a program or an AI model.
* **This is not copy protection.** Once a book is unlocked, whoever unlocked it
  can see it, and anything that can be seen can be captured (§3).

## 2. Threat model

| # | Adversary | Wants | Defence |
|---|---|---|---|
| T1 | Someone who obtains a protected `.darm` file but not its key (a leak, a shared drive, a crawler, an AI model given the file) | To read or extract the book | Authenticated encryption of the manifest and of every chunk; only the header and prelude are clear (§4) |
| T2 | Someone who alters a file in transit or at rest | To corrupt it unnoticed, or to change a protected book | SHA-256 per chunk and per entry (clear books: detects damage); GCM tags bound to position (protected books: detects any change) (§5) |
| T3 | A malicious author | To attack the reader's device through a crafted book: run code, read files, reach the network, exhaust memory or CPU, phish | No scripting in the language or IR; validation; limits; sandboxed decoding; strict Content-Security-Policy; consent for network and external links (§6, §7) |
| T4 | A malicious project given to the compiler (or a careless one) | To make the compiler read files outside the project or embed unsafe content | Path confinement, symlink check, content sniffing, SVG rules (§6.1) |
| T5 | A web page or a local process | To use the reader or the CLI preview server as a way to the user's files | No file-system permission in the webview; token registry in the desktop shell; loopback-only server with Host check (§8) |

Out of scope: an attacker who controls the reader's device or account (malware,
a compromised OS or browser), weak passwords chosen by authors, side channels
in the cryptographic primitives, and denial of service by simply sending a very
large valid file.

## 3. Limits of protection — read this before selling a "protected" book

DARM's protection is **confidentiality of the file at rest and in transit**.
It is not, and cannot be, digital rights management in the sense of preventing
an authorised reader from copying what they read:

* The reader must decrypt the book to show it. On the screen it can be
  screenshotted or filmed; in memory it can be dumped by a debugger; through
  the accessibility outline it is available as text to assistive tools — and to
  anything else that reads the page.
* A password can be shared. A key file can be copied. There is no licence
  server and no per-device binding in the open tools.
* The reader is open source: anyone can build a variant that writes the
  decrypted content to disk once they hold a valid key.
* The **public card** (title, authors, language, cover colour) of a protected
  book is readable by everyone unless the author builds with `--hide-card`.
  Sizes are visible regardless: the total size, the size of the manifest, and
  (from the number of key slots) how many credentials exist.
* Clear (unencrypted) books have **no authenticity**: the hashes detect damage,
  but anyone can change the book and recompute them. Format 1.0 has no
  signatures (a header flag is reserved for them).

So: use protection to keep a book private between an author and the people
given the key, or to distribute files over channels you do not trust. Do not
promise buyers or authors that an unlocked book cannot be copied.

## 4. Cryptography

Nothing here is home-made. The construction is described exactly in
[`file-format.md` §9](file-format.md#9-encryption); in brief:

| Purpose | Primitive | Crate |
|---|---|---|
| Content and manifest encryption, key wrapping | AES-256-GCM, 96-bit nonces, 128-bit tags | `aes-gcm` 0.11 (RustCrypto) |
| Password → key | Argon2id v1.3; default 64 MiB, 3 passes, 1 lane; 16-byte random salt | `argon2` 0.6 (RustCrypto) |
| Sub-keys, key-file KEK, key commitment | HKDF-SHA256 | `hkdf` 0.13, `sha2` 0.11 (RustCrypto) |
| Integrity of clear packages | SHA-256 | `sha2` |
| Randomness (writer only) | operating-system CSPRNG | `getrandom` 0.4 |
| Secret hygiene | zeroing on drop | `zeroize` |

**Audit status, stated plainly.** The RustCrypto `aes-gcm` crate was reviewed
by NCC Group in 2020 (an earlier version than the one pinned here). To our
knowledge the `argon2`, `hkdf` and `sha2` crates have not had a published
independent audit; they are widely used and tested against the RFC vectors,
and DARM's own tests include known-answer and round-trip tests, but that is
not the same thing. DARM itself — the container construction, the reader and
the compiler — **has not been audited by anyone**. Treat it accordingly.

Design points worth knowing:

* **Fresh content key per build.** A random 32-byte content key (CEK) is
  generated for every encrypted build; cipher keys are derived from it with
  HKDF and never reused across packages, so chunk nonces can be positional
  (entry index ‖ chunk index) without ever repeating under one key.
* **Chunks are bound to their place.** The associated data of every chunk
  covers the package UUID, the entry, the chunk index, a final-chunk flag and
  the plaintext length. Reordering chunks, swapping them between entries or
  packages, truncating or extending an entry all fail authentication.
* **The clear part is authenticated too.** The manifest's associated data
  binds the fixed header and the hash of the prelude, so changing the public
  card or the key slots is detected as soon as the book is unlocked.
* **Key commitment.** AES-GCM does not commit to its key, so the prelude
  carries an HKDF-derived commitment that is checked after unwrapping; a file
  cannot be built that shows different books to different key holders.
* **Bounded key derivation.** A reader refuses Argon2 parameters outside
  8 MiB–1 GiB / 1–16 passes / 1–4 lanes before deriving anything; the web
  reader is stricter (256 MiB, 8 passes — `LIMITS` in
  `apps/reader/src/core/book.ts`), so a hostile file cannot make a phone
  allocate a gigabyte to check a password.
* **Passwords** are NFC-normalised by the CLI and the reader, so the same typed
  text works on every platform. The CLI refuses passwords shorter than 8
  characters and never accepts one on the command line (only an environment
  variable, a file, stdin or an interactive prompt), so they do not end up in
  shell history or process listings. `darm.toml` rejects any key that would
  hold a secret.

### Key management

* **Password slots and key-file slots** can be combined (up to 8); any one
  unlocks the book.
* **Key files** (`.darmkey`) hold a random 32-byte key *outside* the book.
  `darm keygen` and `darm build --new-keyfile` create them with mode `0600` and
  refuse to overwrite an existing one; the project template git-ignores
  `*.darmkey`. Losing the key file (and every password) means losing the book:
  there is no recovery and no back door.
* **Rotation** means rebuilding: a new build has a new content key. Copies
  already distributed keep working with the old credentials.
* **The reader never stores a password or key.** It asks every time a
  protected book is opened. The library keeps the *encrypted* file as it was
  imported and does not store a cover thumbnail for protected books, so what
  is on disk in the browser's storage is no more revealing than the file
  itself. Reading position, bookmarks and the values of state variables for
  that book *are* stored unencrypted in the browser's storage on the device.

## 5. Integrity

| Package | Check | When |
|---|---|---|
| Any | Magic, version, reserved flags, header checksum, declared total length = real length | On probe, before anything else |
| Clear | SHA-256 of prelude and manifest (in the header); SHA-256 of every stored chunk; SHA-256 of every entry's content | Manifest on open; **every chunk when a book is imported into the library** (`verifySource`), and whenever a chunk is decoded |
| Protected | GCM tag on the manifest and on every chunk | Every time anything is decrypted |

`darm verify FILE` checks everything in a file and exits with code 4 on any
mismatch. The reader refuses to add a damaged or altered clear book to the
library and says why; for protected books a modification surfaces as an
integrity error the moment the affected part is decrypted.

## 6. Hostile input

### 6.1 The compiler (sources and assets)

* **Paths** in `include` and asset declarations are relative, `/`-separated
  and normalised lexically; anything that would leave the project directory is
  rejected (`E0402`), and on disk the canonical path must still be inside the
  project, so a symlink is not a way out
  (`crates/darm-compiler/src/project.rs`).
* **Assets are identified by content**, not extension, and must match what the
  declaration says (`image(...)` of an MP4 is `E0403`). Dimensions and
  durations are read with bounded header parsers; files are hashed as a
  stream.
* **SVG must be static**: scripts, event handlers, animation elements,
  `foreignObject`, entity declarations, external references and scriptable CSS
  are rejected (`E0405`, `check_svg` in `assets.rs`). This is defence in depth:
  the reader also only ever shows SVG through an `<img>`, where a browser runs
  no script and loads nothing external.
* **The language cannot express computation that does not end**: no loops, no
  calls, components are expanded at compile time with a depth limit and
  recursion check, nesting depth is bounded in the parser, and limits on
  blocks, options, actions and expression depth are enforced
  ([`language.md` §14](language.md#14-semantics-and-limits)).
* The lexer, parser, formatter and lowering are exercised by a mutation test
  (1 500 random edits of a full-featured source per run, asserting no panic
  and valid IR on success) and have cargo-fuzz targets in `fuzz/`.

### 6.2 The container

Everything in [`file-format.md` §11](file-format.md#11-limits): size and count
limits enforced on write *and* read, declared sizes checked against the real
file before allocating, checked arithmetic, entry names that cannot traverse
paths, inflation bounded by the declared chunk length (no decompression
bombs), Argon2 bounds. The package tests include byte-flips in every region,
truncation at many offsets, chunk and entry swaps, lying sizes, crafted
deflate streams, bad names, and a seeded mutation run. Every Rust crate
except the WebAssembly binding (whose generated glue needs it) is compiled
with `#![forbid(unsafe_code)]`.

### 6.3 The IR and the reader

* The reader **never evaluates DARM source**; it receives IR. Before the
  JavaScript side reads a single field, the IR is parsed and validated by
  `darm_ir::validate` in WebAssembly (`DarmBook.parse`): versions, limits,
  every cross-reference (resources, anchors, variables), expression types,
  colours, URL schemes, network policy.
* **Book text is never interpreted as markup.** Pages are painted on a canvas
  from plain strings; the accessibility outline and all UI are built with
  `textContent`. There is no `innerHTML`, `eval` or `Function` in the reader.
* **State is bounded**: integers clamp to their declared range, text
  variables are closed sets, arithmetic saturates. Persisted state is
  re-validated against the book's declarations when loaded
  (`reconcileState`).
* **Media** is handed to the browser's own decoders as blobs with the media
  type the IR declares. Image size is capped (64 megapixels, 96 MiB) before
  decoding.
* **Resource limits**: page textures have a byte budget with eviction; a page
  texture is capped at about 5 megapixels; the IR is capped at 64 MiB in the
  reader.

## 7. Network and privacy

* **The reader makes no network requests for a book** unless the book declares
  remote media. That requires all of: `offline = false`, the host in the
  book's `network` list, an `https` URL, the resource being audio or video
  (images, fonts and captions can never be remote), and **the reader's user
  approving that host for that book** when they first try to play it. The
  end-to-end tests fail if any request leaves the machine while the example
  books are read.
* **Content-Security-Policy** (production web build and desktop shell):
  scripts only from the app itself plus WebAssembly, no `connect-src` beyond
  the app, `object-src 'none'`, `base-uri 'none'`, `form-action 'none'`. The
  only cross-origin loads permitted at all are `https` media.
* **External links** in a book never navigate by themselves: the reader shows
  the real destination and asks; only `https:`, `http:` and `mailto:` exist in
  the IR.
* **No telemetry, no account, no server.** The library, reading positions,
  bookmarks and preferences live in the browser's storage on the device
  (IndexedDB and `localStorage`) and nowhere else.
* Search results never reveal text from a branch of an interactive book that
  is not part of the book in the reader's current state.

## 8. Platform surfaces

* **Web/PWA.** Files are only ever opened by an explicit user action (file
  picker, drop, or the OS handing a file to an installed app). The service
  worker caches only the application's own files and never touches books.
* **Desktop shell (Tauri).** The webview has no file-system, shell or HTTP
  permission. Files the OS hands to the app are registered in Rust under
  random tokens — regular `.darm` files only, symlinks resolved — and the
  webview can only read byte ranges of a file it was given a token for
  (`apps/desktop/src-tauri`). Navigation away from the app is blocked;
  external links go to the system browser.
* **`darm open` / `darm dev`.** The preview server binds to `127.0.0.1` only,
  answers only GET/HEAD, rejects requests whose `Host` is not the loopback
  address (DNS-rebinding defence), serves only the reader's files and the one
  book, and normalises paths (`crates/darm-cli/src/serve.rs`).
* **MCP server (`darm mcp`).** It runs with the invoking user's permissions
  and reads/writes the paths it is asked to, exactly like the CLI. Give it to
  agents you would give the CLI to.

## 9. Known gaps

* No signatures: a clear book cannot prove who made it.
* No independent audit (§4).
* Reading state for protected books is stored unencrypted on the device.
* The web reader keeps an unlocked book's decrypted resources in memory (and
  as blob URLs) while it is open; they are released when the book is closed.
* Remote media, once approved, lets the remote host see the reader's IP
  address and when they play it. The consent dialog says which host.
* `prefers-reduced-data`, certificate pinning and per-device licences do not
  exist.
* The fuzz targets exist but have only been compiled, not run for long
  periods; see [`testing.md`](testing.md).

## 10. Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Write to the
maintainers privately (the security contact of the repository once it is
published; until then, the project owner) with a description, a file or
source that reproduces it, and the versions involved (`darm --version`,
`darm inspect FILE`).
