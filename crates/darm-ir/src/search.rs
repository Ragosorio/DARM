//! Full-text search over the IR.
//!
//! Matching is case-insensitive and diacritic-insensitive (NFKD, combining
//! marks removed, lower-cased), so `cafe` finds `Café`. Hits are reported per
//! block; the reader maps blocks to pages and hides hits whose block is not
//! currently visible (conditional content).

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::{Block, BlockKind, Book, Span};

/// One search hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    /// Chapter id.
    pub chapter: String,
    /// Block id.
    pub block: String,
    /// Text around the match, trimmed to word boundaries.
    pub snippet: String,
    /// Byte range of the match inside `snippet` (UTF-8, on char boundaries).
    pub start: usize,
    pub end: usize,
}

/// Fold text for matching. Returns the folded string and, for every folded
/// byte, the byte offset of the source character it came from.
fn fold(text: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(text.len());
    for (offset, ch) in text.char_indices() {
        for d in ch.nfkd().filter(|c| !is_combining_mark(*c)) {
            for l in d.to_lowercase() {
                let before = out.len();
                out.push(l);
                map.extend(std::iter::repeat_n(offset, out.len() - before));
            }
        }
    }
    (out, map)
}

fn spans_text(spans: &[Span], out: &mut String) {
    for s in spans {
        out.push_str(&s.text);
        if s.line_break {
            out.push(' ');
        }
    }
}

/// The searchable plain text of one block (not including nested blocks).
pub fn block_text(block: &Block) -> String {
    let mut t = String::new();
    let sep = |t: &mut String| {
        if !t.is_empty() {
            t.push_str(" \u{2014} ");
        }
    };
    match &block.kind {
        BlockKind::Paragraph { spans, .. } | BlockKind::Heading { spans, .. } => {
            spans_text(spans, &mut t)
        }
        BlockKind::Quote {
            paragraphs,
            attribution,
        } => {
            for p in paragraphs {
                sep(&mut t);
                spans_text(p, &mut t);
            }
            if let Some(a) = attribution {
                sep(&mut t);
                spans_text(a, &mut t);
            }
        }
        BlockKind::List { items, .. } => {
            for i in items {
                sep(&mut t);
                spans_text(i, &mut t);
            }
        }
        BlockKind::Image { caption, .. }
        | BlockKind::Gallery { caption, .. }
        | BlockKind::Video { caption, .. }
        | BlockKind::Audio { caption, .. } => {
            if let Some(c) = caption {
                spans_text(c, &mut t);
            }
        }
        BlockKind::Box { title, .. } => {
            if let Some(title) = title {
                t.push_str(title);
            }
        }
        BlockKind::Button { label, .. } => t.push_str(label),
        BlockKind::Choice { prompt, options } => {
            spans_text(prompt, &mut t);
            for o in options {
                sep(&mut t);
                t.push_str(&o.label);
            }
        }
        BlockKind::Quiz {
            question,
            answers,
            explanation,
            ..
        } => {
            spans_text(question, &mut t);
            for a in answers {
                sep(&mut t);
                t.push_str(&a.label);
            }
            if let Some(e) = explanation {
                sep(&mut t);
                spans_text(e, &mut t);
            }
        }
        BlockKind::Divider { .. }
        | BlockKind::Space { .. }
        | BlockKind::PageBreak
        | BlockKind::Group { .. }
        | BlockKind::Page { .. } => {}
    }
    t
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i.min(s.len())
}

/// Search the whole book. Returns at most `limit` hits in reading order.
/// Queries shorter than two characters (after folding) return nothing.
pub fn search(book: &Book, query: &str, limit: usize) -> Vec<Hit> {
    let (needle, _) = fold(query.trim());
    let mut hits = Vec::new();
    if needle.chars().count() < 2 {
        return hits;
    }
    const CONTEXT: usize = 48;
    book.walk_blocks(|chapter, block| {
        if hits.len() >= limit {
            return;
        }
        let text = block_text(block);
        if text.is_empty() {
            return;
        }
        let (hay, map) = fold(&text);
        let mut from = 0;
        while let Some(pos) = hay[from..].find(&needle) {
            if hits.len() >= limit {
                break;
            }
            let fs = from + pos;
            let fe = fs + needle.len();
            let src_start = map[fs];
            // End of the source char that produced the last folded byte.
            let last = map[fe - 1];
            let src_end = last + text[last..].chars().next().map_or(0, char::len_utf8);

            let mut a = floor_boundary(&text, src_start.saturating_sub(CONTEXT));
            if a > 0 {
                // Start at the next word boundary so snippets don't begin mid-word.
                a = text[a..src_start]
                    .find(char::is_whitespace)
                    .map_or(a, |i| ceil_boundary(&text, a + i + 1));
            }
            let mut b = ceil_boundary(&text, (src_end + CONTEXT).min(text.len()));
            if b < text.len() {
                b = text[src_end..b]
                    .rfind(char::is_whitespace)
                    .map_or(b, |i| src_end + i);
            }
            let mut snippet = String::new();
            if a > 0 {
                snippet.push('\u{2026}');
            }
            let start = snippet.len() + (src_start - a);
            snippet.push_str(&text[a..b]);
            let end = start + (src_end - src_start);
            if b < text.len() {
                snippet.push('\u{2026}');
            }
            hits.push(Hit {
                chapter: chapter.id.clone(),
                block: block.id.clone(),
                snippet,
                start,
                end,
            });
            from = fe;
        }
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_ignores_case_and_diacritics() {
        let (f, map) = fold("Café Ñandú");
        assert_eq!(f, "cafe nandu");
        assert_eq!(map.len(), f.len());
        // 'é' is two bytes in the source; the following space maps after it.
        assert_eq!(map[4], 5);
    }
}
