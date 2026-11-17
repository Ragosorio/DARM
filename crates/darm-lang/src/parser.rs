//! Recursive-descent parser: tokens → [`ast::File`].
//!
//! The parser recovers at statement boundaries so that one run reports as
//! many independent errors as possible, and it bounds recursion depth so that
//! hostile input cannot overflow the stack.

use crate::ast::*;
use crate::diag::{Diagnostic, FileId, Span};
use crate::lexer::{self, Tok, Token};

/// Maximum nesting of blocks and of expressions.
pub const MAX_DEPTH: usize = 48;

/// Grammar versions this parser accepts in the `darm N` pragma.
pub const SUPPORTED_GRAMMAR: i64 = 1;

#[derive(Debug)]
pub struct Parsed {
    pub file: File,
    pub errors: Vec<Diagnostic>,
}

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    errors: Vec<Diagnostic>,
    depth: usize,
    file: FileId,
}

/// Internal marker: the error was already reported, unwind to the statement.
struct Bail;
type R<T> = Result<T, Bail>;

impl<'a> Parser<'a> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos.min(self.toks.len() - 1)].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> &Token {
        let t = &self.toks[self.pos.min(self.toks.len() - 1)];
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == tok {
            self.bump();
            true
        } else {
            false
        }
    }

    fn error(&mut self, code: &'static str, span: Span, msg: impl Into<String>) -> &mut Diagnostic {
        // A zero-width span (newline, dedent, eof) still needs a visible caret.
        let span = if span.end == span.start {
            Span {
                end: span.start + 1,
                ..span
            }
        } else {
            span
        };
        self.errors.push(Diagnostic::error(code, span, msg));
        self.errors.last_mut().unwrap()
    }

    fn unexpected(&mut self, expected: &str) -> Bail {
        let found = self.peek().describe();
        let span = self.span();
        self.error("E0203", span, format!("expected {expected}, found {found}"));
        Bail
    }

    fn expect(&mut self, tok: &Tok, expected: &str) -> R<Span> {
        if self.peek() == tok {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(expected))
        }
    }

    fn ident(&mut self, expected: &str) -> R<Ident> {
        match self.peek().clone() {
            Tok::Name(name) => {
                let span = self.bump().span;
                Ok(Ident { name, span })
            }
            _ => Err(self.unexpected(expected)),
        }
    }

    /// Skip the rest of the current statement, including an indented block.
    fn recover(&mut self) {
        loop {
            match self.peek() {
                Tok::Eof | Tok::Dedent => return,
                Tok::Newline => {
                    self.bump();
                    break;
                }
                _ => {
                    self.bump();
                }
            }
        }
        if self.peek() == &Tok::Indent {
            let mut depth = 0usize;
            loop {
                match self.peek() {
                    Tok::Indent => depth += 1,
                    Tok::Dedent => depth -= 1,
                    Tok::Eof => return,
                    _ => {}
                }
                self.bump();
                if depth == 0 {
                    return;
                }
            }
        }
    }

    fn end_of_statement(&mut self) -> R<()> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::Eof | Tok::Dedent => Ok(()),
            Tok::Name(_) | Tok::Str { .. } | Tok::Int(_) => {
                let found = self.peek().describe();
                let span = self.span();
                self.error(
                    "E0203",
                    span,
                    format!("expected `,` or the end of the line, found {found}"),
                )
                .help = Some("separate arguments with commas".into());
                Err(Bail)
            }
            _ => Err(self.unexpected("the end of the line")),
        }
    }

    // --- statements ---------------------------------------------------------

    fn statements(&mut self, top: bool) -> Vec<Stmt> {
        let mut out = Vec::new();
        let mut stray_indents = 0usize;
        loop {
            match self.peek() {
                Tok::Eof => break,
                Tok::Dedent if stray_indents > 0 => {
                    stray_indents -= 1;
                    self.bump();
                }
                Tok::Dedent if !top => break,
                Tok::Dedent | Tok::Newline => {
                    self.bump();
                }
                Tok::Indent => {
                    let span = self.span();
                    self.error("E0205", span, "unexpected indentation")
                        .help = Some("only lines that follow a `:` may be indented deeper; remove the extra spaces or end the previous line with `:`".into());
                    stray_indents += 1;
                    self.bump();
                }
                _ => match self.statement() {
                    Ok(s) => out.push(s),
                    Err(Bail) => self.recover(),
                },
            }
        }
        out
    }

    fn block(&mut self, owner: &str) -> R<Block> {
        if self.peek() != &Tok::Newline {
            let span = self.span();
            self.error("E0208", span, "expected a new line after `:`")
                .help = Some(format!(
                "put the contents of `{owner}` on the following lines, indented by 4 spaces"
            ));
            return Err(Bail);
        }
        self.bump();
        if self.peek() != &Tok::Indent {
            let span = self.prev_span();
            self.error(
                "E0204",
                span,
                format!("expected an indented block after `{owner} ...:`"),
            )
            .help =
                Some("indent the lines that belong to it by 4 spaces, or remove the `:`".into());
            // Not a Bail: the header is fine and what follows parses as siblings.
            return Ok(Block {
                stmts: Vec::new(),
                span,
            });
        }
        self.bump();
        if self.depth >= MAX_DEPTH {
            let span = self.span();
            self.error(
                "E0315",
                span,
                format!("blocks are nested more than {MAX_DEPTH} levels deep"),
            );
            // Skip the block.
            let mut depth = 1usize;
            while depth > 0 && self.peek() != &Tok::Eof {
                match self.peek() {
                    Tok::Indent => depth += 1,
                    Tok::Dedent => depth -= 1,
                    _ => {}
                }
                self.bump();
            }
            return Ok(Block {
                stmts: Vec::new(),
                span,
            });
        }
        self.depth += 1;
        let start = self.span();
        let stmts = self.statements(false);
        self.depth -= 1;
        let end = self.prev_span();
        self.eat(&Tok::Dedent);
        Ok(Block {
            stmts,
            span: start.to(end),
        })
    }

    fn statement(&mut self) -> R<Stmt> {
        let start = self.span();
        match self.peek().clone() {
            Tok::If => self.if_statement(),
            Tok::Elif | Tok::Else => {
                let kw = if self.peek() == &Tok::Elif {
                    "elif"
                } else {
                    "else"
                };
                self.error("E0206", start, format!("`{kw}` without a matching `if`"))
                    .help = Some(format!(
                    "`{kw}` must directly follow an `if` block at the same indentation"
                ));
                Err(Bail)
            }
            Tok::Name(name) => {
                if name == "component" && matches!(self.peek_at(1), Tok::Name(_)) {
                    return self.component();
                }
                match self.peek_at(1) {
                    Tok::Assign | Tok::PlusAssign | Tok::MinusAssign => {
                        let target = self.ident("a name")?;
                        let op = match self.bump().tok {
                            Tok::Assign => AssignOp::Set,
                            Tok::PlusAssign => AssignOp::Add,
                            _ => AssignOp::Sub,
                        };
                        let value = self.expr()?;
                        let span = start.to(value.span);
                        if self.peek() == &Tok::Colon {
                            let at = self.span();
                            self.error("E0203", at, "an assignment cannot be followed by `:`")
                                .help = Some(format!(
                                "to open a block write `{}:` without `=`",
                                target.name
                            ));
                            return Err(Bail);
                        }
                        self.end_of_statement()?;
                        Ok(Stmt {
                            kind: StmtKind::Assign { target, op, value },
                            span,
                        })
                    }
                    _ => self.element(),
                }
            }
            Tok::Str { .. } => {
                self.error("E0203", start, "a statement cannot start with a string")
                    .help =
                    Some("text needs an element name in front, for example `text \"...\"`".into());
                Err(Bail)
            }
            _ => Err(self.unexpected("a statement")),
        }
    }

    fn args(&mut self, closing: Option<&Tok>) -> R<Vec<Arg>> {
        let mut args = Vec::new();
        let mut named_seen: Option<Span> = None;
        loop {
            let done = match closing {
                Some(c) => self.peek() == c,
                None => matches!(
                    self.peek(),
                    Tok::Newline | Tok::Colon | Tok::Eof | Tok::Dedent
                ),
            };
            if done {
                break;
            }
            let arg = if matches!(self.peek(), Tok::Name(_)) && self.peek_at(1) == &Tok::Assign {
                let name = self.ident("an argument name")?;
                self.bump();
                named_seen = Some(name.span);
                Arg {
                    name: Some(name),
                    value: self.expr()?,
                }
            } else {
                let value = self.expr()?;
                if named_seen.is_some() {
                    self.error(
                        "E0207",
                        value.span,
                        "positional argument after a named argument",
                    )
                    .help =
                        Some("put positional arguments first, then `name=value` arguments".into());
                }
                Arg { name: None, value }
            };
            args.push(arg);
            if self.peek() == &Tok::Comma {
                let comma = self.bump().span;
                // `text "x",` followed by an unrelated line: the comma joined them.
                let dangling = match closing {
                    None => matches!(
                        self.peek(),
                        Tok::Newline | Tok::Colon | Tok::Eof | Tok::Dedent
                    ),
                    Some(_) => false,
                };
                if dangling {
                    self.error("E0203", comma, "trailing comma at the end of a statement")
                        .help = Some(
                        "remove the comma, or continue the argument list on the next line".into(),
                    );
                }
            } else {
                break;
            }
        }
        Ok(args)
    }

    fn element(&mut self) -> R<Stmt> {
        let name = self.ident("an element name")?;
        let args = self.args(None)?;
        let mut span = name.span.to(self.prev_span());
        let body = if self.peek() == &Tok::Colon {
            span = span.to(self.bump().span);
            Some(self.block(&name.name)?)
        } else {
            self.end_of_statement()?;
            None
        };
        Ok(Stmt {
            kind: StmtKind::Element(Element { name, args, body }),
            span,
        })
    }

    fn if_statement(&mut self) -> R<Stmt> {
        let start = self.span();
        let mut branches = Vec::new();
        let mut otherwise = None;
        let mut keyword = self.bump().span; // `if`
        loop {
            let cond = self.expr()?;
            self.expect(&Tok::Colon, "`:` after the condition")?;
            let body = self.block("if")?;
            branches.push(Branch {
                keyword,
                cond,
                body,
            });
            match self.peek() {
                Tok::Elif => keyword = self.bump().span,
                Tok::Else => {
                    self.bump();
                    self.expect(&Tok::Colon, "`:` after `else`")?;
                    otherwise = Some(self.block("else")?);
                    break;
                }
                _ => break,
            }
        }
        let span = start.to(branches[0].cond.span);
        Ok(Stmt {
            kind: StmtKind::If {
                branches,
                otherwise,
            },
            span,
        })
    }

    fn component(&mut self) -> R<Stmt> {
        let start = self.bump().span; // `component`
        let name = self.ident("the component name")?;
        if self.peek() != &Tok::LParen {
            let span = self.span();
            self.error(
                "E0203",
                span,
                format!(
                    "expected `(` after the component name, found {}",
                    self.peek().describe()
                ),
            )
            .help = Some(format!(
                "write `component {}():` even when it has no parameters",
                name.name
            ));
            return Err(Bail);
        }
        self.bump();
        let mut params = Vec::new();
        while self.peek() != &Tok::RParen {
            let pname = self.ident("a parameter name")?;
            let default = if self.eat(&Tok::Assign) {
                Some(self.expr()?)
            } else {
                None
            };
            params.push(Param {
                name: pname,
                default,
            });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let close = self.expect(&Tok::RParen, "`)` to close the parameter list")?;
        self.expect(&Tok::Colon, "`:` after the parameter list")?;
        let body = self.block("component")?;
        Ok(Stmt {
            kind: StmtKind::Component { name, params, body },
            span: start.to(close),
        })
    }

    // --- expressions --------------------------------------------------------

    fn deeper(&mut self) -> R<()> {
        if self.depth >= MAX_DEPTH * 2 {
            let span = self.span();
            self.error("E0315", span, "this expression is nested too deeply");
            return Err(Bail);
        }
        self.depth += 1;
        Ok(())
    }

    fn expr(&mut self) -> R<Expr> {
        self.deeper()?;
        let r = self.expr_inner();
        self.depth -= 1;
        r
    }

    fn expr_inner(&mut self) -> R<Expr> {
        let value = self.binary(1)?;
        if self.peek() != &Tok::In {
            return Ok(value);
        }
        self.bump();
        let lo = self.binary(5)?;
        let domain = if self.eat(&Tok::DotDot) {
            let hi = self.binary(5)?;
            let span = lo.span.to(hi.span);
            Expr {
                kind: ExprKind::Range {
                    lo: Box::new(lo),
                    hi: Box::new(hi),
                },
                span,
            }
        } else if matches!(lo.kind, ExprKind::List(_)) {
            lo
        } else {
            self.error(
                "E0203",
                lo.span,
                "expected a range `a..b` or a list `[...]` after `in`",
            );
            return Err(Bail);
        };
        let span = value.span.to(domain.span);
        Ok(Expr {
            kind: ExprKind::In {
                value: Box::new(value),
                domain: Box::new(domain),
            },
            span,
        })
    }

    fn binary_op(&self) -> Option<BinaryOp> {
        Some(match self.peek() {
            Tok::Or => BinaryOp::Or,
            Tok::And => BinaryOp::And,
            Tok::EqEq => BinaryOp::Eq,
            Tok::NotEq => BinaryOp::Ne,
            Tok::Lt => BinaryOp::Lt,
            Tok::Le => BinaryOp::Le,
            Tok::Gt => BinaryOp::Gt,
            Tok::Ge => BinaryOp::Ge,
            Tok::Plus => BinaryOp::Add,
            Tok::Minus => BinaryOp::Sub,
            Tok::Star => BinaryOp::Mul,
            _ => return None,
        })
    }

    /// Precedence climbing. `not` sits between `and` (2) and comparisons (4).
    fn binary(&mut self, min: u8) -> R<Expr> {
        let mut left = if self.peek() == &Tok::Not && min <= 3 {
            let start = self.bump().span;
            self.deeper()?;
            let operand = self.binary(3);
            self.depth -= 1;
            let operand = operand?;
            let span = start.to(operand.span);
            Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(operand),
                },
                span,
            }
        } else {
            self.unary()?
        };
        while let Some(op) = self.binary_op() {
            let prec = op.precedence();
            if prec < min {
                break;
            }
            let op_span = self.bump().span;
            let is_comparison = prec == 4;
            self.deeper()?;
            let right = self.binary(prec + 1);
            self.depth -= 1;
            let right = right?;
            let span = left.span.to(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
            if is_comparison && self.binary_op().is_some_and(|o| o.precedence() == 4) {
                let at = self.span();
                self.error("E0203", at.to(op_span), "comparisons cannot be chained")
                    .help =
                    Some("combine two comparisons with `and`, e.g. `0 < x and x < 5`".into());
                return Err(Bail);
            }
        }
        if self.peek() == &Tok::Assign
            && min == 1
            && matches!(left.kind, ExprKind::Name(_) | ExprKind::Binary { .. })
        {
            let at = self.span();
            self.error("E0203", at, "`=` is assignment; use `==` to compare");
            return Err(Bail);
        }
        Ok(left)
    }

    fn unary(&mut self) -> R<Expr> {
        if self.peek() == &Tok::Minus {
            let start = self.bump().span;
            self.deeper()?;
            let operand = self.unary();
            self.depth -= 1;
            let operand = operand?;
            let span = start.to(operand.span);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(operand),
                },
                span,
            });
        }
        self.atom()
    }

    fn atom(&mut self) -> R<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(i) => ExprKind::Int(i),
            Tok::Decimal(d) => ExprKind::Decimal(d),
            Tok::Percent(p) => ExprKind::Percent(p),
            Tok::Duration(d) => ExprKind::Duration(d),
            Tok::Str { value, block } => ExprKind::Str { value, block },
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::Name(name) => {
                if self.peek_at(1) == &Tok::LParen && self.toks[self.pos + 1].span.start == span.end
                {
                    let name = self.ident("a name")?;
                    self.bump();
                    let args = self.args(Some(&Tok::RParen))?;
                    let close = self.expect(&Tok::RParen, "`)` to close the call")?;
                    return Ok(Expr {
                        kind: ExprKind::Call { name, args },
                        span: span.to(close),
                    });
                }
                if matches!(name.as_str(), "True" | "False" | "None") {
                    self.error("E0203", span, format!("`{name}` is not a DARM value"))
                        .help = Some(match name.as_str() {
                        "True" => "booleans are lower-case: `true`".into(),
                        "False" => "booleans are lower-case: `false`".into(),
                        _ => "there is no `None`; omit the argument instead".to_string(),
                    });
                    return Err(Bail);
                }
                ExprKind::Name(name)
            }
            Tok::LBracket => {
                self.bump();
                let mut items = Vec::new();
                while self.peek() != &Tok::RBracket {
                    items.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let close = self.expect(&Tok::RBracket, "`]` to close the list")?;
                return Ok(Expr {
                    kind: ExprKind::List(items),
                    span: span.to(close),
                });
            }
            Tok::LParen => {
                self.bump();
                let first = self.expr()?;
                if self.peek() == &Tok::Comma {
                    let mut items = vec![first];
                    while self.eat(&Tok::Comma) {
                        if self.peek() == &Tok::RParen {
                            break;
                        }
                        items.push(self.expr()?);
                    }
                    let close = self.expect(&Tok::RParen, "`)` to close the tuple")?;
                    return Ok(Expr {
                        kind: ExprKind::Tuple(items),
                        span: span.to(close),
                    });
                }
                let close = self.expect(&Tok::RParen, "`)`")?;
                return Ok(Expr {
                    kind: ExprKind::Paren(Box::new(first)),
                    span: span.to(close),
                });
            }
            _ => return Err(self.unexpected("a value")),
        };
        self.bump();
        Ok(Expr { kind, span })
    }

    fn pragma(&mut self) -> Option<(i64, Span)> {
        while self.peek() == &Tok::Newline {
            self.bump();
        }
        let start = self.span();
        if let (Tok::Name(n), Tok::Int(v)) = (self.peek().clone(), self.peek_at(1).clone())
            && n == "darm"
            && matches!(self.peek_at(2), Tok::Newline | Tok::Eof)
        {
            self.bump();
            let span = start.to(self.bump().span);
            self.eat(&Tok::Newline);
            if v != SUPPORTED_GRAMMAR {
                self.error("E0202", span, format!("this compiler implements grammar version {SUPPORTED_GRAMMAR}, the file asks for {v}"))
                        .help = Some(if v > SUPPORTED_GRAMMAR {
                            "update the `darm` tool, or write `darm 1` if the file only uses version 1 features".into()
                        } else {
                            format!("write `darm {SUPPORTED_GRAMMAR}`")
                        });
            }
            return Some((v, span));
        }
        let at = Span::new(self.file, 0, 1);
        self.errors.push(
            Diagnostic::error(
                "E0201",
                at,
                "the file does not start with a `darm` version line",
            )
            .with_help("add `darm 1` as the first line of every .darml file"),
        );
        None
    }
}

/// Parse one source file.
pub fn parse(file: FileId, src: &str) -> Parsed {
    let lexed = lexer::lex(file, src);
    let mut p = Parser {
        toks: &lexed.tokens,
        pos: 0,
        errors: Vec::new(),
        depth: 0,
        file,
    };
    let pragma = p.pragma();
    let stmts = p.statements(true);
    let mut errors = lexed.errors;
    errors.append(&mut p.errors);
    errors.sort_by_key(|d| d.span.map_or(0, |s| s.start));
    Parsed {
        file: File {
            file,
            pragma,
            stmts,
            comments: lexed.comments,
        },
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> File {
        let p = parse(FileId(0), src);
        assert!(p.errors.is_empty(), "{:#?}", p.errors);
        p.file
    }

    fn codes(src: &str) -> Vec<&'static str> {
        parse(FileId(0), src)
            .errors
            .iter()
            .map(|e| e.code)
            .collect()
    }

    #[test]
    fn parses_elements_blocks_and_assignments() {
        let f = ok(
            "darm 1\n\nbook \"T\":\n    author = \"A\"\n    credit \"Role\", \"Name\"\nchapter \"One\", id=one:\n    text \"Hi\", style=lead\n",
        );
        assert_eq!(f.pragma.unwrap().0, 1);
        assert_eq!(f.stmts.len(), 2);
        let StmtKind::Element(book) = &f.stmts[0].kind else {
            panic!()
        };
        assert_eq!(book.name.name, "book");
        let body = book.body.as_ref().unwrap();
        assert!(matches!(body.stmts[0].kind, StmtKind::Assign { .. }));
        assert!(matches!(body.stmts[1].kind, StmtKind::Element(_)));
        let StmtKind::Element(ch) = &f.stmts[1].kind else {
            panic!()
        };
        assert_eq!(ch.args.len(), 2);
        assert_eq!(ch.args[1].name.as_ref().unwrap().name, "id");
    }

    #[test]
    fn expression_precedence() {
        let f = ok("darm 1\nif not a and b + 1 * 2 >= 3 or c:\n    x\n");
        let StmtKind::If { branches, .. } = &f.stmts[0].kind else {
            panic!()
        };
        // ((not a) and ((b + (1*2)) >= 3)) or c
        let ExprKind::Binary {
            op: BinaryOp::Or,
            left,
            ..
        } = &branches[0].cond.kind
        else {
            panic!()
        };
        let ExprKind::Binary {
            op: BinaryOp::And,
            left: not_a,
            right: cmp,
        } = &left.kind
        else {
            panic!()
        };
        assert!(matches!(
            not_a.kind,
            ExprKind::Unary {
                op: UnaryOp::Not,
                ..
            }
        ));
        let ExprKind::Binary {
            op: BinaryOp::Ge,
            left: sum,
            ..
        } = &cmp.kind
        else {
            panic!()
        };
        let ExprKind::Binary {
            op: BinaryOp::Add,
            right: mul,
            ..
        } = &sum.kind
        else {
            panic!()
        };
        assert!(matches!(
            mul.kind,
            ExprKind::Binary {
                op: BinaryOp::Mul,
                ..
            }
        ));
    }

    #[test]
    fn state_declarations_and_calls() {
        let f = ok(
            "darm 1\nstate:\n    courage = 0 in 0..5\n    route = \"a\" in [\"a\", \"b\"]\nassets:\n    pic = image(\"a.png\", alt=\"x\")\n",
        );
        let StmtKind::Element(state) = &f.stmts[0].kind else {
            panic!()
        };
        let StmtKind::Assign { value, .. } = &state.body.as_ref().unwrap().stmts[0].kind else {
            panic!()
        };
        let ExprKind::In { domain, .. } = &value.kind else {
            panic!()
        };
        assert!(matches!(domain.kind, ExprKind::Range { .. }));
        let StmtKind::Element(assets) = &f.stmts[1].kind else {
            panic!()
        };
        let StmtKind::Assign { value, .. } = &assets.body.as_ref().unwrap().stmts[0].kind else {
            panic!()
        };
        let ExprKind::Call { name, args } = &value.kind else {
            panic!()
        };
        assert_eq!(name.name, "image");
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn if_elif_else_and_components() {
        let f = ok(
            "darm 1\ncomponent note(body, label=\"Note\"):\n    text body\nif a:\n    x\nelif b:\n    y\nelse:\n    z\n",
        );
        assert!(
            matches!(&f.stmts[0].kind, StmtKind::Component { params, .. } if params.len() == 2)
        );
        assert!(
            matches!(&f.stmts[1].kind, StmtKind::If { branches, otherwise: Some(_) } if branches.len() == 2)
        );
    }

    #[test]
    fn syntax_errors_have_specific_codes() {
        assert_eq!(codes("book \"T\":\n    a = 1\n"), ["E0201"]);
        assert_eq!(codes("darm 2\n"), ["E0202"]);
        assert_eq!(codes("darm 1\ntext \"a\" \"b\"\n"), ["E0203"]);
        assert_eq!(codes("darm 1\nchapter \"a\":\ntext \"b\"\n"), ["E0204"]);
        assert_eq!(codes("darm 1\ntext \"a\"\n    text \"b\"\n"), ["E0205"]);
        assert_eq!(codes("darm 1\nelse:\n    x\n"), ["E0206"]);
        assert_eq!(codes("darm 1\nimage a=1, b\n"), ["E0207"]);
        assert_eq!(codes("darm 1\nchapter \"a\": text \"b\"\n"), ["E0208"]);
        assert_eq!(codes("darm 1\nif a = 1:\n    x\n"), ["E0203"]);
        assert_eq!(codes("darm 1\nx = True\n"), ["E0203"]);
    }

    #[test]
    fn recovers_and_reports_several_errors() {
        let c = codes(
            "darm 1\ntext \"a\" \"b\"\nchapter \"x\":\n    image = = 3\n    text \"ok\"\nelse:\n    y\n",
        );
        assert_eq!(c, ["E0203", "E0203", "E0206"]);
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let mut src = String::from("darm 1\nx = ");
        src.push_str(&"(".repeat(5000));
        src.push('1');
        src.push_str(&")".repeat(5000));
        src.push('\n');
        assert!(codes(&src).contains(&"E0315"));
        let mut src = String::from("darm 1\n");
        for i in 0..200 {
            src.push_str(&" ".repeat(i * 4));
            src.push_str("box:\n");
        }
        src.push_str(&" ".repeat(200 * 4));
        src.push_str("text \"x\"\n");
        assert!(codes(&src).contains(&"E0315"));
        let src = format!("darm 1\nx = {}1\n", "not ".repeat(5000));
        assert!(codes(&src).contains(&"E0315"));
        let src = format!("darm 1\nx = {}1\n", "-".repeat(5000));
        assert!(codes(&src).contains(&"E0315"));
    }
}
