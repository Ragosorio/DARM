# DARM

DARM is an open ecosystem for **interactive books that still feel like books**:

* a small, Python-like **authoring language** (`.darml`) that people can write
  by hand and AI models can generate reliably;
* a real **compiler written in Rust** (lexer, parser, typed AST, semantic
  analysis, a versioned IR) with precise, repairable diagnostics;
* a documented, versioned **binary container** (`.darm`) with integrity
  checks, chunked media and optional authenticated encryption;
* a **reader** that opens those files as a physical-feeling book: a cover that
  opens, pages that bend and cast shadows as they turn, a back cover that
  closes — with contents, search, bookmarks, video, audio, branching stories
  and quizzes inside.

```darml
darm 1

book "A Small Atlas of Rain":
    author = "Mara Quill"
    language = "en"

chapter "Drizzle":
    text "Rain begins as a rumour. *Somebody* smells it first."
```

```sh
darm build book.darml      # → a-small-atlas-of-rain.darm
darm open a-small-atlas-of-rain.darm
```

## Status

Version 0.1.0. What exists, what was actually run, and what was not:

| Area | State |
|---|---|
| Language (grammar 1), compiler, formatter, IR 1.0 | Implemented; unit, integration and mutation tests pass |
| `.darm` container 1.0, encryption, key files | Implemented; round-trip, tamper, truncation and hostile-input tests pass |
| CLI (`init check fmt build inspect verify extract open dev keygen reference explain schema mcp`) | Implemented; end-to-end tests drive the real binary |
| Reader (web) | Implemented; end-to-end tests pass in Chrome, WebKit and Firefox on macOS, plus a phone-sized WebKit profile |
| Desktop app (Tauri 2) | Built and launched **on macOS (Apple Silicon) only**, ad-hoc signed. Windows and Linux are configured but were never built |
| Android / iOS | Configured on paper only; **not built** (no SDKs on the development machine) |
| Independent security audit | **None** |

Details and evidence: [`docs/testing.md`](docs/testing.md),
[`docs/platforms.md`](docs/platforms.md). Nothing in this repository is
described as working unless it was run.

## Requirements

* Rust 1.88 or newer (`rustup`), plus the `wasm32-unknown-unknown` target and
  `wasm-bindgen-cli` 0.2.130 for the reader's core
* Node.js 22 and npm for the reader
* Optional: the Tauri 2 prerequisites for the desktop app
  ([`docs/platforms.md`](docs/platforms.md))

## Build and run

```sh
# 1. The compiler / CLI
cargo build -p darm-cli                 # → target/debug/darm   (add --release for an optimised binary)

# 2. The reader's WebAssembly core, then the reader itself
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
scripts/build-wasm.sh                   # → apps/reader/src/wasm-pkg/
npm --prefix apps/reader install
npm --prefix apps/reader run build      # → apps/reader/dist/

# 3. Write, check, build and read a book
target/debug/darm init my-book --title "My Book" --author "Me"
target/debug/darm check my-book
target/debug/darm build my-book         # → my-book/my-book.darm
target/debug/darm open  my-book/my-book.darm     # serves the reader on 127.0.0.1 and opens a browser
target/debug/darm dev   my-book         # live preview: rebuilds and reloads on every save
```

`darm open` and `darm dev` find the reader build at `apps/reader/dist` when
run from a checkout; elsewhere pass `--reader DIR` or set `DARM_READER_DIR`.
The reader can also be used on its own: serve `apps/reader/dist` from any
static web server and add `.darm` files with the "Add a book" button or by
dropping them on the window.

### Protected books

```sh
BOOK_PASSWORD='a long passphrase' target/debug/darm build my-book --encrypt --password-env BOOK_PASSWORD
target/debug/darm build my-book --new-keyfile my-book.darmkey     # lock with a key file instead
target/debug/darm verify my-book/my-book.darm --password-env BOOK_PASSWORD
```

Encryption keeps the file unreadable without the key. It does not stop someone
who has the key from copying what they can see — read
[`docs/security.md`](docs/security.md) before relying on it.

### Tests

```sh
cargo test --workspace                       # compiler, IR, container, CLI end-to-end
npm --prefix apps/reader test                # layout, painter, flip engine, state (Vitest)
npm --prefix apps/reader run e2e             # the reader in Chrome, WebKit, Firefox (Playwright)
```

The first Playwright run needs `npx playwright install webkit firefox` (Chrome
is taken from the system). See [`docs/testing.md`](docs/testing.md).

## Examples

| Path | What it is |
|---|---|
| [`examples/minimal`](examples/minimal) | The smallest valid book |
| [`examples/snippets`](examples/snippets) | Nine one-feature books: canonical samples for people and for AI models |
| [`examples/atlas-de-mareas`](examples/atlas-de-mareas) | An editorial book in Spanish: ten chapters, twelve illustrations, designed cover, two-level contents |
| [`examples/the-lighthouse-protocol`](examples/the-lighthouse-protocol) | A branching story with state, choices, a quiz, fixed-layout plates, video and audio; the tests build it encrypted |

Every example is original work made for this project and is compiled by the
real compiler in the test suite.

## Repository layout

```
crates/darm-lang        lexer, parser, AST, formatter, diagnostics
crates/darm-compiler    semantic analysis, asset inspection, lowering to IR
crates/darm-ir          the IR: types, validation, expression semantics, search
crates/darm-package     the .darm container: reader, writer, cryptography
crates/darm-cli         the `darm` command
crates/darm-wasm        WebAssembly bindings used by the reader
apps/reader             the reader (TypeScript): layout, painter, WebGL page turn, UI
apps/desktop            the desktop shell (Tauri 2)
examples/               example books
schemas/                JSON Schema of the IR, machine-readable language reference
fuzz/                   cargo-fuzz targets
docs/                   documentation
```

## Documentation

| Document | Contents |
|---|---|
| [`docs/tutorial.md`](docs/tutorial.md) | Your first book, step by step |
| [`docs/language.md`](docs/language.md) | Language reference: grammar, elements, state, media, covers |
| [`docs/errors.md`](docs/errors.md) | Every diagnostic, with a failing example and its fix |
| [`docs/file-format.md`](docs/file-format.md) | The `.darm` container, byte by byte |
| [`docs/architecture.md`](docs/architecture.md) | Modules, data flow, decision records |
| [`docs/reader.md`](docs/reader.md) | How the reader works: layout, navigation, accessibility, persistence |
| [`docs/animation.md`](docs/animation.md) | The page turn: reference analysis, model, rendering, fallbacks |
| [`docs/security.md`](docs/security.md) | Threat model, cryptography, limits of protection |
| [`docs/platforms.md`](docs/platforms.md) | What is built and verified on each platform; signing and packaging |
| [`docs/testing.md`](docs/testing.md) | Test commands, what they cover, measured results |
| [`docs/contributing.md`](docs/contributing.md), [`AGENTS.md`](AGENTS.md) | Working on DARM, as a person or as an AI agent |
| [`CHANGELOG.md`](CHANGELOG.md) | Release notes |

For AI models: the binary documents itself. `darm reference --guide` prints
the complete language guide, `darm reference --json` the whole language surface,
`darm check --format json` returns diagnostics with codes, positions and
fixes, `darm explain CODE` explains any of them, and `darm mcp` exposes the
same tools over the Model Context Protocol.

## Licensing

Three different things, deliberately kept apart:

* **Licence of the code and the format.** Everything in this repository —
  compiler, container library, CLI, reader, desktop shell, specifications — is
  offered under the [Apache License 2.0](LICENSE). You may use, modify and
  redistribute it, including commercially and in closed products, provided you
  keep the notices; the licence includes an explicit patent grant from
  contributors and ends for anyone who sues over patents in the work. The
  `.darm` format and the `.darml` language are documented openly so that
  independent implementations are possible; no permission is needed to write
  one.
* **Copyright of books.** A book written in DARM belongs to its author. The
  tools place no claim on what they compile or display, and compiling a book
  does not put it under the Apache licence. The example books are original
  works made for this project ([`NOTICE`](NOTICE)); third-party fonts bundled
  with the reader keep their own licences (SIL OFL 1.1, listed in `NOTICE`).
* **The name.** "DARM" identifies this project. The Apache licence does not
  grant trademark rights (its section 6): a fork may say it is "based on DARM"
  or "compatible with `.darm`", but should not present itself as DARM. No
  trademark has been registered; whether and where to register one is a
  decision for the project's owner, not something this repository can settle.

Apache-2.0 is a proposal that fits the goal (an open core others can build on,
with patent protection); it is the owner's decision to confirm, and nothing
here is legal advice.

### Open core, optional services

The format and every tool needed to write, compile, protect and read a book
are open and work entirely on the user's device: no account, no server, no
network. Services such as cloud libraries, publishing, a marketplace, managed
licences, collaboration or analytics **do not exist**; if they are built they
are meant to plug in at seams that are already in the code (the library store,
the byte-source interface, key slots in the container, the CLI/MCP surface —
see [`docs/architecture.md`](docs/architecture.md)) and never to become a
requirement for reading or writing a book.
