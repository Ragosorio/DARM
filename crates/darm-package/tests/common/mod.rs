//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::path::PathBuf;

use darm_package::{
    Credentials, EncryptOptions, HEADER_LEN, Header, KdfParams, Package, PackageBuilder, PublicCard,
};
use sha2::{Digest, Sha256};

/// Fastest KDF parameters the format accepts; keeps the suite quick.
pub const FAST_KDF: KdfParams = KdfParams::MIN;

pub const PASSWORD: &str = "correct horse battery staple";
pub const KEY: [u8; 32] = [
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
];

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * 1024;

/// Tiny deterministic PRNG (xorshift64*), so the tests need no dependency.
#[derive(Clone)]
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> Self {
        XorShift(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish value in `0..bound` (`bound > 0`).
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len + 8);
        while out.len() < len {
            out.extend_from_slice(&self.next_u64().to_le_bytes());
        }
        out.truncate(len);
        out
    }
}

/// Incompressible pseudo-random bytes.
pub fn noise(seed: u64, len: usize) -> Vec<u8> {
    XorShift::new(seed).bytes(len)
}

/// Highly compressible text of exactly `len` bytes.
pub fn prose(len: usize) -> Vec<u8> {
    let line = b"WEBVTT cue: the quick brown fox jumps over the lazy dog.\n";
    line.iter().copied().cycle().take(len).collect()
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn card() -> PublicCard {
    PublicCard {
        title: "The Marker Title".into(),
        authors: vec!["Ada Marker".into(), "Grace Marker".into()],
        language: Some("en".into()),
        cover_color: Some("#1f6feb".into()),
    }
}

pub const IR: &[u8] = br#"{"ir_version":1,"marker":"IR-PLAINTEXT-MARKER","nodes":[{"id":"n1","text":"Hello, reader"}]}"#;

/// The entries of the standard sample book: name, media type, content.
pub fn sample_entries() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    vec![
        ("media/clip.bin", "video/mp4", noise(11, 150 * KIB + 123)),
        ("text/captions.vtt", "text/vtt", prose(200 * KIB + 7)),
        ("empty.dat", "application/octet-stream", Vec::new()),
        ("media/one-chunk.png", "image/png", noise(12, 64 * KIB)),
        ("notes/small.txt", "text/plain", b"tiny".to_vec()),
    ]
}

/// A builder holding the sample book with 64 KiB chunks.
pub fn sample_builder() -> PackageBuilder {
    let mut builder = PackageBuilder::new();
    builder
        .generator("darm-tests")
        .chunk_size(64 * KIB as u32)
        .card(card())
        .book_summary(serde_json::json!({"title": "The Marker Title", "chapters": 3}))
        .unwrap()
        .ir(IR.to_vec())
        .unwrap();
    for (name, media_type, content) in sample_entries() {
        builder.add_bytes(name, media_type, content).unwrap();
    }
    builder
}

pub fn fast_encryption() -> EncryptOptions {
    EncryptOptions::new().kdf(FAST_KDF)
}

/// Sample book, clear.
pub fn clear_sample() -> Vec<u8> {
    sample_builder().to_vec().unwrap()
}

/// Sample book, encrypted with a keyfile slot only (no Argon2 on open).
pub fn key_sample() -> Vec<u8> {
    let mut builder = sample_builder();
    builder.encrypt(fast_encryption().keyfile_key(&KEY));
    builder.to_vec().unwrap()
}

/// Sample book, encrypted with a password slot and a keyfile slot.
pub fn password_sample() -> Vec<u8> {
    let mut builder = sample_builder();
    builder.encrypt(fast_encryption().password(PASSWORD).keyfile_key(&KEY));
    builder.to_vec().unwrap()
}

/// Asserts that an opened package holds exactly the sample book.
pub fn assert_sample_content<S: darm_package::ReadAt>(package: &Package<S>) {
    assert_eq!(package.ir_bytes().unwrap(), IR);
    let expected = sample_entries();
    assert_eq!(package.entries().len(), expected.len() + 1);
    for (name, media_type, content) in expected {
        let entry = package
            .entry_by_name(name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(entry.media_type, media_type);
        assert_eq!(entry.size, content.len() as u64);
        assert_eq!(entry.sha256.0, sha256(&content), "{name}");
        assert_eq!(
            package.read_entry(entry.index, u64::MAX).unwrap(),
            content,
            "{name}"
        );
    }
}

/// `true` when `haystack` contains `needle`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// A package split into its four regions, for hand-crafting hostile files.
#[derive(Clone)]
pub struct Parts {
    pub header: Header,
    pub prelude: Vec<u8>,
    pub manifest: Vec<u8>,
    pub data: Vec<u8>,
}

impl Parts {
    pub fn split(bytes: &[u8]) -> Parts {
        let header = Header::decode(bytes).unwrap();
        let prelude_end = HEADER_LEN + header.prelude_len as usize;
        let manifest_end = header.head_len() as usize;
        Parts {
            prelude: bytes[HEADER_LEN..prelude_end].to_vec(),
            manifest: bytes[prelude_end..manifest_end].to_vec(),
            data: bytes[manifest_end..].to_vec(),
            header,
        }
    }

    /// Reassembles the file. With `reseal`, lengths, hashes and the header
    /// check are recomputed so only the deliberate inconsistency remains.
    pub fn assemble(&self, reseal: bool) -> Vec<u8> {
        let mut header = self.header.clone();
        if reseal {
            header.prelude_len = self.prelude.len() as u32;
            header.manifest_len = self.manifest.len() as u32;
            header.prelude_sha256 = sha256(&self.prelude);
            header.manifest_sha256 = sha256(&self.manifest);
            header.total_len =
                (HEADER_LEN + self.prelude.len() + self.manifest.len() + self.data.len()) as u64;
        }
        let mut out = header.encode().to_vec();
        out.extend_from_slice(&self.prelude);
        out.extend_from_slice(&self.manifest);
        out.extend_from_slice(&self.data);
        out
    }

    /// The manifest of a clear package as a JSON value.
    pub fn manifest_json(&self) -> serde_json::Value {
        let json = miniz_oxide::inflate::decompress_to_vec(&self.manifest).unwrap();
        serde_json::from_slice(&json).unwrap()
    }

    /// Replaces the manifest of a clear package.
    pub fn set_manifest_json(&mut self, value: &serde_json::Value) {
        self.set_manifest_bytes(serde_json::to_string(value).unwrap().as_bytes());
    }

    pub fn set_manifest_bytes(&mut self, json: &[u8]) {
        self.manifest = miniz_oxide::deflate::compress_to_vec(json, 6);
    }

    pub fn prelude_json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.prelude).unwrap()
    }

    pub fn set_prelude_json(&mut self, value: &serde_json::Value) {
        self.prelude = serde_json::to_vec(value).unwrap();
    }
}

/// Opens `bytes` and fully verifies them; returns the error code on failure.
pub fn open_and_verify(bytes: &[u8], credentials: &Credentials<'_>) -> Result<(), &'static str> {
    let package = Package::open(bytes, credentials).map_err(|e| e.code())?;
    package.verify_all().map_err(|e| e.code())?;
    Ok(())
}

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// A fresh, empty scratch directory unique to `label`.
pub fn scratch_dir(label: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("darm-package-test-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
