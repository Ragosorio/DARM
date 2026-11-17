//! Seeded mutation "mini-fuzz": thousands of random mutations and truncations
//! of valid clear and encrypted packages.
//!
//! Properties checked for every mutant:
//! * the reader never panics (a panic fails the test);
//! * the outcome is either an error, or content byte-identical to the original.
//!
//! The seeds are fixed so a failure is reproducible; the failing iteration
//! and seed are printed. Real coverage-guided fuzzing lives in `/fuzz`.
//!
//! Set `DARM_MINIFUZZ_SCALE=<n>` to multiply the number of iterations, e.g.
//! for a longer soak run in release mode:
//! `DARM_MINIFUZZ_SCALE=100 cargo test -p darm-package --release --test minifuzz`

mod common;

use common::*;
use darm_package::{Credentials, HEADER_LEN, Package, PackageBuilder, PackageHead, probe};

/// Iteration multiplier taken from `DARM_MINIFUZZ_SCALE` (default 1).
fn scaled(iterations: usize) -> usize {
    let scale = std::env::var("DARM_MINIFUZZ_SCALE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    iterations.saturating_mul(scale)
}

/// A small multi-entry, multi-chunk book (about 80 KiB) so iterations are cheap.
fn fuzz_entries() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    vec![
        ("media/a.bin", "video/mp4", noise(31, 70_000)),
        ("text/b.vtt", "text/vtt", prose(9_000)),
        ("empty", "application/octet-stream", Vec::new()),
        ("c.bin", "image/png", noise(32, 300)),
    ]
}

fn fuzz_builder() -> PackageBuilder {
    let mut builder = PackageBuilder::new();
    builder
        .generator("darm-fuzz")
        .chunk_size(64 * 1024)
        .card(card())
        .ir(IR.to_vec())
        .unwrap();
    for (name, media_type, content) in fuzz_entries() {
        builder.add_bytes(name, media_type, content).unwrap();
    }
    builder
}

/// The plaintext of every entry, in index order.
fn original_content() -> Vec<Vec<u8>> {
    let mut content = vec![IR.to_vec()];
    content.extend(fuzz_entries().into_iter().map(|(_, _, bytes)| bytes));
    content
}

#[derive(Default, Debug)]
struct Stats {
    rejected_at_open: usize,
    rejected_at_read: usize,
    identical: usize,
}

/// Opens a mutant and reads everything it offers. Returns an error string if a
/// property is violated.
fn check(
    mutant: &[u8],
    credentials: &Credentials<'_>,
    original: &[Vec<u8>],
    stats: &mut Stats,
) -> Result<(), String> {
    // The cheap entry points must not panic either.
    let _ = probe(mutant);
    let _ = PackageHead::open(mutant, credentials);

    let package = match Package::open(mutant, credentials) {
        Ok(package) => package,
        Err(_) => {
            stats.rejected_at_open += 1;
            return Ok(());
        }
    };
    let verified = package.verify_all().is_ok();
    let mut all_read = true;
    for entry in package.entries() {
        // Exercise the per-chunk and streaming paths as well.
        for chunk in 0..entry.chunks.len() as u32 {
            let _ = package.chunk_location(entry.index, chunk);
            let _ = package.read_chunk(entry.index, chunk);
        }
        match package.read_entry(entry.index, 1 << 20) {
            Ok(content) => {
                // Whatever is returned as verified content must be original content.
                let expected = original.get(entry.index as usize);
                if expected != Some(&content) {
                    return Err(format!(
                        "entry {} ({:?}) returned {} bytes of content that differ from the original",
                        entry.index,
                        entry.name,
                        content.len()
                    ));
                }
            }
            Err(_) => all_read = false,
        }
    }
    if verified {
        if !all_read || package.entries().len() != original.len() {
            return Err("verify_all passed but the content is not the original".into());
        }
        stats.identical += 1;
    } else {
        stats.rejected_at_read += 1;
    }
    Ok(())
}

/// Produces one mutant of `base`.
fn mutate(rng: &mut XorShift, base: &[u8], head_len: usize) -> Vec<u8> {
    let mut out = base.to_vec();
    // Bias positions towards the structured head (header, prelude, manifest).
    let position = |rng: &mut XorShift, len: usize| -> usize {
        match rng.below(4) {
            0 => rng.below(HEADER_LEN.min(len)),
            1 => rng.below(head_len.min(len)),
            _ => rng.below(len),
        }
    };
    match rng.below(10) {
        // Flip one bit.
        0 | 1 => {
            let at = position(rng, out.len());
            out[at] ^= 1 << rng.below(8);
        }
        // Overwrite one byte.
        2 => {
            let at = position(rng, out.len());
            out[at] = rng.next_u64() as u8;
        }
        // Flip a few bits in several places.
        3 => {
            for _ in 0..1 + rng.below(4) {
                let at = position(rng, out.len());
                out[at] ^= 1 << rng.below(8);
            }
        }
        // Truncate.
        4 | 5 => {
            let keep = position(rng, out.len());
            out.truncate(keep);
        }
        // Overwrite a short run with random bytes.
        6 => {
            let at = position(rng, out.len());
            let len = (1 + rng.below(32)).min(out.len() - at);
            let run = rng.bytes(len);
            out[at..at + len].copy_from_slice(&run);
        }
        // Insert random bytes.
        7 => {
            let at = position(rng, out.len());
            let len = 1 + rng.below(16);
            let run = rng.bytes(len);
            out.splice(at..at, run);
        }
        // Delete a run.
        8 => {
            let at = position(rng, out.len());
            let len = (1 + rng.below(64)).min(out.len() - at);
            out.drain(at..at + len);
        }
        // Copy a run from elsewhere in the file (reorders/duplicates chunks).
        _ => {
            let from = position(rng, out.len());
            let to = position(rng, out.len());
            let len = (1 + rng.below(4096))
                .min(out.len() - from)
                .min(out.len() - to);
            out.copy_within(from..from + len, to);
        }
    }
    out
}

fn run(
    label: &str,
    base: &[u8],
    credentials: Credentials<'_>,
    seed: u64,
    iterations: usize,
) -> Stats {
    let original = original_content();
    let head_len = probe(base).unwrap().head_len as usize;
    let mut stats = Stats::default();
    // The unmutated package must pass, otherwise the test proves nothing.
    check(base, &credentials, &original, &mut stats).unwrap();
    assert_eq!(
        stats.identical, 1,
        "{label}: the base package does not verify"
    );

    let mut rng = XorShift::new(seed);
    for iteration in 0..scaled(iterations) {
        let mutant = mutate(&mut rng, base, head_len);
        if mutant == base {
            continue;
        }
        if let Err(problem) = check(&mutant, &credentials, &original, &mut stats) {
            panic!("{label}: seed {seed:#x}, iteration {iteration}: {problem}");
        }
    }
    println!("{label}: {stats:?}");
    stats
}

#[test]
fn mutated_clear_packages_never_panic_or_yield_wrong_content() {
    let base = fuzz_builder().to_vec().unwrap();
    let stats = run("clear", &base, Credentials::None, 0xD0A7_0001, 2500);
    // Every mutation changes at least one byte, and every byte is covered by
    // a hash, so nothing but the base may verify.
    assert_eq!(stats.identical, 1);
    assert!(stats.rejected_at_open > 500);
    assert!(stats.rejected_at_read > 100);
}

#[test]
fn mutated_encrypted_packages_never_panic_or_yield_wrong_content() {
    let mut builder = fuzz_builder();
    builder.encrypt(fast_encryption().keyfile_key(&KEY).expose_card(true));
    let base = builder.to_vec().unwrap();
    let stats = run(
        "encrypted/key",
        &base,
        Credentials::Key(&KEY),
        0xD0A7_0002,
        2500,
    );
    assert_eq!(stats.identical, 1);
    assert!(stats.rejected_at_open > 500);
    assert!(stats.rejected_at_read > 100);

    // The same without credentials, and with the wrong key: only errors.
    let other_key = [0x55u8; 32];
    let mut rng = XorShift::new(0xD0A7_0003);
    let head_len = probe(&base).unwrap().head_len as usize;
    for _ in 0..scaled(300) {
        let mutant = mutate(&mut rng, &base, head_len);
        assert!(Package::open(mutant.as_slice(), &Credentials::None).is_err());
        assert!(Package::open(mutant.as_slice(), &Credentials::Key(&other_key)).is_err());
    }
}

#[test]
fn mutated_password_packages_never_panic_or_yield_wrong_content() {
    // Fewer iterations: every mutant that keeps its prelude intact runs Argon2id.
    let mut builder = fuzz_builder();
    builder.encrypt(fast_encryption().password(PASSWORD));
    let base = builder.to_vec().unwrap();
    let stats = run(
        "encrypted/password",
        &base,
        Credentials::Password(PASSWORD),
        0xD0A7_0004,
        150,
    );
    assert_eq!(stats.identical, 1);
}

/// Structure-aware variant: mutate the *inflated* manifest JSON (or the
/// prelude JSON) and reseal lengths, hashes and the header check, so mutants
/// get past the integrity checks and reach the parsers and the validator.
#[test]
fn resealed_manifest_and_prelude_mutations_never_panic_or_yield_wrong_content() {
    let base = fuzz_builder().to_vec().unwrap();
    let original = original_content();
    let parts = Parts::split(&base);
    let manifest_json = serde_json::to_vec(&parts.manifest_json()).unwrap();
    let prelude_json = parts.prelude.clone();

    // Interesting replacement tokens for JSON-level mutations.
    let tokens: &[&[u8]] = &[
        b"0",
        b"-1",
        b"1",
        b"65536",
        b"65537",
        b"4294967295",
        b"4294967296",
        b"18446744073709551615",
        b"18446744073709551616",
        b"9007199254740993",
        b"1e99",
        b"null",
        b"true",
        b"[]",
        b"{}",
        b"\"\"",
        b"\"..\"",
        b"\"../x\"",
        b"\"/x\"",
        b"\"a\\\\b\"",
        b"\"deflate\"",
        b"\"none\"",
        b"\"ir\"",
        b"\"resource\"",
    ];

    let json_mutation = |rng: &mut XorShift, json: &[u8]| -> Vec<u8> {
        let mut out = json.to_vec();
        match rng.below(6) {
            0 => {
                let at = rng.below(out.len());
                out[at] ^= 1 << rng.below(8);
            }
            1 => {
                // Replace a digit run with an interesting number.
                let digits: Vec<usize> = (0..out.len())
                    .filter(|&i| out[i].is_ascii_digit())
                    .collect();
                let pick = rng.below(digits.len().max(1));
                if let Some(&start) = digits.get(pick) {
                    let mut end = start;
                    while end < out.len() && out[end].is_ascii_digit() {
                        end += 1;
                    }
                    let token = tokens[rng.below(11)];
                    out.splice(start..end, token.iter().copied());
                }
            }
            2 => {
                // Replace a quoted string with a token.
                let quotes: Vec<usize> = (0..out.len()).filter(|&i| out[i] == b'"').collect();
                if quotes.len() >= 2 {
                    let pick = rng.below(quotes.len() - 1);
                    let token = tokens[rng.below(tokens.len())];
                    out.splice(quotes[pick]..=quotes[pick + 1], token.iter().copied());
                }
            }
            3 => {
                let at = rng.below(out.len());
                let len = (1 + rng.below(40)).min(out.len() - at);
                out.drain(at..at + len);
            }
            4 => {
                let from = rng.below(out.len());
                let len = (1 + rng.below(200)).min(out.len() - from);
                let run = out[from..from + len].to_vec();
                let at = rng.below(out.len());
                out.splice(at..at, run);
            }
            _ => {
                let at = rng.below(out.len());
                out.truncate(at);
            }
        }
        out
    };

    let mut rng = XorShift::new(0xD0A7_0005);
    let mut stats = Stats::default();
    let mut parsed_ok = 0;
    for iteration in 0..scaled(2500) {
        let mut mutant = parts.clone();
        if iteration % 5 == 0 {
            mutant.prelude = json_mutation(&mut rng, &prelude_json);
        } else {
            mutant.set_manifest_bytes(&json_mutation(&mut rng, &manifest_json));
        }
        let bytes = mutant.assemble(true);
        // Resealed mutants can legitimately differ in metadata (a renamed
        // entry, another media type): clear packages are not authenticated.
        // What must hold is that verified *content* is original content.
        let package = Package::open(bytes.as_slice(), &Credentials::None);
        match package {
            Err(_) => stats.rejected_at_open += 1,
            Ok(package) => {
                parsed_ok += 1;
                for entry in package.entries() {
                    for chunk in 0..entry.chunks.len() as u32 {
                        let _ = package.read_chunk(entry.index, chunk);
                    }
                    if let Ok(content) = package.read_entry(entry.index, 1 << 20) {
                        assert!(
                            original.get(entry.index as usize) == Some(&content),
                            "iteration {iteration}: entry {} returned foreign content",
                            entry.index
                        );
                    }
                }
                let _ = package.verify_all();
                let _ = package.manifest_json();
                let _ = package.ir_bytes();
            }
        }
    }
    println!(
        "resealed: {} rejected at open, {parsed_ok} opened",
        stats.rejected_at_open
    );
    assert!(stats.rejected_at_open > 1000);
    assert!(
        parsed_ok > 20,
        "the mutations never produce a parseable manifest: {parsed_ok}"
    );
}

/// Container-level reseal, identical to the one in `fuzz/fuzz_targets/package_open.rs`:
/// whatever the mutation did, lengths, hashes and the header check are made
/// consistent again, so the mutant is judged on its content alone.
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
    let prelude_hash = sha256(&out[HEADER_LEN..HEADER_LEN + prelude_len]);
    out[48..80].copy_from_slice(&prelude_hash);
    let manifest_start = HEADER_LEN + prelude_len;
    let manifest_hash = sha256(&out[manifest_start..manifest_start + manifest_len]);
    out[80..112].copy_from_slice(&manifest_hash);
    let check = sha256(&out[..120]);
    out[120..128].copy_from_slice(&check[..8]);
    Some(out)
}

fn credentials_are_key(credentials: &Credentials<'_>) -> bool {
    matches!(credentials, Credentials::Key(_))
}

#[test]
fn resealed_blind_mutations_never_panic_or_yield_wrong_content() {
    let clear = fuzz_builder().to_vec().unwrap();
    let mut builder = fuzz_builder();
    builder.encrypt(fast_encryption().keyfile_key(&KEY).expose_card(true));
    let encrypted = builder.to_vec().unwrap();
    let original = original_content();

    for (label, base, credentials, seed) in [
        ("clear/resealed", &clear, Credentials::None, 0xD0A7_0006u64),
        (
            "encrypted/resealed",
            &encrypted,
            Credentials::Key(&KEY),
            0xD0A7_0007,
        ),
    ] {
        let head_len = probe(base).unwrap().head_len as usize;
        let mut rng = XorShift::new(seed);
        let mut stats = Stats::default();
        for iteration in 0..scaled(1000) {
            let Some(mutant) = reseal(&mutate(&mut rng, base, head_len)) else {
                continue;
            };
            let verified_before = stats.identical;
            if let Err(problem) = check(&mutant, &credentials, &original, &mut stats) {
                panic!("{label}: seed {seed:#x}, iteration {iteration}: {problem}");
            }
            // In an encrypted package every byte is either authenticated or
            // derived from authenticated bytes, so a resealed mutant that
            // verifies can only be the original file (the mutation hit a
            // field that resealing recomputes). Clear packages make no such
            // promise: their metadata can be rewritten consistently.
            if credentials_are_key(&credentials) && stats.identical > verified_before {
                assert!(
                    mutant == *base,
                    "{label}: seed {seed:#x}, iteration {iteration}: a modified encrypted package verified"
                );
            }
        }
        println!("{label}: {stats:?}");
        assert!(
            stats.rejected_at_open + stats.rejected_at_read > 500,
            "{label}: {stats:?}"
        );
    }
}
