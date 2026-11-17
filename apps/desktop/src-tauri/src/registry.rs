//! The only door between the operating system's files and the webview.
//!
//! The reader UI runs in a webview and must never be able to read an
//! arbitrary path. Instead, every file that the *operating system* (a
//! double-click, "Open with", a command-line argument, a second launch) or the
//! *user* (the native File → Open… dialog) hands to the app is recorded here
//! under a random token. The webview only ever sees `{ token, name, size }`
//! and can only ask for byte ranges of a token it was given.
//!
//! Invariants:
//!
//! * Only regular files whose *canonical* path ends in `.darm` are accepted
//!   (so a symlink called `book.darm` that points at `~/.ssh/id_ed25519` is
//!   rejected).
//! * The file is opened once, at registration. Later reads go through that
//!   handle, so swapping the path afterwards cannot redirect a token to
//!   another file.
//! * Tokens are 128 bits from the operating system's CSPRNG, hex encoded.
//! * A read is at most [`MAX_CHUNK`] bytes and must lie entirely inside the
//!   file as it was when it was registered.
//! * At most [`MAX_OPEN`] files are held at a time.
//!
//! This module has no dependency on Tauri, so its tests run without a webview.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

/// Largest read a single `darm_read_chunk` call may ask for: 8 MiB.
pub const MAX_CHUNK: u64 = 8 * 1024 * 1024;

/// Largest number of files that can be registered (pending or being read) at once.
pub const MAX_OPEN: usize = 64;

/// The extension, without the dot, a file must have to be accepted.
pub const EXTENSION: &str = "darm";

/// What the webview learns about a registered file. No path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileInfo {
    pub token: String,
    pub name: String,
    pub size: u64,
}

#[derive(Debug)]
pub enum RegistryError {
    /// The path does not exist or cannot be resolved.
    NotFound(io::ErrorKind),
    /// The canonical path does not end in `.darm`.
    NotDarm,
    /// A directory, socket, FIFO, device…
    NotRegularFile,
    /// The registry already holds [`MAX_OPEN`] files.
    TooManyOpen,
    /// The token was never issued, or was released.
    UnknownToken,
    /// `len` is larger than [`MAX_CHUNK`].
    ChunkTooLarge,
    /// `offset + len` is past the end of the file (or overflows).
    OutOfRange,
    /// Reading failed (for example the file was truncated after registration).
    Io(io::ErrorKind),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(kind) => write!(f, "file not found or not accessible ({kind})"),
            Self::NotDarm => write!(f, "not a .{EXTENSION} file"),
            Self::NotRegularFile => write!(f, "not a regular file"),
            Self::TooManyOpen => write!(f, "too many files are open (limit {MAX_OPEN})"),
            Self::UnknownToken => write!(f, "unknown file token"),
            Self::ChunkTooLarge => write!(f, "chunk larger than {MAX_CHUNK} bytes"),
            Self::OutOfRange => write!(f, "read outside the file"),
            Self::Io(kind) => write!(f, "read failed ({kind})"),
        }
    }
}

impl std::error::Error for RegistryError {}

struct Entry {
    path: PathBuf,
    name: String,
    size: u64,
    file: Mutex<File>,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, Arc<Entry>>,
    /// Tokens the webview has not been told about yet, oldest first.
    pending: VecDeque<String>,
}

/// See the module documentation.
#[derive(Default)]
pub struct Registry {
    inner: Mutex<Inner>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A poisoned lock only means another thread panicked while holding it;
        // the maps are still structurally valid.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registers a file handed over by the operating system or picked by the
    /// user in a native dialog, and queues it for the webview.
    ///
    /// Registering a path that is already queued (not yet taken by the
    /// webview) returns the existing entry instead of queueing it twice.
    pub fn register(&self, path: &Path) -> Result<FileInfo, RegistryError> {
        let canonical = fs::canonicalize(path).map_err(|e| RegistryError::NotFound(e.kind()))?;

        let is_darm = canonical
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION));
        if !is_darm {
            return Err(RegistryError::NotDarm);
        }

        // Checked before opening so that a FIFO named `x.darm` cannot block us…
        let meta = fs::metadata(&canonical).map_err(|e| RegistryError::NotFound(e.kind()))?;
        if !meta.is_file() {
            return Err(RegistryError::NotRegularFile);
        }
        let file = File::open(&canonical).map_err(|e| RegistryError::NotFound(e.kind()))?;
        // …and again on the handle we will actually read from.
        let meta = file.metadata().map_err(|e| RegistryError::Io(e.kind()))?;
        if !meta.is_file() {
            return Err(RegistryError::NotRegularFile);
        }

        let name = canonical
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("book.{EXTENSION}"));

        let mut inner = self.lock();
        if let Some(existing) = inner.pending.iter().find_map(|token| {
            let entry = inner.entries.get(token)?;
            (entry.path == canonical).then(|| FileInfo {
                token: token.clone(),
                name: entry.name.clone(),
                size: entry.size,
            })
        }) {
            return Ok(existing);
        }
        if inner.entries.len() >= MAX_OPEN {
            return Err(RegistryError::TooManyOpen);
        }

        let mut token = new_token();
        while inner.entries.contains_key(&token) {
            token = new_token();
        }
        let info = FileInfo {
            token: token.clone(),
            name: name.clone(),
            size: meta.len(),
        };
        inner.entries.insert(
            token.clone(),
            Arc::new(Entry {
                path: canonical,
                name,
                size: meta.len(),
                file: Mutex::new(file),
            }),
        );
        inner.pending.push_back(token);
        Ok(info)
    }

    /// Hands the queued files to the webview, oldest first, and empties the queue.
    /// The tokens stay valid until [`Registry::release`].
    pub fn take_pending(&self) -> Vec<FileInfo> {
        let mut inner = self.lock();
        let tokens: Vec<String> = inner.pending.drain(..).collect();
        tokens
            .into_iter()
            .filter_map(|token| {
                let entry = inner.entries.get(&token)?;
                Some(FileInfo {
                    name: entry.name.clone(),
                    size: entry.size,
                    token,
                })
            })
            .collect()
    }

    /// Reads exactly `len` bytes at `offset` of a registered file.
    pub fn read_chunk(&self, token: &str, offset: u64, len: u64) -> Result<Vec<u8>, RegistryError> {
        if len > MAX_CHUNK {
            return Err(RegistryError::ChunkTooLarge);
        }
        // Clone the handle out so a slow read does not block other tokens.
        let entry = self
            .lock()
            .entries
            .get(token)
            .cloned()
            .ok_or(RegistryError::UnknownToken)?;
        let end = offset.checked_add(len).ok_or(RegistryError::OutOfRange)?;
        if end > entry.size {
            return Err(RegistryError::OutOfRange);
        }
        let mut buf = vec![0u8; len as usize];
        if len > 0 {
            let mut file = entry.file.lock().unwrap_or_else(|e| e.into_inner());
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| RegistryError::Io(e.kind()))?;
            file.read_exact(&mut buf)
                .map_err(|e| RegistryError::Io(e.kind()))?;
        }
        Ok(buf)
    }

    /// Forgets a token and closes its file. Returns whether the token existed.
    pub fn release(&self, token: &str) -> bool {
        let mut inner = self.lock();
        inner.pending.retain(|t| t != token);
        inner.entries.remove(token).is_some()
    }

    /// Number of files currently held (pending or being read).
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 128 random bits from the operating system, as 32 lowercase hex digits.
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system's random generator is unavailable");
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(b & 0x0f), 16).unwrap_or('0'));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::Write;

    /// A private scratch directory, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("darm-registry-test-{}", new_token()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            File::create(&path).unwrap().write_all(bytes).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    #[test]
    fn accepts_a_darm_file_and_reports_name_and_size_but_no_path() {
        let dir = Scratch::new();
        let path = dir.file("Atlas de mareas.darm", &sample(1000));
        let reg = Registry::new();
        let info = reg.register(&path).unwrap();
        assert_eq!(info.name, "Atlas de mareas.darm");
        assert_eq!(info.size, 1000);
        let json = format!("{:?}", info);
        assert!(
            !json.contains(dir.0.to_str().unwrap()),
            "the path must not leak: {json}"
        );
    }

    #[test]
    fn extension_check_is_case_insensitive() {
        let dir = Scratch::new();
        let reg = Registry::new();
        assert!(reg.register(&dir.file("UPPER.DARM", b"x")).is_ok());
        assert!(reg.register(&dir.file("Mixed.DaRm", b"x")).is_ok());
    }

    #[test]
    fn rejects_files_that_are_not_darm() {
        let dir = Scratch::new();
        let reg = Registry::new();
        for name in [
            "notes.txt",
            "book.darm.bak",
            "darm",
            "book.darmx",
            ".darm.zip",
            "passwd",
        ] {
            let path = dir.file(name, b"secret");
            assert!(
                matches!(reg.register(&path), Err(RegistryError::NotDarm)),
                "{name} must be rejected"
            );
        }
        assert!(reg.is_empty());
        assert!(reg.take_pending().is_empty());
    }

    #[test]
    fn rejects_directories_even_when_named_darm() {
        let dir = Scratch::new();
        let sub = dir.0.join("folder.darm");
        fs::create_dir(&sub).unwrap();
        let reg = Registry::new();
        assert!(matches!(
            reg.register(&sub),
            Err(RegistryError::NotRegularFile)
        ));
        assert!(matches!(reg.register(&dir.0), Err(RegistryError::NotDarm)));
        assert!(reg.is_empty());
    }

    #[test]
    fn rejects_missing_files() {
        let dir = Scratch::new();
        let reg = Registry::new();
        assert!(matches!(
            reg.register(&dir.0.join("nowhere.darm")),
            Err(RegistryError::NotFound(_))
        ));
        assert!(matches!(
            reg.register(Path::new("")),
            Err(RegistryError::NotFound(_))
        ));
        assert!(reg.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_judged_by_its_target() {
        let dir = Scratch::new();
        let secret = dir.file("id_ed25519", b"PRIVATE KEY");
        let disguised = dir.0.join("innocent.darm");
        std::os::unix::fs::symlink(&secret, &disguised).unwrap();
        let reg = Registry::new();
        assert!(matches!(
            reg.register(&disguised),
            Err(RegistryError::NotDarm)
        ));

        let real = dir.file("real.darm", b"book");
        let alias = dir.0.join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let info = reg.register(&alias).unwrap();
        assert_eq!(info.name, "real.darm");

        let dangling = dir.0.join("dangling.darm");
        std::os::unix::fs::symlink(dir.0.join("gone.darm"), &dangling).unwrap();
        assert!(matches!(
            reg.register(&dangling),
            Err(RegistryError::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_socket_named_darm() {
        let dir = Scratch::new();
        let path = dir.0.join("s.darm");
        // The socket path must stay short; fall back to skipping if the temp dir is too deep.
        let Ok(_listener) = std::os::unix::net::UnixListener::bind(&path) else {
            return;
        };
        let reg = Registry::new();
        assert!(matches!(
            reg.register(&path),
            Err(RegistryError::NotRegularFile)
        ));
    }

    #[test]
    fn reads_are_exact_and_reassemble_to_the_file() {
        let dir = Scratch::new();
        let bytes = sample(10_000);
        let reg = Registry::new();
        let info = reg.register(&dir.file("b.darm", &bytes)).unwrap();

        assert_eq!(reg.read_chunk(&info.token, 0, 10_000).unwrap(), bytes);
        assert_eq!(
            reg.read_chunk(&info.token, 9_999, 1).unwrap(),
            bytes[9_999..]
        );
        assert_eq!(
            reg.read_chunk(&info.token, 10_000, 0).unwrap(),
            Vec::<u8>::new()
        );

        let mut joined = Vec::new();
        let mut offset = 0u64;
        while offset < info.size {
            let len = 4096.min(info.size - offset);
            joined.extend(reg.read_chunk(&info.token, offset, len).unwrap());
            offset += len;
        }
        assert_eq!(joined, bytes);
        // Out of order works too.
        assert_eq!(
            reg.read_chunk(&info.token, 100, 50).unwrap(),
            bytes[100..150]
        );
    }

    #[test]
    fn rejects_reads_outside_the_file() {
        let dir = Scratch::new();
        let reg = Registry::new();
        let info = reg.register(&dir.file("b.darm", &sample(100))).unwrap();
        for (offset, len) in [
            (0, 101),
            (100, 1),
            (101, 0),
            (99, 2),
            (u64::MAX, 1),
            (u64::MAX - 3, 4),
            (1 << 40, 1),
        ] {
            assert!(
                matches!(
                    reg.read_chunk(&info.token, offset, len),
                    Err(RegistryError::OutOfRange)
                ),
                "offset {offset} len {len} must be out of range"
            );
        }
    }

    #[test]
    fn rejects_oversized_chunks_before_anything_else() {
        let dir = Scratch::new();
        let reg = Registry::new();
        let info = reg.register(&dir.file("b.darm", &sample(100))).unwrap();
        assert!(matches!(
            reg.read_chunk(&info.token, 0, MAX_CHUNK + 1),
            Err(RegistryError::ChunkTooLarge)
        ));
        assert!(matches!(
            reg.read_chunk(&info.token, 0, u64::MAX),
            Err(RegistryError::ChunkTooLarge)
        ));
        // Even for a token that does not exist: no allocation is attempted.
        assert!(matches!(
            reg.read_chunk("nope", 0, u64::MAX),
            Err(RegistryError::ChunkTooLarge)
        ));
    }

    #[test]
    fn a_full_size_chunk_is_allowed() {
        let dir = Scratch::new();
        let bytes = vec![7u8; MAX_CHUNK as usize + 5];
        let reg = Registry::new();
        let info = reg.register(&dir.file("big.darm", &bytes)).unwrap();
        let chunk = reg.read_chunk(&info.token, 5, MAX_CHUNK).unwrap();
        assert_eq!(chunk.len() as u64, MAX_CHUNK);
        assert!(chunk.iter().all(|&b| b == 7));
    }

    #[test]
    fn unknown_tokens_get_nothing() {
        let dir = Scratch::new();
        let reg = Registry::new();
        let info = reg.register(&dir.file("b.darm", &sample(10))).unwrap();
        let path = dir.0.join("b.darm");
        for token in [
            "",
            "0",
            "00000000000000000000000000000000",
            path.to_str().unwrap(),
            "../b.darm",
            &info.token[..31],
            &format!("{}0", info.token),
            &info.token.to_uppercase(),
        ] {
            if token == info.token {
                continue; // an all-digit token is its own upper-case form
            }
            assert!(
                matches!(
                    reg.read_chunk(token, 0, 1),
                    Err(RegistryError::UnknownToken)
                ),
                "{token:?} must be unknown"
            );
            assert!(!reg.release(token));
        }
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn take_pending_drains_the_queue_but_keeps_tokens_valid_until_release() {
        let dir = Scratch::new();
        let reg = Registry::new();
        let a = reg.register(&dir.file("a.darm", b"aaaa")).unwrap();
        let b = reg.register(&dir.file("b.darm", b"bb")).unwrap();

        assert_eq!(reg.take_pending(), vec![a.clone(), b.clone()]);
        assert!(reg.take_pending().is_empty());

        assert_eq!(reg.read_chunk(&a.token, 0, 4).unwrap(), b"aaaa");
        assert!(reg.release(&a.token));
        assert!(!reg.release(&a.token));
        assert!(matches!(
            reg.read_chunk(&a.token, 0, 4),
            Err(RegistryError::UnknownToken)
        ));
        assert_eq!(reg.read_chunk(&b.token, 0, 2).unwrap(), b"bb");
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn a_path_queued_twice_is_delivered_once_but_can_be_opened_again_later() {
        let dir = Scratch::new();
        let path = dir.file("a.darm", b"aaaa");
        let reg = Registry::new();
        let first = reg.register(&path).unwrap();
        let again = reg.register(&path).unwrap();
        assert_eq!(first, again);
        assert_eq!(reg.take_pending().len(), 1);

        let later = reg.register(&path).unwrap();
        assert_ne!(first.token, later.token);
        assert_eq!(reg.take_pending(), vec![later]);
    }

    #[test]
    fn releasing_a_pending_token_removes_it_from_the_queue() {
        let dir = Scratch::new();
        let reg = Registry::new();
        let a = reg.register(&dir.file("a.darm", b"a")).unwrap();
        assert!(reg.release(&a.token));
        assert!(reg.take_pending().is_empty());
        assert!(reg.is_empty());
    }

    #[test]
    fn the_number_of_open_files_is_bounded() {
        let dir = Scratch::new();
        let reg = Registry::new();
        let mut tokens = Vec::new();
        for i in 0..MAX_OPEN {
            tokens.push(
                reg.register(&dir.file(&format!("{i}.darm"), b"x"))
                    .unwrap()
                    .token,
            );
        }
        let extra = dir.file("extra.darm", b"x");
        assert!(matches!(
            reg.register(&extra),
            Err(RegistryError::TooManyOpen)
        ));
        assert!(reg.release(&tokens[0]));
        assert!(reg.register(&extra).is_ok());
    }

    #[test]
    fn the_token_keeps_pointing_at_the_registered_file_when_the_path_is_swapped() {
        let dir = Scratch::new();
        let path = dir.file("a.darm", b"original");
        let reg = Registry::new();
        let info = reg.register(&path).unwrap();
        fs::rename(&path, dir.0.join("moved.darm")).unwrap();
        dir.file("a.darm", b"imposter");
        assert_eq!(reg.read_chunk(&info.token, 0, 8).unwrap(), b"original");
    }

    #[test]
    fn a_file_truncated_after_registration_fails_cleanly() {
        let dir = Scratch::new();
        let path = dir.file("a.darm", &sample(100));
        let reg = Registry::new();
        let info = reg.register(&path).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(10)
            .unwrap();
        assert!(matches!(
            reg.read_chunk(&info.token, 0, 100),
            Err(RegistryError::Io(_))
        ));
        assert_eq!(reg.read_chunk(&info.token, 0, 10).unwrap(), sample(10));
    }

    #[test]
    fn tokens_are_128_random_bits_in_hex_and_never_repeat() {
        let mut seen = HashSet::new();
        let mut ones = 0u32;
        for _ in 0..4096 {
            let token = new_token();
            assert_eq!(token.len(), 32);
            assert!(
                token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
            ones += u128::from_str_radix(&token, 16).unwrap().count_ones();
            assert!(seen.insert(token), "a token repeated");
        }
        // 4096 × 128 fair bits: the mean is 262 144 and the standard deviation 362.
        // Eight standard deviations either way is a one-in-10^15 event for a sound generator.
        assert!(
            (259_248..=265_040).contains(&ones),
            "token bits look biased: {ones} ones"
        );
    }

    #[test]
    fn registered_tokens_are_unique_and_not_derived_from_the_file() {
        let dir = Scratch::new();
        let path = dir.file("a.darm", b"same");
        let reg = Registry::new();
        let mut seen = HashSet::new();
        for _ in 0..200 {
            let info = reg.register(&path).unwrap();
            reg.take_pending();
            assert!(
                seen.insert(info.token.clone()),
                "the same path got the same token twice"
            );
            reg.release(&info.token);
        }
    }

    #[test]
    fn concurrent_reads_of_different_tokens_do_not_interfere() {
        let dir = Scratch::new();
        let reg = Arc::new(Registry::new());
        let files: Vec<(FileInfo, Vec<u8>)> = (0..8)
            .map(|i| {
                let bytes: Vec<u8> = sample(50_000)
                    .into_iter()
                    .map(|b| b.wrapping_add(i))
                    .collect();
                (
                    reg.register(&dir.file(&format!("{i}.darm"), &bytes))
                        .unwrap(),
                    bytes,
                )
            })
            .collect();
        let handles: Vec<_> = files
            .into_iter()
            .map(|(info, bytes)| {
                let reg = Arc::clone(&reg);
                std::thread::spawn(move || {
                    for round in 0..50u64 {
                        let offset = round * 997 % 40_000;
                        let got = reg.read_chunk(&info.token, offset, 5_000).unwrap();
                        assert_eq!(got, bytes[offset as usize..offset as usize + 5_000]);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }
}
