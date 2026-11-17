# AGENTS.md

Instructions for AI agents that **write DARM books** or **change this
repository**. DARM is a declarative language for interactive books
(`.darml`), a Rust compiler (`darm`), a container (`.darm`) and a reader.

**Rule zero: never describe something as existing unless you found it in the
code or ran it.** No invented elements, arguments, flags or features. When
unsure, ask the compiler (`darm reference`, `darm check`) or read the file.
In reports, say what you ran and what you did not: "implemented" means the
code exists and its tests passed for you; "configured" means files exist but
nothing was built or run. Keep `docs/platforms.md` and `docs/testing.md` true.

## Authoritative sources

| Need | Source |
|---|---|
| Elements, arguments, properties, constructors (machine-readable) | `darm reference --json` (same content: `schemas/darm-language.json`) |
| The language, with grammar and semantics | `darm reference --guide` (same text: `docs/language.md`) |
| Every diagnostic, with a failing example and its fix | `darm explain CODE`, `docs/errors.md` |
| The compiled book model (IR) | `darm schema`, `schemas/darm-ir.schema.json` |
| The container | `docs/file-format.md` |
| A guided example; structure; how to change things | `docs/tutorial.md`, `docs/architecture.md`, `docs/contributing.md` |

## Writing a book: the loop

The binary is `target/debug/darm` after `cargo build -p darm-cli`.

1. Write `book.darml`. Start from `darm init DIR` or an example:
   `examples/minimal/book.darml` (smallest valid book),
   `examples/snippets/01-…` to `09-….darml` (one feature each, ≤ 40 lines),
   `examples/atlas-de-mareas/` and `examples/the-lighthouse-protocol/`
   (complete books), `docs/tutorial.md` §20 (123 lines, most features).
2. `darm fmt DIR` — canonical format. A failure is a syntax error: go to 4.
3. `darm check DIR --format json` — done when the exit status is 0 and
   `"ok": true`.
4. Repair **by code**: each diagnostic has `code`, `file`, `line`, `column`,
   `message` and usually `help` (the exact repair). `darm explain CODE` shows
   a failing example and its fix. Fix syntax errors (`E01xx`, `E02xx`) first:
   the compiler stops before name and type checks while any exist.
5. Repeat 2–4. Then `darm build DIR --deny-warnings` and, to confirm the
   package, `darm verify FILE.darm`.

### Exit codes of `darm`

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | the sources have errors (or warnings with `--deny-warnings`; or `fmt --check` found differences) |
| 2 | invalid command line |
| 3 | environment problem: a file cannot be read or written, no reader build, bad `darm.toml` |
| 4 | the `.darm` package is invalid, damaged, locked, or the credentials are wrong |

### Hard rules models get wrong (with the code the compiler reports)

* First line of **every** `.darml` file is `darm 1` (`E0201`).
* Indent with **spaces**, four per level; never tabs (`E0102`). A block opens
  with `:` at the end of a line and its body starts on the next line
  (`E0204`, `E0208`). No braces, no semicolons (`E0101`).
* Strings use **double quotes** (`E0101`). Long text goes in a `"""` block;
  a blank line inside it starts a new paragraph.
* **Commas between arguments**; positional arguments first, then
  `name=value` (`E0203`, `E0207`): `text "Hi", style=lead, align=center`.
  Inside `book`, `theme`, `cover`, `back_cover`: `name = value`, one per line.
* **Colours are strings**: `"#7a2e22"`; a bare `#` starts a comment.
  Booleans are lower-case `true` / `false` (both `E0203`).
* `book` needs a title, `author` and `language`; a book needs at least one
  `chapter` (`E0308`, `E0313`). Content blocks live inside chapters (`E0302`).
* **Assets are declared in `assets:` and used by name**, never by path
  (`E0304`): `pic = image("art/pic.svg", alt="…")` then `image pic`. The
  `assets:` block may come before or after the chapters that use it. The
  file must exist (`E0401`). `alt` is **required** on `image(...)` and
  `video(...)` (`E0308`). Only `video`/`audio` may be remote (`url=`).
* **Ids are unique** across chapters, sections and blocks (`E0306`); ids
  starting with `_` are reserved (`E0317`). Every `goto`, `goto=` and
  `[label](target)` must name an existing id or one of `cover`, `start`,
  `contents`, `end` (`E0305`). External links: `https:`, `http:`, `mailto:`.
* **No loops, no division, no function calls, no string computation**
  (`E0301`, `E0101`). Arithmetic is `+ - *` on bounded integers.
* State is declared in `state:`. Text variables need a closed set:
  `route = "a" in ["a", "b"]`. Integers take a range: `n = 0 in 0..5`.
* **Actions** (`x = …`, `x += …`, `x -= …`, `goto t`, `reset`) are allowed
  **only** under `button`, `option` and `on_correct` (`E0302`). Conditions
  (`when=`, `if`) must be boolean (`E0311`): `when=n > 0`, not `when=n`.
* `{name}` in text must be a state variable or a component parameter; write
  `\{` for a literal brace (`E0314`). Markup is `*italic*`, `**bold**`,
  `` `code` ``, `[label](target)`. HTML and Markdown headings are not markup.
* A `quiz` has 2–12 `answer`s, at least one with `correct=true`. A `choice`
  has `option`s. A `gallery` takes 2–24 image assets.
* Inside `page:` **every child needs `at=(x, y)` and `size=(w, h)`** in
  percent (`E0316`). `page` must be a direct child of `chapter`. With
  `layout = fixed`, every top-level block of every chapter is a `page`.
* Components: `component name(a, b="default"):` at the top level; no
  recursion, no state; the name must not be a built-in element.
* `W0601`/`W0602` (generated covers) go away with `cover:` and `back_cover:`
  blocks. Warnings fail the build only with `--deny-warnings`.

### MCP server

`darm mcp` is a Model Context Protocol server on stdio (newline-delimited
JSON-RPC 2.0, `tools` only). Six tools: `darm_check {path}`,
`darm_check_source {source}`, `darm_format {source}`,
`darm_build {path, out?}` (unencrypted only), `darm_reference {}`,
`darm_explain {code}`. `darm_check_source` sees no files, so declared assets
report `E0401`; use `darm_check` for projects with media.

## Changing the code

Read `docs/architecture.md` (structure, trust boundaries, ADRs) and
`docs/contributing.md` (touch points for a new element, diagnostic, IR or
format change) before editing. Test commands:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix apps/reader run typecheck
npm --prefix apps/reader test                 # Vitest
npm --prefix apps/reader run e2e              # Playwright; needs Chrome, plus `npx playwright install webkit firefox`
target/debug/darm fmt --check examples
```

After changing `crates/darm-ir`, `darm-package` or `darm-wasm`, rebuild the
reader core with `scripts/build-wasm.sh` (`wasm-bindgen-cli` 0.2.130) first.

### Generated files: do not hand-edit

| File | Regenerate with |
|---|---|
| `docs/errors.md` | `target/debug/darm reference --errors-md > docs/errors.md` |
| `schemas/darm-language.json` | `target/debug/darm reference --json > schemas/darm-language.json` |
| `schemas/darm-ir.schema.json` | `target/debug/darm schema > schemas/darm-ir.schema.json` |
| `apps/reader/src/wasm-pkg/` | `scripts/build-wasm.sh` |
| `apps/reader/e2e/snapshots/`, `crates/darm-package/tests/fixtures/*.darm` | only for an intended visual or format change; see `docs/contributing.md` |

### Invariants to keep

* The reader never parses or evaluates DARM source. It receives IR that the
  WASM core has validated (`DarmBook.parse`) before any field is read.
* Everything from a package or from storage is untrusted: bound it, validate
  it, and put book text into the DOM only through `textContent`.
* A book cannot run code or reach the network, except remote audio/video on
  hosts it declares, over https, after the reader consents.
* Expression semantics exist twice (`crates/darm-ir/src/expr.rs`,
  `apps/reader/src/core/state.ts`); change both and add vectors to
  `crates/darm-ir/tests/vectors/expr.json`.
* Diagnostic codes, the container format 1.x and the IR 1.x are stable
  interfaces. Adding an IR enum member (for example a block type) is **not**
  backward compatible today; see `docs/contributing.md`.
* No account, server or network is required to write, compile, protect or
  read a book; do not add a dependency on one. No secrets on command lines
  or in `darm.toml`; the reader never stores passwords or keys.
