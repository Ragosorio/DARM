//! Hand-crafted hostile packages: lying sizes, bombs, bad names, bad
//! parameters. Every case must be rejected with a specific error, without
//! panicking and without allocating memory in proportion to the lie.
#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::Write;

use common::*;
use darm_package::{
    Credentials, EncryptOptions, HEADER_LEN, KdfParams, MAX_ENTRIES, MAX_ENTRY_LEN,
    MAX_MANIFEST_INFLATED_LEN, MAX_MANIFEST_STORED_LEN, MAX_TOTAL_LEN, Package, PackageBuilder,
    PackageError, PackageHead, PublicCard, probe,
};
use serde_json::{Value, json};

// ---- allocation tracking -------------------------------------------------

thread_local! {
    /// Largest single allocation requested on this thread since the last reset.
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

struct TrackingAllocator;

fn note(size: usize) {
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
}

// SAFETY: every method forwards to the system allocator unchanged; the only
// addition is recording the requested size in a const-initialised thread-local.
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

/// Runs `f` and returns its result with the largest single allocation it made.
fn largest_allocation<T>(f: impl FnOnce() -> T) -> (T, usize) {
    LARGEST.with(|largest| largest.set(0));
    let result = f();
    (result, LARGEST.with(Cell::get))
}

/// A named JSON edit and the error code it must provoke.
type Case = (&'static str, Box<dyn Fn(&mut Value)>, &'static str);

fn open_code(bytes: &[u8]) -> &'static str {
    match Package::open(bytes, &Credentials::None) {
        Ok(_) => "OK",
        Err(error) => error.code(),
    }
}

/// A clear package with one deflated text entry and one raw entry, 64 KiB chunks.
fn small_package() -> Vec<u8> {
    let mut builder = PackageBuilder::new();
    builder
        .generator("darm-tests")
        .chunk_size(64 * 1024)
        .add_bytes("text/a.txt", "text/plain", prose(50_000))
        .unwrap()
        .add_bytes("media/b.bin", "application/octet-stream", noise(9, 70_000))
        .unwrap();
    builder.to_vec().unwrap()
}

/// Applies `edit` to the manifest JSON of a clear package and reseals it.
fn with_manifest(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut parts = Parts::split(bytes);
    let mut manifest = parts.manifest_json();
    edit(&mut manifest);
    parts.set_manifest_json(&manifest);
    parts.assemble(true)
}

fn with_prelude(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut parts = Parts::split(bytes);
    let mut prelude = parts.prelude_json();
    edit(&mut prelude);
    parts.set_prelude_json(&prelude);
    parts.assemble(true)
}

// ---- lying sizes ---------------------------------------------------------

#[test]
fn lying_total_len_is_rejected() {
    let bytes = small_package();
    let real = bytes.len() as u64;
    for (declared, expected) in [
        (real + 1, "E_PKG_TRUNCATED"),
        (real * 1000, "E_PKG_TRUNCATED"),
        (MAX_TOTAL_LEN, "E_PKG_TRUNCATED"),
        (real - 1, "E_PKG_INTEGRITY"),
        (MAX_TOTAL_LEN + 1, "E_PKG_LIMIT"),
        (u64::MAX, "E_PKG_LIMIT"),
        (1 << 60, "E_PKG_LIMIT"),
        (HEADER_LEN as u64, "E_PKG_CORRUPT_HEADER"),
        (0, "E_PKG_CORRUPT_HEADER"),
    ] {
        let mut parts = Parts::split(&bytes);
        parts.header.total_len = declared;
        let forged = parts.assemble(false);
        let (code, largest) = largest_allocation(|| open_code(&forged));
        assert_eq!(code, expected, "total_len {declared}");
        assert!(
            largest < 64 * KIB,
            "total_len {declared} allocated {largest}"
        );
    }
}

#[test]
fn lying_manifest_and_prelude_lengths_are_rejected_without_allocating() {
    let bytes = small_package();
    let real = bytes.len() as u64;

    // A 16 MiB manifest claimed by a tiny file. total_len is kept truthful,
    // so the header is self-inconsistent…
    let mut parts = Parts::split(&bytes);
    parts.header.manifest_len = MAX_MANIFEST_STORED_LEN as u32;
    let forged = parts.assemble(false);
    let (code, largest) = largest_allocation(|| open_code(&forged));
    assert_eq!(code, "E_PKG_CORRUPT_HEADER");
    assert!(largest < 64 * KIB, "allocated {largest}");

    // …and when total_len is inflated to match, the file is simply too short.
    let mut parts = Parts::split(&bytes);
    parts.header.manifest_len = MAX_MANIFEST_STORED_LEN as u32;
    parts.header.total_len = real + MAX_MANIFEST_STORED_LEN as u64;
    let forged = parts.assemble(false);
    let (code, largest) = largest_allocation(|| open_code(&forged));
    assert_eq!(code, "E_PKG_TRUNCATED");
    assert!(largest < 64 * KIB, "allocated {largest}");
    // The head-only constructor behaves the same.
    let (result, largest) =
        largest_allocation(|| PackageHead::open(&forged, &Credentials::None).map(|_| ()));
    assert_eq!(result.unwrap_err().code(), "E_PKG_TRUNCATED");
    assert!(largest < 64 * KIB, "allocated {largest}");

    // Over-limit lengths.
    for (prelude_len, manifest_len) in [
        (64 * 1024 + 1, 100u32),
        (u32::MAX, 100),
        (100, MAX_MANIFEST_STORED_LEN as u32 + 1),
        (100, u32::MAX),
        (0, 100),
        (100, 0),
    ] {
        let mut parts = Parts::split(&bytes);
        parts.header.prelude_len = prelude_len;
        parts.header.manifest_len = manifest_len;
        parts.header.total_len = MAX_TOTAL_LEN;
        let forged = parts.assemble(false);
        let (code, largest) = largest_allocation(|| open_code(&forged));
        assert_eq!(code, "E_PKG_LIMIT", "{prelude_len}/{manifest_len}");
        assert!(largest < 64 * KIB);
        assert_eq!(probe(&forged).unwrap_err().code(), "E_PKG_LIMIT");
    }

    // A manifest length that is off by a little: the manifest hash catches it.
    let mut parts = Parts::split(&bytes);
    parts.header.manifest_len -= 1;
    assert_eq!(open_code(&parts.assemble(false)), "E_PKG_INTEGRITY");
    let mut parts = Parts::split(&bytes);
    parts.header.prelude_len += 1;
    assert_eq!(open_code(&parts.assemble(false)), "E_PKG_INTEGRITY");
}

#[test]
fn lying_entry_sizes_are_rejected_without_allocating() {
    let bytes = small_package();
    let cases: Vec<Case> = vec![
        (
            "huge size, same chunks",
            Box::new(|m| m["entries"][1]["size"] = json!(MAX_ENTRY_LEN)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "size over the limit",
            Box::new(|m| m["entries"][1]["size"] = json!(MAX_ENTRY_LEN + 1)),
            "E_PKG_LIMIT",
        ),
        (
            "size beyond u64",
            Box::new(|m| m["entries"][1]["size"] = json!(1e30)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "negative size",
            Box::new(|m| m["entries"][1]["size"] = json!(-1)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "size one byte off",
            Box::new(|m| m["entries"][1]["size"] = json!(70_001)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "smaller size",
            Box::new(|m| m["entries"][1]["size"] = json!(10)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "stored_size lies",
            Box::new(|m| m["entries"][1]["stored_size"] = json!(1u64 << 40)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "offset beyond the file",
            Box::new(|m| m["entries"][1]["offset"] = json!(1u64 << 40)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "overlapping offset",
            Box::new(|m| m["entries"][1]["offset"] = json!(0)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "raw chunk length beyond the file",
            Box::new(|m| {
                m["entries"][1]["chunks"][1]["len"] = json!(4_000_000_000u32);
                m["entries"][1]["stored_size"] = json!(4_000_065_536u64);
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "deflate chunk length beyond the file",
            Box::new(|m| {
                m["entries"][0]["chunks"][0]["len"] = json!(4_000_000_000u32);
                m["entries"][0]["stored_size"] = json!(4_000_000_000u32);
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "deflate chunk within its bound but beyond the file",
            Box::new(|m| {
                // Make the *last* entry a deflate entry that claims a few more bytes than exist.
                let stored = m["entries"][1]["stored_size"].as_u64().unwrap();
                m["entries"][1]["compression"] = json!("deflate");
                m["entries"][1]["chunks"][1]["len"] = json!(4464 + 500);
                m["entries"][1]["stored_size"] = json!(stored + 500);
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "zero-length deflate chunk",
            Box::new(|m| {
                let old = m["entries"][0]["chunks"][0]["len"].as_u64().unwrap();
                let stored = m["entries"][0]["stored_size"].as_u64().unwrap();
                m["entries"][0]["chunks"][0]["len"] = json!(0);
                m["entries"][0]["stored_size"] = json!(stored - old);
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "extra chunk",
            Box::new(|m| {
                let chunk = m["entries"][1]["chunks"][1].clone();
                m["entries"][1]["chunks"]
                    .as_array_mut()
                    .unwrap()
                    .push(chunk);
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "missing chunk",
            Box::new(|m| {
                m["entries"][1]["chunks"].as_array_mut().unwrap().pop();
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "missing chunk hashes",
            Box::new(|m| {
                for chunk in m["entries"][1]["chunks"].as_array_mut().unwrap() {
                    chunk.as_object_mut().unwrap().remove("sha256");
                }
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "wrong index",
            Box::new(|m| m["entries"][1]["index"] = json!(5)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "dropped entry",
            Box::new(|m| {
                m["entries"].as_array_mut().unwrap().pop();
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "unknown role",
            Box::new(|m| m["entries"][1]["role"] = json!("executable")),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "unknown compression",
            Box::new(|m| m["entries"][1]["compression"] = json!("zstd")),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "two IR entries",
            Box::new(|m| {
                m["entries"][0]["role"] = json!("ir");
                m["entries"][1]["role"] = json!("ir");
            }),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "future manifest version",
            Box::new(|m| m["manifest_version"] = json!(2)),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "bad media type",
            Box::new(|m| m["entries"][1]["media_type"] = json!("a\nb")),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "bad entry hash",
            Box::new(|m| m["entries"][1]["sha256"] = json!("zz")),
            "E_PKG_CORRUPT_MANIFEST",
        ),
        (
            "entries is not an array",
            Box::new(|m| m["entries"] = json!({"0": 1})),
            "E_PKG_CORRUPT_MANIFEST",
        ),
    ];
    for (label, edit, expected) in cases {
        let forged = with_manifest(&bytes, edit);
        let (code, largest) = largest_allocation(|| open_code(&forged));
        assert_eq!(code, expected, "{label}");
        assert!(largest < 256 * KIB, "{label}: allocated {largest}");
    }

    // A wrong (but well-formed) entry hash only shows when the entry is read.
    let forged = with_manifest(&bytes, |m| {
        m["entries"][1]["sha256"] = json!("00".repeat(32));
    });
    let package = Package::open(forged.as_slice(), &Credentials::None).unwrap();
    assert_eq!(
        package.read_entry(1, u64::MAX).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );
    assert_eq!(package.verify_all().unwrap_err().code(), "E_PKG_INTEGRITY");
    assert!(package.read_entry(0, u64::MAX).is_ok());
}

#[test]
fn too_many_entries_or_chunks_are_rejected_while_parsing() {
    let bytes = small_package();
    let forged = with_manifest(&bytes, |m| {
        let template = json!({
            "index": 0, "name": "x", "media_type": "a/b", "role": "resource", "size": 0,
            "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "compression": "none", "offset": 0, "stored_size": 0, "chunks": []
        });
        let entries: Vec<Value> = (0..MAX_ENTRIES + 1)
            .map(|i| {
                let mut entry = template.clone();
                entry["index"] = json!(i);
                entry["name"] = json!(format!("e{i}"));
                entry
            })
            .collect();
        m["entries"] = Value::Array(entries);
    });
    let error = Package::open(forged.as_slice(), &Credentials::None).unwrap_err();
    assert_eq!(error.code(), "E_PKG_LIMIT");
    assert!(error.to_string().contains("number of entries"), "{error}");

    let forged = with_manifest(&bytes, |m| {
        m["entries"][1]["chunks"] = Value::Array(vec![json!({"len": 1}); 262_145]);
    });
    let error = Package::open(forged.as_slice(), &Credentials::None).unwrap_err();
    assert_eq!(error.code(), "E_PKG_LIMIT");
    assert!(error.to_string().contains("chunks"), "{error}");
}

// ---- decompression bombs -------------------------------------------------

/// Replaces the single deflate chunk of entry 0 with `stored` and makes the
/// manifest consistent with it (length, hash, offsets), so that only the
/// inflated size is a lie.
fn replace_deflate_chunk(bytes: &[u8], stored: &[u8]) -> Vec<u8> {
    let mut parts = Parts::split(bytes);
    let mut manifest = parts.manifest_json();
    let old_len = manifest["entries"][0]["stored_size"].as_u64().unwrap() as usize;
    assert_eq!(manifest["entries"][0]["compression"], "deflate");
    assert_eq!(
        manifest["entries"][0]["chunks"].as_array().unwrap().len(),
        1
    );
    parts.data.splice(..old_len, stored.iter().copied());
    manifest["entries"][0]["stored_size"] = json!(stored.len());
    manifest["entries"][0]["chunks"][0]["len"] = json!(stored.len());
    manifest["entries"][0]["chunks"][0]["sha256"] = json!(hex(&sha256(stored)));
    manifest["entries"][1]["offset"] = json!(stored.len());
    parts.set_manifest_json(&manifest);
    parts.assemble(true)
}

#[test]
fn deflate_chunk_inflating_beyond_its_declared_size_is_rejected() {
    let bytes = small_package();
    // Sanity: the untouched helper round-trips.
    let original = Parts::split(&bytes);
    let stored_len = original.manifest_json()["entries"][0]["stored_size"]
        .as_u64()
        .unwrap() as usize;
    let same = replace_deflate_chunk(&bytes, &original.data[..stored_len]);
    assert_eq!(open_and_verify(&same, &Credentials::None), Ok(()));

    // 20 MiB of zeros deflates to ~20 KiB; the entry still declares 50 000 bytes.
    let bomb = miniz_oxide::deflate::compress_to_vec(&vec![0u8; 20 * MIB], 6);
    assert!(bomb.len() < 50_000);
    let forged = replace_deflate_chunk(&bytes, &bomb);
    let package = Package::open(forged.as_slice(), &Credentials::None).unwrap();
    let (result, largest) = largest_allocation(|| package.read_chunk(0, 0));
    let error = result.unwrap_err();
    assert_eq!(error.code(), "E_PKG_INTEGRITY");
    assert!(error.to_string().contains("declared length"), "{error}");
    assert!(
        largest <= 64 * KIB,
        "the bomb made the reader allocate {largest} bytes"
    );
    assert_eq!(package.verify_all().unwrap_err().code(), "E_PKG_INTEGRITY");
    // The other entry is unaffected.
    assert!(package.read_entry(1, u64::MAX).is_ok());

    // One byte too many, one byte too few, garbage, and trailing bytes.
    for (label, payload) in [
        (
            "one more",
            miniz_oxide::deflate::compress_to_vec(&prose(50_001), 6),
        ),
        (
            "one less",
            miniz_oxide::deflate::compress_to_vec(&prose(49_999), 6),
        ),
        ("garbage", noise(1, 300)),
        ("trailing", {
            let mut ok = miniz_oxide::deflate::compress_to_vec(&prose(50_000), 6);
            ok.push(0);
            ok
        }),
    ] {
        let forged = replace_deflate_chunk(&bytes, &payload);
        let package = Package::open(forged.as_slice(), &Credentials::None).unwrap();
        assert_eq!(
            package.read_chunk(0, 0).unwrap_err().code(),
            "E_PKG_INTEGRITY",
            "{label}"
        );
    }

    // Right length but different content: caught by the entry hash.
    let other = miniz_oxide::deflate::compress_to_vec(&vec![b'x'; 50_000], 6);
    let forged = replace_deflate_chunk(&bytes, &other);
    let package = Package::open(forged.as_slice(), &Credentials::None).unwrap();
    assert!(package.read_chunk(0, 0).is_ok());
    assert_eq!(
        package.read_entry(0, u64::MAX).unwrap_err().code(),
        "E_PKG_INTEGRITY"
    );
}

#[test]
fn manifest_inflating_beyond_the_limit_is_rejected() {
    let bytes = small_package();
    let mut parts = Parts::split(&bytes);
    let mut json = serde_json::to_vec(&parts.manifest_json()).unwrap();
    // JSON allows trailing whitespace, so this is a valid 64 MiB + 1 manifest.
    json.resize(MAX_MANIFEST_INFLATED_LEN + 1, b' ');
    parts.set_manifest_bytes(&json);
    assert!(parts.manifest.len() < 200 * KIB);
    let forged = parts.assemble(true);
    let (result, largest) =
        largest_allocation(|| Package::open(forged.as_slice(), &Credentials::None).map(|_| ()));
    let error = result.unwrap_err();
    assert_eq!(error.code(), "E_PKG_LIMIT");
    assert!(error.to_string().contains("inflated manifest"), "{error}");
    assert!(largest <= MAX_MANIFEST_INFLATED_LEN, "allocated {largest}");

    // Exactly at the limit is still accepted.
    json.truncate(MAX_MANIFEST_INFLATED_LEN);
    parts.set_manifest_bytes(&json);
    let forged = parts.assemble(true);
    assert_eq!(open_code(&forged), "OK");

    // A manifest that is not deflate at all.
    let mut parts = Parts::split(&bytes);
    parts.manifest = b"{\"manifest_version\":1}".to_vec();
    assert_eq!(open_code(&parts.assemble(true)), "E_PKG_CORRUPT_MANIFEST");
    // Valid deflate, invalid JSON.
    let mut parts = Parts::split(&bytes);
    parts.set_manifest_bytes(b"{\"manifest_version\":1,");
    assert_eq!(open_code(&parts.assemble(true)), "E_PKG_CORRUPT_MANIFEST");
    // Oversized book summary.
    let forged = with_manifest(&bytes, |m| m["book"] = json!("x".repeat(1024 * 1024 + 1)));
    assert_eq!(open_code(&forged), "E_PKG_LIMIT");
}

// ---- names ---------------------------------------------------------------

const BAD_NAMES: &[&str] = &[
    "",
    "/etc/passwd",
    "/abs",
    "../../etc/passwd",
    "a/../../b",
    "a/./b",
    "./a",
    "..",
    ".",
    "a//b",
    "a/",
    "a\\b",
    "..\\..\\windows\\system32",
    "C:\\Windows\\win.ini",
    "C:/Windows/win.ini",
    "file:name",
    "nul\0byte",
    "new\nline",
    "tab\there",
    "esc\u{1b}[31m",
    "del\u{7f}",
    "c1\u{9b}",
];

#[test]
fn unsafe_names_are_rejected_at_build_time() {
    for name in BAD_NAMES {
        let mut builder = PackageBuilder::new();
        let error = builder
            .add_bytes(*name, "application/octet-stream", vec![1])
            .unwrap_err();
        assert_eq!(error.code(), "E_PKG_BAD_NAME", "{name:?}");
        let error = builder
            .add_file(*name, "application/octet-stream", "/nonexistent")
            .unwrap_err();
        assert_eq!(error.code(), "E_PKG_BAD_NAME", "{name:?}");
        // Messages never contain raw control characters.
        assert!(!error.to_string().chars().any(char::is_control), "{name:?}");
    }
    let mut builder = PackageBuilder::new();
    let long = "n".repeat(513);
    assert_eq!(
        builder.add_bytes(long, "a/b", vec![]).unwrap_err().code(),
        "E_PKG_BAD_NAME"
    );
    builder.add_bytes("dup.txt", "text/plain", vec![]).unwrap();
    assert_eq!(
        builder
            .add_bytes("dup.txt", "text/plain", vec![])
            .unwrap_err(),
        PackageError::DuplicateName("dup.txt".into())
    );
    // The IR name is reserved.
    assert_eq!(
        builder
            .add_bytes("book.ir.json", "application/json", vec![])
            .unwrap_err()
            .code(),
        "E_PKG_BAD_NAME"
    );
    assert_eq!(
        builder
            .add_bytes("ok", "bad\nmedia", vec![])
            .unwrap_err()
            .code(),
        "E_PKG_BAD_OPTION"
    );
    assert_eq!(
        builder.add_bytes("ok", "", vec![]).unwrap_err().code(),
        "E_PKG_BAD_OPTION"
    );
}

#[test]
fn unsafe_names_in_a_crafted_manifest_are_rejected() {
    let bytes = small_package();
    for name in BAD_NAMES {
        let forged = with_manifest(&bytes, |m| m["entries"][1]["name"] = json!(name));
        let error = Package::open(forged.as_slice(), &Credentials::None).unwrap_err();
        assert_eq!(error.code(), "E_PKG_BAD_NAME", "{name:?}");
        assert!(PackageHead::open(&forged, &Credentials::None).is_err());
    }
    let forged = with_manifest(&bytes, |m| m["entries"][1]["name"] = json!("n".repeat(513)));
    assert_eq!(open_code(&forged), "E_PKG_BAD_NAME");
    let forged = with_manifest(&bytes, |m| m["entries"][1]["name"] = json!("text/a.txt"));
    assert_eq!(open_code(&forged), "E_PKG_DUPLICATE_NAME");
    // A legitimate rename is fine (clear packages are not authenticated).
    let forged = with_manifest(&bytes, |m| {
        m["entries"][1]["name"] = json!("media/renamed.bin")
    });
    assert_eq!(open_code(&forged), "OK");
}

// ---- KDF parameters and prelude -------------------------------------------

#[test]
fn out_of_bounds_kdf_parameters_are_rejected_before_any_work() {
    let bytes = password_sample();
    let cases: &[(&str, u64)] = &[
        ("m_kib", 8191),
        ("m_kib", 1024),
        ("m_kib", 0),
        ("m_kib", 1024 * 1024 + 1),
        ("m_kib", u32::MAX as u64),
        ("m_kib", u64::MAX),
        ("t", 0),
        ("t", 17),
        ("t", 1_000_000),
        ("p", 0),
        ("p", 5),
        ("p", 255),
    ];
    for (field, value) in cases {
        let forged = with_prelude(&bytes, |p| {
            p["encryption"]["slots"][0]["kdf"][*field] = json!(value);
        });
        let started = std::time::Instant::now();
        let (result, largest) = largest_allocation(|| {
            Package::open(forged.as_slice(), &Credentials::Password(PASSWORD)).map(|_| ())
        });
        let error = result.unwrap_err();
        // u64::MAX does not even parse as a u32.
        let expected = if *value > u32::MAX as u64 {
            "E_PKG_CORRUPT_PRELUDE"
        } else {
            "E_PKG_LIMIT"
        };
        assert_eq!(error.code(), expected, "{field}={value}");
        assert!(largest < MIB, "{field}={value} allocated {largest}");
        assert!(
            started.elapsed().as_secs() < 2,
            "{field}={value} ran the KDF"
        );
        // The file is refused even when another credential would work, and by probe.
        assert_eq!(
            Package::open(forged.as_slice(), &Credentials::Key(&KEY))
                .unwrap_err()
                .code(),
            expected
        );
        assert_eq!(probe(&forged).unwrap_err().code(), expected);
    }

    // The writer refuses the same parameters.
    for params in [
        KdfParams {
            m_kib: 4096,
            t: 3,
            p: 1,
        },
        KdfParams {
            m_kib: 2 * 1024 * 1024,
            t: 3,
            p: 1,
        },
        KdfParams {
            m_kib: 65536,
            t: 0,
            p: 1,
        },
        KdfParams {
            m_kib: 65536,
            t: 17,
            p: 1,
        },
        KdfParams {
            m_kib: 65536,
            t: 3,
            p: 5,
        },
    ] {
        let mut builder = PackageBuilder::new();
        builder.encrypt(EncryptOptions::new().password("pw").kdf(params));
        assert_eq!(
            builder.to_vec().unwrap_err().code(),
            "E_PKG_LIMIT",
            "{params:?}"
        );
    }
}

#[test]
fn malformed_preludes_are_rejected() {
    let encrypted = password_sample();
    let cases: Vec<Case> = vec![
        (
            "unknown cipher",
            Box::new(|p| p["encryption"]["cipher"] = json!("ROT13")),
            "E_PKG_UNSUPPORTED_FEATURE",
        ),
        (
            "unknown subkey derivation",
            Box::new(|p| p["encryption"]["subkeys"] = json!("MD5")),
            "E_PKG_UNSUPPORTED_FEATURE",
        ),
        (
            "no slots",
            Box::new(|p| p["encryption"]["slots"] = json!([])),
            "E_PKG_LIMIT",
        ),
        (
            "nine slots",
            Box::new(|p| {
                let slot = p["encryption"]["slots"][1].clone();
                p["encryption"]["slots"] = Value::Array(vec![slot; 9]);
            }),
            "E_PKG_LIMIT",
        ),
        (
            "missing key commitment",
            Box::new(|p| {
                p["encryption"]
                    .as_object_mut()
                    .unwrap()
                    .remove("key_commitment");
            }),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "malformed key commitment",
            Box::new(|p| p["encryption"]["key_commitment"] = json!("AAAA")),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "short salt",
            Box::new(|p| p["encryption"]["slots"][0]["salt"] = json!("AAAA")),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "salt is not base64",
            Box::new(|p| p["encryption"]["slots"][0]["salt"] = json!("!!!!!!!!!!!!!!!!!!!!!!==")),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "missing wrapped key",
            Box::new(|p| {
                p["encryption"]["slots"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("wrapped_key");
            }),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "missing kdf",
            Box::new(|p| {
                p["encryption"]["slots"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("kdf");
            }),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "bad key id",
            Box::new(|p| p["encryption"]["slots"][1]["key_id"] = json!("XYZ")),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "encryption removed",
            Box::new(|p| {
                p.as_object_mut().unwrap().remove("encryption");
            }),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "card smuggled in without the flag",
            Box::new(|p| p["card"] = json!({"title": "x"})),
            "E_PKG_CORRUPT_PRELUDE",
        ),
        (
            "chunk size too small",
            Box::new(|p| p["chunk_size"] = json!(1024)),
            "E_PKG_LIMIT",
        ),
        (
            "chunk size too large",
            Box::new(|p| p["chunk_size"] = json!(8 * 1024 * 1024)),
            "E_PKG_LIMIT",
        ),
        (
            "chunk size zero",
            Box::new(|p| p["chunk_size"] = json!(0)),
            "E_PKG_LIMIT",
        ),
        (
            "not an object",
            Box::new(|p| *p = json!([1, 2, 3])),
            "E_PKG_CORRUPT_PRELUDE",
        ),
    ];
    for (label, edit, expected) in cases {
        let forged = with_prelude(&encrypted, edit);
        for credentials in [
            Credentials::Password(PASSWORD),
            Credentials::Key(&KEY),
            Credentials::None,
        ] {
            let error = Package::open(forged.as_slice(), &credentials).unwrap_err();
            assert_eq!(error.code(), expected, "{label}");
        }
        assert_eq!(probe(&forged).unwrap_err().code(), expected, "{label}");
    }

    // A slot with an unknown KDF or kind is skipped, not fatal: the other slot still works…
    let forged = with_prelude(&encrypted, |p| {
        p["encryption"]["slots"][0]["kdf"]["algorithm"] = json!("scrypt");
    });
    // …although here the manifest authentication (which binds the prelude) notices the edit.
    assert_eq!(
        Package::open(forged.as_slice(), &Credentials::Key(&KEY))
            .unwrap_err()
            .code(),
        "E_PKG_INTEGRITY"
    );
    assert_eq!(
        Package::open(forged.as_slice(), &Credentials::Password(PASSWORD)).unwrap_err(),
        PackageError::WrongCredentials
    );
    assert_eq!(
        probe(&forged).unwrap().slots[0].kind,
        darm_package::SlotKind::Unknown
    );

    // Clear package: a bad card colour, and an `encryption` section without the flag.
    let clear = clear_sample();
    let forged = with_prelude(&clear, |p| {
        p["card"]["cover_color"] = json!("javascript:alert(1)")
    });
    assert_eq!(open_code(&forged), "E_PKG_CORRUPT_PRELUDE");
    let enc = Parts::split(&encrypted).prelude_json()["encryption"].clone();
    let forged = with_prelude(&clear, |p| p["encryption"] = enc);
    assert_eq!(open_code(&forged), "E_PKG_CORRUPT_PRELUDE");
}

// ---- version policy --------------------------------------------------------

#[test]
fn version_policy() {
    let bytes = clear_sample();

    // Unknown major: rejected with advice to update.
    let mut parts = Parts::split(&bytes);
    parts.header.format_major = 2;
    let error = Package::open(parts.assemble(true).as_slice(), &Credentials::None).unwrap_err();
    assert_eq!(
        error,
        PackageError::UnsupportedVersion { major: 2, minor: 0 }
    );
    assert!(error.to_string().contains("update the reader"), "{error}");
    let mut parts = Parts::split(&bytes);
    parts.header.format_major = 0;
    assert_eq!(
        open_code(&parts.assemble(true)),
        "E_PKG_UNSUPPORTED_VERSION"
    );

    // Newer minor with unknown JSON members everywhere: accepted, content identical.
    let mut parts = Parts::split(&bytes);
    parts.header.format_minor = 7;
    let mut prelude = parts.prelude_json();
    prelude["future_prelude_field"] = json!({"nested": [1, 2, 3]});
    prelude["card"]["subtitle"] = json!("from the future");
    parts.set_prelude_json(&prelude);
    let mut manifest = parts.manifest_json();
    manifest["future_manifest_field"] = json!(true);
    for entry in manifest["entries"].as_array_mut().unwrap() {
        entry["future_entry_field"] = json!("x");
        for chunk in entry["chunks"].as_array_mut().unwrap() {
            chunk["future_chunk_field"] = json!(1);
        }
    }
    parts.set_manifest_json(&manifest);
    let newer = parts.assemble(true);
    let package = Package::open(newer.as_slice(), &Credentials::None).unwrap();
    assert_eq!(package.header().format_minor, 7);
    assert_eq!(probe(&newer).unwrap().version(), "1.7");
    assert_sample_content(&package);
    package.verify_all().unwrap();

    // Reserved flag bits (including the one earmarked for signatures): rejected.
    for bit in [2u32, 3, 15, 31] {
        let mut parts = Parts::split(&bytes);
        parts.header.flags |= 1 << bit;
        let error = Package::open(parts.assemble(true).as_slice(), &Credentials::None).unwrap_err();
        assert_eq!(error.code(), "E_PKG_UNSUPPORTED_FEATURE", "bit {bit}");
    }
}

// ---- writer-side limits and options ----------------------------------------

#[test]
fn builder_rejects_invalid_options() {
    for chunk_size in [0u32, 1, 65_535, 4 * 1024 * 1024 + 1, u32::MAX] {
        let mut builder = PackageBuilder::new();
        builder.chunk_size(chunk_size);
        assert_eq!(
            builder.to_vec().unwrap_err().code(),
            "E_PKG_LIMIT",
            "{chunk_size}"
        );
    }
    for chunk_size in [65_536u32, 4 * 1024 * 1024] {
        let mut builder = PackageBuilder::new();
        builder.chunk_size(chunk_size).ir(IR.to_vec()).unwrap();
        assert!(builder.to_vec().is_ok());
    }

    let mut builder = PackageBuilder::new();
    builder.encrypt(EncryptOptions::new());
    assert_eq!(builder.to_vec().unwrap_err().code(), "E_PKG_BAD_OPTION");

    let mut builder = PackageBuilder::new();
    builder.encrypt(fast_encryption().password(""));
    assert_eq!(builder.to_vec().unwrap_err().code(), "E_PKG_BAD_OPTION");

    let mut builder = PackageBuilder::new();
    builder.encrypt(fast_encryption().keyfile_key(&KEY).keyfile_key(&KEY));
    assert_eq!(builder.to_vec().unwrap_err().code(), "E_PKG_BAD_OPTION");

    let mut options = fast_encryption();
    for i in 0..9u8 {
        options = options.keyfile_key(&[i; 32]);
    }
    let mut builder = PackageBuilder::new();
    builder.encrypt(options);
    assert_eq!(builder.to_vec().unwrap_err().code(), "E_PKG_LIMIT");

    let mut builder = PackageBuilder::new();
    builder.card(PublicCard {
        title: "t".into(),
        cover_color: Some("blue".into()),
        ..PublicCard::default()
    });
    assert_eq!(
        builder.to_vec().unwrap_err().code(),
        "E_PKG_CORRUPT_PRELUDE"
    );

    // A card too large for the 64 KiB prelude.
    let mut builder = PackageBuilder::new();
    builder.card(PublicCard {
        title: "t".repeat(70_000),
        ..PublicCard::default()
    });
    let error = builder.to_vec().unwrap_err();
    assert_eq!(error.code(), "E_PKG_LIMIT");
    assert!(error.to_string().contains("prelude"), "{error}");

    let mut builder = PackageBuilder::new();
    let error = builder
        .book_summary(json!("x".repeat(1024 * 1024 + 1)))
        .unwrap_err();
    assert_eq!(error.code(), "E_PKG_LIMIT");

    let mut builder = PackageBuilder::new();
    builder.generator("bad\ngenerator");
    assert_eq!(builder.to_vec().unwrap_err().code(), "E_PKG_BAD_OPTION");
    let mut builder = PackageBuilder::new();
    builder.created("x".repeat(300));
    assert_eq!(builder.to_vec().unwrap_err().code(), "E_PKG_LIMIT");

    let mut builder = PackageBuilder::new();
    for i in 0..MAX_ENTRIES {
        builder
            .add_bytes(format!("e{i}"), "a/b", Vec::new())
            .unwrap();
    }
    assert_eq!(
        builder
            .add_bytes("one-too-many", "a/b", Vec::new())
            .unwrap_err()
            .code(),
        "E_PKG_LIMIT"
    );
    // Exactly the maximum number of entries builds and reads back.
    let bytes = builder.to_vec().unwrap();
    let package = Package::open(bytes.as_slice(), &Credentials::None).unwrap();
    assert_eq!(package.entries().len(), MAX_ENTRIES);
    assert_eq!(package.verify_all().unwrap().entries, MAX_ENTRIES);
}

#[test]
fn input_file_changing_during_the_build_is_detected() {
    /// A sink that rewrites the input file as soon as the header is written,
    /// i.e. between the builder's two passes.
    struct Saboteur {
        path: std::path::PathBuf,
        replacement: Vec<u8>,
        done: bool,
        sink: Vec<u8>,
    }
    impl Write for Saboteur {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if !self.done {
                std::fs::write(&self.path, &self.replacement)?;
                self.done = true;
            }
            self.sink.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let dir = scratch_dir("changed");
    let original = noise(4, 100_000);
    let mut same_size = original.clone();
    same_size[50_000] ^= 1;
    let replacements = [
        ("same size, different content", same_size),
        ("shorter", original[..90_000].to_vec()),
        ("longer", noise(4, 100_001)),
        ("empty", Vec::new()),
    ];
    for encrypted in [false, true] {
        for (label, replacement) in &replacements {
            let path = dir.join("input.bin");
            std::fs::write(&path, &original).unwrap();
            let mut builder = PackageBuilder::new();
            builder
                .chunk_size(64 * 1024)
                .add_file("input.bin", "application/octet-stream", &path)
                .unwrap();
            if encrypted {
                builder.encrypt(fast_encryption().keyfile_key(&KEY));
            }
            let sink = Saboteur {
                path: path.clone(),
                replacement: replacement.clone(),
                done: false,
                sink: Vec::new(),
            };
            let error = builder.write_to(sink).unwrap_err();
            assert_eq!(
                error.code(),
                "E_PKG_SOURCE_CHANGED",
                "{label} encrypted={encrypted}"
            );
            assert!(error.to_string().contains("input.bin"), "{error}");
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- streaming ---------------------------------------------------------------

#[test]
fn large_files_are_streamed_not_loaded() {
    // A 12 MiB input with 1 MiB chunks: neither building nor reading may ever
    // hold more than a few chunks' worth of memory in one allocation.
    const SIZE: usize = 12 * MIB;
    const TEXT_SIZE: usize = 6 * MIB + 5;
    const BOUND: usize = 4 * MIB;
    let dir = scratch_dir("streaming");
    let input = dir.join("big.bin");
    let text_input = dir.join("big.txt");
    let content = noise(99, SIZE);
    std::fs::write(&input, &content).unwrap();
    std::fs::write(&text_input, prose(TEXT_SIZE)).unwrap();
    let digest = sha256(&content);
    drop(content);

    for encrypted in [false, true] {
        let mut builder = PackageBuilder::new();
        builder
            .add_file("big.bin", "video/mp4", &input)
            .unwrap()
            .add_file("big.txt", "text/plain", &text_input)
            .unwrap();
        if encrypted {
            builder.encrypt(EncryptOptions::new().keyfile_key(&KEY));
        }
        let credentials = if encrypted {
            Credentials::Key(&KEY)
        } else {
            Credentials::None
        };
        let out = dir.join("big.darm");
        let (summary, largest) = largest_allocation(|| builder.write_to_path(&out).unwrap());
        assert!(
            largest < BOUND,
            "building allocated {largest} bytes at once"
        );
        assert!(summary.total_len > SIZE as u64);
        assert_eq!(summary.compressed_entries, 1);

        let (package, largest) =
            largest_allocation(|| Package::open_path(&out, &credentials).unwrap());
        assert!(largest < BOUND, "opening allocated {largest} bytes at once");
        assert_eq!(package.entries()[0].sha256.0, digest);
        assert_eq!(package.entries()[0].chunks.len(), 12);
        assert_eq!(package.entries()[1].chunks.len(), 7);

        let (report, largest) = largest_allocation(|| package.verify_all().unwrap());
        assert!(
            largest < BOUND,
            "verifying allocated {largest} bytes at once"
        );
        assert_eq!(report.bytes, (SIZE + TEXT_SIZE) as u64);

        for (entry, size) in [(0, SIZE), (1, TEXT_SIZE)] {
            let (copied, largest) = largest_allocation(|| {
                let mut reader = package.entry_reader(entry).unwrap();
                std::io::copy(&mut reader, &mut std::io::sink()).unwrap()
            });
            assert!(
                largest < BOUND,
                "streaming allocated {largest} bytes at once"
            );
            assert_eq!(copied, size as u64);
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
