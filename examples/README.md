# DARM example books

Two complete showcase books, one minimal book and nine single-feature snippets.
Everything here is original work made for this repository: the prose, the SVG
plates, the audio and the video. Authors, publishers and places are fictional.
Section numbers (§) refer to [`docs/language.md`](../docs/language.md).

```
examples/
├── atlas-de-mareas/            Book 1 · Spanish essay-fable · reflow · multi-file
│   ├── book.darml              metadata, theme, assets, covers, component, includes
│   ├── chapters/00…09-*.darml  preface, 8 numbered chapters, closing note
│   └── art/*.svg               12 ink-and-wash plates
├── the-lighthouse-protocol/    Book 2 · English interactive story · reflow + fixed plates
│   ├── book.darml              metadata, theme, assets, state, covers, includes
│   ├── parts/watch.darml       the night: title plate … chart plate
│   ├── parts/mornings.darml    four endings and the ending plate
│   ├── art/*.svg               6 plates
│   └── media/                  2 videos (+ posters, captions), 3 audio clips (+ captions)
├── minimal/book.darml          the smallest valid book (§1)
├── snippets/*.darml            9 single-file books, one feature each
│   └── assets/                 tiny media used by the snippets
└── tools/                      generators and the lint script
```

`darm.toml` project files are not part of this folder's sources; every book can
be checked and built from its entry file:

```sh
darm check examples/atlas-de-mareas/book.darml
darm build examples/atlas-de-mareas/book.darml
darm build examples/the-lighthouse-protocol/book.darml --encrypt --password-env DARM_PASSWORD
python3 examples/tools/lint_examples.py --stats                    # mechanical checks, no compiler needed
```

All `.darml` files are formatted with `darm fmt` (`darm fmt --check examples` passes).

## Book 1 — *Atlas de mareas* (`atlas-de-mareas/`)

A literary essay-fable in Spanish by the fictional Brígida Almenar: the tides
of an invented estuary, the grandfather who kept its tide gauge, and an old
fable about a cartographer. About 6 400 words of prose in ten chapters.

| Feature | Where | § |
|---|---|---|
| Multi-file project with `include` | `book.darml` → `chapters/*.darml` (10 includes) | 4 |
| Full `book` metadata, two `credit` lines | `book.darml` | 4.1 |
| `theme` with dark overrides and `drop_caps = true` | `book.darml` | 4.2 |
| `cover:` (classic, inset image) and `back_cover:` (tagline + two `text`) | `book.darml` | 13 |
| `numbered=false` preface and closing note | `00-prefacio`, `09-nota-de-cierre` | 5 |
| Two-level contents (`section`) | chapters 1–5, 7, 8 and the closing note (18 sections) | 5 |
| `text` styles `lead`, `small`, `verse` (text block + trailing arguments) | every chapter; verse in `06-fabula-del-cartografo` | 5, 6 |
| `quote … by=` | `04-el-oficio-de-la-bajamar` | 5 |
| `list` unordered and `ordered=true` | `00`, `02`, `04`, `05`, `08`, `09` | 5 |
| `box` styles `rule`, `tint`, `bar` (with `title=`) | `00` (rule), `02` and `08` (tint), component (bar) | 5 |
| `divider` (ornament and `style=rule`), `space`, `break`, `heading` | `01`, `02`, `06`, `09` | 5 |
| `image` with `id=`, `caption=`, `width=` | 9 single plates (60 %–100 %) | 5, 7 |
| `gallery` | `03-instrumentos` (two plates) | 5 |
| Internal links to chapters, sections and figures | 12 links, e.g. `[…](lamina_mapa)`, `[…](cero)`, `[…](glosario)` | 6 |
| Reserved targets `contents`, `cover` | last line of `09-nota-de-cierre` | 6 |
| External `https` link | `08-mareas-que-vendran` (`https://psmsl.org`) | 6 |
| Inline `*italic*`, `**bold**`, rich-text captions | throughout | 6 |
| Reusable `component nota_marginal(cuerpo, rotulo="Nota al margen")` | defined in `book.darml`, used 7 times | 12 |
| Search material | recurring *mareógrafo, pleamar, bajamar, estoa, ardora, salicornia, escandallo, sizigia*; diacritics á é í ó ú ñ ü (*Cigüeñal, cigüeñuela, lengüeta, ambigüedad*), ¿ ¡ and raya dialogue | — |

Chapter ids: `prefacio`, `costa`, `luna`, `instrumentos`, `bajamar`, `botanica`,
`fabula`, `ardora`, `venideras`, `epilogo`. Figure ids: `lamina_mapa`,
`lamina_caseta`, `lamina_sizigia`, `lamina_curva`, `lamina_mareografo`,
`lamina_rastrillo`, `lamina_zonacion`, `lamina_salicornia`, `lamina_ardora`.

## Book 2 — *The Lighthouse Protocol* (`the-lighthouse-protocol/`)

A branching story in English by the fictional Edda Hallowick: one night as the
relief keeper of a rock lighthouse, four endings. About 3 650 words across all
branches. It is meant to be built both plain and encrypted.

| Feature | Where | § |
|---|---|---|
| Reflow book with fixed `page` plates | `title_plate`, `chart_plate`, `daybreak` | 10 |
| Positioned children `at=` / `size=`, `fit=cover`, `background=` | all three plates | 10 |
| `if`/`elif`/`else` inside a `page` | `chart_plate`, `daybreak` | 9, 10 |
| `state:` int with range, two bools, two closed-set texts | `book.darml` | 8 |
| `choice` / `option` with actions and `goto` | `first_duty`, `the_signal` | 9 |
| `option … when=` | third option of `the_signal` | 9 |
| Chapters gated with `when=` | `lamp_room`, `bell_house`, `the_answer`, `the_log`, four endings | 9 |
| `if`/`elif`/`else` in flowing text | `lamp_room`, `fog`, `the_answer`, `the_log`, every ending | 9 |
| `{oil}` interpolation | gauges in `lamp_room`, `bell_house`, `fog`, `the_answer`, `the_log`, endings, `daybreak` | 6, 8 |
| Actions `=`, `+=`, `-=`, `goto`, `reset` | buttons and options | 8 |
| `quiz` with `explain` and `on_correct` | `flame_quiz` in `fog` | 9 |
| `button` shorthand `goto=` and with a body | throughout | 8 |
| `goto …, transition=fade` | title plate, third signal option, every restart | 11 |
| Entrance animations `fade`, `rise`, `zoom`, `slide_left` with `delay=` / `duration=` | plates, images, video, ending lines | 11 |
| `video` with poster, captions, transcript, `loop=true` | `beam_film` (in `arrival`), `signal_film` (in `fog`) | 5, 7 |
| `audio` with title, transcript, captions | `sea_sound`, `bell_sound`, `horn_sound` | 5, 7 |
| `box`, ordered `list`, `quote`, `section`, internal link to a block id | `arrival` (`protocol`), `fog` (`three_long`) | 5, 6 |
| `cover:` (modern, full-bleed image) and `back_cover:` | `book.darml` | 13 |
| `theme` with dark overrides, `paragraph = spaced`, sans headings | `book.darml` | 4.2 |
| Multi-file project | `book.darml` → `parts/watch.darml`, `parts/mornings.darml` | 4 |

### Route map

State (declared in `book.darml`):

| Variable | Type | Initial | Meaning |
|---|---|---|---|
| `oil` | int `0..5` | `2` | measures of oil in the lamp; a night needs 4 |
| `half_flame` | bool | `false` | set by answering the quiz correctly |
| `answered` | bool | `false` | the keeper answered the signal |
| `duty` | text `"lamp"`/`"bell"` | `"lamp"` | duty seen to before dark |
| `route` | text `"tower"`/`"landing"` | `"tower"` | where the keeper stands the rest of the watch |

Chapters in source order. A gated chapter is skipped by page turning while its
condition is false; exactly one chapter of each alternative group is visible.

| # | Chapter id | Title | `when=` |
|---|---|---|---|
| 1 | `title_plate` | Title plate (fixed page, `toc=false`) | — |
| 2 | `reader_orders` | Standing Orders for the Reader | — |
| 3 | `arrival` | The Relief | — |
| 4 | `lamp_room` | The Lamp Room | `duty == "lamp"` |
| 5 | `bell_house` | The Bell House | `duty == "bell"` |
| 6 | `fog` | Fog (section `three_long`) | — |
| 7 | `the_answer` | The Answer | `answered` |
| 8 | `the_log` | The Log | `not answered` |
| 9 | `chart_plate` | The Chart of the Rock (fixed page) | — |
| 10 | `ending_landing` | The Landing | `route == "landing"` |
| 11 | `ending_answer` | The Answering Light | `route == "tower" and answered` |
| 12 | `ending_kept` | The Kept Light | `route == "tower" and not answered and (oil >= 4 or duty == "bell")` |
| 13 | `ending_dark` | The Dark Hour | `route == "tower" and not answered and oil < 4 and duty == "lamp"` |
| 14 | `daybreak` | Daybreak (fixed page) | — |

Interactive elements, in reading order (labels are exact):

| In | Element | Label | Actions |
|---|---|---|---|
| `title_plate` | button | `Begin the watch` | `goto reader_orders, transition=fade` |
| `reader_orders` | button | `Land at Skerry Vane` | `goto arrival` |
| `arrival` | choice `first_duty` — “What do you see to first?” | `Climb to the lamp room and see to the oil` | `duty = "lamp"` · `goto lamp_room` |
| | | `Go down to the bell house and wind the bell` | `duty = "bell"` · `goto bell_house` |
| `lamp_room` | button | `Work the pump` | `oil += 1` (clamped at 5; may be pressed repeatedly) |
| `lamp_room` | button | `Keep the watch` | `goto fog` |
| `bell_house` | button | `Keep the watch` | `goto fog` |
| `fog` | quiz `flame_quiz` — “The fog has closed in. How should the flame be set?” | `Full, to push as much light as possible through the fog` | wrong |
| | | `Half, as the Protocol orders in fog` | **correct** → `half_flame = true` · `oil += 1` (once) |
| | | `Out, to save the oil for a clear hour` | wrong |
| `fog` | choice `the_signal` — “The signal is waiting. What do you do?” | `Log it, and keep the character` | `answered = false` · `route = "tower"` · `goto the_log` |
| | | `Take the lanyard and answer: three long` | `answered = true` · `route = "tower"` · `oil -= 1` · `goto the_answer` |
| | | `Leave the lamp and go down the chain to the landing` (only `when=duty == "bell"`) | `answered = false` · `route = "landing"` · `goto ending_landing, transition=fade` |
| `the_answer` | button | `Look at the chart` | `goto chart_plate` |
| `the_log` | button | `Look at the chart` | `goto chart_plate` |
| `chart_plate` | button at (50, 32), size (42, 8) | `Stay with the lamp` | `route = "tower"` · `goto ending_answer` if `answered`; else `goto ending_kept` if `oil >= 4 or duty == "bell"`; else `goto ending_dark` |
| `chart_plate` | button at (50, 64), size (42, 8) | `Go down to the landing` | `route = "landing"` · `goto ending_landing` |
| each ending | button | `Wind the night back` | `reset` · `goto start, transition=fade` |
| `daybreak` | button | `Stand the watch again` | `reset` · `goto start, transition=fade` |

`oil` arithmetic: starts at 2; each `Work the pump` +1 (lamp route only); the
correct quiz answer +1, once; answering the signal −1; always clamped to 0..5.
`start` is the title plate (the first page after the front matter).

Endings and the shortest click sequences that reach them:

| Ending | Clicks | Final state |
|---|---|---|
| `ending_kept` (lamp) | `first_duty` #1 → `Work the pump` ×2 → `Keep the watch` → `the_signal` #1 → `Look at the chart` → `Stay with the lamp` | `oil 4`, `duty lamp`, `answered false`, `route tower` |
| `ending_kept` (lamp, with quiz) | `first_duty` #1 → `Work the pump` ×1 → `Keep the watch` → quiz answer #2 → `the_signal` #1 → `Look at the chart` → `Stay with the lamp` | `oil 4`, `half_flame true` |
| `ending_kept` (bell) | `first_duty` #2 → `Keep the watch` → `the_signal` #1 → `Look at the chart` → `Stay with the lamp` | `oil 2`, `duty bell` |
| `ending_dark` | `first_duty` #1 → `Keep the watch` → `the_signal` #1 → `Look at the chart` → `Stay with the lamp` | `oil 2`, `duty lamp` |
| `ending_answer` | `first_duty` #1 → `Keep the watch` → `the_signal` #2 → `Look at the chart` → `Stay with the lamp` | `oil 1`, `answered true` |
| `ending_landing` (chart) | any route to `chart_plate` → `Go down to the landing` | `route landing` |
| `ending_landing` (direct) | `first_duty` #2 → `Keep the watch` → `the_signal` #3 | `duty bell`, `route landing`, `answered false` |

Turning pages without ever choosing follows the defaults: `title_plate` →
`reader_orders` → `arrival` → `lamp_room` → `fog` → `the_log` → `chart_plate` →
`ending_dark` → `daybreak` → back cover. After any ending the next page is
`daybreak`, then the back cover; no path dead-ends.

Conditional prose worth asserting (opening words of each variant):

| Chapter | Condition | Text begins |
|---|---|---|
| `lamp_room` | `oil >= 4` / `oil == 3` / else | “The flame steadies and whitens.” / “Better. But the card” / “The gauge is barely off its stop.” |
| `fog` | `duty == "bell"` / else | “Below you, faithful as a pulse” / “Below you there is nothing.” |
| `fog` | `half_flame` / else | “You turn the flame down to half.” / “The gauge stands at {oil} of 5.” |
| `the_answer` | `duty == "bell"` / else | “At least there is the bell.” / “And there is no bell” |
| `the_log` | `oil >= 4` / else | “You look at the gauge, {oil} of 5, and for the first time” / “You look at the gauge: {oil} of 5.” |
| `ending_landing` | `answered` / else | “You tell him what you did with the shutter.” / “‘Nobody,’ you say.” |
| `ending_landing` | `oil >= 4` / else | “…the lamp is burning as you left it” / “…the lantern is dark.” |
| `ending_answer` | `duty == "bell"` / else | “‘It was the bell that saved us,’” / “‘If there’d been a bell,’” |
| `ending_kept` | `oil >= 4` / else | “The oil holds.” … “‘Every twelve seconds” / “A little before five the flame sinks” … “‘Till near five.” |
| `daybreak` | `route == "landing"` / `answered` / `oil >= 4 or duty == "bell"` / else | “You left the lamp and went down to the landing.” / “You answered the signal,” / “You logged the signal and kept the character until morning.” / “You kept the character until the oil ran out.” |

## Minimal book and snippets

`minimal/book.darml` is the book from §1 of the reference, verbatim.

Each snippet is a complete single-file book of at most 40 lines, commented, and
meant to be copied. They have no `cover:` blocks (except `08`), so the compiler
reports the documented warnings `W0601`/`W0602` for them and nothing else.

| File | Demonstrates | § |
|---|---|---|
| `01-structure.darml` | chapters, sections, `heading`, text styles, `quote`, lists, `box`, `divider`, `space`, `break` | 4, 5 |
| `02-rich-text.darml` | inline markup, markup escapes, string escapes, text blocks, internal/external links, reserved targets | 2, 6 |
| `03-media.darml` | `assets:` with `image`, `video` (poster, captions, transcript), `audio`; `image`, `gallery`, `audio`, `video` | 7 |
| `04-state-and-choice.darml` | `state:`, `button` actions, `choice`/`option`, `when=`, `if`/`elif`/`else`, `{var}` | 8, 9 |
| `05-quiz.darml` | `quiz`, `answer`, `explain`, `on_correct`, `reset` | 9 |
| `06-fixed-page.darml` | `layout = fixed`, `page_size`, `page` with `at=`/`size=`, `fit=cover` | 10 |
| `07-components.darml` | `component` with defaults, positional and named arguments, `{param}` | 12 |
| `08-covers-and-theme.darml` | full metadata, author list, `front_matter`, `theme`, `cover:`, `back_cover:` | 4.1, 4.2, 13 |
| `09-animations-and-jumps.darml` | `animate=`/`delay=`/`duration=`, `transition=`, `reset name`, `goto start` | 8, 11 |

## How the assets were made

Nothing was downloaded, traced or sampled. Every asset is produced by a script
in `tools/`, and each script is deterministic (fixed seeds; two runs give
identical bytes on the same machine and tool versions).

| Script | Needs | Produces |
|---|---|---|
| `tools/make_atlas_art.py` | Python 3 standard library | the 12 plates in `atlas-de-mareas/art/`: hand-inked strokes baked into path data, flat washes, a four-colour palette (ink `#26302f`, blue-grey `#7d99a3`, sand `#dcc9a2`, terracotta `#a5482e`) on a paper plate `#efe7d6` |
| `tools/make_lighthouse_art.py` | Python 3 standard library | the 6 plates in `the-lighthouse-protocol/art/`: nocturne line-and-wash (navy `#0e1a26`, fog `#8fa3ad`, amber `#f2b45a`, bone `#e9e4d6`); `chart.svg` keeps empty frames where the book places its buttons |
| `tools/make_lighthouse_media.py` | Python 3 with numpy, ffmpeg (libx264, aac) | `media/`: frames are rendered procedurally with numpy and piped to ffmpeg; audio is synthesised (filtered-noise sea with swell envelopes, a bell from decaying inharmonic partials, a reed horn from rolled-off harmonics), loudness-normalised to about −19 LUFS and encoded as AAC; posters and WebVTT captions are written by the same script |
| `tools/make_snippet_assets.sh` | ffmpeg | `snippets/assets/beam.mp4`, `beam.jpg`, `beam.en.vtt`, `bell.m4a`: short low-resolution excerpts of the lighthouse media (`tide.svg` and `moon.svg` there are written by hand) |

All SVG files are static and self-contained: no scripts, event handlers,
`foreignObject`, raster images, external references or `@import`; text uses
the generic `serif` family only. The only filter used is a mild
`feGaussianBlur` on some washes and beams.

Media inventory (from `ffprobe`; all MP4/M4A files have `moov` before `mdat`):

| File | Format | Duration | Size |
|---|---|---|---|
| `media/beam.mp4` | H.264 High, yuv420p, 960×540, 24 fps + AAC-LC 48 kHz stereo | 12.0 s (loops) | 1 079 995 B |
| `media/signal.mp4` | H.264 High, yuv420p, 960×540, 24 fps + AAC-LC 48 kHz stereo | 14.0 s | 1 190 974 B |
| `media/sea.m4a` | AAC-LC 48 kHz stereo | 30.0 s | 488 007 B |
| `media/bell.m4a` | AAC-LC 48 kHz stereo | 24.0 s | 392 397 B |
| `media/horn.m4a` | AAC-LC 48 kHz stereo | 18.0 s | 292 144 B |
| `media/beam.jpg`, `media/signal.jpg` | JPEG 960×540 posters | — | 20 738 B, 16 322 B |
| `media/beam.en.vtt`, `signal.en.vtt`, `horn.en.vtt` | WebVTT, 4 / 9 / 9 cues | — | — |

## Lint

`tools/lint_examples.py` (standard library only) checks what can be checked
without the compiler: the `darm 1` pragma, spaces-only indentation in steps of
four, includes, asset paths and content types, required `alt` texts, static
well-formed SVG with a `viewBox`, WebVTT cues within the media duration, unique
ids, every `goto`/link target, declared assets and state variables, balanced
inline markup and the documented limits. With `ffprobe` on the PATH it also
verifies codecs and size budgets; with `--darm path/to/darm` it runs
`darm check` on every project as well.

```sh
python3 examples/tools/lint_examples.py                 # everything under examples/
python3 examples/tools/lint_examples.py --stats         # plus word counts and media facts
python3 examples/tools/lint_examples.py --darm target/debug/darm
```
