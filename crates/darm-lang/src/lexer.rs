//! Lexer for `.darml` source.
//!
//! Hand-written (no parser-generator dependency) because the language is
//! indentation-sensitive and good diagnostics need precise control over
//! recovery. The lexer turns indentation into `Indent`/`Dedent` tokens like
//! Python does, joins lines inside brackets and after a trailing comma, and
//! collects comments on the side so the formatter can re-emit them.

use crate::diag::{Diagnostic, FileId, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Name(String),
    Int(i64),
    Decimal(f64),
    /// `60%` → 60.0
    Percent(f64),
    /// Milliseconds.
    Duration(u32),
    /// `block` is true for `"""` strings.
    Str {
        value: String,
        block: bool,
    },
    True,
    False,
    And,
    Or,
    Not,
    In,
    If,
    Elif,
    Else,
    Assign,
    PlusAssign,
    MinusAssign,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    DotDot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Newline,
    Indent,
    Dedent,
    Eof,
}

impl Tok {
    /// How the token is named in "expected X, found Y" messages.
    pub fn describe(&self) -> String {
        match self {
            Tok::Name(n) => format!("`{n}`"),
            Tok::Int(_) | Tok::Decimal(_) => "a number".into(),
            Tok::Percent(_) => "a percentage".into(),
            Tok::Duration(_) => "a duration".into(),
            Tok::Str { .. } => "a string".into(),
            Tok::True => "`true`".into(),
            Tok::False => "`false`".into(),
            Tok::And => "`and`".into(),
            Tok::Or => "`or`".into(),
            Tok::Not => "`not`".into(),
            Tok::In => "`in`".into(),
            Tok::If => "`if`".into(),
            Tok::Elif => "`elif`".into(),
            Tok::Else => "`else`".into(),
            Tok::Assign => "`=`".into(),
            Tok::PlusAssign => "`+=`".into(),
            Tok::MinusAssign => "`-=`".into(),
            Tok::EqEq => "`==`".into(),
            Tok::NotEq => "`!=`".into(),
            Tok::Lt => "`<`".into(),
            Tok::Le => "`<=`".into(),
            Tok::Gt => "`>`".into(),
            Tok::Ge => "`>=`".into(),
            Tok::Plus => "`+`".into(),
            Tok::Minus => "`-`".into(),
            Tok::Star => "`*`".into(),
            Tok::DotDot => "`..`".into(),
            Tok::LParen => "`(`".into(),
            Tok::RParen => "`)`".into(),
            Tok::LBracket => "`[`".into(),
            Tok::RBracket => "`]`".into(),
            Tok::Comma => "`,`".into(),
            Tok::Colon => "`:`".into(),
            Tok::Newline => "the end of the line".into(),
            Tok::Indent => "an indented block".into(),
            Tok::Dedent => "the end of the block".into(),
            Tok::Eof => "the end of the file".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// A `#` comment, kept for the formatter.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub span: Span,
    /// Text after `#`, right-trimmed.
    pub text: String,
    /// `true` when nothing but whitespace precedes it on its line.
    pub own_line: bool,
    /// Column (in spaces) of the `#`.
    pub column: u32,
}

#[derive(Debug, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub errors: Vec<Diagnostic>,
}

/// Remove the common indentation of a `"""` block, drop the first line if it
/// is empty and the last line if it is blank. Works on the raw (still escaped)
/// text so the formatter can reuse it.
pub fn dedent_block(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    if lines.first().is_some_and(|l| l.trim().is_empty()) && lines.len() > 1 {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim().is_empty()) && lines.len() > 1 {
        lines.pop();
    }
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                &l[indent.min(l.len())..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    file: FileId,
    pos: usize,
    out: Lexed,
    indents: Vec<usize>,
    depth: usize,
    /// Span of the innermost unclosed bracket, for E0108.
    open: Vec<Span>,
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

impl<'a> Lexer<'a> {
    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, start, end)
    }

    fn push(&mut self, tok: Tok, start: usize, end: usize) {
        let span = self.span(start, end);
        self.out.tokens.push(Token { tok, span });
    }

    fn err(
        &mut self,
        code: &'static str,
        start: usize,
        end: usize,
        msg: impl Into<String>,
    ) -> &mut Diagnostic {
        let span = self.span(start, end.max(start + 1).min(self.src.len().max(start + 1)));
        self.out.errors.push(Diagnostic::error(code, span, msg));
        self.out.errors.last_mut().unwrap()
    }

    fn peek(&self, offset: usize) -> u8 {
        self.bytes.get(self.pos + offset).copied().unwrap_or(0)
    }

    /// Handle the start of a physical line. Returns `false` at end of input.
    fn line_start(&mut self, joined: bool) -> bool {
        loop {
            if self.pos >= self.bytes.len() {
                return false;
            }
            let line_start = self.pos;
            let mut col = 0usize;
            let mut tab_at = None;
            while self.pos < self.bytes.len() {
                match self.bytes[self.pos] {
                    b' ' => col += 1,
                    b'\t' => {
                        tab_at.get_or_insert(self.pos);
                        col += 4;
                    }
                    _ => break,
                }
                self.pos += 1;
            }
            match self.peek(0) {
                b'\n' => {
                    self.pos += 1;
                    continue;
                }
                b'\r' if self.peek(1) == b'\n' => {
                    self.pos += 2;
                    continue;
                }
                0 if self.pos >= self.bytes.len() => return false,
                b'#' => {
                    self.comment(true, col);
                    continue;
                }
                _ => {}
            }
            if joined {
                // Continuation lines carry no indentation meaning.
                return true;
            }
            if let Some(t) = tab_at {
                self.err("E0102", t, t + 1, "tab character in indentation")
                    .help = Some("indent with spaces only (4 per level); `darm fmt` fixes spacing but cannot guess tab widths".into());
            }
            let top = *self.indents.last().unwrap();
            if col > top {
                self.indents.push(col);
                self.push(Tok::Indent, line_start, self.pos);
            } else if col < top {
                while col < *self.indents.last().unwrap() {
                    self.indents.pop();
                    self.push(Tok::Dedent, self.pos, self.pos);
                }
                if col != *self.indents.last().unwrap() {
                    let expected = *self.indents.last().unwrap();
                    self.err(
                        "E0103",
                        line_start,
                        self.pos,
                        "this line's indentation does not match any enclosing block",
                    )
                    .help = Some(format!(
                        "indent with {expected} spaces to continue the enclosing block, or more to stay inside the inner one"
                    ));
                    // Recover by treating the line as part of the enclosing block.
                }
            }
            return true;
        }
    }

    fn comment(&mut self, own_line: bool, column: usize) {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        let text = self.src[start + 1..self.pos].trim_end().to_string();
        let span = self.span(start, self.pos);
        self.out.comments.push(Comment {
            span,
            text,
            own_line,
            column: column as u32,
        });
        if self.pos < self.bytes.len() {
            self.pos += 1; // newline
        }
    }

    fn unescape(&mut self, raw: &str, base: usize) -> String {
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                // Markup escapes stay escaped; the rich-text parser resolves them.
                Some((_, m @ ('*' | '[' | ']' | '{' | '}' | '`' | '(' | ')'))) => {
                    out.push('\\');
                    out.push(m);
                }
                Some((_, 'u')) => {
                    let mut hex = String::new();
                    let mut ok = chars.next().is_some_and(|(_, c)| c == '{');
                    if ok {
                        ok = false;
                        for (_, h) in chars.by_ref() {
                            if h == '}' {
                                ok = true;
                                break;
                            }
                            hex.push(h);
                        }
                    }
                    match u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .filter(|_| ok && !hex.is_empty())
                    {
                        Some(ch) => out.push(ch),
                        None => {
                            self.err("E0105", base + i, base + i + 2, "invalid unicode escape")
                                .help =
                                Some("write `\\u{1F600}` with 1-6 hexadecimal digits".into());
                        }
                    }
                }
                Some((j, other)) => {
                    self.err("E0105", base + i, base + j + other.len_utf8(), format!("unknown escape `\\{other}`"))
                        .help = Some("valid escapes: \\\" \\\\ \\n \\t \\u{...} and the markup escapes \\* \\[ \\] \\{ \\} \\`".into());
                }
                None => {
                    self.err(
                        "E0105",
                        base + i,
                        base + i + 1,
                        "a string cannot end with a single backslash",
                    );
                }
            }
        }
        out
    }

    fn string(&mut self) {
        let start = self.pos;
        let block = self.peek(1) == b'"' && self.peek(2) == b'"';
        if block {
            let body = start + 3;
            let mut i = body;
            let mut end = None;
            while i < self.bytes.len() {
                match self.bytes[i] {
                    b'\\' => i += 2,
                    b'"' if self.bytes.get(i + 1) == Some(&b'"')
                        && self.bytes.get(i + 2) == Some(&b'"') =>
                    {
                        end = Some(i);
                        break;
                    }
                    _ => i += 1,
                }
            }
            let Some(end) = end else {
                self.err("E0104", start, start + 3, "this text block is never closed")
                    .help = Some("add a closing `\"\"\"`".into());
                self.pos = self.bytes.len();
                self.push(
                    Tok::Str {
                        value: String::new(),
                        block: true,
                    },
                    start,
                    self.pos,
                );
                return;
            };
            let raw = dedent_block(&self.src[body..end]);
            // Escape positions are approximate inside dedented blocks; point at the block start.
            let value = self.unescape(&raw, body);
            self.pos = end + 3;
            self.push(Tok::Str { value, block: true }, start, self.pos);
        } else {
            let body = start + 1;
            let mut i = body;
            loop {
                match self.bytes.get(i) {
                    None | Some(b'\n') => {
                        self.err("E0104", start, i, "this string is never closed")
                            .help = Some(
                            "add a closing `\"`; for text that spans lines use a `\"\"\"` block"
                                .into(),
                        );
                        let raw = self.src[body..i].trim_end_matches('\r').to_string();
                        self.pos = i;
                        self.push(
                            Tok::Str {
                                value: raw,
                                block: false,
                            },
                            start,
                            i,
                        );
                        return;
                    }
                    Some(b'\\') => i += 2,
                    Some(b'"') => break,
                    Some(_) => i += 1,
                }
            }
            let i = i.min(self.bytes.len());
            let value = self.unescape(&self.src[body..i], body);
            self.pos = i + 1;
            self.push(
                Tok::Str {
                    value,
                    block: false,
                },
                start,
                self.pos,
            );
        }
    }

    fn number(&mut self) {
        let start = self.pos;
        while self.peek(0).is_ascii_digit() {
            self.pos += 1;
        }
        let mut decimal = false;
        if self.peek(0) == b'.' && self.peek(1).is_ascii_digit() {
            decimal = true;
            self.pos += 1;
            while self.peek(0).is_ascii_digit() {
                self.pos += 1;
            }
        }
        let digits_end = self.pos;
        let text = &self.src[start..digits_end];
        let value: f64 = text.parse().unwrap_or(0.0);
        // Suffix.
        let suffix_start = self.pos;
        while is_ident_char(self.peek(0)) {
            self.pos += 1;
        }
        let suffix = &self.src[suffix_start..self.pos];
        let tok = match suffix {
            "" if self.peek(0) == b'%' => {
                self.pos += 1;
                Tok::Percent(value)
            }
            "" if decimal => Tok::Decimal(value),
            "" => match text.parse::<i64>() {
                Ok(i) => Tok::Int(i),
                Err(_) => {
                    self.err("E0106", start, digits_end, "this number is too large");
                    Tok::Int(0)
                }
            },
            "ms" | "s" => {
                let ms = if suffix == "s" { value * 1000.0 } else { value };
                if !(0.0..=3_600_000.0).contains(&ms) {
                    self.err(
                        "E0106",
                        start,
                        self.pos,
                        "durations must be between 0 and one hour",
                    );
                }
                Tok::Duration(ms.round().clamp(0.0, 3_600_000.0) as u32)
            }
            other => {
                let other = other.to_string();
                self.err(
                    "E0106",
                    start,
                    self.pos,
                    format!("`{other}` is not a number suffix"),
                )
                .help = Some("numbers may end in `%` (percent), `ms` or `s` (duration)".into());
                if decimal {
                    Tok::Decimal(value)
                } else {
                    Tok::Int(value as i64)
                }
            }
        };
        self.push(tok, start, self.pos);
    }

    fn name(&mut self) {
        let start = self.pos;
        while is_ident_char(self.peek(0)) {
            self.pos += 1;
        }
        let word = &self.src[start..self.pos];
        let tok = match word {
            "true" => Tok::True,
            "false" => Tok::False,
            "and" => Tok::And,
            "or" => Tok::Or,
            "not" => Tok::Not,
            "in" => Tok::In,
            "if" => Tok::If,
            "elif" => Tok::Elif,
            "else" => Tok::Else,
            _ => {
                if word.len() > 64 {
                    self.err(
                        "E0101",
                        start,
                        self.pos,
                        "names can be at most 64 characters long",
                    );
                }
                Tok::Name(word.to_string())
            }
        };
        self.push(tok, start, self.pos);
    }

    /// Lex the rest of the current physical line. Returns `true` when the
    /// logical line continues on the next physical line.
    fn line_body(&mut self) -> bool {
        loop {
            let c = self.peek(0);
            let start = self.pos;
            match c {
                b' ' | b'\t' => self.pos += 1,
                b'\r' if self.peek(1) == b'\n' => self.pos += 1,
                b'\n' => {
                    self.pos += 1;
                    return self.end_of_line(start);
                }
                0 if self.pos >= self.bytes.len() => return self.end_of_line(start),
                b'#' => {
                    self.comment(false, 0);
                    return self.end_of_line(start);
                }
                b'"' => self.string(),
                b'0'..=b'9' => self.number(),
                c if is_ident_start(c) => self.name(),
                _ => {
                    let two = [c, self.peek(1)];
                    let (tok, len) = match &two {
                        b"==" => (Tok::EqEq, 2),
                        b"!=" => (Tok::NotEq, 2),
                        b"<=" => (Tok::Le, 2),
                        b">=" => (Tok::Ge, 2),
                        b"+=" => (Tok::PlusAssign, 2),
                        b"-=" => (Tok::MinusAssign, 2),
                        b".." => (Tok::DotDot, 2),
                        [b'=', _] => (Tok::Assign, 1),
                        [b'<', _] => (Tok::Lt, 1),
                        [b'>', _] => (Tok::Gt, 1),
                        [b'+', _] => (Tok::Plus, 1),
                        [b'-', _] => (Tok::Minus, 1),
                        [b'*', _] => (Tok::Star, 1),
                        [b'(', _] => (Tok::LParen, 1),
                        [b')', _] => (Tok::RParen, 1),
                        [b'[', _] => (Tok::LBracket, 1),
                        [b']', _] => (Tok::RBracket, 1),
                        [b',', _] => (Tok::Comma, 1),
                        [b':', _] => (Tok::Colon, 1),
                        _ => {
                            let ch = self.src[self.pos..].chars().next().unwrap();
                            let len = ch.len_utf8();
                            let help = match ch {
                                '{' | '}' => Some(
                                    "DARM has no braces: blocks are indented lines after a `:`",
                                ),
                                ';' => Some("write one statement per line"),
                                '\'' => Some("strings use double quotes: \"...\""),
                                '/' => Some(
                                    "division is not part of the language; `+`, `-` and `*` are",
                                ),
                                '\u{201c}' | '\u{201d}' => Some(
                                    "use straight double quotes `\"` to delimit strings (curly quotes are fine *inside* them)",
                                ),
                                '.' => Some("names cannot contain dots"),
                                _ => None,
                            };
                            let d = self.err(
                                "E0101",
                                self.pos,
                                self.pos + len,
                                format!("unexpected character `{ch}`"),
                            );
                            d.help = help.map(Into::into);
                            self.pos += len;
                            continue;
                        }
                    };
                    match tok {
                        Tok::LParen | Tok::LBracket => {
                            self.depth += 1;
                            self.open.push(self.span(start, start + 1));
                        }
                        Tok::RParen | Tok::RBracket => {
                            self.depth = self.depth.saturating_sub(1);
                            self.open.pop();
                        }
                        _ => {}
                    }
                    self.pos += len;
                    self.push(tok, start, self.pos);
                }
            }
        }
    }

    fn end_of_line(&mut self, at: usize) -> bool {
        if self.depth > 0 {
            return true;
        }
        match self.out.tokens.last().map(|t| &t.tok) {
            Some(Tok::Comma) => true,
            Some(Tok::Newline | Tok::Indent | Tok::Dedent) | None => false,
            Some(_) => {
                self.push(Tok::Newline, at, at);
                false
            }
        }
    }
}

/// Tokenise one file.
pub fn lex(file: FileId, src: &str) -> Lexed {
    let start = if src.starts_with('\u{feff}') { 3 } else { 0 };
    let mut lx = Lexer {
        src,
        bytes: src.as_bytes(),
        file,
        pos: start,
        out: Lexed::default(),
        indents: vec![0],
        depth: 0,
        open: Vec::new(),
    };
    let mut joined = false;
    while lx.line_start(joined) {
        joined = lx.line_body();
    }
    let end = src.len();
    if let Some(open) = lx.open.first().copied() {
        lx.out.errors.push(
            Diagnostic::error("E0108", open, "this bracket is never closed")
                .with_help("add the matching `)` or `]`"),
        );
    }
    if !matches!(
        lx.out.tokens.last().map(|t| &t.tok),
        Some(Tok::Newline | Tok::Dedent) | None
    ) {
        lx.push(Tok::Newline, end, end);
    }
    while lx.indents.len() > 1 {
        lx.indents.pop();
        lx.push(Tok::Dedent, end, end);
    }
    lx.push(Tok::Eof, end, end);
    lx.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        let l = lex(FileId(0), src);
        assert!(l.errors.is_empty(), "{:?}", l.errors);
        l.tokens.into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn indentation_and_blocks() {
        let t = toks("a:\n    b\n    c:\n        d\ne\n");
        use Tok::*;
        let n = |s: &str| Name(s.into());
        assert_eq!(
            t,
            vec![
                n("a"),
                Colon,
                Newline,
                Indent,
                n("b"),
                Newline,
                n("c"),
                Colon,
                Newline,
                Indent,
                n("d"),
                Newline,
                Dedent,
                Dedent,
                n("e"),
                Newline,
                Eof
            ]
        );
    }

    #[test]
    fn literals() {
        let t = toks("x 60%, 300ms, 1.5s, 0..5, 2.5, \"a\\\"b\"\n");
        use Tok::*;
        assert_eq!(
            t,
            vec![
                Name("x".into()),
                Percent(60.0),
                Comma,
                Duration(300),
                Comma,
                Duration(1500),
                Comma,
                Int(0),
                DotDot,
                Int(5),
                Comma,
                Decimal(2.5),
                Comma,
                Str {
                    value: "a\"b".into(),
                    block: false
                },
                Newline,
                Eof
            ]
        );
    }

    #[test]
    fn joins_lines_in_brackets_and_after_comma() {
        let t = toks("a = f(1,\n      2)\ntext \"x\",\n     style=lead\n");
        assert_eq!(t.iter().filter(|t| **t == Tok::Newline).count(), 2);
        assert!(!t.contains(&Tok::Indent));
    }

    #[test]
    fn block_strings_are_dedented() {
        let t = toks("text \"\"\"\n    One\n      two\n\n    Three\n    \"\"\"\n");
        assert_eq!(
            t[1],
            Tok::Str {
                value: "One\n  two\n\nThree".into(),
                block: true
            }
        );
    }

    #[test]
    fn comments_are_collected_not_tokenised() {
        let l = lex(FileId(0), "# top\na 1  # trailing\n    # deeper\nb\n");
        assert!(l.errors.is_empty());
        assert_eq!(l.comments.len(), 3);
        assert!(l.comments[0].own_line && !l.comments[1].own_line && l.comments[2].own_line);
        assert_eq!(l.comments[1].text, " trailing");
        assert!(!l.tokens.iter().any(|t| matches!(t.tok, Tok::Indent)));
    }

    #[test]
    fn errors_are_specific() {
        let codes = |src: &str| {
            lex(FileId(0), src)
                .errors
                .iter()
                .map(|e| e.code)
                .collect::<Vec<_>>()
        };
        assert_eq!(codes("a:\n\tb\n"), ["E0102"]);
        assert_eq!(codes("a:\n        b\n    c\n"), ["E0103"]);
        assert_eq!(codes("text \"abc\n"), ["E0104"]);
        assert_eq!(codes("text \"a\\qb\"\n"), ["E0105"]);
        assert_eq!(codes("space 3px\n"), ["E0106"]);
        assert_eq!(codes("a = {1}\n"), ["E0101", "E0101"]);
        assert_eq!(codes("a = f(1, 2\n"), ["E0108"]);
        assert_eq!(codes("text \"\"\"abc\n"), ["E0104"]);
    }

    #[test]
    fn never_panics_on_odd_input() {
        for src in [
            "",
            "\n\n",
            "\"",
            "\"\"\"",
            "\\",
            "a\u{0}b",
            "\u{feff}darm 1",
            "  \t  x",
            "1.",
            "1..",
            "..",
            "é",
            "a\r\nb\r\n",
        ] {
            let _ = lex(FileId(0), src);
        }
    }
}
