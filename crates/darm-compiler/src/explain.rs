//! Catalogue of diagnostic codes (`darm explain CODE`, `docs/errors.md`).
//!
//! Every code the toolchain can emit is listed here; a test checks that the
//! compiler never emits a code that is missing from this table.

pub struct Entry {
    pub code: &'static str,
    pub title: &'static str,
    pub explanation: &'static str,
    /// A minimal example that triggers the diagnostic, and its fix.
    pub bad: &'static str,
    pub good: &'static str,
}

const fn e(
    code: &'static str,
    title: &'static str,
    explanation: &'static str,
    bad: &'static str,
    good: &'static str,
) -> Entry {
    Entry {
        code,
        title,
        explanation,
        bad,
        good,
    }
}

pub const CATALOGUE: &[Entry] = &[
    e(
        "E0101",
        "unexpected character",
        "The character is not part of the language. DARM has no braces, semicolons, single-quoted strings or division.",
        "text 'hello'",
        "text \"hello\"",
    ),
    e(
        "E0102",
        "tab in indentation",
        "Indentation must use spaces only, so that the block structure is the same in every editor.",
        "chapter \"A\":\n\ttext \"x\"",
        "chapter \"A\":\n    text \"x\"",
    ),
    e(
        "E0103",
        "inconsistent indentation",
        "A line is indented less than its block but does not line up with any enclosing block.",
        "chapter \"A\":\n        text \"x\"\n    text \"y\"",
        "chapter \"A\":\n    text \"x\"\n    text \"y\"",
    ),
    e(
        "E0104",
        "unterminated string",
        "A string must be closed on the line where it starts. Use a \"\"\" block for text that spans lines.",
        "text \"hello",
        "text \"hello\"",
    ),
    e(
        "E0105",
        "invalid escape",
        "Only \\\" \\\\ \\n \\t \\u{...} and the markup escapes \\* \\[ \\] \\{ \\} \\` are recognised.",
        "text \"C:\\dir\"",
        "text \"C:\\\\dir\"",
    ),
    e(
        "E0106",
        "invalid number",
        "Numbers may carry the suffixes % (percent), ms and s (durations); nothing else.",
        "space 3px",
        "space 3",
    ),
    e(
        "E0107",
        "not UTF-8",
        "Source files must be UTF-8 encoded text.",
        "(a Latin-1 encoded file)",
        "(the same file saved as UTF-8)",
    ),
    e(
        "E0108",
        "unclosed bracket",
        "A `(` or `[` has no matching `)` or `]`.",
        "x = image(\"a.png\"",
        "x = image(\"a.png\")",
    ),
    e(
        "E0201",
        "missing version line",
        "Every .darml file starts with `darm 1`, the grammar version it is written in.",
        "book \"T\":",
        "darm 1\n\nbook \"T\":",
    ),
    e(
        "E0202",
        "unsupported grammar version",
        "The file asks for a grammar version this compiler does not implement.",
        "darm 7",
        "darm 1",
    ),
    e(
        "E0203",
        "syntax error",
        "The parser found a token it did not expect. The message names what was expected. Common causes: a missing comma between arguments, `=` where `==` was meant, `True` instead of `true`.",
        "text \"a\" style=lead",
        "text \"a\", style=lead",
    ),
    e(
        "E0204",
        "missing block",
        "A line ending in `:` must be followed by an indented block.",
        "chapter \"A\":\ntext \"x\"",
        "chapter \"A\":\n    text \"x\"",
    ),
    e(
        "E0205",
        "unexpected indentation",
        "A line is indented deeper than the previous one, but the previous line does not end with `:`.",
        "text \"a\"\n    text \"b\"",
        "text \"a\"\ntext \"b\"",
    ),
    e(
        "E0206",
        "`elif`/`else` without `if`",
        "`elif` and `else` must directly follow an `if` block at the same indentation.",
        "else:\n    text \"x\"",
        "if ready:\n    text \"y\"\nelse:\n    text \"x\"",
    ),
    e(
        "E0207",
        "positional argument after named argument",
        "Positional arguments come first.",
        "image width=50%, chart",
        "image chart, width=50%",
    ),
    e(
        "E0208",
        "block on the same line",
        "The contents of a block go on the following lines, indented.",
        "chapter \"A\": text \"x\"",
        "chapter \"A\":\n    text \"x\"",
    ),
    e(
        "E0301",
        "unknown element",
        "The name is neither a built-in element nor a component defined in the project.",
        "paragraph \"x\"",
        "text \"x\"",
    ),
    e(
        "E0302",
        "element not allowed here",
        "Each element has a place: top-level statements (`book`, `chapter`, …) cannot be nested, content blocks need a `chapter`, actions need a `button`/`option`/`on_correct` body, and so on.",
        "text \"x\"   # at top level",
        "chapter \"A\":\n    text \"x\"",
    ),
    e(
        "E0303",
        "unknown argument or property",
        "The element or block does not take an argument with this name. `darm reference` lists what each one accepts.",
        "text \"x\", colour=\"#333333\"",
        "text \"x\", color=\"#333333\"",
    ),
    e(
        "E0304",
        "unknown asset",
        "The name is not declared in an `assets:` block.",
        "image chart",
        "assets:\n    chart = image(\"chart.png\", alt=\"A chart\")",
    ),
    e(
        "E0305",
        "unknown jump target",
        "A `goto`, link or `goto=` argument names an id that no chapter, section or block declares.",
        "button \"Go\", goto=nowhere",
        "chapter \"Somewhere\", id=somewhere:\n    …",
    ),
    e(
        "E0306",
        "duplicate definition",
        "Ids, asset names, state variables, component names, properties and named arguments must be unique.",
        "chapter \"A\", id=x:\n    …\nchapter \"B\", id=x:",
        "chapter \"B\", id=y:",
    ),
    e(
        "E0307",
        "wrong type of value",
        "The argument needs a different kind of value (for example a string where a number was given).",
        "space \"two\"",
        "space 2",
    ),
    e(
        "E0308",
        "missing required argument or property",
        "A required argument, property or child is absent.",
        "book \"T\":\n    author = \"A\"",
        "book \"T\":\n    author = \"A\"\n    language = \"en\"",
    ),
    e(
        "E0309",
        "invalid component use",
        "A component was called with the wrong arguments, calls itself, or nests too deeply.",
        "component a():\n    a",
        "component a():\n    text \"x\"",
    ),
    e(
        "E0310",
        "unknown state variable",
        "The name is not declared in a `state:` block.",
        "if brave:",
        "state:\n    brave = false",
    ),
    e(
        "E0311",
        "type error in expression",
        "Arithmetic and ordering need integers, `and`/`or`/`not` need booleans, and `==`/`!=` need two values of the same type.",
        "if courage and 1:",
        "if courage >= 1:",
    ),
    e(
        "E0312",
        "invalid value",
        "The value has the right type but is out of range, not one of the allowed words, or malformed (colours are \"#rrggbb\").",
        "theme:\n    paper = \"beige\"",
        "theme:\n    paper = \"#f5f0e6\"",
    ),
    e(
        "E0313",
        "invalid structure",
        "The project is missing something structural (no `book`, no chapters) or an element has a block it cannot take / lacks one it needs.",
        "(a project with no chapter)",
        "chapter \"One\":\n    text \"…\"",
    ),
    e(
        "E0314",
        "invalid rich text",
        "Inline markup is unbalanced or a link target is malformed. Escape literal markup characters with a backslash.",
        "text \"2 *3 = 6\"",
        "text \"2 \\*3 = 6\"",
    ),
    e(
        "E0315",
        "limit exceeded",
        "The source exceeds one of the documented limits (nesting depth, number of options, …).",
        "(blocks nested 60 levels deep)",
        "(flatter structure)",
    ),
    e(
        "E0316",
        "fixed-page rule violated",
        "Blocks inside `page` need `at=` and `size=` inside the page; outside a `page` they must not have them; `page` must be a direct child of `chapter`; a `fixed` book contains only pages.",
        "page:\n    text \"x\"",
        "page:\n    text \"x\", at=(10, 10), size=(80, 20)",
    ),
    e(
        "E0317",
        "reserved name",
        "`cover`, `start`, `contents`, `end` and names starting with `_` are reserved; built-in element names cannot be reused for components.",
        "chapter \"A\", id=start:",
        "chapter \"A\", id=opening:",
    ),
    e(
        "E0401",
        "file not found",
        "A declared asset (or cover image, poster, captions file) does not exist at that path. Paths are relative to the source file that declares them.",
        "pic = image(\"art/missing.png\", alt=\"…\")",
        "(add the file, or fix the path)",
    ),
    e(
        "E0402",
        "path outside the project",
        "Asset and include paths must be relative and stay inside the project directory.",
        "pic = image(\"../../secret.png\", alt=\"…\")",
        "pic = image(\"art/pic.png\", alt=\"…\")",
    ),
    e(
        "E0403",
        "unsupported or mismatched file format",
        "Files are identified by content. The file is not one of the formats the constructor accepts, or it is damaged.",
        "pic = image(\"clip.mp4\", alt=\"…\")",
        "clip = video(\"clip.mp4\", alt=\"…\")",
    ),
    e(
        "E0404",
        "unreadable or oversized file",
        "The file could not be read, or exceeds the size limit.",
        "(an 9 GiB video)",
        "(a smaller encode)",
    ),
    e(
        "E0405",
        "unsafe SVG",
        "SVG images must be static and self-contained: no scripts, event handlers, animation elements, `foreignObject` or external references.",
        "<svg onload=\"…\">",
        "<svg>",
    ),
    e(
        "E0406",
        "remote asset not permitted",
        "Remote assets need `offline = false`, an https URL and the URL's host in the book's `network` list.",
        "clip = video(url=\"https://cdn.example.org/a.mp4\", alt=\"…\")",
        "book \"T\":\n    offline = false\n    network = [\"cdn.example.org\"]",
    ),
    e(
        "E0407",
        "invalid captions",
        "Caption files must be WebVTT: UTF-8, first line `WEBVTT`, at least one cue.",
        "(an .srt file)",
        "(the same cues as .vtt)",
    ),
    e(
        "E0501",
        "include failed",
        "The included file does not exist, is included twice, forms a cycle, or includes nest deeper than 16.",
        "include \"chapters/missing.darml\"",
        "include \"chapters/one.darml\"",
    ),
    e(
        "E0502",
        "invalid darm.toml",
        "The project file is malformed or names an entry file that does not exist.",
        "[book]\nentry = 3",
        "[book]\nentry = \"book.darml\"",
    ),
    e(
        "E0503",
        "cannot write output",
        "The output file or directory could not be written.",
        "darm build -o /nonexistent/dir/x.darm",
        "darm build -o dist/x.darm",
    ),
    e(
        "E0504",
        "invalid protection options",
        "The encryption options are incomplete or contradictory (for example `--encrypt` without a password source or key file).",
        "darm build --encrypt",
        "darm build --encrypt --password-env BOOK_PASSWORD",
    ),
    e(
        "E0599",
        "internal compiler error",
        "The compiler produced an IR that failed its own validation. This is a bug in DARM; please report it with the source that triggers it.",
        "",
        "",
    ),
    e(
        "W0601",
        "generated front cover",
        "The project has no `cover:` block, so a deterministic typographic cover was generated from the title, author and book id.",
        "(no cover block)",
        "cover:\n    style = classic\n    color = \"#7a2e22\"",
    ),
    e(
        "W0602",
        "generated back cover",
        "The project has no `back_cover:` block, so one was generated from the description.",
        "(no back_cover block)",
        "back_cover:\n    text \"…\"",
    ),
    e(
        "W0603",
        "unused asset",
        "The asset is declared and will be packaged, but nothing refers to it.",
        "",
        "",
    ),
    e(
        "W0604",
        "unused definition",
        "A state variable is never read, or a component is never used.",
        "",
        "",
    ),
    e(
        "W0605",
        "empty chapter",
        "The chapter has no content.",
        "",
        "",
    ),
    e(
        "W0606",
        "large asset",
        "The asset is larger than 64 MiB; consider a smaller encode for comfortable downloads and imports.",
        "",
        "",
    ),
];

pub fn lookup(code: &str) -> Option<&'static Entry> {
    CATALOGUE.iter().find(|e| e.code.eq_ignore_ascii_case(code))
}

/// Long-form text for `darm explain`.
pub fn render(entry: &Entry) -> String {
    let mut s = format!("{}: {}\n\n{}\n", entry.code, entry.title, entry.explanation);
    if !entry.bad.is_empty() {
        s.push_str("\nProblem:\n");
        for l in entry.bad.lines() {
            s.push_str("    ");
            s.push_str(l);
            s.push('\n');
        }
    }
    if !entry.good.is_empty() {
        s.push_str("\nFix:\n");
        for l in entry.good.lines() {
            s.push_str("    ");
            s.push_str(l);
            s.push('\n');
        }
    }
    s
}

/// `docs/errors.md`, generated so the document can never drift from the tool.
pub fn markdown() -> String {
    let mut s = String::from(
        "# DARM diagnostics\n\n<!-- Generated by `darm reference --errors-md`. Do not edit by hand. -->\n\n\
         Every diagnostic has a stable code. `E` codes fail the build; `W` codes are warnings\n\
         (they fail the build only with `--deny-warnings`). `darm explain CODE` prints the same text.\n\n\
         | Range | Area |\n|---|---|\n| `E01xx` | lexical |\n| `E02xx` | syntax |\n| `E03xx` | names, types, structure |\n\
         | `E04xx` | assets and files |\n| `E05xx` | project and packaging |\n| `W06xx` | warnings |\n\n",
    );
    for e in CATALOGUE {
        s.push_str(&format!(
            "## {} — {}\n\n{}\n\n",
            e.code, e.title, e.explanation
        ));
        if !e.bad.is_empty() {
            s.push_str(&format!("Problem:\n\n```darml\n{}\n```\n\n", e.bad));
        }
        if !e.good.is_empty() {
            s.push_str(&format!("Fix:\n\n```darml\n{}\n```\n\n", e.good));
        }
    }
    s
}
