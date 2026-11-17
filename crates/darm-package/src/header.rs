//! The fixed 128-byte header.
//!
//! | Offset | Size | Field             | Notes                                              |
//! |-------:|-----:|-------------------|----------------------------------------------------|
//! |      0 |    8 | `magic`           | `89 44 41 52 4D 0D 0A 1A` (`"\x89DARM\r\n\x1a"`)    |
//! |      8 |    2 | `format_major`    | u16 LE, `1`                                        |
//! |     10 |    2 | `format_minor`    | u16 LE, `0`                                        |
//! |     12 |    4 | `flags`           | u32 LE, bit 0 `ENCRYPTED`, bit 1 `HAS_PUBLIC_CARD` |
//! |     16 |   16 | `uuid`            | package UUID, big-endian byte order (RFC 9562)     |
//! |     32 |    8 | `total_len`       | u64 LE, length of the whole file                   |
//! |     40 |    4 | `prelude_len`     | u32 LE                                             |
//! |     44 |    4 | `manifest_len`    | u32 LE, length of the manifest as stored           |
//! |     48 |   32 | `prelude_sha256`  | SHA-256 of the prelude bytes                       |
//! |     80 |   32 | `manifest_sha256` | SHA-256 of the manifest bytes as stored            |
//! |    112 |    8 | `reserved`        | must be zero                                       |
//! |    120 |    8 | `header_check`    | first 8 bytes of SHA-256 over bytes `0..120`       |

use crate::error::{PackageError, Result};
use crate::limits::{
    GCM_TAG_LEN, HEADER_LEN, MAX_MANIFEST_STORED_LEN, MAX_PRELUDE_LEN, MAX_TOTAL_LEN,
};
use crate::util::sha256;

/// The eight magic bytes every package starts with: `"\x89DARM\r\n\x1a"`.
pub const MAGIC: [u8; 8] = [0x89, 0x44, 0x41, 0x52, 0x4D, 0x0D, 0x0A, 0x1A];

/// Format major version written and understood by this crate.
pub const FORMAT_MAJOR: u16 = 1;

/// Format minor version written by this crate.
pub const FORMAT_MINOR: u16 = 0;

/// Header flag bit 0: the manifest and every chunk are sealed with AES-256-GCM.
pub const FLAG_ENCRYPTED: u32 = 1 << 0;

/// Header flag bit 1: the prelude carries a public card.
pub const FLAG_HAS_PUBLIC_CARD: u32 = 1 << 1;

/// Header flag bit 2: reserved for a future detached-signature block.
/// Format 1.0 never sets it and 1.0 readers reject files that do.
pub const FLAG_RESERVED_SIGNED: u32 = 1 << 2;

/// All flag bits defined by format 1.0.
pub const KNOWN_FLAGS: u32 = FLAG_ENCRYPTED | FLAG_HAS_PUBLIC_CARD;

/// Number of leading header bytes bound into the manifest's AEAD associated
/// data (everything up to and including `prelude_sha256`).
pub(crate) const HEADER_AAD_LEN: usize = 80;

const OFFSET_CHECK: usize = 120;

/// Decoded fixed header of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Format major version.
    pub format_major: u16,
    /// Format minor version.
    pub format_minor: u16,
    /// Feature flags (`FLAG_*`).
    pub flags: u32,
    /// Package UUID.
    pub uuid: [u8; 16],
    /// Length in bytes of the whole file.
    pub total_len: u64,
    /// Length in bytes of the prelude.
    pub prelude_len: u32,
    /// Length in bytes of the manifest as stored.
    pub manifest_len: u32,
    /// SHA-256 of the prelude bytes.
    pub prelude_sha256: [u8; 32],
    /// SHA-256 of the stored manifest bytes.
    pub manifest_sha256: [u8; 32],
}

impl Header {
    /// `true` when the `ENCRYPTED` flag is set.
    pub fn is_encrypted(&self) -> bool {
        self.flags & FLAG_ENCRYPTED != 0
    }

    /// `true` when the `HAS_PUBLIC_CARD` flag is set.
    pub fn has_public_card(&self) -> bool {
        self.flags & FLAG_HAS_PUBLIC_CARD != 0
    }

    /// Offset of the stored manifest (`HEADER_LEN + prelude_len`).
    pub fn manifest_offset(&self) -> u64 {
        HEADER_LEN as u64 + u64::from(self.prelude_len)
    }

    /// Length of header + prelude + manifest; also the offset of the data area.
    pub fn head_len(&self) -> u64 {
        self.manifest_offset() + u64::from(self.manifest_len)
    }

    /// Length in bytes of the data area.
    pub fn data_len(&self) -> u64 {
        self.total_len.saturating_sub(self.head_len())
    }

    /// Serialises the header, computing the trailing check value.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..8].copy_from_slice(&MAGIC);
        out[8..10].copy_from_slice(&self.format_major.to_le_bytes());
        out[10..12].copy_from_slice(&self.format_minor.to_le_bytes());
        out[12..16].copy_from_slice(&self.flags.to_le_bytes());
        out[16..32].copy_from_slice(&self.uuid);
        out[32..40].copy_from_slice(&self.total_len.to_le_bytes());
        out[40..44].copy_from_slice(&self.prelude_len.to_le_bytes());
        out[44..48].copy_from_slice(&self.manifest_len.to_le_bytes());
        out[48..80].copy_from_slice(&self.prelude_sha256);
        out[80..112].copy_from_slice(&self.manifest_sha256);
        // 112..120 reserved, zero.
        let check = sha256(&out[..OFFSET_CHECK]);
        out[OFFSET_CHECK..].copy_from_slice(&check[..8]);
        out
    }

    /// Parses and validates the fixed header found at the start of `bytes`.
    ///
    /// Checks, in order: magic, major version, header check value, reserved
    /// flags and bytes, and that every declared length is within the format
    /// limits and mutually consistent.
    pub fn decode(bytes: &[u8]) -> Result<Header> {
        let prefix = bytes.len().min(MAGIC.len());
        if bytes[..prefix] != MAGIC[..prefix] || bytes.is_empty() {
            return Err(PackageError::BadMagic);
        }
        let Some(raw) = bytes.first_chunk::<HEADER_LEN>() else {
            return Err(PackageError::Truncated {
                needed: HEADER_LEN as u64,
                available: bytes.len() as u64,
            });
        };

        let u16_at = |at: usize| u16::from_le_bytes([raw[at], raw[at + 1]]);
        let u32_at =
            |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let array_at = |at: usize| {
            let mut out = [0u8; 32];
            out.copy_from_slice(&raw[at..at + 32]);
            out
        };

        // The positions of `magic` and `format_major` are fixed for every
        // version of the format, so the version is checked before anything
        // whose layout could differ in a future major.
        let format_major = u16_at(8);
        let format_minor = u16_at(10);
        if format_major != FORMAT_MAJOR {
            return Err(PackageError::UnsupportedVersion {
                major: format_major,
                minor: format_minor,
            });
        }

        let check = sha256(&raw[..OFFSET_CHECK]);
        if check[..8] != raw[OFFSET_CHECK..] {
            return Err(PackageError::integrity(
                "the header checksum does not match",
            ));
        }

        let flags = u32_at(12);
        if flags & !KNOWN_FLAGS != 0 {
            return Err(PackageError::UnsupportedFeature(format!(
                "reserved header flag bits {:#010x}",
                flags & !KNOWN_FLAGS
            )));
        }
        if raw[112..OFFSET_CHECK].iter().any(|&b| b != 0) {
            return Err(PackageError::CorruptHeader(
                "reserved header bytes are not zero".into(),
            ));
        }

        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&raw[16..32]);
        let mut total = [0u8; 8];
        total.copy_from_slice(&raw[32..40]);
        let header = Header {
            format_major,
            format_minor,
            flags,
            uuid,
            total_len: u64::from_le_bytes(total),
            prelude_len: u32_at(40),
            manifest_len: u32_at(44),
            prelude_sha256: array_at(48),
            manifest_sha256: array_at(80),
        };

        if header.prelude_len < 2 || header.prelude_len as usize > MAX_PRELUDE_LEN {
            return Err(PackageError::limit(
                "prelude length",
                u64::from(header.prelude_len),
                2,
                MAX_PRELUDE_LEN as u64,
            ));
        }
        let min_manifest = if header.is_encrypted() {
            GCM_TAG_LEN as u64 + 1
        } else {
            1
        };
        if u64::from(header.manifest_len) < min_manifest
            || header.manifest_len as usize > MAX_MANIFEST_STORED_LEN
        {
            return Err(PackageError::limit(
                "stored manifest length",
                u64::from(header.manifest_len),
                min_manifest,
                MAX_MANIFEST_STORED_LEN as u64,
            ));
        }
        if header.total_len > MAX_TOTAL_LEN {
            return Err(PackageError::limit(
                "package length",
                header.total_len,
                header.head_len(),
                MAX_TOTAL_LEN,
            ));
        }
        if header.total_len < header.head_len() {
            return Err(PackageError::CorruptHeader(format!(
                "declared package length {} is smaller than its header, prelude and manifest ({})",
                header.total_len,
                header.head_len()
            )));
        }
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Header {
        Header {
            format_major: FORMAT_MAJOR,
            format_minor: FORMAT_MINOR,
            flags: FLAG_HAS_PUBLIC_CARD,
            uuid: [7; 16],
            total_len: 128 + 10 + 20 + 1000,
            prelude_len: 10,
            manifest_len: 20,
            prelude_sha256: [1; 32],
            manifest_sha256: [2; 32],
        }
    }

    fn reseal(raw: &mut [u8; HEADER_LEN]) {
        let check = sha256(&raw[..OFFSET_CHECK]);
        raw[OFFSET_CHECK..].copy_from_slice(&check[..8]);
    }

    #[test]
    fn round_trip() {
        let header = sample();
        let raw = header.encode();
        assert_eq!(Header::decode(&raw).unwrap(), header);
        assert_eq!(header.head_len(), 158);
        assert_eq!(header.data_len(), 1000);
    }

    #[test]
    fn rejects_bad_magic_and_short_input() {
        assert_eq!(Header::decode(b""), Err(PackageError::BadMagic));
        assert_eq!(Header::decode(b"PK\x03\x04"), Err(PackageError::BadMagic));
        let raw = sample().encode();
        assert_eq!(
            Header::decode(&raw[..5]),
            Err(PackageError::Truncated {
                needed: 128,
                available: 5
            })
        );
        assert_eq!(
            Header::decode(&raw[..127]),
            Err(PackageError::Truncated {
                needed: 128,
                available: 127
            })
        );
    }

    #[test]
    fn every_single_bit_flip_is_detected() {
        let raw = sample().encode();
        for byte in 0..HEADER_LEN {
            for bit in 0..8 {
                let mut copy = raw;
                copy[byte] ^= 1 << bit;
                assert!(Header::decode(&copy).is_err(), "byte {byte} bit {bit}");
            }
        }
    }

    #[test]
    fn version_policy() {
        let mut raw = sample().encode();
        raw[8] = 2;
        reseal(&mut raw);
        let error = Header::decode(&raw).unwrap_err();
        assert_eq!(error.code(), "E_PKG_UNSUPPORTED_VERSION");
        assert!(error.to_string().contains("update the reader"));

        let mut raw = sample().encode();
        raw[10] = 7;
        reseal(&mut raw);
        assert_eq!(Header::decode(&raw).unwrap().format_minor, 7);

        let mut raw = sample().encode();
        raw[12] |= FLAG_RESERVED_SIGNED as u8;
        reseal(&mut raw);
        assert_eq!(
            Header::decode(&raw).unwrap_err().code(),
            "E_PKG_UNSUPPORTED_FEATURE"
        );

        let mut raw = sample().encode();
        raw[115] = 1;
        reseal(&mut raw);
        assert_eq!(
            Header::decode(&raw).unwrap_err().code(),
            "E_PKG_CORRUPT_HEADER"
        );
    }

    #[test]
    fn length_limits() {
        let mut header = sample();
        header.prelude_len = MAX_PRELUDE_LEN as u32 + 1;
        header.total_len = u64::from(header.prelude_len) + 100_000;
        assert_eq!(
            Header::decode(&header.encode()).unwrap_err().code(),
            "E_PKG_LIMIT"
        );

        let mut header = sample();
        header.manifest_len = MAX_MANIFEST_STORED_LEN as u32 + 1;
        header.total_len = MAX_MANIFEST_STORED_LEN as u64 * 2;
        assert_eq!(
            Header::decode(&header.encode()).unwrap_err().code(),
            "E_PKG_LIMIT"
        );

        let mut header = sample();
        header.total_len = MAX_TOTAL_LEN + 1;
        assert_eq!(
            Header::decode(&header.encode()).unwrap_err().code(),
            "E_PKG_LIMIT"
        );

        let mut header = sample();
        header.total_len = 150;
        assert_eq!(
            Header::decode(&header.encode()).unwrap_err().code(),
            "E_PKG_CORRUPT_HEADER"
        );

        let mut header = sample();
        header.flags |= FLAG_ENCRYPTED;
        header.manifest_len = 16;
        assert_eq!(
            Header::decode(&header.encode()).unwrap_err().code(),
            "E_PKG_LIMIT"
        );
    }
}
