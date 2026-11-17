//! Small helpers: hashing, hex, UUID formatting, bounded (de)compression.

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};
use sha2::{Digest, Sha256};

use crate::error::{PackageError, Result};

/// SHA-256 of `data`.
pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// SHA-256 over the concatenation of `parts`.
pub(crate) fn sha256_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// Lower-case hexadecimal encoding.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// Strict lower-case hexadecimal decoding into a fixed-size array.
pub(crate) fn hex_decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = (nibble(bytes[2 * i])? << 4) | nibble(bytes[2 * i + 1])?;
    }
    Some(out)
}

/// Formats a package UUID in the canonical `8-4-4-4-12` lower-case form.
pub fn format_uuid(uuid: &[u8; 16]) -> String {
    let hex = hex_encode(uuid);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Allocates a zero-filled buffer, reporting failure instead of aborting.
pub(crate) fn try_zeroed(len: usize) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(len)
        .map_err(|_| PackageError::OutOfMemory { bytes: len as u64 })?;
    buffer.resize(len, 0);
    Ok(buffer)
}

/// Why a bounded inflate failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InflateError {
    /// The deflate stream is malformed or incomplete.
    Corrupt,
    /// The output would exceed the allowed size.
    TooLarge,
    /// Bytes remain after the end of the deflate stream.
    TrailingData,
    /// The output buffer could not be allocated.
    OutOfMemory,
}

/// Inflates a raw deflate stream into at most `max_len` bytes.
///
/// The output buffer starts at `initial_len` bytes and never grows beyond
/// `max_len`, so a decompression bomb costs at most `max_len` bytes of memory.
/// The whole of `input` must be consumed by the stream.
pub(crate) fn inflate_bounded(
    input: &[u8],
    initial_len: usize,
    max_len: usize,
) -> std::result::Result<Vec<u8>, InflateError> {
    if max_len == 0 {
        return Err(InflateError::TooLarge);
    }
    let grow = |buffer: &mut Vec<u8>, new_len: usize| {
        let extra = new_len.saturating_sub(buffer.len());
        buffer
            .try_reserve_exact(extra)
            .map_err(|_| InflateError::OutOfMemory)?;
        buffer.resize(new_len, 0);
        Ok::<(), InflateError>(())
    };

    let mut out = Vec::new();
    grow(&mut out, initial_len.clamp(1, max_len))?;
    let mut state = Box::<DecompressorOxide>::default();
    let mut remaining = input;
    let mut written = 0usize;
    loop {
        let (status, consumed, produced) = decompress(
            &mut state,
            remaining,
            &mut out,
            written,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
        );
        written = written.saturating_add(produced);
        remaining = remaining.get(consumed..).ok_or(InflateError::Corrupt)?;
        match status {
            TINFLStatus::Done => {
                if !remaining.is_empty() {
                    return Err(InflateError::TrailingData);
                }
                if written > out.len() {
                    return Err(InflateError::Corrupt);
                }
                out.truncate(written);
                return Ok(out);
            }
            TINFLStatus::HasMoreOutput => {
                if out.len() >= max_len {
                    return Err(InflateError::TooLarge);
                }
                let new_len = out.len().saturating_mul(2).min(max_len);
                grow(&mut out, new_len)?;
            }
            _ => return Err(InflateError::Corrupt),
        }
    }
}

/// Deflates `input` (raw deflate, fixed level so output is reproducible).
#[cfg(feature = "write")]
pub(crate) fn deflate(input: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec(input, 6)
}

/// Number of chunks an entry of `size` plaintext bytes has.
pub(crate) fn chunk_count(size: u64, chunk_size: u32) -> u64 {
    size.div_ceil(u64::from(chunk_size))
}

/// Plaintext length of chunk `chunk` of an entry of `size` bytes.
///
/// The caller guarantees `chunk < chunk_count(size, chunk_size)`.
pub(crate) fn chunk_plain_len(size: u64, chunk_size: u32, chunk: u64) -> u32 {
    let start = chunk.saturating_mul(u64::from(chunk_size));
    let remaining = size.saturating_sub(start);
    remaining.min(u64::from(chunk_size)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let bytes = [0x00, 0x7f, 0x80, 0xff];
        assert_eq!(hex_encode(&bytes), "007f80ff");
        assert_eq!(hex_decode::<4>("007f80ff"), Some(bytes));
        assert_eq!(hex_decode::<4>("007F80FF"), None);
        assert_eq!(hex_decode::<4>("007f80f"), None);
        assert_eq!(hex_decode::<4>("007f80fg"), None);
    }

    #[test]
    fn uuid_format() {
        let uuid = [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
            0xde, 0xf0,
        ];
        assert_eq!(format_uuid(&uuid), "12345678-9abc-def0-1234-56789abcdef0");
    }

    #[test]
    fn chunk_geometry() {
        assert_eq!(chunk_count(0, 65536), 0);
        assert_eq!(chunk_count(1, 65536), 1);
        assert_eq!(chunk_count(65536, 65536), 1);
        assert_eq!(chunk_count(65537, 65536), 2);
        assert_eq!(chunk_plain_len(65537, 65536, 0), 65536);
        assert_eq!(chunk_plain_len(65537, 65536, 1), 1);
    }

    #[cfg(feature = "write")]
    #[test]
    fn inflate_is_bounded() {
        let plain = vec![7u8; 100_000];
        let packed = deflate(&plain);
        assert!(packed.len() < 1000);
        assert_eq!(inflate_bounded(&packed, 16, 100_000).unwrap(), plain);
        assert_eq!(
            inflate_bounded(&packed, 16, 99_999),
            Err(InflateError::TooLarge)
        );
        let mut trailing = packed.clone();
        trailing.push(0);
        assert_eq!(
            inflate_bounded(&trailing, 16, 100_000),
            Err(InflateError::TrailingData)
        );
        assert_eq!(
            inflate_bounded(&packed[..packed.len() - 1], 16, 100_000),
            Err(InflateError::Corrupt)
        );
    }
}
