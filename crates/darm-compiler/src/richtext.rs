//! Inline markup inside strings → IR spans.
//!
//! Supported: `*italic*`, `**bold**`, `` `code` ``, `[label](target)`,
//! `{name}` interpolation and backslash escapes. With smart typography on,
//! straight quotes, `--`, `---` and `...` become their typographic forms.
//! Whitespace (including newlines) collapses to single spaces, except in
//! verse where every newline is a hard line break.

use darm_ir::{Link, Span, is_ident, validate::is_safe_url};

/// What `{name}` refers to.
pub enum Brace {
    /// A component parameter: inline this text.
    Text(String),
    /// A state variable: emit a live span.
    Var,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub smart: bool,
    /// Newlines are hard line breaks.
    pub verse: bool,
    /// Blank lines separate paragraphs (otherwise they are plain whitespace).
    pub split_paragraphs: bool,
}

#[derive(Debug, PartialEq)]
pub struct RichError {
    pub message: String,
    pub help: Option<String>,
}

fn fail<T>(message: impl Into<String>, help: &str) -> Result<T, RichError> {
    Err(RichError {
        message: message.into(),
        help: (!help.is_empty()).then(|| help.to_string()),
    })
}

#[derive(Debug, Default)]
pub struct Rich {
    pub paragraphs: Vec<Vec<Span>>,
    /// Anchor targets of internal links, to be checked once all ids are known.
    pub targets: Vec<String>,
    /// State variables interpolated with `{name}`.
    pub vars: Vec<String>,
}

struct Builder<'a> {
    opts: Options,
    lookup: &'a dyn Fn(&str) -> Brace,
    spans: Vec<Span>,
    cur: String,
    bold: bool,
    italic: bool,
    code: bool,
    link: Option<Link>,
    /// Last character emitted in this paragraph (for quote direction).
    prev: Option<char>,
    pending_space: bool,
    targets: Vec<String>,
    vars: Vec<String>,
}

impl Builder<'_> {
    fn flush(&mut self) {
        if self.cur.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.cur);
        if let Some(last) = self.spans.last_mut() {
            let same = last.var.is_none()
                && !last.line_break
                && last.bold == self.bold
                && last.italic == self.italic
                && last.code == self.code
                && last.link == self.link;
            if same {
                last.text.push_str(&text);
                return;
            }
        }
        self.spans.push(Span {
            text,
            bold: self.bold,
            italic: self.italic,
            code: self.code,
            link: self.link.clone(),
            var: None,
            line_break: false,
        });
    }

    fn started(&self) -> bool {
        !self.spans.is_empty() || !self.cur.is_empty()
    }

    fn emit(&mut self, c: char) {
        if self.pending_space {
            self.pending_space = false;
            let after_break =
                self.cur.is_empty() && self.spans.last().is_some_and(|s| s.line_break);
            if self.started() && !after_break {
                self.cur.push(' ');
                self.prev = Some(' ');
            }
        }
        self.cur.push(c);
        self.prev = Some(c);
    }

    fn opens(&self) -> bool {
        self.pending_space
            || match self.prev {
                None => true,
                Some(p) => {
                    p.is_whitespace()
                        || matches!(
                            p,
                            '(' | '['
                                | '{'
                                | '\u{2014}'
                                | '\u{2013}'
                                | '\u{201c}'
                                | '\u{2018}'
                                | '\u{00bf}'
                                | '\u{00a1}'
                        )
                }
            }
    }

    fn line_break(&mut self) {
        self.flush();
        self.pending_space = false;
        match self.spans.last_mut() {
            Some(last) if !last.line_break && last.var.is_none() => last.line_break = true,
            _ => self.spans.push(Span {
                line_break: true,
                ..Span::default()
            }),
        }
        self.prev = None;
    }

    fn run(&mut self, s: &[char]) -> Result<(), RichError> {
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            let next = s.get(i + 1).copied();
            if self.code {
                if c == '`' {
                    self.flush();
                    self.code = false;
                } else if c == '\\' && next == Some('`') {
                    self.emit('`');
                    i += 1;
                } else if c.is_whitespace() {
                    self.pending_space = true;
                } else {
                    self.emit(c);
                }
                i += 1;
                continue;
            }
            match c {
                '\\' => match next {
                    Some(n) => {
                        self.emit(n);
                        i += 1;
                    }
                    None => self.emit('\\'),
                },
                '\n' if self.opts.verse => self.line_break(),
                c if c.is_whitespace() => self.pending_space = true,
                '`' => {
                    if !s[i + 1..].contains(&'`') {
                        return fail(
                            "unclosed `` ` `` code span",
                            "add a closing backtick, or write \\` for a literal one",
                        );
                    }
                    // Keep the separating space outside the code span.
                    if self.pending_space && self.started() {
                        self.pending_space = false;
                        self.cur.push(' ');
                    }
                    self.pending_space = false;
                    self.flush();
                    self.code = true;
                }
                '*' => {
                    let before_space = i == 0 || s[i - 1].is_whitespace();
                    let double = next == Some('*');
                    let after = s.get(i + if double { 2 } else { 1 }).copied();
                    let after_space = after.is_none_or(char::is_whitespace);
                    if before_space && after_space && !double {
                        // A lone asterisk between spaces is just an asterisk.
                        self.emit('*');
                    } else {
                        if self.pending_space && self.started() && !after_space {
                            self.pending_space = false;
                            self.cur.push(' ');
                            self.prev = Some(' ');
                        }
                        self.flush();
                        if double {
                            self.bold = !self.bold;
                            i += 1;
                        } else {
                            self.italic = !self.italic;
                        }
                    }
                }
                '[' => match find_link(&s[i..]) {
                    Some((label, target, len)) => {
                        if self.link.is_some() {
                            return fail("links cannot be nested", "");
                        }
                        let target: String = target.iter().collect::<String>().trim().to_string();
                        let link = if target.contains("://")
                            || target.to_ascii_lowercase().starts_with("mailto:")
                        {
                            if !is_safe_url(&target) {
                                return fail(
                                    format!("link target `{target}` is not allowed"),
                                    "external links must start with https://, http:// or mailto: and contain no spaces",
                                );
                            }
                            Link::Url { href: target }
                        } else if is_ident(&target) {
                            self.targets.push(target.clone());
                            Link::Anchor { target }
                        } else {
                            return fail(
                                format!("link target `{target}` is neither an id nor a URL"),
                                "write `[label](some_id)` or `[label](https://example.org)`",
                            );
                        };
                        if self.pending_space && self.started() {
                            self.pending_space = false;
                            self.cur.push(' ');
                            self.prev = Some(' ');
                        }
                        self.flush();
                        self.link = Some(link);
                        self.run(label)?;
                        self.flush();
                        self.link = None;
                        i += len - 1;
                    }
                    None => self.emit('['),
                },
                '{' => {
                    let name: String = s[i + 1..]
                        .iter()
                        .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                        .collect();
                    let closed = s.get(i + 1 + name.chars().count()) == Some(&'}');
                    if closed && is_ident(&name) {
                        match (self.lookup)(&name) {
                            Brace::Text(t) => {
                                let chars: Vec<char> = t.chars().collect();
                                self.plain(&chars);
                            }
                            Brace::Var => {
                                if self.pending_space && self.started() {
                                    self.pending_space = false;
                                    self.cur.push(' ');
                                }
                                self.flush();
                                self.spans.push(Span {
                                    bold: self.bold,
                                    italic: self.italic,
                                    link: self.link.clone(),
                                    var: Some(name.clone()),
                                    ..Span::default()
                                });
                                self.prev = Some('x');
                                self.vars.push(name.clone());
                            }
                            Brace::Unknown => {
                                return fail(
                                    format!(
                                        "`{{{name}}}` does not name a state variable or component parameter"
                                    ),
                                    "declare it in `state:`, or write `\\{` for a literal brace",
                                );
                            }
                        }
                        i += name.chars().count() + 1;
                    } else {
                        self.emit('{');
                    }
                }
                '"' if self.opts.smart => {
                    let q = if self.opens() { '\u{201c}' } else { '\u{201d}' };
                    self.emit(q);
                }
                '\'' if self.opts.smart => {
                    let letter_before =
                        !self.pending_space && self.prev.is_some_and(char::is_alphanumeric);
                    let q = if !letter_before
                        && self.opens()
                        && next.is_some_and(|n| !n.is_whitespace())
                    {
                        // Leading apostrophes in elisions ('tis, '90s) cannot be told from
                        // opening quotes without a dictionary; treat as opening quote.
                        '\u{2018}'
                    } else {
                        '\u{2019}'
                    };
                    self.emit(q);
                }
                '-' if self.opts.smart && next == Some('-') => {
                    if s.get(i + 2) == Some(&'-') {
                        self.emit('\u{2014}');
                        i += 2;
                    } else {
                        self.emit('\u{2013}');
                        i += 1;
                    }
                }
                '.' if self.opts.smart
                    && next == Some('.')
                    && s.get(i + 2) == Some(&'.')
                    && s.get(i + 3) != Some(&'.') =>
                {
                    self.emit('\u{2026}');
                    i += 2;
                }
                other => self.emit(other),
            }
            i += 1;
        }
        Ok(())
    }

    /// Emit text without interpreting markup (substituted parameters).
    fn plain(&mut self, s: &[char]) {
        for &c in s {
            if c.is_whitespace() {
                self.pending_space = true;
            } else {
                self.emit(c);
            }
        }
    }
}

/// Recognise `[label](target)` at the start of `s`. Returns label, target and total length.
fn find_link(s: &[char]) -> Option<(&[char], &[char], usize)> {
    let mut i = 1;
    let mut close = None;
    while i < s.len() {
        match s[i] {
            '\\' => i += 1,
            '[' => return None,
            ']' => {
                close = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let close = close?;
    if s.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = s[close + 2..].iter().position(|c| *c == ')')? + close + 2;
    if close == 1 {
        return None; // empty label
    }
    Some((&s[1..close], &s[close + 2..end], end + 1))
}

/// Parse a string with inline markup.
pub fn parse(src: &str, opts: Options, lookup: &dyn Fn(&str) -> Brace) -> Result<Rich, RichError> {
    let mut rich = Rich::default();
    let mut chunks: Vec<String> = Vec::new();
    if opts.split_paragraphs {
        let mut cur = String::new();
        for line in src.split('\n') {
            if line.trim().is_empty() {
                if !cur.trim().is_empty() {
                    chunks.push(std::mem::take(&mut cur));
                }
                cur.clear();
            } else {
                if !cur.is_empty() {
                    cur.push('\n');
                }
                cur.push_str(line);
            }
        }
        if !cur.trim().is_empty() {
            chunks.push(cur);
        }
    } else {
        chunks.push(src.to_string());
    }
    for chunk in chunks {
        let mut b = Builder {
            opts,
            lookup,
            spans: Vec::new(),
            cur: String::new(),
            bold: false,
            italic: false,
            code: false,
            link: None,
            prev: None,
            pending_space: false,
            targets: Vec::new(),
            vars: Vec::new(),
        };
        let chars: Vec<char> = chunk.trim().chars().collect();
        b.run(&chars)?;
        if b.code {
            return fail("unclosed `` ` `` code span", "add a closing backtick");
        }
        if b.bold {
            return fail(
                "unclosed `**` (bold)",
                "add a closing `**`, or write `\\*\\*` for literal asterisks",
            );
        }
        if b.italic {
            return fail(
                "unclosed `*` (italic)",
                "add a closing `*`, or write `\\*` for a literal asterisk",
            );
        }
        b.flush();
        // A trailing hard break carries no information.
        if let Some(last) = b.spans.last_mut() {
            last.line_break = false;
        }
        b.spans
            .retain(|s| !s.text.is_empty() || s.var.is_some() || s.line_break);
        rich.targets.append(&mut b.targets);
        rich.vars.append(&mut b.vars);
        if !b.spans.is_empty() {
            rich.paragraphs.push(b.spans);
        }
    }
    Ok(rich)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Brace {
        Brace::Unknown
    }

    fn one(src: &str) -> Vec<Span> {
        let mut r = parse(
            src,
            Options {
                smart: true,
                ..Options::default()
            },
            &none,
        )
        .unwrap();
        assert_eq!(r.paragraphs.len(), 1, "{src}");
        r.paragraphs.remove(0)
    }

    fn texts(spans: &[Span]) -> Vec<(&str, bool, bool, bool)> {
        spans
            .iter()
            .map(|s| (s.text.as_str(), s.bold, s.italic, s.code))
            .collect()
    }

    #[test]
    fn emphasis_and_code() {
        let s = one("Plain *it* and **bold with *both* inside** and `x * y`.");
        assert_eq!(
            texts(&s),
            vec![
                ("Plain ", false, false, false),
                ("it", false, true, false),
                (" and ", false, false, false),
                ("bold with ", true, false, false),
                ("both", true, true, false),
                (" inside", true, false, false),
                (" and ", false, false, false),
                ("x * y", false, false, true),
                (".", false, false, false),
            ]
        );
    }

    #[test]
    fn whitespace_collapses_and_literal_asterisk() {
        let s = one("  one\n   two \t three * four  ");
        assert_eq!(
            texts(&s),
            vec![("one two three * four", false, false, false)]
        );
    }

    #[test]
    fn smart_typography() {
        let s = one("\"Hello,\" she said -- 'it's fine'... really---yes.");
        assert_eq!(
            s[0].text,
            "\u{201c}Hello,\u{201d} she said \u{2013} \u{2018}it\u{2019}s fine\u{2019}\u{2026} really\u{2014}yes."
        );
        let plain = parse("\"a\" -- b", Options::default(), &none).unwrap();
        assert_eq!(plain.paragraphs[0][0].text, "\"a\" -- b");
    }

    #[test]
    fn links_and_targets() {
        let r = parse(
            "See [the *ebb*](ebb) or [site](https://example.org/a).",
            Options::default(),
            &none,
        )
        .unwrap();
        let p = &r.paragraphs[0];
        assert_eq!(
            p[1].link,
            Some(Link::Anchor {
                target: "ebb".into()
            })
        );
        assert!(p[2].italic && p[2].link.is_some());
        assert_eq!(
            p[4].link,
            Some(Link::Url {
                href: "https://example.org/a".into()
            })
        );
        assert_eq!(r.targets, ["ebb"]);
        assert!(parse("[x](javascript://alert(1))", Options::default(), &none).is_err());
        assert!(parse("[x](not an id)", Options::default(), &none).is_err());
        // Brackets that are not links stay literal.
        assert_eq!(one("a [sic] b")[0].text, "a [sic] b");
    }

    #[test]
    fn interpolation() {
        let lookup = |n: &str| match n {
            "courage" => Brace::Var,
            "label" => Brace::Text("Field note".into()),
            _ => Brace::Unknown,
        };
        let r = parse(
            "{label}: you have {courage} left, \\{literal}.",
            Options::default(),
            &lookup,
        )
        .unwrap();
        let p = &r.paragraphs[0];
        assert_eq!(p[0].text, "Field note: you have ");
        assert_eq!(p[1].var.as_deref(), Some("courage"));
        assert_eq!(p[2].text, " left, {literal}.");
        assert_eq!(r.vars, ["courage"]);
        assert!(parse("{nope}", Options::default(), &lookup).is_err());
        assert_eq!(
            parse("a { b } c", Options::default(), &lookup)
                .unwrap()
                .paragraphs[0][0]
                .text,
            "a { b } c"
        );
    }

    #[test]
    fn paragraphs_and_verse() {
        let r = parse(
            "One\nstill one.\n\n\nTwo.",
            Options {
                split_paragraphs: true,
                ..Options::default()
            },
            &none,
        )
        .unwrap();
        assert_eq!(r.paragraphs.len(), 2);
        assert_eq!(r.paragraphs[0][0].text, "One still one.");
        let r = parse(
            "Line one\n  line *two*\nline three",
            Options {
                verse: true,
                ..Options::default()
            },
            &none,
        )
        .unwrap();
        let p = &r.paragraphs[0];
        assert!(p[0].line_break && p[0].text == "Line one");
        assert_eq!(p[1].text, "line ");
        assert!(p[2].italic && p[2].line_break);
        assert_eq!(p[3].text, "line three");
        assert!(!p[3].line_break);
    }

    #[test]
    fn unclosed_markup_is_an_error() {
        for bad in ["a *b", "a **b", "a `b", "**a*"] {
            assert!(parse(bad, Options::default(), &none).is_err(), "{bad}");
        }
        assert_eq!(one("5 \\* 3 and \\[x\\]")[0].text, "5 * 3 and [x]");
    }
}
