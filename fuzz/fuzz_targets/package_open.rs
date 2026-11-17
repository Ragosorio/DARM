//! Fuzzes the whole reader: open, manifest validation, chunk decoding,
//! streaming and full verification.
//!
//! Every byte of a package is covered by a hash or an AEAD tag, so blind
//! mutations rarely get past the first integrity check. Each input is
//! therefore run twice: as is, and *resealed* — with the header's lengths,
//! hashes and check value recomputed — so that mutations of the prelude and
//! manifest reach the parsers and the layout validator.
//!
//! Password credentials are not exercised here: a single Argon2id run costs
//! far more than everything else in the target. `Credentials::Key` uses the
//! test key of `crates/darm-package/tests/fixtures/encrypted-v1.darm`, so that
//! fixture is a useful seed for the encrypted paths.
#![no_main]

use std::io::{Read, Seek, SeekFrom};

use darm_package::{Credentials, HEADER_LEN, Package, PackageHead};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};

const KEY: [u8; 32] = [
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
];

/// Largest entry the target will read whole.
const READ_CAP: u64 = 8 * 1024 * 1024;

fn exercise(bytes: &[u8]) {
    for credentials in [Credentials::None, Credentials::Key(&KEY)] {
        let _ = PackageHead::open(bytes, &credentials);
        let Ok(package) = Package::open(bytes, &credentials) else {
            continue;
        };
        let _ = package.manifest_json();
        let _ = package.manifest().book();
        let _ = package.ir_bytes();
        for entry in package.entries() {
            let _ = package.direct_range(entry.index);
            let _ = package.entry_by_name(&entry.name);
            for chunk in 0..entry.chunks.len() as u32 {
                let _ = package.chunk_location(entry.index, chunk);
                let _ = package.read_chunk(entry.index, chunk);
            }
            let whole = package.read_entry(entry.index, READ_CAP);
            if let Ok(mut reader) = package.entry_reader(entry.index) {
                let _ = reader.seek(SeekFrom::Start(entry.size / 2));
                let mut tail = Vec::new();
                let streamed = reader.take(READ_CAP).read_to_end(&mut tail);
                // Verified content must be the same whichever way it is read.
                if let (Ok(whole), Ok(_)) = (&whole, &streamed) {
                    assert_eq!(&whole[(entry.size / 2) as usize..], tail.as_slice());
                }
            }
        }
        let _ = package.verify_all();
    }
}

/// Recomputes lengths, hashes and the header check so the input is
/// self-consistent at the container level.
fn reseal(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < HEADER_LEN {
        return None;
    }
    let mut out = bytes.to_vec();
    let body = out.len() - HEADER_LEN;
    let u32_at = |data: &[u8], at: usize| {
        u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize
    };
    let prelude_len = u32_at(&out, 40).min(body);
    let manifest_len = u32_at(&out, 44).min(body - prelude_len);
    let total_len = out.len() as u64;
    out[32..40].copy_from_slice(&total_len.to_le_bytes());
    out[40..44].copy_from_slice(&(prelude_len as u32).to_le_bytes());
    out[44..48].copy_from_slice(&(manifest_len as u32).to_le_bytes());
    let prelude_hash = Sha256::digest(&out[HEADER_LEN..HEADER_LEN + prelude_len]);
    out[48..80].copy_from_slice(&prelude_hash);
    let manifest_start = HEADER_LEN + prelude_len;
    let manifest_hash = Sha256::digest(&out[manifest_start..manifest_start + manifest_len]);
    out[80..112].copy_from_slice(&manifest_hash);
    let check = Sha256::digest(&out[..120]);
    out[120..128].copy_from_slice(&check[..8]);
    Some(out)
}

fuzz_target!(|data: &[u8]| {
    exercise(data);
    if let Some(resealed) = reseal(data) {
        exercise(&resealed);
    }
});
