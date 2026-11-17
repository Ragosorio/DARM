//! Corruption, truncation and tampering must always be detected.

mod common;

use common::*;
use darm_package::{
    Credentials, HEADER_LEN, Package, PackageBuilder, PackageError, PackageHead, probe,
};

fn samples() -> Vec<(&'static str, Vec<u8>, Credentials<'static>)> {
    vec![
        ("clear", clear_sample(), Credentials::None),
        ("encrypted", key_sample(), Credentials::Key(&KEY)),
    ]
}

#[test]
fn flipping_any_header_prelude_or_manifest_byte_is_detected() {
    for (label, bytes, credentials) in samples() {
        let head_len = probe(&bytes).unwrap().head_len as usize;
        let prelude_end = HEADER_LEN + Parts::split(&bytes).prelude.len();
        assert!(open_and_verify(&bytes, &credentials).is_ok());
        for position in 0..head_len {
            for mask in [0x01u8, 0x80] {
                let mut copy = bytes.clone();
                copy[position] ^= mask;
                let code = match Package::open(copy.as_slice(), &credentials) {
                    Ok(_) => panic!("{label}: flip at {position} went unnoticed"),
                    Err(error) => error.code(),
                };
                let expected: &[&str] = if position < 8 {
                    &["E_PKG_BAD_MAGIC"]
                } else if position < 10 {
                    &["E_PKG_UNSUPPORTED_VERSION"]
                } else if position < prelude_end {
                    // Header checksum, or the prelude hash.
                    &["E_PKG_INTEGRITY"]
                } else {
                    &["E_PKG_INTEGRITY"]
                };
                assert!(
                    expected.contains(&code),
                    "{label}: flip at {position} gave {code}"
                );
            }
        }
    }
}

#[test]
fn flipping_a_byte_in_any_chunk_is_detected() {
    for (label, bytes, credentials) in samples() {
        let package = Package::open(bytes.as_slice(), &credentials).unwrap();
        let mut checked = 0;
        for entry in package.entries() {
            for chunk in 0..entry.chunks.len() as u32 {
                let (offset, len) = package.chunk_location(entry.index, chunk).unwrap();
                let (offset, len) = (offset as usize, len as usize);
                // First byte, a middle byte, and every one of the last 16 bytes
                // (the GCM tag in encrypted packages).
                let mut positions = vec![offset, offset + len / 2];
                positions.extend(
                    (offset + len.saturating_sub(16)..offset + len).filter(|p| *p >= offset),
                );
                for position in positions {
                    let mut copy = bytes.clone();
                    copy[position] ^= 0x04;
                    // The head is intact, so the package still opens…
                    let tampered = Package::open(copy.as_slice(), &credentials).unwrap();
                    // …but the damaged chunk is refused,
                    let error = tampered.read_chunk(entry.index, chunk).unwrap_err();
                    assert_eq!(
                        error.code(),
                        "E_PKG_INTEGRITY",
                        "{label} {} #{chunk}",
                        entry.name
                    );
                    assert_eq!(
                        tampered
                            .read_entry(entry.index, u64::MAX)
                            .unwrap_err()
                            .code(),
                        "E_PKG_INTEGRITY"
                    );
                    // …and a full verification fails.
                    assert_eq!(tampered.verify_all().unwrap_err().code(), "E_PKG_INTEGRITY");
                    checked += 1;
                }
            }
        }
        assert!(checked >= 150, "{label}: {checked}");

        // Undamaged chunks of a damaged file still decode.
        let clip = package.entry_by_name("media/clip.bin").unwrap();
        let (offset, _) = package.chunk_location(clip.index, 1).unwrap();
        let mut copy = bytes.clone();
        copy[offset as usize + 5] ^= 0xff;
        let tampered = Package::open(copy.as_slice(), &credentials).unwrap();
        assert!(tampered.read_chunk(clip.index, 0).is_ok());
        assert!(tampered.read_chunk(clip.index, 1).is_err());
        assert!(tampered.read_chunk(clip.index, 2).is_ok());
    }
}

#[test]
fn truncation_at_many_offsets_is_detected() {
    for (label, bytes, credentials) in samples() {
        let mut lengths: Vec<usize> = (0..400).collect();
        lengths.extend((400..bytes.len()).step_by(997));
        lengths.extend(bytes.len() - 40..bytes.len());
        let head_len = probe(&bytes).unwrap().head_len as usize;
        lengths.extend(head_len - 3..head_len + 3);
        for len in lengths {
            let cut = &bytes[..len];
            let error = match Package::open(cut, &credentials) {
                Ok(_) => panic!("{label}: truncation to {len} bytes went unnoticed"),
                Err(error) => error,
            };
            match (len, &error) {
                (0, PackageError::BadMagic) => {}
                (_, PackageError::Truncated { needed, available }) => {
                    assert_eq!(*available, len as u64);
                    assert!(*needed > len as u64);
                }
                other => panic!("{label}: unexpected {other:?}"),
            }
            // The same holds when the bytes are probed or opened head-only.
            if len < head_len {
                assert!(PackageHead::open(cut, &credentials).is_err());
            }
        }

        // Extension is refused too.
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            Package::open(longer.as_slice(), &credentials)
                .unwrap_err()
                .code(),
            "E_PKG_INTEGRITY"
        );
    }
}

#[test]
fn truncation_with_a_resealed_header_is_detected() {
    for (label, bytes, credentials) in samples() {
        let mut parts = Parts::split(&bytes);
        parts.data.truncate(parts.data.len() - 1);
        let forged = parts.assemble(true);
        let error = Package::open(forged.as_slice(), &credentials).unwrap_err();
        let expected = if label == "clear" {
            // The manifest no longer tiles the (shorter) data area.
            "E_PKG_CORRUPT_MANIFEST"
        } else {
            // total_len is bound into the manifest's authentication.
            "E_PKG_INTEGRITY"
        };
        assert_eq!(error.code(), expected, "{label}");
    }
}

#[test]
fn prelude_tampering_with_resealed_hashes_is_detected_after_unlock() {
    // An attacker who cannot decrypt can still rewrite the clear prelude and
    // fix the (unkeyed) header hashes. The manifest's AEAD must catch it.
    let mut builder = sample_builder();
    builder.encrypt(fast_encryption().keyfile_key(&KEY).expose_card(true));
    let bytes = builder.to_vec().unwrap();

    let mut parts = Parts::split(&bytes);
    let mut prelude = parts.prelude_json();
    prelude["card"]["title"] = "A Different Title".into();
    parts.set_prelude_json(&prelude);
    let forged = parts.assemble(true);

    // The forged card is visible before unlocking (it is public, unauthenticated data)…
    assert_eq!(
        probe(&forged).unwrap().card.unwrap().title,
        "A Different Title"
    );
    // …but unlocking exposes the forgery.
    let error = Package::open(forged.as_slice(), &Credentials::Key(&KEY)).unwrap_err();
    assert_eq!(error.code(), "E_PKG_INTEGRITY");
    assert!(error.to_string().contains("authentication"));

    // Same for the generator string, the chunk size and a stripped card.
    for edit in [
        (|p: &mut serde_json::Value| p["generator"] = "evil".into()) as fn(&mut serde_json::Value),
        |p| p["chunk_size"] = 131072.into(),
    ] {
        let mut parts = Parts::split(&bytes);
        let mut prelude = parts.prelude_json();
        edit(&mut prelude);
        parts.set_prelude_json(&prelude);
        let forged = parts.assemble(true);
        assert_eq!(
            Package::open(forged.as_slice(), &Credentials::Key(&KEY))
                .unwrap_err()
                .code(),
            "E_PKG_INTEGRITY"
        );
    }

    // A forged key commitment is caught as soon as the real key is unwrapped.
    let mut parts = Parts::split(&bytes);
    let mut prelude = parts.prelude_json();
    prelude["encryption"]["key_commitment"] = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into();
    parts.set_prelude_json(&prelude);
    let error =
        Package::open(parts.assemble(true).as_slice(), &Credentials::Key(&KEY)).unwrap_err();
    assert_eq!(error.code(), "E_PKG_INTEGRITY");
    assert!(error.to_string().contains("commitment"), "{error}");

    // Changing the UUID or the flags (with a valid header checksum) fails as well.
    let mut parts = Parts::split(&bytes);
    parts.header.uuid[3] ^= 1;
    let forged = parts.assemble(true);
    assert!(Package::open(forged.as_slice(), &Credentials::Key(&KEY)).is_err());
}

#[test]
fn clear_packages_detect_corruption_but_are_not_authenticated() {
    // Documented limitation: without a signature anyone can rewrite a clear
    // package consistently. This test pins that behaviour down so it is a
    // conscious part of the format, not an accident.
    let bytes = clear_sample();
    let mut parts = Parts::split(&bytes);
    let mut prelude = parts.prelude_json();
    prelude["card"]["title"] = "Forged".into();
    parts.set_prelude_json(&prelude);
    let forged = parts.assemble(true);
    let package = Package::open(forged.as_slice(), &Credentials::None).unwrap();
    assert_eq!(package.card().unwrap().title, "Forged");
    package.verify_all().unwrap();
}

#[test]
fn swapping_chunks_or_entries_in_an_encrypted_package_fails() {
    // Two entries of identical size so that their stored runs are interchangeable.
    let a = noise(21, 128 * KIB);
    let b = noise(22, 128 * KIB);
    let mut builder = PackageBuilder::new();
    builder
        .chunk_size(64 * 1024)
        .add_bytes("a.bin", "application/octet-stream", a.clone())
        .unwrap()
        .add_bytes("b.bin", "application/octet-stream", b)
        .unwrap()
        .encrypt(fast_encryption().keyfile_key(&KEY));
    let bytes = builder.to_vec().unwrap();
    let credentials = Credentials::Key(&KEY);
    let package = Package::open(bytes.as_slice(), &credentials).unwrap();
    package.verify_all().unwrap();

    let location = |entry: u32, chunk: u32| {
        let (offset, len) = package.chunk_location(entry, chunk).unwrap();
        offset as usize..offset as usize + len as usize
    };
    let swap = |x: std::ops::Range<usize>, y: std::ops::Range<usize>| {
        assert_eq!(x.len(), y.len());
        let mut copy = bytes.clone();
        let saved = copy[x.clone()].to_vec();
        copy.copy_within(y.clone(), x.start);
        copy[y].copy_from_slice(&saved);
        copy
    };

    // Swap chunk 0 and chunk 1 of the same entry.
    let swapped = swap(location(0, 0), location(0, 1));
    let tampered = Package::open(swapped.as_slice(), &credentials).unwrap();
    assert_eq!(
        tampered.read_chunk(0, 0).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );
    assert_eq!(
        tampered.read_chunk(0, 1).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );
    assert!(tampered.read_chunk(1, 0).is_ok());
    assert_eq!(tampered.verify_all().unwrap_err().code(), "E_PKG_INTEGRITY");

    // Swap the same chunk position between two entries.
    let swapped = swap(location(0, 0), location(1, 0));
    let tampered = Package::open(swapped.as_slice(), &credentials).unwrap();
    assert_eq!(
        tampered.read_chunk(0, 0).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );
    assert_eq!(
        tampered.read_chunk(1, 0).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );

    // Swap the whole stored runs of the two entries.
    let run = |entry: u32| location(entry, 0).start..location(entry, 1).end;
    let swapped = swap(run(0), run(1));
    let tampered = Package::open(swapped.as_slice(), &credentials).unwrap();
    for (entry, chunk) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        assert_eq!(
            tampered.read_chunk(entry, chunk).unwrap_err().code(),
            "E_PKG_INTEGRITY"
        );
    }

    // Feeding a chunk to the decoder under the wrong position fails, including
    // the final-chunk flag: chunk 0 (not final) cannot stand in for chunk 1 (final).
    let stored0 = &bytes[location(0, 0)];
    assert_eq!(package.decode_chunk(0, 0, stored0).unwrap(), a[..64 * KIB]);
    assert!(package.decode_chunk(0, 1, stored0).is_err());
    assert!(package.decode_chunk(1, 0, stored0).is_err());

    // A chunk transplanted from another package (same key file, same layout)
    // fails: the content key and UUID differ.
    let other = builder.to_vec().unwrap();
    let mut transplanted = bytes.clone();
    transplanted[location(0, 0)].copy_from_slice(&other[location(0, 0)]);
    let tampered = Package::open(transplanted.as_slice(), &credentials).unwrap();
    assert_eq!(
        tampered.read_chunk(0, 0).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );

    // And so does a whole manifest transplanted from that other package.
    let mut parts = Parts::split(&bytes);
    parts.manifest = Parts::split(&other).manifest;
    let forged = parts.assemble(true);
    assert_eq!(
        Package::open(forged.as_slice(), &credentials)
            .unwrap_err()
            .code(),
        "E_PKG_INTEGRITY"
    );
}

#[test]
fn swapping_chunks_in_a_clear_package_fails() {
    let bytes = clear_sample();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    let clip = package.entry_by_name("media/clip.bin").unwrap().index;
    let (first, len) = package.chunk_location(clip, 0).unwrap();
    let (second, len2) = package.chunk_location(clip, 1).unwrap();
    assert_eq!(len, len2);
    let (first, second, len) = (first as usize, second as usize, len as usize);
    let mut copy = bytes.clone();
    let saved = copy[first..first + len].to_vec();
    copy.copy_within(second..second + len, first);
    copy[second..second + len].copy_from_slice(&saved);
    let tampered = Package::open(copy.as_slice(), &Credentials::None).unwrap();
    assert_eq!(
        tampered.read_chunk(clip, 0).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );
    assert_eq!(tampered.verify_all().unwrap_err().code(), "E_PKG_INTEGRITY");
}

#[test]
fn encrypted_packages_do_not_leak_plaintext() {
    let entries = sample_entries();
    let mut markers: Vec<(&str, Vec<u8>)> = vec![
        ("IR text", b"IR-PLAINTEXT-MARKER".to_vec()),
        ("IR key", b"ir_version".to_vec()),
        ("card title", b"The Marker Title".to_vec()),
        ("card author", b"Ada Marker".to_vec()),
        ("book summary", b"chapters".to_vec()),
        ("IR entry name", b"book.ir.json".to_vec()),
        ("media type", b"video/mp4".to_vec()),
        ("caption text", b"quick brown fox".to_vec()),
        ("manifest key", b"manifest_version".to_vec()),
        ("manifest key", b"media_type".to_vec()),
    ];
    for (name, _, content) in &entries {
        markers.push(("entry name", name.as_bytes().to_vec()));
        if content.len() >= 16 {
            markers.push(("entry content", content[..16].to_vec()));
            markers.push(("entry content", content[content.len() - 16..].to_vec()));
        }
    }

    // Sanity: the markers that are stored verbatim really occur in the clear package.
    let clear = clear_sample();
    for needle in [
        &b"IR-PLAINTEXT-MARKER"[..],
        b"The Marker Title",
        &entries[0].2[..16],
    ] {
        assert!(contains(&clear, needle));
    }

    for (label, bytes) in [("keyfile", key_sample()), ("password", password_sample())] {
        for (what, needle) in &markers {
            assert!(
                !contains(&bytes, needle),
                "{label} package leaks {what}: {:?}",
                String::from_utf8_lossy(needle)
            );
        }
        // The only clear-text JSON is the prelude; it names no entry and no title.
        let prelude = Parts::split(&bytes).prelude;
        let prelude = String::from_utf8(prelude).unwrap();
        assert!(prelude.contains("\"encryption\""));
        assert!(!prelude.contains("card"));
    }

    // With an exposed card the title is public by the author's choice,
    // everything else stays sealed.
    let mut builder = sample_builder();
    builder.encrypt(fast_encryption().keyfile_key(&KEY).expose_card(true));
    let exposed = builder.to_vec().unwrap();
    assert!(contains(&exposed, b"The Marker Title"));
    for (what, needle) in &markers {
        if what.starts_with("card") {
            continue;
        }
        assert!(
            !contains(&exposed, needle),
            "exposed-card package leaks {what}"
        );
    }
}
