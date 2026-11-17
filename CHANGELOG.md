# Changelog

All notable changes to DARM are recorded in this file. The format follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and the
toolchain version follows [Semantic Versioning](https://semver.org/).

Four things are versioned independently of the toolchain and are named in
every entry that touches them: the **grammar** (`darm N` pragma), the **IR**
(`ir_version`), the **container format** (`format_major.format_minor`) and the
**key file** format. See [`docs/architecture.md`](docs/architecture.md#3-versioning).

This file lists what exists in the code. Whether each part was built and run,
and on which machines, is recorded in [`README.md`](README.md),
[`docs/platforms.md`](docs/platforms.md) and [`docs/testing.md`](docs/testing.md).

## Unreleased

### Changed

- Reader shell redesigned: always dark (`#090909`) with one accent
  (`#b890d9`), floating translucent controls that fade while reading, an
  accent progress line, floating panels. The book fills the window.
- Pages are paper by default. The setting is now "Pages colour: Paper /
  Night" and no longer follows the system's dark mode.
- Page turns are faster: the turn spring went from 8.4 to 13.5 rad/s (about
  0.45 s instead of 0.75 s); release, board and riffle timings follow.
- A leaf in the air keeps more of its paper colour (ambient light 0.76
  instead of 0.58).
- `darm check --format json`: `book.chapters` counts authored chapters only;
  the generated title page, colophon and contents are reported as
  `book.generated_front_matter`.
- `darm init --template interactive` now includes a component and a back
  cover.

### Added

- `darm reference --guide` prints the complete language guide from inside
  the binary. `darm reference` and `darm reference --json` now describe
  state declarations, expressions, conditions and components.
- End-to-end tests for fast page turning, including a seeded random walk
  through the interactive example (`e2e/turning-fast.spec.ts`).

### Fixed

- Pages with entrance animations could stay frozen at the start of their
  animation (text or images missing) when they had been painted ahead of a
  turn. The animation clock now starts when a page comes into view.
- Choosing an option no longer replays the entrance animations of the page
  the reader is looking at.
- An error in a position update can no longer stop the frame loop with a
  half-turned page on screen.
- Removed a made-up repository URL from the package metadata.

## 0.1.0 — 2026-11-16

First version. Grammar 1, IR 1.0, container format 1.0, key file format 1.

### Added

**Language (grammar 1)** — [`docs/language.md`](docs/language.md)

- Indentation-based, declarative `.darml` source with a uniform grammar:
  elements, assignments, `if`/`elif`/`else`, component definitions.
- Top level: `book`, `theme`, `assets`, `state`, `cover`, `back_cover`,
  `component`, `chapter`, `include`.
- Content blocks: `section`, `heading`, `text` (eight styles), `quote`,
  `list`/`item`, `image`, `gallery`, `video`, `audio`, `divider`, `space`,
  `break`, `box`, `button`, `choice`/`option`, `quiz`/`answer`/`explain`/
  `on_correct`, `page` (fixed compositions).
- Rich text: italic, bold, code, internal and external links, `{name}`
  interpolation, escapes, optional smart typography.
- Assets: `image` (PNG, JPEG, GIF, WebP, static SVG), `video` (MP4, WebM)
  with poster, captions and transcript, `audio` (MP3, M4A/AAC, Ogg, WAV,
  FLAC), `font` (WOFF2, TTF, OTF); formats verified by content. Remote
  `video`/`audio` on declared https hosts.
- Bounded state (integers with ranges, booleans, closed sets of words),
  expressions with `+ - *`, comparisons and `and or not`, and actions
  (assign, `+=`, `-=`, `goto`, `reset`).
- Conditional chapters, sections and options (`when=`), entrance animations,
  jump transitions, components expanded at compile time, generated or
  designed covers, generated front matter (title page, colophon, contents).
- Not Turing-complete by design: no loops, calls, division, I/O or scripting.

**Compiler** — `crates/darm-lang`, `crates/darm-compiler`

- Hand-written lexer and recursive-descent parser with error recovery.
- Table-driven semantic analysis; diagnostics with stable codes (`E01xx`–
  `E05xx`, `W06xx`), source excerpts, "did you mean" suggestions and help
  lines; text and JSON output. Catalogue in [`docs/errors.md`](docs/errors.md).
- Canonical formatter.
- Project file `darm.toml`; multi-file projects through `include`; paths
  confined to the project directory.
- Deterministic output: the same sources and assets give the same IR and,
  for unencrypted books, the same bytes.

**IR 1.0** — `crates/darm-ir`, [`schemas/darm-ir.schema.json`](schemas/darm-ir.schema.json)

- Typed, serializable book model (JSON), independent of the source syntax.
- Structural validation shared by the compiler and the reader.
- Reference semantics for expressions and actions with integer saturation at
  ±(2^53 − 1), and conformance vectors run by the Rust and TypeScript
  implementations.
- Case- and diacritic-insensitive full-text search.

**Container format 1.0** — `crates/darm-package`, [`docs/file-format.md`](docs/file-format.md)

- Fixed 128-byte header, clear JSON prelude with an optional public card,
  deflated manifest, chunked data area with per-chunk SHA-256.
- Random access to any chunk; uncompressed entries of clear packages stored
  verbatim for zero-copy media.
- Optional encryption of the manifest and every chunk with AES-256-GCM; key
  slots for passwords (Argon2id) and key files (HKDF-SHA256); key commitment;
  `.darmkey` key files.
- Published limits checked before allocation; stable `E_PKG_…` error codes.

**Command line** — `crates/darm-cli` (`darm`)

- `init` (templates `essay`, `interactive`), `check`, `fmt`, `build`,
  `inspect`, `verify`, `extract`, `open`, `dev`, `keygen`, `reference`,
  `explain`, `schema`, `mcp`.
- `--format json` for `check`, `build` and `inspect`; stable exit codes 0–4.
- `open` / `dev`: a local server on 127.0.0.1 that hosts the reader and the
  book with HTTP range requests; `dev` rebuilds on change and reloads.
- `mcp`: a Model Context Protocol server on stdio with six tools.
- Passwords from an environment variable, a file, standard input or a
  prompt, never from arguments.

**Reader** — `apps/reader`, `crates/darm-wasm`, [`docs/reader.md`](docs/reader.md)

- WebAssembly core (the same Rust code as the CLI) for opening, verifying
  and decrypting packages and for validating the IR.
- Local library in IndexedDB: import by file picker or drag and drop, cover
  thumbnails, progress; full integrity check of clear books on import; unlock
  dialog for protected books (password or key file).
- Own layout engine: reflow and fixed layouts, single page or spread,
  justification with optimal line breaking, hyphenation (en, es, fr, de, pt,
  it), widow and orphan control, drop capitals, running heads, folios,
  procedural covers, light and dark palettes with enforced contrast.
- Page turning as a deformable WebGL2 mesh driven by springs, with dragging,
  riffled jumps and a Canvas 2D fallback — [`docs/animation.md`](docs/animation.md).
- Keyboard, pointer, touch and trackpad navigation; scrubber; contents;
  search that does not reveal content outside the reader's current path;
  bookmarks.
- Interactive content: buttons, choices, quizzes, conditional content with
  immediate re-layout, persisted state.
- Media: video with native controls and caption tracks, an audio player with
  transcripts, image and gallery lightbox, per-host consent for remote media.
- Accessibility: a text outline of each page for assistive technology,
  focusable controls over painted widgets, focus trapping, live
  announcements, reduced motion, 44 px targets, forced-colours and safe-area
  support.
- Reading position, variables, answers and bookmarks saved per book;
  settings for text size, line spacing, margins, theme, page mode and motion.
- PWA manifest and service worker for offline use; strict
  Content-Security-Policy in production builds.

**Desktop shell** — `apps/desktop` (Tauri 2), [`docs/platforms.md`](docs/platforms.md)

- A shell that hosts the reader build and passes it files through a token
  registry; the webview has three commands and no file-system permission.
- Status: built and launched on macOS (Apple Silicon), ad-hoc signed.
  Windows and Linux are configured and were never built. Android and iOS are
  configured and were never built.

**Examples and tests**

- Two complete books, a minimal book and nine single-feature snippets in
  `examples/`, with a lint script.
- Rust: unit tests in every crate, compiler integration tests, expression
  vectors, container golden, round-trip, tamper, hostile-input and
  mini-fuzz tests, and CLI tests that drive the built binary. Four
  `cargo-fuzz` targets.
- Reader: Vitest unit tests (layout, text, paint, flip, state) and a
  Playwright end-to-end suite for Chrome, WebKit, Firefox and a phone
  profile, with visual snapshots for Chrome and WebKit.
- A CI workflow definition (`.github/workflows/ci.yml`).

### Known limitations

These match the limitations stated in the other documents.

Reader ([`docs/reader.md`](docs/reader.md#12-accessibility),
[`docs/animation.md`](docs/animation.md#8-known-limits))

- Text on a page is painted on a canvas: it cannot be selected or copied, and
  browser find, translation and reader modes do not see it.
- No text-to-speech. Search results are not highlighted on the page.
- No right-to-left, bidirectional or vertical text layout.
- Hyphenation patterns for six languages only; the reader's own interface is
  English only.
- No zoom into a page; the text-size setting does not apply to fixed pages.
- The accessibility outline is rebuilt on every page turn. In Safari on
  macOS, buttons take focus with Option+Tab unless full keyboard access is
  enabled.
- Unlocking a password-protected book runs Argon2id on the main thread and
  blocks the interface while it runs.
- The library is local to one browser profile or app install. There is no
  sync, and the browser may evict stored books.
- The Canvas 2D fallback has no perspective and no corner skew. There is no
  page-turn sound.

Format and security ([`docs/security.md`](docs/security.md#9-known-gaps),
[`docs/file-format.md`](docs/file-format.md#8-integrity-model))

- No signatures: an unencrypted book detects corruption but cannot prove who
  made it.
- No independent security audit.
- Encryption does not stop someone who holds the key from copying what they
  can read.
- Reading state of protected books (position, variables, answers,
  bookmarks) is stored unencrypted on the device.
- A lost password or key file cannot be recovered.

Compatibility ([`docs/architecture.md`](docs/architecture.md#3-versioning))

- Unknown IR *fields* are ignored by readers, but an unknown IR *enum member*
  (for example a new block type) makes a 1.0 reader reject the book. Adding
  one is therefore not a backward-compatible change today.
- The TypeScript IR types in `apps/reader/src/ir.ts` are maintained by hand.

Platforms and process ([`docs/platforms.md`](docs/platforms.md))

- Only the macOS (Apple Silicon) desktop build has been produced and run.
- "Open with" is not implemented on Android.
- Offline use of the web app was verified on localhost only, never on a
  deployed https origin; PWA installation and file handlers were not
  verified.
- The CI workflow has never been executed.
- The fuzz targets were compiled but not run for long periods.

Language (by design, see [`docs/language.md`](docs/language.md#14-semantics-and-limits))

- No loops, division, string computation, timers, scripting or embedded HTML.
- Only audio and video may be remote; images, fonts and captions are always
  packaged.
