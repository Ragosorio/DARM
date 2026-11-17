//! Source positions and diagnostics.
//!
//! Every token and AST node carries a [`Span`]. Diagnostics are rendered either
//! as rustc-style text with the offending line and a caret, or as JSON for
//! tools and AI agents (`darm check --format json`).

use serde::Serialize;

/// Index of a file in a [`SourceMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub struct FileId(pub u32);

/// Half-open byte range in one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: usize, end: usize) -> Span {
        Span {
            file,
            start: start as u32,
            end: end as u32,
        }
    }

    /// Smallest span covering both.
    pub fn to(self, other: Span) -> Span {
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Path as shown to the user (relative to the project when possible).
    pub name: String,
    pub text: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> SourceFile {
        let text = text.into();
        let mut line_starts = vec![0u32];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i as u32 + 1),
        );
        SourceFile {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// 1-based line and 1-based column (in characters) of a byte offset.
    pub fn line_col(&self, offset: u32) -> (usize, usize) {
        let offset = (offset as usize).min(self.text.len());
        let line = match self.line_starts.binary_search(&(offset as u32)) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line] as usize;
        let col = self
            .text
            .get(start..offset)
            .map_or(offset - start, |s| s.chars().count());
        (line + 1, col + 1)
    }

    /// Text of a 1-based line without its line terminator.
    pub fn line_text(&self, line: usize) -> &str {
        let start = self.line_starts[line - 1] as usize;
        let end = self
            .line_starts
            .get(line)
            .map_or(self.text.len(), |e| *e as usize);
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

/// All files of one compilation.
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap::default()
    }

    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> FileId {
        self.files.push(SourceFile::new(name, text));
        FileId(self.files.len() as u32 - 1)
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn snippet(&self, span: Span) -> &str {
        self.file(span.file)
            .text
            .get(span.start as usize..span.end as usize)
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One problem found in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable code such as `E0304`. See `docs/errors.md`.
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
    /// A concrete repair, when one is known.
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            span: Some(span),
            help: None,
        }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span: Some(span),
            help: None,
        }
    }

    /// A diagnostic that is not tied to a source position (I/O, project).
    pub fn global(
        severity: Severity,
        code: &'static str,
        message: impl Into<String>,
    ) -> Diagnostic {
        Diagnostic {
            severity,
            code,
            message: message.into(),
            span: None,
            help: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help = Some(help.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// rustc-style rendering. `color` adds ANSI colours.
    pub fn render(&self, map: &SourceMap, color: bool) -> String {
        let (red, yellow, blue, bold, reset) = if color {
            (
                "\x1b[31;1m",
                "\x1b[33;1m",
                "\x1b[34;1m",
                "\x1b[1m",
                "\x1b[0m",
            )
        } else {
            ("", "", "", "", "")
        };
        let (label, tint) = match self.severity {
            Severity::Error => ("error", red),
            Severity::Warning => ("warning", yellow),
        };
        let mut out = format!(
            "{tint}{label}[{}]{reset}{bold}: {}{reset}\n",
            self.code, self.message
        );
        if let Some(span) = self.span {
            let file = map.file(span.file);
            let (line, col) = file.line_col(span.start);
            let (end_line, end_col) = file.line_col(span.end);
            let gutter = line.to_string().len();
            let text = file.line_text(line);
            out.push_str(&format!(
                "{:gutter$}{blue}-->{reset} {}:{line}:{col}\n",
                "", file.name
            ));
            out.push_str(&format!("{:gutter$} {blue}|{reset}\n", ""));
            out.push_str(&format!("{blue}{line} |{reset} {text}\n"));
            let width = if end_line == line {
                end_col.saturating_sub(col).max(1)
            } else {
                text.chars().count().saturating_sub(col - 1).max(1)
            };
            // Keep tabs in the prefix so the caret lines up in terminals.
            let pad: String = text
                .chars()
                .take(col - 1)
                .map(|c| if c == '\t' { '\t' } else { ' ' })
                .collect();
            out.push_str(&format!(
                "{:gutter$} {blue}|{reset} {pad}{tint}{}{reset}\n",
                "",
                "^".repeat(width)
            ));
            if let Some(help) = &self.help {
                out.push_str(&format!(
                    "{:gutter$} {blue}={reset} {bold}help{reset}: {help}\n",
                    ""
                ));
            }
        } else if let Some(help) = &self.help {
            out.push_str(&format!("  = {bold}help{reset}: {help}\n"));
        }
        out
    }

    /// Machine-readable form.
    pub fn to_json(&self, map: &SourceMap) -> serde_json::Value {
        let mut v = serde_json::json!({
            "severity": self.severity,
            "code": self.code,
            "message": self.message,
        });
        if let Some(span) = self.span {
            let file = map.file(span.file);
            let (line, column) = file.line_col(span.start);
            let (end_line, end_column) = file.line_col(span.end);
            v["file"] = file.name.clone().into();
            v["line"] = line.into();
            v["column"] = column.into();
            v["end_line"] = end_line.into();
            v["end_column"] = end_column.into();
            v["source_line"] = file.line_text(line).into();
        }
        if let Some(help) = &self.help {
            v["help"] = help.clone().into();
        }
        v
    }
}

/// Best "did you mean" candidate by edit distance, if one is close enough.
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        let d = edit_distance(name, c);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, c));
        }
    }
    let (d, c) = best?;
    let threshold = (name.chars().count().max(c.chars().count()) / 3).max(1);
    (d <= threshold).then_some(c)
}

/// Optimal string alignment distance: insertions, deletions, substitutions
/// and transpositions of adjacent characters each cost 1 (`txet` → `text` is 1).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_characters() {
        let f = SourceFile::new("t", "aé\n  ñandú x\n");
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(3), (1, 3)); // after 'é' (2 bytes)
        let x = f.text.find('x').unwrap() as u32;
        assert_eq!(f.line_col(x), (2, 9));
        assert_eq!(f.line_text(2), "  ñandú x");
    }

    #[test]
    fn suggestions() {
        assert_eq!(
            suggest("colour", ["color", "scale", "align"]),
            Some("color")
        );
        assert_eq!(
            suggest("tide_chrt", ["tide_chart", "film"]),
            Some("tide_chart")
        );
        assert_eq!(suggest("zzz", ["color", "scale"]), None);
        assert_eq!(suggest("txet", ["text", "quote", "list"]), Some("text"));
        assert_eq!(suggest("iamge", ["image", "item"]), Some("image"));
    }

    #[test]
    fn render_has_position_and_caret() {
        let mut map = SourceMap::new();
        let id = map.add("book.darml", "darm 1\nimage nope\n");
        let d = Diagnostic::error("E0304", Span::new(id, 13, 17), "unknown asset `nope`")
            .with_help("declare it in `assets:`");
        let r = d.render(&map, false);
        assert!(r.contains("error[E0304]: unknown asset `nope`"));
        assert!(r.contains("--> book.darml:2:7"));
        assert!(r.contains("      ^^^^"), "{r}");
        assert!(r.contains("help: declare it"));
        let j = d.to_json(&map);
        assert_eq!(j["line"], 2);
        assert_eq!(j["column"], 7);
    }
}
