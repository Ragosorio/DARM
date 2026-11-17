//! WebAssembly bindings for the DARM reader core.
//!
//! Built with `scripts/build-wasm.sh` into `apps/reader/src/wasm-pkg`.
//! Each binding area lives in its own module:
//!
//! * [`package`] — probing and opening `.darm` containers, decoding chunks.
//! * [`book`] — validating the IR of a book and searching its text.

pub mod book;
pub mod package;
