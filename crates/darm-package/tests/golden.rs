//! Format-compatibility guard.
//!
//! `tests/fixtures/golden-v1.darm` is a committed clear package. It must keep
//! opening forever (any reader of major version 1 must accept it), and
//! rebuilding it from the same inputs must reproduce it byte for byte — if
//! that second property breaks, the writer's output changed and the change
//! must be a conscious decision (bless new fixtures, never edit the old
//! expectations so that old files stop opening).
//!
//! The encrypted fixtures cannot be reproduced (encryption is randomised),
//! so they only guard the reading side: key derivation, AEAD framing and
//! associated data must stay compatible with files written in the past.
//!
//! Regenerate with:
//! `cargo test -p darm-package --test golden -- --ignored bless_fixtures`

mod common;

use common::*;
use darm_package::{
    Compression, Credentials, EncryptOptions, KdfParams, Package, PackageBuilder, PublicCard,
    SlotKind, probe,
};

const GOLDEN: &str = "golden-v1.darm";
const ENCRYPTED: &str = "encrypted-v1.darm";
const ENCRYPTED_DEFAULT_KDF: &str = "encrypted-default-kdf-v1.darm";

/// Password of both encrypted fixtures.
const FIXTURE_PASSWORD: &str = "golden-password";

/// UUID of the golden fixture (derived from its content).
const GOLDEN_UUID: &str = "7dcd6c88-abac-85ae-81d8-778e6b427bdc";

fn golden_ir() -> Vec<u8> {
    let paragraphs: Vec<String> = (1..=24)
        .map(|n| format!(r#"{{"id":"p{n}","type":"paragraph","text":"Paragraph {n} of the golden fixture."}}"#))
        .collect();
    format!(
        r#"{{"ir_version":1,"title":"The Golden Fixture","nodes":[{}]}}"#,
        paragraphs.join(",")
    )
    .into_bytes()
}

fn golden_entries() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    vec![
        (
            "media/noise.bin",
            "application/octet-stream",
            noise(0xD0A7, 66_000),
        ),
        ("text/captions.vtt", "text/vtt", prose(3_000)),
        ("empty", "application/octet-stream", Vec::new()),
    ]
}

fn golden_card() -> PublicCard {
    PublicCard {
        title: "The Golden Fixture".into(),
        authors: vec!["DARM".into()],
        language: Some("en".into()),
        cover_color: Some("#c9a227".into()),
    }
}

fn golden_builder() -> PackageBuilder {
    let mut builder = PackageBuilder::new();
    builder
        .generator("darm-golden/1")
        .chunk_size(64 * 1024)
        .created("2026-01-01T00:00:00Z")
        .card(golden_card())
        .book_summary(serde_json::json!({
            "title": "The Golden Fixture",
            "format": "darm",
            "chapters": 1
        }))
        .unwrap()
        .ir(golden_ir())
        .unwrap();
    for (name, media_type, content) in golden_entries() {
        builder.add_bytes(name, media_type, content).unwrap();
    }
    builder
}

fn small_builder() -> PackageBuilder {
    let mut builder = PackageBuilder::new();
    builder
        .generator("darm-golden/1")
        .ir(golden_ir())
        .unwrap()
        .add_bytes("text/captions.vtt", "text/vtt", prose(3_000))
        .unwrap();
    builder
}

fn assert_golden_content<S: darm_package::ReadAt>(package: &Package<S>) {
    assert_eq!(package.ir_bytes().unwrap(), golden_ir());
    assert_eq!(package.entries().len(), 4);
    for (name, media_type, content) in golden_entries() {
        let entry = package.entry_by_name(name).unwrap();
        assert_eq!(entry.media_type, media_type);
        assert_eq!(
            package.read_entry(entry.index, u64::MAX).unwrap(),
            content,
            "{name}"
        );
    }
    assert_eq!(
        package.manifest().created.as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
    assert_eq!(
        package.manifest().book().unwrap(),
        serde_json::json!({"title": "The Golden Fixture", "format": "darm", "chapters": 1})
    );
    let report = package.verify_all().unwrap();
    assert_eq!((report.entries, report.chunks), (4, 4));
}

fn read_fixture(name: &str) -> Vec<u8> {
    let path = fixture_path(name);
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; regenerate with \
             `cargo test -p darm-package --test golden -- --ignored bless_fixtures`",
            path.display()
        )
    })
}

#[test]
fn golden_fixture_still_opens() {
    let bytes = read_fixture(GOLDEN);
    let info = probe(&bytes).unwrap();
    assert_eq!(info.version(), "1.0");
    assert!(!info.encrypted);
    assert_eq!(info.total_len, bytes.len() as u64);
    assert_eq!(info.uuid_string(), GOLDEN_UUID);
    assert_eq!(info.card, Some(golden_card()));
    assert_eq!(info.chunk_size, Some(65_536));
    assert_eq!(info.generator.as_deref(), Some("darm-golden/1"));

    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    assert_golden_content(&package);
    let compression: Vec<_> = package
        .entries()
        .iter()
        .map(|e| (e.name.as_str(), e.compression))
        .collect();
    assert_eq!(
        compression,
        [
            ("book.ir.json", Compression::Deflate),
            ("media/noise.bin", Compression::None),
            ("text/captions.vtt", Compression::Deflate),
            ("empty", Compression::None),
        ]
    );

    // The magic and version bytes of the fixture, spelled out.
    assert_eq!(
        bytes[..16],
        [
            0x89, 0x44, 0x41, 0x52, 0x4D, 0x0D, 0x0A, 0x1A, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00,
            0x00, 0x00
        ]
    );
}

#[test]
fn rebuilding_the_golden_inputs_reproduces_the_fixture_byte_for_byte() {
    let fixture = read_fixture(GOLDEN);
    let rebuilt = golden_builder().to_vec().unwrap();
    assert_eq!(
        rebuilt.len(),
        fixture.len(),
        "the writer's output size changed"
    );
    assert!(
        rebuilt == fixture,
        "the writer no longer reproduces the golden fixture; first difference at byte {}",
        rebuilt
            .iter()
            .zip(&fixture)
            .position(|(a, b)| a != b)
            .unwrap_or(0)
    );
}

#[test]
fn encrypted_fixture_still_opens_with_password_and_key() {
    let bytes = read_fixture(ENCRYPTED);
    let info = probe(&bytes).unwrap();
    assert!(info.encrypted);
    assert_eq!(info.card, Some(golden_card()));
    let kinds: Vec<_> = info.slots.iter().map(|slot| slot.kind).collect();
    assert_eq!(kinds, [SlotKind::Password, SlotKind::Keyfile]);
    assert_eq!(info.kdf, Some(KdfParams::MIN));
    assert_eq!(
        info.slots[1].key_id.as_deref(),
        Some(darm_package::key_id(&KEY).as_str())
    );

    for credentials in [
        Credentials::Password(FIXTURE_PASSWORD),
        Credentials::Key(&KEY),
    ] {
        let package = Package::open(bytes.as_slice(), &credentials).unwrap();
        assert_golden_content(&package);
    }
    assert_eq!(
        Package::open(bytes.as_slice(), &Credentials::Password("wrong"))
            .unwrap_err()
            .code(),
        "E_PKG_WRONG_CREDENTIALS"
    );
    assert_eq!(
        Package::open(bytes.as_slice(), &Credentials::None)
            .unwrap_err()
            .code(),
        "E_PKG_CREDENTIALS_REQUIRED"
    );
}

#[test]
fn default_kdf_fixture_still_opens() {
    let bytes = read_fixture(ENCRYPTED_DEFAULT_KDF);
    let info = probe(&bytes).unwrap();
    assert_eq!(info.kdf, Some(KdfParams::DEFAULT));
    assert_eq!(
        info.kdf,
        Some(KdfParams {
            m_kib: 65_536,
            t: 3,
            p: 1
        })
    );
    assert_eq!(info.card, None);
    let package =
        Package::open(bytes.as_slice(), &Credentials::Password(FIXTURE_PASSWORD)).unwrap();
    assert_eq!(package.ir_bytes().unwrap(), golden_ir());
    assert_eq!(
        package
            .read_entry_by_name("text/captions.vtt", u64::MAX)
            .unwrap(),
        prose(3_000)
    );
    package.verify_all().unwrap();
}

/// Rewrites the committed fixtures. Run deliberately; see the module docs.
#[test]
#[ignore = "regenerates the committed fixtures"]
fn bless_fixtures() {
    std::fs::create_dir_all(fixture_path("")).unwrap();
    let golden = golden_builder()
        .write_to_path(fixture_path(GOLDEN))
        .unwrap();
    println!(
        "{GOLDEN}: {} bytes, uuid {}",
        golden.total_len,
        golden.uuid_string()
    );

    let mut builder = golden_builder();
    builder.encrypt(
        EncryptOptions::new()
            .kdf(KdfParams::MIN)
            .password(FIXTURE_PASSWORD)
            .keyfile_key(&KEY)
            .expose_card(true),
    );
    let encrypted = builder.write_to_path(fixture_path(ENCRYPTED)).unwrap();
    println!("{ENCRYPTED}: {} bytes", encrypted.total_len);

    let mut builder = small_builder();
    builder.encrypt(EncryptOptions::new().password(FIXTURE_PASSWORD));
    let default_kdf = builder
        .write_to_path(fixture_path(ENCRYPTED_DEFAULT_KDF))
        .unwrap();
    println!("{ENCRYPTED_DEFAULT_KDF}: {} bytes", default_kdf.total_len);
}
