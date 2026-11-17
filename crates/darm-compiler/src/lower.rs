//! Semantic analysis and lowering: checked AST → IR.
//!
//! This module holds the shared context ([`Lowerer`]), argument handling,
//! expressions, actions and content blocks. Top-level declarations (book,
//! theme, assets, state, covers) live in [`crate::decl`].

use std::collections::BTreeMap;

use darm_ir::expr::Ty;
use darm_ir::validate::limits;
use darm_ir::{
    self as ir, Action, Anchor, Block, BlockKind, RESERVED_TARGETS, ResourceKind, Span as TextSpan,
    StateType,
};
use darm_lang::ast::{
    self, Arg, AssignOp, BinaryOp, Element, Expr, ExprKind, Ident, Stmt, StmtKind, UnaryOp,
};
use darm_lang::{Diagnostic, Span, suggest};

use crate::PackFile;
use crate::project::Fs;
use crate::richtext::{self, Brace};
use crate::spec::{self, Body, ElementSpec, Param, Place};

/// Component parameters bound to (closed) argument expressions.
pub(crate) type Env = Vec<(String, Expr)>;

pub(crate) struct AssetDecl {
    pub kind: ResourceKind,
    pub span: Span,
    pub used: bool,
}

pub(crate) struct VarDecl {
    pub var: ir::StateVar,
    pub span: Span,
    pub read: bool,
}

pub(crate) struct Component<'a> {
    pub params: &'a [ast::Param],
    pub body: &'a ast::Block,
    pub span: Span,
    pub used: bool,
}

/// Where a list of blocks is being lowered.
#[derive(Clone, Copy)]
pub(crate) struct Bx {
    pub in_page: bool,
    pub in_box: bool,
    pub chapter_top: bool,
    pub depth: usize,
}

pub(crate) struct Lowerer<'a> {
    pub fs: &'a dyn Fs,
    /// Directory (project-relative) of each source file, by `FileId`.
    pub file_dirs: Vec<String>,
    pub diags: Vec<Diagnostic>,
    pub smart: bool,
    pub layout: ir::Layout,
    pub language: String,
    pub offline: bool,
    pub network: Vec<String>,
    pub assets: BTreeMap<String, AssetDecl>,
    pub resources: Vec<ir::Resource>,
    pub files: Vec<PackFile>,
    pub vars: Vec<VarDecl>,
    pub components: BTreeMap<String, Component<'a>>,
    pub anchors: BTreeMap<String, Anchor>,
    pub id_spans: BTreeMap<String, Span>,
    pub pending_targets: Vec<(String, Span)>,
    pub toc: Vec<ir::TocEntry>,
    pub next_id: u32,
    pub stack: Vec<String>,
    pub chapter_id: String,
    pub conds: Vec<ir::Expr>,
    pub block_count: usize,
}

/// Arguments of one element after validation against its spec.
pub(crate) struct Args<'e> {
    pos: Vec<&'e Expr>,
    named: Vec<(&'e str, &'e Expr)>,
}

impl<'e> Args<'e> {
    /// Value of a parameter given positionally or by name.
    pub fn get(&self, spec: &ElementSpec, name: &str) -> Option<&'e Expr> {
        if let Some(i) = spec.positional.iter().position(|p| p.name == name)
            && let Some(e) = self.pos.get(i)
        {
            return Some(e);
        }
        self.named.iter().find(|(n, _)| *n == name).map(|(_, e)| *e)
    }

    /// All positional values (variadic elements).
    pub fn positional(&self) -> &[&'e Expr] {
        &self.pos
    }
}

pub(crate) fn and(a: ir::Expr, b: ir::Expr) -> ir::Expr {
    ir::Expr::Bin {
        kind: ir::BinOp::And,
        left: Box::new(a),
        right: Box::new(b),
    }
}

pub(crate) fn not(a: ir::Expr) -> ir::Expr {
    match a {
        ir::Expr::Not { expr } => *expr,
        other => ir::Expr::Not {
            expr: Box::new(other),
        },
    }
}

fn signature(spec: &ElementSpec) -> String {
    let mut parts: Vec<String> = spec
        .positional
        .iter()
        .map(|p| {
            if p.required {
                p.name.to_string()
            } else {
                format!("[{}]", p.name)
            }
        })
        .collect();
    if spec.variadic {
        parts.push("...".into());
    }
    parts.extend(spec.named.iter().map(|p| format!("{}=", p.name)));
    format!("`{} {}`", spec.name, parts.join(", ")).replace(" `", "`")
}

impl<'a> Lowerer<'a> {
    // --- diagnostics --------------------------------------------------------

    pub fn err(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
    ) -> &mut Diagnostic {
        self.diags.push(Diagnostic::error(code, span, msg));
        self.diags.last_mut().unwrap()
    }

    pub fn warn(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
    ) -> &mut Diagnostic {
        self.diags.push(Diagnostic::warning(code, span, msg));
        self.diags.last_mut().unwrap()
    }

    pub fn fresh_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("_{prefix}{}", self.next_id)
    }

    // --- argument handling --------------------------------------------------

    /// Replace a bare parameter name by the argument it is bound to.
    pub fn resolve<'e>(&self, e: &'e Expr, env: &'e Env) -> &'e Expr {
        let u = e.unparen();
        if let ExprKind::Name(n) = &u.kind
            && let Some((_, bound)) = env.iter().rev().find(|(name, _)| name == n)
        {
            return bound;
        }
        u
    }

    pub fn args<'e>(
        &mut self,
        name: &Ident,
        list: &'e [Arg],
        spec: &ElementSpec,
        env: &'e Env,
    ) -> Args<'e> {
        let mut out = Args {
            pos: Vec::new(),
            named: Vec::new(),
        };
        for arg in list {
            let value = self.resolve(&arg.value, env);
            match &arg.name {
                None => {
                    if out.pos.len() >= spec.positional.len() && !spec.variadic {
                        let help = format!("usage: {}", signature(spec));
                        let msg = match spec.positional.len() {
                            0 => format!("`{}` takes no positional arguments", name.name),
                            1 => format!("`{}` takes one positional argument", name.name),
                            n => format!("`{}` takes {n} positional arguments", name.name),
                        };
                        self.err("E0303", arg.value.span, msg).help = Some(help);
                    } else {
                        out.pos.push(value);
                    }
                }
                Some(key) => {
                    let known = spec
                        .named
                        .iter()
                        .chain(spec.positional)
                        .any(|p| p.name == key.name)
                        || (spec.common && spec::COMMON.iter().any(|p| p.name == key.name));
                    if !known {
                        let candidates = spec
                            .named
                            .iter()
                            .chain(spec.positional)
                            .chain(if spec.common { spec::COMMON } else { &[] })
                            .map(|p| p.name);
                        let help = match suggest(&key.name, candidates) {
                            Some(s) => format!("did you mean `{s}`?"),
                            None => format!("usage: {}", signature(spec)),
                        };
                        self.err(
                            "E0303",
                            key.span,
                            format!("`{}` has no argument named `{}`", name.name, key.name),
                        )
                        .help = Some(help);
                    } else if out.named.iter().any(|(n, _)| *n == key.name) {
                        self.err(
                            "E0306",
                            key.span,
                            format!("argument `{}` is given twice", key.name),
                        );
                    } else {
                        out.named.push((key.name.as_str(), value));
                    }
                }
            }
        }
        for (i, p) in spec.positional.iter().enumerate() {
            if out.pos.len() > i && out.named.iter().any(|(n, _)| *n == p.name) {
                self.err(
                    "E0306",
                    name.span,
                    format!("`{}` is given both positionally and by name", p.name),
                );
            }
        }
        out
    }

    /// Report required parameters that were not supplied.
    pub fn require(&mut self, name: &Ident, spec: &ElementSpec, args: &Args) -> bool {
        let mut ok = true;
        for p in spec.positional.iter().chain(spec.named) {
            if p.required && args.get(spec, p.name).is_none() {
                ok = false;
                self.err(
                    "E0308",
                    name.span,
                    format!(
                        "`{}` needs {} `{}` ({})",
                        name.name,
                        article(p.ty),
                        p.name,
                        p.ty
                    ),
                )
                .help = Some(format!("usage: {}", signature(spec)));
            }
        }
        ok
    }

    pub fn body_rule(&mut self, el: &Element, spec: &ElementSpec) -> bool {
        match (spec.body, &el.body) {
            (Body::None, Some(b)) => {
                self.err(
                    "E0313",
                    b.span,
                    format!("`{}` does not take a block", el.name.name),
                )
                .help = Some("remove the `:` and the indented lines".into());
                false
            }
            (
                Body::Blocks
                | Body::FramedBlocks
                | Body::Items
                | Body::Actions
                | Body::Properties
                | Body::Declarations,
                None,
            ) => {
                self.err(
                    "E0313",
                    el.name.span,
                    format!("`{}` needs an indented block", el.name.name),
                )
                .help = Some(format!(
                    "end the line with `:` and indent the contents of `{}` below it",
                    el.name.name
                ));
                false
            }
            _ => true,
        }
    }

    // --- typed values -------------------------------------------------------

    pub fn string(&mut self, e: &Expr) -> Option<String> {
        match &e.kind {
            ExprKind::Str { value, .. } => {
                if value.len() > limits::MAX_STRING {
                    self.err("E0315", e.span, "this string is longer than 8 KiB");
                    return None;
                }
                Some(unescape_markup(value))
            }
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!("expected a string, found {}", e.describe()),
                )
                .help = Some("wrap the value in double quotes".into());
                None
            }
        }
    }

    pub fn word(&mut self, e: &Expr, allowed: &[&'static str], what: &str) -> Option<&'static str> {
        let given = match &e.kind {
            ExprKind::Name(n) => n.as_str(),
            ExprKind::Str { value, .. } => value.as_str(),
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!(
                        "expected {what} ({}), found {}",
                        allowed.join(", "),
                        e.describe()
                    ),
                );
                return None;
            }
        };
        if let Some(w) = allowed.iter().find(|w| **w == given) {
            return Some(w);
        }
        let help = match suggest(given, allowed.iter().copied()) {
            Some(s) => format!("did you mean `{s}`? allowed: {}", allowed.join(", ")),
            None => format!("allowed: {}", allowed.join(", ")),
        };
        self.err("E0312", e.span, format!("`{given}` is not a valid {what}"))
            .help = Some(help);
        None
    }

    pub fn boolean(&mut self, e: &Expr) -> Option<bool> {
        match &e.kind {
            ExprKind::Bool(b) => Some(*b),
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!("expected `true` or `false`, found {}", e.describe()),
                );
                None
            }
        }
    }

    pub fn number(&mut self, e: &Expr) -> Option<f64> {
        match &e.kind {
            ExprKind::Int(i) => Some(*i as f64),
            ExprKind::Decimal(d) | ExprKind::Percent(d) => Some(*d),
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr,
            } => self.number(expr).map(|v| -v),
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!("expected a number, found {}", e.describe()),
                );
                None
            }
        }
    }

    pub fn ranged(&mut self, e: &Expr, lo: f64, hi: f64, what: &str) -> Option<f64> {
        let v = self.number(e)?;
        if v.is_finite() && (lo..=hi).contains(&v) {
            Some(v)
        } else {
            self.err(
                "E0312",
                e.span,
                format!("{what} must be between {lo} and {hi}"),
            );
            None
        }
    }

    pub fn color(&mut self, e: &Expr) -> Option<String> {
        let ExprKind::Str { value, .. } = &e.kind else {
            self.err(
                "E0307",
                e.span,
                format!(
                    "expected a colour string like \"#7a2e22\", found {}",
                    e.describe()
                ),
            );
            return None;
        };
        let v = value.trim();
        let hex = v.strip_prefix('#').unwrap_or("");
        let ok = v.starts_with('#') && hex.chars().all(|c| c.is_ascii_hexdigit());
        match (ok, hex.len()) {
            (true, 6) => Some(v.to_ascii_lowercase()),
            (true, 3) => Some(
                format!("#{}", hex.chars().flat_map(|c| [c, c]).collect::<String>())
                    .to_ascii_lowercase(),
            ),
            _ => {
                self.err("E0312", e.span, format!("`{value}` is not a colour"))
                    .help = Some(
                    "colours are written \"#rrggbb\" (six hexadecimal digits), e.g. \"#7a2e22\""
                        .into(),
                );
                None
            }
        }
    }

    pub fn duration(&mut self, e: &Expr, lo: u32, hi: u32, what: &str) -> Option<u32> {
        match &e.kind {
            ExprKind::Duration(ms) if (lo..=hi).contains(ms) => Some(*ms),
            ExprKind::Duration(_) => {
                self.err(
                    "E0312",
                    e.span,
                    format!("{what} must be between {lo}ms and {hi}ms"),
                );
                None
            }
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!(
                        "expected a duration such as `300ms` or `1.5s`, found {}",
                        e.describe()
                    ),
                );
                None
            }
        }
    }

    /// A new id (`id=name`). Registers nothing.
    pub fn new_id(&mut self, e: &Expr) -> Option<String> {
        let name = match &e.kind {
            ExprKind::Name(n) => n.clone(),
            ExprKind::Str { value, .. } => value.clone(),
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!("expected an identifier, found {}", e.describe()),
                );
                return None;
            }
        };
        if !ir::is_ident(&name) {
            self.err("E0312", e.span, format!("`{name}` is not a valid id"))
                .help =
                Some("ids use letters, digits and `_`, and do not start with a digit".into());
            return None;
        }
        if RESERVED_TARGETS.contains(&name.as_str()) || name.starts_with('_') {
            self.err("E0317", e.span, format!("`{name}` is a reserved name")).help =
                Some("`cover`, `start`, `contents`, `end` and names starting with `_` are reserved; choose another id".into());
            return None;
        }
        Some(name)
    }

    /// Declare an anchor for `id` at `block` of the current chapter.
    pub fn declare_id(&mut self, id: &str, span: Span, block: Option<String>) -> bool {
        if let Some(prev) = self.id_spans.get(id).copied() {
            self.err("E0306", span, format!("the id `{id}` is already used"))
                .help = Some("ids must be unique across chapters, sections and blocks".into());
            let _ = prev;
            return false;
        }
        self.id_spans.insert(id.to_string(), span);
        self.anchors.insert(
            id.to_string(),
            Anchor {
                chapter: self.chapter_id.clone(),
                block,
            },
        );
        true
    }

    /// A jump target (`goto x`, `goto=x`). Checked once all ids are known.
    pub fn target(&mut self, e: &Expr) -> Option<String> {
        let name = match &e.kind {
            ExprKind::Name(n) => n.clone(),
            ExprKind::Str { value, .. } => value.clone(),
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!("expected the id of a jump target, found {}", e.describe()),
                );
                return None;
            }
        };
        self.pending_targets.push((name.clone(), e.span));
        Some(name)
    }

    pub fn asset(&mut self, e: &Expr, kind: ResourceKind) -> Option<String> {
        let name = match &e.kind {
            ExprKind::Name(n) => n.clone(),
            ExprKind::Str { value, .. } => {
                self.err("E0304", e.span, format!("`\"{value}\"` is a string, not an asset name")).help = Some(
                    "declare the file once in `assets:` (e.g. `pic = image(\"art/pic.png\", alt=\"...\")`) and refer to it by name"
                        .into(),
                );
                return None;
            }
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!("expected an asset name, found {}", e.describe()),
                );
                return None;
            }
        };
        match self.assets.get_mut(&name) {
            Some(decl) if decl.kind == kind => {
                decl.used = true;
                Some(name)
            }
            Some(decl) => {
                let found = kind_name(decl.kind);
                self.err(
                    "E0307",
                    e.span,
                    format!(
                        "`{name}` is {} asset, but {} asset is needed here",
                        article_kind(found),
                        article_kind(kind_name(kind))
                    ),
                );
                None
            }
            None => {
                let help = match suggest(&name, self.assets.keys().map(String::as_str)) {
                    Some(s) => format!("did you mean `{s}`?"),
                    None => format!(
                        "declare it in an `assets:` block: `{name} = {}(\"path\", ...)`",
                        kind_name(kind)
                    ),
                };
                self.err("E0304", e.span, format!("unknown asset `{name}`"))
                    .help = Some(help);
                None
            }
        }
    }

    // --- rich text ----------------------------------------------------------

    pub fn rich(
        &mut self,
        e: &Expr,
        env: &Env,
        verse: bool,
        split: bool,
    ) -> Option<Vec<Vec<TextSpan>>> {
        let ExprKind::Str { value, .. } = &e.kind else {
            self.err(
                "E0307",
                e.span,
                format!("expected text in double quotes, found {}", e.describe()),
            );
            return None;
        };
        let opts = richtext::Options {
            smart: self.smart,
            verse,
            split_paragraphs: split,
        };
        let vars = &self.vars;
        let lookup = |name: &str| -> Brace {
            if let Some((_, bound)) = env.iter().rev().find(|(n, _)| n == name) {
                return match &bound.unparen().kind {
                    ExprKind::Str { value, .. } => Brace::Text(unescape_markup(value)),
                    ExprKind::Int(i) => Brace::Text(i.to_string()),
                    ExprKind::Bool(b) => Brace::Text(b.to_string()),
                    ExprKind::Name(n) => Brace::Text(n.clone()),
                    _ => Brace::Unknown,
                };
            }
            if vars.iter().any(|v| v.var.name == name) {
                Brace::Var
            } else {
                Brace::Unknown
            }
        };
        match richtext::parse(value, opts, &lookup) {
            Ok(rich) => {
                for v in &rich.vars {
                    if let Some(decl) = self.vars.iter_mut().find(|d| d.var.name == *v) {
                        decl.read = true;
                    }
                }
                for t in rich.targets {
                    self.pending_targets.push((t, e.span));
                }
                if rich.paragraphs.is_empty() {
                    self.err("E0312", e.span, "the text is empty");
                    return None;
                }
                for p in &rich.paragraphs {
                    if p.iter().any(|s| s.text.len() > limits::MAX_SPAN_TEXT) {
                        self.err(
                            "E0315",
                            e.span,
                            "a single run of text is longer than 64 KiB; split it into paragraphs",
                        );
                        return None;
                    }
                }
                Some(rich.paragraphs)
            }
            Err(error) => {
                self.err("E0314", e.span, error.message).help = error.help;
                None
            }
        }
    }

    /// Rich text that must be a single paragraph.
    pub fn rich1(&mut self, e: &Expr, env: &Env) -> Option<Vec<TextSpan>> {
        self.rich(e, env, false, false).map(|mut p| p.remove(0))
    }

    // --- expressions --------------------------------------------------------

    pub fn expr(&mut self, e: &Expr, env: &Env, depth: usize) -> Option<(ir::Expr, Ty)> {
        if depth > limits::MAX_EXPR_DEPTH - 2 {
            self.err(
                "E0315",
                e.span,
                "this expression is nested too deeply (limit 32)",
            );
            return None;
        }
        let e = self.resolve(e, env);
        Some(match &e.kind {
            ExprKind::Int(v) => (ir::Expr::Int { value: *v }, Ty::Int),
            ExprKind::Bool(v) => (ir::Expr::Bool { value: *v }, Ty::Bool),
            ExprKind::Str { value, .. } => (
                ir::Expr::Str {
                    value: value.clone(),
                },
                Ty::Str,
            ),
            ExprKind::Name(n) => match self.vars.iter_mut().find(|d| d.var.name == *n) {
                Some(decl) => {
                    decl.read = true;
                    (ir::Expr::Var { name: n.clone() }, decl.var.ty())
                }
                None => {
                    let help = match suggest(n, self.vars.iter().map(|d| d.var.name.as_str())) {
                        Some(s) => format!("did you mean `{s}`?"),
                        None => format!(
                            "declare it in a `state:` block, e.g. `{n} = false`; text values need double quotes"
                        ),
                    };
                    self.err("E0310", e.span, format!("unknown state variable `{n}`"))
                        .help = Some(help);
                    return None;
                }
            },
            ExprKind::Unary {
                op: UnaryOp::Not,
                expr,
            } => {
                let (inner, ty) = self.expr(expr, env, depth + 1)?;
                if ty != Ty::Bool {
                    self.err(
                        "E0311",
                        expr.span,
                        format!("`not` needs a bool, but this is {}", article(ty.name())),
                    );
                    return None;
                }
                (
                    ir::Expr::Not {
                        expr: Box::new(inner),
                    },
                    Ty::Bool,
                )
            }
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr,
            } => {
                let (inner, ty) = self.expr(expr, env, depth + 1)?;
                if ty != Ty::Int {
                    self.err(
                        "E0311",
                        expr.span,
                        format!("unary `-` needs an int, but this is {}", article(ty.name())),
                    );
                    return None;
                }
                match inner {
                    ir::Expr::Int { value } => (
                        ir::Expr::Int {
                            value: value.saturating_neg(),
                        },
                        Ty::Int,
                    ),
                    other => (
                        ir::Expr::Neg {
                            expr: Box::new(other),
                        },
                        Ty::Int,
                    ),
                }
            }
            ExprKind::Binary { op, left, right } => {
                let l = self.expr(left, env, depth + 1);
                let r = self.expr(right, env, depth + 1);
                let ((le, lt), (re, rt)) = (l?, r?);
                let sym = op.symbol();
                let (kind, operands, result) = match op {
                    BinaryOp::Or => (ir::BinOp::Or, Some(Ty::Bool), Ty::Bool),
                    BinaryOp::And => (ir::BinOp::And, Some(Ty::Bool), Ty::Bool),
                    BinaryOp::Add => (ir::BinOp::Add, Some(Ty::Int), Ty::Int),
                    BinaryOp::Sub => (ir::BinOp::Sub, Some(Ty::Int), Ty::Int),
                    BinaryOp::Mul => (ir::BinOp::Mul, Some(Ty::Int), Ty::Int),
                    BinaryOp::Lt => (ir::BinOp::Lt, Some(Ty::Int), Ty::Bool),
                    BinaryOp::Le => (ir::BinOp::Le, Some(Ty::Int), Ty::Bool),
                    BinaryOp::Gt => (ir::BinOp::Gt, Some(Ty::Int), Ty::Bool),
                    BinaryOp::Ge => (ir::BinOp::Ge, Some(Ty::Int), Ty::Bool),
                    BinaryOp::Eq => (ir::BinOp::Eq, None, Ty::Bool),
                    BinaryOp::Ne => (ir::BinOp::Ne, None, Ty::Bool),
                };
                match operands {
                    Some(want) => {
                        for (side, ty) in [(left, lt), (right, rt)] {
                            if ty != want {
                                self.err(
                                    "E0311",
                                    side.span,
                                    format!(
                                        "`{sym}` needs {} operands, but this is {}",
                                        want.name(),
                                        article(ty.name())
                                    ),
                                );
                                return None;
                            }
                        }
                    }
                    None => {
                        if lt != rt {
                            self.err(
                                "E0311",
                                e.span,
                                format!(
                                    "cannot compare {} with {}",
                                    article(lt.name()),
                                    article(rt.name())
                                ),
                            )
                            .help =
                                Some("both sides of `==` / `!=` must have the same type".into());
                            return None;
                        }
                        // `route == "clif"` against a closed set is almost certainly a typo.
                        for (var_side, lit_side, lit_span) in
                            [(&le, &re, right.span), (&re, &le, left.span)]
                        {
                            if let (ir::Expr::Var { name }, ir::Expr::Str { value }) =
                                (var_side, lit_side)
                                && let Some(StateType::Enum { values, .. }) = self
                                    .vars
                                    .iter()
                                    .find(|d| d.var.name == *name)
                                    .map(|d| &d.var.ty)
                                && !values.contains(value)
                            {
                                let help = match suggest(value, values.iter().map(String::as_str)) {
                                    Some(s) => format!("did you mean \"{s}\"?"),
                                    None => format!("`{name}` can be: {}", values.join(", ")),
                                };
                                self.err(
                                    "E0312",
                                    lit_span,
                                    format!("\"{value}\" is not a possible value of `{name}`"),
                                )
                                .help = Some(help);
                                return None;
                            }
                        }
                    }
                }
                (
                    ir::Expr::Bin {
                        kind,
                        left: Box::new(le),
                        right: Box::new(re),
                    },
                    result,
                )
            }
            _ => {
                self.err(
                    "E0311",
                    e.span,
                    format!("{} cannot be used in an expression", e.describe()),
                )
                .help = Some(
                    "expressions use integers, `true`/`false`, quoted text and state variables"
                        .into(),
                );
                return None;
            }
        })
    }

    pub fn cond(&mut self, e: &Expr, env: &Env) -> Option<ir::Expr> {
        let (expr, ty) = self.expr(e, env, 0)?;
        if ty != Ty::Bool {
            self.err(
                "E0311",
                e.span,
                format!(
                    "a condition must be true or false, but this is {}",
                    article(ty.name())
                ),
            )
            .help = Some(
                "compare it with something, e.g. `courage > 0` or `route == \"cliff\"`".into(),
            );
            return None;
        }
        Some(expr)
    }

    // --- actions ------------------------------------------------------------

    fn goto_args(&mut self, name: &Ident, args: &Args, spec: &ElementSpec) -> Option<Action> {
        let target = self.target(args.get(spec, "goto").or(args.get(spec, "target"))?)?;
        let transition = match args.get(spec, "transition") {
            Some(t) => Some(match self.word(t, spec::TRANSITIONS, "transition")? {
                "fade" => ir::Transition::Fade,
                "none" => ir::Transition::None,
                _ => ir::Transition::Flip,
            }),
            None => None,
        };
        let _ = name;
        Some(Action::Goto { target, transition })
    }

    pub fn actions(&mut self, stmts: &[Stmt], env: &Env, owner: &str) -> Vec<Action> {
        let mut out = Vec::new();
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Assign { target, op, value } => {
                    let Some(index) = self.vars.iter().position(|d| d.var.name == target.name)
                    else {
                        let help = match suggest(
                            &target.name,
                            self.vars.iter().map(|d| d.var.name.as_str()),
                        ) {
                            Some(s) => format!("did you mean `{s}`?"),
                            None => format!(
                                "declare it in a `state:` block, e.g. `{} = 0`",
                                target.name
                            ),
                        };
                        self.err(
                            "E0310",
                            target.span,
                            format!("unknown state variable `{}`", target.name),
                        )
                        .help = Some(help);
                        continue;
                    };
                    let var_ty = self.vars[index].var.ty();
                    let Some((mut expr, ty)) = self.expr(value, env, 0) else {
                        continue;
                    };
                    if ty != var_ty {
                        self.err(
                            "E0311",
                            value.span,
                            format!(
                                "`{}` is {} but the value is {}",
                                target.name,
                                article(var_ty.name()),
                                article(ty.name())
                            ),
                        );
                        continue;
                    }
                    if let (StateType::Enum { values, .. }, ir::Expr::Str { value: v }) =
                        (&self.vars[index].var.ty, &expr)
                        && !values.contains(v)
                    {
                        let help = match suggest(v, values.iter().map(String::as_str)) {
                            Some(s) => format!("did you mean \"{s}\"?"),
                            None => format!("`{}` can be: {}", target.name, values.join(", ")),
                        };
                        self.err(
                            "E0312",
                            value.span,
                            format!("\"{v}\" is not a possible value of `{}`", target.name),
                        )
                        .help = Some(help);
                        continue;
                    }
                    if *op != AssignOp::Set {
                        if var_ty != Ty::Int {
                            self.err(
                                "E0311",
                                stmt.span,
                                format!("`{}` only works on int variables", op.symbol()),
                            );
                            continue;
                        }
                        let kind = if *op == AssignOp::Add {
                            ir::BinOp::Add
                        } else {
                            ir::BinOp::Sub
                        };
                        expr = ir::Expr::Bin {
                            kind,
                            left: Box::new(ir::Expr::Var {
                                name: target.name.clone(),
                            }),
                            right: Box::new(expr),
                        };
                    }
                    out.push(Action::Set {
                        var: target.name.clone(),
                        value: expr,
                    });
                }
                StmtKind::Element(el) if el.name.name == "goto" || el.name.name == "reset" => {
                    let spec = spec::element(&el.name.name).unwrap();
                    self.body_rule(el, spec);
                    let args = self.args(&el.name, &el.args, spec, env);
                    if !self.require(&el.name, spec, &args) {
                        continue;
                    }
                    if el.name.name == "goto" {
                        if let Some(a) = self.goto_args(&el.name, &args, spec) {
                            out.push(a);
                        }
                    } else {
                        match args.get(spec, "variable") {
                            None => out.push(Action::Reset { var: None }),
                            Some(v) => match &v.kind {
                                ExprKind::Name(n) if self.vars.iter().any(|d| d.var.name == *n) => {
                                    out.push(Action::Reset {
                                        var: Some(n.clone()),
                                    });
                                }
                                ExprKind::Name(n) => {
                                    self.err(
                                        "E0310",
                                        v.span,
                                        format!("unknown state variable `{n}`"),
                                    );
                                }
                                _ => {
                                    self.err(
                                        "E0307",
                                        v.span,
                                        format!(
                                            "expected a state variable name, found {}",
                                            v.describe()
                                        ),
                                    );
                                }
                            },
                        }
                    }
                }
                StmtKind::Element(el) => {
                    self.err(
                        "E0302",
                        el.name.span,
                        format!("`{}` is not an action", el.name.name),
                    )
                    .help = Some(format!(
                        "inside `{owner}` only actions are allowed: `name = value`, `name += n`, `goto target`, `reset`"
                    ));
                }
                StmtKind::If { .. } => {
                    self.err("E0302", stmt.span, "conditions are not allowed inside action blocks").help =
                        Some("offer separate `option`s with `when=`, or put the `button` inside an `if` block".into());
                }
                StmtKind::Component { .. } => {
                    self.err(
                        "E0302",
                        stmt.span,
                        "components can only be defined at the top level",
                    );
                }
            }
        }
        if out.len() > limits::MAX_ACTIONS {
            let span = stmts[0].span;
            self.err(
                "E0315",
                span,
                format!("`{owner}` has more than {} actions", limits::MAX_ACTIONS),
            );
            out.truncate(limits::MAX_ACTIONS);
        }
        out
    }

    /// Actions of `button` / `option`: `goto=` shorthand plus an optional body.
    fn element_actions(
        &mut self,
        el: &Element,
        args: &Args,
        spec: &ElementSpec,
        env: &Env,
    ) -> Vec<Action> {
        let mut actions = match &el.body {
            Some(body) => self.actions(&body.stmts, env, &el.name.name),
            None => Vec::new(),
        };
        if args.get(spec, "goto").is_some() {
            if actions.iter().any(|a| matches!(a, Action::Goto { .. })) {
                self.err(
                    "E0306",
                    el.name.span,
                    "this has both `goto=` and a `goto` action",
                )
                .help = Some("keep only one of them".into());
            } else if let Some(a) = self.goto_args(&el.name, args, spec) {
                actions.push(a);
            }
        } else if let Some(t) = args.get(spec, "transition") {
            self.err(
                "E0308",
                t.span,
                "`transition=` needs `goto=` on the same line",
            )
            .help = Some("write `goto target, transition=fade` inside the block instead".into());
        }
        actions
    }

    // --- blocks -------------------------------------------------------------

    fn tuple2(&mut self, e: &Expr, what: &str) -> Option<(f64, f64)> {
        match &e.kind {
            ExprKind::Tuple(items) if items.len() == 2 => {
                Some((self.number(&items[0])?, self.number(&items[1])?))
            }
            _ => {
                self.err(
                    "E0307",
                    e.span,
                    format!(
                        "expected {what} as a pair like `(10, 20)`, found {}",
                        e.describe()
                    ),
                );
                None
            }
        }
    }

    /// Apply `id`, `align`, `color`, `scale`, animation and frame arguments.
    fn common(&mut self, el: &Element, args: &Args, spec: &ElementSpec, block: &mut Block, bx: Bx) {
        if let Some(e) = args.get(spec, "id")
            && let Some(id) = self.new_id(e)
            && self.declare_id(&id, e.span, Some(id.clone()))
        {
            block.id = id;
        }
        if !spec.common {
            return;
        }
        if let Some(e) = args.get(spec, "align") {
            block.align = self.word(e, spec::ALIGN, "alignment").map(|w| match w {
                "left" => ir::Align::Left,
                "center" => ir::Align::Center,
                "right" => ir::Align::Right,
                _ => ir::Align::Justify,
            });
        }
        if let Some(e) = args.get(spec, "color") {
            block.color = self.color(e);
        }
        if let Some(e) = args.get(spec, "scale") {
            block.scale = self.ranged(e, 0.25, 8.0, "scale");
        }
        let delay = args
            .get(spec, "delay")
            .and_then(|e| self.duration(e, 0, 30_000, "delay"));
        let duration = args
            .get(spec, "duration")
            .and_then(|e| self.duration(e, 1, 10_000, "duration"));
        match args.get(spec, "animate") {
            Some(e) => {
                if let Some(w) = self.word(e, spec::ANIMATIONS, "animation") {
                    let kind = match w {
                        "fade" => ir::AnimKind::Fade,
                        "rise" => ir::AnimKind::Rise,
                        "drop" => ir::AnimKind::Drop,
                        "slide_left" => ir::AnimKind::SlideLeft,
                        "slide_right" => ir::AnimKind::SlideRight,
                        _ => ir::AnimKind::Zoom,
                    };
                    block.anim = Some(ir::Anim {
                        kind,
                        delay_ms: delay.unwrap_or(0),
                        duration_ms: duration.unwrap_or(600),
                    });
                }
            }
            None => {
                if let Some(e) = args.get(spec, "delay").or(args.get(spec, "duration")) {
                    self.err("E0308", e.span, "`delay=` and `duration=` need `animate=`")
                        .help = Some("add e.g. `animate=fade`".into());
                }
            }
        }
        let at = args.get(spec, "at");
        let size = args.get(spec, "size");
        match (at, size, bx.in_page) {
            (Some(at), Some(size), true) => {
                if let (Some((x, y)), Some((w, h))) =
                    (self.tuple2(at, "`at`"), self.tuple2(size, "`size`"))
                {
                    let inside = x >= 0.0
                        && y >= 0.0
                        && w > 0.0
                        && h > 0.0
                        && x + w <= 100.0001
                        && y + h <= 100.0001;
                    if inside {
                        block.frame = Some(ir::Frame { x, y, w, h });
                    } else {
                        self.err("E0316", at.span.to(size.span), "the frame does not fit inside the page").help =
                            Some("`at` and `size` are percentages of the page: x + width and y + height must not exceed 100".into());
                    }
                }
            }
            (None, None, true) => {
                self.err(
                    "E0316",
                    el.name.span,
                    format!(
                        "`{}` is inside a `page` and needs `at=(x, y)` and `size=(w, h)`",
                        el.name.name
                    ),
                )
                .help = Some(
                    "positions are percentages of the page, e.g. `at=(10, 10), size=(80, 20)`"
                        .into(),
                );
            }
            (a, s, true) => {
                let span = a.or(s).unwrap().span;
                self.err("E0316", span, "`at=` and `size=` must be given together");
            }
            (a, s, false) => {
                if let Some(e) = a.or(s) {
                    self.err(
                        "E0316",
                        e.span,
                        "`at=` and `size=` are only valid inside a `page`",
                    )
                    .help = Some("in flowing text use `width=` and `align=` instead".into());
                }
            }
        }
    }

    fn block(&mut self, kind: BlockKind) -> Block {
        self.block_count += 1;
        Block {
            id: self.fresh_id("b"),
            kind,
            align: None,
            anim: None,
            frame: None,
            color: None,
            scale: None,
        }
    }

    pub fn blocks(&mut self, stmts: &[Stmt], env: &Env, bx: Bx) -> Vec<Block> {
        let mut out = Vec::new();
        if bx.depth > limits::MAX_NESTING - 1 {
            if let Some(s) = stmts.first() {
                self.err(
                    "E0315",
                    s.span,
                    format!(
                        "blocks are nested more than {} levels deep",
                        limits::MAX_NESTING
                    ),
                );
            }
            return out;
        }
        if self.block_count > limits::MAX_BLOCKS {
            return out;
        }
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Element(el) => self.element(el, env, bx, &mut out),
                StmtKind::If {
                    branches,
                    otherwise,
                } => {
                    let mut negated: Option<ir::Expr> = None;
                    let mut broken = false;
                    for b in branches {
                        let Some(cond) = self.cond(&b.cond, env) else {
                            broken = true;
                            continue;
                        };
                        let when = match &negated {
                            Some(n) => and(n.clone(), cond.clone()),
                            None => cond.clone(),
                        };
                        negated = Some(match negated {
                            Some(n) => and(n, not(cond)),
                            None => not(cond),
                        });
                        self.group(Some(when), &b.body.stmts, env, bx, &mut out);
                    }
                    if let (Some(body), false) = (otherwise, broken) {
                        self.group(negated, &body.stmts, env, bx, &mut out);
                    }
                }
                StmtKind::Assign { target, .. } => {
                    self.err("E0302", stmt.span, format!("`{} = ...` is not allowed here", target.name)).help = Some(
                        "assignments to state belong in an action block (`button`, `option`, `on_correct`); arguments are written on the element's line as `name=value`"
                            .into(),
                    );
                }
                StmtKind::Component { name, .. } => {
                    self.err(
                        "E0302",
                        name.span,
                        "components can only be defined at the top level of a file",
                    );
                }
            }
        }
        out
    }

    fn group(
        &mut self,
        when: Option<ir::Expr>,
        stmts: &[Stmt],
        env: &Env,
        bx: Bx,
        out: &mut Vec<Block>,
    ) {
        if let Some(w) = &when {
            self.conds.push(w.clone());
        }
        let blocks = self.blocks(
            stmts,
            env,
            Bx {
                chapter_top: false,
                depth: bx.depth + 1,
                ..bx
            },
        );
        if when.is_some() {
            self.conds.pop();
        }
        let b = self.block(BlockKind::Group { when, blocks });
        out.push(b);
    }

    fn element(&mut self, el: &Element, env: &Env, bx: Bx, out: &mut Vec<Block>) {
        let name = el.name.name.as_str();
        let Some(spec) =
            spec::element(name).filter(|s| matches!(s.place, Place::Block | Place::Chapter))
        else {
            if self.components.contains_key(name) {
                self.expand(el, env, bx, out);
                return;
            }
            self.misplaced(el);
            return;
        };
        self.body_rule(el, spec);
        let args = self.args(&el.name, &el.args, spec, env);
        if !self.require(&el.name, spec, &args) {
            return;
        }
        let inner = Bx {
            chapter_top: false,
            depth: bx.depth + 1,
            ..bx
        };
        let caption = |me: &mut Self| args.get(spec, "caption").and_then(|c| me.rich1(c, env));
        let flag = |me: &mut Self, key: &str| {
            args.get(spec, key)
                .and_then(|e| me.boolean(e))
                .unwrap_or(false)
        };

        let kind = match name {
            "section" => {
                if bx.in_page || bx.in_box {
                    self.err(
                        "E0302",
                        el.name.span,
                        "`section` cannot be used inside a `page` or `box`",
                    );
                    return;
                }
                let Some(title) = self.rich1(args.get(spec, "title").unwrap(), env) else {
                    return;
                };
                let when = args.get(spec, "when").and_then(|w| self.cond(w, env));
                let listed = args
                    .get(spec, "toc")
                    .and_then(|e| self.boolean(e))
                    .unwrap_or(true);
                let mut heading = self.block(BlockKind::Heading {
                    spans: title.clone(),
                    level: 2,
                });
                let anchor = match args
                    .get(spec, "id")
                    .and_then(|e| self.new_id(e).map(|id| (id, e.span)))
                {
                    Some((id, span)) => {
                        if self.declare_id(&id, span, Some(id.clone())) {
                            heading.id = id;
                        }
                        heading.id.clone()
                    }
                    None => {
                        let id = heading.id.clone();
                        self.anchors.insert(
                            id.clone(),
                            Anchor {
                                chapter: self.chapter_id.clone(),
                                block: Some(id.clone()),
                            },
                        );
                        id
                    }
                };
                if let Some(w) = &when {
                    self.conds.push(w.clone());
                }
                if listed {
                    let condition = self.conds.iter().cloned().reduce(and);
                    let title: String = title.iter().map(|s| s.text.as_str()).collect();
                    self.toc.push(ir::TocEntry {
                        title,
                        target: anchor,
                        level: 2,
                        number: None,
                        when: condition,
                    });
                }
                let mut blocks = vec![heading];
                if let Some(body) = &el.body {
                    blocks.extend(self.blocks(&body.stmts, env, inner));
                }
                if when.is_some() {
                    self.conds.pop();
                }
                let g = self.block(BlockKind::Group { when, blocks });
                out.push(g);
                return;
            }
            "heading" => {
                let Some(spans) = self.rich1(args.get(spec, "text").unwrap(), env) else {
                    return;
                };
                BlockKind::Heading { spans, level: 3 }
            }
            "text" => {
                let style = match args.get(spec, "style") {
                    Some(e) => match self.word(e, spec::TEXT_STYLES, "text style") {
                        Some(w) => text_style(w),
                        None => return,
                    },
                    None => ir::TextStyle::Body,
                };
                let verse = style == ir::TextStyle::Verse;
                let Some(mut paragraphs) =
                    self.rich(args.get(spec, "text").unwrap(), env, verse, true)
                else {
                    return;
                };
                if bx.in_page && paragraphs.len() > 1 {
                    // One frame holds one block: keep paragraphs as hard breaks.
                    let mut merged: Vec<TextSpan> = Vec::new();
                    for p in paragraphs {
                        if let Some(last) = merged.last_mut() {
                            last.line_break = true;
                        }
                        merged.extend(p);
                    }
                    paragraphs = vec![merged];
                }
                let count = paragraphs.len();
                for (i, spans) in paragraphs.into_iter().enumerate() {
                    let mut b = self.block(BlockKind::Paragraph { spans, style });
                    if i == 0 {
                        self.common(el, &args, spec, &mut b, bx);
                    } else if let Some(first) = out.last() {
                        // Later paragraphs share presentation but not the id.
                        b.align = first.align;
                        b.color = first.color.clone();
                        b.scale = first.scale;
                        b.anim = first.anim.clone();
                    }
                    out.push(b);
                }
                let _ = count;
                return;
            }
            "quote" => {
                let Some(paragraphs) = self.rich(args.get(spec, "text").unwrap(), env, false, true)
                else {
                    return;
                };
                let attribution = args.get(spec, "by").and_then(|b| self.rich1(b, env));
                BlockKind::Quote {
                    paragraphs,
                    attribution,
                }
            }
            "list" => {
                let ordered = flag(self, "ordered");
                let item_spec = spec::element("item").unwrap();
                let mut items = Vec::new();
                for s in el.body.iter().flat_map(|b| &b.stmts) {
                    match &s.kind {
                        StmtKind::Element(item) if item.name.name == "item" => {
                            self.body_rule(item, item_spec);
                            let a = self.args(&item.name, &item.args, item_spec, env);
                            if self.require(&item.name, item_spec, &a)
                                && let Some(spans) =
                                    self.rich1(a.get(item_spec, "text").unwrap(), env)
                            {
                                items.push(spans);
                            }
                        }
                        _ => {
                            self.err("E0302", s.span, "a `list` contains only `item` statements")
                                .help = Some("write `item \"...\"` for each entry".into());
                        }
                    }
                }
                if items.is_empty() {
                    return;
                }
                BlockKind::List { ordered, items }
            }
            "image" => {
                let Some(resource) =
                    self.asset(args.get(spec, "asset").unwrap(), ResourceKind::Image)
                else {
                    return;
                };
                let width = match args.get(spec, "width") {
                    Some(e) => match &e.kind {
                        ExprKind::Percent(p) if (10.0..=100.0).contains(p) => p / 100.0,
                        ExprKind::Percent(_) => {
                            self.err("E0312", e.span, "image width must be between 10% and 100%");
                            return;
                        }
                        _ => {
                            self.err(
                                "E0307",
                                e.span,
                                format!(
                                    "expected a percentage such as `60%`, found {}",
                                    e.describe()
                                ),
                            );
                            return;
                        }
                    },
                    None => 1.0,
                };
                let fit = match args.get(spec, "fit") {
                    Some(e) => match self.word(e, spec::FITS, "fit") {
                        Some("cover") => ir::Fit::Cover,
                        Some(_) => ir::Fit::Contain,
                        None => return,
                    },
                    None => ir::Fit::Contain,
                };
                BlockKind::Image {
                    resource,
                    caption: caption(self),
                    width,
                    fit,
                }
            }
            "gallery" => {
                let mut resources = Vec::new();
                for e in args.positional() {
                    if let Some(r) = self.asset(e, ResourceKind::Image) {
                        resources.push(r);
                    }
                }
                if resources.len() != args.positional().len() {
                    return;
                }
                if !(2..=24).contains(&resources.len()) {
                    self.err(
                        "E0312",
                        el.name.span,
                        "a `gallery` needs between 2 and 24 images",
                    )
                    .help = Some("for a single picture use `image`".into());
                    return;
                }
                BlockKind::Gallery {
                    resources,
                    caption: caption(self),
                }
            }
            "video" => {
                let Some(resource) =
                    self.asset(args.get(spec, "asset").unwrap(), ResourceKind::Video)
                else {
                    return;
                };
                BlockKind::Video {
                    resource,
                    caption: caption(self),
                    autoplay: flag(self, "autoplay"),
                    looped: flag(self, "loop"),
                    muted: flag(self, "muted"),
                }
            }
            "audio" => {
                let Some(resource) =
                    self.asset(args.get(spec, "asset").unwrap(), ResourceKind::Audio)
                else {
                    return;
                };
                BlockKind::Audio {
                    resource,
                    caption: caption(self),
                }
            }
            "divider" => {
                let style = match args.get(spec, "style") {
                    Some(e) => match self.word(e, spec::DIVIDERS, "divider style") {
                        Some("rule") => ir::DividerStyle::Rule,
                        Some("blank") => ir::DividerStyle::Blank,
                        Some(_) => ir::DividerStyle::Ornament,
                        None => return,
                    },
                    None => ir::DividerStyle::Ornament,
                };
                BlockKind::Divider { style }
            }
            "space" => {
                let Some(lines) =
                    self.ranged(args.get(spec, "lines").unwrap(), 0.25, 40.0, "space")
                else {
                    return;
                };
                if bx.in_page {
                    self.err(
                        "E0302",
                        el.name.span,
                        "`space` has no meaning inside a `page`; position blocks with `at=`",
                    );
                    return;
                }
                BlockKind::Space { lines }
            }
            "break" => {
                if bx.in_page || bx.in_box {
                    self.err(
                        "E0302",
                        el.name.span,
                        "`break` cannot be used inside a `page` or `box`",
                    );
                    return;
                }
                BlockKind::PageBreak
            }
            "box" => {
                if bx.in_page {
                    self.err(
                        "E0302",
                        el.name.span,
                        "`box` cannot be used inside a `page`; frame the blocks directly",
                    );
                    return;
                }
                let style = match args.get(spec, "style") {
                    Some(e) => match self.word(e, spec::BOX_STYLES, "box style") {
                        Some("tint") => ir::BoxStyle::Tint,
                        Some("bar") => ir::BoxStyle::Bar,
                        Some(_) => ir::BoxStyle::Rule,
                        None => return,
                    },
                    None => ir::BoxStyle::Rule,
                };
                let title = args.get(spec, "title").and_then(|t| self.string(t));
                let blocks = self.blocks(
                    el.body.as_ref().map_or(&[][..], |b| &b.stmts),
                    env,
                    Bx {
                        in_box: true,
                        ..inner
                    },
                );
                BlockKind::Box {
                    style,
                    title,
                    blocks,
                }
            }
            "button" => {
                let Some(label) = self
                    .string(args.get(spec, "label").unwrap())
                    .filter(|l| !l.trim().is_empty())
                else {
                    return;
                };
                let errors_before = self.diags.len();
                let actions = self.element_actions(el, &args, spec, env);
                if actions.is_empty() && self.diags.len() > errors_before {
                    // The actions were reported individually; do not pile on.
                    return;
                }
                if actions.is_empty() {
                    self.err("E0308", el.name.span, "this button does nothing")
                        .help = Some(
                        "add `goto=target`, or a block of actions such as `score += 1`".into(),
                    );
                    return;
                }
                BlockKind::Button { label, actions }
            }
            "choice" => {
                let Some(prompt) = self.rich1(args.get(spec, "prompt").unwrap(), env) else {
                    return;
                };
                let errors_before = self.diags.len();
                let option_spec = spec::element("option").unwrap();
                let mut options = Vec::new();
                for s in el.body.iter().flat_map(|b| &b.stmts) {
                    match &s.kind {
                        StmtKind::Element(o) if o.name.name == "option" => {
                            let a = self.args(&o.name, &o.args, option_spec, env);
                            if !self.require(&o.name, option_spec, &a) {
                                continue;
                            }
                            let Some(label) = self.string(a.get(option_spec, "label").unwrap())
                            else {
                                continue;
                            };
                            let when = a.get(option_spec, "when").and_then(|w| self.cond(w, env));
                            let actions = self.element_actions(o, &a, option_spec, env);
                            options.push(ir::ChoiceOption {
                                label,
                                when,
                                actions,
                            });
                        }
                        _ => {
                            self.err(
                                "E0302",
                                s.span,
                                "a `choice` contains only `option` statements",
                            )
                            .help = Some(
                                "write `option \"Label\":` followed by indented actions".into(),
                            );
                        }
                    }
                }
                if options.is_empty() && self.diags.len() > errors_before {
                    return;
                }
                if options.is_empty() || options.len() > limits::MAX_OPTIONS {
                    self.err(
                        "E0312",
                        el.name.span,
                        format!(
                            "a `choice` needs between 1 and {} options",
                            limits::MAX_OPTIONS
                        ),
                    );
                    return;
                }
                BlockKind::Choice { prompt, options }
            }
            "quiz" => {
                let Some(question) = self.rich1(args.get(spec, "question").unwrap(), env) else {
                    return;
                };
                let errors_before = self.diags.len();
                let (answer_spec, explain_spec) = (
                    spec::element("answer").unwrap(),
                    spec::element("explain").unwrap(),
                );
                let mut answers = Vec::new();
                let mut explanation = None;
                let mut on_correct = Vec::new();
                for s in el.body.iter().flat_map(|b| &b.stmts) {
                    match &s.kind {
                        StmtKind::Element(c) if c.name.name == "answer" => {
                            self.body_rule(c, answer_spec);
                            let a = self.args(&c.name, &c.args, answer_spec, env);
                            if self.require(&c.name, answer_spec, &a)
                                && let Some(label) =
                                    self.string(a.get(answer_spec, "label").unwrap())
                            {
                                let correct = a
                                    .get(answer_spec, "correct")
                                    .and_then(|e| self.boolean(e))
                                    .unwrap_or(false);
                                answers.push(ir::QuizAnswer { label, correct });
                            }
                        }
                        StmtKind::Element(c) if c.name.name == "explain" => {
                            self.body_rule(c, explain_spec);
                            let a = self.args(&c.name, &c.args, explain_spec, env);
                            if explanation.is_some() {
                                self.err(
                                    "E0306",
                                    c.name.span,
                                    "a `quiz` can have only one `explain`",
                                );
                            } else if self.require(&c.name, explain_spec, &a) {
                                explanation = self.rich1(a.get(explain_spec, "text").unwrap(), env);
                            }
                        }
                        StmtKind::Element(c) if c.name.name == "on_correct" => {
                            let oc = spec::element("on_correct").unwrap();
                            self.args(&c.name, &c.args, oc, env);
                            if self.body_rule(c, oc) {
                                on_correct.extend(self.actions(
                                    &c.body.as_ref().unwrap().stmts,
                                    env,
                                    "on_correct",
                                ));
                            }
                        }
                        _ => {
                            self.err(
                                "E0302",
                                s.span,
                                "a `quiz` contains only `answer`, `explain` and `on_correct`",
                            );
                        }
                    }
                }
                if self.diags.len() > errors_before {
                    return;
                }
                if !(2..=limits::MAX_OPTIONS).contains(&answers.len()) {
                    self.err(
                        "E0312",
                        el.name.span,
                        format!(
                            "a `quiz` needs between 2 and {} answers",
                            limits::MAX_OPTIONS
                        ),
                    );
                    return;
                }
                if !answers.iter().any(|a| a.correct) {
                    self.err(
                        "E0308",
                        el.name.span,
                        "no answer of this `quiz` is marked correct",
                    )
                    .help = Some("add `correct=true` to the right `answer`".into());
                    return;
                }
                BlockKind::Quiz {
                    question,
                    answers,
                    explanation,
                    on_correct,
                }
            }
            "page" => {
                if !bx.chapter_top {
                    self.err(
                        "E0316",
                        el.name.span,
                        "`page` must be a direct child of `chapter`",
                    )
                    .help = Some(
                        "to show a page conditionally, put it in a chapter with `when=`".into(),
                    );
                    return;
                }
                let background = args.get(spec, "background").and_then(|b| self.color(b));
                let blocks = self.blocks(
                    el.body.as_ref().map_or(&[][..], |b| &b.stmts),
                    env,
                    Bx {
                        in_page: true,
                        in_box: false,
                        chapter_top: false,
                        depth: 1,
                    },
                );
                BlockKind::Page { background, blocks }
            }
            _ => unreachable!("block element `{name}` is in the spec table but not lowered"),
        };
        let mut b = self.block(kind);
        self.common(el, &args, spec, &mut b, bx);
        out.push(b);
    }

    /// Explain why an element name is not valid at this point.
    fn misplaced(&mut self, el: &Element) {
        let name = el.name.name.as_str();
        match spec::element(name) {
            Some(s) if s.place == Place::Top => {
                self.err(
                    "E0302",
                    el.name.span,
                    format!("`{name}` is a top-level statement and cannot be nested"),
                )
                .help = Some(format!(
                    "move `{name}` out to the start of a line with no indentation"
                ));
            }
            Some(s) if s.place == Place::Child => {
                self.err(
                    "E0302",
                    el.name.span,
                    format!("`{name}` is only valid inside `{}`", s.parent),
                );
            }
            Some(_) => {
                self.err("E0302", el.name.span, format!("`{name}` is an action and cannot appear among content")).help =
                    Some("put it in the block of a `button` or `option`, or use `button \"Label\", goto=target`".into());
            }
            None => {
                let names: Vec<String> = spec::block_names()
                    .map(String::from)
                    .chain(self.components.keys().cloned())
                    .collect();
                let candidates = names.iter().map(String::as_str);
                let help = match suggest(name, candidates) {
                    Some(s) => format!("did you mean `{s}`?"),
                    None => match name {
                        "p" | "para" | "paragraph" => {
                            "paragraphs are written `text \"...\"`".into()
                        }
                        "h1" | "h2" | "h3" | "title" => {
                            "use `section \"Title\":` or `heading \"...\"`".into()
                        }
                        "img" | "picture" | "figure" => {
                            "images are written `image asset_name`".into()
                        }
                        "hr" | "rule" | "separator" => "use `divider`".into(),
                        "br" | "newpage" | "pagebreak" | "page_break" => {
                            "use `break` to start a new page".into()
                        }
                        "link" | "a" => {
                            "links are inline: `text \"see [label](target_id)\"`".into()
                        }
                        "ul" | "ol" | "bullets" => "use `list:` with `item \"...\"` lines".into(),
                        _ => {
                            "run `darm reference` for the list of elements, or define a `component`"
                                .into()
                        }
                    },
                };
                self.err("E0301", el.name.span, format!("unknown element `{name}`"))
                    .help = Some(help);
            }
        }
    }

    fn expand(&mut self, el: &Element, env: &Env, bx: Bx, out: &mut Vec<Block>) {
        let name = el.name.name.clone();
        if self.stack.contains(&name) {
            self.err(
                "E0309",
                el.name.span,
                format!("component `{name}` uses itself"),
            )
            .help = Some("components cannot recurse; DARM has no loops".into());
            return;
        }
        if self.stack.len() >= 16 {
            self.err(
                "E0309",
                el.name.span,
                "components are nested more than 16 levels deep",
            );
            return;
        }
        if let Some(body) = &el.body {
            self.err(
                "E0313",
                body.span,
                format!("component `{name}` does not take a block"),
            )
            .help = Some("pass content as arguments".into());
        }
        let (params, body) = {
            let c = self.components.get_mut(&name).unwrap();
            c.used = true;
            (c.params, c.body)
        };
        let mut bound: Env = Vec::new();
        let mut positional = 0usize;
        for arg in &el.args {
            let value = self.resolve(&arg.value, env).clone();
            match &arg.name {
                None => match params.get(positional) {
                    Some(p) => {
                        bound.push((p.name.name.clone(), value));
                        positional += 1;
                    }
                    None => {
                        self.err(
                            "E0309",
                            arg.value.span,
                            format!("`{name}` takes {} argument(s)", params.len()),
                        );
                    }
                },
                Some(key) => {
                    if !params.iter().any(|p| p.name.name == key.name) {
                        let help = suggest(&key.name, params.iter().map(|p| p.name.name.as_str()))
                            .map(|s| format!("did you mean `{s}`?"));
                        self.err(
                            "E0303",
                            key.span,
                            format!("component `{name}` has no parameter `{}`", key.name),
                        )
                        .help = help;
                    } else if bound.iter().any(|(n, _)| *n == key.name) {
                        self.err(
                            "E0306",
                            key.span,
                            format!("parameter `{}` is given twice", key.name),
                        );
                    } else {
                        bound.push((key.name.clone(), value));
                    }
                }
            }
        }
        let mut complete = true;
        for p in params {
            if bound.iter().any(|(n, _)| *n == p.name.name) {
                continue;
            }
            match &p.default {
                Some(d) => bound.push((p.name.name.clone(), d.clone())),
                None => {
                    complete = false;
                    self.err(
                        "E0309",
                        el.name.span,
                        format!("`{name}` needs a value for `{}`", p.name.name),
                    );
                }
            }
        }
        if !complete {
            return;
        }
        self.stack.push(name);
        let blocks = self.blocks(&body.stmts, &bound, bx);
        self.stack.pop();
        out.extend(blocks);
    }
}

pub(crate) fn text_style(word: &str) -> ir::TextStyle {
    match word {
        "lead" => ir::TextStyle::Lead,
        "small" => ir::TextStyle::Small,
        "caption" => ir::TextStyle::Caption,
        "dropcap" => ir::TextStyle::Dropcap,
        "verse" => ir::TextStyle::Verse,
        "mono" => ir::TextStyle::Mono,
        "display" => ir::TextStyle::Display,
        _ => ir::TextStyle::Body,
    }
}

pub(crate) fn kind_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Image => "image",
        ResourceKind::Video => "video",
        ResourceKind::Audio => "audio",
        ResourceKind::Captions => "captions",
        ResourceKind::Font => "font",
    }
}

fn article_kind(kind: &str) -> String {
    article(kind)
}

/// `"int"` → `"an int"`, `"bool"` → `"a bool"`.
pub(crate) fn article(noun: &str) -> String {
    let an = noun.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {noun}", if an { "an" } else { "a" })
}

/// Plain strings keep the lexer's markup escapes (`\*`); drop the backslashes.
pub(crate) fn unescape_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[allow(dead_code)]
fn _param_used(_: &Param) {}
