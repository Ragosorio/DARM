//! Clear and encrypted round trips through the public API.

mod common;

use std::io::{Read, Seek, SeekFrom};

use common::*;
use darm_package::{
    Compression, CompressionMode, Credentials, EncryptOptions, HEADER_LEN, IR_ENTRY_NAME,
    IR_MEDIA_TYPE, KeyFile, Package, PackageBuilder, PackageError, PackageHead, Role, SlotKind,
    generate_keyfile, probe,
};

#[test]
fn clear_round_trip() {
    let bytes = clear_sample();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    assert!(!package.is_encrypted());
    assert_eq!(package.total_len(), bytes.len() as u64);
    assert_eq!(package.chunk_size(), 64 * 1024);
    assert_eq!(package.card(), Some(&card()));
    assert_eq!(package.prelude().generator, "darm-tests");
    assert_eq!(
        package.manifest().book().unwrap(),
        serde_json::json!({"title": "The Marker Title", "chapters": 3})
    );
    assert_sample_content(&package);

    let ir = package.ir_entry().unwrap();
    assert_eq!(ir.index, 0);
    assert_eq!(ir.name, IR_ENTRY_NAME);
    assert_eq!(ir.media_type, IR_MEDIA_TYPE);
    assert_eq!(ir.role, Role::Ir);

    let report = package.verify_all().unwrap();
    assert_eq!(report.entries, 6);
    // 150 KiB+123 -> 3 chunks, 200 KiB+7 -> 4, empty -> 0, 64 KiB -> 1, tiny -> 1, IR -> 1.
    assert_eq!(report.chunks, 10);
    let plain: u64 = package.entries().iter().map(|e| e.size).sum();
    assert_eq!(report.bytes, plain);

    // Credentials are simply ignored for a clear package.
    let again = Package::open(bytes.as_slice(), &Credentials::Password("whatever")).unwrap();
    assert_sample_content(&again);
}

#[test]
fn compression_is_per_entry_and_only_for_text_that_shrinks() {
    let bytes = clear_sample();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    let compression = |name: &str| package.entry_by_name(name).unwrap().compression;
    assert_eq!(compression("media/clip.bin"), Compression::None);
    assert_eq!(compression("media/one-chunk.png"), Compression::None);
    assert_eq!(compression("empty.dat"), Compression::None);
    assert_eq!(compression("text/captions.vtt"), Compression::Deflate);
    // "tiny" does not shrink under deflate, so it is stored verbatim even though it is text.
    assert_eq!(compression("notes/small.txt"), Compression::None);
    // The sample IR is too short to shrink; a realistic one is deflated.
    assert_eq!(compression(IR_ENTRY_NAME), Compression::None);
    let mut builder = PackageBuilder::new();
    let big_ir = format!(
        "[{}]",
        vec![r#"{"type":"paragraph","text":"lorem ipsum"}"#; 500].join(",")
    );
    builder.ir(big_ir.clone().into_bytes()).unwrap();
    let with_ir = builder.to_vec().unwrap();
    let with_ir = Package::open(with_ir.as_slice(), &Credentials::None).unwrap();
    assert_eq!(
        with_ir.ir_entry().unwrap().compression,
        Compression::Deflate
    );
    assert_eq!(with_ir.ir_bytes().unwrap(), big_ir.as_bytes());

    let captions = package.entry_by_name("text/captions.vtt").unwrap();
    assert!(captions.stored_size < captions.size / 20);
    assert_eq!(captions.chunks.len(), 4);

    // Text that does not compress stays raw.
    let mut builder = PackageBuilder::new();
    builder
        .add_bytes("random.txt", "text/plain", noise(5, 100_000))
        .unwrap();
    let bytes = builder.to_vec().unwrap();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    assert_eq!(package.entries()[0].compression, Compression::None);

    // CompressionMode::None disables deflate entirely.
    let mut builder = sample_builder();
    builder.compression(CompressionMode::None);
    let bytes = builder.to_vec().unwrap();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    assert!(
        package
            .entries()
            .iter()
            .all(|e| e.compression == Compression::None)
    );
    assert_sample_content(&package);
}

#[test]
fn uncompressed_entries_of_clear_packages_are_stored_verbatim_and_contiguous() {
    let bytes = clear_sample();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    for (name, _, content) in sample_entries() {
        let entry = package.entry_by_name(name).unwrap();
        let range = package.direct_range(entry.index).unwrap();
        if entry.compression == Compression::None {
            let (offset, len) = range.unwrap();
            assert_eq!(len, content.len() as u64);
            assert_eq!(offset, package.head_len() + entry.offset);
            assert_eq!(
                &bytes[offset as usize..(offset + len) as usize],
                content.as_slice(),
                "{name}"
            );
            assert_eq!(entry.stored_size, entry.size);
        } else {
            assert_eq!(range, None);
        }
    }
    // Entries tile the data area exactly.
    let mut cursor = 0;
    for entry in package.entries() {
        assert_eq!(entry.offset, cursor);
        cursor += entry.stored_size;
    }
    assert_eq!(package.head_len() + cursor, bytes.len() as u64);
}

#[test]
fn five_mebibyte_entry_with_one_mebibyte_chunks() {
    let big = noise(77, 5 * MIB);
    let text = prose(3 * MIB + 17);
    for encrypted in [false, true] {
        let mut builder = PackageBuilder::new();
        builder
            .add_bytes("big.bin", "application/octet-stream", big.clone())
            .unwrap()
            .add_bytes("big.txt", "text/plain", text.clone())
            .unwrap();
        if encrypted {
            builder.encrypt(fast_encryption().password(PASSWORD));
        }
        let bytes = builder.to_vec().unwrap();
        let credentials = if encrypted {
            Credentials::Password(PASSWORD)
        } else {
            Credentials::None
        };
        let package = Package::open(bytes.as_slice(), &credentials).unwrap();
        assert_eq!(package.chunk_size(), 1024 * 1024);
        let entry = package.entry_by_name("big.bin").unwrap();
        assert_eq!(entry.chunks.len(), 5);
        let tag = if encrypted { 16 } else { 0 };
        for chunk in 0..5 {
            let (offset, len) = package.chunk_location(entry.index, chunk).unwrap();
            assert_eq!(len as usize, MIB + tag);
            assert_eq!(
                offset,
                package.head_len() + u64::from(chunk) * (MIB + tag) as u64
            );
            let plain = package.read_chunk(entry.index, chunk).unwrap();
            assert_eq!(plain, big[chunk as usize * MIB..(chunk as usize + 1) * MIB]);
        }
        assert_eq!(package.read_entry(entry.index, u64::MAX).unwrap(), big);
        let text_entry = package.entry_by_name("big.txt").unwrap();
        assert_eq!(text_entry.compression, Compression::Deflate);
        assert_eq!(text_entry.chunks.len(), 4);
        assert_eq!(
            package.read_entry(text_entry.index, u64::MAX).unwrap(),
            text
        );
        assert_eq!(package.verify_all().unwrap().chunks, 9);
    }
}

#[test]
fn encrypted_round_trip_with_password_and_keyfile() {
    let bytes = password_sample();

    let by_password = Package::open(bytes.as_slice(), &Credentials::Password(PASSWORD)).unwrap();
    assert!(by_password.is_encrypted());
    assert_sample_content(&by_password);
    assert_eq!(by_password.verify_all().unwrap().entries, 6);
    // The card was not exposed.
    assert_eq!(by_password.card(), None);

    let by_key = Package::open(bytes.as_slice(), &Credentials::Key(&KEY)).unwrap();
    assert_sample_content(&by_key);
    assert_eq!(by_key.uuid(), by_password.uuid());

    // Stored chunks carry a 16-byte tag and no per-chunk hash.
    let clip = by_key.entry_by_name("media/clip.bin").unwrap();
    assert_eq!(clip.stored_size, clip.size + 3 * 16);
    assert!(!clip.chunks.has_hashes());
    assert_eq!(by_key.direct_range(clip.index).unwrap(), None);
}

#[test]
fn keyfile_only_package_and_generated_keyfile() {
    let keyfile = generate_keyfile().unwrap();
    let text = keyfile.to_text();
    let mut builder = sample_builder();
    builder.encrypt(EncryptOptions::new().keyfile_key(keyfile.key()));
    let bytes = builder.to_vec().unwrap();

    let parsed = KeyFile::parse(&text).unwrap();
    let package = Package::open(bytes.as_slice(), &Credentials::Key(parsed.key())).unwrap();
    assert_sample_content(&package);

    let info = probe(&bytes).unwrap();
    assert_eq!(info.slots.len(), 1);
    assert_eq!(info.slots[0].kind, SlotKind::Keyfile);
    assert_eq!(
        info.slots[0].key_id.as_deref(),
        Some(keyfile.key_id().as_str())
    );
    assert_eq!(info.kdf, None);

    // A password cannot open a keyfile-only package, nor can another key.
    assert_eq!(
        Package::open(bytes.as_slice(), &Credentials::Password(PASSWORD)).unwrap_err(),
        PackageError::WrongCredentials
    );
    assert_eq!(
        Package::open(bytes.as_slice(), &Credentials::Key(&KEY)).unwrap_err(),
        PackageError::WrongCredentials
    );
}

#[test]
fn wrong_credentials_and_missing_credentials() {
    let bytes = password_sample();
    let wrong = Package::open(bytes.as_slice(), &Credentials::Password("not the password"));
    let error = wrong.unwrap_err();
    assert_eq!(error, PackageError::WrongCredentials);
    assert_eq!(error.code(), "E_PKG_WRONG_CREDENTIALS");
    assert!(error.to_string().contains("does not unlock"));

    let mut other_key = KEY;
    other_key[0] ^= 1;
    assert_eq!(
        Package::open(bytes.as_slice(), &Credentials::Key(&other_key)).unwrap_err(),
        PackageError::WrongCredentials
    );

    let locked = Package::open(bytes.as_slice(), &Credentials::None).unwrap_err();
    assert_eq!(locked.code(), "E_PKG_CREDENTIALS_REQUIRED");
    assert_eq!(locked, PackageError::CredentialsRequired { card: None });
}

#[test]
fn exposed_card_is_available_on_a_locked_package() {
    let mut builder = sample_builder();
    builder.encrypt(fast_encryption().password(PASSWORD).expose_card(true));
    let bytes = builder.to_vec().unwrap();

    match Package::open(bytes.as_slice(), &Credentials::None).unwrap_err() {
        PackageError::CredentialsRequired { card: Some(found) } => assert_eq!(*found, card()),
        other => panic!("unexpected error {other:?}"),
    }
    let info = probe(&bytes).unwrap();
    assert!(info.encrypted);
    assert!(info.prelude_loaded);
    assert_eq!(info.card, Some(card()));
    assert_eq!(info.slots.len(), 1);
    assert_eq!(info.slots[0].kind, SlotKind::Password);
    assert_eq!(info.kdf, Some(FAST_KDF));

    let package = Package::open(bytes.as_slice(), &Credentials::Password(PASSWORD)).unwrap();
    assert_eq!(package.card(), Some(&card()));
}

#[test]
fn probe_reports_what_is_needed_to_open() {
    let bytes = clear_sample();
    let header_only = probe(&bytes[..HEADER_LEN]).unwrap();
    assert_eq!(header_only.version(), "1.0");
    assert!(!header_only.encrypted);
    assert!(!header_only.prelude_loaded);
    assert_eq!(header_only.card, None);
    assert_eq!(header_only.total_len, bytes.len() as u64);
    assert!(header_only.head_len > HEADER_LEN as u64);
    assert!(header_only.head_len < bytes.len() as u64);

    let full = probe(&bytes).unwrap();
    assert!(full.prelude_loaded);
    assert_eq!(full.card, Some(card()));
    assert_eq!(full.chunk_size, Some(64 * 1024));
    assert_eq!(full.generator.as_deref(), Some("darm-tests"));
    assert_eq!(full.uuid, header_only.uuid);
    assert_eq!(full.uuid_string().len(), 36);

    assert_eq!(probe(&bytes[..100]).unwrap_err().code(), "E_PKG_TRUNCATED");
    assert_eq!(probe(b"GIF89a..").unwrap_err().code(), "E_PKG_BAD_MAGIC");
}

#[test]
fn head_only_open_decodes_chunks_fed_by_the_caller() {
    for (bytes, credentials) in [
        (clear_sample(), Credentials::None),
        (key_sample(), Credentials::Key(&KEY)),
    ] {
        let head_len = probe(&bytes[..HEADER_LEN]).unwrap().head_len as usize;
        let head = PackageHead::open(&bytes[..head_len], &credentials).unwrap();
        assert_eq!(head.total_len(), bytes.len() as u64);
        assert!(!head.manifest_json().unwrap().is_empty());

        // One byte short of the head is a truncation error.
        assert_eq!(
            PackageHead::open(&bytes[..head_len - 1], &credentials)
                .unwrap_err()
                .code(),
            "E_PKG_TRUNCATED"
        );

        for (name, _, content) in sample_entries() {
            let entry = head.entry_by_name(name).unwrap();
            let mut plain = Vec::new();
            for chunk in 0..head.chunk_count(entry.index).unwrap() {
                let (offset, len) = head.chunk_location(entry.index, chunk).unwrap();
                let stored = &bytes[offset as usize..offset as usize + len as usize];
                plain.extend(head.decode_chunk(entry.index, chunk, stored).unwrap());
            }
            assert_eq!(plain, content, "{name}");
        }

        // Out-of-range lookups are errors, not panics.
        assert_eq!(
            head.chunk_location(999, 0).unwrap_err().code(),
            "E_PKG_NOT_FOUND"
        );
        assert_eq!(
            head.chunk_location(0, 999).unwrap_err().code(),
            "E_PKG_NOT_FOUND"
        );
        assert_eq!(
            head.decode_chunk(0, 999, b"").unwrap_err().code(),
            "E_PKG_NOT_FOUND"
        );
        assert_eq!(
            head.decode_chunk(0, 0, b"short").unwrap_err().code(),
            "E_PKG_INTEGRITY"
        );
        assert!(head.entry_by_name("nope").is_none());
    }
}

#[test]
fn read_entry_honours_the_size_cap() {
    let bytes = clear_sample();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    let clip = package.entry_by_name("media/clip.bin").unwrap();
    let error = package.read_entry(clip.index, clip.size - 1).unwrap_err();
    assert_eq!(error.code(), "E_PKG_LIMIT");
    assert!(package.read_entry(clip.index, clip.size).is_ok());
    assert_eq!(
        package
            .read_entry_by_name("nope", u64::MAX)
            .unwrap_err()
            .code(),
        "E_PKG_NOT_FOUND"
    );
    assert_eq!(
        package.read_entry(999, u64::MAX).unwrap_err().code(),
        "E_PKG_NOT_FOUND"
    );
}

#[test]
fn entry_reader_streams_and_seeks() {
    for (bytes, credentials) in [
        (clear_sample(), Credentials::None),
        (key_sample(), Credentials::Key(&KEY)),
    ] {
        let package = Package::open(bytes.as_slice(), &credentials).unwrap();
        for (name, _, content) in sample_entries() {
            let entry = package.entry_by_name(name).unwrap();
            let mut reader = package.entry_reader(entry.index).unwrap();
            assert_eq!(reader.size(), content.len() as u64);

            let mut all = Vec::new();
            reader.read_to_end(&mut all).unwrap();
            assert_eq!(all, content, "{name}");
            assert_eq!(reader.read(&mut [0u8; 8]).unwrap(), 0);

            if content.len() > 70_000 {
                // A read that straddles a chunk boundary.
                reader.seek(SeekFrom::Start(65_530)).unwrap();
                let mut window = [0u8; 20];
                reader.read_exact(&mut window).unwrap();
                assert_eq!(window, content[65_530..65_550]);

                reader.seek(SeekFrom::End(-5)).unwrap();
                let mut tail = Vec::new();
                reader.read_to_end(&mut tail).unwrap();
                assert_eq!(tail, content[content.len() - 5..]);

                reader.seek(SeekFrom::Start(10)).unwrap();
                assert_eq!(reader.seek(SeekFrom::Current(5)).unwrap(), 15);
                let mut one = [0u8; 1];
                reader.read_exact(&mut one).unwrap();
                assert_eq!(one[0], content[15]);
            }
            // Seeking past the end reads nothing; seeking before the start fails.
            reader.seek(SeekFrom::End(10)).unwrap();
            assert_eq!(reader.read(&mut [0u8; 8]).unwrap(), 0);
            assert!(reader.seek(SeekFrom::Current(-1_000_000_000)).is_err());
        }
    }
}

#[test]
fn files_are_streamed_from_disk_and_written_atomically() {
    let dir = scratch_dir("files");
    let video = noise(3, 300 * KIB + 5);
    let captions = prose(90 * KIB);
    std::fs::write(dir.join("video.mp4"), &video).unwrap();
    std::fs::write(dir.join("captions.vtt"), &captions).unwrap();
    std::fs::write(dir.join("empty"), b"").unwrap();

    for encrypted in [false, true] {
        let mut builder = PackageBuilder::new();
        builder
            .chunk_size(64 * 1024)
            .ir(IR.to_vec())
            .unwrap()
            .add_file("media/video.mp4", "video/mp4", dir.join("video.mp4"))
            .unwrap()
            .add_file("media/captions.vtt", "text/vtt", dir.join("captions.vtt"))
            .unwrap()
            .add_file("media/empty", "application/octet-stream", dir.join("empty"))
            .unwrap();
        if encrypted {
            builder.encrypt(fast_encryption().keyfile_key(&KEY));
        }
        let out = dir.join(if encrypted { "enc.darm" } else { "clear.darm" });
        std::fs::write(&out, b"previous content").unwrap();
        let summary = builder.write_to_path(&out).unwrap();
        assert_eq!(summary.total_len, std::fs::metadata(&out).unwrap().len());
        assert_eq!(summary.entry_count, 4);
        assert_eq!(summary.compressed_entries, 1);
        assert_eq!(summary.encrypted, encrypted);
        assert_eq!(
            summary.plain_len,
            (IR.len() + video.len() + captions.len()) as u64
        );
        assert_eq!(summary.head_len + summary.data_len, summary.total_len);

        let credentials = if encrypted {
            Credentials::Key(&KEY)
        } else {
            Credentials::None
        };
        let package = Package::open_path(&out, &credentials).unwrap();
        assert_eq!(*package.uuid(), summary.uuid);
        assert_eq!(
            package
                .read_entry_by_name("media/video.mp4", u64::MAX)
                .unwrap(),
            video
        );
        assert_eq!(
            package
                .read_entry_by_name("media/captions.vtt", u64::MAX)
                .unwrap(),
            captions
        );
        assert_eq!(
            package.read_entry_by_name("media/empty", u64::MAX).unwrap(),
            b""
        );
        package.verify_all().unwrap();

        let mut reader = package
            .entry_reader(package.entry_by_name("media/video.mp4").unwrap().index)
            .unwrap();
        reader.seek(SeekFrom::Start(200_000)).unwrap();
        let mut window = vec![0u8; 1000];
        reader.read_exact(&mut window).unwrap();
        assert_eq!(window, video[200_000..201_000]);
    }

    // A failed build leaves the destination untouched and no temp file behind.
    let target = dir.join("untouched.darm");
    std::fs::write(&target, b"keep me").unwrap();
    let mut builder = PackageBuilder::new();
    builder
        .add_file(
            "gone.bin",
            "application/octet-stream",
            dir.join("does-not-exist"),
        )
        .unwrap();
    let error = builder.write_to_path(&target).unwrap_err();
    assert_eq!(error.code(), "E_PKG_IO");
    assert!(error.to_string().contains("does-not-exist"));
    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn clear_builds_are_deterministic_and_encrypted_builds_are_not() {
    let first = clear_sample();
    let second = clear_sample();
    assert_eq!(first, second);

    // File-backed and memory-backed inputs give the same bytes too.
    let dir = scratch_dir("determinism");
    let mut from_files = PackageBuilder::new();
    from_files
        .generator("darm-tests")
        .chunk_size(64 * 1024)
        .card(card())
        .book_summary(serde_json::json!({"title": "The Marker Title", "chapters": 3}))
        .unwrap()
        .ir(IR.to_vec())
        .unwrap();
    for (index, (name, media_type, content)) in sample_entries().into_iter().enumerate() {
        let path = dir.join(format!("input-{index}"));
        std::fs::write(&path, content).unwrap();
        from_files.add_file(name, media_type, path).unwrap();
    }
    assert_eq!(from_files.to_vec().unwrap(), first);
    std::fs::remove_dir_all(&dir).unwrap();

    // Any change of input changes the UUID.
    let mut changed = sample_builder();
    changed.created("2026-11-16T00:00:00Z");
    let changed = changed.to_vec().unwrap();
    assert_ne!(probe(&changed).unwrap().uuid, probe(&first).unwrap().uuid);
    let package = Package::open(changed.as_slice(), &Credentials::None).unwrap();
    assert_eq!(
        package.manifest().created.as_deref(),
        Some("2026-11-16T00:00:00Z")
    );
    // Clear UUIDs are version 8, RFC 9562 variant.
    assert_eq!(package.uuid()[6] >> 4, 8);
    assert_eq!(package.uuid()[8] >> 6, 0b10);

    let a = key_sample();
    let b = key_sample();
    assert_ne!(a, b);
    assert_ne!(probe(&a).unwrap().uuid, probe(&b).unwrap().uuid);
    assert_eq!(probe(&a).unwrap().uuid[6] >> 4, 4);
}

#[test]
fn empty_package_and_package_without_ir() {
    let bytes = PackageBuilder::new().to_vec().unwrap();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    assert!(package.entries().is_empty());
    assert_eq!(package.total_len(), package.head_len());
    assert_eq!(package.ir_bytes().unwrap_err().code(), "E_PKG_NOT_FOUND");
    assert_eq!(package.manifest().book_json(), "{}");
    assert_eq!(package.verify_all().unwrap().entries, 0);

    let mut builder = PackageBuilder::new();
    builder.encrypt(fast_encryption().keyfile_key(&KEY));
    let bytes = builder.to_vec().unwrap();
    let package = Package::open(bytes.as_slice(), &Credentials::Key(&KEY)).unwrap();
    assert!(package.entries().is_empty());
}

#[test]
fn eight_key_slots_all_unlock() {
    let keys: Vec<[u8; 32]> = (0..7u8).map(|i| [i + 1; 32]).collect();
    let mut options = fast_encryption().password(PASSWORD);
    for key in &keys {
        options = options.keyfile_key(key);
    }
    let mut builder = PackageBuilder::new();
    builder.ir(IR.to_vec()).unwrap().encrypt(options);
    let bytes = builder.to_vec().unwrap();
    assert_eq!(probe(&bytes).unwrap().slots.len(), 8);
    for key in &keys {
        let package = Package::open(bytes.as_slice(), &Credentials::Key(key)).unwrap();
        assert_eq!(package.ir_bytes().unwrap(), IR);
    }
    let package = Package::open(bytes.as_slice(), &Credentials::Password(PASSWORD)).unwrap();
    assert_eq!(package.ir_bytes().unwrap(), IR);
}

#[test]
fn packages_can_be_shared_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PackageHead>();
    assert_send_sync::<Package<std::fs::File>>();
    assert_send_sync::<Package<Vec<u8>>>();
    assert_send_sync::<PackageBuilder>();
    assert_send_sync::<PackageError>();

    let package =
        std::sync::Arc::new(Package::open(key_sample(), &Credentials::Key(&KEY)).unwrap());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let package = std::sync::Arc::clone(&package);
            std::thread::spawn(move || package.verify_all().unwrap().chunks)
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), 10);
    }
}
