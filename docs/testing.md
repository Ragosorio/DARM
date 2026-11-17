# Testing

What is tested, how to run it, and the results of the last full run. Every
number below was produced by running the command shown on the development
machine; nothing here is estimated.

**Environment of the recorded run** (2026-11-16): macOS 26.5.2 on Apple M4;
rustc 1.99.1; Node 22.23.4; Google Chrome 157 (system install, headless);
Playwright 1.65.1 with its WebKit and Firefox 159 builds.

## 1. Commands

```sh
# Rust: compiler, IR, container, CLI (unit + integration + end-to-end through the binary)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# WebAssembly core
scripts/build-wasm.sh
node crates/darm-wasm/tests/smoke.mjs

# Reader: types, unit tests, end-to-end
npm --prefix apps/reader run typecheck
npm --prefix apps/reader test
npx --prefix apps/reader playwright install webkit firefox     # once; Chrome comes from the system
npm --prefix apps/reader run e2e                               # all engines
npm --prefix apps/reader run e2e -- --project=chromium         # one engine
npm --prefix apps/reader run e2e -- --update-snapshots         # re-record visual baselines

# Generated files are up to date (each prints nothing when fresh)
target/debug/darm schema                | diff -q - schemas/darm-ir.schema.json
target/debug/darm reference --errors-md | diff -q - docs/errors.md
target/debug/darm reference --json      | diff -q - schemas/darm-language.json

# Desktop shell (its own Cargo project; needs apps/reader/dist to exist)
(cd apps/desktop/src-tauri && cargo test --release --features tauri/custom-protocol)
```

The end-to-end suite builds the production reader, **compiles its books from
`.darml` source with the real compiler** (`apps/reader/e2e/global-setup.ts`),
and drives the app in real browser engines. No test uses a pre-built book or a
mocked reader.

## 2. Results

### 2.0 After the reader redesign (2026-11-17)

The shell was redesigned, page turns were made faster and the timing of
entrance animations was reworked after the full run recorded below. What was
run again afterwards, on the same machine:

| Suite | Result |
|---|---|
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | 151 passed (doc-tests included), 0 failed |
| Reader type-check and unit tests (Vitest) | clean; 156 passed |
| Playwright, `chromium` and `firefox` projects together | 48 passed, 11 skipped by design, **1 failed**: the random walk of `turning-fast.spec.ts` timed out waiting for the book to settle while three workers shared a disk that was nearly full |
| The same random walk run alone in Chrome | passed (4.0 min) |
| Playwright, `webkit` and `phone` projects | **not run again.** The screen of the machine was locked, and in that state Playwright's WebKit delivers every click to `<html>` (checked on a page with a single button). The WebKit visual baselines in `e2e/snapshots/webkit` therefore still show the previous design and will fail until they are re-recorded with `--update-snapshots` |
| Desktop app | rebuilt, installed in `/Applications`, opened by `open FILE.darm`; one screenshot of the window confirmed the new shell with a book open. Turning pages in it was not exercised |

The Chrome visual baselines were re-recorded for the new design. Frame
pacing in that run: Chrome 16.7 / 16.7 / 16.8 / 16.8 ms (median / p95 / p99 /
max), no frame over 33 ms; Firefox 16.7 / 17.3 / 50 / 95.6 ms with 8 of 600
frames over 33 ms, measured while the disk was almost full and three workers
were running, so it is a worse figure than the one in section 4 and not
comparable with it.

`e2e/turning-fast.spec.ts` is new. It compares what is on screen at rest with
a fresh paint of the same position after bursts of key presses, interrupted
jumps, and a seeded random walk of choices and turns through the interactive
example. It is how the frozen-page defect described in section 7 was found.

### 2.1 The last full run (2026-11-16, before the redesign)

| Suite | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace` | **149 passed**, 0 failed, 1 ignored (the fixture re-blessing helper), across 14 test binaries |
| `crates/darm-wasm/tests/smoke.mjs` | passed (235 checks) |
| Reader type-check (`tsc --noEmit`, strict) | clean |
| Reader unit tests (Vitest) | **156 passed** in 5 files |
| Reader end-to-end (Playwright) | **74 passed**, 0 failed, 13 skipped by design, in 4 min 9 s |
| Generated files | fresh |
| Desktop shell unit tests | 27 passed (run when the shell was built; not repeated in the final pass) |

Breakdown of `cargo test --workspace`:

| Crate | Test binary | Tests |
|---|---|---|
| darm-lang | unit (lexer, parser, formatter, diagnostics) | 23 |
| darm-ir | unit (expressions, validation, search) | 7 |
| darm-ir | `tests/vectors.rs` (vectors shared with the reader) | 1 |
| darm-compiler | unit (rich text, assets, project) | 13 |
| darm-compiler | `tests/compile.rs` (source → IR or diagnostics) | 15 |
| darm-package | unit | 28 |
| darm-package | `golden`, `hostile`, `minifuzz`, `roundtrip`, `tamper` | 4 + 14 + 5 + 17 + 9 |
| darm-wasm | unit (native) | 3 |
| darm-cli | unit (range parsing, path confinement, MCP) | 4 |
| darm-cli | `tests/cli.rs` (the binary, end to end) | 6 |

End-to-end tests by browser project:

| Project | Engine | Passed | Skipped |
|---|---|---|---|
| `chromium` | Google Chrome 157, 1280×800 | 26 | 1 |
| `webkit` | Playwright WebKit (Safari's engine; also the macOS/iOS webview engine), 1280×800 | 24 | 3 |
| `firefox` | Firefox 159, 1280×800 | 18 | 9 |
| `phone` | WebKit with an iPhone 14 profile (390×664, touch, device scale 3) | 6 | 0 |

Skips are declared in the specs: the CLI server tests run in Chrome only, the
example-book, offline and visual-snapshot tests in Chrome and WebKit, the
phone spec only in the phone project.

## 3. What the tests establish

### Acceptance flows

| Requirement | Where it is tested |
|---|---|
| Create a project, edit the source, format, validate, compile, open and read it | `crates/darm-cli/tests/cli.rs::create_edit_format_check_build_inspect` (create → edit → `fmt` → `check` → `build` → `inspect` → `verify` → `extract`, byte-identical rebuild); `e2e/cli-open.spec.ts` (`darm open` and `darm dev` serve the result to a real browser, edits reload in place, build errors are shown) |
| Opens on the front cover, animated opening, turn pages both ways, end on the back cover | `e2e/read-through.spec.ts` (every sheet turned with the keyboard, in four projects) |
| Mouse, keyboard and touch | `e2e/gestures.spec.ts` (drag to turn, hold in mid-air, short drag falls back, drag back, flick, edge clicks, dragging the cover open, five leaves in the air, reversal in mid-air, Home/End riffles); `e2e/phone.spec.ts` (touch swipes both ways, hesitant swipe, taps, rotation) |
| Close and reopen at the same place | `e2e/read-through.spec.ts` (reload → shelf shows the cover and "% read" → book opens itself to the saved page; "Finished" after the back cover) |
| Real media with working controls | `e2e/media.spec.ts` (video metadata, poster, two caption tracks with cues, play, seek, stop on page turn; audio play/pause; image viewer; gallery) |
| Change a narrative choice and get the right route; reset; persist state | `e2e/interactive.spec.ts`; `e2e/examples.spec.ts` (two different routes through *The Lighthouse Protocol* to two different endings, with a restart in between) |
| Import a valid file, reject a corrupt one clearly, reject a missing asset at compile time | `e2e/protected.spec.ts` (altered, truncated, fake and bad-header files each refused with a reason and not added to the shelf); `cli.rs::source_errors_are_reported_with_positions_and_exit_code_1` (`E0401` with file, line, column and help) |
| Encrypted book: right password opens, wrong one is refused, tampering detected, no plaintext without the key | `cli.rs::protected_books`; `crates/darm-package/tests/{roundtrip,tamper,hostile}.rs`; `e2e/protected.spec.ts` (in the browser, including a hidden-title book opened with a key file, and a check that what the library stores is still encrypted) |
| Works with no network | `e2e/offline.spec.ts` (network cut, then import, read, play video, unlock an encrypted copy; and a service-worker test: load once, go offline, reload, reopen the book); every e2e test fails if any request leaves the machine |

### Other properties

* **Language front end.** Lexer/parser/formatter unit tests; formatting is
  idempotent and never changes the AST; 28 diagnostic codes are each triggered
  by a minimal source in `compile.rs` (the catalogue has more; the rest have
  an example in `darm explain` but no dedicated test); every code the compiler can emit is
  checked to exist in the catalogue; nesting bombs (5 000 parentheses, 200
  nested blocks) are rejected, not overflowed.
* **Mutation test of the compiler.** 1 500 random character edits of a
  full-featured source per run: no panic, and any successful compile yields
  IR that passes validation; formatting accepted source stays stable.
* **One semantics on both sides.** `crates/darm-ir/tests/vectors/expr.json`
  (26 expressions, 10 action sequences, including saturation at ±(2^53 − 1))
  is executed by the Rust evaluator and by the reader's TypeScript evaluator.
* **Container.** Round trips (clear, password, key file, multi-chunk,
  zero-length, compressed), byte flips in every region, truncation, chunk and
  entry swaps, lying sizes, decompression bombs, bad names, KDF bounds,
  determinism, a golden fixture for format compatibility, and five seeded
  mutation tests (`tests/minifuzz.rs`) that alter clear, key-file and password
  packages, including mutants whose header checks are re-sealed so that they
  reach the manifest parser. Their iteration counts are a few thousand per
  run; `DARM_MINIFUZZ_SCALE=N` multiplies them for a longer soak.
* **Page-turn engine** (`tests/flip.test.ts`). The leaf never stretches or
  sinks; the grabbed point tracks the pointer; a turn takes a consistent
  sub-second time and is frame-rate independent; leaves stay in stacking
  order; long jumps are bounded; 4 000 steps of random input never leave a
  valid state.
* **Layout and painter** (`tests/layout*.test.ts`, `tests/paint.test.ts`).
  Nothing is drawn outside the page; no overlapping lines; every block lands
  on exactly one starting page; anchors resolve; widows/orphans and heading
  keep-with-next hold; justification limits hold; hard line breaks; phone
  widths produce no word-fragment lines; state changes add and remove
  conditional chapters; tiny pages and huge type terminate.
* **Accessibility** (`e2e/a11y.spec.ts`). Reduced motion (system setting and
  in-app override) removes the flight but keeps every way of navigating;
  keyboard-only operation with a visible focus ring; a text outline of each
  page for assistive technology; named buttons; target sizes; focus trapped in
  panels; and the reader working with WebGL disabled (Canvas 2D renderer).
* **Visual snapshots** (`e2e/visual.spec.ts`, baselines in
  `apps/reader/e2e/snapshots/{chromium,webkit}`): front cover, title spread,
  figure spread, interactive spread, a page curled in the hand, a fan of three
  leaves, dark mode, back cover — eight images per engine, compared with a 2 %
  pixel tolerance. Poses in mid-turn are frozen through a test hook, so they
  are as reproducible as pages at rest. A further test checks that a page in
  mid-turn really differs from the page at rest and that its cast shadow is in
  the pixels.

## 4. Measurements

From `e2e/performance.spec.ts` in the recorded run: the *Atlas de mareas*
example (61 pages, two-page spread, 1280×800, device scale 1), ten single
turns, a burst of five, and a jump across the whole book and back. Headless
browsers on an Apple M4. **These are observations on one machine, not
guarantees for other hardware.**

| Engine | Frames sampled | Frame interval median / p95 / p99 / max (ms) | Frames over 33 ms | Layout of the whole book (ms) | Key press → first moving frame (ms) | Page textures (MiB) | Import → cover shown (ms) |
|---|---|---|---|---|---|---|---|
| Chrome 157 | 600 | 16.7 / 16.7 / 16.8 / 16.8 | 0 | 41.6 | 11.5 | 73.9 | 290 |
| Firefox 159 | 600 | 16.7 / 17.3 / 17.7 / 33.3 | 0 | 51 | 8 | 73.9 | 334 |
| WebKit | 600 | 17 / 27 / 29 / 44 | 1 | 34 | 2 | 73.9 | 219 |

* 16.7 ms is the display interval at 60 Hz: Chrome and Firefox held it for
  every sampled frame; WebKit held it for the median frame with some frames at
  two intervals.
* JavaScript heap after the run (Chrome only exposes it): 6.3 MiB.
* A jump across the whole book (riffle, then the back cover closing) took 2.9 s
  including the test's own polling.
* Unlocking the showcase book with full-strength key derivation (Argon2id,
  64 MiB, 3 passes, in WebAssembly) and showing its cover took 201–240 ms.

The suite asserts only loose bounds (median frame under 34 ms, layout under
400 ms, key-to-motion under 120 ms, textures under 240 MiB) so that it does
not flake on slower machines; the JSON for each engine is written to
`apps/reader/e2e/results/perf-<project>.json`.

## 5. Manual checks performed

These were done by hand during development and are not automated:

* The reference video was opened and analysed frame by frame
  ([`animation.md` §1](animation.md#1-the-reference-video)).
* Poses of the page turn (drag curl with corner lift, fan of leaves, cover
  opening, back cover, dark mode, single-page phone layout, Canvas 2D
  fallback) and every kind of page of the fixture and of both example books
  were inspected visually in a browser.
* The desktop app was built for macOS (Apple Silicon), launched, handed
  `.darm` files with `open -a`, and observed to open them in the real reader
  ([`platforms.md`](platforms.md)).

## 6. Not tested

Stated so nobody assumes otherwise:

* **Windows and Linux** builds of anything: only compile checks were ever done
  there, and none in the final pass. **Android and iOS**: nothing was built.
* **Real phones and tablets.** The phone project is desktop WebKit with a
  phone-sized viewport and synthetic touch events.
* **Safari itself**, as opposed to Playwright's WebKit build, and browsers
  older than the current releases.
* **The CI workflow** (`.github/workflows/ci.yml`) has never run. Its visual
  job would also need baselines recorded on its own platform: the committed
  ones were rendered on macOS.
* **cargo-fuzz targets** (`fuzz/`: `package_open`, `package_probe`,
  `compile_source`, `ir_validate`) compile on stable but were never run
  (they need a nightly toolchain). The in-suite mutation tests are the only
  fuzzing that has actually executed.
* **Screen readers.** The accessibility outline and control names are tested
  through the DOM; no session with VoiceOver, NVDA or TalkBack was done.
* **Very large books** (gigabytes of video). The format and the reader are
  designed for random access, but the largest package any test builds is a
  few megabytes.
* **Remote media** (`url=` assets): compiled and validated in tests, never
  played from a real host.
* **Long-term storage behaviour** of the browser library (eviction, quotas),
  the PWA install flow, and the operating system handing files to an installed
  PWA.
* **Embedded book fonts** are implemented but no test book uses one.
* Frame pacing on low-end hardware, battery use, and memory on devices with
  small GPUs.

## 7. Defects and flakiness found and fixed

For the record, because a green suite is only worth something if its failures
were understood:

* `darm open` intermittently never answered a font request (about one page
  load in five). The HTTP library's keep-alive handling was at fault; the
  server now uses a small standard-library implementation that answers one
  request per connection. 40 consecutive loads then succeeded.
* WebKit in ephemeral sessions cannot store Blobs in IndexedDB — the write
  neither fails nor finishes. The library now probes for this once and stores
  plain bytes there.
* Chrome sends an `Origin` header on module-script requests, so a cache entry
  stored with `Vary: Origin` did not match and offline reload failed; the
  service worker's lookups now ignore `Vary`.
* Caption tracks were attached in the order their data resolved, so the
  default track was not always first (seen in Firefox).
* After a rotation, the stage restored a stale page *index* instead of the
  reader's place in the text.
* Pages with entrance animations (`animate=`) were painted ahead of a turn at
  the start of their animation, and nothing repainted them when the reader
  arrived: text, captions or whole plates stayed missing. The animation clock
  now starts when a page comes into view, and a page under a leaf that is
  still in the air keeps its finished state.
* A hover `transform` on a button inside a `pointer-events: none` container
  was suspected of making it unclickable in WebKit and was removed; the cause
  of that failure turned out to be the locked screen described in section
  2.0, not the style.

