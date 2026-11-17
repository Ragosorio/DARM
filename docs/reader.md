# The reader

How the DARM reader behaves, for someone reading a book and for someone
changing the code. Source: `apps/reader/src`. The same build runs as a web app
/ PWA and inside the desktop shell; platform differences are in
[`platforms.md`](platforms.md). The page-turn animation has its own document,
[`animation.md`](animation.md), and is not repeated here. The overall
structure is in [`architecture.md`](architecture.md).

Every behaviour below was read from the source file named next to it.
Numbers are constants in that file.

Contents: [1. Opening a book](#1-opening-a-book) ·
[2. The book model](#2-the-book-model) · [3. Layout modes](#3-layout-modes) ·
[4. Layout and painting](#4-layout-and-painting) · [5. Navigation](#5-navigation) ·
[6. Contents, search and bookmarks](#6-contents-search-and-bookmarks) ·
[7. Interactive content](#7-interactive-content) · [8. Media](#8-media) ·
[9. Progress and persistence](#9-progress-and-persistence) ·
[10. Settings](#10-settings) · [11. Themes](#11-themes) ·
[12. Accessibility](#12-accessibility) · [13. Performance](#13-performance) ·
[14. Diagnostics and URL parameters](#14-diagnostics-and-url-parameters)

---

## 1. Opening a book

Code: `ui/app.ts` (`App`), `core/book.ts`, `ui/reader.ts` (`ReaderView.start`).

### The library

The reader starts on a shelf of imported books, most recently opened first.
Each card shows a cover thumbnail (or a plain cloth placeholder), title,
author, and "Not started", "N% read" or "Finished". A book gets onto the shelf
in one of these ways:

* the **Add a book** button (a file picker for `.darm`);
* dropping one or more files anywhere on the window;
* an installed PWA receiving a file from the operating system
  (`window.launchQueue`);
* the desktop shell handing over a file (`shell/tauri.ts` → `App.importFiles`).

Books are stored on the device in IndexedDB. If IndexedDB is unavailable (some
private windows) the reader falls back to an in-memory library that lasts for
the session.

### Import

For each file, `App.importFiles`:

1. **Probes** it (`probeSource`): reads at most the first 65 664 bytes and
   asks the WebAssembly core for the format version, UUID, whether it is
   protected, the public card and the key-slot kinds. A file whose size
   differs from the length its header declares is refused as incomplete; one
   without the DARM signature as "not a DARM book".
2. **Verifies clear books in full** (`verifySource`): every chunk of every
   entry is read and decoded, which checks its SHA-256. A damaged or altered
   clear book never reaches the shelf. Files over 32 MiB show a "Checking…"
   dialog meanwhile. Protected books are *not* verified at import, because
   that needs the key; they are authenticated chunk by chunk whenever they are
   decrypted.
3. **Stores** the file verbatim and a record (title and authors from the
   public card, or the file name when there is no card). Importing a book that
   is already in the library replaces the file and keeps its progress.

Importing a single file opens it straight away. Failures are listed in a
dialog with a plain-language title, the detail and the error code.

### Unlocking

A protected book shows an unlock dialog with the title and authors from the
public card (or "Protected book" when the card is hidden). It offers a
password field if the file has a password slot and a **Use a key file** button
if it has a key-file slot. A wrong password or key asks again with a message;
Cancel returns to the shelf.

* The password is normalised to NFC and passed to the core; it is not stored
  anywhere. A protected book asks every time it is opened.
* A key file must be at most 16 KiB.
* Before deriving a key from a password, the reader checks the Argon2id
  parameters of the file's first password slot against its own limits (256
  MiB, 8 passes; `LIMITS` in `core/book.ts`) and refuses with `E_READER_KDF`
  above them.
* Key derivation runs in WebAssembly on the main thread (there is no worker),
  so the interface does not respond until it finishes; an "Unlocking…"
  notice is put up first.

### Cover, opening, resume

After the IR has been validated (see
[`architecture.md`](architecture.md#trust-boundaries)), `ReaderView.start`
loads the fonts, the hyphenation patterns for the book's language and the
saved reading state, lays the book out and shows it **closed, front cover
up**.

* **First time**: a button under the cover says "Open the book". Clicking it,
  tapping the cover, or pressing Space, Enter or → opens the board and shows
  the first page.
* **Returning reader**: if the saved state says the reader was inside the
  book, the book opens itself 650 ms after it appears and riffles to the saved
  page. A book that was left on its back cover turns to the back cover.
  Opening by hand before the 650 ms have passed cancels the timer and goes to
  the same place.
* If the reader closes the book on its front cover, the page is not forgotten:
  the button then reads "Continue reading · page N".
* On the back cover the button reads "Read again from the beginning" and
  jumps to the first page after the front matter.

The saved position is a `PageAnchor` (chapter id, block id, line index), not a
page number, because page numbers change with window size and type size.

### Errors

`explain()` in `ui/app.ts` maps error codes to titles:

| Codes | Title shown |
|---|---|
| `E_PKG_BAD_MAGIC` | This is not a DARM book |
| `E_PKG_TRUNCATED` | This file is incomplete |
| `E_PKG_UNSUPPORTED_VERSION`, `E_IR_VERSION` | This book needs a newer reader |
| `E_PKG_INTEGRITY`, `E_PKG_CORRUPT…`, `E_PKG_HEADER` | This book is damaged or was modified |
| other `E_IR…` | This book's contents are not valid |
| `E_READER_KDF`, `E_READER_LIMIT`, `E_PKG_LIMIT` | This book exceeds what this reader allows |
| anything else | The book could not be opened |

---

## 2. The book model

Code: `flip/controller.ts` (`buildSheets`, `posForPage`, `pagesAt`).

The reader models a physical book: a stack of **sheets**. The first sheet is
the front board (front cover outside, endpaper inside), the last is the back
board, and the paper leaves are in between.

`pos` is the number of sheets lying on the left:

| `pos` | What is seen |
|---|---|
| 0 | closed, front cover up |
| 1 … S − 1 | open |
| S (the sheet count) | closed, back cover up |

* **Spread** (two pages): a leaf carries two pages, page 2k on its front and
  2k + 1 on its back. At `pos = 1` page 0 is alone on the right, facing the
  front endpaper; then pages (1, 2), (3, 4), … Even page indices are
  right-hand pages. A book with an odd page count ends with a blank back.
* **Single page**: every page is its own leaf. The back of a leaf shows the
  same page's ink faintly, mirrored, as thin paper does.

"Page N" in the interface is the page index plus one, counted from the first
generated page (the title page). The printed folio on a page is different: it
counts from the first page after the front matter.

`ReaderView.goTo(target)` resolves the reserved targets `cover` (pos 0), `end`
(pos S), `start` (first page after the front matter), `contents`, and any
anchor present in the current layout.

---

## 3. Layout modes

### One page or two

`Stage.measure` (`ui/stage.ts`) decides from the stage's size in CSS pixels:

* **Two pages** when the *Pages* setting is "Two", or when it is "Auto" and
  the stage is at least 600 px wide and at least 1.2 times as wide as tall.
  That covers desktop windows, tablets in landscape and phones in landscape.
* **One page** otherwise, or when the setting is "One".

### Reflow books (`layout = reflow`, the default)

Text flows into pages sized for the window.

* **Page proportion.** The page takes the proportion that best fills the
  window, clamped to bookish bounds (width ÷ height): 0.60–0.80 in a spread,
  0.60–0.82 for a single page, 0.46–0.86 for a single page on a phone (shorter
  side under 520 px). The book's `page_size` does not set the proportion of
  a reflow book.
* **Type size.** Body size is page width ÷ 34.5, clamped to 14.5–21 px, times
  the reader's text-size setting (`computeGeometry` in `layout/context.ts`).
* **Margins.** Inner 12.5% and outer 10.5% of the page width in a spread
  (the gutter visually eats part of the inner margin); 10% each side for a
  single page; scaled by the *Margins* setting; at least 14 px.
* **What adapts.** Page count, line breaks, where images fall, the running
  heads and folios. The reading position is kept across every re-layout.
* **Limits.** There is one column per page. Images scale to the column, never
  wrap text around them. A `page` block in a reflow book is a full-page plate
  between flowing text.

### Fixed books (`layout = fixed`)

Every page is an authored composition (`page` with children placed by
`at=`/`size=` in percent).

* The page keeps the book's `page_size` proportion on every device. The stage
  scales it to fit and leaves desk around it (letter-boxing). One page or two
  is chosen by the same rule as above.
* Type in a fixed page scales with the page, not with the reader's text-size
  setting (`layout/fixed.ts`). The settings panel says so: "This book uses
  fixed pages; text size applies only to flowing passages."
* Text that overflows its frame is shrunk in steps, down to 50%. Beyond that
  it overflows.
* **Limits.** On a phone in portrait a dense fixed page is small; there is no
  zoom or pan.

In both modes pages are painted at device resolution (see
[Performance](#13-performance)).

---

## 4. Layout and painting

Contract: [`apps/reader/src/layout/types.ts`](../apps/reader/src/layout/types.ts).

```
IR Book ── layoutBook(LayoutInput) ──▶ BookLayout
                                         pages[]: PageLayout
                                           items[]      positioned draw items (text, rect, line, image, ornament, widget)
                                           regions[]    interactive areas
                                           semantics[]  reading-order outline
                                           anchor       where the page starts (for resuming)
                                         anchors, blockPages, toc, startPage, contentsPage
PageLayout ── paintPage(ctx, page, size, PaintEnv) ──▶ pixels on a 2D canvas
```

`layoutBook` (`layout/index.ts`) is pure: its result depends only on its
input, the injected `Measurer` and the `hyphenate` function. It does not
touch the DOM, so it runs under Node in the unit tests. Blocks become *rows*
(`layout/blocks.ts`, `layout/text.ts`), rows become pages
(`layout/paginate.ts`). Chapters and blocks whose `when` condition is false in
the current state are simply not laid out.

Typesetting rules that are implemented:

* **Justification** (`layout/text.ts`). Justified paragraphs choose all their
  breaks together (a total-fit breaker in the manner of Knuth and Plass);
  ragged text uses a greedy breaker. Word spaces may shrink to 0.8 of their
  natural width and stretch within stated limits; a line that cannot be set
  within the hard limit is left ragged rather than opened into rivers.
  Letters are never spaced out.
* **Hyphenation** (`paint/hyphenate.ts`). Liang patterns from the `hyphen`
  package, loaded on demand for the book's language. Languages with patterns:
  **en, es, fr, de, pt, it** (matched on the primary subtag, so `es-MX` uses
  `es`). Any other language is not hyphenated. Words of at least 6 letters,
  at least 2 letters before and 3 after the break, never the last word of a
  paragraph, at most 2 consecutive hyphenated lines.
* **Baseline grid.** Body lines snap to a grid of one line height (half a
  line when the theme uses `paragraph = spaced`).
* **Widows and orphans** (`textRows` in `layout/blocks.ts`). A page break
  inside a paragraph leaves at least two lines on each side; paragraphs
  shorter than four lines are not split. The same mechanism (`keepWithNext`
  on rows) keeps headings with their text, figures with their captions and
  choices whole.
* **Drop capitals.** Three lines deep, for paragraphs with the style
  `dropcap`. (When the theme sets `drop_caps`, it is the compiler that gives
  that style to the first paragraph of each chapter.)
* **Running heads** (`furniture` in `layout/paginate.ts`). Letterspaced
  capitals: the book title on left-hand pages of a spread, the chapter title
  on right-hand pages and on single pages. Omitted on a chapter-opener page.
* **Folios.** In the outer corner on a spread, centred on a single page,
  numbered from the first page after the front matter.
* **Generated front matter** (`layout/frontmatter.ts`). Title page, colophon
  and contents are laid out by the reader from the IR's metadata and table of
  contents; the contents page shows live page numbers and only the entries
  that exist on the reader's current path. The few words the reader prints
  itself ("Contents", "Published", …) exist in en, es, fr, de, pt and it;
  other languages fall back to English.
* **Covers** (`paint/cover.ts`). Procedural and deterministic: cloth grain
  seeded from the book id, foil rules, debossed lettering, four styles.
  Covers keep their own colours with Night pages.
* **Fonts** (`paint/fonts.ts`). Three bundled variable faces: Literata
  (serif), Inter (sans), JetBrains Mono. A book may embed its own body or
  heading face as a `font` asset.

The painter fills the whole page, draws items in order, and reports which
images were not decoded yet so the stage can repaint when they arrive.

---

## 5. Navigation

### Keyboard

From `ReaderView.onKey` (`ui/reader.ts`). Keys are ignored while Alt or Meta
is held, while typing in a field, and while the lightbox or the external-link
dialog is open.

| Key | Action |
|---|---|
| `→`, `PageDown` | Next page (opens the book from the front cover) |
| `←`, `PageUp` | Previous page |
| `Space` | Next page; with `Shift`, previous page. Not when a button, link or video has focus. |
| `Enter` | Opens the book, only while it is closed on the front cover. Not when a control has focus. |
| `Home` | First page after the front matter |
| `Ctrl+Home` | Front cover |
| `End` | Back cover |
| `Escape` | Closes the open panel; otherwise shows the toolbar |
| `/` | Open search |
| `Ctrl+F` | Open search |
| `t` | Toggle contents |
| `b` | Toggle a bookmark on the current page |
| `,` | Toggle reading settings |

`PageUp`/`PageDown` also work while the position slider has focus. Inside the
lightbox, `←`/`→` change image and `Escape` closes it. `Tab` moves through the
toolbar and the controls on the page.

### Pointer and touch

From `Stage.bindInput` (`ui/stage.ts`) and `ReaderView.onTap`.

* **Drag** a page sideways (more than 7 px, mostly horizontal) to pick it up;
  it follows the pointer and settles forward or back on release. With one
  page on screen, a rightward swipe anywhere pulls the previous leaf back in.
* **Tap zones** on an open book:

  | Mode | Next | Previous | Elsewhere |
  |---|---|---|---|
  | Two pages | right page, right of 55% of its width | left page, left of 45% of its width | toggle the toolbar |
  | One page | right 34% of the page | left 28% of the page | toggle the toolbar |

  On the front cover a tap opens the book; on the back cover a tap turns back.
  While a panel is open, a tap on the stage closes the panel instead.
* **Trackpad**: a two-finger horizontal swipe turns one page (at most one
  turn per 420 ms).
* **Mouse**: moving the pointer within 72 px of the top or bottom edge shows
  the toolbar. It hides again after 3.8 s unless a control in it has focus or
  a panel is open; it is always visible while the book is closed.
* Vertical scrolling and pinch-zoom gestures are left to the browser
  (`touch-action: pan-y pinch-zoom`).

With reduced motion, dragging is disabled; a swipe of more than 40 px still
turns the page, without the flight.

### Scrubber

The progress line at the bottom spans every page. Dragging it previews "Page N
of M · Chapter"; releasing it jumps there (riffling the leaves in between).

### Jumps

Links, buttons with `goto`, contents lines and search results all go through
`ReaderView.goTo` / `goToPage`. The transition is the book's choice per jump:
`flip` (default, a riffle of up to 6 leaves), `fade` or `none`. A jump to a
target that is not part of the book in the current state shows "That part of
the book is not available on this path." and goes nowhere.

---

## 6. Contents, search and bookmarks

Code: `ui/panels.ts`.

**Contents.** A side panel listing Cover, every visible table-of-contents
entry with its page, and Back cover; the current entry is marked
(`aria-current="location"`). Entries whose chapter or section is hidden by a
condition are absent. Bookmarks are listed below.

**Search.** Case- and diacritic-insensitive full-text search runs in the
WebAssembly core (`darm_ir::search`) over the whole IR. The panel waits for
two characters, shows up to 150 results with the match marked in a snippet,
and jumps to the page of the block.

What search does **not** reveal: a hit in a block that is not laid out in the
reader's current state (a chapter behind `when=`, the untaken branch of an
`if`) is dropped before display, so searching cannot be used to read ahead in
a branching story. What it does not do: it does not highlight the match on
the page.

**Bookmarks.** `b` or the ribbon button marks the current page (either
visible page in a spread counts). A bookmark stores a `PageAnchor` and a
label made of the chapter title and the first words on the page, so it
survives re-layout. At most 500 are restored from storage.

---

## 7. Interactive content

Code: `ui/reader.ts` (`activate`, `runActions`), `core/state.ts`,
`ui/overlays.ts`. Language side: [`language.md` §8–9](language.md#8-state-expressions-and-actions).

Buttons, choice options and quiz answers are painted on the page (so they
bend with it) and are also real, focusable `<button>` elements laid over the
page while it is at rest.

* **Button**: runs its actions.
* **Choice**: picking an option records it and runs its actions. The reader
  may pick a different option later; its actions run each time.
* **Quiz**: the **first answer stands**. It is recorded with whether it was
  correct; `on_correct` actions run if it was. The page then shows the
  correct answer and the explanation. A live region announces "Correct." or
  "Not quite."
* **Actions** are applied by `applyActions`, in order, never throwing: an
  assignment that cannot be computed is skipped; integers clamp to their
  declared range; the last `goto` wins and is performed after the others.
* **Re-layout on state change.** If variables or records changed, the book is
  laid out again at once: `if` blocks, `when=` chapters, the contents and
  `{name}` interpolations all update, and the reader stays at the same
  anchor.
* **`reset`** restores every variable to its initial value and clears all
  quiz and choice records; `reset name` restores one variable.
* **Persistence.** Variables and records are saved with the reading position
  and restored on the next open, after being reconciled against the book's
  declarations (`reconcileState`: unknown names dropped, values coerced into
  range, anything else reset to the initial value).
* **Never traps the reader.** Interactive blocks do not gate page turning.
  Arrow keys, taps, the scrubber, the contents and every toolbar control work
  on every page regardless of state. The only content a reader cannot reach
  is content the book's own conditions exclude in the current state.

**External links** (`https:`, `http:`, `mailto:`) never open by themselves:
a dialog shows the real host and path and asks. Confirmed links open with
`noopener,noreferrer`.

---

## 8. Media

Code: `ui/overlays.ts`. Media elements exist only while their page is at
rest; when a page starts to turn they are paused and removed.

* **Images** are painted on the page. Activating one opens a **lightbox**
  with its alt text and caption. A **gallery** opens the same lightbox with
  previous/next buttons, `←`/`→`, swipe, and an "n / N" counter. Focus is
  trapped in the lightbox; `Escape` or a click on the backdrop closes it.
* **Video** is a real `<video controls playsinline preload="metadata">` over
  its widget, with the browser's native controls. The poster and every
  caption track are attached (`<track kind="captions">`, the first one
  default). `autoplay` implies muted. If the device cannot play the file the
  widget says so.
* **Audio** has a custom player: a play/pause button, a seek slider, the
  time, and, when the asset has one, a collapsible transcript. The underlying
  `<audio>` element stays in the document.
* **Packaged media** in a clear book plays from a zero-copy slice of the
  file, so video seeks without loading the whole entry. In a protected book,
  or for a compressed entry, the entry is decoded chunk by chunk into memory
  first.
* **Remote media.** If a book declares a remote `video` or `audio`, nothing is
  requested until the reader agrees. The widget shows "This media is not
  inside the book. Playing it contacts HOST." and a button "Allow HOST for
  this book". Consent is per host and per book and is saved with the reading
  state. The URL must be `https`, carry no credentials, and name a host on
  the book's `network` list, or the media is reported as unavailable. Remote
  images, fonts and captions do not exist in the format.

Which codecs play depends on the browser or webview; the reader adds none.

---

## 9. Progress and persistence

Code: `core/library.ts`, `ReaderView.save`, `core/prefs.ts`.

### What is stored, and where

| Store | Key | Content |
|---|---|---|
| IndexedDB `darm-library` / `files` | package UUID | The `.darm` file exactly as imported. A protected book stays encrypted. |
| IndexedDB `darm-library` / `books` | package UUID | `BookRecord`: title, authors, language, cover colour, `encrypted`, `hasCard`, file name, size, added/opened times, `progress` (0–1), `finished`, and a cover thumbnail (PNG, 360 px wide) |
| IndexedDB `darm-library` / `state` | package UUID | `ReadingState`: `at` (front / page / back), `anchor`, `vars`, `ui` (quiz and choice records), `bookmarks`, `allowedHosts`, `updatedAt` |
| `localStorage` `darm.prefs.v1` | — | Reader preferences (section 10) |

### When

`ReaderView.save` runs 350 ms after the book comes to rest on a new position
or after any action, bookmark or consent change; when the tab is hidden; and
when leaving to the library. Progress is the page index over the last page
index, or 1 on the back cover; `finished` is set on reaching the back cover
and never cleared.

Removing a book from the library deletes its file, record and state. The
original file the user imported from is not touched.

### What is never stored

* Passwords and key-file keys.
* Decrypted content of a protected book. Its decoded resources live in memory
  and as blob URLs while the book is open and are released on close.
* A cover thumbnail of a protected book. The shelf shows a placeholder with a
  lock; its title and authors are whatever the public card says, or
  "Protected book".
* Anything about a book opened through `?book=` (`darm open`, `darm dev`):
  those use a throwaway in-memory library.

What *is* stored for a protected book, unencrypted: its reading state
(position, variable values, answers, bookmarks). See
[`security.md` §9](security.md#9-known-gaps).

Nothing is sent anywhere. Stored data is treated as untrusted when read back
(`Library.state` and `reconcileState` sanitise it).

Limits: browser storage can be evicted by the browser; the reader requests
persistent storage after an import, which the browser may refuse. In WebKit
private sessions, where Blobs cannot be stored in IndexedDB, files up to 512
MiB are kept as bytes instead.

---

## 10. Settings

Code: `core/prefs.ts` (`Prefs`, `DEFAULT_PREFS`), `ui/panels.ts`.

| Setting | Values | Default | Effect |
|---|---|---|---|
| Text size | 80%–160% in steps of 10% | 100% | Re-layout. Not applied inside fixed pages. |
| Line spacing | Compact, Normal, Relaxed (1.38, 1.5, 1.68) | Normal | Re-layout |
| Margins | Narrow, Normal, Wide | Normal | Re-layout |
| Pages colour | Paper, Night | Paper | Paper uses the book's own palette; Night its dark one. The system's dark mode does not change the pages: a book is paper unless the reader asks otherwise |
| Pages | Auto, One, Two | Auto | See section 3 |
| Motion | System, Full, Reduced | System | System follows `prefers-reduced-motion` |

Preferences are per device, not per book. `Prefs` also has `justify` and
`hyphenate` overrides (`null` = follow the book's theme); the layout honours
them, but the settings panel has no control for them, so in practice the
book's theme decides.

---

## 11. Themes

Code: `ui/color.ts` (`resolvePalette`, `sceneStyle`).

A book's `theme` gives paper, ink and accent colours and, optionally, a dark
set. The reader resolves a palette for light or dark reading:

* **Light**: the book's `paper`, `ink`, `accent`.
* **Dark**: the book's dark set if it has one; otherwise paper `#191715`, ink
  `#e8e1d3`, and the accent mixed 45% towards white.
* **Forced contrast**, in both modes: ink is pushed towards black or white
  until ink on paper reaches at least **7:1**; the accent until it reaches at
  least **3:1**. A book cannot make itself unreadable through its theme.
  (A per-block `color=` override is the author's responsibility.)
* Derived colours: `muted` (captions, folios, running heads), `rule`
  (hairlines), `tint` (box backgrounds).

The shell around the book does not change with the setting: it is always
dark (`#090909`, with `#b890d9` as its one accent), so that the book is the
only lit thing on screen. The desk under the book is the same near-black
with a pool of light around the book. Controls float over it in translucent
capsules and fade out while reading; moving the mouse, pressing Escape or
tapping the middle of a page brings them back. Controls that sit *on* a
page (the audio player, hover states of choices) take the page's colours
instead. Covers do not change with the Night setting.

---

## 12. Accessibility

What is implemented:

* **A text outline of every page at rest** (`Overlays.show`). The canvas is
  `aria-hidden`. For each visible page the reader adds a visually hidden
  `<article aria-label="Page N">` built from the layout's `semantics`:
  headings, paragraphs, block quotes, lists, images as `role="img"` with
  their alt text, separators. A screen reader reads the page from it.
* **Focusable controls** for every link, button, option, answer, contents
  line, image and gallery: transparent `<button>`s positioned over the
  painted widgets, labelled, in reading order. Options and answers are
  grouped under their prompt (`role="group"`); a selected option has
  `aria-pressed`, an answered quiz `aria-disabled`.
* **Live region** announcing "Page N of M", front and back cover, "Correct."
  / "Not quite.", bookmark added or removed.
* **Focus management.** Panels, dialogs and the lightbox trap focus, close on
  `Escape` and return focus to where it was (`trapFocus` in `ui/dom.ts`).
* **Keyboard** equivalents for page turning, jumps, panels and every
  control on a page (section 5). Dragging a page is pointer-only.
* **Reduced motion.** With the system setting or Motion → Reduced, page turns
  become an immediate change with a 140 ms opacity dip, jumps a short fade,
  entrance animations show their final state, and UI transitions are off.
* **Target sizes.** Painted controls and toolbar buttons are at least 44 CSS
  px (`MIN_TARGET` in `layout/widgetMetrics.ts`, `styles.css`).
* **Contrast**: section 11.
* **Forced colours** (Windows high contrast): selected segments and focus
  rings use system colours (`@media (forced-colors: active)`).
* **Safe areas**: toolbars, panels and the shelf respect
  `env(safe-area-inset-*)`.
* **Language**: the reading view's `lang` is the book's language.
* **Media**: caption tracks, alt text, transcripts (section 8). The compiler
  requires `alt` on every image and video.

Honest limits:

* **Text on the page cannot be selected or copied.** It is pixels on a
  canvas. Browser find-in-page, translation and reader modes do not see it
  either.
* **No text-to-speech** in the reader. Screen readers work through the
  outline; there is no read-aloud feature and no highlighting of spoken text.
* **Search matches are not highlighted on the page**; search takes you to the
  page.
* **The outline exists only while pages are at rest.** During a turn it is
  removed and rebuilt, so a screen reader's position inside the page is lost
  on every turn. Inline emphasis and links are flattened to text in the
  outline; links are reachable as separate buttons.
* **Safari on macOS** moves focus to buttons with Option+Tab unless "Keyboard
  navigation" (full keyboard access) is enabled in system settings. That is
  Safari's behaviour for all buttons; the end-to-end test uses Option+Tab
  there.
* **No right-to-left or bidirectional layout.** The typesetter places words
  left to right. Arabic, Hebrew and similar scripts are not laid out
  correctly. There is no vertical text.
* **Hyphenation** exists for six languages only (section 4).
* **The reader's own interface is in English.**
* **No zoom** into a page; the text-size setting does not apply to fixed
  pages.
* No formal audit against WCAG has been done. What is tested automatically is
  in [`testing.md`](testing.md).

---

## 13. Performance

Code: `ui/stage.ts`, `gl/renderer.ts`.

* **Render on demand.** There is no continuous frame loop. A frame is drawn
  only while a leaf is moving, while an entrance animation runs, or after
  `requestRender()`. A book that is simply being read uses no CPU.
* **Pages are textures.** A page is painted once into a scratch canvas and
  uploaded; turning it re-uses the texture. A page is repainted when its
  content changes (state, settings, an image finishing decoding).
* **Texture resolution** is the device pixel ratio (at most 3), reduced so
  that no side exceeds 3072 px (or the GPU's limit) and no page exceeds about
  5 megapixels.
* **Texture cache.** The WebGL renderer keeps page textures up to a budget of
  220 MiB and evicts least-recently-used ones down to 80% of it.
* **Prewarming.** When idle, the stage paints the surfaces a turn in either
  direction would need (the sheets at `pos`, `pos ± 1`, `pos − 2`, and the
  riffle stand-in) in `requestIdleCallback`.
* **Images** decode asynchronously through `<img>`; a page painted before its
  images are ready is repainted when they arrive. Reader-side limits: 96 MiB
  per image, font or caption file; 64 megapixels per raster image; 64 MiB of
  IR (`LIMITS` in `core/book.ts`).
* **Layout** is synchronous and runs for the whole book on every re-layout.
  `diagnostics().layoutMs` reports the last run. Measured numbers are in
  [`testing.md`](testing.md).
* **Context loss.** If the WebGL context is lost and restored, every surface
  is repainted.

---

## 14. Diagnostics and URL parameters

### `window.darm`

Set in `main.ts`. It exposes no book content.

```js
window.darm.diagnostics()
// → null on the shelf, or:
// {
//   renderer: "webgl2" | "canvas2d",
//   pages, page, pos, count,      // page is null while the book is closed
//   settled, spread,
//   layoutMs,                      // duration of the last layoutBook()
//   frameTimes,                    // ms between frames of recent turns (≤ 600)
//   state,                         // current variable values
//   textureBytes                   // 0 for the 2D renderer
// }
```

`window.darm.openFile(bytes, name)` hands a file to the reader (used by
native shells). `window.darm.app` is the `App` instance, for tests.

### URL parameters

Read in `App.start`.

| Parameter | Effect |
|---|---|
| `?book=/path` | Open this book instead of the library. Only same-origin relative URLs are accepted. The book is read with HTTP range requests and is not added to the library. Used by `darm open` and `darm dev`. |
| `&name=…` | Display name for that preview. |
| `&dev=1` | With `book`: long-poll `/__darm/wait` and reload the book after each rebuild, keeping the position; show the compiler's diagnostics in a banner when a rebuild fails. Used by `darm dev`. |
| `?renderer=2d` | Force the Canvas 2D renderer. |

The service worker (offline use of the installed web app) is registered only
in production builds, in a secure context, and not when `book` is present.
