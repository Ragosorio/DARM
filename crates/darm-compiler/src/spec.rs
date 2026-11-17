//! Declarative tables of everything the language knows: elements, their
//! arguments, property blocks and asset constructors.
//!
//! These tables drive argument validation in [`crate::lower`] (an argument a
//! table does not list is an error) and are exported verbatim by
//! `darm reference --json`, so tools and AI agents always see exactly what
//! the compiler accepts.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Param {
    pub name: &'static str,
    /// Human-readable type, e.g. `rich text`, `asset(image)`, `bool`.
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub required: bool,
    pub doc: &'static str,
}

const fn req(name: &'static str, ty: &'static str, doc: &'static str) -> Param {
    Param {
        name,
        ty,
        required: true,
        doc,
    }
}

const fn opt(name: &'static str, ty: &'static str, doc: &'static str) -> Param {
    Param {
        name,
        ty,
        required: false,
        doc,
    }
}

/// What may follow the element's `:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Body {
    /// No block allowed.
    None,
    /// Content blocks.
    Blocks,
    /// Content blocks that each need `at=`/`size=`.
    FramedBlocks,
    /// Only the listed child elements.
    Items,
    /// Action statements, required.
    Actions,
    /// Action statements, optional.
    OptionalActions,
    /// `name = value` properties (plus a few listed child elements).
    Properties,
    /// `name = constructor(...)` or `name = value [in domain]` declarations.
    Declarations,
}

/// Where an element may appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// Top level of a file.
    Top,
    /// Anywhere content blocks are allowed.
    Block,
    /// Only as a direct child of `chapter`.
    Chapter,
    /// Only inside the named parent element.
    Child,
    /// Inside action blocks.
    Action,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ElementSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub place: Place,
    /// Parent element for `Place::Child`.
    pub parent: &'static str,
    pub positional: &'static [Param],
    /// The last positional parameter may repeat (`gallery a, b, c`).
    pub variadic: bool,
    pub named: &'static [Param],
    pub body: Body,
    /// Child element names for `Body::Items` / extra children of `Body::Properties`.
    pub children: &'static [&'static str],
    /// Accepts the common block arguments ([`COMMON`]).
    pub common: bool,
}

pub const ALIGN: &[&str] = &["left", "center", "right", "justify"];
pub const TEXT_STYLES: &[&str] = &[
    "body", "lead", "small", "caption", "dropcap", "verse", "mono", "display",
];
pub const ANIMATIONS: &[&str] = &["fade", "rise", "drop", "slide_left", "slide_right", "zoom"];
pub const TRANSITIONS: &[&str] = &["flip", "fade", "none"];
pub const DIVIDERS: &[&str] = &["rule", "ornament", "blank"];
pub const BOX_STYLES: &[&str] = &["rule", "tint", "bar"];
pub const FITS: &[&str] = &["contain", "cover"];
pub const COVER_STYLES: &[&str] = &["classic", "band", "minimal", "modern"];
pub const LAYOUTS: &[&str] = &["reflow", "fixed"];
pub const FONTS: &[&str] = &["serif", "sans", "mono"];
pub const FRONT_MATTER: &[&str] = &["title", "colophon", "contents"];

/// Arguments every content block accepts.
pub const COMMON: &[Param] = &[
    opt(
        "id",
        "identifier",
        "Makes the block a jump target. Unique in the book.",
    ),
    opt(
        "align",
        "left|center|right|justify",
        "Horizontal alignment.",
    ),
    opt("color", "color", "Text colour override, \"#rrggbb\"."),
    opt("scale", "decimal 0.25..8", "Type size multiplier."),
    opt(
        "animate",
        "fade|rise|drop|slide_left|slide_right|zoom",
        "Entrance animation.",
    ),
    opt(
        "delay",
        "duration",
        "Delay before the entrance animation (default 0ms, max 30s).",
    ),
    opt(
        "duration",
        "duration",
        "Length of the entrance animation (default 600ms, 1ms..10s).",
    ),
    opt(
        "at",
        "(x%, y%)",
        "Top-left corner inside a fixed page, in percent of the page.",
    ),
    opt(
        "size",
        "(w%, h%)",
        "Size inside a fixed page, in percent of the page.",
    ),
];

const fn el(
    name: &'static str,
    summary: &'static str,
    place: Place,
    positional: &'static [Param],
    named: &'static [Param],
    body: Body,
    common: bool,
) -> ElementSpec {
    ElementSpec {
        name,
        summary,
        place,
        parent: "",
        positional,
        variadic: false,
        named,
        body,
        children: &[],
        common,
    }
}

const WHEN: Param = opt(
    "when",
    "condition",
    "Present only while the condition holds.",
);
const GOTO: Param = opt(
    "goto",
    "target",
    "Jump target: an id, or cover|start|contents|end.",
);
const TRANSITION: Param = opt(
    "transition",
    "flip|fade|none",
    "How the jump is shown (default flip).",
);
const CAPTION: Param = opt("caption", "rich text", "Caption shown under the media.");

pub const ELEMENTS: &[ElementSpec] = &[
    el(
        "book",
        "Book metadata. Exactly one per project.",
        Place::Top,
        &[req("title", "text", "Title of the book.")],
        &[],
        Body::Properties,
        false,
    ),
    el(
        "theme",
        "Interior colours and typography.",
        Place::Top,
        &[],
        &[],
        Body::Properties,
        false,
    ),
    el(
        "assets",
        "Media and font declarations: `name = image(\"path\", alt=\"...\")`.",
        Place::Top,
        &[],
        &[],
        Body::Declarations,
        false,
    ),
    el(
        "state",
        "State variable declarations: `name = initial [in domain]`.",
        Place::Top,
        &[],
        &[],
        Body::Declarations,
        false,
    ),
    el(
        "cover",
        "Front cover design.",
        Place::Top,
        &[],
        &[],
        Body::Properties,
        false,
    ),
    el(
        "back_cover",
        "Back cover design; `text` children are the blurb.",
        Place::Top,
        &[],
        &[],
        Body::Properties,
        false,
    ),
    el(
        "include",
        "Splice another .darml file's top-level statements here.",
        Place::Top,
        &[req("path", "text", "Path relative to this file.")],
        &[],
        Body::None,
        false,
    ),
    el(
        "chapter",
        "A chapter; starts on a new page.",
        Place::Top,
        &[req("title", "text", "Chapter title.")],
        &[
            opt("id", "identifier", "Jump target for the chapter."),
            opt(
                "numbered",
                "bool",
                "Show an ordinal on the opener (default true).",
            ),
            opt(
                "opener",
                "bool",
                "Render the chapter opener (default true).",
            ),
            opt(
                "toc",
                "bool",
                "List in the table of contents (default true).",
            ),
            WHEN,
        ],
        Body::Blocks,
        false,
    ),
    el(
        "section",
        "A titled section inside a chapter.",
        Place::Block,
        &[req("title", "rich text", "Section title.")],
        &[
            opt("id", "identifier", "Jump target."),
            opt("toc", "bool", "List in the contents (default true)."),
            WHEN,
        ],
        Body::Blocks,
        false,
    ),
    el(
        "heading",
        "A minor heading (not listed in the contents).",
        Place::Block,
        &[req("text", "rich text", "Heading text.")],
        &[],
        Body::None,
        true,
    ),
    el(
        "text",
        "A paragraph. In a \"\"\" block, blank lines separate paragraphs.",
        Place::Block,
        &[req("text", "rich text", "Paragraph text.")],
        &[opt(
            "style",
            "body|lead|small|caption|dropcap|verse|mono|display",
            "Paragraph style (default body).",
        )],
        Body::None,
        true,
    ),
    el(
        "quote",
        "A block quotation.",
        Place::Block,
        &[req("text", "rich text", "Quoted text.")],
        &[opt("by", "rich text", "Attribution.")],
        Body::None,
        true,
    ),
    el(
        "list",
        "A bulleted or numbered list of `item`s.",
        Place::Block,
        &[],
        &[opt("ordered", "bool", "Numbered list (default false).")],
        Body::Items,
        true,
    ),
    el(
        "image",
        "An image with an optional caption.",
        Place::Block,
        &[req("asset", "asset(image)", "Image asset name.")],
        &[
            CAPTION,
            opt(
                "width",
                "percent 10%..100%",
                "Width relative to the text column (default 100%).",
            ),
            opt(
                "fit",
                "contain|cover",
                "How the image fills its frame (default contain).",
            ),
        ],
        Body::None,
        true,
    ),
    ElementSpec {
        variadic: true,
        ..el(
            "gallery",
            "A swipeable set of 2-24 images.",
            Place::Block,
            &[req(
                "assets",
                "asset(image)...",
                "Two or more image asset names.",
            )],
            &[CAPTION],
            Body::None,
            true,
        )
    },
    el(
        "video",
        "A video player.",
        Place::Block,
        &[req("asset", "asset(video)", "Video asset name.")],
        &[
            CAPTION,
            opt(
                "autoplay",
                "bool",
                "Start (muted) when the page is shown (default false).",
            ),
            opt("loop", "bool", "Repeat (default false)."),
            opt("muted", "bool", "Start muted (default false)."),
        ],
        Body::None,
        true,
    ),
    el(
        "audio",
        "An audio player.",
        Place::Block,
        &[req("asset", "asset(audio)", "Audio asset name.")],
        &[CAPTION],
        Body::None,
        true,
    ),
    el(
        "divider",
        "A separator between passages.",
        Place::Block,
        &[],
        &[opt(
            "style",
            "rule|ornament|blank",
            "Appearance (default ornament).",
        )],
        Body::None,
        true,
    ),
    el(
        "space",
        "Vertical space measured in text lines.",
        Place::Block,
        &[req("lines", "number 0.25..40", "Height in lines.")],
        &[],
        Body::None,
        false,
    ),
    el(
        "break",
        "Start a new page.",
        Place::Block,
        &[],
        &[],
        Body::None,
        false,
    ),
    el(
        "box",
        "A framed or tinted group of blocks.",
        Place::Block,
        &[],
        &[
            opt("style", "rule|tint|bar", "Appearance (default rule)."),
            opt("title", "text", "Label shown at the top."),
        ],
        Body::Blocks,
        true,
    ),
    el(
        "button",
        "A button that runs actions.",
        Place::Block,
        &[req("label", "text", "Button label.")],
        &[GOTO, TRANSITION],
        Body::OptionalActions,
        true,
    ),
    el(
        "choice",
        "A narrative choice between `option`s.",
        Place::Block,
        &[req("prompt", "rich text", "The question or situation.")],
        &[],
        Body::Items,
        true,
    ),
    el(
        "quiz",
        "A multiple-choice question with `answer`s.",
        Place::Block,
        &[req("question", "rich text", "The question.")],
        &[],
        Body::Items,
        true,
    ),
    el(
        "page",
        "A fixed composition that fills one page.",
        Place::Chapter,
        &[],
        &[
            opt("background", "color", "Page background colour."),
            opt("id", "identifier", "Jump target."),
        ],
        Body::FramedBlocks,
        false,
    ),
    ElementSpec {
        parent: "list",
        ..el(
            "item",
            "One list entry.",
            Place::Child,
            &[req("text", "rich text", "Item text.")],
            &[],
            Body::None,
            false,
        )
    },
    ElementSpec {
        parent: "choice",
        ..el(
            "option",
            "One selectable option of a `choice`.",
            Place::Child,
            &[req("label", "text", "Option label.")],
            &[WHEN, GOTO, TRANSITION],
            Body::OptionalActions,
            false,
        )
    },
    ElementSpec {
        parent: "quiz",
        ..el(
            "answer",
            "One answer of a `quiz`.",
            Place::Child,
            &[req("label", "text", "Answer text.")],
            &[opt(
                "correct",
                "bool",
                "Marks a correct answer (default false).",
            )],
            Body::None,
            false,
        )
    },
    ElementSpec {
        parent: "quiz",
        ..el(
            "explain",
            "Explanation shown after answering a `quiz`.",
            Place::Child,
            &[req("text", "rich text", "Explanation.")],
            &[],
            Body::None,
            false,
        )
    },
    ElementSpec {
        parent: "quiz",
        ..el(
            "on_correct",
            "Actions run the first time a `quiz` is answered correctly.",
            Place::Child,
            &[],
            &[],
            Body::Actions,
            false,
        )
    },
    ElementSpec {
        parent: "book",
        ..el(
            "credit",
            "A credit line in the colophon.",
            Place::Child,
            &[
                req("role", "text", "Role, e.g. \"Illustrations\"."),
                req("name", "text", "Person or organisation."),
            ],
            &[],
            Body::None,
            false,
        )
    },
    el(
        "goto",
        "Action: jump to a target.",
        Place::Action,
        &[req(
            "target",
            "target",
            "An id, or cover|start|contents|end.",
        )],
        &[TRANSITION],
        Body::None,
        false,
    ),
    el(
        "reset",
        "Action: restore all state (and quiz/choice records), or one variable.",
        Place::Action,
        &[opt(
            "variable",
            "state variable",
            "Restore only this variable.",
        )],
        &[],
        Body::None,
        false,
    ),
];

/// Elements whose children are listed explicitly.
pub fn children_of(parent: &str) -> Vec<&'static str> {
    ELEMENTS
        .iter()
        .filter(|e| e.place == Place::Child && e.parent == parent)
        .map(|e| e.name)
        .collect()
}

pub fn element(name: &str) -> Option<&'static ElementSpec> {
    ELEMENTS.iter().find(|e| e.name == name)
}

/// Names of every element that can appear where content blocks are allowed.
pub fn block_names() -> impl Iterator<Item = &'static str> {
    ELEMENTS
        .iter()
        .filter(|e| matches!(e.place, Place::Block | Place::Chapter))
        .map(|e| e.name)
}

pub const BOOK_PROPS: &[Param] = &[
    opt(
        "id",
        "text",
        "Stable identifier of the work; derived from title and author when absent.",
    ),
    opt("subtitle", "text", "Subtitle."),
    req("author", "text or list of text", "Author name(s)."),
    req(
        "language",
        "text",
        "BCP 47 language tag, e.g. \"en\" or \"es-MX\".",
    ),
    opt("edition", "text", "Edition statement."),
    opt("version", "text", "Version of this edition."),
    opt("publisher", "text", "Publisher."),
    opt("published", "text", "Publication date, YYYY-MM-DD."),
    opt(
        "description",
        "text",
        "Short description; default back-cover blurb.",
    ),
    opt("rights", "text", "Copyright / licence statement."),
    opt("layout", "reflow|fixed", "Body layout (default reflow)."),
    opt(
        "page_size",
        "(width, height)",
        "Page proportions (default (2, 3) reflow, (3, 4) fixed).",
    ),
    opt(
        "offline",
        "bool",
        "All resources are packaged; never touch the network (default true).",
    ),
    opt(
        "network",
        "list of text",
        "Hosts remote assets may load from; requires offline = false.",
    ),
    opt(
        "front_matter",
        "list of title|colophon|contents",
        "Generated front matter (default all three).",
    ),
    opt(
        "typography",
        "smart|plain",
        "Smart quotes, dashes and ellipses (default smart).",
    ),
];

pub const THEME_PROPS: &[Param] = &[
    opt("paper", "color", "Paper colour (default \"#f7f3ea\")."),
    opt("ink", "color", "Text colour (default \"#1f1c18\")."),
    opt("accent", "color", "Accent colour (default \"#8c3b2a\")."),
    opt(
        "dark_paper",
        "color",
        "Dark-mode paper; give all three dark_* or none.",
    ),
    opt("dark_ink", "color", "Dark-mode text colour."),
    opt("dark_accent", "color", "Dark-mode accent."),
    opt(
        "body_font",
        "serif|sans|mono|asset(font)",
        "Body typeface (default serif).",
    ),
    opt(
        "heading_font",
        "serif|sans|mono|asset(font)",
        "Heading typeface (default: same as body).",
    ),
    opt(
        "paragraph",
        "indent|spaced",
        "Paragraph separation (default indent).",
    ),
    opt("justify", "bool", "Justify body text (default true)."),
    opt("hyphenate", "bool", "Hyphenate body text (default true)."),
    opt(
        "drop_caps",
        "bool",
        "Drop capital on the first paragraph of each chapter (default false).",
    ),
];

pub const COVER_PROPS: &[Param] = &[
    opt(
        "style",
        "classic|band|minimal|modern",
        "Lettering layout (default classic).",
    ),
    opt("color", "color", "Cloth colour."),
    opt("ink", "color", "Lettering colour."),
    opt("accent", "color", "Foil / ornament colour."),
    opt("image", "asset(image)", "Cover image."),
    opt(
        "full_bleed",
        "bool",
        "Image covers the whole board (default false).",
    ),
    opt(
        "show_text",
        "bool",
        "Draw the lettering (default true; false needs an image).",
    ),
    opt("title", "text", "Lettering override (default: book title)."),
    opt(
        "subtitle",
        "text",
        "Lettering override (default: book subtitle).",
    ),
    opt("author", "text", "Lettering override (default: authors)."),
];

pub const BACK_COVER_PROPS: &[Param] = &[
    opt("color", "color", "Cloth colour (default: front cover)."),
    opt("ink", "color", "Lettering colour (default: front cover)."),
    opt("accent", "color", "Ornament colour (default: front cover)."),
    opt("tagline", "text", "One line set above the blurb."),
    opt(
        "image",
        "asset(image)",
        "Small image, e.g. an author portrait or device.",
    ),
];

/// Asset constructors usable inside `assets:`.
pub const CONSTRUCTORS: &[ElementSpec] = &[
    el(
        "image",
        "PNG, JPEG, GIF, WebP or static SVG.",
        Place::Child,
        &[opt(
            "path",
            "text",
            "File path relative to this source file.",
        )],
        &[
            req("alt", "text", "Text alternative."),
            opt("title", "text", "Title."),
        ],
        Body::None,
        false,
    ),
    el(
        "video",
        "MP4 or WebM.",
        Place::Child,
        &[opt(
            "path",
            "text",
            "File path relative to this source file.",
        )],
        &[
            opt("url", "text", "Remote https URL instead of a path."),
            req("alt", "text", "Text alternative."),
            opt("title", "text", "Title."),
            opt("poster", "text", "Path of the poster image."),
            opt(
                "captions",
                "text or list of text",
                "WebVTT path(s); language from a `.<lang>.vtt` suffix.",
            ),
            opt("transcript", "text", "Full transcript."),
        ],
        Body::None,
        false,
    ),
    el(
        "audio",
        "MP3, M4A/AAC, Ogg, WAV or FLAC.",
        Place::Child,
        &[opt(
            "path",
            "text",
            "File path relative to this source file.",
        )],
        &[
            opt("url", "text", "Remote https URL instead of a path."),
            opt("title", "text", "Title shown in the player."),
            opt("captions", "text or list of text", "WebVTT path(s)."),
            opt("transcript", "text", "Full transcript."),
        ],
        Body::None,
        false,
    ),
    el(
        "font",
        "WOFF2, TTF or OTF.",
        Place::Child,
        &[req(
            "path",
            "text",
            "File path relative to this source file.",
        )],
        &[],
        Body::None,
        false,
    ),
];

pub fn constructor(name: &str) -> Option<&'static ElementSpec> {
    CONSTRUCTORS.iter().find(|e| e.name == name)
}

/// The whole language surface as JSON (`darm reference --json`).
pub fn reference_json() -> serde_json::Value {
    serde_json::json!({
        "grammar_version": darm_ir::GRAMMAR_VERSION,
        "ir_version": darm_ir::IR_VERSION,
        "source_extension": ".darml",
        "pragma": "darm 1",
        "elements": ELEMENTS,
        "common_block_arguments": COMMON,
        "properties": {
            "book": BOOK_PROPS,
            "theme": THEME_PROPS,
            "cover": COVER_PROPS,
            "back_cover": BACK_COVER_PROPS,
        },
        "asset_constructors": CONSTRUCTORS,
        "reserved_targets": darm_ir::RESERVED_TARGETS,
        "rich_text": {
            "*italic*": "italic",
            "**bold**": "bold",
            "`code`": "monospace",
            "[label](target)": "link to an id, a reserved target, or an https/http/mailto URL",
            "{name}": "value of a state variable or component parameter",
            "\\*": "literal character (also \\[ \\] \\{ \\} \\` \\\\)",
        },
        "actions": ["name = expr", "name += expr", "name -= expr", "goto target[, transition=...]", "reset", "reset name"],
        "state_declarations": ["name = 0 in 0..5", "name = false", "name = \"a\" in [\"a\", \"b\"]", "name = 0"],
        "expressions": {
            "operators": ["+", "-", "*", "==", "!=", "<", "<=", ">", ">=", "and", "or", "not"],
            "not_available": ["division", "function calls", "loops", "string computation"],
        },
        "conditions": {
            "when": "when=EXPR on a chapter, a section or an option",
            "if": ["if EXPR:", "elif EXPR:", "else:"],
        },
        "components": {
            "declare": "component name(param, other=\"default\"):  at the top level, followed by an indented body of content blocks",
            "use": "name \"value\", other=\"x\"  (positional arguments first, then name=value)",
            "interpolate": "{param} inside any text of the body",
            "rules": ["expanded at compile time", "no recursion", "no state of their own", "the name must not be a built-in element"],
        },
        "diagnostics": crate::explain::CATALOGUE.iter().map(|e| serde_json::json!({
            "code": e.code, "title": e.title, "explanation": e.explanation,
        })).collect::<Vec<_>>(),
    })
}
