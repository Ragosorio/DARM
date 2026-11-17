//! Reading packages: probing, opening, decoding chunks, streaming entries.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Deref;

use sha2::{Digest, Sha256};

use crate::crypto::{
    AAD_COMPRESSION_DEFLATE, AAD_COMPRESSION_NONE, ChunkContext, ContentKeys, Secret, SlotBinding,
    key_commitment, key_id_bytes, keyfile_kek, password_kek, unwrap_cek,
};
use crate::error::{PackageError, Result, quote};
use crate::header::Header;
use crate::limits::{HEADER_LEN, MAX_MANIFEST_INFLATED_LEN};
use crate::manifest::{Compression, Entry, EntryLayout, Manifest};
use crate::prelude::{DecodedSlot, KdfParams, Prelude, PublicCard, SlotKind};
use crate::util::{
    InflateError, chunk_plain_len, format_uuid, inflate_bounded, sha256, try_zeroed,
};

/// Random-access byte source a package is read from.
///
/// Implemented for byte slices, `Vec<u8>`, references to any `ReadAt`, and —
/// with the `std` feature — `std::fs::File`.
pub trait ReadAt {
    /// Total length of the source in bytes.
    fn len(&self) -> io::Result<u64>;

    /// Fills `buf` with the bytes starting at `offset`, or fails with
    /// [`io::ErrorKind::UnexpectedEof`] if the source is too short.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;

    /// `true` when the source holds no bytes.
    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }
}

impl ReadAt for [u8] {
    fn len(&self) -> io::Result<u64> {
        Ok(<[u8]>::len(self) as u64)
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let source = usize::try_from(offset)
            .ok()
            .and_then(|start| Some(start..start.checked_add(buf.len())?))
            .and_then(|range| self.get(range))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read past the end of the buffer",
                )
            })?;
        buf.copy_from_slice(source);
        Ok(())
    }
}

impl ReadAt for Vec<u8> {
    fn len(&self) -> io::Result<u64> {
        Ok(Vec::len(self) as u64)
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.as_slice().read_exact_at(offset, buf)
    }
}

impl<T: ReadAt + ?Sized> ReadAt for &T {
    fn len(&self) -> io::Result<u64> {
        (**self).len()
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        (**self).read_exact_at(offset, buf)
    }
}

#[cfg(feature = "std")]
impl ReadAt for std::fs::File {
    fn len(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }

    #[cfg(unix)]
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(self, buf, offset)
    }

    #[cfg(windows)]
    fn read_exact_at(&self, mut offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            match self.seek_read(buf, offset) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "read past the end of the file",
                    ));
                }
                Ok(read) => {
                    buf = &mut buf[read..];
                    offset += read as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Portable fallback: seeks the shared file cursor, so concurrent readers
    /// of the same handle must synchronise externally on such targets.
    #[cfg(not(any(unix, windows)))]
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let mut file = self;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)
    }
}

/// Credentials used to unlock an encrypted package.
#[derive(Clone, Copy)]
pub enum Credentials<'a> {
    /// No credentials: only clear packages can be opened.
    None,
    /// A password. Its UTF-8 bytes are used as-is; front-ends should apply
    /// Unicode NFC normalisation before passing it, on write and on read.
    Password(&'a str),
    /// The 32-byte key of a `.darmkey` file.
    Key(&'a [u8; 32]),
}

impl fmt::Debug for Credentials<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Credentials::None => "Credentials::None",
            Credentials::Password(_) => "Credentials::Password(…)",
            Credentials::Key(_) => "Credentials::Key(…)",
        })
    }
}

/// Public description of one key slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    /// Kind of the slot.
    pub kind: SlotKind,
    /// Password slots: the Argon2id parameters.
    pub kdf: Option<KdfParams>,
    /// Keyfile slots: identifier of the key that opens the slot.
    pub key_id: Option<String>,
}

/// What can be learnt from the first bytes of a package without any key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Format major version of the file.
    pub format_major: u16,
    /// Format minor version of the file.
    pub format_minor: u16,
    /// `true` when the package is encrypted.
    pub encrypted: bool,
    /// Package UUID.
    pub uuid: [u8; 16],
    /// Declared length of the whole file.
    pub total_len: u64,
    /// Number of leading bytes (header + prelude + manifest) needed to open the package.
    pub head_len: u64,
    /// `true` when the bytes given to [`probe`] included the whole prelude,
    /// i.e. when the fields below are meaningful.
    pub prelude_loaded: bool,
    /// Name of the tool that wrote the package.
    pub generator: Option<String>,
    /// Plaintext chunk size of the package.
    pub chunk_size: Option<u32>,
    /// The public card, if the package has one.
    pub card: Option<PublicCard>,
    /// The key slots of an encrypted package.
    pub slots: Vec<SlotInfo>,
    /// Argon2id parameters of the first password slot, if any.
    pub kdf: Option<KdfParams>,
}

impl Probe {
    /// The format version as `"major.minor"`.
    pub fn version(&self) -> String {
        format!("{}.{}", self.format_major, self.format_minor)
    }

    /// The package UUID in canonical text form.
    pub fn uuid_string(&self) -> String {
        format_uuid(&self.uuid)
    }
}

fn load_prelude(header: &Header, head: &[u8]) -> Result<Prelude> {
    let end = HEADER_LEN + header.prelude_len as usize;
    let bytes = head.get(HEADER_LEN..end).ok_or(PackageError::Truncated {
        needed: end as u64,
        available: head.len() as u64,
    })?;
    if sha256(bytes) != header.prelude_sha256 {
        return Err(PackageError::integrity(
            "the prelude does not match its SHA-256 in the header",
        ));
    }
    Prelude::parse(bytes, header)
}

fn slot_infos(prelude: &Prelude) -> Vec<SlotInfo> {
    let Some(encryption) = &prelude.encryption else {
        return Vec::new();
    };
    encryption
        .slots
        .iter()
        .map(|slot| {
            let kind = slot.slot_kind();
            SlotInfo {
                kind,
                kdf: match kind {
                    SlotKind::Password => slot.kdf.as_ref().map(|kdf| kdf.params()),
                    _ => None,
                },
                key_id: match kind {
                    SlotKind::Keyfile => slot.key_id.clone(),
                    _ => None,
                },
            }
        })
        .collect()
}

/// Inspects the first bytes of a package without unlocking it.
///
/// `head` must hold at least the fixed header ([`HEADER_LEN`] bytes). If it
/// also holds the whole prelude — passing the first
/// `HEADER_LEN + MAX_PRELUDE_LEN` bytes of the file (or the whole file when it
/// is shorter) always suffices — the card, slots and KDF parameters are
/// returned too and [`Probe::prelude_loaded`] is `true`.
pub fn probe(head: &[u8]) -> Result<Probe> {
    let header = Header::decode(head)?;
    let mut probe = Probe {
        format_major: header.format_major,
        format_minor: header.format_minor,
        encrypted: header.is_encrypted(),
        uuid: header.uuid,
        total_len: header.total_len,
        head_len: header.head_len(),
        prelude_loaded: false,
        generator: None,
        chunk_size: None,
        card: None,
        slots: Vec::new(),
        kdf: None,
    };
    if head.len() >= HEADER_LEN + header.prelude_len as usize {
        let prelude = load_prelude(&header, head)?;
        probe.prelude_loaded = true;
        probe.slots = slot_infos(&prelude);
        probe.kdf = probe.slots.iter().find_map(|slot| slot.kdf);
        probe.generator = Some(prelude.generator);
        probe.chunk_size = Some(prelude.chunk_size);
        probe.card = prelude.card;
    }
    Ok(probe)
}

/// Finds a slot the credentials open and returns the content key.
fn unlock(header: &Header, prelude: &Prelude, credentials: &Credentials<'_>) -> Result<Secret> {
    let encryption = prelude
        .encryption
        .as_ref()
        .ok_or_else(|| PackageError::prelude("an encrypted package has no encryption section"))?;
    if matches!(credentials, Credentials::None) {
        return Err(PackageError::CredentialsRequired {
            card: prelude.card.clone().map(Box::new),
        });
    }
    let commitment = encryption.commitment()?;
    let committed = |cek: Secret| {
        if key_commitment(&cek, &header.uuid) == commitment {
            Ok(cek)
        } else {
            Err(PackageError::integrity(
                "the unlocked content key does not match the package's key commitment",
            ))
        }
    };
    for (index, slot) in encryption.slots.iter().enumerate() {
        let index = index as u32;
        match (slot.decode()?, credentials) {
            (
                DecodedSlot::Password {
                    params,
                    salt,
                    wrapped,
                },
                Credentials::Password(password),
            ) => {
                let kek = password_kek(password.as_bytes(), &salt, params)?;
                let binding = SlotBinding::Password {
                    params,
                    salt: &salt,
                };
                if let Some(cek) = unwrap_cek(&kek, &wrapped, &header.uuid, index, &binding) {
                    return committed(cek);
                }
            }
            (DecodedSlot::Keyfile { key_id, wrapped }, Credentials::Key(key)) => {
                // A different key id cannot open the slot; skip the AEAD attempt.
                if key_id_bytes(key) != key_id {
                    continue;
                }
                let kek = keyfile_kek(key, &header.uuid, index);
                let binding = SlotBinding::Keyfile { key_id: &key_id };
                if let Some(cek) = unwrap_cek(&kek, &wrapped, &header.uuid, index, &binding) {
                    return committed(cek);
                }
            }
            _ => {}
        }
    }
    Err(PackageError::WrongCredentials)
}

/// An opened package without a byte source: the parsed header, prelude and
/// manifest plus, for encrypted packages, the unlocked data key.
///
/// This is all a reader needs to locate and decode chunks that someone else
/// fetches (a browser slicing a `File`, an HTTP range request, …):
/// [`chunk_location`](PackageHead::chunk_location) says where the stored
/// bytes of a chunk are, [`decode_chunk`](PackageHead::decode_chunk) verifies
/// and decodes them.
pub struct PackageHead {
    header: Header,
    prelude: Prelude,
    manifest: Manifest,
    layout: Vec<EntryLayout>,
    names: BTreeMap<String, u32>,
    ir_index: Option<u32>,
    keys: Option<ContentKeys>,
}

impl fmt::Debug for PackageHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackageHead")
            .field("uuid", &format_uuid(&self.header.uuid))
            .field("encrypted", &self.is_encrypted())
            .field("entries", &self.manifest.entries.len())
            .field("total_len", &self.header.total_len)
            .finish_non_exhaustive()
    }
}

impl PackageHead {
    /// Opens a package from its *head*: the first [`Probe::head_len`] bytes
    /// of the file (header, prelude and manifest). Extra bytes are ignored.
    ///
    /// The caller is responsible for comparing [`total_len`](Self::total_len)
    /// with the real size of the file when it knows it;
    /// [`Package::open`] does this itself.
    ///
    /// # Errors
    ///
    /// [`PackageError::CredentialsRequired`] if the package is encrypted and
    /// `credentials` is [`Credentials::None`]; [`PackageError::WrongCredentials`]
    /// if no key slot accepts them; any other variant if the head is
    /// truncated, corrupt, tampered with or exceeds a format limit.
    pub fn open(head: &[u8], credentials: &Credentials<'_>) -> Result<PackageHead> {
        let header = Header::decode(head)?;
        let head_len = header.head_len();
        if (head.len() as u64) < head_len {
            return Err(PackageError::Truncated {
                needed: head_len,
                available: head.len() as u64,
            });
        }
        let prelude = load_prelude(&header, head)?;

        // `head_len` fits in usize: it is at most HEADER_LEN + 64 KiB + 16 MiB
        // and `head` is at least that long.
        let manifest_start = header.manifest_offset() as usize;
        let stored = &head[manifest_start..head_len as usize];
        if sha256(stored) != header.manifest_sha256 {
            return Err(PackageError::integrity(
                "the manifest does not match its SHA-256 in the header",
            ));
        }

        let (keys, deflated) = if header.is_encrypted() {
            let cek = unlock(&header, &prelude, credentials)?;
            let keys = ContentKeys::derive(&cek, &header.uuid);
            let mut sealed = try_zeroed(stored.len())?;
            sealed.copy_from_slice(stored);
            if !keys.open_manifest(&head[..HEADER_LEN], &mut sealed) {
                return Err(PackageError::integrity(
                    "the manifest failed authentication (the header, prelude or manifest was modified)",
                ));
            }
            (Some(keys), std::borrow::Cow::Owned(sealed))
        } else {
            (None, std::borrow::Cow::Borrowed(stored))
        };

        let initial = deflated.len().saturating_mul(4).max(4096);
        let json =
            inflate_bounded(&deflated, initial, MAX_MANIFEST_INFLATED_LEN).map_err(
                |e| match e {
                    InflateError::TooLarge => PackageError::limit(
                        "inflated manifest length",
                        MAX_MANIFEST_INFLATED_LEN as u64 + 1,
                        0,
                        MAX_MANIFEST_INFLATED_LEN as u64,
                    ),
                    InflateError::OutOfMemory => PackageError::OutOfMemory {
                        bytes: MAX_MANIFEST_INFLATED_LEN as u64,
                    },
                    InflateError::Corrupt | InflateError::TrailingData => {
                        PackageError::manifest("it is not a valid deflate stream")
                    }
                },
            )?;
        drop(deflated);
        let manifest = Manifest::parse(&json)?;
        drop(json);
        let layout = manifest.validate(
            header.is_encrypted(),
            prelude.chunk_size,
            head_len,
            header.data_len(),
        )?;

        Ok(PackageHead {
            header,
            prelude,
            manifest,
            layout: layout.entries,
            names: layout.names,
            ir_index: layout.ir_index,
            keys,
        })
    }

    /// The decoded fixed header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The clear-text prelude.
    pub fn prelude(&self) -> &Prelude {
        &self.prelude
    }

    /// The manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The manifest re-serialised as compact JSON.
    pub fn manifest_json(&self) -> Result<String> {
        self.manifest.to_json()
    }

    /// The entries, in storage order.
    pub fn entries(&self) -> &[Entry] {
        &self.manifest.entries
    }

    /// The entry with index `index`.
    pub fn entry(&self, index: u32) -> Result<&Entry> {
        self.manifest
            .entries
            .get(index as usize)
            .ok_or_else(|| PackageError::NotFound(format!("entry index {index}")))
    }

    /// Looks an entry up by name.
    pub fn entry_by_name(&self, name: &str) -> Option<&Entry> {
        let index = *self.names.get(name)?;
        self.manifest.entries.get(index as usize)
    }

    /// The IR entry, if the package has one.
    pub fn ir_entry(&self) -> Option<&Entry> {
        self.manifest.entries.get(self.ir_index? as usize)
    }

    /// The public card, if the package has one.
    pub fn card(&self) -> Option<&PublicCard> {
        self.prelude.card.as_ref()
    }

    /// Package UUID.
    pub fn uuid(&self) -> &[u8; 16] {
        &self.header.uuid
    }

    /// Package UUID in canonical text form.
    pub fn uuid_string(&self) -> String {
        format_uuid(&self.header.uuid)
    }

    /// `true` when the package is encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.header.is_encrypted()
    }

    /// Plaintext chunk size of the package.
    pub fn chunk_size(&self) -> u32 {
        self.prelude.chunk_size
    }

    /// Declared length of the whole file.
    pub fn total_len(&self) -> u64 {
        self.header.total_len
    }

    /// Length of header + prelude + manifest; the data area starts here.
    pub fn head_len(&self) -> u64 {
        self.header.head_len()
    }

    /// Number of chunks of entry `entry`.
    pub fn chunk_count(&self, entry: u32) -> Result<u32> {
        Ok(self.entry(entry)?.chunks.len() as u32)
    }

    fn chunk_info(&self, entry: u32, chunk: u32) -> Result<(&Entry, &EntryLayout, u32)> {
        let info = self.entry(entry)?;
        let layout = self
            .layout
            .get(entry as usize)
            .ok_or_else(|| PackageError::NotFound(format!("entry index {entry}")))?;
        let stored_len = info.chunks.stored_len(chunk as usize).ok_or_else(|| {
            PackageError::NotFound(format!(
                "chunk {chunk} of entry \"{}\" (it has {} chunks)",
                quote(&info.name),
                info.chunks.len()
            ))
        })?;
        Ok((info, layout, stored_len))
    }

    /// Absolute file offset and stored length of a chunk.
    pub fn chunk_location(&self, entry: u32, chunk: u32) -> Result<(u64, u32)> {
        let (info, layout, stored_len) = self.chunk_info(entry, chunk)?;
        let relative = match &layout.chunk_offsets {
            Some(offsets) => offsets.get(chunk as usize).copied().unwrap_or(0),
            // Uncompressed entries: every chunk before `chunk` is full-sized.
            None => u64::from(chunk) * u64::from(info.chunks.stored_len(0).unwrap_or(0)),
        };
        Ok((layout.start + relative, stored_len))
    }

    /// For an entry stored verbatim (clear package, no compression): the
    /// absolute byte range `(offset, len)` of its content inside the file.
    /// Returns `None` for encrypted or compressed entries.
    ///
    /// Bytes read this way bypass chunk verification.
    pub fn direct_range(&self, entry: u32) -> Result<Option<(u64, u64)>> {
        let info = self.entry(entry)?;
        if self.is_encrypted() || info.compression != Compression::None {
            return Ok(None);
        }
        Ok(Some((self.layout[entry as usize].start, info.size)))
    }

    /// Verifies and decodes the stored bytes of one chunk.
    ///
    /// `stored` must be exactly the bytes at [`chunk_location`](Self::chunk_location).
    /// Clear packages check the chunk's SHA-256; encrypted packages check its
    /// GCM tag, which also binds the chunk to its package, entry and
    /// position. Deflated chunks are then inflated into a buffer of exactly
    /// the declared plaintext length.
    pub fn decode_chunk(&self, entry: u32, chunk: u32, stored: &[u8]) -> Result<Vec<u8>> {
        let (info, _, stored_len) = self.chunk_info(entry, chunk)?;
        let describe = || format!("chunk {chunk} of entry \"{}\"", quote(&info.name));
        if stored.len() as u64 != u64::from(stored_len) {
            return Err(PackageError::integrity(format!(
                "{} is {} bytes long, expected {stored_len}",
                describe(),
                stored.len()
            )));
        }
        let plain_len = chunk_plain_len(info.size, self.prelude.chunk_size, u64::from(chunk));
        let deflated = info.compression == Compression::Deflate;

        let mut data = try_zeroed(stored.len())?;
        data.copy_from_slice(stored);
        match &self.keys {
            Some(keys) => {
                let context = ChunkContext {
                    uuid: &self.header.uuid,
                    entry,
                    chunk: u64::from(chunk),
                    is_final: chunk as usize + 1 == info.chunks.len(),
                    plain_len,
                    compression: if deflated {
                        AAD_COMPRESSION_DEFLATE
                    } else {
                        AAD_COMPRESSION_NONE
                    },
                };
                if !keys.open_chunk(&context, &mut data) {
                    return Err(PackageError::integrity(format!(
                        "{} failed authentication",
                        describe()
                    )));
                }
            }
            None => {
                let expected = info.chunks.sha256(chunk as usize).ok_or_else(|| {
                    PackageError::manifest(format!("{} has no sha256", describe()))
                })?;
                if sha256(&data) != *expected {
                    return Err(PackageError::integrity(format!(
                        "{} does not match its SHA-256",
                        describe()
                    )));
                }
            }
        }

        if deflated {
            let plain_len = plain_len as usize;
            data = match inflate_bounded(&data, plain_len, plain_len) {
                Ok(plain) if plain.len() == plain_len => plain,
                Err(InflateError::OutOfMemory) => {
                    return Err(PackageError::OutOfMemory {
                        bytes: plain_len as u64,
                    });
                }
                _ => {
                    return Err(PackageError::integrity(format!(
                        "{} does not inflate to its declared length of {plain_len} bytes",
                        describe()
                    )));
                }
            };
        } else if data.len() != plain_len as usize {
            return Err(PackageError::integrity(format!(
                "{} has the wrong length",
                describe()
            )));
        }
        Ok(data)
    }
}

/// Summary returned by [`Package::verify_all`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyReport {
    /// Number of entries verified.
    pub entries: usize,
    /// Number of chunks verified.
    pub chunks: u64,
    /// Total plaintext bytes verified.
    pub bytes: u64,
}

/// An opened package together with the source it is read from.
///
/// Dereferences to [`PackageHead`] for everything that needs no I/O.
pub struct Package<S> {
    head: PackageHead,
    source: S,
}

impl<S> fmt::Debug for Package<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.head.fmt(f)
    }
}

impl<S> Deref for Package<S> {
    type Target = PackageHead;

    fn deref(&self) -> &PackageHead {
        &self.head
    }
}

fn truncated_or_io(error: io::Error, needed: u64, available: u64) -> PackageError {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        PackageError::Truncated { needed, available }
    } else {
        error.into()
    }
}

impl<S: ReadAt> Package<S> {
    /// Opens a package from a random-access source.
    ///
    /// Reads and validates the header, prelude and manifest; no entry data is
    /// touched. The declared `total_len` must equal the real source length,
    /// so truncated files and files with trailing data are rejected here.
    ///
    /// # Errors
    ///
    /// See [`PackageHead::open`]; additionally [`PackageError::Truncated`] or
    /// [`PackageError::Integrity`] when the source length is wrong, and
    /// [`PackageError::Io`] when the source fails.
    pub fn open(source: S, credentials: &Credentials<'_>) -> Result<Package<S>> {
        let available = source.len()?;
        let mut fixed = [0u8; HEADER_LEN];
        let have = available.min(HEADER_LEN as u64) as usize;
        source
            .read_exact_at(0, &mut fixed[..have])
            .map_err(|e| truncated_or_io(e, have as u64, available))?;
        let header = Header::decode(&fixed[..have])?;
        if available < header.total_len {
            return Err(PackageError::Truncated {
                needed: header.total_len,
                available,
            });
        }
        if available > header.total_len {
            return Err(PackageError::integrity(format!(
                "the file is {available} bytes long but its header declares {}",
                header.total_len
            )));
        }
        // head_len <= total_len == real length, and is bounded by the format
        // limits (128 + 64 KiB + 16 MiB), so this allocation is safe.
        let head_len = header.head_len();
        let mut head = try_zeroed(head_len as usize)?;
        source
            .read_exact_at(0, &mut head)
            .map_err(|e| truncated_or_io(e, head_len, available))?;
        let head = PackageHead::open(&head, credentials)?;
        Ok(Package { head, source })
    }

    /// The source-independent part of the package.
    pub fn head(&self) -> &PackageHead {
        &self.head
    }

    /// The underlying source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Consumes the package and returns its parts.
    pub fn into_parts(self) -> (PackageHead, S) {
        (self.head, self.source)
    }

    /// Reads, verifies and decodes one chunk.
    pub fn read_chunk(&self, entry: u32, chunk: u32) -> Result<Vec<u8>> {
        let (offset, len) = self.head.chunk_location(entry, chunk)?;
        // `len` was validated against the chunk size bound and the entry
        // against the real file length, so this allocation is bounded.
        let mut stored = try_zeroed(len as usize)?;
        self.source
            .read_exact_at(offset, &mut stored)
            .map_err(|e| truncated_or_io(e, offset + u64::from(len), self.head.total_len()))?;
        self.head.decode_chunk(entry, chunk, &stored)
    }

    /// Reads a whole entry into memory, verifying every chunk and the SHA-256
    /// of the complete content.
    ///
    /// `max_len` is the largest size the caller is willing to hold in memory;
    /// larger entries fail with [`PackageError::Limit`] before anything is
    /// allocated. Use [`entry_reader`](Self::entry_reader) to stream instead.
    pub fn read_entry(&self, entry: u32, max_len: u64) -> Result<Vec<u8>> {
        let info = self.head.entry(entry)?;
        if info.size > max_len {
            return Err(PackageError::limit(
                "entry size for an in-memory read",
                info.size,
                0,
                max_len,
            ));
        }
        let size = usize::try_from(info.size)
            .map_err(|_| PackageError::OutOfMemory { bytes: info.size })?;
        let mut content = Vec::new();
        content
            .try_reserve_exact(size)
            .map_err(|_| PackageError::OutOfMemory { bytes: info.size })?;
        let mut hasher = Sha256::new();
        for chunk in 0..info.chunks.len() as u32 {
            let plain = self.read_chunk(entry, chunk)?;
            hasher.update(&plain);
            content.extend_from_slice(&plain);
        }
        let digest: [u8; 32] = hasher.finalize().into();
        if digest != info.sha256.0 || content.len() != size {
            return Err(PackageError::integrity(format!(
                "entry \"{}\" does not match its SHA-256",
                quote(&info.name)
            )));
        }
        Ok(content)
    }

    /// Reads an entry by name (see [`read_entry`](Self::read_entry)).
    pub fn read_entry_by_name(&self, name: &str, max_len: u64) -> Result<Vec<u8>> {
        let entry = self
            .head
            .entry_by_name(name)
            .ok_or_else(|| PackageError::NotFound(format!("entry \"{}\"", quote(name))))?;
        self.read_entry(entry.index, max_len)
    }

    /// Reads the compiled book IR (at most [`MAX_IR_LEN`](crate::MAX_IR_LEN) bytes).
    pub fn ir_bytes(&self) -> Result<Vec<u8>> {
        let entry = self
            .head
            .ir_entry()
            .ok_or_else(|| PackageError::NotFound("the package has no IR entry".into()))?;
        self.read_entry(entry.index, crate::limits::MAX_IR_LEN)
    }

    /// A streaming, seekable reader over the plaintext of an entry.
    ///
    /// One chunk is decoded (and verified) at a time; memory use is one chunk
    /// regardless of the entry size. Random access cannot check the SHA-256
    /// of the whole entry, only that every chunk read is authentic; use
    /// [`verify_all`](Self::verify_all) or [`read_entry`](Self::read_entry) for that.
    pub fn entry_reader(&self, entry: u32) -> Result<EntryReader<'_, S>> {
        let info = self.head.entry(entry)?;
        Ok(EntryReader {
            package: self,
            entry,
            size: info.size,
            position: 0,
            cached: None,
        })
    }

    /// Verifies the whole package: every chunk (hash or GCM tag, inflation)
    /// and the SHA-256 of every entry's complete plaintext.
    pub fn verify_all(&self) -> Result<VerifyReport> {
        let mut report = VerifyReport {
            entries: 0,
            chunks: 0,
            bytes: 0,
        };
        for info in self.head.entries() {
            let mut hasher = Sha256::new();
            let mut size = 0u64;
            for chunk in 0..info.chunks.len() as u32 {
                let plain = self.read_chunk(info.index, chunk)?;
                hasher.update(&plain);
                size += plain.len() as u64;
                report.chunks += 1;
            }
            let digest: [u8; 32] = hasher.finalize().into();
            if digest != info.sha256.0 || size != info.size {
                return Err(PackageError::integrity(format!(
                    "entry \"{}\" does not match its SHA-256",
                    quote(&info.name)
                )));
            }
            report.entries += 1;
            report.bytes += size;
        }
        Ok(report)
    }
}

#[cfg(feature = "std")]
impl Package<std::fs::File> {
    /// Opens the package file at `path`.
    pub fn open_path(
        path: impl AsRef<std::path::Path>,
        credentials: &Credentials<'_>,
    ) -> Result<Package<std::fs::File>> {
        Package::open(std::fs::File::open(path)?, credentials)
    }
}

/// Streaming reader over one entry; see [`Package::entry_reader`].
pub struct EntryReader<'a, S> {
    package: &'a Package<S>,
    entry: u32,
    size: u64,
    position: u64,
    cached: Option<(u32, Vec<u8>)>,
}

impl<S> fmt::Debug for EntryReader<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntryReader")
            .field("entry", &self.entry)
            .field("size", &self.size)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl<S> EntryReader<'_, S> {
    /// Plaintext size of the entry.
    pub fn size(&self) -> u64 {
        self.size
    }
}

impl<S: ReadAt> Read for EntryReader<'_, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.position >= self.size {
            return Ok(0);
        }
        let chunk_size = u64::from(self.package.chunk_size());
        let chunk = (self.position / chunk_size) as u32;
        if self.cached.as_ref().map(|(index, _)| *index) != Some(chunk) {
            let plain = self.package.read_chunk(self.entry, chunk)?;
            self.cached = Some((chunk, plain));
        }
        let plain = self
            .cached
            .as_ref()
            .map(|(_, data)| data.as_slice())
            .unwrap_or(&[]);
        let within = (self.position % chunk_size) as usize;
        let available = plain.get(within..).unwrap_or(&[]);
        let count = available.len().min(buf.len());
        buf[..count].copy_from_slice(&available[..count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl<S: ReadAt> Seek for EntryReader<'_, S> {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let position = match target {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        match position {
            Some(position) => {
                self.position = position;
                Ok(position)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek to a negative or overflowing position",
            )),
        }
    }
}
