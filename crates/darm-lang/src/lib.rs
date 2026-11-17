//! # darm-lang
//!
//! The front end of the DARM compiler: everything that only needs the text of
//! a `.darml` file.
//!
//! ```text
//! source text ──lexer──▶ tokens ──parser──▶ ast::File ──(darm-compiler)──▶ IR
//!                  └─ comments ─────────────▶ fmt (canonical formatter)
//! ```
//!
//! * [`lexer`] – indentation-aware tokeniser.
//! * [`parser`] – recursive descent parser with error recovery.
//! * [`ast`] – the syntax tree, every node with a source span.
//! * [`fmt`] – canonical formatter (`darm fmt`).
//! * [`diag`] – source map and diagnostics (text and JSON rendering).
//!
//! The parser is hand-written rather than generated: the grammar is small
//! (see `docs/language.md` §3), indentation handling is easiest in a custom
//! lexer, and precise, repairable diagnostics are a product requirement.

#![forbid(unsafe_code)]

pub mod ast;
pub mod diag;
pub mod fmt;
pub mod lexer;
pub mod parser;

pub use diag::{Diagnostic, FileId, Severity, SourceFile, SourceMap, Span, suggest};
pub use parser::{Parsed, parse};
