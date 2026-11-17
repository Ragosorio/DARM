# Architecture

DARM is a toolchain for interactive books. An author writes `.darml` source;
the `darm` compiler (Rust) checks it and lowers it to a typed intermediate
representation (the *IR*); the IR and the book's media are written into one
`.darm` container file, optionally encrypted; and a reader (TypeScript, with a
Rust core compiled to WebAssembly) opens that file, validates it, lays the
pages out itself and draws them on a canvas with a physical page turn. The
same reader build runs as a web app / PWA and inside a Tauri 2 desktop shell.
Everything runs on the user's device; there is no server component.

This document describes how the parts fit together. The parts themselves are
specified elsewhere and are only summarised here:

| Topic | Document |
|---|---|
| The source language | [`language.md`](language.md), [`errors.md`](errors.md), [`../schemas/darm-language.json`](../schemas/darm-language.json) |
| The IR | [`../schemas/darm-ir.schema.json`](../schemas/darm-ir.schema.json), `crates/darm-ir/src/lib.rs` |
| The container | [`file-format.md`](file-format.md) |
| The reader | [`reader.md`](reader.md), [`animation.md`](animation.md) |
| Platforms and the desktop shell | [`platforms.md`](platforms.md) |
| Threat model | [`security.md`](security.md) |
| Tests | [`testing.md`](testing.md) |

Contents: [1. Modules](#1-modules) · [2. Data flow](#2-data-flow) ·
[3. Versioning](#3-versioning) · [4. Technology choices](#4-technology-choices) ·
[5. Architecture decision records](#5-architecture-decision-records) ·
[6. Extension seams](#6-extension-seams-for-optional-services)

---

## 1. Modules

### Rust workspace (`Cargo.toml`, six crates)

```
                    ┌───────────────┐        ┌─────────────┐
          ┌────────▶│ darm-compiler │───────▶│  darm-lang  │
          │         │ analysis,     │        │ lexer,      │
          │         │ lowering,     │        │ parser, AST,│
          │         │ assets, spec  │        │ fmt, diag   │
          │         └───────┬───────┘        └─────────────┘
          │                 ▼
  ┌───────┴───────┐ ┌───────────────┐        ┌─────────────┐
  │   darm-cli    │ │    darm-ir    │        │  darm-wasm  │
  │ binary `darm` ├▶│ Book, validate│◀───────┤ DarmPackage,│
  │               │ │ expr, search  │        │ DarmBook    │
  └───────┬───────┘ └───────────────┘        └──────┬──────┘
          │         ┌───────────────┐               │
          └────────▶│ darm-package  │◀──────────────┘
                    │ container:    │   read-only build
                    │ read, write,  │   (default-features = false)
                    │ crypto        │
                    └───────────────┘
```

An arrow means "depends on" (from each crate's `Cargo.toml`). `darm-cli`
also depends on `darm-lang` directly, for the formatter and for rendering
diagnostics. Three facts are worth stating because they are easy to miss:

* `darm-lang` and `darm-ir` do not know about each other. The language front
  end never sees an IR type; the IR never sees a syntax tree. Only
  `darm-compiler` knows both.
* `darm-package` does not depend on `darm-ir`. To the container, the IR is an
  entry with the role `ir` and opaque bytes. `darm-cli` (when building) and
  `darm-wasm` (when reading) are the two places that put the halves together.
* `darm-wasm` uses `darm-package` with `default-features = false`: no writer,
  no random source, no `std::fs`. It builds for `wasm32-unknown-unknown`
  without `cfg` flags.

Outside the workspace (excluded in the root `Cargo.toml` so that `cargo test`
never builds a webview stack or a fuzzer):

* `apps/desktop/src-tauri` — the Tauri 2 shell. It has no dependency on the
  `darm-*` crates; it hosts the reader build and hands it files.
* `fuzz/` — four `cargo-fuzz` targets: `compile_source`, `ir_validate`,
  `package_open`, `package_probe`.

| Crate | Files | Responsibility |
|---|---|---|
| `darm-lang` | `lexer.rs`, `parser.rs`, `ast.rs`, `fmt.rs`, `diag.rs` | Text → tokens → `ast::File`; canonical formatter; `SourceMap` and `Diagnostic` (text and JSON rendering) |
| `darm-compiler` | `lib.rs` (driver), `decl.rs`, `lower.rs`, `spec.rs`, `richtext.rs`, `assets.rs`, `project.rs`, `explain.rs` | Includes, declarations, semantic analysis, lowering to IR, asset sniffing and hashing, `darm.toml`, the element tables, the diagnostics catalogue |
| `darm-ir` | `lib.rs`, `expr.rs`, `validate.rs`, `search.rs` | The `Book` model (serde), reference semantics of expressions and actions, structural validation, full-text search |
| `darm-package` | `header.rs`, `prelude.rs`, `manifest.rs`, `reader.rs`, `writer.rs`, `crypto.rs`, `keyfile.rs`, `limits.rs` | The `.darm` container: `probe`, `PackageHead`, `Package`, `PackageBuilder`, key slots, key files |
| `darm-wasm` | `package.rs`, `book.rs` | JavaScript bindings: `probe`, `DarmPackage`, `parseKeyfile`, `DarmBook` |
| `darm-cli` | `main.rs`, `pack.rs`, `inspect.rs`, `serve.rs`, `init.rs`, `mcp.rs`, `secrets.rs`, `workspace.rs` | The `darm` command, the local preview server, the MCP server |

### Reader (`apps/reader/src`)

```
main.ts ──▶ ui/app.ts ──────────▶ core/library.ts   IndexedDB (books, files, state)
   │           │    │             core/prefs.ts     localStorage
   │           │    └───────────▶ core/source.ts    ByteSource: Blob or HTTP range
   │           ▼
   │        ui/reader.ts ───────▶ core/book.ts ───▶ wasm-pkg/ (darm-wasm build)
   │         │  │  │  │           core/state.ts     expression evaluator
   │         │  │  │  └─────────▶ layout/  ───────▶ (pure data: BookLayout)
   │         │  │  └────────────▶ paint/   ───────▶ (2D canvas)
   │         │  └───────────────▶ ui/overlays.ts, ui/panels.ts, ui/color.ts
   │         ▼
   │        ui/stage.ts ────────▶ flip/controller.ts ──▶ flip/geometry.ts
   │                   ├────────▶ gl/renderer.ts        (WebGL2)
   │                   └────────▶ fallback/renderer2d.ts (Canvas 2D)
   └──────▶ shell/tauri.ts        (does nothing in a browser)
```

| Module | Role |
|---|---|
| `core/` | Opening packages (`OpenBook`), byte sources, the expression evaluator, the library store, preferences. No DOM beyond storage and `Image`. |
| `ir.ts` | Hand-written TypeScript mirror of the IR types. |
| `layout/` | `layoutBook`: IR + page size + settings + state → `BookLayout`. Pure; runs under Node in tests. `layout/types.ts` is the contract with the rest of the reader. |
| `paint/` | `paintPage`, `paintSurface`: a `PageLayout` or a cover → pixels on a 2D context. Fonts, hyphenation patterns, text measurement. |
| `flip/` | The book as a stack of sheets and the spring physics of turning them. Pure state and arithmetic. |
| `gl/`, `fallback/` | Two implementations of `SceneRenderer` (`ui/stage.ts`). |
| `ui/` | `App` (library, import, unlock), `ReaderView` (one open book), `Stage` (canvas, gestures, frame loop), `Overlays` (accessible DOM controls and media), `Panels` (contents, search, settings). |
| `shell/` | Bridge to the Tauri shell through `window.__TAURI__`. |

There is no UI framework and no state-management library; `ui/dom.ts` is a
~100-line element helper.

---

## 2. Data flow

```
 .darml ──▶ tokens ──▶ AST ──▶ (semantic analysis) ──▶ IR ──▶ .darm ──▶ reader
  text      lexer     parser    decl.rs + lower.rs     Book   package   OpenBook
```

### Compile side (native)

| Stage | Entry point | Output |
|---|---|---|
| Locate the project | `darm_cli::workspace::Project::locate` | root, entry file, `ProjectFile` (`darm.toml`) |
| Read files | `darm_compiler::Fs` (`DiskFs` on disk) | bytes; paths are project-relative and may not leave the root, symlinks included |
| Lex | `darm_lang::lexer::lex` | `Token`s with `Indent`/`Dedent`, comments on the side |
| Parse | `darm_lang::parse(file, src)` → `Parsed { file, errors }` | `ast::File`; recovers at statement boundaries; nesting bounded by `MAX_DEPTH = 48` |
| Includes | `load` / `flatten` in `darm-compiler/src/lib.rs` | one list of top-level statements; cycles and repeats are `E0501`; depth ≤ 16 |
| Declarations | `decl.rs` | `book`, `theme`, `assets`, `state`, `cover`, `back_cover`, components |
| Lowering | `lower.rs` (`Lowerer`), `richtext.rs`, `assets.rs` | IR blocks, rich-text spans, typed expressions, actions; arguments are checked against `spec.rs` |
| Deferred checks | end of `darm_compiler::compile` | jump targets, unused definitions, generated covers (`W0601`/`W0602`) |
| Validate | `darm_ir::validate(&Book)` | a failure here is reported as `E0599` (compiler bug) |
| Result | `darm_compiler::compile(fs, entry)` → `Compilation { sources, diagnostics, book, files }` | `book` is `None` when there are errors |
| Package | `darm_package::PackageBuilder` (`darm-cli/src/pack.rs`): `.card()`, `.book_summary()`, `.ir()`, `.add_file()`, `.encrypt()`, `.write_to_path()` | the `.darm` file; `build` then re-probes the file it just wrote |

If the parser reports any error, the pipeline stops before semantic analysis
(a tree with holes produces misleading follow-up errors). The compiler never
executes anything from the source: there is nothing to execute.

`darm_compiler::compile_str(source)` runs the same pipeline on one in-memory
file (`MemoryFs`); the MCP tool `darm_check_source`, tests and the fuzzer use
it.

### Read side (browser or shell webview)

| Stage | Entry point | Notes |
|---|---|---|
| Bytes | `ByteSource` (`core/source.ts`): `blobSource`, `urlSource` | random access: a `File` is sliced and a range-capable server is asked for pieces, so a large book is not read into memory as a whole (a server without range support falls back to one download) |
| Probe | `probeSource` → WASM `probe(head)` | reads at most the first 65 664 bytes; yields format, UUID, `encrypted`, public card, key-slot kinds, `headLen` |
| Integrity on import | `verifySource` | clear books only: every chunk is decoded (and so hashed) before the book is added to the library |
| Open | `OpenBook.open` → WASM `DarmPackage.open(head, password, key)` | header, prelude and manifest checks; key unwrap and key commitment for protected books |
| Decode the IR | `pkg.decodeChunk(entry, chunk, stored)` | each chunk is verified (SHA-256 or AES-GCM tag) inside the core |
| Validate the IR | WASM `DarmBook.parse(json)` → `Book::from_json` + `darm_ir::validate` | only after this succeeds does JavaScript call `JSON.parse` on the same text |
| Layout | `layoutBook(input)` (`layout/index.ts`) → `BookLayout` | re-run on resize, settings change and state change |
| Paint | `paintPage` / `paintSurface` (`paint/`) | into a scratch canvas, at the request of the stage |
| Present | `Stage` → `FlipController.pose()` → `GlRenderer.render` or `Canvas2dRenderer.render` | painted surfaces become textures keyed `page:N`, `front-cover`, … |
| Interact | `Overlays` (DOM), `ReaderView.runActions` → `applyActions` (`core/state.ts`) | state change → `relayout()` |
| Persist | `Library.saveState`, `Library.update` | IndexedDB; see [`reader.md`](reader.md#9-progress-and-persistence) |

### Trust boundaries

1. **Source and assets → compiler.** Sources may be hostile (a shared project,
   an AI-generated file). The compiler bounds nesting, sizes and counts,
   confines paths to the project directory, sniffs media by content, rejects
   active SVG (`E0405`) and never runs anything. See
   [`security.md`](security.md#61-the-compiler-sources-and-assets).
2. **`.darm` file → reader core.** A package is untrusted. Every length is
   checked against a published limit and the real file size before allocation
   ([`file-format.md` §11](file-format.md#11-limits)); all parsing, hashing and
   decryption happen in `darm-package` compiled to WebAssembly.
3. **IR → reader UI.** The IR is untrusted even when the container is intact
   (anyone can build a valid clear package). `DarmBook.parse` validates the
   version, the shape and every cross-reference with the same
   `darm_ir::validate` the compiler ran, *before* the TypeScript side reads a
   field. The layout engine is nevertheless written defensively (it checks
   types and uses own-property lookups) and state restored from storage is
   reconciled against the book's declarations (`reconcileState`).
4. **Resources → browser decoders.** Media bytes are only ever handed to the
   browser as blobs typed with the media type the IR declares; images are
   decoded through `<img>` (so an SVG cannot script or fetch). Book text
   reaches the DOM through `textContent`, never as HTML.
5. **Webview → operating system** (desktop shell). The webview has three
   commands and no file-system permission; see ADR 12.

The reader never sees DARM source. There is no `.darml` parser in the reader
and no way for a book to carry one.

---

## 3. Versioning

Five things carry a version. They change independently.

| What | Where it is written | Current | Policy | On mismatch |
|---|---|---|---|---|
| **Grammar** | First line of every `.darml` file: `darm 1`. `SUPPORTED_GRAMMAR` in `darm-lang/src/parser.rs`; `GRAMMAR_VERSION` in `darm-ir`. | 1 | An integer. A source file declares the grammar it was written for. | Compiler: `E0201` if the line is missing, `E0202` if the number is not 1. The IR records `grammar_version`; the reader does not act on it. |
| **IR** | `ir_version` in the IR JSON (`"MAJOR.MINOR"`). `IR_VERSION_MAJOR`, `IR_VERSION_MINOR` in `darm-ir/src/lib.rs`. | 1.0 | A minor version only adds. Unknown JSON fields are ignored (no `deny_unknown_fields`). | Reader: another major, or a malformed string, is `E_IR_VERSION` ("This book needs a newer reader"). Any `1.x` passes the version check. See the note below. |
| **Container** | `format_major`, `format_minor` in the fixed header. `FORMAT_MAJOR`, `FORMAT_MINOR` in `darm-package`. | 1.0 | [`file-format.md` §12](file-format.md#12-versioning-and-compatibility): unknown major → reject; newer minor → accept and ignore unknown members and slot kinds; reserved flag bit → reject. | `E_PKG_UNSUPPORTED_VERSION`, `E_PKG_UNSUPPORTED_FEATURE`. |
| **Key file** | `darmkey` member of a `.darmkey` file. | 1 | [`file-format.md` §10](file-format.md#10-key-files-darmkey). Independent of the container version. | `E_PKG_BAD_KEYFILE`. |
| **Tools** | `[workspace.package] version` in `Cargo.toml`; `apps/reader/package.json`; `apps/desktop`. Recorded in every book as `meta.generator` and in the prelude (`darm 0.1.0`). | 0.1.0 | Informational. | None: no reader behaviour depends on the generator string. |

Also versioned, locally: the reader's IndexedDB schema (`DB_VERSION = 1` in
`core/library.ts`) and the preferences key (`darm.prefs.v1`).

**IR minor versions: what is and is not tolerated.** A newer minor may add
optional *fields*; older readers ignore unknown fields (verified). It may not
add enum members: the IR enums (`BlockKind`, text styles, actions, …) are
plain serde enums without a catch-all variant, so a 1.0 reader given a block
of an unknown `type` fails in `Book::from_json` with `E_IR_JSON` and refuses
the book — the module documentation of `darm-ir` states exactly this rule, and
a new block type or enum value therefore needs a new major (or readers updated
first). (`layout/blocks.ts` contains a branch that draws nothing for an
unknown block type; validation in the WASM core runs first, so it is not
reachable.) See
[`contributing.md`](contributing.md#changing-the-ir).

---

## 4. Technology choices

**Rust for the compiler, the container and the reader core.** The container
parser and the IR validator face hostile input; `darm-lang`, `darm-compiler`,
`darm-ir`, `darm-package` and `darm-cli` are `#![forbid(unsafe_code)]`
(`darm-wasm` is not, because `wasm-bindgen` generates glue). The same crates compile natively (CLI) and to
WebAssembly (reader), so there is one implementation of the format and one of
IR validation instead of a Rust one and a JavaScript one that can drift. The
WebAssembly build uses its own Cargo profile (`wasm-release`: `opt-level =
"s"`, LTO, `panic = "abort"`).

**TypeScript and Vite for the reader, no UI framework.** The reader's surface
is one canvas, a few panels and dialogs. The code that matters (layout,
painting, the flip model) is pure functions over plain data that run under
Node in Vitest without a DOM. Runtime dependencies are four: three bundled
variable fonts (`@fontsource-variable/*`) and `hyphen` (hyphenation
patterns). TypeScript is `strict` with `noUncheckedIndexedAccess`.

**Canvas-painted pages and WebGL2.** A page has to bend while it turns, with
its real content on it. That requires the page to be a texture, which requires
the reader to paint it, which requires the reader to lay it out. See ADRs 8
and 9. Canvas 2D is the fallback renderer when WebGL2 is unavailable.

**Tauri 2 for the desktop shell.** The reader is already a web application;
the shell only has to host it and pass files in. Tauri uses the system
webview (small bundles) and has a deny-by-default capability model, which the
shell uses to grant the webview three commands and nothing else. Status per
platform is in [`platforms.md`](platforms.md); only macOS on Apple Silicon has
been built and run.

**IndexedDB for the library.** It stores `Blob`s (a book stays one file,
encrypted books stay encrypted at rest), is a standard browser API, and
needs no server. Preferences are small and device-specific and
live in `localStorage`.

**JSON for the IR, the prelude and the manifest.** Debuggable (`darm inspect
--ir`), schema-describable (`darm schema`), and parsed by a memory-safe parser
on both sides. The IR is stored deflated inside the container; in the tutorial
book 10.5 KiB of IR becomes 3.0 KiB.

---

## 5. Architecture decision records

Each record was checked against the code named in it.

### ADR 1 — Hand-written lexer and recursive-descent parser

**Context.** The language is indentation-sensitive, small (the EBNF in
[`language.md` §3](language.md#3-grammar-ebnf) is 30 lines), and its main
users include people and AI models who need exact, repairable error messages.
**Decision.** `darm-lang/src/lexer.rs` and `parser.rs` are written by hand;
`darm-lang` depends only on `serde`. The lexer emits `Indent`/`Dedent`, joins
lines inside brackets and after a trailing comma, and keeps comments for the
formatter. The parser recovers at statement boundaries and bounds recursion.
**Consequences.** Diagnostics such as "`False` is not a DARM value — booleans
are lower-case" are possible because the parser knows what it was doing. The
grammar document and the parser must be kept in step by hand.

### ADR 2 — A uniform grammar; meaning from tables

**Context.** A grammar with one production per element (`image`, `quiz`, …)
turns every typo into a generic syntax error and every new element into a
parser change.
**Decision.** Every statement is an element (`name args` with an optional `:`
block), an assignment, an `if`, or a component definition. Which names,
arguments and children are legal where is decided in semantic analysis from
the tables in `darm-compiler/src/spec.rs` (`ELEMENTS`, `COMMON`, `BOOK_PROPS`,
`THEME_PROPS`, `COVER_PROPS`, `BACK_COVER_PROPS`, `CONSTRUCTORS`).
`spec::reference_json()` exports the same tables as `darm reference --json`,
which is committed as `schemas/darm-language.json`.
**Consequences.** Tools and models see exactly what the compiler accepts. An
unknown argument gets a "did you mean" from the table. Adding an element does
not touch the parser, but it does touch `spec.rs` *and* `lower.rs`; the tables
describe shape, not lowering.

### ADR 3 — A separate, versioned, serializable IR

**Context.** The reader must not depend on source syntax, and whatever it
loads is untrusted.
**Decision.** `darm_ir::Book` is a serde model with its own version
(`ir_version`), serialized as JSON into the package. `darm_ir::validate` is
called by the compiler after lowering and, through `DarmBook.parse`, by the
reader before it uses the document. A JSON Schema is generated from the types
(`darm schema`) and committed.
**Consequences.** The language can change without changing readers, and the
reverse. Validation logic exists once. The TypeScript types in
`apps/reader/src/ir.ts` are a hand-maintained mirror and can drift from the
Rust types; nothing checks them automatically today.

### ADR 4 — The language is not Turing-complete

**Context.** A book format that can compute is a book format that can hang,
exfiltrate or surprise. Readers must be able to open any book safely.
**Decision.** No loops, no user functions, no division, no I/O. Components
are expanded at compile time (no recursion, depth ≤ 16). State is a fixed set
of variables (≤ 256): bounded integers, booleans and closed sets of strings.
Assignments clamp to the declared range. Integer arithmetic saturates at
±(2^53 − 1) (`INT_SATURATION` in `darm-ir/src/expr.rs` and
`apps/reader/src/core/state.ts`), the largest range in which Rust's `i64` and
JavaScript numbers agree. Both implementations run the vectors in
`crates/darm-ir/tests/vectors/expr.json`.
**Consequences.** Evaluation always terminates and state is finite by
construction. Anything that needs computation (scores as percentages, timers,
random events) cannot be expressed. Two evaluators exist and are kept equal
only by the shared vectors.

### ADR 5 — A purpose-built chunked container, not zip

**Context.** Books contain large media. The reader runs in a browser with
only `Blob.slice` or HTTP range requests, and needs per-part integrity and
optional encryption.
**Decision.** A fixed 128-byte header, a clear JSON prelude, a manifest and a
data area of entries cut into independently verified chunks
([`file-format.md`](file-format.md)). Chunks are sealed individually with an
AEAD bound to package, entry and position. In a clear package an uncompressed
entry is stored verbatim, so `OpenBook.blob()` returns a `Blob.slice` of the
file and video streams and seeks without copying.
**Consequences.** Random access, bounded memory and early truncation detection
come from the layout itself. Standard archive tools cannot open a `.darm`;
`darm inspect`, `darm verify` and `darm extract` exist for that. The format
is ours to maintain; a golden fixture guards it.

### ADR 6 — Standard cryptography only

**Context.** Protected books need confidentiality and integrity; home-made
constructions fail in ways their authors cannot see.
**Decision.** AES-256-GCM, Argon2id and HKDF-SHA256 from the RustCrypto crates
(`aes-gcm`, `argon2`, `hkdf`, `sha2`), with `zeroize` for secrets and the
operating system's random source in the writer. A random content key is
wrapped once per *key slot* (password or key file), so a book can have several
credentials. A public key commitment in the prelude ensures every slot leads
to the same content key. Reader and writer bound the Argon2id parameters.
**Consequences.** The primitives are standard; the *composition* is specified
in [`file-format.md` §9](file-format.md#9-encryption) and has not been
independently audited. Which crates have had an audit is stated in
[`security.md` §4](security.md#4-cryptography). Clear packages are not
authenticated and there is no signature scheme (a header flag bit is
reserved).

### ADR 7 — A sans-I/O package core compiled to WebAssembly

**Context.** Web, desktop and mobile shells all need to read `.darm`; each has
a different way of getting bytes.
**Decision.** `darm-package` exposes `probe`, `PackageHead::open`,
`chunk_location` and `decode_chunk`, which take and return byte slices.
`darm-wasm` binds them (`probe`, `DarmPackage.open`, `chunkOffset`,
`chunkLen`, `decodeChunk`). JavaScript owns the file and feeds ranges in
through `ByteSource`. Natively, the same code is driven through the `ReadAt`
trait.
**Consequences.** One implementation of parsing, hashing and decryption on
every platform; the desktop shell contains no format code at all. Password
key derivation (Argon2id, 64 MiB by default) runs inside WebAssembly on the
main thread, so unlocking blocks the UI for its duration; the reader refuses
parameters above 256 MiB or 8 passes (`LIMITS` in `core/book.ts`).

### ADR 8 — Pages are painted by the reader, not rendered as HTML

**Context.** The product requirement is a book whose pages bend when turned,
showing their real content. CSS can rotate a rigid plane; it cannot curl one.
**Decision.** `layoutBook` computes every page as a list of positioned draw
items; `paintPage` draws them on a 2D canvas; the result is a texture.
Interactive areas are reported as `regions`, and the reading-order outline of
each page as `semantics`. While pages are at rest, `Overlays` adds a visually
hidden outline for assistive technology, a transparent focusable `<button>`
over each region, and real `<video>`/`<audio>` elements over media widgets.
**Consequences.** Typesetting is under the reader's control (justification,
hyphenation, widows and orphans, baseline grid) and identical across engines
up to font rasterisation. The costs are real and listed in
[`reader.md`](reader.md#12-accessibility): text cannot be selected or copied,
browser find and translation do not see it, right-to-left scripts are not
handled, and accessibility is a parallel structure that must be kept honest.

### ADR 9 — WebGL2 mesh, geometry on the CPU, camera straight down

**Context.** The turn must show curvature, the reverse side, lighting and a
soft shadow at display refresh rate, and resting text must stay sharp.
**Decision.** `flip/geometry.ts` computes each airborne leaf as a mesh on the
CPU every frame from a small leaf model; `gl/renderer.ts` uploads it and
shades per pixel from the surface normal, with a blurred shadow map. The
perspective camera is perpendicular to the page plane, which maps that plane
by a pure scale: resting pages are drawn texel-for-pixel. Without WebGL2,
`fallback/renderer2d.ts` draws the same model as 44 vertical slices,
orthographically. Details: [`animation.md`](animation.md).
**Consequences.** One leaf model feeds both renderers and the unit tests. The
vertex shader stays trivial. The fallback has no perspective and no corner
skew. Page textures are memory: resolution is capped and least-recently-used
textures are evicted beyond a 220 MiB budget.

### ADR 10 — Springs, not fixed-duration tweens

**Context.** A reader grabs a page, lets go half-way, taps twice quickly,
changes direction. A tween has a start, an end and a duration; none of those
survive interruption.
**Decision.** Every leaf in the air is a critically damped spring on its turn
progress (`flip/controller.ts`). All constants are in one object, `MOTION`.
The stage steps the integrator with a time step clamped to 100 ms.
**Consequences.** Turns can be reversed, grabbed and released mid-flight
without a discontinuity, several leaves can be airborne, and motion is
frame-rate independent. There is no "duration" to configure; feel is tuned
through natural frequencies.

### ADR 11 — A local-only library

**Context.** A reading app normally means an account. This one must work with
no server.
**Decision.** `core/library.ts` keeps three IndexedDB stores: `books`
(metadata, progress, cover thumbnail), `files` (the `.darm` file verbatim) and
`state` (position, variables, answers, bookmarks, approved media hosts).
Passwords and keys are never stored: a protected book asks every time it is
opened. For protected books no cover thumbnail is rendered to storage and the
stored title comes only from the public card. `MemoryLibrary` replaces the
store when IndexedDB is unavailable and for `darm open` previews.
**Consequences.** Nothing leaves the device, and nothing follows the reader to
another device either. Browser storage can be evicted (the reader asks for
`navigator.storage.persist()`). Reading state of protected books is stored
unencrypted ([`security.md` §9](security.md#9-known-gaps)).

### ADR 12 — Desktop shell: token registry, no file-system permission

**Context.** A webview that can read paths can be turned into a file reader
by any bug in the page.
**Decision.** In `apps/desktop/src-tauri/src/registry.rs`, every file the
operating system or the native Open dialog hands to the app is registered
under a random 128-bit token with an already-open handle. The webview is
granted exactly `darm_take_pending`, `darm_read_chunk` and `darm_release`
plus event listening (`capabilities/main.json`). It learns `{ token, name,
size }` and never a path. `shell/tauri.ts` pulls the bytes in 4 MiB pieces
and gives the reader an ordinary `File`.
**Consequences.** From the reader's point of view the shell is one more way
to obtain a `File`; there is no shell-specific reading code. The registry has
its own tests with no Tauri dependency. Only regular files whose canonical
path ends in `.darm` can be registered.

### ADR 13 — Remote media is the only network access a book can cause

**Context.** Video is large; some publishers will want to host it. Any
network access from a book is a privacy and safety surface.
**Decision.** Only `video` and `audio` assets may be remote (`url=`); images,
fonts and captions are always packaged. The compiler requires `offline =
false`, `https`, and the host in `book: network` (`E0406`). The reader
re-checks (`OpenBook.remoteUrl`: https, no credentials in the URL, host on the
book's list) and asks the user once per host per book before the first
request. The Content-Security-Policy of production builds allows scripts
only from the app (plus WebAssembly), limits `connect-src` to the app's own
origin and Tauri's IPC origins, and allows cross-origin loads only as
`media-src https:`.
**Consequences.** A book cannot fetch, post or load a remote image. A reader
who approves a host reveals their IP address and playback times to it; the
consent text names the host. Books are offline by default.

### ADR 14 — Open core under Apache-2.0

**Context.** A document format is only useful if anyone can implement it and
nothing in it depends on one company's service.
**Decision.** Everything in this repository is offered under Apache-2.0 (the
`license` field of the Cargo and npm manifests; see [`../LICENSE`](../LICENSE) and
[`../NOTICE`](../NOTICE)). The format, the language and the IR are specified
in this repository. The tools need no account, no server and no network.
Optional services, if they are ever built, attach at the seams in section 6.
**Consequences.** Independent readers and writers are possible and welcome.
"Works fully offline with the open tools" is a constraint on every future
change, not a marketing line. The licence choice is a proposal for the
project's owner to confirm.

---

## 6. Extension seams for optional services

**None of the services named in this section exist.** There is no cloud sync,
no publishing service, no marketplace, no licence server, no collaboration
feature and no analytics, in this repository or anywhere else. Nothing in the
open tools requires an account or a server, and that is a design constraint:
a change that makes reading or writing a book depend on a service is a
regression.

What does exist is a small number of interfaces narrow enough that such a
service could attach without changing the format or forking the reader.

| Seam | Where | What exists today | What could attach |
|---|---|---|---|
| **Library store** | `class Library` in `apps/reader/src/core/library.ts`: `list`, `get`, `file`, `add`, `update`, `remove`, `state`, `saveState` | `Library` (IndexedDB) and `MemoryLibrary`, a subclass that overrides all eight methods. `App` and `ReaderView` use only these methods. | A store that mirrors `ReadingState` or files elsewhere (sync). The existence of `MemoryLibrary` shows the class is replaceable; note that it is a class with overridable methods, not a declared interface, and `App` constructs it directly. |
| **Byte source** | `interface ByteSource` in `core/source.ts`: `size`, `name`, `read(offset, length)`, optional `blob` | `blobSource` (File/Blob) and `urlSource` (same-origin HTTP range, used by `darm open`) | A source that streams a purchased or lent book. Today `urlSource` deliberately refuses anything but a same-origin relative URL. |
| **Key slots** | `slots` in the container prelude ([`file-format.md` §9.3](file-format.md#93-key-slots)); `SlotKind` in `darm-package/src/prelude.rs` | `password` and `keyfile` slots. A slot of unknown `kind` is skipped, not an error. | A new slot kind whose key-encryption key comes from a licence server or a device key. Old readers would skip it and still open the book through any slot they understand. |
| **Reserved signature flag** | Header flag bit 2 ([`file-format.md` §8.3](file-format.md#83-signatures-reserved-extension-point)) | Never set by the writer; a 1.0 reader rejects a file that has it. | Publisher signatures. |
| **Compiler file access** | `trait Fs` in `darm-compiler/src/project.rs`: `read`, `open`, `disk_path` | `DiskFs` and `MemoryFs` | Compiling from an editor buffer, a web service or a repository without a checkout. |
| **Tooling surface** | `darm` exit codes and `--format json`; `darm mcp` (six tools); `darm reference --json`; `darm schema` | All implemented | Editors, CI, authoring agents, a publishing pipeline. |
| **Native shell bridge** | `initNativeShell(open)` in `shell/tauri.ts`; `window.darm.openFile(bytes, name)` in `main.ts` | Used by the Tauri shell | Another host application handing books to the reader. |
| **Public card and book summary** | `PublicCard` in the prelude; `book` in the manifest | Title, authors, language, cover colour; readable without a key unless `--hide-card` | A catalogue that lists books without opening them. |

Things that are **not** seams today, so that nobody plans around them: there
is no plugin system in the reader; no event or telemetry hook (and the
`connect-src` policy would block one); no way for a book to call out; no
per-user or per-device licence concept in the container.
