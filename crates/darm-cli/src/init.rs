//! `darm init`: create a new project from a template.

use std::path::Path;

use clap::ValueEnum;

use crate::{CmdResult, Failure};

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Template {
    /// A reflowable book with chapters, a figure and a designed cover.
    Essay,
    /// A short branching story with state, choices and a quiz.
    Interactive,
}

const ORNAMENT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" role="img">
  <rect width="800" height="500" fill="#f3ecdc"/>
  <g fill="none" stroke="#7a2e22" stroke-width="3" stroke-linecap="round">
    <path d="M60 380 C 180 300, 260 420, 400 340 S 620 300, 740 370"/>
    <path d="M60 410 C 200 340, 280 450, 400 380 S 600 340, 740 400" opacity=".6"/>
    <path d="M60 440 C 220 380, 300 470, 400 420 S 580 380, 740 430" opacity=".35"/>
  </g>
  <circle cx="590" cy="150" r="58" fill="#d9b46a"/>
  <path d="M110 330 L250 150 L340 250 L420 190 L560 330 Z" fill="#7a2e22" opacity=".85"/>
</svg>
"##;

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn essay(title: &str, author: &str, language: &str) -> String {
    format!(
        r##"darm 1

# A DARM book is a `book` block, optional design blocks, and chapters.
# Validate with `darm check`, format with `darm fmt`, compile with `darm build`.

book "{title}":
    author = "{author}"
    language = "{language}"
    edition = "First edition"
    description = "A short description. It becomes the back-cover blurb."

theme:
    paper = "#f7f3ea"
    ink = "#1f1c18"
    accent = "#7a2e22"
    drop_caps = true

assets:
    landscape = image("art/landscape.svg", alt="A red hill, a low sun and three lines of water")

cover:
    style = classic
    color = "#7a2e22"
    ink = "#f4ead5"
    accent = "#d9b46a"

back_cover:
    tagline = "Every book starts somewhere."
    text "Replace this blurb with a few sentences about your book. *Rich text* works here too."

chapter "A beginning", id=beginning:
    text "This is the first paragraph of your book. It gets a drop capital because the theme says so."
    text """
        Longer passages are easier to write in a text block.
        Line breaks inside the block are just spaces.

        A blank line starts a new paragraph. You can use *italic*, **bold**,
        and links to other places in the book, like [the second chapter](second).
        """
    image landscape, caption="Fig. 1. An illustration from `art/landscape.svg`.", width=80%

chapter "A second chapter", id=second:
    section "Sections appear in the contents":
        text "Sections group related passages and give the table of contents a second level."
        quote "A coast is a sentence the sea keeps rewriting.", by="Harbour proverb"
    section "Lists and breaks":
        list:
            item "Lists are written with `item` lines."
            item "`divider` draws an ornament, `break` starts a new page."
        divider
        text "Run `darm reference` to see every element and its arguments."
"##,
        title = escape(title),
        author = escape(author),
        language = escape(language),
    )
}

fn interactive(title: &str, author: &str, language: &str) -> String {
    format!(
        r##"darm 1

# A small branching story. State lives in `state:`, changes only through
# actions inside `button` and `option` blocks, and is read by `if` and `when=`.

book "{title}":
    author = "{author}"
    language = "{language}"
    description = "A story that changes with the reader's choices."

assets:
    landscape = image("art/landscape.svg", alt="A red hill, a low sun and three lines of water")

state:
    courage = 0 in 0..3
    path = "none" in ["none", "hill", "shore"]

cover:
    style = modern
    color = "#1f3a4d"
    ink = "#f1ead9"
    accent = "#c9a45c"

back_cover:
    color = "#1f3a4d"
    ink = "#f1ead9"
    accent = "#c9a45c"
    tagline = "Two roads, one evening."
    text "A short story with two paths and more than one ending."

# A component is a reusable piece of content. It is expanded at compile time:
# `{{line}}` is replaced by the argument each time it is used.
component aside(line):
    text "{{line}}", style=small

chapter "The fork", id=fork, numbered=false:
    image landscape, width=70%, animate=fade, duration=900ms
    text "The road ends where the hill meets the water. You have to choose."
    aside "The hill is steep. The shore is long."
    choice "Where do you go?", id=which_way:
        option "Climb the hill":
            path = "hill"
            courage += 1
            goto hill
        option "Follow the shore":
            path = "shore"
            goto shore

chapter "The hill", id=hill, when=path == "hill", numbered=false:
    text "From the top the whole coast is a map. Courage: {{courage}}."
    quiz "Which way does the evening sun set?":
        answer "West", correct=true
        answer "East"
        explain "The sun sets in the west everywhere on Earth."
        on_correct:
            courage += 1
    button "Go on", goto=ending

chapter "The shore", id=shore, when=path == "shore", numbered=false:
    text "The shingle is loud under your boots and the tide is coming in."
    aside "Nobody else has walked here today."
    button "Go on", goto=ending

chapter "An ending", id=ending, numbered=false:
    if path == "hill" and courage >= 2:
        text "You come down the hill at dusk, certain of the way home."
    elif path == "hill":
        text "You come down the hill in the dark."
    elif path == "shore":
        text "You walk the tide line until the lights of the harbour appear."
    else:
        text "You never left the fork. Turn back and choose."
    button "Start again":
        reset
        goto fork
"##,
        title = escape(title),
        author = escape(author),
        language = escape(language),
    )
}

fn title_from(dir: &Path) -> String {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled");
    let words: Vec<String> = name
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect();
    if words.is_empty() {
        "Untitled".into()
    } else {
        words.join(" ")
    }
}

pub fn run(
    dir: &Path,
    title: Option<String>,
    author: &str,
    language: &str,
    template: Template,
) -> CmdResult {
    let io = |what: &str, e: std::io::Error| Failure::env(format!("cannot {what}: {e}"));
    if dir.exists() {
        let occupied = std::fs::read_dir(dir)
            .map_err(|e| io("read the directory", e))?
            .next()
            .is_some();
        if occupied {
            return Err(Failure::env(format!(
                "`{}` already exists and is not empty",
                dir.display()
            )));
        }
    }
    let title = title.unwrap_or_else(|| title_from(dir));
    let source = match template {
        Template::Essay => essay(&title, author, language),
        Template::Interactive => interactive(&title, author, language),
    };
    std::fs::create_dir_all(dir.join("art")).map_err(|e| io("create the project", e))?;
    std::fs::write(dir.join("book.darml"), source).map_err(|e| io("write book.darml", e))?;
    std::fs::write(dir.join("art/landscape.svg"), ORNAMENT_SVG)
        .map_err(|e| io("write art/landscape.svg", e))?;
    std::fs::write(
        dir.join("darm.toml"),
        "# DARM project file. Every key is optional.\n\n[book]\nentry = \"book.darml\"\noutput = \"{slug}.darm\"\n\n\
         # To protect the book, uncomment and run `BOOK_PASSWORD=... darm build`:\n\
         # [protect]\n# mode = \"password\"\n# password_env = \"BOOK_PASSWORD\"\n",
    )
    .map_err(|e| io("write darm.toml", e))?;
    std::fs::write(dir.join(".gitignore"), "*.darm\n*.darmkey\n.darm-cache/\n")
        .map_err(|e| io("write .gitignore", e))?;
    let d = dir.display();
    println!("created {d}/");
    println!("  book.darml         the book source");
    println!("  art/landscape.svg  an example illustration");
    println!("  darm.toml          project settings");
    println!("\nnext:\n  darm check {d}\n  darm build {d}\n  darm dev {d}      # live preview");
    Ok(())
}
