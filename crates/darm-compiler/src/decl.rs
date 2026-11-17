//! Lowering of top-level declarations: `book`, `theme`, `assets`, `state`,
//! `cover`, `back_cover` and `chapter`.

use darm_ir::validate::{https_host, limits};
use darm_ir::{self as ir, BlockKind, ResourceKind, StateType};
use darm_lang::ast::{AssignOp, Element, Expr, ExprKind, Ident, Stmt, StmtKind, UnaryOp};
use darm_lang::{Span, suggest};

use crate::PackFile;
use crate::assets::{self, LARGE_ASSET_BYTES};
use crate::lower::{AssetDecl, Bx, Env, Lowerer, VarDecl, kind_name};
use crate::project::{dir_of, join_rel};
use crate::spec::{self, Param};

/// `name = value` properties of a block, validated against a table.
pub(crate) struct Props<'e> {
    values: Vec<(&'e Ident, &'e Expr)>,
    pub children: Vec<&'e Element>,
}

impl<'e> Props<'e> {
    pub fn get(&self, name: &str) -> Option<&'e Expr> {
        self.values
            .iter()
            .find(|(k, _)| k.name == name)
            .map(|(_, v)| *v)
    }
}

/// Everything `book` contributes besides [`ir::Meta`].
pub(crate) struct BookDecl {
    pub meta: ir::Meta,
    pub front_matter: Vec<&'static str>,
}

/// Cloth, lettering and foil colours for generated covers.
const CLOTHS: &[(&str, &str, &str)] = &[
    ("#7a2e22", "#f4ead5", "#d9b46a"),
    ("#1f3a4d", "#f1ead9", "#c9a45c"),
    ("#2f4a3a", "#f2ecd8", "#d1b06b"),
    ("#3b2f4a", "#efe6d6", "#c8a96a"),
    ("#4a3728", "#f3e9d4", "#cfa55b"),
    ("#243b3a", "#f0ead8", "#d6b56e"),
    ("#5a2a3a", "#f5ebd9", "#d7b26a"),
    ("#2b2b2e", "#efe9dc", "#c2a05a"),
    ("#16424a", "#f3ecda", "#e0b867"),
    ("#6b3f1d", "#f6ecd6", "#e2bd73"),
];

fn language_label(tag: &str) -> String {
    match tag
        .split('-')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "en" => "English".into(),
        "es" => "Español".into(),
        "fr" => "Français".into(),
        "de" => "Deutsch".into(),
        "pt" => "Português".into(),
        "it" => "Italiano".into(),
        "ca" => "Català".into(),
        "nl" => "Nederlands".into(),
        "ja" => "日本語".into(),
        "zh" => "中文".into(),
        _ => tag.to_string(),
    }
}

fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let primary = parts.next().unwrap_or("");
    (2..=3).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
        && tag.len() <= 35
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && (1..=12).contains(&s[5..7].parse::<u32>().unwrap_or(0))
        && (1..=31).contains(&s[8..10].parse::<u32>().unwrap_or(0))
}

impl<'a> Lowerer<'a> {
    pub(crate) fn props<'e>(
        &mut self,
        el: &'e Element,
        table: &[Param],
        children: &[&str],
    ) -> Props<'e> {
        let block = el.name.name.as_str();
        let mut props = Props {
            values: Vec::new(),
            children: Vec::new(),
        };
        let Some(body) = &el.body else {
            self.err(
                "E0313",
                el.name.span,
                format!("`{block}` needs an indented block of properties"),
            )
            .help = Some(format!(
                "write `{block}:` and put `name = value` lines below it"
            ));
            return props;
        };
        for stmt in &body.stmts {
            match &stmt.kind {
                StmtKind::Assign {
                    target,
                    op: AssignOp::Set,
                    value,
                } => {
                    if !table.iter().any(|p| p.name == target.name) {
                        let help = match suggest(&target.name, table.iter().map(|p| p.name)) {
                            Some(s) => format!("did you mean `{s}`?"),
                            None => format!(
                                "`{block}` properties: {}",
                                table.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")
                            ),
                        };
                        self.err(
                            "E0303",
                            target.span,
                            format!("`{block}` has no property `{}`", target.name),
                        )
                        .help = Some(help);
                    } else if props.get(&target.name).is_some() {
                        self.err(
                            "E0306",
                            target.span,
                            format!("property `{}` is set twice", target.name),
                        );
                    } else {
                        props.values.push((target, value));
                    }
                }
                StmtKind::Assign { op, .. } => {
                    self.err(
                        "E0302",
                        stmt.span,
                        format!(
                            "`{}` is not valid in `{block}`; properties use `=`",
                            op.symbol()
                        ),
                    );
                }
                StmtKind::Element(child) if children.contains(&child.name.name.as_str()) => {
                    props.children.push(child)
                }
                StmtKind::Element(child) => {
                    let name = &child.name.name;
                    let help = if table.iter().any(|p| p.name == *name) {
                        format!("properties are written with `=`: `{name} = ...`")
                    } else if children.is_empty() {
                        format!("`{block}` contains only `name = value` properties")
                    } else {
                        format!(
                            "`{block}` contains `name = value` properties and `{}` statements",
                            children.join("`, `")
                        )
                    };
                    self.err(
                        "E0302",
                        child.name.span,
                        format!("`{name}` is not allowed inside `{block}`"),
                    )
                    .help = Some(help);
                }
                _ => {
                    self.err(
                        "E0302",
                        stmt.span,
                        format!("only properties are allowed inside `{block}`"),
                    );
                }
            }
        }
        for p in table.iter().filter(|p| p.required) {
            if props.get(p.name).is_none() {
                self.err(
                    "E0308",
                    el.name.span,
                    format!("`{block}` needs the property `{}`", p.name),
                )
                .help = Some(format!("add `{} = ...` ({})", p.name, p.doc));
            }
        }
        props
    }

    fn opt_string(&mut self, props: &Props, name: &str) -> Option<String> {
        let e = props.get(name)?;
        let s = self.string(e)?;
        if s.trim().is_empty() {
            self.err("E0312", e.span, format!("`{name}` is empty"));
            return None;
        }
        Some(s.trim().to_string())
    }

    fn string_list(&mut self, e: &Expr, what: &str) -> Option<Vec<String>> {
        match &e.kind {
            ExprKind::Str { .. } => self.string(e).map(|s| vec![s]),
            ExprKind::List(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.push(self.string(i)?);
                }
                Some(out)
            }
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!(
                        "expected {what} as a string or a list of strings, found {}",
                        e.describe()
                    ),
                );
                None
            }
        }
    }

    pub(crate) fn book(&mut self, el: &Element) -> Option<BookDecl> {
        let spec = spec::element("book").unwrap();
        let empty = Env::new();
        let args = self.args(&el.name, &el.args, spec, &empty);
        let complete = self.require(&el.name, spec, &args);
        let props = self.props(el, spec::BOOK_PROPS, &["credit"]);
        let title = args
            .get(spec, "title")
            .and_then(|t| self.string(t))
            .map(|t| t.trim().to_string());
        if let (Some(t), Some(e)) = (&title, args.get(spec, "title"))
            && t.is_empty()
        {
            self.err("E0312", e.span, "the book title is empty");
        }

        let authors = props.get("author").and_then(|e| {
            let list = self.string_list(e, "the author")?;
            if list.is_empty() || list.iter().any(|a| a.trim().is_empty()) {
                self.err("E0312", e.span, "author names cannot be empty");
                return None;
            }
            Some(
                list.into_iter()
                    .map(|a| a.trim().to_string())
                    .collect::<Vec<_>>(),
            )
        });
        let language = props.get("language").and_then(|e| {
            let tag = self.string(e)?;
            if !is_language_tag(&tag) {
                self.err(
                    "E0312",
                    e.span,
                    format!("`{tag}` is not a BCP 47 language tag"),
                )
                .help = Some("examples: \"en\", \"es\", \"pt-BR\"".into());
                return None;
            }
            Some(tag)
        });
        let layout = match props.get("layout") {
            Some(e) => match self.word(e, spec::LAYOUTS, "layout") {
                Some("fixed") => ir::Layout::Fixed,
                _ => ir::Layout::Reflow,
            },
            None => ir::Layout::Reflow,
        };
        let mut page_aspect = if layout == ir::Layout::Fixed {
            0.75
        } else {
            2.0 / 3.0
        };
        if let Some(e) = props.get("page_size") {
            match &e.kind {
                ExprKind::Tuple(items) if items.len() == 2 => {
                    if let (Some(w), Some(h)) = (self.number(&items[0]), self.number(&items[1])) {
                        let aspect = w / h;
                        if w > 0.0 && h > 0.0 && (0.4..=2.5).contains(&aspect) {
                            page_aspect = aspect;
                        } else {
                            self.err(
                                "E0312",
                                e.span,
                                "page proportions (width / height) must be between 0.4 and 2.5",
                            );
                        }
                    }
                }
                _ => {
                    self.err(
                        "E0307",
                        e.span,
                        format!("expected `(width, height)`, found {}", e.describe()),
                    );
                }
            }
        }
        let offline = props
            .get("offline")
            .and_then(|e| self.boolean(e))
            .unwrap_or(true);
        let mut network = Vec::new();
        if let Some(e) = props.get("network") {
            for host in self.string_list(e, "`network`").unwrap_or_default() {
                let host = host.trim().to_ascii_lowercase();
                let plain = !host.is_empty()
                    && host
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
                if !plain {
                    self.err("E0312", e.span, format!("`{host}` is not a host name")).help =
                        Some("list bare host names such as \"media.example.org\" (no scheme, path or port)".into());
                } else if !network.contains(&host) {
                    network.push(host);
                }
            }
            if offline && !network.is_empty() {
                self.err(
                    "E0406",
                    e.span,
                    "`network` hosts are listed but the book is offline",
                )
                .help = Some(
                    "add `offline = false` to allow remote assets, or remove `network`".into(),
                );
                network.clear();
            }
        }
        let mut front_matter: Vec<&'static str> = spec::FRONT_MATTER.to_vec();
        if let Some(e) = props.get("front_matter") {
            match &e.kind {
                ExprKind::List(items) => {
                    front_matter.clear();
                    for i in items {
                        if let Some(w) = self.word(i, spec::FRONT_MATTER, "front matter part")
                            && !front_matter.contains(&w)
                        {
                            front_matter.push(w);
                        }
                    }
                    front_matter.sort_by_key(|w| spec::FRONT_MATTER.iter().position(|f| f == w));
                }
                _ => {
                    self.err(
                        "E0307",
                        e.span,
                        "expected a list such as `[title, contents]`",
                    );
                }
            }
        }
        if let Some(e) = props.get("typography") {
            self.smart = self.word(e, &["smart", "plain"], "typography mode") != Some("plain");
        }
        let published = props.get("published").and_then(|e| {
            let d = self.string(e)?;
            if !is_date(&d) {
                self.err("E0312", e.span, format!("`{d}` is not a date"))
                    .help = Some("write dates as \"YYYY-MM-DD\"".into());
                return None;
            }
            Some(d)
        });
        let mut credits = Vec::new();
        let credit_spec = spec::element("credit").unwrap();
        for c in &props.children {
            self.body_rule(c, credit_spec);
            let a = self.args(&c.name, &c.args, credit_spec, &empty);
            if self.require(&c.name, credit_spec, &a)
                && let (Some(role), Some(name)) = (
                    self.string(a.get(credit_spec, "role").unwrap()),
                    self.string(a.get(credit_spec, "name").unwrap()),
                )
            {
                credits.push(ir::Credit { role, name });
            }
        }
        let subtitle = self.opt_string(&props, "subtitle");
        let edition = self.opt_string(&props, "edition");
        let version = self.opt_string(&props, "version");
        let publisher = self.opt_string(&props, "publisher");
        let description = self.opt_string(&props, "description");
        let rights = self.opt_string(&props, "rights");
        let explicit_id = props.get("id").and_then(|e| {
            let id = self.string(e)?;
            let ok = !id.is_empty()
                && id.len() <= 200
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
            if !ok {
                self.err(
                    "E0312",
                    e.span,
                    "the book id may contain letters, digits, `.`, `-`, `_` and `:` (max 200)",
                )
                .help = Some("a reverse-domain id works well, e.g. \"org.example.my-book\"".into());
                return None;
            }
            Some(id)
        });

        if !complete {
            return None;
        }
        let (title, authors, language) = (title?, authors?, language?);
        if title.is_empty() {
            return None;
        }
        let id = explicit_id.unwrap_or_else(|| {
            format!(
                "darm-{}",
                &assets::sha256_hex(format!("{title}\n{}", authors.join("\n")).as_bytes())[..20]
            )
        });
        self.layout = layout;
        self.language = language.clone();
        self.offline = offline;
        self.network = network.clone();
        Some(BookDecl {
            meta: ir::Meta {
                id,
                title,
                subtitle,
                authors,
                language,
                edition,
                version,
                publisher,
                published,
                description,
                rights,
                credits,
                layout,
                page_aspect,
                offline,
                network,
                generator: format!("darm {}", env!("CARGO_PKG_VERSION")),
            },
            front_matter,
        })
    }

    pub(crate) fn theme(&mut self, el: Option<&Element>) -> ir::Theme {
        let mut theme = ir::Theme {
            paper: "#f7f3ea".into(),
            ink: "#1f1c18".into(),
            accent: "#8c3b2a".into(),
            dark: None,
            body_font: ir::FontRef::Builtin {
                family: ir::BuiltinFont::Serif,
            },
            heading_font: ir::FontRef::Builtin {
                family: ir::BuiltinFont::Serif,
            },
            paragraph: ir::ParagraphStyle::Indent,
            justify: true,
            hyphenate: true,
            drop_caps: false,
        };
        let Some(el) = el else { return theme };
        let spec = spec::element("theme").unwrap();
        let empty = Env::new();
        self.args(&el.name, &el.args, spec, &empty);
        let props = self.props(el, spec::THEME_PROPS, &[]);
        for (name, slot) in [
            ("paper", &mut theme.paper),
            ("ink", &mut theme.ink),
            ("accent", &mut theme.accent),
        ] {
            if let Some(c) = props.get(name).and_then(|e| self.color(e)) {
                *slot = c;
            }
        }
        let dark: Vec<Option<String>> = ["dark_paper", "dark_ink", "dark_accent"]
            .iter()
            .map(|n| props.get(n).and_then(|e| self.color(e)))
            .collect();
        let given = ["dark_paper", "dark_ink", "dark_accent"]
            .iter()
            .filter(|n| props.get(n).is_some())
            .count();
        match given {
            0 => {}
            3 => {
                if let [Some(paper), Some(ink), Some(accent)] = &dark[..] {
                    theme.dark = Some(ir::Palette {
                        paper: paper.clone(),
                        ink: ink.clone(),
                        accent: accent.clone(),
                    });
                }
            }
            _ => {
                self.err("E0308", el.name.span, "dark-mode colours are incomplete")
                    .help = Some(
                    "give all three of `dark_paper`, `dark_ink` and `dark_accent`, or none".into(),
                );
            }
        }
        let font = |me: &mut Self, name: &str| -> Option<ir::FontRef> {
            let e = props.get(name)?;
            let word = match &e.kind {
                ExprKind::Name(n) => n.clone(),
                ExprKind::Str { value, .. } => value.clone(),
                _ => {
                    me.err(
                        "E0307",
                        e.span,
                        format!(
                            "expected serif, sans, mono or a font asset name, found {}",
                            e.describe()
                        ),
                    );
                    return None;
                }
            };
            Some(match word.as_str() {
                "serif" => ir::FontRef::Builtin {
                    family: ir::BuiltinFont::Serif,
                },
                "sans" => ir::FontRef::Builtin {
                    family: ir::BuiltinFont::Sans,
                },
                "mono" => ir::FontRef::Builtin {
                    family: ir::BuiltinFont::Mono,
                },
                _ => ir::FontRef::Resource {
                    name: me.asset(e, ResourceKind::Font)?,
                },
            })
        };
        if let Some(f) = font(self, "body_font") {
            theme.heading_font = f.clone();
            theme.body_font = f;
        }
        if let Some(f) = font(self, "heading_font") {
            theme.heading_font = f;
        }
        if let Some(e) = props.get("paragraph")
            && self.word(e, &["indent", "spaced"], "paragraph style") == Some("spaced")
        {
            theme.paragraph = ir::ParagraphStyle::Spaced;
        }
        for (name, slot) in [
            ("justify", &mut theme.justify),
            ("hyphenate", &mut theme.hyphenate),
            ("drop_caps", &mut theme.drop_caps),
        ] {
            if let Some(v) = props.get(name).and_then(|e| self.boolean(e)) {
                *slot = v;
            }
        }
        theme
    }

    // --- assets -------------------------------------------------------------

    /// Inspect a packaged file and register it as a resource + pack file.
    fn packaged(
        &mut self,
        name: &str,
        path_expr: &Expr,
        kind: ResourceKind,
    ) -> Option<ir::Resource> {
        let path = self.string(path_expr)?;
        let base = &self.file_dirs[path_expr.span.file.0 as usize];
        let rel = match join_rel(base, &path) {
            Ok(rel) => rel,
            Err(reason) => {
                self.err("E0402", path_expr.span, format!("invalid path `{path}`: {reason}")).help =
                    Some("asset paths are relative to the source file and must stay inside the project directory".into());
                return None;
            }
        };
        let mut reader = match self.fs.open(&rel) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let dir = if dir_of(&rel).is_empty() {
                    "the project directory".to_string()
                } else {
                    format!("`{}/`", dir_of(&rel))
                };
                self.err("E0401", path_expr.span, format!("file not found: `{rel}`"))
                    .help = Some(format!(
                    "the path is relative to the source file; expected the file in {dir}"
                ));
                return None;
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                self.err("E0402", path_expr.span, format!("`{rel}`: {e}"));
                return None;
            }
            Err(e) => {
                self.err("E0404", path_expr.span, format!("cannot open `{rel}`: {e}"));
                return None;
            }
        };
        let info = match assets::inspect(&mut *reader, kind) {
            Ok(info) => info,
            Err(e) => {
                self.err(e.code, path_expr.span, format!("`{rel}`: {}", e.message))
                    .help = e.help;
                return None;
            }
        };
        drop(reader);
        if info.size > LARGE_ASSET_BYTES {
            self.warn(
                "W0606",
                path_expr.span,
                format!("`{rel}` is {} MiB", info.size >> 20),
            );
        }
        let entry = format!("res/{name}{}", info.ext);
        self.files.push(PackFile {
            entry: entry.clone(),
            media_type: info.media_type.to_string(),
            source: rel.clone(),
            path: self.fs.disk_path(&rel),
            size: info.size,
            sha256: info.sha256.clone(),
        });
        Some(ir::Resource {
            name: name.to_string(),
            kind,
            source: ir::ResourceSource::Packaged { entry },
            media_type: info.media_type.to_string(),
            size: info.size,
            sha256: info.sha256,
            width: info.width,
            height: info.height,
            duration_ms: info.duration_ms,
            alt: None,
            title: None,
            poster: None,
            captions: Vec::new(),
            transcript: None,
        })
    }

    fn remote(&mut self, name: &str, url_expr: &Expr, kind: ResourceKind) -> Option<ir::Resource> {
        let url = self.string(url_expr)?;
        if !matches!(kind, ResourceKind::Video | ResourceKind::Audio) {
            self.err("E0406", url_expr.span, "only video and audio can be remote")
                .help =
                Some("images, fonts and captions are always packaged inside the book".into());
            return None;
        }
        let Some(host) = https_host(&url) else {
            self.err(
                "E0406",
                url_expr.span,
                format!("`{url}` is not a plain https:// URL"),
            )
            .help = Some("remote assets must use https and may not contain credentials".into());
            return None;
        };
        if self.offline {
            self.err(
                "E0406",
                url_expr.span,
                "this book is offline, so it cannot use remote assets",
            )
            .help = Some(format!(
                "package the file instead, or add `offline = false` and `network = [\"{host}\"]` to `book`"
            ));
            return None;
        }
        if !self.network.contains(&host) {
            self.err(
                "E0406",
                url_expr.span,
                format!("host `{host}` is not in the book's `network` list"),
            )
            .help = Some(format!("add \"{host}\" to `network` in `book`"));
            return None;
        }
        let path = url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let media_type = match (kind, path.rsplit('.').next().unwrap_or("")) {
            (ResourceKind::Video, "webm") => "video/webm",
            (ResourceKind::Video, _) => "video/mp4",
            (ResourceKind::Audio, "m4a" | "mp4" | "aac") => "audio/mp4",
            (ResourceKind::Audio, "ogg" | "oga") => "audio/ogg",
            (ResourceKind::Audio, "wav") => "audio/wav",
            (ResourceKind::Audio, _) => "audio/mpeg",
            (_, "png") => "image/png",
            (_, "gif") => "image/gif",
            (_, "webp") => "image/webp",
            (_, "svg") => "image/svg+xml",
            _ => "image/jpeg",
        };
        Some(ir::Resource {
            name: name.to_string(),
            kind,
            source: ir::ResourceSource::Remote { url },
            media_type: media_type.to_string(),
            size: 0,
            sha256: String::new(),
            width: None,
            height: None,
            duration_ms: None,
            alt: None,
            title: None,
            poster: None,
            captions: Vec::new(),
            transcript: None,
        })
    }

    pub(crate) fn assets_block(&mut self, el: &Element) {
        let spec = spec::element("assets").unwrap();
        let empty = Env::new();
        self.args(&el.name, &el.args, spec, &empty);
        if !self.body_rule(el, spec) {
            return;
        }
        for stmt in &el.body.as_ref().unwrap().stmts {
            let StmtKind::Assign {
                target,
                op: AssignOp::Set,
                value,
            } = &stmt.kind
            else {
                self.err("E0302", stmt.span, "`assets:` contains only declarations")
                    .help = Some(
                    "write `name = image(\"path\", alt=\"...\")` (or `video`, `audio`, `font`)"
                        .into(),
                );
                continue;
            };
            let ExprKind::Call {
                name: ctor,
                args: call_args,
            } = &value.kind
            else {
                self.err(
                    "E0307",
                    value.span,
                    format!("expected an asset constructor, found {}", value.describe()),
                )
                .help = Some(format!(
                    "write `{} = image(\"path\", alt=\"...\")` (or `video`, `audio`, `font`)",
                    target.name
                ));
                continue;
            };
            let Some(cspec) = spec::constructor(&ctor.name) else {
                let help = suggest(&ctor.name, spec::CONSTRUCTORS.iter().map(|c| c.name))
                    .map(|s| format!("did you mean `{s}`?"));
                self.err(
                    "E0301",
                    ctor.span,
                    format!("`{}` is not an asset constructor", ctor.name),
                )
                .help = help.or(Some("asset constructors: image, video, audio, font".into()));
                continue;
            };
            if !ir::is_ident(&target.name) || target.name.starts_with('_') {
                self.err(
                    "E0317",
                    target.span,
                    format!("`{}` cannot be used as an asset name", target.name),
                );
                continue;
            }
            if self.assets.contains_key(&target.name) {
                self.err(
                    "E0306",
                    target.span,
                    format!("the asset `{}` is already declared", target.name),
                );
                continue;
            }
            let kind = match ctor.name.as_str() {
                "image" => ResourceKind::Image,
                "video" => ResourceKind::Video,
                "audio" => ResourceKind::Audio,
                _ => ResourceKind::Font,
            };
            let args = self.args(ctor, call_args, cspec, &empty);
            let (path, url) = (args.get(cspec, "path"), args.get(cspec, "url"));
            let alt = args
                .get(cspec, "alt")
                .and_then(|e| self.string(e))
                .filter(|a| !a.trim().is_empty());
            if matches!(kind, ResourceKind::Image | ResourceKind::Video) && alt.is_none() {
                self.err("E0308", ctor.span, format!("`{}(...)` needs `alt=\"...\"`", ctor.name)).help = Some(
                    "describe the content for readers who cannot see it; it is also what search and screen readers use".into(),
                );
            }
            let resource = match (path, url) {
                (Some(p), None) => self.packaged(&target.name, p, kind),
                (None, Some(u)) => self.remote(&target.name, u, kind),
                (Some(_), Some(u)) => {
                    self.err("E0306", u.span, "give either a path or `url=`, not both");
                    None
                }
                (None, None) => {
                    self.err(
                        "E0308",
                        ctor.span,
                        format!("`{}(...)` needs a file path", ctor.name),
                    )
                    .help = Some(format!("write `{}(\"relative/path\", ...)`", ctor.name));
                    None
                }
            };
            // Declare the name even when the file is broken, so that uses of it
            // do not cascade into "unknown asset" errors.
            self.assets.insert(
                target.name.clone(),
                AssetDecl {
                    kind,
                    span: target.span,
                    used: false,
                },
            );
            let Some(mut resource) = resource else {
                continue;
            };
            resource.alt = alt;
            resource.title = args.get(cspec, "title").and_then(|e| self.string(e));
            resource.transcript = args.get(cspec, "transcript").and_then(|e| self.string(e));
            let mut extra = Vec::new();
            if let Some(p) = args.get(cspec, "poster") {
                let poster_name = format!("{}__poster", target.name);
                if let Some(mut poster) = self.packaged(&poster_name, p, ResourceKind::Image) {
                    poster.alt = resource.alt.clone();
                    resource.poster = Some(poster_name);
                    extra.push(poster);
                }
            }
            if let Some(c) = args.get(cspec, "captions") {
                let items: Vec<&Expr> = match &c.kind {
                    ExprKind::List(items) => items.iter().collect(),
                    _ => vec![c],
                };
                for item in items {
                    let Some(path) = self.string(item) else {
                        continue;
                    };
                    let stem = path
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches(".vtt");
                    let lang = stem
                        .rsplit_once('.')
                        .map(|(_, l)| l)
                        .filter(|l| is_language_tag(l))
                        .unwrap_or(&self.language)
                        .to_string();
                    if resource.captions.iter().any(|t| t.lang == lang) {
                        self.err(
                            "E0306",
                            item.span,
                            format!(
                                "there is already a `{lang}` caption track for `{}`",
                                target.name
                            ),
                        )
                        .help =
                            Some("name caption files `<name>.<lang>.vtt`, one per language".into());
                        continue;
                    }
                    let track_name = format!(
                        "{}__cc_{}",
                        target.name,
                        lang.replace('-', "_").to_ascii_lowercase()
                    );
                    if let Some(track) = self.packaged(&track_name, item, ResourceKind::Captions) {
                        resource.captions.push(ir::CaptionTrack {
                            resource: track_name,
                            label: language_label(&lang),
                            lang,
                        });
                        extra.push(track);
                    }
                }
            }
            self.resources.push(resource);
            self.resources.extend(extra);
        }
    }

    // --- state --------------------------------------------------------------

    fn int_literal(e: &Expr) -> Option<i64> {
        match &e.unparen().kind {
            ExprKind::Int(i) => Some(*i),
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr,
            } => Self::int_literal(expr).map(|v| -v),
            _ => None,
        }
    }

    pub(crate) fn state_block(&mut self, el: &Element) {
        let spec = spec::element("state").unwrap();
        let empty = Env::new();
        self.args(&el.name, &el.args, spec, &empty);
        if !self.body_rule(el, spec) {
            return;
        }
        for stmt in &el.body.as_ref().unwrap().stmts {
            let StmtKind::Assign {
                target,
                op: AssignOp::Set,
                value,
            } = &stmt.kind
            else {
                self.err("E0302", stmt.span, "`state:` contains only declarations").help =
                    Some("examples: `courage = 0 in 0..5`, `has_key = false`, `route = \"a\" in [\"a\", \"b\"]`".into());
                continue;
            };
            let name = &target.name;
            if name.starts_with('_') || spec::element(name).is_some() {
                self.err(
                    "E0317",
                    target.span,
                    format!("`{name}` cannot be used as a variable name"),
                );
                continue;
            }
            if self.vars.iter().any(|d| d.var.name == *name) {
                self.err(
                    "E0306",
                    target.span,
                    format!("the state variable `{name}` is already declared"),
                );
                continue;
            }
            if self.vars.len() >= limits::MAX_STATE_VARS {
                self.err(
                    "E0315",
                    target.span,
                    format!("more than {} state variables", limits::MAX_STATE_VARS),
                );
                return;
            }
            let (initial, domain) = match &value.kind {
                ExprKind::In { value, domain } => (value.as_ref(), Some(domain.as_ref())),
                _ => (value, None),
            };
            let ty = match (&initial.unparen().kind, domain.map(|d| &d.kind)) {
                (ExprKind::Bool(b), None) => Some(StateType::Bool { initial: *b }),
                (ExprKind::Bool(_), Some(_)) => {
                    self.err(
                        "E0307",
                        value.span,
                        "a bool variable has no domain; write just `true` or `false`",
                    );
                    None
                }
                (ExprKind::Str { value: s, .. }, Some(ExprKind::List(items))) => {
                    let mut values = Vec::new();
                    let mut ok = true;
                    for i in items {
                        match &i.kind {
                            ExprKind::Str { value, .. } if !values.contains(value) => {
                                values.push(value.clone())
                            }
                            ExprKind::Str { value, .. } => {
                                ok = false;
                                self.err("E0306", i.span, format!("\"{value}\" is listed twice"));
                            }
                            _ => {
                                ok = false;
                                self.err(
                                    "E0307",
                                    i.span,
                                    format!("expected a string, found {}", i.describe()),
                                );
                            }
                        }
                    }
                    if ok && (values.is_empty() || values.len() > limits::MAX_ENUM_VALUES) {
                        ok = false;
                        self.err(
                            "E0315",
                            value.span,
                            format!("a closed set needs 1 to {} values", limits::MAX_ENUM_VALUES),
                        );
                    }
                    if ok && !values.contains(s) {
                        ok = false;
                        self.err(
                            "E0312",
                            initial.span,
                            format!(
                                "the initial value \"{s}\" is not in the list of allowed values"
                            ),
                        );
                    }
                    ok.then(|| StateType::Enum {
                        initial: s.clone(),
                        values,
                    })
                }
                (ExprKind::Str { .. }, _) => {
                    self.err(
                        "E0308",
                        value.span,
                        "text variables need a closed set of values",
                    )
                    .help = Some(format!("write `{name} = \"a\" in [\"a\", \"b\", \"c\"]`"));
                    None
                }
                (_, d) if Self::int_literal(initial).is_some() => {
                    let init = Self::int_literal(initial).unwrap();
                    let range = match d {
                        None => Some((init.min(0), init.max(1_000_000))),
                        Some(ExprKind::Range { lo, hi }) => {
                            match (Self::int_literal(lo), Self::int_literal(hi)) {
                                (Some(lo), Some(hi)) => Some((lo, hi)),
                                _ => {
                                    self.err(
                                        "E0307",
                                        value.span,
                                        "range bounds must be integer literals, e.g. `0..5`",
                                    );
                                    None
                                }
                            }
                        }
                        Some(_) => {
                            self.err(
                                "E0307",
                                value.span,
                                "an int variable needs a range such as `0..5` after `in`",
                            );
                            None
                        }
                    };
                    match range {
                        Some((min, max)) if min > max => {
                            self.err(
                                "E0312",
                                value.span,
                                format!("the range {min}..{max} is empty"),
                            );
                            None
                        }
                        Some((min, max)) if min < limits::INT_MIN || max > limits::INT_MAX => {
                            self.err(
                                "E0315",
                                value.span,
                                "integers are limited to ±1 000 000 000",
                            );
                            None
                        }
                        Some((min, max)) if init < min || init > max => {
                            self.err(
                                "E0312",
                                initial.span,
                                format!("the initial value {init} is outside {min}..{max}"),
                            );
                            None
                        }
                        Some((min, max)) => Some(StateType::Int {
                            initial: init,
                            min,
                            max,
                        }),
                        None => None,
                    }
                }
                _ => {
                    self.err(
                        "E0307",
                        initial.span,
                        format!(
                            "a state variable starts as an integer, `true`/`false` or text, not {}",
                            initial.describe()
                        ),
                    );
                    None
                }
            };
            if let Some(ty) = ty {
                self.vars.push(VarDecl {
                    var: ir::StateVar {
                        name: name.clone(),
                        ty,
                    },
                    span: target.span,
                    read: false,
                });
            }
        }
    }

    // --- covers -------------------------------------------------------------

    pub(crate) fn covers(
        &mut self,
        front: Option<&Element>,
        back: Option<&Element>,
        meta: &ir::Meta,
    ) -> ir::Covers {
        let digest = assets::sha256_hex(meta.id.as_bytes());
        let pick = usize::from_str_radix(&digest[..4], 16).unwrap_or(0) % CLOTHS.len();
        let (cloth, ink, foil) = CLOTHS[pick];
        let authors = meta.authors.join(", ");
        let mut f = ir::FrontCover {
            style: [
                ir::CoverStyle::Classic,
                ir::CoverStyle::Band,
                ir::CoverStyle::Modern,
                ir::CoverStyle::Minimal,
            ][usize::from_str_radix(&digest[4..6], 16).unwrap_or(0) % 4],
            color: cloth.into(),
            ink: ink.into(),
            accent: foil.into(),
            image: None,
            full_bleed: false,
            show_text: true,
            title: meta.title.clone(),
            subtitle: meta.subtitle.clone(),
            author: authors,
            generated: front.is_none(),
        };
        let empty = Env::new();
        if let Some(el) = front {
            let spec = spec::element("cover").unwrap();
            self.args(&el.name, &el.args, spec, &empty);
            let props = self.props(el, spec::COVER_PROPS, &[]);
            f.style = match props
                .get("style")
                .and_then(|e| self.word(e, spec::COVER_STYLES, "cover style"))
            {
                Some("band") => ir::CoverStyle::Band,
                Some("minimal") => ir::CoverStyle::Minimal,
                Some("modern") => ir::CoverStyle::Modern,
                _ => ir::CoverStyle::Classic,
            };
            for (name, slot) in [
                ("color", &mut f.color),
                ("ink", &mut f.ink),
                ("accent", &mut f.accent),
            ] {
                if let Some(c) = props.get(name).and_then(|e| self.color(e)) {
                    *slot = c;
                }
            }
            f.image = props
                .get("image")
                .and_then(|e| self.asset(e, ResourceKind::Image));
            f.full_bleed = props
                .get("full_bleed")
                .and_then(|e| self.boolean(e))
                .unwrap_or(false);
            f.show_text = props
                .get("show_text")
                .and_then(|e| self.boolean(e))
                .unwrap_or(true);
            if let Some(e) = props
                .get("full_bleed")
                .filter(|_| f.full_bleed && props.get("image").is_none())
            {
                self.err("E0308", e.span, "`full_bleed = true` needs an `image`");
            }
            if let Some(e) = props
                .get("show_text")
                .filter(|_| !f.show_text && props.get("image").is_none())
            {
                self.err("E0308", e.span, "a cover without lettering needs an `image`").help =
                    Some("every book must have a recognisable cover: keep `show_text = true` or add `image = ...`".into());
                f.show_text = true;
            }
            if let Some(t) = self.opt_string(&props, "title") {
                f.title = t;
            }
            if let Some(s) = self.opt_string(&props, "subtitle") {
                f.subtitle = Some(s);
            }
            if let Some(a) = self.opt_string(&props, "author") {
                f.author = a;
            }
        }
        let mut b = ir::BackCover {
            color: f.color.clone(),
            ink: f.ink.clone(),
            accent: f.accent.clone(),
            blurb: Vec::new(),
            tagline: None,
            image: None,
            generated: back.is_none(),
        };
        if let Some(el) = back {
            let spec = spec::element("back_cover").unwrap();
            self.args(&el.name, &el.args, spec, &empty);
            let props = self.props(el, spec::BACK_COVER_PROPS, &["text"]);
            for (name, slot) in [
                ("color", &mut b.color),
                ("ink", &mut b.ink),
                ("accent", &mut b.accent),
            ] {
                if let Some(c) = props.get(name).and_then(|e| self.color(e)) {
                    *slot = c;
                }
            }
            b.tagline = self.opt_string(&props, "tagline");
            b.image = props
                .get("image")
                .and_then(|e| self.asset(e, ResourceKind::Image));
            let text_spec = spec::element("text").unwrap();
            for t in &props.children {
                self.body_rule(t, text_spec);
                let a = self.args(&t.name, &t.args, text_spec, &empty);
                if let Some(e) = a.get(text_spec, "text") {
                    if let Some(paragraphs) = self.rich(e, &empty, false, true) {
                        b.blurb.extend(paragraphs);
                    }
                } else {
                    self.require(&t.name, text_spec, &a);
                }
                if let Some((key, _)) = t
                    .args
                    .iter()
                    .filter_map(|a| a.name.as_ref().map(|n| (n, a)))
                    .next()
                {
                    self.err(
                        "E0303",
                        key.span,
                        "back-cover `text` takes only the text itself",
                    );
                }
            }
        }
        if b.blurb.is_empty()
            && let Some(d) = &meta.description
        {
            b.blurb.push(vec![ir::Span::plain(d.clone())]);
        }
        ir::Covers { front: f, back: b }
    }

    // --- chapters -----------------------------------------------------------

    pub(crate) fn chapter(
        &mut self,
        el: &Element,
        ordinal: &mut u32,
        drop_caps: bool,
    ) -> Option<ir::Chapter> {
        let spec = spec::element("chapter").unwrap();
        let empty = Env::new();
        let args = self.args(&el.name, &el.args, spec, &empty);
        if !self.require(&el.name, spec, &args) {
            return None;
        }
        self.body_rule(el, spec);
        let title = self
            .string(args.get(spec, "title").unwrap())?
            .trim()
            .to_string();
        if title.is_empty() {
            self.err("E0312", el.name.span, "the chapter title is empty").help =
                Some("for a chapter without a visible heading use `opener=false`, but still give it a title for the contents".into());
            return None;
        }
        let numbered = args
            .get(spec, "numbered")
            .and_then(|e| self.boolean(e))
            .unwrap_or(true);
        let opener = args
            .get(spec, "opener")
            .and_then(|e| self.boolean(e))
            .unwrap_or(true);
        let listed = args
            .get(spec, "toc")
            .and_then(|e| self.boolean(e))
            .unwrap_or(true);
        let when = args.get(spec, "when").and_then(|e| self.cond(e, &empty));
        let id = match args
            .get(spec, "id")
            .and_then(|e| self.new_id(e).map(|id| (id, e.span)))
        {
            Some((id, span)) => {
                self.chapter_id = id.clone();
                if !self.declare_id(&id, span, None) {
                    self.chapter_id = self.fresh_id("c");
                }
                self.chapter_id.clone()
            }
            None => {
                let id = self.fresh_id("c");
                self.chapter_id = id.clone();
                self.anchors.insert(
                    id.clone(),
                    ir::Anchor {
                        chapter: id.clone(),
                        block: None,
                    },
                );
                id
            }
        };
        let number = numbered.then(|| {
            *ordinal += 1;
            *ordinal
        });
        if listed {
            self.toc.push(ir::TocEntry {
                title: title.clone(),
                target: id.clone(),
                level: 1,
                number,
                when: when.clone(),
            });
        }
        self.conds.clear();
        if let Some(w) = &when {
            self.conds.push(w.clone());
        }
        let stmts: &[Stmt] = el.body.as_ref().map_or(&[], |b| &b.stmts);
        let mut blocks = self.blocks(
            stmts,
            &empty,
            Bx {
                in_page: false,
                in_box: false,
                chapter_top: true,
                depth: 0,
            },
        );
        self.conds.clear();
        if blocks.is_empty() && el.body.is_some() && !self.diags.iter().any(|d| d.is_error()) {
            self.warn(
                "W0605",
                el.name.span,
                format!("the chapter \"{title}\" has no content"),
            );
        }
        if self.layout == ir::Layout::Fixed {
            for (stmt, _) in stmts.iter().zip(0..) {
                let is_page = matches!(&stmt.kind, StmtKind::Element(e) if e.name.name == "page");
                if !is_page {
                    self.err("E0316", stmt.span, "in a `layout = fixed` book every block of a chapter must be a `page`").help =
                        Some("wrap the content in `page:` and position it with `at=` / `size=`, or use `layout = reflow`".into());
                    break;
                }
            }
        }
        if drop_caps
            && opener
            && let Some(ir::Block {
                kind: BlockKind::Paragraph { style, .. },
                ..
            }) = blocks.first_mut()
            && *style == ir::TextStyle::Body
        {
            *style = ir::TextStyle::Dropcap;
        }
        Some(ir::Chapter {
            id,
            kind: ir::ChapterKind::Chapter,
            title,
            number,
            opener,
            when,
            blocks,
        })
    }

    pub(crate) fn finish_warnings(&mut self) {
        let unused_assets: Vec<(String, Span, ResourceKind)> = self
            .assets
            .iter()
            .filter(|(_, d)| !d.used)
            .map(|(n, d)| (n.clone(), d.span, d.kind))
            .collect();
        for (name, span, kind) in unused_assets {
            self.warn(
                "W0603",
                span,
                format!("the {} asset `{name}` is never used", kind_name(kind)),
            )
            .help = Some(
                "it is still packaged; remove the declaration to make the book smaller".into(),
            );
        }
        let unread: Vec<(String, Span)> = self
            .vars
            .iter()
            .filter(|d| !d.read)
            .map(|d| (d.var.name.clone(), d.span))
            .collect();
        for (name, span) in unread {
            self.warn(
                "W0604",
                span,
                format!("the state variable `{name}` is never read"),
            );
        }
        let unused: Vec<(String, Span)> = self
            .components
            .iter()
            .filter(|(_, c)| !c.used)
            .map(|(n, c)| (n.clone(), c.span))
            .collect();
        for (name, span) in unused {
            self.warn(
                "W0604",
                span,
                format!("the component `{name}` is never used"),
            );
        }
    }
}
