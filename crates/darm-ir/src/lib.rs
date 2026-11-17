//! # DARM IR
//!
//! The intermediate representation of a DARM book: a stable, typed, serializable
//! model that is completely independent from the `.darml` source syntax.
//!
//! * The compiler lowers a checked AST into a [`Book`] and serializes it as JSON
//!   into the `.darm` package (entry role `ir`).
//! * The reader deserializes it, re-validates it with [`validate::validate`] (a
//!   package is untrusted input) and interprets it. The reader never sees or
//!   evaluates DARM source code.
//!
//! ## Versioning
//!
//! `ir_version` is `"MAJOR.MINOR"`. Readers reject an unknown MAJOR. A newer
//! MINOR may only add optional fields, which older readers ignore (there is no
//! `deny_unknown_fields`). A new block kind, action or enum member is *not*
//! skippable — an older reader rejects the document — so those require a new
//! MAJOR.
//!
//! The JSON Schema for this model is generated from these types
//! (`darm schema`, feature `schema`) and committed at `schemas/darm-ir.schema.json`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod expr;
pub mod search;
pub mod validate;

pub use expr::{EvalError, State, Value};
pub use validate::{IrError, validate};

/// Major version of the IR. Readers refuse any other major.
pub const IR_VERSION_MAJOR: u32 = 1;
/// Minor version of the IR written by this toolchain.
pub const IR_VERSION_MINOR: u32 = 0;
/// `"MAJOR.MINOR"` string stored in [`Book::ir_version`].
pub const IR_VERSION: &str = "1.0";
/// Version of the `.darml` grammar this toolchain implements (`darm 1` pragma).
pub const GRAMMAR_VERSION: u32 = 1;

/// Anchor names that always exist and cannot be redefined by authors.
pub const RESERVED_TARGETS: &[&str] = &["cover", "start", "contents", "end"];

fn is_false(b: &bool) -> bool {
    !*b
}

/// A whole book. This is the document stored in the `ir` entry of a package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Book {
    /// `"MAJOR.MINOR"`, see [`IR_VERSION`].
    pub ir_version: String,
    /// Grammar version of the source this IR was compiled from.
    pub grammar_version: u32,
    pub meta: Meta,
    pub theme: Theme,
    /// Media and fonts, in declaration order. Names are unique.
    #[serde(default)]
    pub resources: Vec<Resource>,
    /// Bounded state variables used by interactive content.
    #[serde(default)]
    pub state: Vec<StateVar>,
    /// Front and back cover. Both are always present.
    pub cover: Covers,
    /// Reading order: generated front matter followed by authored chapters.
    pub chapters: Vec<Chapter>,
    /// Table of contents, precomputed by the compiler.
    #[serde(default)]
    pub toc: Vec<TocEntry>,
    /// Named jump targets (`id=` in source) mapped to their location.
    #[serde(default)]
    pub anchors: BTreeMap<String, Anchor>,
}

/// Bibliographic and behavioural metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Meta {
    /// Stable identifier of the work (reverse-DNS style or derived hash).
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    /// BCP 47 language tag of the main text.
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    /// Version of this edition of the book (free-form, e.g. `1.0.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// ISO 8601 date (`YYYY-MM-DD`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credits: Vec<Credit>,
    pub layout: Layout,
    /// Page width divided by page height. Authoritative for `fixed` books;
    /// a preferred proportion for `reflow` books.
    pub page_aspect: f64,
    /// `true`: every resource is inside the package and the reader must not
    /// touch the network for this book.
    pub offline: bool,
    /// Hosts remote resources may be loaded from. Empty means "no network".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network: Vec<String>,
    /// Name and version of the tool that produced this IR.
    pub generator: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Credit {
    pub role: String,
    pub name: String,
}

/// How the body of the book is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Text flows into pages sized for the device; user can change type size.
    Reflow,
    /// Every page is an authored composition with a fixed aspect ratio.
    Fixed,
}

/// Visual identity of the book interior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Theme {
    /// Paper colour, `#rrggbb`.
    pub paper: String,
    /// Text colour, `#rrggbb`.
    pub ink: String,
    /// Accent colour for ornaments, links and controls, `#rrggbb`.
    pub accent: String,
    /// Optional palette for the reader's dark mode. Derived by the reader when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<Palette>,
    pub body_font: FontRef,
    pub heading_font: FontRef,
    pub paragraph: ParagraphStyle,
    pub justify: bool,
    pub hyphenate: bool,
    pub drop_caps: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Palette {
    pub paper: String,
    pub ink: String,
    pub accent: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum FontRef {
    /// One of the typefaces every reader ships.
    Builtin { family: BuiltinFont },
    /// A font resource embedded in the package.
    Resource { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BuiltinFont {
    Serif,
    Sans,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ParagraphStyle {
    /// First-line indent, no space between paragraphs (book style).
    Indent,
    /// No indent, space between paragraphs.
    Spaced,
}

/// A media file, caption track or font used by the book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Resource {
    /// Name used by blocks to refer to this resource. Unique.
    pub name: String,
    pub kind: ResourceKind,
    pub source: ResourceSource,
    /// IANA media type, verified against the file content by the compiler.
    pub media_type: String,
    /// Size in bytes (0 for remote resources).
    #[serde(default)]
    pub size: u64,
    /// Lower-case hex SHA-256 of the file (empty for remote resources).
    #[serde(default)]
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Text alternative. Required by the compiler for images and video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Name of an image resource shown before a video plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    /// Caption/subtitle tracks (video and audio).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captions: Vec<CaptionTrack>,
    /// Full-text transcript (audio and video).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Image,
    Video,
    Audio,
    Captions,
    Font,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "location", rename_all = "snake_case")]
pub enum ResourceSource {
    /// Stored in the package under the entry name `entry`.
    Packaged { entry: String },
    /// Loaded from the network, only if the host is in [`Meta::network`]
    /// and the reader's user allows it.
    Remote { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CaptionTrack {
    /// Name of a resource of kind `captions` (WebVTT).
    pub resource: String,
    /// BCP 47 language tag.
    pub lang: String,
    pub label: String,
}

/// A bounded state variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StateVar {
    pub name: String,
    #[serde(flatten)]
    pub ty: StateType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StateType {
    /// Integer clamped to `min..=max` on every assignment.
    Int {
        initial: i64,
        min: i64,
        max: i64,
    },
    Bool {
        initial: bool,
    },
    /// One of a closed set of strings.
    Enum {
        initial: String,
        values: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Covers {
    pub front: FrontCover,
    pub back: BackCover,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FrontCover {
    pub style: CoverStyle,
    /// Cloth/board colour, `#rrggbb`.
    pub color: String,
    /// Lettering colour, `#rrggbb`.
    pub ink: String,
    /// Foil/ornament colour, `#rrggbb`.
    pub accent: String,
    /// Optional image resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// `true`: the image covers the whole board. `false`: it sits in a panel.
    #[serde(default, skip_serializing_if = "is_false")]
    pub full_bleed: bool,
    /// Draw title/author lettering (may be disabled when the image carries it).
    pub show_text: bool,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub author: String,
    /// `true` when the compiler derived this design because the source had no `cover:` block.
    #[serde(default, skip_serializing_if = "is_false")]
    pub generated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverStyle {
    /// Centered lettering inside a double-rule frame.
    Classic,
    /// A horizontal title band across the board.
    Band,
    /// Small lettering, generous empty cloth.
    Minimal,
    /// Large display title set flush left.
    Modern,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BackCover {
    pub color: String,
    pub ink: String,
    pub accent: String,
    /// Blurb paragraphs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blurb: Vec<Vec<Span>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub generated: bool,
}

/// One unit of the reading order. A chapter always starts on a new page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Chapter {
    pub id: String,
    pub kind: ChapterKind,
    pub title: String,
    /// Ordinal shown on the chapter opener; `None` for unnumbered chapters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// Render a chapter opener (number, title, ornament) before the blocks.
    #[serde(default, skip_serializing_if = "is_false")]
    pub opener: bool,
    /// The chapter is part of the reading order only while this holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Expr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ChapterKind {
    /// Generated title page (rendered from [`Meta`]).
    Title,
    /// Generated colophon: edition, rights, credits.
    Colophon,
    /// Generated table of contents (rendered from [`Book::toc`]).
    Contents,
    /// Authored content.
    Chapter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TocEntry {
    pub title: String,
    /// Anchor name to jump to.
    pub target: String,
    /// 1 = chapter, 2 = section.
    pub level: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// Entry is listed only while this holds (mirrors the chapter/section condition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Expr>,
}

/// Location of a named anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Anchor {
    /// Id of the chapter that contains the target.
    pub chapter: String,
    /// Id of the block inside the chapter; `None` targets the chapter start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
}

/// A piece of content. `id` is unique in the book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Block {
    pub id: String,
    #[serde(flatten)]
    pub kind: BlockKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// Entrance animation played when the page becomes visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<Anim>,
    /// Placement inside a fixed `page` (percent of the page box). Only valid there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Frame>,
    /// Text colour override, `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Type size multiplier (0.25..=8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    Paragraph {
        spans: Vec<Span>,
        style: TextStyle,
    },
    Heading {
        spans: Vec<Span>,
        /// 2 = section title, 3 = minor heading. (Level 1 is the chapter opener.)
        level: u8,
    },
    Quote {
        paragraphs: Vec<Vec<Span>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attribution: Option<Vec<Span>>,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Span>>,
    },
    Image {
        resource: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<Vec<Span>>,
        /// Fraction of the text width (0.1..=1).
        width: f64,
        fit: Fit,
    },
    Gallery {
        resources: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<Vec<Span>>,
    },
    Video {
        resource: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<Vec<Span>>,
        #[serde(default, skip_serializing_if = "is_false")]
        autoplay: bool,
        #[serde(default, rename = "loop", skip_serializing_if = "is_false")]
        looped: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        muted: bool,
    },
    Audio {
        resource: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<Vec<Span>>,
    },
    Divider {
        style: DividerStyle,
    },
    Space {
        /// Height in text lines (0.25..=40).
        lines: f64,
    },
    PageBreak,
    Box {
        style: BoxStyle,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        blocks: Vec<Block>,
    },
    Button {
        label: String,
        actions: Vec<Action>,
    },
    Choice {
        prompt: Vec<Span>,
        options: Vec<ChoiceOption>,
    },
    Quiz {
        question: Vec<Span>,
        answers: Vec<QuizAnswer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        explanation: Option<Vec<Span>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_correct: Vec<Action>,
    },
    /// Content shown only while `when` holds (lowered from `if`/`elif`/`else`
    /// and from `section ..., when=`). `when: None` is an unconditional group.
    Group {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<Expr>,
        blocks: Vec<Block>,
    },
    /// A fixed-layout composition that occupies exactly one page.
    Page {
        /// Background colour, `#rrggbb`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        background: Option<String>,
        blocks: Vec<Block>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TextStyle {
    Body,
    /// Larger opening paragraph.
    Lead,
    Small,
    Caption,
    /// Body paragraph with a drop capital.
    Dropcap,
    /// Line breaks are preserved, no justification.
    Verse,
    Mono,
    /// Large display lettering (title plates, fixed pages).
    Display,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Whole image visible.
    Contain,
    /// Fill the frame, cropping if needed.
    Cover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DividerStyle {
    Rule,
    Ornament,
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BoxStyle {
    /// Hairline border.
    Rule,
    /// Tinted background.
    Tint,
    /// Accent bar on the leading edge.
    Bar,
}

/// A run of inline text with uniform formatting.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Span {
    /// Literal text. Empty when `var` is set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Link>,
    /// Name of a state variable whose current value is shown in place of `text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var: Option<String>,
    /// Forced line break after this span (verse and addresses).
    #[serde(default, rename = "break", skip_serializing_if = "is_false")]
    pub line_break: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Link {
    /// Jump inside the book.
    Anchor { target: String },
    /// External `https:`, `http:` or `mailto:` link, opened outside the reader after confirmation.
    Url { href: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Anim {
    pub kind: AnimKind,
    #[serde(default)]
    pub delay_ms: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AnimKind {
    Fade,
    /// Fade in while moving up.
    Rise,
    /// Fade in while moving down.
    Drop,
    SlideLeft,
    SlideRight,
    Zoom,
}

/// Placement of a block on a fixed page, in percent of the page box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChoiceOption {
    pub label: String,
    /// Option is offered only while this holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Expr>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QuizAnswer {
    pub label: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub correct: bool,
}

/// How the reader moves to a non-adjacent location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// Riffle through the leaves in between (default).
    Flip,
    /// Cross-fade.
    Fade,
    /// Cut.
    None,
}

/// A declarative effect. Actions are the only way content can change state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    /// Assign `value` to the state variable `var` (ints are clamped to range).
    Set { var: String, value: Expr },
    /// Navigate to an anchor or a reserved target (see [`RESERVED_TARGETS`]).
    Goto {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition: Option<Transition>,
    },
    /// Restore one variable, or all of them and every quiz/choice when `var` is `None`.
    Reset {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        var: Option<String>,
    },
}

/// A total, side-effect free expression over state variables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Expr {
    Int {
        value: i64,
    },
    Bool {
        value: bool,
    },
    Str {
        value: String,
    },
    Var {
        name: String,
    },
    Not {
        expr: Box<Expr>,
    },
    Neg {
        expr: Box<Expr>,
    },
    Bin {
        kind: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl Book {
    /// Serialize as compact JSON (the form stored in packages).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("IR serialization cannot fail")
    }

    /// Serialize as indented JSON (for `darm inspect --ir` and snapshots).
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("IR serialization cannot fail")
    }

    /// Parse IR JSON and check the version. Does **not** run [`validate`].
    pub fn from_json(json: &str) -> Result<Book, IrError> {
        // Check the version first so that a future major gives a version error
        // rather than a confusing missing-field error.
        let probe: serde_json::Value =
            serde_json::from_str(json).map_err(|e| IrError::new("E_IR_JSON", "", e.to_string()))?;
        let version = probe
            .get("ir_version")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        validate::check_version(version)?;
        serde_json::from_value(probe).map_err(|e| IrError::new("E_IR_JSON", "", e.to_string()))
    }

    pub fn resource(&self, name: &str) -> Option<&Resource> {
        self.resources.iter().find(|r| r.name == name)
    }

    pub fn state_var(&self, name: &str) -> Option<&StateVar> {
        self.state.iter().find(|v| v.name == name)
    }

    /// Visit every block in reading order, including nested ones.
    pub fn walk_blocks<'a>(&'a self, mut f: impl FnMut(&'a Chapter, &'a Block)) {
        fn walk<'a>(
            c: &'a Chapter,
            blocks: &'a [Block],
            f: &mut impl FnMut(&'a Chapter, &'a Block),
        ) {
            for b in blocks {
                f(c, b);
                if let Some(children) = b.kind.children() {
                    walk(c, children, f);
                }
            }
        }
        for c in &self.chapters {
            walk(c, &c.blocks, &mut f);
        }
    }
}

impl BlockKind {
    /// Nested blocks of container kinds.
    pub fn children(&self) -> Option<&[Block]> {
        match self {
            BlockKind::Box { blocks, .. }
            | BlockKind::Group { blocks, .. }
            | BlockKind::Page { blocks, .. } => Some(blocks),
            _ => None,
        }
    }

    /// The `type` tag used in JSON.
    pub fn type_name(&self) -> &'static str {
        match self {
            BlockKind::Paragraph { .. } => "paragraph",
            BlockKind::Heading { .. } => "heading",
            BlockKind::Quote { .. } => "quote",
            BlockKind::List { .. } => "list",
            BlockKind::Image { .. } => "image",
            BlockKind::Gallery { .. } => "gallery",
            BlockKind::Video { .. } => "video",
            BlockKind::Audio { .. } => "audio",
            BlockKind::Divider { .. } => "divider",
            BlockKind::Space { .. } => "space",
            BlockKind::PageBreak => "page_break",
            BlockKind::Box { .. } => "box",
            BlockKind::Button { .. } => "button",
            BlockKind::Choice { .. } => "choice",
            BlockKind::Quiz { .. } => "quiz",
            BlockKind::Group { .. } => "group",
            BlockKind::Page { .. } => "page",
        }
    }
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            ..Span::default()
        }
    }
}

/// `true` for `#rgb`-free, strict `#rrggbb` colours (the only form the IR allows).
pub fn is_color(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[0] == b'#' && b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

/// `true` for identifiers usable as ids, resource names and variable names.
pub fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    s.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
