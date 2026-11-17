//! The `.darmkey` key file: a 32-byte random key kept outside the book.
//!
//! A `.darmkey` file is UTF-8 JSON text of at most 4096 bytes:
//!
//! ```json
//! {
//!   "darmkey": 1,
//!   "key_id": "3f0c9a6e5d2b8e71",
//!   "key": "q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=",
//!   "created": "2026-11-16T09:30:00Z"
//! }
//! ```
//!
//! * `darmkey` — format version of the key file, `1`.
//! * `key` — the 32 key bytes, standard base64 with padding.
//! * `key_id` — optional; 16 lower-case hex digits, the first 8 bytes of
//!   `SHA-256("darm/1/key-id\0" || key)`. Packages store the same identifier
//!   in their keyfile slots so a reader can tell which key a book needs.
//!   When present it must match the key.
//! * `created` — optional free-form creation time (RFC 3339 recommended).
//!
//! Unknown members are ignored.

use base64ct::{Base64, Encoding};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::key_id_bytes;
use crate::error::{PackageError, Result};
use crate::limits::MAX_KEYFILE_LEN;
use crate::util::hex_encode;

/// Version number written in the `darmkey` member.
pub const KEYFILE_VERSION: u32 = 1;

/// Conventional file extension of key files.
pub const KEYFILE_EXTENSION: &str = "darmkey";

/// A parsed or freshly generated key file.
#[derive(Clone)]
pub struct KeyFile {
    key: Zeroizing<[u8; 32]>,
    created: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct KeyFileJson {
    darmkey: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_id: Option<String>,
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<String>,
}

impl Drop for KeyFileJson {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl std::fmt::Debug for KeyFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyFile")
            .field("key_id", &self.key_id())
            .field("created", &self.created)
            .finish_non_exhaustive()
    }
}

/// Identifier of a key: 16 hex digits derived from (but not revealing) the key.
pub fn key_id(key: &[u8; 32]) -> String {
    hex_encode(&key_id_bytes(key))
}

impl KeyFile {
    /// Wraps existing key bytes.
    pub fn from_key(key: [u8; 32], created: Option<String>) -> KeyFile {
        KeyFile {
            key: Zeroizing::new(key),
            created,
        }
    }

    /// The 32-byte key.
    pub fn key(&self) -> &[u8; 32] {
        &self.key
    }

    /// Identifier of the key (see [`key_id`]).
    pub fn key_id(&self) -> String {
        key_id(&self.key)
    }

    /// Creation time recorded in the file, if any.
    pub fn created(&self) -> Option<&str> {
        self.created.as_deref()
    }

    /// Parses the text of a `.darmkey` file.
    pub fn parse(text: &str) -> Result<KeyFile> {
        if text.len() > MAX_KEYFILE_LEN {
            return Err(PackageError::BadKeyfile(format!(
                "the file is larger than {MAX_KEYFILE_LEN} bytes"
            )));
        }
        let json: KeyFileJson = serde_json::from_str(text).map_err(|e| {
            // Only the position is reported: serde_json messages can echo
            // values, and nothing from a key file may end up in a log.
            PackageError::BadKeyfile(format!(
                "it is not a .darmkey JSON document (problem at line {}, column {})",
                e.line(),
                e.column()
            ))
        })?;
        if json.darmkey != KEYFILE_VERSION {
            return Err(PackageError::BadKeyfile(format!(
                "unsupported key file version {} (this reader understands version {KEYFILE_VERSION})",
                json.darmkey
            )));
        }
        let mut key = Zeroizing::new([0u8; 32]);
        let decoded_ok = json.key.len() == 44
            && matches!(Base64::decode(json.key.as_bytes(), key.as_mut_slice()), Ok(d) if d.len() == 32);
        if !decoded_ok {
            return Err(PackageError::BadKeyfile(
                "the key is not base64 of exactly 32 bytes".into(),
            ));
        }
        if let Some(declared) = &json.key_id
            && *declared != key_id(&key)
        {
            return Err(PackageError::BadKeyfile(
                "key_id does not match the key (the file is damaged)".into(),
            ));
        }
        Ok(KeyFile {
            key,
            created: json.created.clone(),
        })
    }

    /// Serialises the key file to its text form (pretty JSON ending in a newline).
    pub fn to_text(&self) -> String {
        let json = KeyFileJson {
            darmkey: KEYFILE_VERSION,
            key_id: Some(self.key_id()),
            key: Base64::encode_string(self.key.as_slice()),
            created: self.created.clone(),
        };
        let mut text = serde_json::to_string_pretty(&json)
            .expect("a struct of strings and integers always serialises");
        text.push('\n');
        text
    }
}

/// Generates a new random key file. The creation time is taken from the system clock.
#[cfg(feature = "write")]
pub fn generate_keyfile() -> Result<KeyFile> {
    let mut key = Zeroizing::new([0u8; 32]);
    getrandom::fill(key.as_mut_slice()).map_err(|e| PackageError::Random(e.to_string()))?;
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|elapsed| rfc3339_utc(elapsed.as_secs()));
    Ok(KeyFile { key, created })
}

/// Formats seconds since the Unix epoch as an RFC 3339 UTC timestamp.
#[cfg(feature = "write")]
fn rfc3339_utc(unix_seconds: u64) -> String {
    let days = unix_seconds / 86_400;
    let secs = unix_seconds % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm), valid for the whole u64 range we can get here.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip() {
        let keyfile = KeyFile::from_key([0x42; 32], Some("2026-01-02T03:04:05Z".into()));
        let text = keyfile.to_text();
        assert!(text.ends_with("}\n"));
        let parsed = KeyFile::parse(&text).unwrap();
        assert_eq!(parsed.key(), keyfile.key());
        assert_eq!(parsed.key_id(), keyfile.key_id());
        assert_eq!(parsed.created(), Some("2026-01-02T03:04:05Z"));
        assert_eq!(keyfile.key_id().len(), 16);
    }

    #[test]
    fn debug_does_not_print_the_key() {
        let keyfile = KeyFile::from_key([0x42; 32], None);
        let debug = format!("{keyfile:?}");
        assert!(!debug.contains("66"), "{debug}");
        assert!(!debug.contains("QkJC"), "{debug}");
    }

    #[test]
    fn rejects_malformed_files() {
        let good = KeyFile::from_key([1; 32], None).to_text();
        let cases = [
            String::new(),
            "not json".to_string(),
            good.replace("\"darmkey\": 1", "\"darmkey\": 2"),
            good.replace("\"key\": \"AQEB", "\"key\": \"AQE"),
            good.replace("\"key\": \"AQEB", "\"key\": \"!QEB"),
            good.replace("\"key\": \"AQEB", "\"key\": \"BQEB"),
            format!("{}{}", good, " ".repeat(MAX_KEYFILE_LEN)),
        ];
        for case in cases {
            let error = KeyFile::parse(&case).unwrap_err();
            assert_eq!(error.code(), "E_PKG_BAD_KEYFILE", "{case}");
        }
    }

    #[test]
    fn minimal_file_without_optional_members() {
        let text =
            r#"{"darmkey":1,"key":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=","note":"x"}"#;
        let keyfile = KeyFile::parse(text).unwrap();
        assert_eq!(keyfile.key(), &[1u8; 32]);
        assert_eq!(keyfile.created(), None);
    }

    #[cfg(feature = "write")]
    #[test]
    fn generated_keys_differ_and_carry_a_timestamp() {
        let a = generate_keyfile().unwrap();
        let b = generate_keyfile().unwrap();
        assert_ne!(a.key(), b.key());
        let created = a.created().unwrap();
        assert_eq!(created.len(), 20);
        assert!(created.ends_with('Z'));
        assert!(KeyFile::parse(&a.to_text()).is_ok());
    }

    #[cfg(feature = "write")]
    #[test]
    fn timestamps() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(1_794_821_696), "2026-11-16T09:34:56Z");
        assert_eq!(rfc3339_utc(4_102_444_799), "2099-12-31T23:59:59Z");
    }
}
