//! Hard limits of the `.darm` 1.x format.
//!
//! Every limit is enforced by the writer *and* by the reader. The reader checks
//! each length declared inside a file against these constants and against the
//! real length of the source before it allocates anything.

/// Size in bytes of the fixed header at the start of every package.
pub const HEADER_LEN: usize = 128;

/// Maximum size in bytes of the clear-text prelude JSON.
pub const MAX_PRELUDE_LEN: usize = 64 * 1024;

/// Maximum size in bytes of the manifest as stored (deflated, and sealed when encrypted).
pub const MAX_MANIFEST_STORED_LEN: usize = 16 * 1024 * 1024;

/// Maximum size in bytes of the manifest JSON after inflation.
pub const MAX_MANIFEST_INFLATED_LEN: usize = 64 * 1024 * 1024;

/// Maximum size in bytes of the opaque `book` summary JSON inside the manifest.
pub const MAX_BOOK_SUMMARY_LEN: usize = 1024 * 1024;

/// Maximum number of entries in one package.
pub const MAX_ENTRIES: usize = 16_384;

/// Maximum length in bytes (UTF-8) of an entry name.
pub const MAX_NAME_LEN: usize = 512;

/// Maximum length in bytes of an entry media type.
pub const MAX_MEDIA_TYPE_LEN: usize = 255;

/// Maximum length in bytes of the `generator` and `created` strings.
pub const MAX_LABEL_LEN: usize = 256;

/// Smallest allowed plaintext chunk size.
pub const MIN_CHUNK_SIZE: u32 = 64 * 1024;

/// Largest allowed plaintext chunk size.
pub const MAX_CHUNK_SIZE: u32 = 4 * 1024 * 1024;

/// Chunk size used by the writer unless told otherwise.
pub const DEFAULT_CHUNK_SIZE: u32 = 1024 * 1024;

/// Maximum plaintext size in bytes of a single entry.
pub const MAX_ENTRY_LEN: u64 = 16 * 1024 * 1024 * 1024;

/// Maximum plaintext size in bytes of the IR entry (it is always read whole into memory).
pub const MAX_IR_LEN: u64 = 256 * 1024 * 1024;

/// Maximum total size in bytes of a package file.
pub const MAX_TOTAL_LEN: u64 = 64 * 1024 * 1024 * 1024;

/// Largest integer a JavaScript `number` holds exactly (2^53 - 1). Every size
/// and offset in a package is far below it because of [`MAX_TOTAL_LEN`] and
/// [`MAX_ENTRY_LEN`].
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Maximum number of chunks one entry can have (`MAX_ENTRY_LEN / MIN_CHUNK_SIZE`).
pub const MAX_CHUNKS_PER_ENTRY: usize = (MAX_ENTRY_LEN / MIN_CHUNK_SIZE as u64) as usize;

/// Minimum and maximum number of key slots of an encrypted package.
pub const MAX_KEY_SLOTS: usize = 8;

/// Length in bytes of the AES-256-GCM authentication tag appended to every sealed blob.
pub const GCM_TAG_LEN: usize = 16;

/// Lower bound accepted for the Argon2id memory cost, in KiB (8 MiB).
pub const KDF_MIN_M_KIB: u32 = 8 * 1024;

/// Upper bound accepted for the Argon2id memory cost, in KiB (1 GiB).
pub const KDF_MAX_M_KIB: u32 = 1024 * 1024;

/// Lower bound accepted for the Argon2id pass count.
pub const KDF_MIN_T: u32 = 1;

/// Upper bound accepted for the Argon2id pass count.
pub const KDF_MAX_T: u32 = 16;

/// Lower bound accepted for the Argon2id lane count.
pub const KDF_MIN_P: u32 = 1;

/// Upper bound accepted for the Argon2id lane count.
pub const KDF_MAX_P: u32 = 4;

/// Maximum size in bytes of a `.darmkey` file.
pub const MAX_KEYFILE_LEN: usize = 4096;

/// Largest number of stored bytes a deflated chunk of `plain_len` plaintext
/// bytes may occupy (before the GCM tag). The writer falls back to storing an
/// entry raw when deflate would exceed it; the reader rejects larger chunks.
pub const fn max_deflated_chunk_len(plain_len: u32) -> u32 {
    plain_len + plain_len / 8 + 1024
}
