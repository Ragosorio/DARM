//! JavaScript bindings for the book IR: validation and full-text search.
//!
//! A package is untrusted input, so the reader never hands IR JSON straight
//! to its layout engine. It first calls [`DarmBook::parse`], which checks the
//! version, the shape and every cross-reference with the same
//! `darm_ir::validate` the compiler uses, and only then parses the JSON on
//! the JavaScript side.

use darm_ir::{Book, search};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
/** One element of `JSON.parse(book.search(query, limit))`. */
export interface DarmSearchHit {
  /** Chapter id. */
  chapter: string;
  /** Block id. */
  block: string;
  /** Text around the match. */
  snippet: string;
  /** UTF-8 byte range of the match inside `snippet`. */
  start: number;
  end: number;
}
"#;

fn error(code: &str, message: &str) -> JsValue {
    let e = js_sys::Error::new(&format!("{code}: {message}"));
    let _ = js_sys::Reflect::set(&e, &JsValue::from_str("code"), &JsValue::from_str(code));
    e.into()
}

/// A validated book.
#[wasm_bindgen]
pub struct DarmBook {
    book: Book,
}

#[wasm_bindgen]
impl DarmBook {
    /// Parse and validate IR JSON. Throws an `Error` with a `code` property
    /// (`E_IR_VERSION`, `E_IR_JSON`, `E_IR_REF`, …) describing the first
    /// problem; the message lists up to five.
    pub fn parse(json: &str) -> Result<DarmBook, JsValue> {
        let book = Book::from_json(json).map_err(|e| error(e.code, &e.message))?;
        if let Err(errors) = darm_ir::validate(&book) {
            let code = errors.first().map_or("E_IR_INVALID", |e| e.code);
            let text = errors
                .iter()
                .take(5)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ");
            let more = if errors.len() > 5 {
                format!(" (and {} more)", errors.len() - 5)
            } else {
                String::new()
            };
            return Err(error(
                code,
                &format!("the book data is not valid: {text}{more}"),
            ));
        }
        Ok(DarmBook { book })
    }

    /// Case- and diacritic-insensitive search. Returns a JSON array of hits
    /// in reading order (at most `limit`, capped at 500).
    pub fn search(&self, query: &str, limit: u32) -> String {
        let hits = search::search(&self.book, query, limit.min(500) as usize);
        serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into())
    }

    /// IR version of the book, e.g. `"1.0"`.
    #[wasm_bindgen(js_name = irVersion)]
    pub fn ir_version(&self) -> String {
        self.book.ir_version.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_runs_natively() {
        let json = include_str!("../../../apps/reader/tests/fixtures/kitchen-sink.ir.json");
        let book = Book::from_json(json).unwrap();
        darm_ir::validate(&book).unwrap();
        let b = DarmBook { book };
        let hits: serde_json::Value = serde_json::from_str(&b.search("POLARIS", 10)).unwrap();
        assert!(!hits.as_array().unwrap().is_empty());
        assert_eq!(b.ir_version(), "1.0");
    }
}
