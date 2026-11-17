# Contributing

How to build, test and change DARM. Read [`architecture.md`](architecture.md)
first for how the parts fit together; this document is the practical
companion. Evidence of what the test suites cover and what was measured is in
[`testing.md`](testing.md).

One rule above the others: **documentation and status tables describe only
what is implemented and was run.** If you add a claim, add the code or the
command that backs it.

Contents: [Repository layout](#repository-layout) ·
[Prerequisites](#prerequisites) · [Building](#building) · [Testing](#testing) ·
[Conventions](#conventions) · [Generated files](#generated-files) ·
[Adding a language element](#adding-a-language-element) ·
[Adding a diagnostic](#adding-a-diagnostic) · [Changing the IR](#changing-the-ir) ·
[Changing the container format](#changing-the-container-format) ·
[Visual snapshots](#visual-snapshots) · [Security reports](#security-reports) ·
[Licensing of contributions](#licensing-of-contributions)

---

## Repository layout

```
Cargo.toml                  workspace: the six crates below; excludes apps/desktop/src-tauri and fuzz
crates/
  darm-lang/                lexer, parser, AST, formatter, diagnostics
  darm-compiler/            semantic analysis, lowering to IR, assets, element tables, diagnostics catalogue
  darm-ir/                  IR types, validation, expression semantics, search; tests/vectors/expr.json
  darm-package/             .darm container: reader, writer, crypto; tests/fixtures/*.darm
  darm-wasm/                wasm-bindgen bindings for the reader; tests/smoke.mjs
  darm-cli/                 the `darm` binary; tests/cli.rs drives the real binary
apps/
  reader/                   the reader (TypeScript, Vite)
    src/                    core/ layout/ paint/ flip/ gl/ fallback/ ui/ shell/  (+ wasm-pkg/, generated)
    tests/                  Vitest unit tests and fixtures
    e2e/                    Playwright tests, helpers, snapshots/
    harness/                layout.html, flip.html: content-free playgrounds; shoot.mjs screenshots them
    public/                 PWA manifest, service worker, icons
  desktop/                  Tauri 2 shell (src-tauri/ is its own Cargo workspace)
fuzz/                       cargo-fuzz targets (own workspace)
schemas/                    darm-ir.schema.json, darm-language.json (generated)
examples/                   two books, a minimal book, nine snippets, tools/
scripts/                    build-wasm.sh, build-reader.sh, build-desktop.sh
docs/                       specifications and guides
.github/workflows/ci.yml    CI definition (see the note under Testing)
```

## Prerequisites

Versions are the ones pinned or declared in the repository.

| Tool | Version | Where it is declared | Needed for |
|---|---|---|---|
| Rust | 1.88 or newer (edition 2024) | `rust-version` in `Cargo.toml` | everything in `crates/` |
| Rust, for the desktop shell | 1.90 or newer | `apps/desktop/src-tauri/Cargo.toml` | `apps/desktop` |
| `wasm32-unknown-unknown` target | — | `scripts/build-wasm.sh` | the reader core |
| `wasm-bindgen-cli` | exactly 0.2.130 | pinned as `wasm-bindgen = "=0.2.130"` in `crates/darm-wasm/Cargo.toml`; the script refuses another version | the reader core |
| Node.js and npm | 22 | `NODE_VERSION` in `ci.yml` | the reader, the desktop tooling |
| TypeScript 7.0.2, Vite 8.3.8, Vitest 5.1.0, Playwright 1.65.1 | exact | `apps/reader/package.json` (installed by `npm ci`) | the reader |
| Google Chrome | system install | `channel: "chrome"` in `playwright.config.ts` | the `chromium` end-to-end project |
| `@tauri-apps/cli` 2.12.5 and the Tauri 2 system prerequisites | exact | `apps/desktop/package.json`, [`platforms.md`](platforms.md) | the desktop app |
| Rust nightly and `cargo-fuzz` | — | `ci.yml` | running the fuzz targets |
| Python 3 | standard library only | `examples/tools/lint_examples.py` | the examples lint |

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

## Building

All scripts can be run from any directory.

### The CLI

```sh
cargo build -p darm-cli            # → target/debug/darm
cargo build -p darm-cli --release  # → target/release/darm
```

`darm open` and `darm dev` look for a reader build in this order: `--reader
DIR`, `$DARM_READER_DIR`, a `reader/` directory next to the binary,
`../share/darm/reader`, and `apps/reader/dist` in the checkout. With
`--features embed-reader` the reader build is compiled into the binary
(`apps/reader/dist` must exist at compile time).

### The reader

```sh
scripts/build-wasm.sh              # crates/darm-wasm → apps/reader/src/wasm-pkg/ (profile wasm-release)
npm --prefix apps/reader ci        # exact dependencies from package-lock.json
npm --prefix apps/reader run build # tsc --noEmit && vite build → apps/reader/dist/
```

or all three in one go:

```sh
scripts/build-reader.sh [--skip-wasm] [--skip-install]
```

For development:

```sh
npm --prefix apps/reader run dev       # Vite dev server on port 5173
npm --prefix apps/reader run preview   # serves the production build on port 4173
```

The Content-Security-Policy and the service worker's `precache.json` are
added only by the production build (`apps/reader/vite.config.ts`).

### The desktop app

```sh
scripts/build-desktop.sh [--skip-reader] [--skip-wasm] [--skip-install] [-- <tauri build args>]
# output: apps/desktop/src-tauri/target/release/bundle/
```

Signing, notarisation and per-platform status: [`platforms.md`](platforms.md).
Only the macOS (Apple Silicon) build has ever been produced.

### Disk space

Rust target directories are large. `scripts/build-reader.sh` and
`scripts/build-desktop.sh` refuse to start with less than 2 GiB free
(`DARM_MIN_FREE_GIB` overrides) and turn debug info off; the workspace's dev
profile keeps line tables only.

## Testing

| What | Command | Notes |
|---|---|---|
| Rust workspace | `cargo test --workspace` | Unit tests in every crate; `crates/darm-compiler/tests/compile.rs`; `crates/darm-ir/tests/vectors.rs`; `crates/darm-package/tests/{golden,hostile,minifuzz,roundtrip,tamper}.rs`; `crates/darm-cli/tests/cli.rs` (runs the built binary) |
| Formatting | `cargo fmt --all --check` | |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | |
| Fuzz targets compile | `cargo check --manifest-path fuzz/Cargo.toml` | |
| WASM core | `node crates/darm-wasm/tests/smoke.mjs` | Builds the crate and the bindings into a temporary directory, then opens the package fixtures in Node. `--no-build` reuses an existing `.wasm`. |
| Reader types | `npm --prefix apps/reader run typecheck` | |
| Reader unit tests | `npm --prefix apps/reader test` | Vitest, Node environment, `apps/reader/tests/*.test.ts`: layout, text, paint, flip, state |
| Reader end to end | `npm --prefix apps/reader run e2e` | Playwright; see below |
| Desktop shell | `cargo test --release --features tauri/custom-protocol --manifest-path apps/desktop/src-tauri/Cargo.toml` | File registry and URL policy; no webview needed |
| Examples | `target/debug/darm fmt --check examples` and `python3 examples/tools/lint_examples.py --darm target/debug/darm` | |
| Fuzzing | `cargo +nightly fuzz run package_open fuzz/corpus/package_open crates/darm-package/tests/fixtures -- -max_total_time=60` | Targets: `package_open`, `package_probe`, `compile_source`, `ir_validate` |

**End-to-end tests.** `playwright.config.ts` defines four projects: `chromium`
(system Chrome), `webkit`, `firefox`, and `phone` (WebKit with an iPhone 14
profile, running the `phone`, `read-through` and `interactive` specs). The
first run needs the two bundled engines:

```sh
cd apps/reader
npx playwright install webkit firefox
npm run e2e                               # all projects
npm run e2e -- --project=chromium         # one engine
npm run e2e -- interactive.spec.ts        # one file
```

The suite builds its own inputs: `e2e/global-setup.ts` runs `cargo build -p
darm-cli` and compiles the fixture book and both example books from source
with `target/debug/darm` (clear, password-protected and key-file variants,
plus tampered and truncated copies) into `e2e/.tmp/`. Playwright then runs
`npm run build && npm run preview` unless a server is already listening on
port 4173.

**The shared expression vectors.** `crates/darm-ir/tests/vectors/expr.json`
is run by `crates/darm-ir/tests/vectors.rs` and by
`apps/reader/tests/state.test.ts`. A change to expression or action semantics
must add vectors there and pass both.

**Continuous integration.** `.github/workflows/ci.yml` defines jobs for the
Rust workspace on three operating systems, the WASM core, the reader
(typecheck, unit, build, end to end), the examples (including a protected
build that must verify with its password and not without), the desktop shell
on three operating systems, dependency audits and a 60-second fuzz smoke per
target. Per [`platforms.md`](platforms.md), **this workflow has never been
executed**; treat it as a description of intent until it has run.

## Conventions

These are the conventions visible in the code and enforced by the commands
above; there is no separate style guide.

**Rust**

* `cargo fmt` with default settings (there is no `rustfmt.toml`).
* `cargo clippy … -- -D warnings`: warnings are errors.
* `#![forbid(unsafe_code)]` in `darm-lang`, `darm-compiler`, `darm-ir`,
  `darm-package` and `darm-cli`.
* Dependencies that parse untrusted input or do cryptography are few, and
  their versions are fixed by the committed `Cargo.lock`; `darm-package`
  builds without default features for `wasm32-unknown-unknown`.
* Diagnostics for authors go through `darm_lang::Diagnostic` with a stable
  code and, wherever a repair is known, a `help`. Package errors carry a
  stable `E_PKG_…` code (`PackageError::code`).
* Limits are named constants (`darm-ir/src/validate.rs::limits`,
  `darm-package/src/limits.rs`) and are documented where users see them.
* Secrets never come from command-line arguments and are held in `Zeroizing`.

**TypeScript**

* `strict`, `noUncheckedIndexedAccess`, `noImplicitOverride`,
  `noFallthroughCasesInSwitch`, `verbatimModuleSyntax` (`tsconfig.json`).
  `npm run build` fails on a type error.
* No linter or formatter is configured for the reader. Match the surrounding
  code: two-space indentation, double quotes, semicolons, long lines allowed.
* `layout/`, `flip/` and `core/state.ts` stay free of DOM access so that they
  run in Vitest's Node environment.
* Book content reaches the DOM only through `textContent` (`ui/dom.ts`);
  never build HTML from book strings.
* Constants that define behaviour live at the top of their module with a
  comment (`MOTION` in `flip/controller.ts`, `LIMITS` in `core/book.ts`).

**DARM sources** (`examples/`, fixtures): formatted with `darm fmt`.

**Prose**: plain, specific, with file paths. No claims of support for
something that was not built and run.

## Generated files

Do not edit these by hand. Regenerate and commit them together with the
change that affects them.

| File | Regenerate with | Source of truth |
|---|---|---|
| `docs/errors.md` | `target/debug/darm reference --errors-md > docs/errors.md` | `crates/darm-compiler/src/explain.rs` |
| `schemas/darm-language.json` | `target/debug/darm reference --json > schemas/darm-language.json` | `crates/darm-compiler/src/spec.rs` (+ the diagnostics catalogue) |
| `schemas/darm-ir.schema.json` | `target/debug/darm schema > schemas/darm-ir.schema.json` | the types in `crates/darm-ir/src/lib.rs` |
| `apps/reader/src/wasm-pkg/` | `scripts/build-wasm.sh` | `crates/darm-wasm` (the directory is git-ignored) |
| `apps/reader/tests/fixtures/kitchen-sink.ir.json` and `kitchen-sink.darm` | `target/debug/darm build apps/reader/tests/fixtures/kitchen-sink -o apps/reader/tests/fixtures/kitchen-sink.darm --emit-ir apps/reader/tests/fixtures/kitchen-sink.ir.json` | `apps/reader/tests/fixtures/kitchen-sink/` |
| `apps/reader/e2e/snapshots/` | see [Visual snapshots](#visual-snapshots) | the reader |
| `crates/darm-package/tests/fixtures/*.darm` | see [Changing the container format](#changing-the-container-format) | `crates/darm-package/tests/golden.rs` |

No test or CI step compares the first three with freshly generated output, so
it is on you to regenerate them. A quick check before committing:

```sh
target/debug/darm reference --errors-md | diff - docs/errors.md
target/debug/darm reference --json      | diff - schemas/darm-language.json
target/debug/darm schema                | diff - schemas/darm-ir.schema.json
```

## Adding a language element

The grammar is uniform (element / assignment / `if` / component), so a new
element needs **no parser or formatter change**. It does touch every layer
after the parser. Using a new content block as the example:

1. **Tables** — `crates/darm-compiler/src/spec.rs`. Add an `ElementSpec` to
   `ELEMENTS`: name, summary, `place`, positional and named `Param`s, `body`,
   `children`, `common`. This alone makes the name known, validates its
   arguments, and puts it in `darm reference`.
2. **IR** — `crates/darm-ir/src/lib.rs`. Add or extend a `BlockKind` variant.
   Give new fields `#[serde(default)]` so older documents still parse.
3. **Validation** — `crates/darm-ir/src/validate.rs`. Check everything the
   reader will rely on: references to resources, anchors and variables;
   ranges; limits. If the block has searchable text, add it to
   `crates/darm-ir/src/search.rs`.
4. **Lowering** — `crates/darm-compiler/src/lower.rs`. Add an arm to the
   `match` on the element name in `Lowerer::blocks` that reads the arguments
   and produces the IR block. Emit diagnostics with codes from the catalogue.
5. **Reader types** — `apps/reader/src/ir.ts`. Mirror the IR change by hand.
6. **Layout** — `apps/reader/src/layout/blocks.ts`. Add a `case` to the
   `switch` in `addBlock` that turns the block into rows: draw items, a
   `SemanticNode` for the accessibility outline, and a `Region` if it is
   interactive. If it may appear inside a `page`, check `layout/fixed.ts`.
7. **Paint** — only if it needs a new `DrawItem` kind or `Widget`
   (`layout/types.ts`, then `paint/page.ts` or `paint/widgets.ts`).
8. **Interaction** — only if it has a new `Region` type: the control in
   `ui/overlays.ts` and the behaviour in `ReaderView.activate`
   (`ui/reader.ts`).
9. **WASM** — run `scripts/build-wasm.sh`; the reader validates with the
   compiled `darm-ir`, so a stale `wasm-pkg` rejects the new block.
10. **Versions and generated files** — see [Changing the IR](#changing-the-ir);
    regenerate the two schemas.
11. **Documentation** — the element table in [`language.md`](language.md)
    §5 and the relevant section; a snippet in `examples/snippets/` if it is a
    feature of its own (and its row in `examples/README.md`).
    `examples/tools/lint_examples.py` has its own small tables of element
    names (`RICH_ELEMENTS`, `PROSE_ELEMENTS`); update them if the element
    carries rich text.
12. **Tests** — a case in `crates/darm-compiler/tests/compile.rs` (and the
    block in `tests/fixtures/kitchen_sink.darml`); the reader fixture project
    `apps/reader/tests/fixtures/kitchen-sink/` plus its regenerated `.ir.json`;
    layout assertions in `apps/reader/tests/layout.test.ts`; an end-to-end
    case if it is interactive.

A new *argument* on an existing element is steps 1–6 in miniature. A new
value in a closed set (a text style, an animation kind) is listed in
[`animation.md` §7](animation.md#7-changing-or-adding-animations) for
animations; the same four places apply to the others: the constant table in
`spec.rs`, the enum in `darm-ir`, `ir.ts`, and the layout or painter.

## Adding a diagnostic

1. Pick a code in the right range (`E01xx` lexical, `E02xx` syntax, `E03xx`
   names/types/structure, `E04xx` assets, `E05xx` project, `W06xx` warnings).
   Prefer an existing code when the new message is another case of the same
   mistake. **Codes are stable**: never renumber or reuse one.
2. Emit it with a span and, if a repair is known, a help line (in
   `lower.rs`/`decl.rs`: `lw.err("E0312", span, "…").help = Some("…".into())`;
   warnings use `lw.warn`).
3. If the code is new, add an `Entry` to `CATALOGUE` in
   `crates/darm-compiler/src/explain.rs`: code, title, explanation, a minimal
   failing example and its fix. The test `every_emitted_code_is_documented`
   in `crates/darm-compiler/tests/compile.rs` fails for a code that is
   emitted but not in the catalogue.
4. Regenerate `docs/errors.md` and `schemas/darm-language.json` (the latter
   embeds the catalogue under `diagnostics`).
5. Add the failing input to the matching test in `compile.rs`
   (`structural_errors`, `argument_errors`, `asset_errors`, …) or, for lexer
   and parser errors, to the unit tests in `darm-lang`.

Write the message for the person who made the mistake: say what is wrong at
that position, and put the exact repair in `help`.

## Changing the IR

The IR is the contract between every compiler and every reader. Its version
is `ir_version = "MAJOR.MINOR"` (`IR_VERSION_MAJOR`, `IR_VERSION_MINOR`,
`IR_VERSION` in `crates/darm-ir/src/lib.rs`).

| Change | Compatible? | What to do |
|---|---|---|
| Add an optional field (`#[serde(default)]`, usually `skip_serializing_if`) | Yes. Old readers ignore unknown fields; new readers default a missing one. | Bump the minor version if readers should be able to tell. |
| Add an enum member (a block type, a text style, an action) | **Not today.** A reader built before the change rejects the whole book with `E_IR_JSON`, because the IR enums have no catch-all variant ([`architecture.md` §3](architecture.md#3-versioning)). | Either ship reader support first and accept that older readers refuse such books, or first add a catch-all variant to the enum in `darm-ir` (and decide how validation and layout treat it), release that, and add members afterwards. Bump the minor version. |
| Remove or rename a field, change a type or a meaning, tighten a limit | No. | New major version. Readers refuse other majors (`E_IR_VERSION`), so this strands every existing reader; avoid it. |

For any change: update `validate.rs`; update `apps/reader/src/ir.ts`; rebuild
the WASM core; regenerate `schemas/darm-ir.schema.json` and the reader
fixture `kitchen-sink.ir.json`; state the change in `CHANGELOG.md`.

Changing expression or action semantics changes two implementations
(`crates/darm-ir/src/expr.rs`, `apps/reader/src/core/state.ts`) and must add
vectors to `tests/vectors/expr.json`.

## Changing the container format

The rules are normative in
[`file-format.md` §12–13](file-format.md#12-versioning-and-compatibility).
In short: a minor version only adds things a 1.0 reader can ignore (optional
JSON members, new key-slot kinds); anything a 1.0 reader could not ignore (a
cipher, a compression method, a header layout, an entry role) is a new major
version or a reserved feature flag.

* `crates/darm-package/tests/fixtures/golden-v1.darm` is a committed clear
  package. `tests/golden.rs` checks that it still opens and that rebuilding
  the same inputs reproduces it byte for byte. Two encrypted fixtures guard
  key derivation and AEAD framing on the reading side.
* If the golden test fails, the writer's output changed. That must be a
  decision, not an accident. If it is one, bless new fixtures:

  ```sh
  cargo test -p darm-package --test golden -- --ignored bless_fixtures
  ```

  Never change expectations so that files written in the past stop opening.
* Update [`file-format.md`](file-format.md) in the same change; it is the
  specification, and the crate is the reference implementation of it.
* Add hostile cases to `tests/hostile.rs` / `tests/tamper.rs` for every new
  length, offset or tag, and bound it with a constant in `limits.rs`.
* The reader side: `crates/darm-wasm/src/package.rs` exposes the binding and
  its TypeScript types; `apps/reader/src/core/book.ts` consumes them.
  Rebuild with `scripts/build-wasm.sh` and run `tests/smoke.mjs`.

## Visual snapshots

`apps/reader/e2e/visual.spec.ts` compares eight deterministic states (covers,
spreads, dark mode, a page frozen in mid-turn, a fan of leaves) against
baselines in `apps/reader/e2e/snapshots/<project>/`. Baselines are kept for
the `chromium` and `webkit` desktop projects only; the tolerance is 2% of
pixels (`maxDiffPixelRatio: 0.02`).

After an intended visual change:

```sh
cd apps/reader
npm run e2e -- visual.spec.ts --update-snapshots
git diff --stat e2e/snapshots        # then look at the images before committing
```

Baselines depend on the browser build, the operating system's font
rasteriser and the GPU. The committed ones were made on macOS; expect
differences elsewhere and do not "fix" them by blessing baselines from
another platform over the existing ones.

For layout and typesetting work there is also a content-free harness:
`apps/reader/harness/layout.html` and `flip.html` under `npm run dev`, and
`node harness/shoot.mjs` (from `apps/reader`), which writes screenshots and a
text report to `harness/shots/` (git-ignored).

## Security reports

Do not open a public issue for a suspected vulnerability. The reporting
procedure and the threat model are in
[`security.md` §10](security.md#10-reporting-a-vulnerability). Changes to
`darm-package`, `darm-ir/src/validate.rs`, the desktop file registry or the
Content-Security-Policy deserve a second reviewer and a test that fails
without the change.

## Licensing of contributions

This section is a **proposal for the maintainers to confirm**, and it is not
legal advice.

* The repository is offered under the Apache License 2.0
  ([`../LICENSE`](../LICENSE), [`../NOTICE`](../NOTICE)).
* **Inbound = outbound.** Unless you state otherwise in writing, a
  contribution you submit for inclusion is licensed under Apache-2.0, as
  section 5 of that licence already provides. No separate contributor
  licence agreement is proposed.
* **Sign-off suggested.** Add a `Signed-off-by: Name <email>` line to each
  commit (`git commit -s`) to certify the
  [Developer Certificate of Origin 1.1](https://developercertificate.org/):
  that you wrote the change or have the right to submit it under the
  project's licence. Nothing in the repository enforces this today.
* Third-party material (fonts, code, images, text) needs a licence compatible
  with Apache-2.0 and an entry in `NOTICE`. The example books are original
  work; keep them that way.
* Books that people write with DARM are theirs. Nothing in a contribution
  should make the tools claim rights over what they compile or display.
