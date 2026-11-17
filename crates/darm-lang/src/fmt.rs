//! Canonical formatter for `.darml` (`darm fmt`).
//!
//! The formatter prints the AST back with 4-space indentation, normalised
//! spacing and wrapped argument lists, and re-attaches comments. It never
//! changes meaning: the tests check that the formatted text parses to the same
//! tree and that formatting is idempotent. Literals are copied from the
//! source verbatim so that numbers and escapes keep the author's spelling.

use crate::ast::*;
use crate::diag::{Diagnostic, FileId, SourceFile};
use crate::lexer::{Comment, dedent_block};
use crate::parser;

/// Lines longer than this are wrapped at argument boundaries.
pub const MAX_WIDTH: usize = 100;
const INDENT: usize = 4;
const CONTINUATION: usize = 8;

struct Fmt<'a> {
    src: &'a SourceFile,
    comments: &'a [Comment],
    next_comment: usize,
    out: String,
    /// Source line of the last emitted item (0 = nothing yet).
    prev_line: usize,
}

impl<'a> Fmt<'a> {
    fn line_of(&self, offset: u32) -> usize {
        self.src.line_col(offset).0
    }

    fn col_of(&self, offset: u32) -> usize {
        self.src.line_col(offset).1 - 1
    }

    fn raw(&self, e: &Expr) -> &'a str {
        &self.src.text[e.span.start as usize..e.span.end as usize]
    }

    fn pad(&mut self, indent: usize) {
        self.out.extend(std::iter::repeat_n(' ', indent));
    }

    /// Blank-line policy before an item that starts on source line `line`.
    fn separate(&mut self, line: usize, top: bool, first_in_block: bool, after_comment: bool) {
        if first_in_block {
            return;
        }
        let had_blank = line > self.prev_line + 1;
        let force = top && !after_comment;
        if (had_blank || force) && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    /// Emit own-line comments that precede `before`. Returns `true` if any were written.
    fn leading_comments(
        &mut self,
        before: u32,
        indent: usize,
        top: bool,
        mut first: bool,
        min_column: Option<usize>,
    ) -> bool {
        let mut any = false;
        while let Some(c) = self.comments.get(self.next_comment) {
            if c.span.start >= before || min_column.is_some_and(|m| (c.column as usize) < m) {
                break;
            }
            let line = self.line_of(c.span.start);
            self.separate(line, top, first, any);
            self.pad(indent);
            self.out.push('#');
            self.out.push_str(&c.text);
            self.out.push('\n');
            self.prev_line = line;
            self.next_comment += 1;
            any = true;
            first = false;
        }
        any
    }

    /// A comment that sits on `line` after the statement header.
    fn trailing_comment(&mut self, line: usize) {
        if let Some(c) = self.comments.get(self.next_comment)
            && !c.own_line
            && self.line_of(c.span.start) == line
        {
            self.out.push_str("  #");
            self.out.push_str(&c.text);
            self.next_comment += 1;
        }
    }

    fn block(&mut self, stmts: &[Stmt], indent: usize, top: bool, limit: u32) {
        let source_column = stmts.first().map(|s| self.col_of(s.span.start));
        for (i, stmt) in stmts.iter().enumerate() {
            let next = stmts.get(i + 1).map_or(limit, |s| s.span.start);
            self.statement(stmt, indent, top, i == 0, next);
        }
        // Comments after the last statement that are indented like the block belong to it.
        if !top {
            self.leading_comments(limit, indent, false, stmts.is_empty(), source_column);
        }
    }

    fn header(
        &mut self,
        stmt_start: u32,
        stmt_end: u32,
        text: &str,
        indent: usize,
        top: bool,
        first: bool,
    ) {
        // Comments inside a wrapped header are hoisted above it.
        let had_comment = self.leading_comments(stmt_end, indent, top, first, None);
        let line = self.line_of(stmt_start);
        self.separate(line, top, first && !had_comment, had_comment);
        self.pad(indent);
        self.out.push_str(text);
        let end_line = self.line_of(stmt_end.saturating_sub(1).max(stmt_start));
        self.trailing_comment(end_line);
        self.out.push('\n');
        self.prev_line = end_line;
    }

    fn statement(&mut self, stmt: &Stmt, indent: usize, top: bool, first: bool, limit: u32) {
        match &stmt.kind {
            StmtKind::Element(el) => {
                let args: Vec<String> = el.args.iter().map(|a| self.arg(a, indent)).collect();
                let suffix = if el.body.is_some() { ":" } else { "" };
                let prefix = if args.is_empty() {
                    el.name.name.clone()
                } else {
                    format!("{} ", el.name.name)
                };
                let text = wrap(&prefix, &args, suffix, indent);
                self.header(stmt.span.start, stmt.span.end, &text, indent, top, first);
                if let Some(body) = &el.body {
                    self.block(&body.stmts, indent + INDENT, false, limit);
                }
            }
            StmtKind::Assign { target, op, value } => {
                let prefix = format!("{} {} ", target.name, op.symbol());
                let text = match &value.kind {
                    ExprKind::Call { name, args } => {
                        let args: Vec<String> = args.iter().map(|a| self.arg(a, indent)).collect();
                        wrap(&format!("{prefix}{}(", name.name), &args, ")", indent)
                    }
                    _ => format!("{prefix}{}", self.expr(value, indent)),
                };
                self.header(stmt.span.start, stmt.span.end, &text, indent, top, first);
            }
            StmtKind::If {
                branches,
                otherwise,
            } => {
                for (i, b) in branches.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "elif" };
                    let text = format!("{kw} {}:", self.expr(&b.cond, indent));
                    let next = branches
                        .get(i + 1)
                        .map(|n| n.keyword.start)
                        .or(otherwise.as_ref().map(|o| o.span.start))
                        .unwrap_or(limit);
                    self.header(
                        b.keyword.start,
                        b.cond.span.end,
                        &text,
                        indent,
                        top && i == 0,
                        first && i == 0,
                    );
                    self.block(&b.body.stmts, indent + INDENT, false, next);
                }
                if let Some(o) = otherwise {
                    // `else:` has no node of its own; it sits on the line before its block.
                    self.leading_comments(o.span.start, indent, false, false, None);
                    self.pad(indent);
                    self.out.push_str("else:\n");
                    self.prev_line = self.line_of(o.span.start).saturating_sub(1);
                    self.block(&o.stmts, indent + INDENT, false, limit);
                }
            }
            StmtKind::Component { name, params, body } => {
                let params: Vec<String> = params
                    .iter()
                    .map(|p| match &p.default {
                        Some(d) => format!("{}={}", p.name.name, self.expr(d, indent)),
                        None => p.name.name.clone(),
                    })
                    .collect();
                let text = wrap(&format!("component {}(", name.name), &params, "):", indent);
                self.header(stmt.span.start, stmt.span.end, &text, indent, top, first);
                self.block(&body.stmts, indent + INDENT, false, limit);
            }
        }
    }

    fn arg(&self, a: &Arg, indent: usize) -> String {
        match &a.name {
            Some(n) => format!("{}={}", n.name, self.expr(&a.value, indent)),
            None => self.expr(&a.value, indent),
        }
    }

    fn expr(&self, e: &Expr, indent: usize) -> String {
        match &e.kind {
            ExprKind::Int(_)
            | ExprKind::Decimal(_)
            | ExprKind::Percent(_)
            | ExprKind::Duration(_) => self.raw(e).to_string(),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Name(n) => n.clone(),
            ExprKind::Str { block, .. } => {
                let raw = self.raw(e);
                if !*block || !raw.contains('\n') {
                    return raw.to_string();
                }
                let inner = dedent_block(&raw[3..raw.len() - 3]);
                let pad = " ".repeat(indent + INDENT);
                let mut s = String::from("\"\"\"\n");
                for line in inner.split('\n') {
                    if !line.is_empty() {
                        s.push_str(&pad);
                        s.push_str(line.trim_end());
                    }
                    s.push('\n');
                }
                s.push_str(&pad);
                s.push_str("\"\"\"");
                s
            }
            ExprKind::List(items) => {
                format!(
                    "[{}]",
                    items
                        .iter()
                        .map(|i| self.expr(i, indent))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ExprKind::Tuple(items) => {
                let inner = items
                    .iter()
                    .map(|i| self.expr(i, indent))
                    .collect::<Vec<_>>()
                    .join(", ");
                if items.len() == 1 {
                    format!("({inner},)")
                } else {
                    format!("({inner})")
                }
            }
            ExprKind::Call { name, args } => {
                format!(
                    "{}({})",
                    name.name,
                    args.iter()
                        .map(|a| self.arg(a, indent))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ExprKind::Unary {
                op: UnaryOp::Not,
                expr,
            } => format!("not {}", self.expr(expr, indent)),
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr,
            } => format!("-{}", self.expr(expr, indent)),
            ExprKind::Binary { op, left, right } => {
                format!(
                    "{} {} {}",
                    self.expr(left, indent),
                    op.symbol(),
                    self.expr(right, indent)
                )
            }
            ExprKind::Range { lo, hi } => {
                format!("{}..{}", self.expr(lo, indent), self.expr(hi, indent))
            }
            ExprKind::In { value, domain } => format!(
                "{} in {}",
                self.expr(value, indent),
                self.expr(domain, indent)
            ),
            ExprKind::Paren(inner) => format!("({})", self.expr(inner, indent)),
        }
    }
}

/// Join `prefix item, item, … suffix`, wrapping after commas when the line
/// would exceed [`MAX_WIDTH`]. Items that contain newlines are never re-wrapped.
fn wrap(prefix: &str, items: &[String], suffix: &str, indent: usize) -> String {
    let one_line = format!("{prefix}{}{suffix}", items.join(", "));
    let width = |s: &str| s.chars().count();
    if items.len() < 2
        || items.iter().any(|i| i.contains('\n'))
        || indent + width(&one_line) <= MAX_WIDTH
    {
        return one_line;
    }
    let pad = " ".repeat(indent + CONTINUATION);
    let mut out = String::from(prefix);
    let mut line_width = indent + width(prefix);
    for (i, item) in items.iter().enumerate() {
        let last = i + 1 == items.len();
        let piece = if last {
            format!("{item}{suffix}")
        } else {
            format!("{item},")
        };
        if i > 0 {
            if line_width + 1 + width(&piece) > MAX_WIDTH {
                out.push('\n');
                out.push_str(&pad);
                line_width = indent + CONTINUATION;
            } else {
                out.push(' ');
                line_width += 1;
            }
        }
        line_width += width(&piece);
        out.push_str(&piece);
    }
    out
}

/// Format a source file. Fails with the parse diagnostics if the file does
/// not parse (a formatter must not guess at broken code).
pub fn format_source(file: FileId, name: &str, src: &str) -> Result<String, Vec<Diagnostic>> {
    let parsed = parser::parse(file, src);
    if !parsed.errors.is_empty() {
        return Err(parsed.errors);
    }
    let source = SourceFile::new(name, src);
    let mut f = Fmt {
        src: &source,
        comments: &parsed.file.comments,
        next_comment: 0,
        out: String::new(),
        prev_line: 0,
    };
    let end = src.len() as u32;
    if let Some((version, span)) = parsed.file.pragma {
        // Comments above the pragma (licence headers) stay above it.
        f.leading_comments(span.start, 0, false, true, None);
        f.out.push_str(&format!("darm {version}"));
        let line = f.line_of(span.start);
        f.trailing_comment(line);
        f.out.push('\n');
        f.prev_line = line;
    }
    if !parsed.file.stmts.is_empty() {
        f.out.push('\n');
        // The forced blank line after the pragma is already there.
        let stmts = &parsed.file.stmts;
        for (i, stmt) in stmts.iter().enumerate() {
            let next = stmts.get(i + 1).map_or(end, |s| s.span.start);
            f.statement(stmt, 0, true, i == 0, next);
        }
    }
    f.leading_comments(
        end,
        0,
        true,
        parsed.file.stmts.is_empty() && parsed.file.pragma.is_none(),
        None,
    );
    while f.out.ends_with("\n\n") {
        f.out.pop();
    }
    Ok(f.out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::same_meaning;

    fn fmt(src: &str) -> String {
        let out = format_source(FileId(0), "t.darml", src).expect("source parses");
        let again = format_source(FileId(0), "t.darml", &out).expect("formatted source parses");
        assert_eq!(out, again, "formatting is not idempotent");
        let a = parser::parse(FileId(0), src).file;
        let b = parser::parse(FileId(0), &out).file;
        assert!(
            same_meaning(&a, &b),
            "formatting changed the AST\n--- in\n{src}\n--- out\n{out}"
        );
        assert_eq!(
            a.comments.len(),
            b.comments.len(),
            "formatting lost a comment"
        );
        out
    }

    #[test]
    fn normalises_indentation_and_spacing() {
        let out = fmt(
            "darm 1\nbook \"T\":\n  author=\"A\"\n  language   =  \"en\"\nchapter \"One\" ,id = one:\n  text \"Hi\",style=lead\n  if a>=1 and not b:\n     text \"x\"\n  else:\n     text \"y\"\n",
        );
        assert_eq!(
            out,
            "darm 1\n\nbook \"T\":\n    author = \"A\"\n    language = \"en\"\n\nchapter \"One\", id=one:\n    text \"Hi\", style=lead\n    if a >= 1 and not b:\n        text \"x\"\n    else:\n        text \"y\"\n"
        );
    }

    #[test]
    fn keeps_comments_and_single_blank_lines() {
        let out = fmt(
            "# licence\ndarm 1  # version\n# about the book\nbook \"T\":  # trailing\n    author = \"A\"\n\n\n\n    # before language\n    language = \"en\"\n    # end of block\n# top level\nchapter \"C\":\n    text \"x\"\n# eof\n",
        );
        assert_eq!(
            out,
            "# licence\ndarm 1  # version\n\n# about the book\nbook \"T\":  # trailing\n    author = \"A\"\n\n    # before language\n    language = \"en\"\n    # end of block\n\n# top level\nchapter \"C\":\n    text \"x\"\n\n# eof\n"
        );
    }

    #[test]
    fn reindents_text_blocks() {
        let out = fmt(
            "darm 1\nchapter \"C\":\n  text \"\"\"\n      One\n        two\n\n      Three\n      \"\"\", style=verse\n",
        );
        assert_eq!(
            out,
            "darm 1\n\nchapter \"C\":\n    text \"\"\"\n        One\n          two\n\n        Three\n        \"\"\", style=verse\n"
        );
    }

    #[test]
    fn wraps_long_argument_lists() {
        let out = fmt(
            "darm 1\nassets:\n    film = video(\"media/harbour-at-dawn.mp4\", alt=\"Fog lifting over a harbour at dawn\", poster=\"media/harbour.jpg\", captions=\"media/harbour.en.vtt\")\n",
        );
        assert!(out.lines().all(|l| l.chars().count() <= MAX_WIDTH), "{out}");
        assert!(out.contains("film = video(\"media/harbour-at-dawn.mp4\", alt=\"Fog lifting over a harbour at dawn\",\n            poster="), "{out}");
    }

    #[test]
    fn refuses_broken_source() {
        assert!(format_source(FileId(0), "t", "darm 1\ntext \"a\" \"b\"\n").is_err());
    }

    #[test]
    fn hoists_comments_from_wrapped_headers() {
        let out = fmt("darm 1\nx = f(1,  # one\n      2)\n");
        assert_eq!(out, "darm 1\n\n# one\nx = f(1, 2)\n");
    }
}
