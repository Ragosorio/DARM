# The DARM language (grammar version 1)

DARM source files (`.darml`) describe an interactive book. The language is
**declarative**, **indentation-based** and deliberately **not Turing-complete**:
there are no loops, no user functions that compute, no I/O and no way to run
foreign code. It looks like Python, but it is its own language with its own
compiler (`darm`, written in Rust).

* Source file extension: **`.darml`** · project file: **`darm.toml`** ·
  compiled book: **`.darm`** · key file for protected books: **`.darmkey`**.
* Every construct below is implemented by the compiler in this repository.
  `darm reference --json` prints the same element tables in machine-readable form.

Contents: [A first book](#1-a-first-book) · [Lexical rules](#2-lexical-rules) ·
[Grammar](#3-grammar-ebnf) · [File structure](#4-file-structure) ·
[Content blocks](#5-content-blocks) · [Rich text](#6-rich-text) ·
[Media](#7-media-and-assets) · [State, expressions, actions](#8-state-expressions-and-actions) ·
[Interactive blocks](#9-interactive-blocks) · [Fixed pages](#10-fixed-pages) ·
[Animations and transitions](#11-animations-and-transitions) ·
[Components](#12-components) · [Covers and front matter](#13-covers-and-front-matter) ·
[Semantics and limits](#14-semantics-and-limits) · [Diagnostics](#15-diagnostics)

---

## 1. A first book

```darml
darm 1

book "A Small Atlas of Rain":
    author = "Mara Quill"
    language = "en"

chapter "Drizzle":
    text "Rain begins as a rumour. *Somebody* smells it first."
    text """
        The second paragraph is long, so it uses a triple-quoted string.
        Line breaks inside it are just spaces.

        A blank line starts a new paragraph.
        """

chapter "Downpour":
    text "And then it arrives."
```

```sh
darm check book.darml          # validate
darm build book.darml          # writes a-small-atlas-of-rain.darm
darm open  a-small-atlas-of-rain.darm
```

That is a complete book: the compiler generates a deterministic cover and back
cover (see [§13](#13-covers-and-front-matter)), a title page, a colophon and a
table of contents.

## 2. Lexical rules

* **Encoding**: UTF-8. Line endings LF or CRLF. A leading BOM is ignored.
* **Indentation** is significant and uses **spaces only** (a tab in indentation
  is error `E0102`). A block is any consistent deeper indentation; `darm fmt`
  normalises to 4 spaces.
* **Comments** start with `#` and run to the end of the line. (Colours are
  therefore written as strings: `"#7a2e22"`.)
* **String escapes** also pass the rich-text escapes of [§6](#6-rich-text)
  (`\*`, `\[`, `\{` …) through unchanged.
* **Line joining**: inside `( )` and `[ ]` newlines and indentation are ignored,
  and **a line that ends with a comma continues on the next line** (so long
  argument lists can be wrapped without parentheses).
* **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`, at most 64 characters.
* **Keywords**: `true false and or not in if elif else`.
* **Literals**

  | Kind | Examples | Notes |
  |---|---|---|
  | integer | `0`, `42`, `-3` | |
  | decimal | `1.5` | |
  | percent | `60%` | a decimal with a `%` suffix |
  | duration | `300ms`, `1.5s` | stored as milliseconds |
  | boolean | `true`, `false` | lower-case only |
  | string | `"…"` | escapes: `\"` `\\` `\n` `\t` `\u{1F600}` |
  | text block | `"""…"""` | multi-line; common indentation is removed |
  | list | `["a", "b"]` | |
  | tuple | `(10, 20)` | |
  | range | `0..5` | inclusive, integers only |

* **Operators**: `= += -= == != < <= > >= + - * ..` and the punctuation `( ) [ ] , :`.

## 3. Grammar (EBNF)

`NEWLINE`, `INDENT` and `DEDENT` are produced by the lexer exactly as in Python.

```ebnf
file        = pragma , { statement } ;
pragma      = "darm" , INT , NEWLINE ;

statement   = if_stmt | component | assignment | element ;

element     = NAME , [ args ] , ( NEWLINE | ":" , block ) ;
args        = arg , { "," , arg } ;
arg         = [ NAME , "=" ] , expr ;
block       = NEWLINE , INDENT , statement , { statement } , DEDENT ;

assignment  = NAME , ( "=" | "+=" | "-=" ) , expr , NEWLINE ;

if_stmt     = "if" , expr , ":" , block ,
              { "elif" , expr , ":" , block } ,
              [ "else" , ":" , block ] ;

component   = "component" , NAME , "(" , [ params ] , ")" , ":" , block ;
params      = param , { "," , param } ;
param       = NAME , [ "=" , expr ] ;

expr        = or_expr , [ "in" , ( range | list ) ] ;
or_expr     = and_expr , { "or" , and_expr } ;
and_expr    = not_expr , { "and" , not_expr } ;
not_expr    = "not" , not_expr | comparison ;
comparison  = sum , [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) , sum ] ;
sum         = term , { ( "+" | "-" ) , term } ;
term        = unary , { "*" , unary } ;
unary       = "-" , unary | atom ;
atom        = INT | DECIMAL | PERCENT | DURATION | STRING | "true" | "false"
            | NAME | call | list | tuple | "(" , expr , ")" ;
call        = NAME , "(" , [ args ] , ")" ;
list        = "[" , [ expr , { "," , expr } , [ "," ] ] , "]" ;
tuple       = "(" , expr , "," , [ expr , { "," , expr } , [ "," ] ] , ")" ;
range       = sum , ".." , sum ;
```

The grammar is uniform on purpose: **every statement is either an element
(`name args` with an optional `:` block), an assignment, an `if`, or a
component definition.** Which element names, arguments and child statements
are allowed where is decided by semantic analysis (the tables below), so a
mistake produces a targeted message ("`image` does not take `colour`; did you
mean `color`?") rather than a generic syntax error.

Calls (`image("a.png")`) are only meaningful inside `assets:`; there are no
user-callable functions.

## 4. File structure

A file starts with the pragma `darm 1` (the grammar version) followed by
top-level statements, in any order:

| Statement | Count | Purpose |
|---|---|---|
| `book "Title":` | exactly 1 | metadata |
| `theme:` | 0–1 | interior colours and type |
| `assets:` | any | media and font declarations |
| `state:` | any | state variables |
| `cover:` | 0–1 | front cover design |
| `back_cover:` | 0–1 | back cover design |
| `component name(...):` | any | reusable block templates |
| `chapter "Title":` | 1 or more | content, in reading order |
| `include "file.darml"` | any | splice another file's top-level statements here |

`include` paths are relative to the including file, must stay inside the
project directory and may not form a cycle. Included files also start with
`darm 1`. Exactly one file in the include tree contains `book`.

### 4.1 `book`

```darml
book "The Cartographer of Tides":
    id = "org.example.cartographer"       # stable id; derived from title+author when absent
    subtitle = "A coastal fable"
    author = "Inés Valcárcel"             # or a list: ["A", "B"]
    language = "en"                       # required, BCP 47
    edition = "First digital edition"
    version = "1.0.0"
    publisher = "Saltmarsh Press"
    published = "2026-11-16"              # YYYY-MM-DD
    description = "Shown on the back cover when no blurb is given."
    rights = "© 2026 Inés Valcárcel. CC BY-SA 4.0."
    layout = reflow                       # reflow (default) | fixed
    page_size = (3, 4)                    # width : height for `fixed` books (default (3, 4)); reflow pages fit the window
    offline = true                        # default true
    network = ["media.example.org"]       # hosts for remote assets; requires offline = false
    front_matter = [title, colophon, contents]   # default; any subset, this order
    typography = smart                    # smart (default) | plain
    credit "Illustrations", "Tomás Ferrer"
    credit "Editor", "Ada Lindqvist"
```

Required: the title (positional), `author` and `language`. `typography = smart`
turns `"…"`/`'…'` into curly quotes, `--` into –, `---` into — and `...` into ….

### 4.2 `theme`

```darml
theme:
    paper = "#f6f1e7"
    ink = "#1d1a16"
    accent = "#9c3d2a"
    dark_paper = "#191715"      # optional dark-mode overrides (all three or none)
    dark_ink = "#e9e2d4"
    dark_accent = "#e08a6f"
    body_font = serif           # serif | sans | mono | a font asset name
    heading_font = serif
    paragraph = indent          # indent (default) | spaced
    justify = true              # default true
    hyphenate = true            # default true
    drop_caps = true            # default false: first paragraph of each chapter gets a drop cap
```

## 5. Content blocks

Blocks appear inside `chapter`, `section`, `box`, `if`/`elif`/`else` and
component bodies. **Every block accepts the common arguments**:

| Argument | Type | Meaning |
|---|---|---|
| `id=name` | identifier | makes the block a jump target (unique in the book) |
| `align=` | `left` `center` `right` `justify` | horizontal alignment |
| `color=` | colour string | text colour override |
| `scale=` | decimal 0.25–8 | type size multiplier |
| `animate=` `delay=` `duration=` | see [§11](#11-animations-and-transitions) | entrance animation |
| `at=(x, y)` `size=(w, h)` | percent tuples | placement, **only inside `page`** ([§10](#10-fixed-pages)) |

| Element | Positional | Named | Body |
|---|---|---|---|
| `chapter` | title | `id` `numbered=true` `opener=true` `toc=true` `when=` | blocks |
| `section` | title | `id` `toc=true` `when=` | blocks |
| `heading` | text | | — |
| `text` | rich text | `style=body` | — |
| `quote` | rich text | `by=` rich text | — |
| `list` | | `ordered=false` | `item` statements |
| `item` | rich text | | — |
| `image` | asset | `caption=` `width=100%` `fit=contain` | — |
| `gallery` | 2–24 assets | `caption=` | — |
| `video` | asset | `caption=` `autoplay=false` `loop=false` `muted=false` | — |
| `audio` | asset | `caption=` | — |
| `divider` | | `style=ornament` (`rule` `ornament` `blank`) | — |
| `space` | lines (0.25–40) | | — |
| `break` | | | — (start a new page) |
| `box` | | `style=rule` (`rule` `tint` `bar`) `title=` | blocks |
| `button` | label | `goto=` `transition=` | optional actions |
| `choice` | prompt | | `option` statements |
| `option` | label | `when=` `goto=` `transition=` | optional actions |
| `quiz` | question | | `answer`, `explain`, `on_correct` |
| `answer` | label | `correct=false` | — |
| `explain` | rich text | | — |
| `on_correct` | | | actions |
| `page` | | `background=` colour | framed blocks |

`text` styles: `body`, `lead` (larger opening paragraph), `small`, `caption`,
`dropcap`, `verse` (line breaks preserved), `mono`, `display` (title lettering).

```darml
chapter "The Ebb", id=ebb:
    text "Every evening the sea withdrew from Port Ameline.", style=lead
    section "What the tide left", id=left_behind:
        image tide_chart, caption="Fig. 1. The *long* ebb.", width=80%
        quote "A coast is a sentence the sea keeps rewriting.", by="Harbour proverb"
        list ordered=true:
            item "Rope, tarred."
            item "A **brass** sounding lead."
        divider
    break
    text "…"
```

A chapter always starts on a new page with an *opener* (number, title,
ornament) unless `opener=false`. `numbered=false` omits the ordinal (prefaces,
epilogues). `toc=false` hides it from the table of contents.

## 6. Rich text

Wherever the tables say *rich text*, the string may contain inline markup:

| Markup | Result |
|---|---|
| `*italic*` | *italic* |
| `**bold**` | **bold** |
| `` `code` `` | monospaced |
| `[label](target_id)` | link to an anchor in the book (`id=` or a reserved target) |
| `[label](https://example.org)` | external link (`https:`, `http:`, `mailto:` only) |
| `{name}` | current value of a state variable (or a component parameter) |
| `\*` `\[` `\]` `\{` `\}` `` \` `` `\\` | literal character |

Markup nests one level (`**bold with *italic* inside**`). In `"""` text blocks
a blank line separates paragraphs (for `text`), and a single newline is a
space — except with `style=verse`, where each newline is a line break.

Reserved link/jump targets: `cover`, `contents`, `start` (first page after the
front matter) and `end` (the back cover).

## 7. Media and assets

Files are declared once in `assets:` and referred to by name.

```darml
assets:
    tide_chart = image("art/tide-chart.svg", alt="Hand-drawn chart of a bay at low water")
    film = video("media/harbour.mp4", alt="Fog lifting over a harbour",
                 poster="media/harbour.jpg", captions="media/harbour.en.vtt")
    bell = audio("media/bell.m4a", title="The fog bell", transcript="One bell stroke, then waves.")
    display_face = font("fonts/Marcellus-Regular.ttf")
    trailer = video(url="https://media.example.org/trailer.mp4", alt="Trailer")
```

| Constructor | Formats (verified by content, not by extension) | Named arguments |
|---|---|---|
| `image(path)` | PNG, JPEG, GIF, WebP, SVG | `alt` (**required**) `title` |
| `video(path)` | MP4, WebM | `alt` (**required**) `title` `poster` (image path) `captions` `transcript` |
| `audio(path)` | MP3, M4A/AAC, Ogg, WAV, FLAC | `title` `captions` `transcript` |
| `font(path)` | WOFF2, TTF, OTF | |

* Paths are relative to the file that declares them and must stay inside the
  project directory. A missing file is error `E0401`.
* `captions` is a WebVTT path or a list of paths. The language of a track is
  taken from a `.<lang>.vtt` suffix (`harbour.es.vtt` → `es`), otherwise the
  book language.
* SVG images must be static: scripts, event handlers, `foreignObject` and
  external references are rejected (`E0405`).
* `url=` instead of a path declares a **remote** asset. Only `video` and
  `audio` can be remote; images, fonts and captions are always packaged. A
  remote asset needs `offline = false` and its host listed in `network`; only
  `https` is allowed; and the reader still asks the user before the first
  request to each host.
* Unused assets are still packaged but reported (`W0603`).

## 8. State, expressions and actions

```darml
state:
    courage = 0 in 0..5                          # int, clamped to 0..5
    has_lantern = false                          # bool
    route = "shore" in ["shore", "cliff", "caves"]   # one of a closed set
    visits = 0                                   # int, default range 0..1000000
```

**Types**: `int`, `bool`, `text` (only as members of a closed set). Types are
checked at compile time.

**Expressions**: integer arithmetic `+ - *` (saturating), comparisons
`== != < <= > >=` (ordering only on ints; equality on same types), boolean
`and or not`, parentheses, literals and state variables. No division, no
calls.

**Actions** are the statements allowed inside `button`, `option` and
`on_correct` bodies:

```darml
button "Light the lantern":
    has_lantern = true
    courage += 1
    goto cellar, transition=fade
```

| Action | Meaning |
|---|---|
| `name = expr` | assign (ints clamp to their range) |
| `name += expr`, `name -= expr` | add / subtract (ints only) |
| `goto target` | jump to an anchor; optional `transition=flip|fade|none` |
| `reset` | restore **all** state and clear recorded quiz/choice answers |
| `reset name` | restore one variable |

Actions run in order; the last `goto` wins and is performed after the others.
`button "Label", goto=target` is shorthand for a body with a single `goto`.

## 9. Interactive blocks

```darml
choice "The path forks at the breakwater.", id=fork:
    option "Climb to the lighthouse":
        route = "cliff"
        courage += 1
        goto lighthouse
    option "Wade into the sea caves", when=has_lantern:
        route = "caves"
        goto caves

if route == "cliff" and courage >= 2:
    text "From up here the whole coast is a map."
elif route == "caves":
    text "The lantern hisses."
else:
    text "You stay on the shingle."

quiz "Which star does the pilot steer by?", id=star_quiz:
    answer "Polaris", correct=true
    answer "Sirius"
    answer "Vega"
    explain "Polaris sits within a degree of the celestial pole."
    on_correct:
        courage += 1
```

* `when=` on `chapter`, `section` and `option` keeps that item out of the book
  while the condition is false. A chapter with `when=` is skipped by linear
  reading and omitted from the contents until its condition holds — this plus
  `goto` is how branching narratives are built.
* `if`/`elif`/`else` include or exclude blocks; the reader re-flows the chapter
  when state changes.
* A `quiz` has 2 to 12 `answer`s, at least one of them `correct=true`; a
  `choice` has 1 to 12 `option`s. The reader keeps the first answer given to a
  quiz (it cannot be changed until a `reset`).
* A choice remembers the selected option and may be changed by the reader; its
  actions run each time an option is picked. `on_correct` runs once, the first
  time a correct answer is picked. `reset` clears both.
* Interactive blocks never trap the reader: page turning, the contents and
  every reader control remain available at all times.

## 10. Fixed pages

`page:` is a composition that occupies exactly one page. Children are placed
with `at=(x, y)` and `size=(w, h)` in **percent of the page** (origin top-left).

```darml
chapter "Plates", opener=false:
    page background="#10202c":
        image tide_chart, at=(0, 0), size=(100, 100), fit=cover
        text "The Charted Coast", style=display, color="#ffffff",
             at=(8, 6), size=(84, 14), align=center
        button "Begin", goto=ebb, at=(35, 84), size=(30, 8)
```

* With `layout = fixed`, every top-level block of every chapter must be a
  `page`, and the reader keeps `page_size` proportions on every device
  (letter-boxing as needed).
* In a `reflow` book, `page` blocks are full-page plates between flowing text.
* `page` must be a direct child of `chapter`. To show a plate conditionally,
  put it in a chapter with `when=`.
* Inside a page: no `break`, no nested `page`, no `section`; `if` is allowed
  (its children carry the frames). Text that overflows its frame is shrunk to
  fit by the reader, down to 50%; beyond that the compiler cannot know, so keep
  frames generous.

## 11. Animations and transitions

**Entrance animations** play when a page becomes visible:

```darml
image tide_chart, animate=rise, delay=200ms, duration=700ms
```

`animate` is one of `fade`, `rise`, `drop`, `slide_left`, `slide_right`,
`zoom`. `duration` defaults to `600ms` (1 ms–10 s), `delay` to `0ms` (max 30 s).
Readers with reduced motion enabled show the final state immediately.

**Page turns** are always the reader's physical page-flip. A **jump** (`goto`,
links, contents) riffles the leaves in between by default; `transition=fade`
cross-fades and `transition=none` cuts.

## 12. Components

A component is a named, parameterised group of blocks, expanded at compile
time. It cannot recurse and cannot define state.

```darml
component field_note(body, label="Field note"):
    box style=bar, title=label:
        text body, style=small

chapter "Notes":
    field_note "Spring tides run **two hours** late here."
    field_note "Never trust a calm.", label="Warning"
```

Parameters may be used as argument values (`text body`, `title=label`) and as
`{label}` inside rich text, where they shadow state variables. Arguments are
matched positionally then by name; parameters without a default are required.
A component name must not collide with a built-in element.

## 13. Covers and front matter

Every DARM book has a front cover and a back cover; the reader opens on the
former and ends on the latter. **Policy**: if the source has no `cover:` (or
`back_cover:`) block, the compiler generates a deterministic typographic design
from the title, author and a colour derived from the book id, and reports
`W0601` (`W0602`). A cover that *is* declared but refers to a missing image is
an error (`E0401`).

```darml
cover:
    style = classic         # classic | band | minimal | modern
    color = "#7a2e22"       # cloth colour
    ink = "#f3e9d2"         # lettering
    accent = "#d9b46a"      # foil rules and ornaments
    image = cover_art       # optional image asset
    full_bleed = false      # true: image covers the board
    show_text = true        # false only with an image that carries the lettering
    title = "…"             # default: the book title / subtitle / authors
    subtitle = "…"
    author = "…"

back_cover:
    color = "#7a2e22"       # defaults to the front cover's colours
    tagline = "A coastal fable about maps that lie."
    image = author_portrait
    text "The blurb. One or more `text` statements; *rich text* allowed."
```

Without `text`, the back-cover blurb is the book `description`.

Generated front matter, in order: **title page**, **colophon** (edition,
publisher, rights, credits, identifier, "made with DARM" line) and **contents**.
`book: front_matter = [...]` selects a subset.

## 14. Semantics and limits

* **Ids** (`id=`) share one namespace across chapters, sections and blocks.
  Items without `id` get internal ids that start with `_` and cannot be jump
  targets; ids starting with `_` are reserved (`E0317`).
* Arguments whose value is one of a fixed set of words (`style=lead`,
  `layout = reflow`) accept the bare word or the same word as a string.
* **Reading order** is the order of `chapter` statements (after includes),
  filtered by `when=`.
* **Determinism**: the same sources and assets always produce the same IR and,
  for unencrypted books, a byte-identical `.darm`.
* **Limits** (compile errors beyond them): 4096 chapters, 200 000 blocks,
  nesting depth 12, 256 state variables, 64 enum values, 12 options/answers,
  32 actions per body, 24 gallery images, expression depth 32, include depth 16,
  component expansion depth 16, 8 KiB per plain string, 64 KiB per text run.
  Integers are limited to ±1 000 000 000.
* What the language **cannot** do, by design: loops, arithmetic division,
  string computation, timers, network requests, file access, scripting,
  embedding HTML or JavaScript.

## 15. Diagnostics

Every diagnostic has a stable code, a source position (`file:line:column`), the
offending line with a caret, and usually a `help:` line with the exact repair.
`darm check --format json` emits the same information as JSON, and
`darm explain E0304` prints a longer explanation. See
[`docs/errors.md`](errors.md) for the full catalogue and the most common
mistakes.

```
error[E0304]: unknown asset `tide_chrt`
  --> book.darml:14:11
   |
14 |     image tide_chrt, caption="Fig. 1"
   |           ^^^^^^^^^
   = help: did you mean `tide_chart`?
```

| Range | Area |
|---|---|
| `E01xx` | lexical (characters, strings, indentation) |
| `E02xx` | syntax |
| `E03xx` | names, types, structure |
| `E04xx` | assets and files |
| `E05xx` | project, includes, packaging |
| `W06xx` | warnings (never fail the build unless `--deny-warnings`) |
