//! Reader and writer for `.darm`, the distribution container of DARM
//! interactive books (format 1.0).
//!
//! A `.darm` file is a purpose-built binary container:
//!
//! ```text
//! +--------------------+ 0
//! | fixed header       | 128 bytes: magic, version, flags, UUID, lengths, hashes
//! +--------------------+ 128
//! | prelude            | clear JSON: generator, chunk size, public card, key slots
//! +--------------------+
//! | manifest           | deflated JSON (AES-256-GCM sealed when encrypted)
//! +--------------------+ head_len
//! | data area          | entries back to back, each a run of chunks
//! +--------------------+ total_len
//! ```
//!
//! The byte-level specification lives in `docs/file-format.md` at the root of
//! the repository; the module docs of [`header`], [`prelude`], [`manifest`]
//! and [`keyfile`] repeat the essentials.
//!
//! # Reading
//!
//! ```no_run
//! use darm_package::{Credentials, Package, PackageError};
//!
//! # fn main() -> Result<(), PackageError> {
//! let package = match Package::open_path("book.darm", &Credentials::None) {
//!     Err(PackageError::CredentialsRequired { card }) => {
//!         println!("locked: {:?}", card.map(|c| c.title));
//!         Package::open_path("book.darm", &Credentials::Password("secret"))?
//!     }
//!     other => other?,
//! };
//! let ir = package.ir_bytes()?;
//! for entry in package.entries() {
//!     println!("{} ({} bytes, {})", entry.name, entry.size, entry.media_type);
//! }
//! package.verify_all()?;
//! # let _ = ir;
//! # Ok(())
//! # }
//! ```
//!
//! Readers that fetch bytes themselves (the WebAssembly binding does) use
//! [`probe`] and [`PackageHead`] instead: `probe` tells how many leading
//! bytes are needed, `PackageHead::open` parses them, and
//! [`PackageHead::chunk_location`] / [`PackageHead::decode_chunk`] locate and
//! decode chunks on demand.
//!
//! # Writing
//!
//! See [`PackageBuilder`] (cargo feature `write`, on by default).
//!
//! # Cargo features
//!
//! * `std` (default) — `ReadAt for std::fs::File` and [`Package::open_path`].
//! * `write` (default) — [`PackageBuilder`], [`generate_keyfile`]; pulls in
//!   `getrandom` for the operating-system random source. Implies `std`.
//!
//! With `default-features = false` the crate is a read-only library that
//! needs no randomness and builds for `wasm32-unknown-unknown` as is.
//!
//! # Security model in one paragraph
//!
//! Clear packages detect accidental corruption (every byte is covered by a
//! SHA-256) but are **not authenticated**: anyone can produce a different,
//! internally consistent file. Encrypted packages authenticate every byte
//! with AES-256-GCM under keys derived from a random content key; the
//! clear-text prelude (and so the public card) is authenticated as soon as
//! the package is unlocked, not before. See `docs/file-format.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod crypto;
mod error;
pub mod header;
pub mod keyfile;
pub mod limits;
pub mod manifest;
pub mod prelude;
mod reader;
mod util;
#[cfg(feature = "write")]
mod writer;

pub use error::{PackageError, Result};
pub use header::{FLAG_ENCRYPTED, FLAG_HAS_PUBLIC_CARD, FORMAT_MAJOR, FORMAT_MINOR, Header, MAGIC};
#[cfg(feature = "write")]
pub use keyfile::generate_keyfile;
pub use keyfile::{KeyFile, key_id};
pub use limits::*;
pub use manifest::{
    ChunkTable, Compression, Entry, IR_ENTRY_NAME, IR_MEDIA_TYPE, Manifest, Role, Sha256Hex,
    is_text_like, validate_entry_name, validate_media_type,
};
pub use prelude::{KdfParams, KeySlot, Prelude, PublicCard, SlotKind};
pub use reader::{
    Credentials, EntryReader, Package, PackageHead, Probe, ReadAt, SlotInfo, VerifyReport, probe,
};
pub use util::format_uuid;
#[cfg(feature = "write")]
pub use writer::{BuildSummary, CompressionMode, EncryptOptions, PackageBuilder};
