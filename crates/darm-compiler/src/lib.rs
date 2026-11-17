//! # darm-compiler
//!
//! Turns a DARM project (`.darml` sources plus assets) into a validated
//! [`darm_ir::Book`] and the list of files to package.
//!
//! ```text
//! entry.darml ─┬─ parse (darm-lang) ─▶ AST ─┬─ resolve includes
//!  includes ───┘                            ├─ declarations: book, theme, assets, state, components
//!                                           ├─ lower chapters: blocks, rich text, expressions, actions
//!                                           ├─ deferred checks: jump targets, unused definitions
//!                                           └─ darm_ir::validate ─▶ Book + PackFile list
//! ```
//!
//! The compiler never executes anything from the source: the language has no
//! loops or calls, components are expanded at compile time with a depth
//! limit, and assets are only inspected, hashed and copied.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use darm_ir as ir;
use darm_lang::ast::{self, ExprKind, Stmt, StmtKind};
use darm_lang::{Diagnostic, Severity, SourceMap, Span, suggest};

pub mod assets;
mod decl;
pub mod explain;
mod lower;
pub mod project;
pub mod richtext;
pub mod spec;

pub use project::{DiskFs, Fs, MemoryFs, ProjectFile};

use lower::{Bx, Component, Lowerer};
use project::{MAX_INCLUDE_DEPTH, dir_of, join_rel};

/// A file that goes into the package next to the IR.
#[derive(Debug, Clone, PartialEq)]
pub struct PackFile {
    /// Entry name inside the package, e.g. `res/tide_chart.svg`.
    pub entry: String,
    pub media_type: String,
    /// Project-relative source path.
    pub source: String,
    /// Absolute path on disk (absent for in-memory projects).
    pub path: Option<PathBuf>,
    pub size: u64,
    pub sha256: String,
}

/// Result of compiling a project.
pub struct Compilation {
    pub sources: SourceMap,
    /// Errors and warnings, in source order per file.
    pub diagnostics: Vec<Diagnostic>,
    /// The IR. `None` when there are errors.
    pub book: Option<ir::Book>,
    /// Files to package. Empty when there are errors.
    pub files: Vec<PackFile>,
}

impl Compilation {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics.len() - self.error_count()
    }

    /// All diagnostics as rustc-style text.
    pub fn render(&self, color: bool) -> String {
        self.diagnostics
            .iter()
            .map(|d| d.render(&self.sources, color))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// All diagnostics as a JSON array.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.diagnostics
                .iter()
                .map(|d| d.to_json(&self.sources))
                .collect(),
        )
    }
}

struct Loaded {
    files: Vec<ast::File>,
    /// Project-relative path of each file.
    paths: Vec<String>,
    /// `(file, statement index)` of an `include` → index of the included file.
    includes: BTreeMap<(usize, usize), usize>,
}

fn load(
    fs: &dyn Fs,
    rel: &str,
    from: Option<Span>,
    chain: &mut Vec<String>,
    sources: &mut SourceMap,
    loaded: &mut Loaded,
    diags: &mut Vec<Diagnostic>,
) -> Option<usize> {
    let fail =
        |diags: &mut Vec<Diagnostic>, code: &'static str, msg: String, help: Option<&str>| {
            let mut d = match from {
                Some(span) => Diagnostic::error(code, span, msg),
                None => Diagnostic::global(Severity::Error, code, msg),
            };
            d.help = help.map(Into::into);
            diags.push(d);
        };
    if chain.iter().any(|c| c == rel) {
        fail(
            diags,
            "E0501",
            format!(
                "`{rel}` includes itself (cycle: {} → {rel})",
                chain.join(" → ")
            ),
            None,
        );
        return None;
    }
    if loaded.paths.iter().any(|p| p == rel) {
        fail(
            diags,
            "E0501",
            format!("`{rel}` is included more than once"),
            Some("each file can be included only once"),
        );
        return None;
    }
    if chain.len() >= MAX_INCLUDE_DEPTH {
        fail(
            diags,
            "E0501",
            format!("includes are nested more than {MAX_INCLUDE_DEPTH} levels deep"),
            None,
        );
        return None;
    }
    let bytes = match fs.read(rel) {
        Ok(b) => b,
        Err(e) => {
            let (code, help) = match e.kind() {
                std::io::ErrorKind::NotFound if from.is_some() => (
                    "E0501",
                    Some("include paths are relative to the including file"),
                ),
                std::io::ErrorKind::PermissionDenied => ("E0402", None),
                _ => ("E0501", None),
            };
            fail(diags, code, format!("cannot read `{rel}`: {e}"), help);
            return None;
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => {
            fail(
                diags,
                "E0107",
                format!("`{rel}` is not valid UTF-8"),
                Some("save the file as UTF-8"),
            );
            return None;
        }
    };
    let id = sources.add(rel, text.clone());
    let parsed = darm_lang::parse(id, &text);
    diags.extend(parsed.errors);
    let index = loaded.files.len();
    debug_assert_eq!(index, id.0 as usize);
    loaded.files.push(parsed.file);
    loaded.paths.push(rel.to_string());

    // Resolve includes now so that every file is loaded exactly once.
    chain.push(rel.to_string());
    let targets: Vec<(usize, Span, Option<String>)> = loaded.files[index]
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match &s.kind {
            StmtKind::Element(el) if el.name.name == "include" => {
                let path = match el.args.as_slice() {
                    [arg] if arg.name.is_none() => match &arg.value.kind {
                        ExprKind::Str { value, .. } => Some(value.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                Some((
                    i,
                    el.args.first().map_or(el.name.span, |a| a.value.span),
                    path,
                ))
            }
            _ => None,
        })
        .collect();
    for (stmt_index, span, path) in targets {
        let Some(path) = path else {
            diags.push(
                Diagnostic::error(
                    "E0501",
                    span,
                    "`include` takes one string: the path of a .darml file",
                )
                .with_help("write `include \"chapters/one.darml\"`"),
            );
            continue;
        };
        match join_rel(dir_of(rel), &path) {
            Ok(target) if target.ends_with(".darml") => {
                if let Some(i) = load(fs, &target, Some(span), chain, sources, loaded, diags) {
                    loaded.includes.insert((index, stmt_index), i);
                }
            }
            Ok(target) => diags.push(
                Diagnostic::error("E0501", span, format!("`{target}` is not a .darml file"))
                    .with_help(
                        "only DARM source files can be included; declare media in `assets:`",
                    ),
            ),
            Err(reason) => diags.push(Diagnostic::error(
                "E0402",
                span,
                format!("invalid include path `{path}`: {reason}"),
            )),
        }
    }
    chain.pop();
    Some(index)
}

fn flatten<'a>(loaded: &'a Loaded, file: usize, out: &mut Vec<&'a Stmt>) {
    for (i, stmt) in loaded.files[file].stmts.iter().enumerate() {
        match &stmt.kind {
            StmtKind::Element(el) if el.name.name == "include" => {
                if let Some(target) = loaded.includes.get(&(file, i)) {
                    flatten(loaded, *target, out);
                }
            }
            _ => out.push(stmt),
        }
    }
}

/// Compile the project whose entry file is `entry` (project-relative).
pub fn compile(fs: &dyn Fs, entry: &str) -> Compilation {
    let mut sources = SourceMap::new();
    let mut diags = Vec::new();
    let mut loaded = Loaded {
        files: Vec::new(),
        paths: Vec::new(),
        includes: BTreeMap::new(),
    };
    let root = load(
        fs,
        entry,
        None,
        &mut Vec::new(),
        &mut sources,
        &mut loaded,
        &mut diags,
    );
    let done = |sources, diagnostics, book, files| Compilation {
        sources,
        diagnostics,
        book,
        files,
    };
    // Semantic analysis on a tree with holes produces misleading follow-up
    // errors, so syntax errors stop the pipeline here.
    let Some(root) = root.filter(|_| diags.is_empty()) else {
        return done(sources, diags, None, Vec::new());
    };
    let mut top = Vec::new();
    flatten(&loaded, root, &mut top);

    let mut lw = Lowerer {
        fs,
        file_dirs: loaded.paths.iter().map(|p| dir_of(p).to_string()).collect(),
        diags,
        smart: true,
        layout: ir::Layout::Reflow,
        language: String::new(),
        offline: true,
        network: Vec::new(),
        assets: BTreeMap::new(),
        resources: Vec::new(),
        files: Vec::new(),
        vars: Vec::new(),
        components: BTreeMap::new(),
        anchors: BTreeMap::new(),
        id_spans: BTreeMap::new(),
        pending_targets: Vec::new(),
        toc: Vec::new(),
        next_id: 0,
        stack: Vec::new(),
        chapter_id: String::new(),
        conds: Vec::new(),
        block_count: 0,
    };

    // --- sort top-level statements -------------------------------------------
    let mut singles: BTreeMap<&str, &ast::Element> = BTreeMap::new();
    let (mut asset_blocks, mut state_blocks, mut chapters) = (Vec::new(), Vec::new(), Vec::new());
    for stmt in &top {
        match &stmt.kind {
            StmtKind::Element(el) => match el.name.name.as_str() {
                name @ ("book" | "theme" | "cover" | "back_cover") => {
                    if singles.insert(name, el).is_some() {
                        lw.err(
                            "E0306",
                            el.name.span,
                            format!("there is already a `{name}` block in this project"),
                        )
                        .help = Some(format!(
                            "a project has exactly one `{name}`; merge the two blocks"
                        ));
                    }
                }
                "assets" => asset_blocks.push(el),
                "state" => state_blocks.push(el),
                "chapter" => chapters.push(el),
                name => match spec::element(name) {
                    Some(s) if s.place != spec::Place::Top => {
                        let help = match s.place {
                            spec::Place::Child => format!("`{name}` belongs inside `{}`", s.parent),
                            spec::Place::Action => format!(
                                "`{name}` is an action; it belongs in a `button` or `option`"
                            ),
                            _ => format!(
                                "content goes inside a chapter:\n\n    chapter \"Title\":\n        {name} ..."
                            ),
                        };
                        lw.err(
                            "E0302",
                            el.name.span,
                            format!("`{name}` cannot appear at the top level"),
                        )
                        .help = Some(help);
                    }
                    _ => {
                        let names = spec::ELEMENTS
                            .iter()
                            .filter(|e| e.place == spec::Place::Top)
                            .map(|e| e.name);
                        let help = match suggest(name, names) {
                            Some(s) => format!("did you mean `{s}`?"),
                            None => "top-level statements: book, theme, assets, state, cover, back_cover, component, chapter, include".into(),
                        };
                        lw.err(
                            "E0301",
                            el.name.span,
                            format!("unknown top-level statement `{name}`"),
                        )
                        .help = Some(help);
                    }
                },
            },
            StmtKind::Component { name, params, body } => {
                if spec::element(&name.name).is_some() || spec::constructor(&name.name).is_some() {
                    lw.err(
                        "E0317",
                        name.span,
                        format!(
                            "`{}` is a built-in element and cannot be redefined",
                            name.name
                        ),
                    );
                } else if lw.components.contains_key(&name.name) {
                    lw.err(
                        "E0306",
                        name.span,
                        format!("the component `{}` is already defined", name.name),
                    );
                } else {
                    for (i, p) in params.iter().enumerate() {
                        if params[..i].iter().any(|q| q.name.name == p.name.name) {
                            lw.err(
                                "E0306",
                                p.name.span,
                                format!("parameter `{}` is listed twice", p.name.name),
                            );
                        }
                        if p.default.is_none() && params[..i].iter().any(|q| q.default.is_some()) {
                            lw.err(
                                "E0309",
                                p.name.span,
                                "parameters without a default must come before those with one",
                            );
                        }
                    }
                    lw.components.insert(
                        name.name.clone(),
                        Component {
                            params,
                            body,
                            span: name.span,
                            used: false,
                        },
                    );
                }
            }
            StmtKind::Assign { target, .. } => {
                lw.err(
                    "E0302",
                    stmt.span,
                    format!("`{} = ...` cannot appear at the top level", target.name),
                )
                .help = Some(
                    "properties belong inside a block such as `book \"Title\":` or `theme:`".into(),
                );
            }
            StmtKind::If { .. } => {
                lw.err("E0302", stmt.span, "`if` cannot appear at the top level")
                    .help = Some(
                    "use `chapter \"Title\", when=condition:` for conditional chapters".into(),
                );
            }
        }
    }

    // --- declarations, in dependency order ------------------------------------
    let book = match singles.get("book") {
        Some(el) => lw.book(el),
        None => {
            lw.diags.push(
                Diagnostic::global(Severity::Error, "E0313", "the project has no `book` block")
                    .with_help("add:\n\n    book \"Title\":\n        author = \"Name\"\n        language = \"en\""),
            );
            None
        }
    };
    for el in &asset_blocks {
        lw.assets_block(el);
    }
    for el in &state_blocks {
        lw.state_block(el);
    }
    let theme = lw.theme(singles.get("theme").copied());
    let covers = book.as_ref().map(|b| {
        lw.covers(
            singles.get("cover").copied(),
            singles.get("back_cover").copied(),
            &b.meta,
        )
    });

    // --- chapters --------------------------------------------------------------
    let mut out_chapters = Vec::new();
    let front = book.as_ref().map_or(Vec::new(), |b| b.front_matter.clone());
    for part in &front {
        let (id, kind, title) = match *part {
            "title" => ("_title", ir::ChapterKind::Title, "Title page"),
            "colophon" => ("_colophon", ir::ChapterKind::Colophon, "Colophon"),
            _ => ("_contents", ir::ChapterKind::Contents, "Contents"),
        };
        out_chapters.push(ir::Chapter {
            id: id.into(),
            kind,
            title: title.into(),
            number: None,
            opener: false,
            when: None,
            blocks: Vec::new(),
        });
    }
    let mut ordinal = 0u32;
    for el in &chapters {
        if let Some(c) = lw.chapter(el, &mut ordinal, theme.drop_caps) {
            out_chapters.push(c);
        }
    }
    if chapters.is_empty() {
        lw.diags.push(
            Diagnostic::global(Severity::Error, "E0313", "the book has no chapters")
                .with_help("add at least one:\n\n    chapter \"One\":\n        text \"...\""),
        );
    } else if !chapters.is_empty()
        && out_chapters
            .iter()
            .all(|c| c.kind != ir::ChapterKind::Chapter || c.when.is_some())
        && !lw.diags.iter().any(|d| d.is_error())
    {
        lw.err(
            "E0313",
            chapters[0].name.span,
            "every chapter has a `when=` condition",
        )
        .help = Some(
            "a book needs at least one unconditional chapter so that it is never empty".into(),
        );
    }
    if out_chapters.len() > ir::validate::limits::MAX_CHAPTERS {
        lw.diags.push(Diagnostic::global(
            Severity::Error,
            "E0315",
            "the book has more than 4096 chapters",
        ));
    }

    // --- deferred checks ---------------------------------------------------------
    let pending = std::mem::take(&mut lw.pending_targets);
    for (target, span) in pending {
        if ir::RESERVED_TARGETS.contains(&target.as_str())
            || (lw.anchors.contains_key(&target) && !target.starts_with('_'))
        {
            continue;
        }
        let candidates = lw
            .anchors
            .keys()
            .filter(|k| !k.starts_with('_'))
            .map(String::as_str)
            .chain(ir::RESERVED_TARGETS.iter().copied());
        let help = match suggest(&target, candidates) {
            Some(s) => format!("did you mean `{s}`?"),
            None => format!(
                "give the destination an id, e.g. `chapter \"Title\", id={target}:`; built-in targets are cover, start, contents, end"
            ),
        };
        lw.err("E0305", span, format!("unknown jump target `{target}`"))
            .help = Some(help);
    }
    if !singles.contains_key("cover") && book.is_some() {
        let span = singles["book"].name.span;
        lw.warn(
            "W0601",
            span,
            "no `cover:` block; a typographic cover was generated from the title and author",
        )
        .help = Some("add a `cover:` block to design it (style, color, ink, accent, image)".into());
    }
    if !singles.contains_key("back_cover") && book.is_some() {
        let span = singles["book"].name.span;
        lw.warn(
            "W0602",
            span,
            "no `back_cover:` block; one was generated from the book description",
        )
        .help = Some("add a `back_cover:` block with `text \"...\"` for the blurb".into());
    }
    lw.finish_warnings();

    let mut diags = std::mem::take(&mut lw.diags);
    let (mut out_book, mut files) = (None, Vec::new());
    if !diags.iter().any(Diagnostic::is_error)
        && let (Some(b), Some(cover)) = (book, covers)
    {
        let candidate = ir::Book {
            ir_version: ir::IR_VERSION.into(),
            grammar_version: ir::GRAMMAR_VERSION,
            meta: b.meta,
            theme,
            resources: std::mem::take(&mut lw.resources),
            state: lw.vars.iter().map(|d| d.var.clone()).collect(),
            cover,
            chapters: out_chapters,
            toc: std::mem::take(&mut lw.toc),
            anchors: std::mem::take(&mut lw.anchors),
        };
        match ir::validate(&candidate) {
            Ok(()) => {
                out_book = Some(candidate);
                files = std::mem::take(&mut lw.files);
            }
            Err(errors) => {
                for e in errors {
                    diags.push(
                            Diagnostic::global(Severity::Error, "E0599", format!("internal compiler error: {e}"))
                                .with_help("this is a bug in DARM; please report it with the source that triggers it"),
                        );
                }
            }
        }
    }
    // Stable order: by file, then position; position-less diagnostics last.
    diags.sort_by_key(|d| d.span.map_or((u32::MAX, u32::MAX), |s| (s.file.0, s.start)));
    done(sources, diags, out_book, files)
}

/// Compile a single in-memory source with no assets (tests, fuzzing, tooling).
pub fn compile_str(source: &str) -> Compilation {
    let fs = MemoryFs::new().with("book.darml", source.as_bytes().to_vec());
    compile(&fs, "book.darml")
}

#[allow(dead_code)]
fn _unused(_: Bx) {}
