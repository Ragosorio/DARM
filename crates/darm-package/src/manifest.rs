//! The manifest: the table of entries plus a caller-supplied book summary.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::error::{PackageError, Result, quote};
use crate::limits::{
    GCM_TAG_LEN, MAX_BOOK_SUMMARY_LEN, MAX_CHUNKS_PER_ENTRY, MAX_ENTRIES, MAX_ENTRY_LEN,
    MAX_IR_LEN, MAX_MEDIA_TYPE_LEN, MAX_NAME_LEN, max_deflated_chunk_len,
};
use crate::prelude::validate_label;
use crate::util::{chunk_count, chunk_plain_len, hex_decode, hex_encode};

/// Value of `manifest_version` written and accepted by this crate.
pub const MANIFEST_VERSION: u32 = 1;

/// Name of the entry that holds the compiled book IR.
pub const IR_ENTRY_NAME: &str = "book.ir.json";

/// Media type of the IR entry.
pub const IR_MEDIA_TYPE: &str = "application/vnd.darm.ir+json";

// Messages of the two size limits that are enforced while the manifest JSON
// is being parsed (so that an oversized list is never materialised). They are
// recognised again in `Manifest::parse` to report `PackageError::Limit`.
const TOO_MANY_ENTRIES: &str = "the manifest lists too many entries";
const TOO_MANY_CHUNKS: &str = "an entry lists too many chunks";

/// What an entry is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The compiled book IR. At most one entry has this role.
    Ir,
    /// Any other file of the book (images, video, audio, fonts, …).
    Resource,
}

/// How the chunks of an entry are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compression {
    /// Chunks hold the original bytes.
    None,
    /// Every chunk is an independent raw deflate stream.
    Deflate,
}

/// A SHA-256 digest that (de)serialises as 64 lower-case hex digits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sha256Hex(pub [u8; 32]);

impl fmt::Debug for Sha256Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex_encode(&self.0))
    }
}

impl fmt::Display for Sha256Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex_encode(&self.0))
    }
}

impl Serialize for Sha256Hex {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex_encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Sha256Hex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct HexVisitor;
        impl Visitor<'_> for HexVisitor {
            type Value = Sha256Hex;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("64 lower-case hexadecimal digits")
            }
            fn visit_str<E: de::Error>(self, text: &str) -> std::result::Result<Sha256Hex, E> {
                hex_decode::<32>(text)
                    .map(Sha256Hex)
                    .ok_or_else(|| E::custom("a SHA-256 value must be 64 lower-case hex digits"))
            }
        }
        deserializer.deserialize_str(HexVisitor)
    }
}

/// The chunk list of an entry, stored compactly.
///
/// Serialises as a JSON array of `{"len": <stored bytes>, "sha256": "<hex>"}`
/// objects; `sha256` (of the stored bytes) is present for every chunk of a
/// clear package and absent in encrypted packages, where the GCM tag
/// authenticates each chunk instead.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct ChunkTable {
    lens: Vec<u32>,
    hashes: Vec<[u8; 32]>,
}

impl ChunkTable {
    /// Number of chunks.
    pub fn len(&self) -> usize {
        self.lens.len()
    }

    /// `true` when the entry has no chunks (it is empty).
    pub fn is_empty(&self) -> bool {
        self.lens.is_empty()
    }

    /// Stored length in bytes of chunk `chunk`.
    pub fn stored_len(&self, chunk: usize) -> Option<u32> {
        self.lens.get(chunk).copied()
    }

    /// SHA-256 of the stored bytes of chunk `chunk` (clear packages only).
    pub fn sha256(&self, chunk: usize) -> Option<&[u8; 32]> {
        self.hashes.get(chunk)
    }

    /// `true` when per-chunk hashes are present.
    pub fn has_hashes(&self) -> bool {
        !self.hashes.is_empty()
    }

    #[cfg(feature = "write")]
    pub(crate) fn push(&mut self, len: u32, hash: Option<[u8; 32]>) {
        self.lens.push(len);
        if let Some(hash) = hash {
            self.hashes.push(hash);
        }
    }

    pub(crate) fn lens(&self) -> &[u32] {
        &self.lens
    }
}

impl fmt::Debug for ChunkTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkTable")
            .field("chunks", &self.lens.len())
            .field("hashed", &self.has_hashes())
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
struct ChunkJson {
    len: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<Sha256Hex>,
}

impl Serialize for ChunkTable {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.lens.len()))?;
        for (index, &len) in self.lens.iter().enumerate() {
            seq.serialize_element(&ChunkJson {
                len,
                sha256: self.hashes.get(index).copied().map(Sha256Hex),
            })?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for ChunkTable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct TableVisitor;
        impl<'de> Visitor<'de> for TableVisitor {
            type Value = ChunkTable;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array of chunk objects")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<ChunkTable, A::Error> {
                let mut table = ChunkTable::default();
                while let Some(chunk) = seq.next_element::<ChunkJson>()? {
                    if table.lens.len() >= MAX_CHUNKS_PER_ENTRY {
                        return Err(de::Error::custom(TOO_MANY_CHUNKS));
                    }
                    match chunk.sha256 {
                        Some(hash) if table.hashes.len() == table.lens.len() => {
                            table.hashes.push(hash.0);
                        }
                        None if table.hashes.is_empty() => {}
                        _ => {
                            return Err(de::Error::custom(
                                "either all or none of an entry's chunks must carry a sha256",
                            ));
                        }
                    }
                    table.lens.push(chunk.len);
                }
                Ok(table)
            }
        }
        deserializer.deserialize_seq(TableVisitor)
    }
}

/// One file stored in the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Position of the entry in the manifest (0-based).
    pub index: u32,
    /// Relative, `/`-separated name (see [`validate_entry_name`]).
    pub name: String,
    /// Media (MIME) type.
    pub media_type: String,
    /// What the entry is for.
    pub role: Role,
    /// Size in bytes of the original (plaintext, uncompressed) content.
    pub size: u64,
    /// SHA-256 of the original content.
    pub sha256: Sha256Hex,
    /// How the chunks are stored.
    pub compression: Compression,
    /// Offset of the first stored byte, relative to the start of the data area.
    pub offset: u64,
    /// Total stored bytes (sum of the chunk lengths).
    pub stored_size: u64,
    /// The chunks, in order.
    pub chunks: ChunkTable,
}

fn deserialize_entries<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Entry>, D::Error> {
    struct EntriesVisitor;
    impl<'de> Visitor<'de> for EntriesVisitor {
        type Value = Vec<Entry>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an array of entries")
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Vec<Entry>, A::Error> {
            let mut entries = Vec::new();
            while let Some(entry) = seq.next_element::<Entry>()? {
                if entries.len() >= MAX_ENTRIES {
                    return Err(de::Error::custom(TOO_MANY_ENTRIES));
                }
                entries.push(entry);
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_seq(EntriesVisitor)
}

/// The manifest of a package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Version of the manifest schema; `1`.
    pub manifest_version: u32,
    /// Creation time supplied by the author, if any (free-form, RFC 3339 recommended).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Opaque summary of the book supplied by whoever built the package.
    pub book: Box<RawValue>,
    /// The entries, in storage order.
    #[serde(deserialize_with = "deserialize_entries")]
    pub entries: Vec<Entry>,
}

/// Where the stored bytes of an entry live, resolved against a real file.
#[derive(Debug, Clone)]
pub(crate) struct EntryLayout {
    /// Absolute offset of the entry's first stored byte.
    pub start: u64,
    /// For deflate entries: offset of every chunk relative to `start`.
    pub chunk_offsets: Option<Vec<u64>>,
}

/// Result of validating a manifest against the file it came from.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub entries: Vec<EntryLayout>,
    pub names: BTreeMap<String, u32>,
    pub ir_index: Option<u32>,
}

/// Checks that `name` is a safe, portable, relative entry name.
///
/// Rules: 1 to 512 bytes of UTF-8; segments separated by `/`; no empty, `.`
/// or `..` segment (so no leading, trailing or doubled `/`); no backslash,
/// no `:`, no NUL or other control character.
pub fn validate_entry_name(name: &str) -> Result<()> {
    let bad = |reason: &'static str| PackageError::BadName {
        name: name.to_owned(),
        reason,
    };
    if name.is_empty() {
        return Err(bad("the name is empty"));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(bad("the name is longer than 512 bytes"));
    }
    if name.starts_with('/') {
        return Err(bad("absolute paths are not allowed"));
    }
    for ch in name.chars() {
        match ch {
            '\\' => return Err(bad("backslashes are not allowed; use '/' as the separator")),
            ':' => return Err(bad("':' is not allowed")),
            c if c.is_control() => return Err(bad("control characters are not allowed")),
            _ => {}
        }
    }
    for segment in name.split('/') {
        match segment {
            "" => return Err(bad("empty path segments are not allowed")),
            "." | ".." => return Err(bad("'.' and '..' path segments are not allowed")),
            _ => {}
        }
    }
    Ok(())
}

/// Checks that `media_type` is plausible: 1 to 255 printable ASCII characters.
pub fn validate_media_type(media_type: &str) -> Result<()> {
    let ok = !media_type.is_empty()
        && media_type.len() <= MAX_MEDIA_TYPE_LEN
        && media_type.bytes().all(|b| (0x20..0x7f).contains(&b));
    if ok {
        Ok(())
    } else {
        Err(PackageError::BadOption(format!(
            "invalid media type \"{}\": it must be 1 to {MAX_MEDIA_TYPE_LEN} printable ASCII characters",
            quote(media_type)
        )))
    }
}

/// `true` for media types whose content is text and therefore worth deflating:
/// `text/*`, `application/json`, `application/xml`, `image/svg+xml` and any
/// `+json` / `+xml` structured-syntax suffix. Parameters are ignored.
pub fn is_text_like(media_type: &str) -> bool {
    let essence = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    essence.starts_with("text/")
        || essence == "application/json"
        || essence == "application/xml"
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
}

impl Manifest {
    /// Parses inflated manifest JSON. Unknown members are ignored.
    pub(crate) fn parse(json: &[u8]) -> Result<Manifest> {
        serde_json::from_slice(json).map_err(|e| {
            let message = e.to_string();
            if message.starts_with(TOO_MANY_ENTRIES) {
                PackageError::limit(
                    "number of entries",
                    MAX_ENTRIES as u64 + 1,
                    0,
                    MAX_ENTRIES as u64,
                )
            } else if message.starts_with(TOO_MANY_CHUNKS) {
                PackageError::limit(
                    "number of chunks of an entry",
                    MAX_CHUNKS_PER_ENTRY as u64 + 1,
                    0,
                    MAX_CHUNKS_PER_ENTRY as u64,
                )
            } else {
                PackageError::manifest(format!("it is not valid manifest JSON ({message})"))
            }
        })
    }

    /// Serialises the manifest to compact JSON (fields in declaration order).
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|e| PackageError::manifest(format!("cannot serialise the manifest: {e}")))
    }

    /// The opaque book summary as JSON text.
    pub fn book_json(&self) -> &str {
        self.book.get()
    }

    /// The book summary parsed into a JSON value.
    pub fn book(&self) -> Result<serde_json::Value> {
        serde_json::from_str(self.book.get()).map_err(|e| {
            PackageError::manifest(format!("the book summary is not valid JSON ({e})"))
        })
    }

    /// Validates every invariant of the manifest against the package it
    /// belongs to and resolves the absolute position of every entry.
    ///
    /// `data_start` and `data_len` describe the data area as declared by the
    /// header; the entries must tile it exactly, in order, without gaps.
    pub(crate) fn validate(
        &self,
        encrypted: bool,
        chunk_size: u32,
        data_start: u64,
        data_len: u64,
    ) -> Result<Layout> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(PackageError::manifest(format!(
                "unsupported manifest_version {} (expected {MANIFEST_VERSION})",
                self.manifest_version
            )));
        }
        if let Some(created) = &self.created {
            validate_label("created length", created).map_err(|_| {
                PackageError::manifest("the created string is too long or malformed")
            })?;
        }
        if self.book.get().len() > MAX_BOOK_SUMMARY_LEN {
            return Err(PackageError::limit(
                "book summary length",
                self.book.get().len() as u64,
                0,
                MAX_BOOK_SUMMARY_LEN as u64,
            ));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(PackageError::limit(
                "number of entries",
                self.entries.len() as u64,
                0,
                MAX_ENTRIES as u64,
            ));
        }

        let tag_len: u32 = if encrypted { GCM_TAG_LEN as u32 } else { 0 };
        let mut layout = Layout {
            entries: Vec::with_capacity(self.entries.len()),
            names: BTreeMap::new(),
            ir_index: None,
        };
        let mut cursor: u64 = 0;

        for (position, entry) in self.entries.iter().enumerate() {
            let index = position as u32;
            let fail = |message: String| {
                PackageError::manifest(format!(
                    "entry {index} (\"{}\"): {message}",
                    quote(&entry.name)
                ))
            };
            if entry.index != index {
                return Err(fail(format!("its index field is {}", entry.index)));
            }
            validate_entry_name(&entry.name)?;
            validate_media_type(&entry.media_type)
                .map_err(|_| fail("its media type is malformed".into()))?;
            if layout.names.insert(entry.name.clone(), index).is_some() {
                return Err(PackageError::DuplicateName(entry.name.clone()));
            }
            if entry.role == Role::Ir {
                if layout.ir_index.is_some() {
                    return Err(fail("a package can hold only one IR entry".into()));
                }
                if entry.size > MAX_IR_LEN {
                    return Err(PackageError::limit("IR size", entry.size, 0, MAX_IR_LEN));
                }
                layout.ir_index = Some(index);
            }
            if entry.size > MAX_ENTRY_LEN {
                return Err(PackageError::limit(
                    "entry size",
                    entry.size,
                    0,
                    MAX_ENTRY_LEN,
                ));
            }

            let expected_chunks = chunk_count(entry.size, chunk_size);
            if entry.chunks.len() as u64 != expected_chunks {
                return Err(fail(format!(
                    "it lists {} chunks but its size requires {expected_chunks}",
                    entry.chunks.len()
                )));
            }
            if !encrypted && !entry.chunks.is_empty() && !entry.chunks.has_hashes() {
                return Err(fail("its chunks carry no sha256".into()));
            }

            let deflated = entry.compression == Compression::Deflate;
            let mut chunk_offsets = deflated.then(|| Vec::with_capacity(entry.chunks.len()));
            let mut stored: u64 = 0;
            for (chunk, &len) in entry.chunks.lens().iter().enumerate() {
                let plain_len = chunk_plain_len(entry.size, chunk_size, chunk as u64);
                let valid = if deflated {
                    len > tag_len && len - tag_len <= max_deflated_chunk_len(plain_len)
                } else {
                    len == plain_len + tag_len
                };
                if !valid {
                    return Err(fail(format!(
                        "chunk {chunk} has an impossible stored length of {len} bytes"
                    )));
                }
                if let Some(offsets) = chunk_offsets.as_mut() {
                    offsets.push(stored);
                }
                stored += u64::from(len);
            }
            if stored != entry.stored_size {
                return Err(fail(format!(
                    "its chunks add up to {stored} bytes but stored_size is {}",
                    entry.stored_size
                )));
            }
            if entry.offset != cursor {
                return Err(fail(format!(
                    "its offset is {} but the previous entry ends at {cursor}",
                    entry.offset
                )));
            }
            cursor = cursor
                .checked_add(stored)
                .filter(|&end| end <= data_len)
                .ok_or_else(|| {
                    fail(format!(
                        "it extends beyond the end of the package (the data area is {data_len} bytes)"
                    ))
                })?;
            let start = data_start
                .checked_add(entry.offset)
                .ok_or_else(|| fail("its offset overflows".into()))?;
            layout.entries.push(EntryLayout {
                start,
                chunk_offsets,
            });
        }

        if cursor != data_len {
            return Err(PackageError::manifest(format!(
                "the entries cover {cursor} bytes but the data area is {data_len} bytes"
            )));
        }
        Ok(layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names() {
        for good in [
            "a",
            "book.ir.json",
            "media/video/intro.mp4",
            "imágenes/portada 1.png",
            "a/b/c/.hidden",
            "...",
            "a..b/c",
        ] {
            assert!(validate_entry_name(good).is_ok(), "{good}");
        }
        let long = "x".repeat(MAX_NAME_LEN + 1);
        for bad in [
            "",
            "/etc/passwd",
            "../secret",
            "a/../b",
            "a/./b",
            "a//b",
            "a/",
            ".",
            "..",
            "a\\b",
            "C:\\x",
            "c:/x",
            "a\0b",
            "a\nb",
            "a\u{7f}b",
            "a\u{85}b",
            long.as_str(),
        ] {
            let error = validate_entry_name(bad).unwrap_err();
            assert_eq!(error.code(), "E_PKG_BAD_NAME", "{bad:?}");
        }
        assert!(validate_entry_name(&"x".repeat(MAX_NAME_LEN)).is_ok());
    }

    #[test]
    fn text_like_media_types() {
        for yes in [
            "text/plain",
            "text/vtt",
            "TEXT/HTML; charset=utf-8",
            "application/json",
            "application/vnd.darm.ir+json",
            "image/svg+xml",
            "application/xml",
        ] {
            assert!(is_text_like(yes), "{yes}");
        }
        for no in [
            "image/png",
            "video/mp4",
            "audio/mpeg",
            "font/woff2",
            "application/octet-stream",
            "",
        ] {
            assert!(!is_text_like(no), "{no}");
        }
    }

    #[test]
    fn chunk_table_json() {
        let json = r#"[{"len":3,"sha256":"00000000000000000000000000000000000000000000000000000000000000ff"},{"len":4,"sha256":"1111111111111111111111111111111111111111111111111111111111111111"}]"#;
        let table: ChunkTable = serde_json::from_str(json).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(table.stored_len(1), Some(4));
        assert_eq!(table.sha256(0).unwrap()[31], 0xff);
        assert_eq!(serde_json::to_string(&table).unwrap(), json);

        let bare: ChunkTable = serde_json::from_str(r#"[{"len":3},{"len":4,"x":1}]"#).unwrap();
        assert!(!bare.has_hashes());
        assert_eq!(
            serde_json::to_string(&bare).unwrap(),
            r#"[{"len":3},{"len":4}]"#
        );

        let mixed = r#"[{"len":3},{"len":4,"sha256":"1111111111111111111111111111111111111111111111111111111111111111"}]"#;
        assert!(serde_json::from_str::<ChunkTable>(mixed).is_err());
        let mixed = r#"[{"len":4,"sha256":"1111111111111111111111111111111111111111111111111111111111111111"},{"len":3}]"#;
        assert!(serde_json::from_str::<ChunkTable>(mixed).is_err());
        assert!(serde_json::from_str::<ChunkTable>(r#"[{"len":-1}]"#).is_err());
        assert!(serde_json::from_str::<ChunkTable>(r#"[{"len":4294967296}]"#).is_err());
        assert!(serde_json::from_str::<ChunkTable>(r#"[{"len":1,"sha256":"AB"}]"#).is_err());
    }
}
