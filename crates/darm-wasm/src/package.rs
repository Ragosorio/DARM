//! JavaScript bindings for reading `.darm` packages.
//!
//! The binding is deliberately I/O-free: JavaScript owns the file (a `File`,
//! an OPFS handle, an HTTP range reader…) and feeds bytes in. The flow is:
//!
//! 1. Read the first [`headerLen()`](header_len) bytes (128) — or, to get the
//!    public card in the same call, the first 65 664 bytes (or the whole file
//!    if it is smaller) — and call [`probe`]. It returns `headLen`, the number
//!    of leading bytes that hold the header, prelude and manifest.
//! 2. Read the first `headLen` bytes and call `DarmPackage.open(head, password?, key?)`.
//! 3. For any chunk, read `chunkLen(entry, chunk)` bytes at
//!    `chunkOffset(entry, chunk)` and pass them to `decodeChunk`, which
//!    verifies (SHA-256 or AES-GCM tag), decrypts and inflates them.
//!
//! Every failure is thrown as a JavaScript `Error` whose `message` is
//! `"<CODE>: <human readable message>"` and whose `code` property is the
//! stable `E_PKG_…` code of [`darm_package::PackageError::code`].

use darm_package::{
    Credentials, HEADER_LEN, KeyFile, PackageError, PackageHead, Probe, format_uuid,
};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
/** Result of `JSON.parse(probe(head))`. Nested objects use the on-disk (snake_case) member names. */
export interface DarmProbe {
  /** Format version of the file, e.g. "1.0". */
  format: string;
  encrypted: boolean;
  /** Package UUID, canonical 8-4-4-4-12 form. */
  uuid: string;
  /** Declared length of the whole file; compare it with the real file size. */
  totalLen: number;
  /** Leading bytes (header + prelude + manifest) to pass to `DarmPackage.open`. */
  headLen: number;
  /** False when `head` was too short to contain the prelude; then `card`, `slots`, `kdf`, `generator` and `chunkSize` are unknown. */
  preludeLoaded: boolean;
  generator: string | null;
  chunkSize: number | null;
  card: { title: string; authors: string[]; language?: string; cover_color?: string } | null;
  slots: Array<{ kind: "password" | "keyfile" | "unknown"; key_id?: string; kdf?: DarmKdf }>;
  /** Argon2id parameters of the first password slot, if any. */
  kdf: DarmKdf | null;
}
export interface DarmKdf { algorithm: "argon2id"; version: number; m_kib: number; t: number; p: number }

/** Result of `JSON.parse(pkg.manifestJson())`. */
export interface DarmManifest {
  manifest_version: number;
  created?: string;
  /** Opaque summary supplied by the tool that built the book. */
  book: unknown;
  entries: DarmEntry[];
}
export interface DarmEntry {
  index: number;
  name: string;
  media_type: string;
  role: "ir" | "resource";
  /** Plaintext size in bytes. */
  size: number;
  /** Hex SHA-256 of the plaintext. */
  sha256: string;
  compression: "none" | "deflate";
  /** Offset of the first stored byte, relative to `pkg.dataOffset()`. */
  offset: number;
  stored_size: number;
  chunks: Array<{ len: number; sha256?: string }>;
}
"#;

/// Converts a package error into a JS `Error` with a `code` property.
fn js_error(error: &PackageError) -> JsValue {
    let code = error.code();
    let js = js_sys::Error::new(&format!("{code}: {error}"));
    // Setting a property on a freshly created Error object cannot fail.
    let _ = js_sys::Reflect::set(&js, &JsValue::from_str("code"), &JsValue::from_str(code));
    js.into()
}

fn throw<T>(result: Result<T, PackageError>) -> Result<T, JsValue> {
    result.map_err(|error| js_error(&error))
}

/// Builds the JSON document returned by [`probe`].
fn probe_value(probe: &Probe) -> Value {
    let kdf = |params: &darm_package::KdfParams| {
        json!({
            "algorithm": "argon2id",
            "version": 19,
            "m_kib": params.m_kib,
            "t": params.t,
            "p": params.p,
        })
    };
    let slots: Vec<Value> = probe
        .slots
        .iter()
        .map(|slot| {
            let mut value = json!({ "kind": slot.kind.as_str() });
            if let Some(key_id) = &slot.key_id {
                value["key_id"] = json!(key_id);
            }
            if let Some(params) = &slot.kdf {
                value["kdf"] = kdf(params);
            }
            value
        })
        .collect();
    json!({
        "format": probe.version(),
        "encrypted": probe.encrypted,
        "uuid": format_uuid(&probe.uuid),
        "totalLen": probe.total_len,
        "headLen": probe.head_len,
        "preludeLoaded": probe.prelude_loaded,
        "generator": probe.generator,
        "chunkSize": probe.chunk_size,
        "card": probe.card,
        "slots": slots,
        "kdf": probe.kdf.as_ref().map(kdf),
    })
}

/// Pure (JS-free) implementation of [`probe`], so it can be unit-tested natively.
pub fn probe_json(head: &[u8]) -> Result<String, PackageError> {
    Ok(probe_value(&darm_package::probe(head)?).to_string())
}

/// Size in bytes of the fixed header: the minimum number of leading bytes
/// [`probe`] needs to report `headLen`.
#[wasm_bindgen(js_name = headerLen)]
pub fn header_len() -> u32 {
    HEADER_LEN as u32
}

/// Inspects the first bytes of a package without any key and returns a JSON
/// string (see the `DarmProbe` TypeScript interface).
///
/// `head` must hold at least `headerLen()` bytes. `card`, `slots` and `kdf`
/// are only filled in when `head` also contains the whole prelude
/// (`preludeLoaded` is then `true`); 65 664 bytes always suffice.
#[wasm_bindgen]
pub fn probe(head: &[u8]) -> Result<String, JsValue> {
    throw(probe_json(head))
}

/// Parses the text of a `.darmkey` file and returns its raw 32-byte key.
#[wasm_bindgen(js_name = parseKeyfile)]
pub fn parse_keyfile(text: &str) -> Result<Vec<u8>, JsValue> {
    let keyfile = throw(KeyFile::parse(text))?;
    Ok(keyfile.key().to_vec())
}

/// An opened `.darm` package (header, prelude, manifest and, when encrypted,
/// the unlocked data key). Call `free()` when done to release its memory and
/// wipe the key.
#[wasm_bindgen(js_name = DarmPackage)]
pub struct DarmPackage {
    head: PackageHead,
}

/// Opens with the given credentials. Pure Rust so it can be tested natively.
///
/// With both a key and a password, the key is tried first. A password that
/// fails is retried in Unicode NFC form when `nfc` yields a different string,
/// so a book locked with a precomposed "é" opens when the reader's keyboard
/// produced "e" + combining accent.
fn open_with(
    head: &[u8],
    password: Option<&str>,
    key: Option<&[u8]>,
    nfc: impl FnOnce(&str) -> Option<String>,
) -> Result<PackageHead, PackageError> {
    let key: Option<Zeroizing<[u8; 32]>> = match key {
        None => None,
        Some(bytes) => Some(Zeroizing::new(<[u8; 32]>::try_from(bytes).map_err(
            |_| {
                PackageError::BadOption(format!(
                    "a key must be exactly 32 bytes, got {}",
                    bytes.len()
                ))
            },
        )?)),
    };
    if let Some(key) = &key {
        match PackageHead::open(head, &Credentials::Key(key)) {
            Err(PackageError::WrongCredentials) if password.is_some() => {}
            other => return other,
        }
    }
    let Some(password) = password else {
        return PackageHead::open(head, &Credentials::None);
    };
    match PackageHead::open(head, &Credentials::Password(password)) {
        Err(PackageError::WrongCredentials) => match nfc(password).map(Zeroizing::new) {
            Some(normalized) if normalized.as_str() != password => {
                PackageHead::open(head, &Credentials::Password(&normalized))
            }
            _ => Err(PackageError::WrongCredentials),
        },
        other => other,
    }
}

/// Unicode NFC normalisation via the host's `String.prototype.normalize`.
fn js_nfc(text: &str) -> Option<String> {
    // "NFC" is always a valid form, so `normalize` cannot throw here.
    js_sys::JsString::from(text).normalize("NFC").as_string()
}

#[wasm_bindgen(js_class = DarmPackage)]
impl DarmPackage {
    /// Opens a package from its head: the first `headLen` bytes of the file
    /// (extra bytes are ignored).
    ///
    /// * Clear package: pass no credentials.
    /// * Encrypted package: pass `password`, or `key` (the 32 bytes returned
    ///   by `parseKeyfile`), or both (the key is tried first).
    ///
    /// Throws `E_PKG_CREDENTIALS_REQUIRED` when the package is encrypted and
    /// no credentials were given, `E_PKG_WRONG_CREDENTIALS` when they do not
    /// unlock it. Unlocking with a password runs Argon2id and can take a
    /// noticeable fraction of a second and 64 MiB of memory; call it from a
    /// worker.
    pub fn open(
        head: &[u8],
        password: Option<String>,
        key: Option<Vec<u8>>,
    ) -> Result<DarmPackage, JsValue> {
        let password = password.map(Zeroizing::new);
        let key = key.map(Zeroizing::new);
        let head = throw(open_with(
            head,
            password.as_ref().map(|p| p.as_str()),
            key.as_ref().map(|k| k.as_slice()),
            js_nfc,
        ))?;
        Ok(DarmPackage { head })
    }

    /// The manifest as a JSON string (see the `DarmManifest` TypeScript interface).
    #[wasm_bindgen(js_name = manifestJson)]
    pub fn manifest_json(&self) -> Result<String, JsValue> {
        throw(self.head.manifest_json())
    }

    /// Number of entries.
    #[wasm_bindgen(js_name = entryCount)]
    pub fn entry_count(&self) -> u32 {
        self.head.entries().len() as u32
    }

    /// Index of the entry called `name`, or -1 if there is none.
    #[wasm_bindgen(js_name = entryIndex)]
    pub fn entry_index(&self, name: &str) -> i32 {
        self.head
            .entry_by_name(name)
            .map_or(-1, |entry| entry.index as i32)
    }

    /// Number of chunks of entry `entry`.
    #[wasm_bindgen(js_name = chunkCount)]
    pub fn chunk_count(&self, entry: u32) -> Result<u32, JsValue> {
        throw(self.head.chunk_count(entry))
    }

    /// Absolute file offset of the stored bytes of a chunk.
    #[wasm_bindgen(js_name = chunkOffset)]
    pub fn chunk_offset(&self, entry: u32, chunk: u32) -> Result<f64, JsValue> {
        // Offsets are below 2^36 (64 GiB), so the conversion to f64 is exact.
        throw(self.head.chunk_location(entry, chunk)).map(|(offset, _)| offset as f64)
    }

    /// Stored length in bytes of a chunk.
    #[wasm_bindgen(js_name = chunkLen)]
    pub fn chunk_len(&self, entry: u32, chunk: u32) -> Result<u32, JsValue> {
        throw(self.head.chunk_location(entry, chunk)).map(|(_, len)| len)
    }

    /// Verifies and decodes the stored bytes of a chunk (exactly `chunkLen`
    /// bytes read at `chunkOffset`) and returns its plaintext. Throws
    /// `E_PKG_INTEGRITY` if the bytes are corrupt or were tampered with.
    #[wasm_bindgen(js_name = decodeChunk)]
    pub fn decode_chunk(&self, entry: u32, chunk: u32, stored: &[u8]) -> Result<Vec<u8>, JsValue> {
        throw(self.head.decode_chunk(entry, chunk, stored))
    }

    /// Offset of the data area (equals `headLen`). An uncompressed entry of a
    /// clear package occupies `dataOffset() + entry.offset` up to
    /// `+ entry.size` verbatim, so it can be sliced from the file directly
    /// (for example to hand a video to the browser) — but bytes read that
    /// way are not verified.
    #[wasm_bindgen(js_name = dataOffset)]
    pub fn data_offset(&self) -> f64 {
        self.head.head_len() as f64
    }

    /// Length of the whole file as declared by its header. The caller should
    /// compare it with the real file size: a mismatch means the file is
    /// truncated or has trailing data.
    #[wasm_bindgen(js_name = totalLen)]
    pub fn total_len(&self) -> f64 {
        self.head.total_len() as f64
    }

    /// Whether the package is encrypted.
    #[wasm_bindgen(js_name = isEncrypted)]
    pub fn is_encrypted(&self) -> bool {
        self.head.is_encrypted()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../darm-package/tests/fixtures"
    );
    const KEY: [u8; 32] = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad,
        0xae, 0xaf,
    ];

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{FIXTURES}/{name}")).unwrap()
    }

    fn no_nfc(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn probe_json_shape() {
        let bytes = fixture("golden-v1.darm");
        let value: Value = serde_json::from_str(&probe_json(&bytes).unwrap()).unwrap();
        assert_eq!(value["format"], "1.0");
        assert_eq!(value["encrypted"], false);
        assert_eq!(value["uuid"], "7dcd6c88-abac-85ae-81d8-778e6b427bdc");
        assert_eq!(value["totalLen"], bytes.len());
        assert_eq!(value["preludeLoaded"], true);
        assert_eq!(value["card"]["title"], "The Golden Fixture");
        assert_eq!(value["card"]["cover_color"], "#c9a227");
        assert_eq!(value["slots"], json!([]));
        assert_eq!(value["kdf"], Value::Null);

        let header_only: Value =
            serde_json::from_str(&probe_json(&bytes[..HEADER_LEN]).unwrap()).unwrap();
        assert_eq!(header_only["preludeLoaded"], false);
        assert_eq!(header_only["card"], Value::Null);
        assert_eq!(header_only["headLen"], value["headLen"]);

        let encrypted = fixture("encrypted-v1.darm");
        let value: Value = serde_json::from_str(&probe_json(&encrypted).unwrap()).unwrap();
        assert_eq!(value["encrypted"], true);
        assert_eq!(value["slots"][0]["kind"], "password");
        assert_eq!(value["slots"][1]["kind"], "keyfile");
        assert_eq!(value["slots"][1]["key_id"], darm_package::key_id(&KEY));
        assert_eq!(
            value["kdf"],
            json!({"algorithm": "argon2id", "version": 19, "m_kib": 8192, "t": 1, "p": 1})
        );

        assert_eq!(probe_json(b"nope").unwrap_err().code(), "E_PKG_BAD_MAGIC");
    }

    #[test]
    fn credential_selection() {
        let clear = fixture("golden-v1.darm");
        assert!(open_with(&clear, None, None, no_nfc).is_ok());

        let encrypted = fixture("encrypted-v1.darm");
        let code =
            |result: Result<PackageHead, PackageError>| result.map(|_| ()).map_err(|e| e.code());
        assert_eq!(
            code(open_with(&encrypted, None, None, no_nfc)),
            Err("E_PKG_CREDENTIALS_REQUIRED")
        );
        assert_eq!(
            code(open_with(&encrypted, Some("golden-password"), None, no_nfc)),
            Ok(())
        );
        assert_eq!(
            code(open_with(&encrypted, None, Some(&KEY), no_nfc)),
            Ok(())
        );
        assert_eq!(
            code(open_with(&encrypted, Some("wrong"), None, no_nfc)),
            Err("E_PKG_WRONG_CREDENTIALS")
        );
        assert_eq!(
            code(open_with(&encrypted, None, Some(&[0u8; 32]), no_nfc)),
            Err("E_PKG_WRONG_CREDENTIALS")
        );
        // Wrong key but right password, and the other way round: either suffices.
        assert_eq!(
            code(open_with(
                &encrypted,
                Some("golden-password"),
                Some(&[0u8; 32]),
                no_nfc
            )),
            Ok(())
        );
        assert_eq!(
            code(open_with(&encrypted, Some("wrong"), Some(&KEY), no_nfc)),
            Ok(())
        );
        assert_eq!(
            code(open_with(&encrypted, None, Some(&[0u8; 31]), no_nfc)),
            Err("E_PKG_BAD_OPTION")
        );
        // The NFC retry: a "normaliser" that maps the typed text to the real password.
        let nfc = |_: &str| Some("golden-password".to_string());
        assert_eq!(
            code(open_with(
                &encrypted,
                Some("golden-password-nfd"),
                None,
                nfc
            )),
            Ok(())
        );
    }
}
