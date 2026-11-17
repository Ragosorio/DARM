//! Structural validation of an IR document.
//!
//! The compiler runs this after lowering (a failure is a compiler bug) and
//! the reader runs it on every package it opens (the IR is untrusted input).
//! Everything a reader relies on is checked here, so the presentation layer
//! can index resources, anchors and variables without defensive code.

use std::collections::{BTreeMap, BTreeSet};

use crate::expr::{self, Ty};
use crate::*;

/// Hard limits for an IR document. Exceeding any of them is an error.
pub mod limits {
    pub const MAX_CHAPTERS: usize = 4_096;
    pub const MAX_BLOCKS: usize = 200_000;
    pub const MAX_NESTING: usize = 12;
    pub const MAX_RESOURCES: usize = 16_384;
    pub const MAX_STATE_VARS: usize = 256;
    pub const MAX_ENUM_VALUES: usize = 64;
    pub const MAX_EXPR_DEPTH: usize = 32;
    pub const MAX_ACTIONS: usize = 32;
    pub const MAX_OPTIONS: usize = 12;
    pub const MAX_SPAN_TEXT: usize = 64 * 1024;
    pub const MAX_STRING: usize = 8 * 1024;
    pub const INT_MIN: i64 = -1_000_000_000;
    pub const INT_MAX: i64 = 1_000_000_000;
}

/// A validation failure with a stable code and a JSON-pointer-like path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrError {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

impl IrError {
    pub fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> IrError {
        IrError {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}: {}", self.code, self.message)
        } else {
            write!(f, "{}: {} (at {})", self.code, self.message, self.path)
        }
    }
}

impl std::error::Error for IrError {}

/// Accept `MAJOR.MINOR` with our major; reject anything else.
pub fn check_version(version: &str) -> Result<(), IrError> {
    let major = version
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok());
    let minor = version
        .split('.')
        .nth(1)
        .and_then(|m| m.parse::<u32>().ok());
    match (major, minor) {
        (Some(IR_VERSION_MAJOR), Some(_)) if version.split('.').count() == 2 => Ok(()),
        (Some(m), Some(_)) if m > IR_VERSION_MAJOR => Err(IrError::new(
            "E_IR_VERSION",
            "ir_version",
            format!(
                "this book uses IR version {version}; this reader understands {IR_VERSION_MAJOR}.x. Update the reader."
            ),
        )),
        _ => Err(IrError::new(
            "E_IR_VERSION",
            "ir_version",
            format!(
                "unsupported or malformed IR version `{version}` (expected {IR_VERSION_MAJOR}.x)"
            ),
        )),
    }
}

struct Ctx<'a> {
    book: &'a Book,
    errors: Vec<IrError>,
    resources: BTreeMap<&'a str, &'a Resource>,
    vars: BTreeMap<&'a str, &'a StateVar>,
    block_ids: BTreeSet<&'a str>,
    block_count: usize,
}

impl<'a> Ctx<'a> {
    fn err(&mut self, code: &'static str, path: &str, message: impl Into<String>) {
        // Keep the list bounded even for adversarial documents.
        if self.errors.len() < 200 {
            self.errors.push(IrError::new(code, path, message));
        }
    }

    fn color(&mut self, path: &str, c: &str) {
        if !is_color(c) {
            self.err(
                "E_IR_COLOR",
                path,
                format!("`{c}` is not a `#rrggbb` colour"),
            );
        }
    }

    fn text(&mut self, path: &str, s: &str) {
        if s.len() > limits::MAX_STRING {
            self.err("E_IR_LIMIT", path, "string is too long");
        }
        if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
            self.err("E_IR_TEXT", path, "string contains control characters");
        }
    }

    fn resource(&mut self, path: &str, name: &str, kinds: &[ResourceKind]) {
        match self.resources.get(name) {
            None => self.err("E_IR_REF", path, format!("unknown resource `{name}`")),
            Some(r) if !kinds.contains(&r.kind) => self.err(
                "E_IR_REF",
                path,
                format!("resource `{name}` has the wrong kind for this use"),
            ),
            Some(_) => {}
        }
    }

    fn target(&mut self, path: &str, target: &str) {
        if !RESERVED_TARGETS.contains(&target) && !self.book.anchors.contains_key(target) {
            self.err("E_IR_REF", path, format!("unknown jump target `{target}`"));
        }
    }

    fn var_ty(&self, name: &str) -> Option<Ty> {
        self.vars.get(name).map(|v| v.ty())
    }

    fn expr(&mut self, path: &str, e: &Expr, want: Ty) {
        if expr::depth(e) > limits::MAX_EXPR_DEPTH {
            self.err("E_IR_LIMIT", path, "expression is nested too deeply");
            return;
        }
        let lookup = |n: &str| self.var_ty(n);
        match expr::type_of(e, &lookup) {
            Ok(t) if t == want => {}
            Ok(t) => self.err(
                "E_IR_TYPE",
                path,
                format!("expected {} but expression is {}", want.name(), t.name()),
            ),
            Err(m) => self.err("E_IR_TYPE", path, m),
        }
    }

    fn when(&mut self, path: &str, when: &Option<Expr>) {
        if let Some(e) = when {
            self.expr(&format!("{path}/when"), e, Ty::Bool);
        }
    }

    fn spans(&mut self, path: &str, spans: &[Span]) {
        for (i, s) in spans.iter().enumerate() {
            let p = format!("{path}/{i}");
            if s.text.len() > limits::MAX_SPAN_TEXT {
                self.err("E_IR_LIMIT", &p, "text run is too long");
            }
            if s.text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                self.err("E_IR_TEXT", &p, "text contains control characters");
            }
            if let Some(v) = &s.var
                && !self.vars.contains_key(v.as_str())
            {
                self.err("E_IR_REF", &p, format!("unknown state variable `{v}`"));
            }
            match &s.link {
                Some(Link::Anchor { target }) => self.target(&p, target),
                Some(Link::Url { href }) if !is_safe_url(href) => {
                    self.err(
                        "E_IR_URL",
                        &p,
                        format!("link `{href}` must use https:, http: or mailto:"),
                    );
                }
                Some(Link::Url { .. }) => {}
                None => {}
            }
        }
    }

    fn actions(&mut self, path: &str, actions: &[Action]) {
        if actions.len() > limits::MAX_ACTIONS {
            self.err("E_IR_LIMIT", path, "too many actions");
        }
        for (i, a) in actions.iter().enumerate() {
            let p = format!("{path}/{i}");
            match a {
                Action::Set { var, value } => match self.vars.get(var.as_str()).copied() {
                    None => self.err("E_IR_REF", &p, format!("unknown state variable `{var}`")),
                    Some(decl) => {
                        self.expr(&p, value, decl.ty());
                        if let (StateType::Enum { values, .. }, Expr::Str { value }) =
                            (&decl.ty, value)
                            && !values.contains(value)
                        {
                            self.err(
                                "E_IR_TYPE",
                                &p,
                                format!("`{value}` is not a value of `{var}`"),
                            );
                        }
                    }
                },
                Action::Goto { target, .. } => self.target(&p, target),
                Action::Reset { var: Some(var) } => {
                    if !self.vars.contains_key(var.as_str()) {
                        self.err("E_IR_REF", &p, format!("unknown state variable `{var}`"));
                    }
                }
                Action::Reset { var: None } => {}
            }
        }
    }

    fn blocks(&mut self, path: &str, blocks: &'a [Block], depth: usize, in_page: bool) {
        if depth > limits::MAX_NESTING {
            self.err("E_IR_LIMIT", path, "blocks are nested too deeply");
            return;
        }
        for (i, b) in blocks.iter().enumerate() {
            let p = format!("{path}/{i}");
            self.block_count += 1;
            if self.block_count > limits::MAX_BLOCKS {
                self.err("E_IR_LIMIT", &p, "too many blocks");
                return;
            }
            if b.id.is_empty() || b.id.len() > 80 {
                self.err("E_IR_ID", &p, "block id is empty or too long");
            } else if !self.block_ids.insert(&b.id) {
                self.err("E_IR_ID", &p, format!("duplicate block id `{}`", b.id));
            }
            if let Some(c) = &b.color {
                self.color(&p, c);
            }
            if let Some(s) = b.scale
                && !(0.25..=8.0).contains(&s)
            {
                self.err("E_IR_RANGE", &p, "scale must be between 0.25 and 8");
            }
            if let Some(a) = &b.anim
                && (a.duration_ms == 0 || a.duration_ms > 10_000 || a.delay_ms > 30_000)
            {
                self.err(
                    "E_IR_RANGE",
                    &p,
                    "animation duration must be 1..=10000 ms and delay at most 30000 ms",
                );
            }
            match (&b.frame, in_page) {
                (Some(_), true) if matches!(b.kind, BlockKind::Group { .. }) => self.err(
                    "E_IR_FRAME",
                    &p,
                    "a conditional group cannot have a frame; frame its children",
                ),
                (Some(f), true) => {
                    let ok = [f.x, f.y, f.w, f.h].iter().all(|v| v.is_finite())
                        && f.x >= 0.0
                        && f.y >= 0.0
                        && f.w > 0.0
                        && f.h > 0.0
                        && f.x + f.w <= 100.0001
                        && f.y + f.h <= 100.0001;
                    if !ok {
                        self.err(
                            "E_IR_RANGE",
                            &p,
                            "frame must lie inside the page (percent units, 0..100)",
                        );
                    }
                }
                (Some(_), false) => self.err(
                    "E_IR_FRAME",
                    &p,
                    "only blocks inside a fixed page may have a frame",
                ),
                (None, true) if !matches!(b.kind, BlockKind::Group { .. }) => {
                    self.err("E_IR_FRAME", &p, "blocks inside a fixed page need a frame")
                }
                (None, true) => {}
                (None, false) => {}
            }
            match &b.kind {
                BlockKind::Paragraph { spans, .. } => self.spans(&p, spans),
                BlockKind::Heading { spans, level } => {
                    self.spans(&p, spans);
                    if !(2..=3).contains(level) {
                        self.err("E_IR_RANGE", &p, "heading level must be 2 or 3");
                    }
                }
                BlockKind::Quote {
                    paragraphs,
                    attribution,
                } => {
                    for q in paragraphs {
                        self.spans(&p, q);
                    }
                    if let Some(a) = attribution {
                        self.spans(&p, a);
                    }
                }
                BlockKind::List { items, .. } => {
                    for it in items {
                        self.spans(&p, it);
                    }
                }
                BlockKind::Image {
                    resource,
                    caption,
                    width,
                    ..
                } => {
                    self.resource(&p, resource, &[ResourceKind::Image]);
                    if let Some(c) = caption {
                        self.spans(&p, c);
                    }
                    if !(0.1..=1.0).contains(width) {
                        self.err("E_IR_RANGE", &p, "image width must be between 10% and 100%");
                    }
                }
                BlockKind::Gallery { resources, caption } => {
                    if resources.len() < 2 || resources.len() > 24 {
                        self.err("E_IR_RANGE", &p, "a gallery needs 2 to 24 images");
                    }
                    for r in resources {
                        self.resource(&p, r, &[ResourceKind::Image]);
                    }
                    if let Some(c) = caption {
                        self.spans(&p, c);
                    }
                }
                BlockKind::Video {
                    resource, caption, ..
                } => {
                    self.resource(&p, resource, &[ResourceKind::Video]);
                    if let Some(c) = caption {
                        self.spans(&p, c);
                    }
                }
                BlockKind::Audio { resource, caption } => {
                    self.resource(&p, resource, &[ResourceKind::Audio]);
                    if let Some(c) = caption {
                        self.spans(&p, c);
                    }
                }
                BlockKind::Divider { .. } | BlockKind::PageBreak => {}
                BlockKind::Space { lines } => {
                    if !(0.25..=40.0).contains(lines) {
                        self.err("E_IR_RANGE", &p, "space must be between 0.25 and 40 lines");
                    }
                }
                BlockKind::Box { title, blocks, .. } => {
                    if let Some(t) = title {
                        self.text(&p, t);
                    }
                    self.blocks(&format!("{p}/blocks"), blocks, depth + 1, false);
                }
                BlockKind::Button { label, actions } => {
                    self.text(&p, label);
                    if label.trim().is_empty() {
                        self.err("E_IR_TEXT", &p, "button label is empty");
                    }
                    self.actions(&format!("{p}/actions"), actions);
                }
                BlockKind::Choice { prompt, options } => {
                    self.spans(&p, prompt);
                    if options.is_empty() || options.len() > limits::MAX_OPTIONS {
                        self.err("E_IR_RANGE", &p, "a choice needs 1 to 12 options");
                    }
                    for (j, o) in options.iter().enumerate() {
                        let op = format!("{p}/options/{j}");
                        self.text(&op, &o.label);
                        self.when(&op, &o.when);
                        self.actions(&format!("{op}/actions"), &o.actions);
                    }
                }
                BlockKind::Quiz {
                    question,
                    answers,
                    explanation,
                    on_correct,
                } => {
                    self.spans(&p, question);
                    if answers.len() < 2 || answers.len() > limits::MAX_OPTIONS {
                        self.err("E_IR_RANGE", &p, "a quiz needs 2 to 12 answers");
                    }
                    if !answers.iter().any(|a| a.correct) {
                        self.err("E_IR_QUIZ", &p, "a quiz needs at least one correct answer");
                    }
                    for a in answers {
                        self.text(&p, &a.label);
                    }
                    if let Some(e) = explanation {
                        self.spans(&p, e);
                    }
                    self.actions(&format!("{p}/on_correct"), on_correct);
                }
                BlockKind::Group { when, blocks } => {
                    self.when(&p, when);
                    self.blocks(&format!("{p}/blocks"), blocks, depth + 1, in_page);
                }
                BlockKind::Page { background, blocks } => {
                    if in_page || depth != 0 {
                        self.err(
                            "E_IR_FRAME",
                            &p,
                            "a fixed page must be a top-level block of a chapter",
                        );
                    }
                    if b.frame.is_some() {
                        self.err("E_IR_FRAME", &p, "a fixed page cannot itself have a frame");
                    }
                    if let Some(c) = background {
                        self.color(&p, c);
                    }
                    for child in blocks {
                        if matches!(child.kind, BlockKind::Page { .. } | BlockKind::PageBreak) {
                            self.err(
                                "E_IR_FRAME",
                                &p,
                                "fixed pages cannot contain pages or page breaks",
                            );
                        }
                    }
                    self.blocks(&format!("{p}/blocks"), blocks, depth + 1, true);
                }
            }
        }
    }
}

/// `https:`, `http:` and `mailto:` only; no whitespace or control characters.
pub fn is_safe_url(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:"))
        && href.len() <= 2048
        && !href.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Host part of an `https://` URL, lower-cased, without port. `None` if the URL
/// is not a plain https URL (userinfo is rejected).
pub fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = authority.rsplit_once(':').map_or(authority, |(h, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            authority
        }
    });
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    ok.then(|| host.to_ascii_lowercase())
}

/// Validate a whole book. Returns every problem found (bounded to 200).
pub fn validate(book: &Book) -> Result<(), Vec<IrError>> {
    let mut cx = Ctx {
        book,
        errors: Vec::new(),
        resources: BTreeMap::new(),
        vars: BTreeMap::new(),
        block_ids: BTreeSet::new(),
        block_count: 0,
    };

    if let Err(e) = check_version(&book.ir_version) {
        return Err(vec![e]);
    }

    // --- metadata -----------------------------------------------------------
    let m = &book.meta;
    if m.title.trim().is_empty() {
        cx.err("E_IR_META", "meta/title", "the book has no title");
    }
    if m.authors.is_empty() || m.authors.iter().any(|a| a.trim().is_empty()) {
        cx.err(
            "E_IR_META",
            "meta/authors",
            "the book needs at least one author",
        );
    }
    if m.language.trim().is_empty() || m.language.len() > 35 {
        cx.err(
            "E_IR_META",
            "meta/language",
            "the book needs a BCP 47 language tag",
        );
    }
    if m.id.trim().is_empty() || m.id.len() > 200 {
        cx.err(
            "E_IR_META",
            "meta/id",
            "the book needs an id of at most 200 characters",
        );
    }
    if !(m.page_aspect.is_finite() && (0.4..=2.5).contains(&m.page_aspect)) {
        cx.err(
            "E_IR_RANGE",
            "meta/page_aspect",
            "page aspect (width/height) must be between 0.4 and 2.5",
        );
    }
    for s in [&m.title, &m.id, &m.language, &m.generator]
        .into_iter()
        .chain(m.authors.iter())
    {
        cx.text("meta", s);
    }
    for s in [
        &m.subtitle,
        &m.edition,
        &m.version,
        &m.publisher,
        &m.published,
        &m.description,
        &m.rights,
    ]
    .into_iter()
    .flatten()
    {
        cx.text("meta", s);
    }
    for h in &m.network {
        let ok = !h.is_empty()
            && h.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !ok {
            cx.err(
                "E_IR_NETWORK",
                "meta/network",
                format!("`{h}` is not a plain host name"),
            );
        }
    }
    if m.offline && !m.network.is_empty() {
        cx.err(
            "E_IR_NETWORK",
            "meta/network",
            "an offline book cannot list network hosts",
        );
    }

    // --- resources ----------------------------------------------------------
    if book.resources.len() > limits::MAX_RESOURCES {
        cx.err("E_IR_LIMIT", "resources", "too many resources");
    }
    for (i, r) in book.resources.iter().enumerate() {
        let p = format!("resources/{i}");
        if !is_ident(&r.name) {
            cx.err(
                "E_IR_ID",
                &p,
                format!("`{}` is not a valid resource name", r.name),
            );
        }
        if cx.resources.insert(&r.name, r).is_some() {
            cx.err("E_IR_ID", &p, format!("duplicate resource `{}`", r.name));
        }
        match &r.source {
            ResourceSource::Packaged { entry } => {
                if entry.is_empty()
                    || entry.starts_with('/')
                    || entry.contains('\\')
                    || entry
                        .split('/')
                        .any(|s| s.is_empty() || s == "." || s == "..")
                {
                    cx.err(
                        "E_IR_PATH",
                        &p,
                        format!("`{entry}` is not a safe package entry name"),
                    );
                }
                if r.sha256.len() != 64 || !r.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
                    cx.err("E_IR_HASH", &p, "packaged resources need a SHA-256 digest");
                }
            }
            ResourceSource::Remote { url } => {
                if m.offline {
                    cx.err(
                        "E_IR_NETWORK",
                        &p,
                        "an offline book cannot use remote resources",
                    );
                }
                match https_host(url) {
                    Some(host) if m.network.iter().any(|h| h.eq_ignore_ascii_case(&host)) => {}
                    Some(host) => cx.err(
                        "E_IR_NETWORK",
                        &p,
                        format!("host `{host}` is not in the book's network allow-list"),
                    ),
                    None => cx.err(
                        "E_IR_NETWORK",
                        &p,
                        "remote resources must use plain https:// URLs",
                    ),
                }
                if !matches!(r.kind, ResourceKind::Video | ResourceKind::Audio) {
                    cx.err("E_IR_NETWORK", &p, "only video and audio can be remote");
                }
            }
        }
        for s in [&r.alt, &r.title].into_iter().flatten() {
            cx.text(&p, s);
        }
    }
    for (i, r) in book.resources.iter().enumerate() {
        let p = format!("resources/{i}");
        if let Some(poster) = &r.poster {
            cx.resource(&p, poster, &[ResourceKind::Image]);
        }
        for t in &r.captions {
            cx.resource(&p, &t.resource, &[ResourceKind::Captions]);
        }
    }

    // --- state --------------------------------------------------------------
    if book.state.len() > limits::MAX_STATE_VARS {
        cx.err("E_IR_LIMIT", "state", "too many state variables");
    }
    for (i, v) in book.state.iter().enumerate() {
        let p = format!("state/{i}");
        if !is_ident(&v.name) {
            cx.err(
                "E_IR_ID",
                &p,
                format!("`{}` is not a valid variable name", v.name),
            );
        }
        if cx.vars.insert(&v.name, v).is_some() {
            cx.err(
                "E_IR_ID",
                &p,
                format!("duplicate state variable `{}`", v.name),
            );
        }
        match &v.ty {
            StateType::Int { initial, min, max } => {
                if min > max
                    || initial < min
                    || initial > max
                    || *min < limits::INT_MIN
                    || *max > limits::INT_MAX
                {
                    cx.err(
                        "E_IR_RANGE",
                        &p,
                        "integer range is empty, too large, or does not contain the initial value",
                    );
                }
            }
            StateType::Bool { .. } => {}
            StateType::Enum { initial, values } => {
                let unique: BTreeSet<_> = values.iter().collect();
                if values.is_empty()
                    || values.len() > limits::MAX_ENUM_VALUES
                    || unique.len() != values.len()
                {
                    cx.err("E_IR_RANGE", &p, "an enum needs 1 to 64 distinct values");
                }
                if !values.contains(initial) {
                    cx.err(
                        "E_IR_RANGE",
                        &p,
                        "the initial value is not one of the enum values",
                    );
                }
            }
        }
    }

    // --- theme and covers ---------------------------------------------------
    let t = &book.theme;
    for (name, c) in [("paper", &t.paper), ("ink", &t.ink), ("accent", &t.accent)] {
        cx.color(&format!("theme/{name}"), c);
    }
    if let Some(d) = &t.dark {
        for c in [&d.paper, &d.ink, &d.accent] {
            cx.color("theme/dark", c);
        }
    }
    for (name, f) in [
        ("body_font", &t.body_font),
        ("heading_font", &t.heading_font),
    ] {
        if let FontRef::Resource { name: r } = f {
            cx.resource(&format!("theme/{name}"), r, &[ResourceKind::Font]);
        }
    }
    let f = &book.cover.front;
    for c in [&f.color, &f.ink, &f.accent] {
        cx.color("cover/front", c);
    }
    if f.title.trim().is_empty() {
        cx.err(
            "E_IR_COVER",
            "cover/front/title",
            "the front cover has no title",
        );
    }
    cx.text("cover/front", &f.title);
    cx.text("cover/front", &f.author);
    if let Some(img) = &f.image {
        cx.resource("cover/front/image", img, &[ResourceKind::Image]);
    } else if !f.show_text {
        cx.err(
            "E_IR_COVER",
            "cover/front",
            "a cover without an image must show its lettering",
        );
    }
    let b = &book.cover.back;
    for c in [&b.color, &b.ink, &b.accent] {
        cx.color("cover/back", c);
    }
    if let Some(img) = &b.image {
        cx.resource("cover/back/image", img, &[ResourceKind::Image]);
    }
    for para in &b.blurb {
        cx.spans("cover/back/blurb", para);
    }

    // --- chapters -----------------------------------------------------------
    if book.chapters.is_empty() || book.chapters.len() > limits::MAX_CHAPTERS {
        cx.err(
            "E_IR_LIMIT",
            "chapters",
            "a book needs between 1 and 4096 chapters",
        );
    }
    if !book
        .chapters
        .iter()
        .any(|c| c.kind == ChapterKind::Chapter && c.when.is_none())
    {
        cx.err(
            "E_IR_EMPTY",
            "chapters",
            "a book needs at least one unconditional authored chapter",
        );
    }
    let mut chapter_ids = BTreeSet::new();
    for (i, c) in book.chapters.iter().enumerate() {
        let p = format!("chapters/{i}");
        if !is_ident(&c.id) {
            cx.err(
                "E_IR_ID",
                &p,
                format!("`{}` is not a valid chapter id", c.id),
            );
        }
        if !chapter_ids.insert(c.id.as_str()) {
            cx.err("E_IR_ID", &p, format!("duplicate chapter id `{}`", c.id));
        }
        cx.text(&p, &c.title);
        cx.when(&p, &c.when);
        if c.kind != ChapterKind::Chapter && (!c.blocks.is_empty() || c.when.is_some()) {
            cx.err(
                "E_IR_FRONT",
                &p,
                "generated front matter cannot carry blocks or conditions",
            );
        }
        if book.meta.layout == Layout::Fixed
            && c.kind == ChapterKind::Chapter
            && c.blocks
                .iter()
                .any(|b| !matches!(b.kind, BlockKind::Page { .. }))
        {
            cx.err(
                "E_IR_FRAME",
                &p,
                "in a fixed-layout book every top-level block of a chapter must be a page",
            );
        }
        cx.blocks(&format!("{p}/blocks"), &c.blocks, 0, false);
    }

    // --- anchors and toc ----------------------------------------------------
    for (name, a) in &book.anchors {
        let p = format!("anchors/{name}");
        if RESERVED_TARGETS.contains(&name.as_str()) {
            cx.err("E_IR_ID", &p, format!("`{name}` is a reserved target name"));
        }
        if !chapter_ids.contains(a.chapter.as_str()) {
            cx.err(
                "E_IR_REF",
                &p,
                format!("anchor points at unknown chapter `{}`", a.chapter),
            );
        }
        if let Some(b) = &a.block
            && !cx.block_ids.contains(b.as_str())
        {
            cx.err(
                "E_IR_REF",
                &p,
                format!("anchor points at unknown block `{b}`"),
            );
        }
    }
    for (i, e) in book.toc.iter().enumerate() {
        let p = format!("toc/{i}");
        cx.target(&p, &e.target);
        cx.text(&p, &e.title);
        cx.when(&p, &e.when);
        if !(1..=2).contains(&e.level) {
            cx.err("E_IR_RANGE", &p, "toc level must be 1 or 2");
        }
    }

    if cx.errors.is_empty() {
        Ok(())
    } else {
        Err(cx.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_policy() {
        assert!(check_version("1.0").is_ok());
        assert!(check_version("1.7").is_ok());
        assert!(
            check_version("2.0")
                .unwrap_err()
                .message
                .contains("Update the reader")
        );
        assert!(check_version("0.9").is_err());
        assert!(check_version("1").is_err());
        assert!(check_version("").is_err());
        assert!(check_version("1.0.0").is_err());
    }

    #[test]
    fn url_rules() {
        assert!(is_safe_url("https://example.org/a?b=c"));
        assert!(is_safe_url("mailto:someone@example.org"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("data:text/html,x"));
        assert!(!is_safe_url("https://exa mple.org"));
        assert_eq!(
            https_host("https://Media.Example.org:8443/x.mp4").as_deref(),
            Some("media.example.org")
        );
        assert_eq!(https_host("https://user@evil.org/x"), None);
        assert_eq!(https_host("http://example.org/x"), None);
    }
}
