//! Building packages.
//!
//! The writer makes two passes over the inputs so that it never holds more
//! than one chunk in memory and never needs to seek the output: the first
//! pass hashes every entry and decides its compression, which fixes the whole
//! layout (and the manifest); the second pass streams the chunks out and
//! checks that the inputs did not change in between.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use base64ct::{Base64, Encoding};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::crypto::{
    AAD_COMPRESSION_DEFLATE, AAD_COMPRESSION_NONE, ChunkContext, ContentKeys, Secret, SlotBinding,
    key_commitment, key_id_bytes, keyfile_kek, password_kek, wrap_cek,
};
use crate::error::{PackageError, Result, quote};
use crate::header::{FLAG_ENCRYPTED, FLAG_HAS_PUBLIC_CARD, FORMAT_MAJOR, FORMAT_MINOR, Header};
use crate::limits::{
    DEFAULT_CHUNK_SIZE, GCM_TAG_LEN, HEADER_LEN, MAX_BOOK_SUMMARY_LEN, MAX_ENTRIES, MAX_ENTRY_LEN,
    MAX_IR_LEN, MAX_KEY_SLOTS, MAX_MANIFEST_INFLATED_LEN, MAX_MANIFEST_STORED_LEN, MAX_PRELUDE_LEN,
    MAX_TOTAL_LEN, max_deflated_chunk_len,
};
use crate::manifest::{
    ChunkTable, Compression, Entry, IR_ENTRY_NAME, IR_MEDIA_TYPE, MANIFEST_VERSION, Manifest, Role,
    Sha256Hex, is_text_like, validate_entry_name, validate_media_type,
};
use crate::prelude::{
    CIPHER_AES_256_GCM, EncryptionInfo, KdfParams, KeySlot, Prelude, PublicCard, SALT_LEN,
    SUBKEYS_HKDF_SHA256, validate_chunk_size, validate_label,
};
use crate::util::{chunk_plain_len, deflate, sha256, sha256_parts};

const UUID_DOMAIN: &[u8] = b"darm/1/package-id\0";

/// Whether the writer may compress entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionMode {
    /// Deflate an entry when its media type is text-like (see
    /// [`is_text_like`](crate::is_text_like)) *and* doing so makes it smaller.
    /// Media (images, video, audio, fonts) is always stored verbatim.
    #[default]
    Auto,
    /// Store every entry verbatim.
    None,
}

enum SlotSecret {
    Password(Zeroizing<String>),
    Key(Secret),
}

/// How to encrypt a package. Secrets held here are wiped on drop.
///
/// At least one of [`password`](Self::password) or
/// [`keyfile_key`](Self::keyfile_key) must be given; each call adds one key
/// slot (up to 8), and any single one of them unlocks the book.
pub struct EncryptOptions {
    secrets: Vec<SlotSecret>,
    kdf: KdfParams,
    expose_card: bool,
}

impl Default for EncryptOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EncryptOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptOptions")
            .field("slots", &self.secrets.len())
            .field("kdf", &self.kdf)
            .field("expose_card", &self.expose_card)
            .finish()
    }
}

impl EncryptOptions {
    /// No slots yet, default KDF parameters (Argon2id 64 MiB, t=3, p=1), card hidden.
    pub fn new() -> Self {
        EncryptOptions {
            secrets: Vec::new(),
            kdf: KdfParams::DEFAULT,
            expose_card: false,
        }
    }

    /// Adds a password slot. The UTF-8 bytes are used as-is (normalise to NFC first).
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.secrets
            .push(SlotSecret::Password(Zeroizing::new(password.into())));
        self
    }

    /// Adds a keyfile slot for the 32-byte key of a `.darmkey` file
    /// (see [`generate_keyfile`](crate::generate_keyfile)).
    pub fn keyfile_key(mut self, key: &[u8; 32]) -> Self {
        self.secrets.push(SlotSecret::Key(Zeroizing::new(*key)));
        self
    }

    /// Sets the Argon2id parameters used for password slots.
    pub fn kdf(mut self, params: KdfParams) -> Self {
        self.kdf = params;
        self
    }

    /// Whether the public card stays readable without the key (default: no).
    pub fn expose_card(mut self, expose: bool) -> Self {
        self.expose_card = expose;
        self
    }
}

enum Source {
    Bytes(Vec<u8>),
    File(PathBuf),
}

struct PendingEntry {
    name: String,
    media_type: String,
    role: Role,
    source: Source,
}

/// Sequential reader over an entry's source.
enum SourceReader<'a> {
    Bytes(&'a [u8]),
    File(File, &'a Path),
}

impl<'a> SourceReader<'a> {
    fn open(source: &'a Source) -> Result<SourceReader<'a>> {
        match source {
            Source::Bytes(bytes) => Ok(SourceReader::Bytes(bytes)),
            Source::File(path) => File::open(path)
                .map(|file| SourceReader::File(file, path.as_path()))
                .map_err(|e| io_context(e, path)),
        }
    }

    /// Reads up to `want` bytes into `buf` (replacing its contents). Returns
    /// fewer than `want` bytes only at the end of the source.
    fn read_chunk(&mut self, buf: &mut Vec<u8>, want: usize) -> Result<usize> {
        buf.clear();
        match self {
            SourceReader::Bytes(rest) => {
                let take = want.min(rest.len());
                let (head, tail) = rest.split_at(take);
                buf.extend_from_slice(head);
                *rest = tail;
                Ok(take)
            }
            SourceReader::File(file, path) => {
                let read = Read::by_ref(file)
                    .take(want as u64)
                    .read_to_end(buf)
                    .map_err(|e| io_context(e, path))?;
                Ok(read)
            }
        }
    }
}

fn io_context(error: io::Error, path: &Path) -> PackageError {
    PackageError::Io {
        kind: error.kind(),
        message: format!("{}: {error}", path.display()),
    }
}

/// Layout of one entry as decided by the first pass.
struct PlannedEntry {
    size: u64,
    sha256: [u8; 32],
    compression: Compression,
    stored_size: u64,
    chunks: ChunkTable,
}

/// What [`PackageBuilder`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSummary {
    /// UUID of the package.
    pub uuid: [u8; 16],
    /// Total size of the package in bytes.
    pub total_len: u64,
    /// Size of header + prelude + manifest.
    pub head_len: u64,
    /// Size of the data area (stored bytes of all entries).
    pub data_len: u64,
    /// Sum of the original sizes of all entries.
    pub plain_len: u64,
    /// Number of entries, including the IR.
    pub entry_count: usize,
    /// Number of entries stored deflated.
    pub compressed_entries: usize,
    /// Whether the package is encrypted.
    pub encrypted: bool,
}

impl BuildSummary {
    /// The package UUID in canonical text form.
    pub fn uuid_string(&self) -> String {
        crate::util::format_uuid(&self.uuid)
    }
}

/// Builds a `.darm` package.
///
/// ```
/// use darm_package::{Credentials, Package, PackageBuilder, PublicCard};
///
/// # fn main() -> Result<(), darm_package::PackageError> {
/// let mut builder = PackageBuilder::new();
/// builder
///     .card(PublicCard { title: "My book".into(), ..PublicCard::default() })
///     .book_summary(serde_json::json!({ "title": "My book", "chapters": 3 }))?
///     .ir(br#"{"ir":1}"#.to_vec())?
///     .add_bytes("media/cover.png", "image/png", vec![0x89, b'P', b'N', b'G'])?;
/// let bytes = builder.to_vec()?;
///
/// let package = Package::open(bytes.as_slice(), &Credentials::None)?;
/// assert_eq!(package.ir_bytes()?, br#"{"ir":1}"#);
/// assert_eq!(package.entries().len(), 2);
/// # Ok(())
/// # }
/// ```
///
/// Building a clear package is deterministic: the same inputs and options
/// always produce the same bytes (the UUID is derived from the content and no
/// clock is consulted). Encrypted builds draw a fresh content key, UUID and
/// salts from the operating system and therefore differ every time.
pub struct PackageBuilder {
    generator: String,
    chunk_size: u32,
    compression: CompressionMode,
    card: Option<PublicCard>,
    book: Box<RawValue>,
    created: Option<String>,
    ir: Option<PendingEntry>,
    entries: Vec<PendingEntry>,
    encrypt: Option<EncryptOptions>,
}

impl Default for PackageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PackageBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageBuilder")
            .field("generator", &self.generator)
            .field("chunk_size", &self.chunk_size)
            .field(
                "entries",
                &(self.entries.len() + usize::from(self.ir.is_some())),
            )
            .field("encrypt", &self.encrypt)
            .finish_non_exhaustive()
    }
}

impl PackageBuilder {
    /// A builder with no entries, 1 MiB chunks, automatic compression and no encryption.
    pub fn new() -> Self {
        PackageBuilder {
            generator: concat!("darm-package/", env!("CARGO_PKG_VERSION")).to_owned(),
            chunk_size: DEFAULT_CHUNK_SIZE,
            compression: CompressionMode::Auto,
            card: None,
            book: RawValue::from_string("{}".to_owned()).expect("{} is valid JSON"),
            created: None,
            ir: None,
            entries: Vec::new(),
            encrypt: None,
        }
    }

    /// Sets the generator string recorded in the prelude
    /// (default `darm-package/<crate version>`).
    pub fn generator(&mut self, generator: impl Into<String>) -> &mut Self {
        self.generator = generator.into();
        self
    }

    /// Sets the public card. In an encrypted package it is only written when
    /// [`EncryptOptions::expose_card`] is enabled.
    pub fn card(&mut self, card: PublicCard) -> &mut Self {
        self.card = Some(card);
        self
    }

    /// Sets the opaque book summary stored in the manifest (default `{}`).
    ///
    /// The value is stored in a canonical form — compact, object members
    /// sorted by name — so equal summaries always produce equal packages.
    pub fn book_summary(&mut self, summary: serde_json::Value) -> Result<&mut Self> {
        let mut json = String::new();
        write_canonical_json(&summary, &mut json, 0)?;
        if json.len() > MAX_BOOK_SUMMARY_LEN {
            return Err(PackageError::limit(
                "book summary length",
                json.len() as u64,
                0,
                MAX_BOOK_SUMMARY_LEN as u64,
            ));
        }
        self.book = RawValue::from_string(json).map_err(|e| {
            PackageError::BadOption(format!("the book summary cannot be serialised: {e}"))
        })?;
        Ok(self)
    }

    /// Records a creation time in the manifest (free-form, RFC 3339
    /// recommended). Nothing is recorded unless this is called, which keeps
    /// builds reproducible.
    pub fn created(&mut self, created: impl Into<String>) -> &mut Self {
        self.created = Some(created.into());
        self
    }

    /// Sets the plaintext chunk size (64 KiB to 4 MiB, default 1 MiB).
    /// Validated when the package is written.
    pub fn chunk_size(&mut self, chunk_size: u32) -> &mut Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Sets the compression policy (default [`CompressionMode::Auto`]).
    pub fn compression(&mut self, mode: CompressionMode) -> &mut Self {
        self.compression = mode;
        self
    }

    /// Encrypts the package.
    pub fn encrypt(&mut self, options: EncryptOptions) -> &mut Self {
        self.encrypt = Some(options);
        self
    }

    /// Sets the compiled book IR. It becomes entry 0, named
    /// [`IR_ENTRY_NAME`] with media type [`IR_MEDIA_TYPE`].
    pub fn ir(&mut self, bytes: Vec<u8>) -> Result<&mut Self> {
        if bytes.len() as u64 > MAX_IR_LEN {
            return Err(PackageError::limit(
                "IR size",
                bytes.len() as u64,
                0,
                MAX_IR_LEN,
            ));
        }
        if self.ir.is_none() {
            self.check_new_entry(IR_ENTRY_NAME, IR_MEDIA_TYPE)?;
        }
        self.ir = Some(PendingEntry {
            name: IR_ENTRY_NAME.to_owned(),
            media_type: IR_MEDIA_TYPE.to_owned(),
            role: Role::Ir,
            source: Source::Bytes(bytes),
        });
        Ok(self)
    }

    /// Adds a resource held in memory.
    pub fn add_bytes(
        &mut self,
        name: impl Into<String>,
        media_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<&mut Self> {
        self.add(name.into(), media_type.into(), Source::Bytes(bytes))
    }

    /// Adds a resource read from a file when the package is written. The file
    /// is streamed chunk by chunk and never loaded whole.
    pub fn add_file(
        &mut self,
        name: impl Into<String>,
        media_type: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Result<&mut Self> {
        self.add(name.into(), media_type.into(), Source::File(path.into()))
    }

    fn check_new_entry(&self, name: &str, media_type: &str) -> Result<()> {
        validate_entry_name(name)?;
        validate_media_type(media_type)?;
        let count = self.entries.len() + usize::from(self.ir.is_some());
        if count >= MAX_ENTRIES {
            return Err(PackageError::limit(
                "number of entries",
                count as u64 + 1,
                0,
                MAX_ENTRIES as u64,
            ));
        }
        if self.entries.iter().any(|e| e.name == name) {
            return Err(PackageError::DuplicateName(name.to_owned()));
        }
        Ok(())
    }

    fn add(&mut self, name: String, media_type: String, source: Source) -> Result<&mut Self> {
        if name == IR_ENTRY_NAME {
            return Err(PackageError::BadName {
                name,
                reason: "this name is reserved for the book IR; use PackageBuilder::ir",
            });
        }
        self.check_new_entry(&name, &media_type)?;
        self.entries.push(PendingEntry {
            name,
            media_type,
            role: Role::Resource,
            source,
        });
        Ok(self)
    }

    /// Entries in storage order: the IR first, then resources as added.
    fn ordered(&self) -> impl Iterator<Item = &PendingEntry> {
        self.ir.iter().chain(self.entries.iter())
    }

    /// First pass over one entry: size, hashes and compression decision.
    fn plan_entry(
        &self,
        entry: &PendingEntry,
        encrypted: bool,
        buf: &mut Vec<u8>,
    ) -> Result<PlannedEntry> {
        let chunk_size = self.chunk_size as usize;
        let tag_len = if encrypted { GCM_TAG_LEN as u32 } else { 0 };
        let mut try_deflate =
            self.compression == CompressionMode::Auto && is_text_like(&entry.media_type);

        let mut reader = SourceReader::open(&entry.source)?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let mut raw = ChunkTable::default();
        let mut packed = ChunkTable::default();
        let mut packed_total = 0u64;
        loop {
            let read = reader.read_chunk(buf, chunk_size)?;
            if read == 0 {
                break;
            }
            size += read as u64;
            if size > MAX_ENTRY_LEN {
                return Err(PackageError::limit("entry size", size, 0, MAX_ENTRY_LEN));
            }
            hasher.update(&buf[..]);
            raw.push(read as u32 + tag_len, (!encrypted).then(|| sha256(buf)));
            if try_deflate {
                let deflated = deflate(buf);
                if deflated.len() as u64 > u64::from(max_deflated_chunk_len(read as u32)) {
                    try_deflate = false;
                } else {
                    packed_total += deflated.len() as u64;
                    packed.push(
                        deflated.len() as u32 + tag_len,
                        (!encrypted).then(|| sha256(&deflated)),
                    );
                }
            }
            if read < chunk_size {
                break;
            }
        }
        if entry.role == Role::Ir && size > MAX_IR_LEN {
            return Err(PackageError::limit("IR size", size, 0, MAX_IR_LEN));
        }

        let (compression, chunks) = if try_deflate && packed_total < size {
            (Compression::Deflate, packed)
        } else {
            (Compression::None, raw)
        };
        let stored_size = chunks.lens().iter().map(|&len| u64::from(len)).sum();
        Ok(PlannedEntry {
            size,
            sha256: hasher.finalize().into(),
            compression,
            stored_size,
            chunks,
        })
    }

    /// Writes the package to `out`.
    ///
    /// # Errors
    ///
    /// [`PackageError::Limit`] when an input exceeds a format limit,
    /// [`PackageError::BadOption`] for invalid options,
    /// [`PackageError::SourceChanged`] when an input file changes while the
    /// package is being written, and [`PackageError::Io`] for I/O failures.
    pub fn write_to<W: Write>(&self, mut out: W) -> Result<BuildSummary> {
        validate_chunk_size(self.chunk_size)?;
        validate_label("generator length", &self.generator)?;
        if let Some(created) = &self.created {
            validate_label("created length", created)?;
        }
        if let Some(card) = &self.card {
            card.validate()?;
        }
        let encrypted = self.encrypt.is_some();

        // ---- Pass 1: plan every entry. ------------------------------------
        let mut buf = Vec::new();
        let mut planned = Vec::with_capacity(self.entries.len() + 1);
        let mut entries = Vec::with_capacity(self.entries.len() + 1);
        let mut offset = 0u64;
        let mut plain_len = 0u64;
        for (index, entry) in self.ordered().enumerate() {
            let plan = self.plan_entry(entry, encrypted, &mut buf)?;
            entries.push(Entry {
                index: index as u32,
                name: entry.name.clone(),
                media_type: entry.media_type.clone(),
                role: entry.role,
                size: plan.size,
                sha256: Sha256Hex(plan.sha256),
                compression: plan.compression,
                offset,
                stored_size: plan.stored_size,
                chunks: plan.chunks.clone(),
            });
            offset = offset
                .checked_add(plan.stored_size)
                .filter(|&end| end <= MAX_TOTAL_LEN)
                .ok_or_else(|| PackageError::limit("package length", u64::MAX, 0, MAX_TOTAL_LEN))?;
            plain_len += plan.size;
            planned.push(plan);
        }
        let data_len = offset;

        let manifest = Manifest {
            manifest_version: MANIFEST_VERSION,
            created: self.created.clone(),
            book: self.book.clone(),
            entries,
        };
        let manifest_json = manifest.to_json()?;
        if manifest_json.len() > MAX_MANIFEST_INFLATED_LEN {
            return Err(PackageError::limit(
                "inflated manifest length (use a larger chunk size)",
                manifest_json.len() as u64,
                0,
                MAX_MANIFEST_INFLATED_LEN as u64,
            ));
        }
        let mut manifest_stored = deflate(manifest_json.as_bytes());

        // ---- Prelude, keys and header. ------------------------------------
        let card = match &self.encrypt {
            Some(options) if !options.expose_card => None,
            _ => self.card.clone(),
        };
        let mut prelude = Prelude {
            generator: self.generator.clone(),
            chunk_size: self.chunk_size,
            card,
            encryption: None,
        };
        let (uuid, keys) = match &self.encrypt {
            Some(options) => {
                let mut uuid = random_bytes::<16>()?;
                uuid[6] = (uuid[6] & 0x0f) | 0x40; // version 4 (random)
                uuid[8] = (uuid[8] & 0x3f) | 0x80; // RFC 9562 variant
                let cek: Secret = Zeroizing::new(random_bytes::<32>()?);
                prelude.encryption = Some(EncryptionInfo {
                    cipher: CIPHER_AES_256_GCM.to_owned(),
                    subkeys: SUBKEYS_HKDF_SHA256.to_owned(),
                    key_commitment: Base64::encode_string(&key_commitment(&cek, &uuid)),
                    slots: make_slots(options, &cek, &uuid)?,
                });
                (uuid, Some(ContentKeys::derive(&cek, &uuid)))
            }
            None => (derive_uuid(&prelude, manifest_json.as_bytes())?, None),
        };
        let prelude_bytes = prelude.to_bytes()?;
        if prelude_bytes.len() > MAX_PRELUDE_LEN {
            return Err(PackageError::limit(
                "prelude length",
                prelude_bytes.len() as u64,
                2,
                MAX_PRELUDE_LEN as u64,
            ));
        }

        let manifest_len = manifest_stored.len() + if encrypted { GCM_TAG_LEN } else { 0 };
        if manifest_len > MAX_MANIFEST_STORED_LEN {
            return Err(PackageError::limit(
                "stored manifest length (use a larger chunk size)",
                manifest_len as u64,
                1,
                MAX_MANIFEST_STORED_LEN as u64,
            ));
        }
        let head_len = (HEADER_LEN + prelude_bytes.len() + manifest_len) as u64;
        let total_len = head_len
            .checked_add(data_len)
            .filter(|&total| total <= MAX_TOTAL_LEN)
            .ok_or_else(|| {
                PackageError::limit(
                    "package length",
                    head_len.saturating_add(data_len),
                    0,
                    MAX_TOTAL_LEN,
                )
            })?;

        let mut flags = 0;
        if encrypted {
            flags |= FLAG_ENCRYPTED;
        }
        if prelude.card.is_some() {
            flags |= FLAG_HAS_PUBLIC_CARD;
        }
        let mut header = Header {
            format_major: FORMAT_MAJOR,
            format_minor: FORMAT_MINOR,
            flags,
            uuid,
            total_len,
            prelude_len: prelude_bytes.len() as u32,
            manifest_len: manifest_len as u32,
            prelude_sha256: sha256(&prelude_bytes),
            manifest_sha256: [0u8; 32],
        };
        if let Some(keys) = &keys {
            // The sealed manifest binds header bytes 0..80, none of which
            // depend on the manifest ciphertext.
            keys.seal_manifest(&header.encode(), &mut manifest_stored)?;
        }
        header.manifest_sha256 = sha256(&manifest_stored);

        // The writer holds itself to the reader's rules.
        prelude.validate(encrypted, prelude.card.is_some())?;
        manifest.validate(encrypted, self.chunk_size, head_len, data_len)?;
        drop(manifest_json);

        // ---- Pass 2: write. -----------------------------------------------
        let mut written = 0u64;
        let mut put = |out: &mut W, bytes: &[u8]| -> Result<()> {
            out.write_all(bytes)?;
            written += bytes.len() as u64;
            Ok(())
        };
        put(&mut out, &header.encode())?;
        put(&mut out, &prelude_bytes)?;
        put(&mut out, &manifest_stored)?;

        let mut compressed_entries = 0;
        for ((index, entry), plan) in self.ordered().enumerate().zip(&planned) {
            let changed = || {
                PackageError::SourceChanged(match &entry.source {
                    Source::File(path) => format!("{}", path.display()),
                    Source::Bytes(_) => format!("entry \"{}\"", quote(&entry.name)),
                })
            };
            let deflated = plan.compression == Compression::Deflate;
            compressed_entries += usize::from(deflated);
            let mut reader = SourceReader::open(&entry.source)?;
            let mut hasher = Sha256::new();
            let chunk_total = plan.chunks.len();
            for chunk in 0..chunk_total {
                let plain_len = chunk_plain_len(plan.size, self.chunk_size, chunk as u64);
                let read = reader.read_chunk(&mut buf, plain_len as usize)?;
                if read != plain_len as usize {
                    return Err(changed());
                }
                if encrypted {
                    hasher.update(&buf[..]);
                }
                if deflated {
                    buf = deflate(&buf);
                }
                if let Some(keys) = &keys {
                    let context = ChunkContext {
                        uuid: &uuid,
                        entry: index as u32,
                        chunk: chunk as u64,
                        is_final: chunk + 1 == chunk_total,
                        plain_len,
                        compression: if deflated {
                            AAD_COMPRESSION_DEFLATE
                        } else {
                            AAD_COMPRESSION_NONE
                        },
                    };
                    keys.seal_chunk(&context, &mut buf)?;
                }
                if plan.chunks.stored_len(chunk) != Some(buf.len() as u32) {
                    return Err(changed());
                }
                if let Some(expected) = plan.chunks.sha256(chunk) {
                    // Clear package: the stored bytes must hash to what the
                    // manifest (already written) promises.
                    if sha256(&buf) != *expected {
                        return Err(changed());
                    }
                }
                put(&mut out, &buf)?;
            }
            if reader.read_chunk(&mut buf, 1)? != 0 {
                return Err(changed());
            }
            if encrypted {
                let digest: [u8; 32] = hasher.finalize().into();
                if digest != plan.sha256 {
                    return Err(changed());
                }
            }
        }
        out.flush()?;
        if written != total_len {
            return Err(PackageError::SourceChanged(format!(
                "wrote {written} bytes where {total_len} were planned"
            )));
        }

        Ok(BuildSummary {
            uuid,
            total_len,
            head_len,
            data_len,
            plain_len,
            entry_count: planned.len(),
            compressed_entries,
            encrypted,
        })
    }

    /// Builds the package in memory.
    pub fn to_vec(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.write_to(&mut out)?;
        Ok(out)
    }

    /// Writes the package to `path` atomically: the bytes go to a temporary
    /// file in the same directory, are flushed to disk, and the file is then
    /// renamed over `path`. On failure the temporary file is removed and
    /// `path` is left untouched.
    pub fn write_to_path(&self, path: impl AsRef<Path>) -> Result<BuildSummary> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = path.as_ref();
        let file_name = path.file_name().ok_or_else(|| {
            PackageError::BadOption(format!("\"{}\" is not a file path", path.display()))
        })?;
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let temp_path = path.with_file_name(temp_name);

        let result = (|| {
            let file = File::options()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|e| io_context(e, &temp_path))?;
            let mut writer = io::BufWriter::with_capacity(256 * 1024, file);
            let summary = self.write_to(&mut writer)?;
            let file = writer
                .into_inner()
                .map_err(|e| io_context(e.into_error(), &temp_path))?;
            file.sync_all().map_err(|e| io_context(e, &temp_path))?;
            drop(file);
            std::fs::rename(&temp_path, path).map_err(|e| io_context(e, path))?;
            Ok(summary)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }
}

/// Deepest nesting accepted in a book summary (the limit `serde_json` itself
/// applies when parsing).
const MAX_JSON_DEPTH: usize = 128;

/// Serialises `value` compactly with object members sorted by name, so the
/// output does not depend on how the value was built (or on whether some
/// other crate enabled `serde_json`'s `preserve_order` feature).
fn write_canonical_json(value: &serde_json::Value, out: &mut String, depth: usize) -> Result<()> {
    use serde_json::Value;
    let scalar = |value: &Value, out: &mut String| -> Result<()> {
        let text = serde_json::to_string(value).map_err(|e| {
            PackageError::BadOption(format!("the book summary cannot be serialised: {e}"))
        })?;
        out.push_str(&text);
        Ok(())
    };
    if depth > MAX_JSON_DEPTH {
        return Err(PackageError::BadOption(format!(
            "the book summary is nested deeper than {MAX_JSON_DEPTH} levels"
        )));
    }
    match value {
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical_json(item, out, depth + 1)?;
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut sorted: Vec<(&String, &Value)> = members.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (name, member)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                scalar(&Value::String(name.clone()), out)?;
                out.push(':');
                write_canonical_json(member, out, depth + 1)?;
            }
            out.push('}');
        }
        other => scalar(other, out)?,
    }
    Ok(())
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| PackageError::Random(e.to_string()))?;
    Ok(bytes)
}

/// UUID of a clear package: a version-8 UUID made of the first 16 bytes of
/// `SHA-256("darm/1/package-id\0" || u32-LE(prelude length) || prelude || manifest JSON)`.
fn derive_uuid(prelude: &Prelude, manifest_json: &[u8]) -> Result<[u8; 16]> {
    let prelude_bytes = prelude.to_bytes()?;
    let digest = sha256_parts(&[
        UUID_DOMAIN,
        &(prelude_bytes.len() as u32).to_le_bytes(),
        &prelude_bytes,
        manifest_json,
    ]);
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&digest[..16]);
    uuid[6] = (uuid[6] & 0x0f) | 0x80; // version 8 (custom / hash-derived)
    uuid[8] = (uuid[8] & 0x3f) | 0x80; // RFC 9562 variant
    Ok(uuid)
}

fn make_slots(options: &EncryptOptions, cek: &Secret, uuid: &[u8; 16]) -> Result<Vec<KeySlot>> {
    if options.secrets.is_empty() {
        return Err(PackageError::BadOption(
            "encryption needs a password or a keyfile key".into(),
        ));
    }
    if options.secrets.len() > MAX_KEY_SLOTS {
        return Err(PackageError::limit(
            "number of key slots",
            options.secrets.len() as u64,
            1,
            MAX_KEY_SLOTS as u64,
        ));
    }
    options.kdf.validate()?;
    let mut slots = Vec::with_capacity(options.secrets.len());
    for (index, secret) in options.secrets.iter().enumerate() {
        let index = index as u32;
        let slot = match secret {
            SlotSecret::Password(password) => {
                if password.is_empty() {
                    return Err(PackageError::BadOption("the password is empty".into()));
                }
                let salt = random_bytes::<SALT_LEN>()?;
                let kek = password_kek(password.as_bytes(), &salt, options.kdf)?;
                let binding = SlotBinding::Password {
                    params: options.kdf,
                    salt: &salt,
                };
                let wrapped = wrap_cek(&kek, cek, uuid, index, &binding)?;
                KeySlot::new_password(options.kdf, &salt, &wrapped)
            }
            SlotSecret::Key(key) => {
                let key_id = key_id_bytes(key);
                let duplicate = options.secrets[..index as usize].iter().any(
                    |other| matches!(other, SlotSecret::Key(k) if k.as_slice() == key.as_slice()),
                );
                if duplicate {
                    return Err(PackageError::BadOption(
                        "the same keyfile key was added twice".into(),
                    ));
                }
                let kek = keyfile_kek(key, uuid, index);
                let binding = SlotBinding::Keyfile { key_id: &key_id };
                let wrapped = wrap_cek(&kek, cek, uuid, index, &binding)?;
                KeySlot::new_keyfile(&key_id, &wrapped)
            }
        };
        slots.push(slot);
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_sorts_members_and_is_compact() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{ "b": [1, {"z": null, "a": "x\"y"}], "a": {"k": 1.5, "c": true}, "\u00e9": -3 }"#,
        )
        .unwrap();
        let mut out = String::new();
        write_canonical_json(&value, &mut out, 0).unwrap();
        assert_eq!(
            out,
            r#"{"a":{"c":true,"k":1.5},"b":[1,{"a":"x\"y","z":null}],"é":-3}"#
        );
        // The canonical form parses back to the same value.
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&out).unwrap(),
            value
        );

        let mut deep = serde_json::Value::Null;
        for _ in 0..200 {
            deep = serde_json::Value::Array(vec![deep]);
        }
        let mut out = String::new();
        assert_eq!(
            write_canonical_json(&deep, &mut out, 0).unwrap_err().code(),
            "E_PKG_BAD_OPTION"
        );
    }

    #[test]
    fn derived_uuid_is_version_8() {
        let prelude = Prelude {
            generator: "g".into(),
            chunk_size: DEFAULT_CHUNK_SIZE,
            card: None,
            encryption: None,
        };
        let a = derive_uuid(&prelude, b"{}").unwrap();
        let b = derive_uuid(&prelude, b"{ }").unwrap();
        assert_ne!(a, b);
        assert_eq!(a[6] >> 4, 8);
        assert_eq!(a[8] >> 6, 0b10);
        assert_eq!(a, derive_uuid(&prelude, b"{}").unwrap());
    }
}
