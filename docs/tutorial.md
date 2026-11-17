# Tutorial: a small book from zero

This tutorial builds one original book, *Notes from a Small Hill*, in ten
short steps, and uses every part of the toolchain on the way. It is written
to be followed top to bottom by a person or by an AI agent. At the end you
will have a 123-line source file, a `.darm` package of about 4 KiB, a
password-protected copy, and the book open in the reader.

* Reference for everything used here: [`language.md`](language.md).
* Every diagnostic code: [`errors.md`](errors.md).
* Ready-made examples: [`../examples/README.md`](../examples/README.md).

**How this document was checked.** Every `darml` block below was assembled
into the file it belongs to and run through `darm check` and `darm fmt
--check` with the `darm 0.1.0` binary built from this repository; the blocks
are in canonical format, so `darm fmt` will not change them. Command output
shown in `text` blocks is real output, with terminal colours removed. Blocks
that show a mistake on purpose are compiler output, not source to copy.

Contents: [1. Build the CLI](#1-build-the-cli) ·
[2. Create a project](#2-create-a-project) ·
[3. The smallest book](#3-the-smallest-book) · [4. Metadata](#4-metadata) ·
[5. Text and rich text](#5-text-and-rich-text) ·
[6. Sections, lists, quotes, boxes](#6-sections-lists-quotes-boxes) ·
[7. An image](#7-an-image) · [8. Theme and covers](#8-theme-and-covers) ·
[9. State, a choice, conditional chapters](#9-state-a-choice-conditional-chapters) ·
[10. A quiz](#10-a-quiz) · [11. A fixed page](#11-a-fixed-page) ·
[12. A component](#12-a-component) · [13. Format](#13-format-darm-fmt) ·
[14. Check](#14-check-a-real-error-and-its-fix) ·
[15. Build and inspect](#15-build-and-inspect) ·
[16. Protect the book](#16-protect-the-book) · [17. Read it](#17-read-it) ·
[18. Common mistakes](#18-common-mistakes) ·
[19. Writing DARM with an AI model](#19-writing-darm-with-an-ai-model) ·
[20. The complete book](#20-the-complete-book)

---

## 1. Build the CLI

You need Rust 1.88 or newer. From the repository root:

```sh
cargo build -p darm-cli
```

The binary is `target/debug/darm`. The rest of this tutorial writes `darm`;
either put it on your `PATH` or define an alias:

```sh
alias darm="$PWD/target/debug/darm"
darm --version
```

```text
darm 0.1.0
```

Sections 1–16 need nothing else. Section 17 (`darm open`, `darm dev`) also
needs a build of the reader, which needs Node.js; see
[`contributing.md`](contributing.md#building).

## 2. Create a project

```sh
darm init hill-notes --title "Notes from a Small Hill" --author "Noor Ibarra"
```

```text
created hill-notes/
  book.darml         the book source
  art/landscape.svg  an example illustration
  darm.toml          project settings

next:
  darm check hill-notes
  darm build hill-notes
  darm dev hill-notes      # live preview
```

`darm init` refuses a directory that exists and is not empty. Its other
options are `--language` (default `en`) and `--template essay|interactive`
(default `essay`).

### Anatomy of the project

| File | What it is |
|---|---|
| `book.darml` | The entry source file. A project may split its source over several files with `include "other.darml"`. |
| `art/landscape.svg` | An illustration (a red hill, a low sun, three lines of water). We reuse it below. |
| `darm.toml` | Optional project settings: `[book] entry` (default `book.darml`) and `output` (default `{slug}.darm`, where `{slug}` is the title in lower case with dashes), and a commented-out `[protect]` section. |
| `.gitignore` | Ignores `*.darm`, `*.darmkey` and `.darm-cache/`. |

Every `darm` command that takes a project accepts the directory, or the entry
`.darml` file, or nothing (the current directory).

The generated `book.darml` is a complete book that already compiles
(`darm check hill-notes` prints `ok`). Read it once, then **delete its
contents**: we are going to write the file from an empty page so that every
line is explained. Keep `art/landscape.svg` and `darm.toml`.

## 3. The smallest book

Put this in `hill-notes/book.darml`. It is the whole file.

```darml
darm 1

book "Notes from a Small Hill":
    author = "Noor Ibarra"
    language = "en"

chapter "The path up", id=path:
    text "The hill behind the harbour is not tall. It takes twenty minutes to climb."
```

* `darm 1` is the grammar version. It must be the first line of every
  `.darml` file.
* `book "Title":` holds the metadata. The title, `author` and `language` are
  required.
* Blocks are opened with `:` and indented with **spaces** (four per level).
  Tabs are an error.
* Strings use **double quotes**. Arguments after the first are separated by
  **commas**: `chapter "The path up", id=path:`.
* `id=path` gives the chapter a name other parts of the book can jump to.

```sh
darm check hill-notes
```

```text
warning[W0601]: no `cover:` block; a typographic cover was generated from the title and author
 --> book.darml:3:1
  |
3 | book "Notes from a Small Hill":
  | ^^^^
  = help: add a `cover:` block to design it (style, color, ink, accent, image)

warning[W0602]: no `back_cover:` block; one was generated from the book description
 --> book.darml:3:1
  |
3 | book "Notes from a Small Hill":
  | ^^^^
  = help: add a `back_cover:` block with `text "..."` for the blurb

0 errors, 2 warnings
ok: "Notes from a Small Hill" (4 chapters, 0 resources)
```

Two things to notice:

* The book is valid. Warnings (`W…`) never fail a build unless you ask with
  `--deny-warnings`. These two say the compiler generated both covers; we
  design them in step 8.
* "4 chapters" for a book with one: the compiler generates a title page, a
  colophon and a table of contents, and each is a chapter in the compiled
  book.

## 4. Metadata

Replace the `book` block with a fuller one:

```darml
book "Notes from a Small Hill":
    id = "org.example.small-hill"
    subtitle = "A walk in six short parts"
    author = "Noor Ibarra"
    language = "en"
    edition = "First edition"
    version = "1.0.0"
    published = "2026-11-16"
    description = "One hill, one evening, and a decision at the top."
    rights = "© 2026 Noor Ibarra. CC BY 4.0."
    credit "Illustration", "The DARM template"
```

`id` is the stable identifier of the work (derived from title and author when
absent). `description` becomes the back-cover blurb unless you write one.
`credit` lines, `edition`, `published`, `rights` and the id appear on the
generated colophon. The complete list of properties is in
[`language.md` §4.1](language.md#41-book), or run `darm reference`.

## 5. Text and rich text

Replace the chapter with this:

```darml
chapter "The path up", id=path:
    text "The hill behind the harbour is not tall. It takes twenty minutes to climb.", style=lead
    text """
        People climb it for the *view*, or so they say. I think they climb it
        because it is **there**, and because twenty minutes is short enough to
        be an excuse and long enough to be a walk.

        The path is marked `H1` on the town map. If you lose it, go back to
        [the contents](contents) and start again, or read about hills at
        [example.org](https://example.org/hills).
        """
    text "Some days you can see the far shore -- other days, nothing...", align=center
```

* `style=lead` sets a larger opening paragraph. Other styles: `body`,
  `small`, `caption`, `dropcap`, `verse`, `mono`, `display`.
* A `"""` text block holds long passages. Its common indentation is removed,
  a single newline is a space, and a **blank line starts a new paragraph**.
* Inline markup: `*italic*`, `**bold**`, `` `code` ``,
  `[label](target)` for a place in the book, `[label](https://…)` for the
  web. `contents` is one of four built-in targets (`cover`, `contents`,
  `start`, `end`).
* With the default `typography = smart`, `--` becomes an en dash, `...` an
  ellipsis and straight quotes become curly.
* `align=center` is one of the arguments every block accepts (`align`,
  `color`, `scale`, `id`, `animate`, `delay`, `duration`).

## 6. Sections, lists, quotes, boxes

Add a second chapter at the end of the file:

```darml
chapter "What to carry", id=carry:
    section "The short list", id=short_list:
        text "You need very little for a small hill."
        list ordered=true:
            item "Water."
            item "A coat, *even in summer*."
            item "Something to sit on."
        quote "A hill is a staircase that forgot its house.", by="Local saying"
    section "What to leave behind":
        box style=tint, title="A warning":
            text "Leave the map. The hill has one path, and the map is wrong about it."
        divider
        text "That is the whole of the preparation."
```

* `section` groups blocks under a title and gives the table of contents a
  second level.
* `list` contains `item` lines; `ordered=true` numbers them.
* `quote` takes an optional `by=`.
* `box` is a container with a style (`rule`, `tint`, `bar`) and an optional
  title.
* `divider` draws an ornament; `break` (not used here) starts a new page.

`darm check hill-notes` now reports `(5 chapters, 0 resources)`.

## 7. An image

Files are declared once, in an `assets:` block, and then referred to **by
name**. Add this block after `book`:

```darml
assets:
    landscape = image("art/landscape.svg", alt="A red hill, a low sun and three lines of water")
```

and add one line at the end of the chapter "The path up":

```darml
    image landscape, caption="Fig. 1. The hill from the harbour wall.", width=80%
```

so that the chapter reads:

```darml
chapter "The path up", id=path:
    text "The hill behind the harbour is not tall. It takes twenty minutes to climb.", style=lead
    text """
        People climb it for the *view*, or so they say. I think they climb it
        because it is **there**, and because twenty minutes is short enough to
        be an excuse and long enough to be a walk.

        The path is marked `H1` on the town map. If you lose it, go back to
        [the contents](contents) and start again, or read about hills at
        [example.org](https://example.org/hills).
        """
    text "Some days you can see the far shore -- other days, nothing...", align=center
    image landscape, caption="Fig. 1. The hill from the harbour wall.", width=80%
```

* The path is relative to the file that declares it and must stay inside the
  project.
* `alt` is **required** for images and video. It is what a screen reader
  says and what search finds.
* The format is checked from the file's content, not its extension. SVG must
  be static (no scripts, no external references).
* `width=80%` is a percent literal; `caption=` takes rich text.

The `assets:` block may sit anywhere at the top level; the order of top-level
blocks does not matter, only the order of chapters does.

`darm check hill-notes` now reports `(5 chapters, 1 resources)`.

## 8. Theme and covers

Add a `theme` block after `book`, and `cover` and `back_cover` blocks after
`assets`:

```darml
theme:
    paper = "#f7f3ea"
    ink = "#1f1c18"
    accent = "#7a2e22"
    drop_caps = true
```

```darml
cover:
    style = band
    color = "#7a2e22"
    ink = "#f4ead5"
    accent = "#d9b46a"
    image = landscape

back_cover:
    tagline = "Twenty minutes up. Longer down."
    text "A short book about a short walk, with *one* decision in it."
```

* Colours are **strings**: `"#7a2e22"`. A bare `#` starts a comment.
* `drop_caps = true` gives the first paragraph of each chapter a drop
  capital. Other theme properties: `dark_paper`/`dark_ink`/`dark_accent`,
  `body_font`, `heading_font`, `paragraph`, `justify`, `hyphenate`.
* Cover styles are `classic`, `band`, `minimal` and `modern`. `image =
  landscape` puts our asset on the cover.
* Inside these blocks, properties are written `name = value` on their own
  lines. On an element's own line, arguments are written `name=value` after
  commas. `darm fmt` normalises the spacing either way.

```sh
darm check hill-notes
```

```text
ok: "Notes from a Small Hill" (5 chapters, 1 resources)
```

No warnings left.

## 9. State, a choice, conditional chapters

A book can remember things. State is declared once, changed only by
*actions*, and read by conditions. Add a `state` block after `assets`:

```darml
state:
    route = "none" in ["none", "summit", "bench"]
    breath = 3 in 0..5
    saw_sunset = false
```

Three kinds of variable exist: a closed set of words, an integer with a
range (values are clamped to it), and a boolean.

Now add four chapters at the end of the file:

```darml
chapter "The fork", id=fork:
    text "Two thirds of the way up, the path splits. Breath left: {breath} of 5."
    choice "Which way?", id=which_way:
        option "Straight up to the summit":
            route = "summit"
            breath -= 2
            goto summit
        option "Left, to the bench":
            route = "bench"
            breath += 1
            goto bench

chapter "The summit", id=summit, when=route == "summit":
    text "The last stretch is steep. You arrive with {breath} breaths to spare."
    button "Watch the sun go down":
        saw_sunset = true
        goto way_down

chapter "The bench", id=bench, when=route == "bench":
    text "The bench faces the harbour. Somebody has carved a date into it."
    button "Head back", goto=way_down

chapter "The way down", id=way_down:
    if route == "summit" and saw_sunset:
        text "You walk down in the afterglow, slower than you came up."
    elif route == "bench":
        text "You walk down rested, with {breath} breaths left and nothing to prove."
    else:
        text "You have not chosen a way yet. Go back to [the fork](fork)."
    button "Walk it again":
        reset
        goto fork
```

* `{breath}` inside text shows the current value of a variable.
* A `choice` holds `option`s. The indented lines under an option are
  **actions**: assignments (`=`, `+=`, `-=`), `goto target`, and `reset`.
  Actions are only allowed inside `button`, `option` and `on_correct`.
* `when=` on a chapter keeps it out of the book until the condition holds.
  "The summit" and "The bench" do not exist for a reader who has not chosen,
  and only one of them ever does. Linear page turning skips the other, and
  the contents and search do not show it.
* `if` / `elif` / `else` include or exclude blocks inside a chapter.
* `button "Head back", goto=way_down` is shorthand for a button whose only
  action is a `goto`.
* `reset` restores every variable and clears recorded answers.
* Expressions have `+ - *`, comparisons, `and or not`. There is no division,
  no loop and no function call.

`breath -= 2` from 3 gives 1; `breath += 1` from 3 gives 4; neither can leave
`0..5`.

## 10. A quiz

Replace the chapter "The summit" with this version:

```darml
chapter "The summit", id=summit, when=route == "summit":
    text "The last stretch is steep. You arrive with {breath} breaths to spare."
    quiz "From the summit, where does the sun set?", id=sunset_quiz:
        answer "Over the water, to the west", correct=true
        answer "Behind the town, to the east"
        answer "It depends on the season"
        explain "The sun always sets towards the west; the season only moves it along the horizon."
        on_correct:
            breath += 1
    button "Watch the sun go down":
        saw_sunset = true
        goto way_down
```

A `quiz` holds between 2 and 12 `answer` lines, at least one of them with
`correct=true`, an optional `explain`, and an optional `on_correct` block of
actions, which runs once, the first time a correct answer is picked. In the
reader the first answer stands; `reset` clears it.

## 11. A fixed page

A `page` is a composition that fills exactly one page. Its children are
placed with `at=(x, y)` and `size=(w, h)` in **percent of the page**. Add
this chapter at the end:

```darml
chapter "Plate", id=plate, opener=false, numbered=false:
    page background="#1f2a33":
        image landscape, at=(0, 0), size=(100, 62), fit=cover
        text "The Small Hill", style=display, color="#f4ead5", at=(8, 66), size=(84, 12),
                align=center
        text "Seen from the harbour wall at dusk.", color="#f4ead5", at=(10, 79), size=(80, 8),
                align=center
        button "Back to the start", goto=start, at=(30, 89), size=(40, 7)
```

* `opener=false` drops the chapter-opener page (number, title, ornament);
  `numbered=false` leaves the chapter out of the chapter numbering.
* Every child of a `page` needs both `at=` and `size=`.
* `fit=cover` crops the image to fill its frame.
* A line that ends with a comma continues on the next line. That is how the
  two long `text` lines are wrapped; `darm fmt` does this for you at 100
  columns.
* `goto=start` jumps to the first page after the front matter.

In this book (`layout = reflow`, the default) a `page` is a plate between
flowing chapters. With `layout = fixed` in `book`, every top-level block of
every chapter must be a `page`.

## 12. A component

A component is a named group of blocks with parameters, expanded at compile
time. Define it at the top level (we put it after `back_cover`):

```darml
component trail_note(body, label="Trail note"):
    box style=bar, title=label:
        text body, style=small
```

and use it like an element in a last chapter:

```darml
chapter "Trail notes", id=notes, numbered=false:
    trail_note "The gate at the bottom sticks. Lift it, then push."
    trail_note "After rain the top third is **slippery**.", label="Warning"
```

`body` is required; `label` has a default. Arguments are matched by position
and then by name. A component cannot recurse and cannot define state.

```sh
darm check hill-notes
```

```text
ok: "Notes from a Small Hill" (11 chapters, 1 resources)
```

The file is finished. It is printed whole in [section 20](#20-the-complete-book).

## 13. Format: `darm fmt`

`darm fmt` rewrites `.darml` files in the canonical style: four-space
indentation, one blank line between top-level blocks, normalised spacing
around `=` and after commas, lines wrapped at 100 columns. It never changes
meaning, and it refuses to touch a file with syntax errors.

To see it work, here is a deliberately untidy (but valid) file,
`fmtdemo/book.darml`:

```text
darm 1
book "T":
  author="A"
  language =  "en"
chapter "One" , id = one :
  text   "Hello",style=lead
```

```sh
darm fmt --check fmtdemo
```

```text
would reformat fmtdemo/book.darml
```

`--check` writes nothing and exits with status 1 when a file would change,
which is what a CI job wants.

```sh
darm fmt fmtdemo
```

```text
formatted fmtdemo/book.darml
1 file(s) checked, 1 changed
```

The file is now:

```darml
darm 1

book "T":
    author = "A"
    language = "en"

chapter "One", id=one:
    text "Hello", style=lead
```

`darm fmt` with no path formats every `.darml` file under the current
directory. `darm fmt --stdout FILE` prints the result instead of writing it.

For our book:

```sh
darm fmt --check hill-notes
```

```text
1 file(s) checked, 0 would change
```

## 14. Check: a real error and its fix

Break the book on purpose. In the chapter "The fork", change `goto summit` to
`goto sumit` and run:

```sh
darm check hill-notes
```

```text
error[E0305]: unknown jump target `sumit`
  --> book.darml:78:18
   |
78 |             goto sumit
   |                  ^^^^^
   = help: did you mean `summit`?

1 error, 0 warnings
```

The exit status is 1. Every diagnostic has a stable code, a position
(`file:line:column`), the offending line with a caret, and usually a `help:`
line that states the repair. For a longer explanation with an example:

```sh
darm explain E0305
```

```text
E0305: unknown jump target

A `goto`, link or `goto=` argument names an id that no chapter, section or block declares.

Problem:
    button "Go", goto=nowhere

Fix:
    chapter "Somewhere", id=somewhere:
        …
```

Tools and AI agents should ask for JSON instead:

```sh
darm check hill-notes --format json
```

```text
{
  "diagnostics": [
    {
      "code": "E0305",
      "column": 18,
      "end_column": 23,
      "end_line": 78,
      "file": "book.darml",
      "help": "did you mean `summit`?",
      "line": 78,
      "message": "unknown jump target `sumit`",
      "severity": "error",
      "source_line": "            goto sumit"
    }
  ],
  "errors": 1,
  "ok": false,
  "warnings": 0
}
```

The fix is the one the help line gives: write `goto summit` again. Then:

```sh
darm check hill-notes --deny-warnings
```

```text
ok: "Notes from a Small Hill" (11 chapters, 1 resources)
```

`darm check` never writes anything. `darm build` runs the same checks first
and writes no file when there are errors.

## 15. Build and inspect

```sh
darm build hill-notes
```

```text
built hill-notes/notes-from-a-small-hill.darm (4.1 KiB, 2 entries)
```

The output name comes from `output = "{slug}.darm"` in `darm.toml`; `-o FILE`
overrides it. Useful options: `--deny-warnings`, `--format json`, and
`--emit-ir FILE` to also write the compiled book as readable JSON.

```sh
darm inspect hill-notes/notes-from-a-small-hill.darm
```

```text
hill-notes/notes-from-a-small-hill.darm
  format      DARM 1.0
  uuid        6fe4ba41-bd86-89b7-8515-6bf309b50757
  size        4.1 KiB (4202 bytes)
  written by  darm 0.1.0
  protection  none (integrity checks only)
  public card "Notes from a Small Hill" by Noor Ibarra
  book        "Notes from a Small Hill" by Noor Ibarra
              id org.example.small-hill · language en · layout reflow · IR 1.0
  entries     2
    book.ir.json                       application/vnd.darm.ir+json   10.5 KiB → 3.0 KiB
    res/landscape.svg                  image/svg+xml           595 B → 329 B
```

A `.darm` file is a container with two kinds of entry: the compiled book
(`book.ir.json`, the *IR*) and its resources. The reader never sees your
`.darml` source; it is not in the package. An unprotected build is
reproducible: the same sources and assets give the same bytes, and the UUID
of an unprotected package is derived from its content. With `darm 0.1.0` and
the file in section 20, yours should be the one above.

```sh
darm verify hill-notes/notes-from-a-small-hill.darm
```

```text
ok: hill-notes/notes-from-a-small-hill.darm — 2 entries, 2 chunks, 11.1 KiB verified (SHA-256)
```

`verify` checks every byte against the integrity data in the file. Two more
commands look inside a package: `darm inspect FILE --ir` prints the IR as
JSON, and `darm extract FILE --out DIR` copies the entries out (add `--entry
NAME` to pick one). The container is specified in
[`file-format.md`](file-format.md).

## 16. Protect the book

A protected book is encrypted with AES-256-GCM under a key derived from a
password (Argon2id) or from a key file. Secrets are never taken from
command-line arguments; the password comes from an environment variable, a
file, standard input, or a prompt.

```sh
BOOK_PASSWORD="correct horse battery staple" \
  darm build hill-notes --encrypt --password-env BOOK_PASSWORD -o hill-notes/locked.darm
```

```text
built hill-notes/locked.darm (4.4 KiB, 2 entries, encrypted)
```

Use a passphrase of your own; the one above is printed in a public document.
Passwords shorter than 8 characters are refused (`E0504`). With `--encrypt`
and no password option, `darm` prompts for the password twice when run in a
terminal.

Without the password, only the public card is readable:

```sh
darm inspect hill-notes/locked.darm
```

```text
hill-notes/locked.darm
  format      DARM 1.0
  uuid        d72f230d-04df-4b44-8461-7663f8f3e292
  size        4.4 KiB (4506 bytes)
  written by  darm 0.1.0
  protection  AES-256-GCM, key slots: password
  kdf         Argon2id m=64 MiB t=3 p=1
  public card "Notes from a Small Hill" by Noor Ibarra
  contents    locked — supply the password or key file to list entries
```

(Your UUID will differ: a protected build uses a fresh random key every
time.) Add `--hide-card` to the build to keep the title and author out of the
clear part as well.

```sh
darm verify hill-notes/locked.darm
```

```text
darm: E_PKG_CREDENTIALS_REQUIRED: this book is protected; pass --password-env VAR, --password-file FILE, --password-stdin or --keyfile FILE
```

That exits with status 4. With the password:

```sh
BOOK_PASSWORD="correct horse battery staple" \
  darm verify hill-notes/locked.darm --password-env BOOK_PASSWORD
```

```text
ok: hill-notes/locked.darm — 2 entries, 2 chunks, 11.1 KiB verified (authenticated with AES-256-GCM)
```

Variations, all implemented:

| You want | Do |
|---|---|
| Always protect this project | Uncomment `[protect]` in `darm.toml` (`mode = "password"`, `password_env = "BOOK_PASSWORD"`) and run `BOOK_PASSWORD=… darm build hill-notes` |
| A key file instead of a password | `darm build hill-notes --new-keyfile hill.darmkey` creates the key file (on Unix, readable only by you) and locks the book with it. `darm keygen -o FILE` makes a key file on its own; `--keyfile FILE` uses an existing one. |
| Both a password and a key file | Pass both options; either one then opens the book. |
| Slower password guessing | `--kdf-memory MIB` (8–1024, default 64) and `--kdf-passes N` (1–16, default 3). The reader refuses books above 256 MiB or 8 passes. |

Before relying on protection, read
[`security.md` §3](security.md#3-limits-of-protection--read-this-before-selling-a-protected-book):
encryption keeps the file unreadable without the key; it cannot stop someone
who has the key from copying what they can see. A lost password or key file
cannot be recovered.

## 17. Read it

`darm open` and `darm dev` start a small server on `127.0.0.1` that hosts the
reader and the book, and open your browser. They need a build of the reader
(`apps/reader/dist`); from a checkout it is found automatically, otherwise
pass `--reader DIR` or set `DARM_READER_DIR`. If there is none, `darm` says
how to build it and exits with status 3.

```sh
darm open hill-notes/notes-from-a-small-hill.darm --port 4711
```

```text
reader: http://127.0.0.1:4711/?book=/__darm/book.darm&name=notes-from-a-small-hill.darm
press Ctrl+C to stop
```

(Without `--port` a free port is picked. `--no-browser` prints the address
and does not launch a browser.) `darm open hill-notes` builds the project first and then
opens the result. A protected book opens the same way and the reader asks for
the password.

In the reader: click the cover or press Space to open the book, use the arrow
keys or drag a page to turn it, make the choice at the fork, answer the quiz
on the summit, and watch the contents change (`t`). Everything the reader can
do is in [`reader.md`](reader.md).

While writing, use the live preview instead:

```sh
darm dev hill-notes --port 4712
```

```text
built hill-notes/.darm-cache/preview.darm (4.1 KiB, 2 entries)
reader: http://127.0.0.1:4712/?book=/__darm/book.darm&name=hill-notes%20%28live%20preview%29&dev=1
press Ctrl+C to stop
```

`darm dev` watches the project directory, rebuilds on every change and
reloads the book in the browser at the page you were on. When a save has
errors the previous build stays on screen and the compiler's messages appear
in a banner. The preview is never encrypted and never added to the reader's
library.

A `.darm` file can also be read without the CLI: serve `apps/reader/dist`
from any static web server and use "Add a book", or open it in the desktop
app where that exists ([`platforms.md`](platforms.md)).

## 18. Common mistakes

Each of these was triggered with the real compiler. The first line is the
message it printed; the last column is the repair.

| # | What was written | Diagnostic | Fix |
|---|---|---|---|
| 1 | A tab at the start of a line | `error[E0102]: tab character in indentation` | Indent with spaces, four per level. `darm fmt` cannot fix tabs. |
| 2 | No version line at the top | `error[E0201]: the file does not start with a `darm` version line` | Make `darm 1` the first line of every `.darml` file, included files too. |
| 3 | `text 'Hello'` | ``error[E0101]: unexpected character `'` `` | Strings use double quotes: `text "Hello"`. |
| 4 | `text "Hello", color=#7a2e22` | `error[E0203]: expected a value, found the end of the line` | `#` starts a comment. Colours are strings: `color="#7a2e22"`. |
| 5 | `chapter "One", numbered=False:` | ``error[E0203]: `False` is not a DARM value`` | Booleans are lower-case: `true`, `false`. |
| 6 | `text "Hello" style=lead` | ``error[E0203]: expected `,` or the end of the line, found `style` `` | Separate arguments with commas: `text "Hello", style=lead`. |
| 7 | `image "art/landscape.svg"` | ``error[E0304]: `"art/landscape.svg"` is a string, not an asset name`` | Declare the file in `assets:` and use its name: `image landscape`. |
| 8 | `landscape = image("art/landscape.svg")` | ``error[E0308]: `image(...)` needs `alt="..."` `` | Add `alt="…"` describing the picture. |
| 9 | `image landscap` | ``error[E0304]: unknown asset `landscap` `` (help: did you mean `landscape`?) | Use the declared name. |
| 10 | Two chapters with `id=one` | ``error[E0306]: the id `one` is already used`` | Ids are unique across chapters, sections and blocks. |
| 11 | `n += 1` directly inside a chapter | ``error[E0302]: `n = ...` is not allowed here`` | Assignments are actions; put them under a `button`, an `option` or `on_correct`. |
| 12 | `n = n / 2` | ``error[E0101]: unexpected character `/` `` (help: division is not part of the language) | Only `+`, `-` and `*` exist. Restructure the state. |
| 13 | `for i in 0..3:` | ``error[E0301]: unknown element `for` `` | There are no loops. Write the blocks out, or define a `component` and use it several times. |
| 14 | `text "Hello"` inside `page:` with no frame | ``error[E0316]: `text` is inside a `page` and needs `at=(x, y)` and `size=(w, h)` `` | Give every child of a `page` both arguments, in percent. |
| 15 | `button "Go", goto=nowhere` | ``error[E0305]: unknown jump target `nowhere` `` | Give the destination an `id=`, or use `cover`, `start`, `contents`, `end`. |
| 16 | `chapter "One", when=n:` with `n` an integer | `error[E0311]: a condition must be true or false, but this is an int` | Compare it: `when=n > 0`. |
| 17 | `text "Score: {score}"` with no such variable | ``error[E0314]: `{score}` does not name a state variable or component parameter`` | Declare it in `state:`, or write `\{` for a literal brace. |
| 18 | `text "Hello", colour="#7a2e22"` | ``error[E0303]: `text` has no argument named `colour` `` (help: did you mean `color`?) | Use the spelling in `darm reference`. |
| 19 | `chapter "One"` with no `:` before its indented body | `error[E0205]: unexpected indentation` | End the line that opens a block with `:`. |
| 20 | `text "Hello"` at the top level | ``error[E0302]: `text` cannot appear at the top level`` | Content goes inside a `chapter`. |
| 21 | `book` without `language` | ``error[E0308]: `book` needs the property `language` `` | Add `language = "en"` (a BCP 47 tag). |
| 22 | An asset path that does not exist | ``error[E0401]: file not found: `art/hill.svg` `` | Paths are relative to the source file that declares the asset. |

Two warnings you will meet: `W0603` (an asset is declared but never used; it
is still packaged) and `W0604` (a state variable or component is never
used).

When there is a syntax error (`E01xx`, `E02xx`), the compiler stops before
checking names and types, so fix those first and run `darm check` again.

## 19. Writing DARM with an AI model

DARM is small and regular on purpose, and the compiler is the judge. A model
does well when it is given the real reference and a loop to close.

**Give the model the language, not a description of it.** Either of:

```sh
darm reference --json     # elements, arguments, properties, constructors, diagnostics (also schemas/darm-language.json)
darm reference            # the same tables as plain text
```

or the text of [`language.md`](language.md). The JSON is generated from the
tables the compiler itself validates against, so it cannot be out of date.
One or two files from `examples/snippets/` make good few-shot examples.

**Close the loop with JSON diagnostics.**

1. The model writes `book.darml`.
2. `darm fmt PROJECT` (a failure here is a syntax error; go to step 4).
3. `darm check PROJECT --format json`. Exit status 0 and `"ok": true` means
   done.
4. For each entry in `diagnostics`, apply the `help` text at `file`, `line`,
   `column`. If the message is not enough, `darm explain CODE` prints a
   failing example and its fix.
5. Repeat from step 2. Then `darm build PROJECT --deny-warnings`.

Exit statuses are stable: 0 success, 1 source errors, 2 bad command line, 3
environment problem (file not found, no reader build, bad `darm.toml`), 4
invalid or locked package.

**Or let the model call the compiler directly.** `darm mcp` runs a Model
Context Protocol server on standard input and output (newline-delimited
JSON-RPC 2.0; only the `tools` capability). It has six tools:

| Tool | Arguments | Returns |
|---|---|---|
| `darm_check` | `path` | Diagnostics of a project on disk, as JSON |
| `darm_check_source` | `source` | Diagnostics of one `.darml` text with no assets on disk |
| `darm_format` | `source` | The canonical text, or diagnostics if it does not parse |
| `darm_build` | `path`, optional `out` | An **unencrypted** `.darm`; the output path and diagnostics |
| `darm_reference` | — | The language reference as JSON |
| `darm_explain` | `code` | The explanation of a diagnostic code |

A client configuration is one command with one argument, for example:

```json
{ "mcpServers": { "darm": { "command": "darm", "args": ["mcp"] } } }
```

`darm_check_source` cannot see files, so a source that declares assets
reports `E0401` for each of them; use `darm_check` with a path for real
projects. The server does not expose protection, `open` or `dev`.

**Rules that models most often get wrong** are the first twelve rows of the
table in section 18. The same list, with the workflow, is in
[`../AGENTS.md`](../AGENTS.md) for agents working in this repository.

## 20. The complete book

`hill-notes/book.darml`, as checked (`darm check --deny-warnings` and `darm
fmt --check` both pass):

```darml
darm 1

book "Notes from a Small Hill":
    id = "org.example.small-hill"
    subtitle = "A walk in six short parts"
    author = "Noor Ibarra"
    language = "en"
    edition = "First edition"
    version = "1.0.0"
    published = "2026-11-16"
    description = "One hill, one evening, and a decision at the top."
    rights = "© 2026 Noor Ibarra. CC BY 4.0."
    credit "Illustration", "The DARM template"

theme:
    paper = "#f7f3ea"
    ink = "#1f1c18"
    accent = "#7a2e22"
    drop_caps = true

assets:
    landscape = image("art/landscape.svg", alt="A red hill, a low sun and three lines of water")

state:
    route = "none" in ["none", "summit", "bench"]
    breath = 3 in 0..5
    saw_sunset = false

cover:
    style = band
    color = "#7a2e22"
    ink = "#f4ead5"
    accent = "#d9b46a"
    image = landscape

back_cover:
    tagline = "Twenty minutes up. Longer down."
    text "A short book about a short walk, with *one* decision in it."

component trail_note(body, label="Trail note"):
    box style=bar, title=label:
        text body, style=small

chapter "The path up", id=path:
    text "The hill behind the harbour is not tall. It takes twenty minutes to climb.", style=lead
    text """
        People climb it for the *view*, or so they say. I think they climb it
        because it is **there**, and because twenty minutes is short enough to
        be an excuse and long enough to be a walk.

        The path is marked `H1` on the town map. If you lose it, go back to
        [the contents](contents) and start again, or read about hills at
        [example.org](https://example.org/hills).
        """
    text "Some days you can see the far shore -- other days, nothing...", align=center
    image landscape, caption="Fig. 1. The hill from the harbour wall.", width=80%

chapter "What to carry", id=carry:
    section "The short list", id=short_list:
        text "You need very little for a small hill."
        list ordered=true:
            item "Water."
            item "A coat, *even in summer*."
            item "Something to sit on."
        quote "A hill is a staircase that forgot its house.", by="Local saying"
    section "What to leave behind":
        box style=tint, title="A warning":
            text "Leave the map. The hill has one path, and the map is wrong about it."
        divider
        text "That is the whole of the preparation."

chapter "The fork", id=fork:
    text "Two thirds of the way up, the path splits. Breath left: {breath} of 5."
    choice "Which way?", id=which_way:
        option "Straight up to the summit":
            route = "summit"
            breath -= 2
            goto summit
        option "Left, to the bench":
            route = "bench"
            breath += 1
            goto bench

chapter "The summit", id=summit, when=route == "summit":
    text "The last stretch is steep. You arrive with {breath} breaths to spare."
    quiz "From the summit, where does the sun set?", id=sunset_quiz:
        answer "Over the water, to the west", correct=true
        answer "Behind the town, to the east"
        answer "It depends on the season"
        explain "The sun always sets towards the west; the season only moves it along the horizon."
        on_correct:
            breath += 1
    button "Watch the sun go down":
        saw_sunset = true
        goto way_down

chapter "The bench", id=bench, when=route == "bench":
    text "The bench faces the harbour. Somebody has carved a date into it."
    button "Head back", goto=way_down

chapter "The way down", id=way_down:
    if route == "summit" and saw_sunset:
        text "You walk down in the afterglow, slower than you came up."
    elif route == "bench":
        text "You walk down rested, with {breath} breaths left and nothing to prove."
    else:
        text "You have not chosen a way yet. Go back to [the fork](fork)."
    button "Walk it again":
        reset
        goto fork

chapter "Plate", id=plate, opener=false, numbered=false:
    page background="#1f2a33":
        image landscape, at=(0, 0), size=(100, 62), fit=cover
        text "The Small Hill", style=display, color="#f4ead5", at=(8, 66), size=(84, 12),
                align=center
        text "Seen from the harbour wall at dusk.", color="#f4ead5", at=(10, 79), size=(80, 8),
                align=center
        button "Back to the start", goto=start, at=(30, 89), size=(40, 7)

chapter "Trail notes", id=notes, numbered=false:
    trail_note "The gate at the bottom sticks. Lift it, then push."
    trail_note "After rain the top third is **slippery**.", label="Warning"
```

Where next:

* [`language.md`](language.md) for everything not used here: `include`,
  `video` and `audio` assets with captions, galleries, embedded fonts,
  entrance animations (`animate=`), jump transitions, `layout = fixed`.
* [`../examples/README.md`](../examples/README.md) for two complete books and
  nine single-feature snippets.
* [`reader.md`](reader.md) for what a reader can do with your book.
