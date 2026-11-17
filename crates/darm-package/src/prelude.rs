//! The clear-text prelude: everything a reader may know without the key.

use base64ct::{Base64, Encoding};
use serde::{Deserialize, Serialize};

use crate::error::{PackageError, Result};
use crate::header::Header;
use crate::limits::{
    GCM_TAG_LEN, KDF_MAX_M_KIB, KDF_MAX_P, KDF_MAX_T, KDF_MIN_M_KIB, KDF_MIN_P, KDF_MIN_T,
    MAX_CHUNK_SIZE, MAX_KEY_SLOTS, MAX_LABEL_LEN, MIN_CHUNK_SIZE,
};
use crate::util::hex_decode;

/// Cipher identifier written into the prelude of encrypted packages.
pub const CIPHER_AES_256_GCM: &str = "AES-256-GCM";

/// Sub-key derivation identifier written into the prelude of encrypted packages.
pub const SUBKEYS_HKDF_SHA256: &str = "HKDF-SHA256";

/// Password KDF identifier.
pub const KDF_ARGON2ID: &str = "argon2id";

/// Argon2 version number (0x13) stored with password slots.
pub const ARGON2_VERSION: u32 = 0x13;

/// Length in bytes of a password slot salt.
pub const SALT_LEN: usize = 16;

/// Length in bytes of a wrapped content key (32-byte key + GCM tag).
pub const WRAPPED_KEY_LEN: usize = 32 + GCM_TAG_LEN;

/// What a library may show for a book it cannot (yet) unlock.
///
/// For encrypted packages the card is only stored when the author opted in.
/// It is covered by the prelude hash, and for encrypted packages it is also
/// authenticated once the package is unlocked.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PublicCard {
    /// Book title.
    pub title: String,
    /// Author names, in display order.
    #[serde(default)]
    pub authors: Vec<String>,
    /// BCP 47 language tag of the book, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Cover colour as `#rrggbb`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_color: Option<String>,
}

impl PublicCard {
    /// Checks the card's invariants (currently: `cover_color` is `#rrggbb`).
    pub fn validate(&self) -> Result<()> {
        if let Some(color) = &self.cover_color {
            let bytes = color.as_bytes();
            let ok = bytes.len() == 7
                && bytes[0] == b'#'
                && bytes[1..].iter().all(|b| b.is_ascii_hexdigit());
            if !ok {
                return Err(PackageError::prelude(
                    "the card cover_color must have the form #rrggbb",
                ));
            }
        }
        Ok(())
    }
}

/// Argon2id cost parameters of a password slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m_kib: u32,
    /// Number of passes.
    pub t: u32,
    /// Number of lanes.
    pub p: u32,
}

impl KdfParams {
    /// Parameters used by the writer by default: 64 MiB, 3 passes, 1 lane.
    pub const DEFAULT: KdfParams = KdfParams {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };

    /// The weakest parameters the format accepts: 8 MiB, 1 pass, 1 lane.
    /// Meant for tests; do not protect real books with it.
    pub const MIN: KdfParams = KdfParams {
        m_kib: KDF_MIN_M_KIB,
        t: KDF_MIN_T,
        p: KDF_MIN_P,
    };

    /// Checks the parameters against the bounds the format allows
    /// (m in 8 MiB..=1 GiB, t in 1..=16, p in 1..=4).
    pub fn validate(&self) -> Result<()> {
        let check = |what, value: u32, min: u32, max: u32| {
            if value < min || value > max {
                Err(PackageError::limit(
                    what,
                    u64::from(value),
                    u64::from(min),
                    u64::from(max),
                ))
            } else {
                Ok(())
            }
        };
        check(
            "Argon2id memory cost (KiB)",
            self.m_kib,
            KDF_MIN_M_KIB,
            KDF_MAX_M_KIB,
        )?;
        check("Argon2id pass count", self.t, KDF_MIN_T, KDF_MAX_T)?;
        check("Argon2id lane count", self.p, KDF_MIN_P, KDF_MAX_P)
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        KdfParams::DEFAULT
    }
}

/// The `kdf` object of a password slot as stored in the prelude.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotKdf {
    /// KDF name; `"argon2id"` in format 1.0.
    pub algorithm: String,
    /// KDF version; `19` (0x13) in format 1.0.
    pub version: u32,
    /// Memory cost in KiB.
    pub m_kib: u32,
    /// Number of passes.
    pub t: u32,
    /// Number of lanes.
    pub p: u32,
}

impl SlotKdf {
    /// The cost parameters.
    pub fn params(&self) -> KdfParams {
        KdfParams {
            m_kib: self.m_kib,
            t: self.t,
            p: self.p,
        }
    }

    fn is_supported(&self) -> bool {
        self.algorithm == KDF_ARGON2ID && self.version == ARGON2_VERSION
    }
}

/// Kind of a key slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// Unlocked with a password (Argon2id).
    Password,
    /// Unlocked with the 32-byte key of a `.darmkey` file.
    Keyfile,
    /// A slot kind (or KDF) introduced by a newer minor version; ignored by this reader.
    Unknown,
}

impl SlotKind {
    /// The kind as written in the prelude (`"password"`, `"keyfile"`) or `"unknown"`.
    pub fn as_str(self) -> &'static str {
        match self {
            SlotKind::Password => "password",
            SlotKind::Keyfile => "keyfile",
            SlotKind::Unknown => "unknown",
        }
    }
}

/// One way of unwrapping the content key, as stored in the prelude.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySlot {
    /// `"password"` or `"keyfile"`.
    pub kind: String,
    /// Password slots: KDF and its parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kdf: Option<SlotKdf>,
    /// Password slots: base64 of the 16-byte salt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salt: Option<String>,
    /// Keyfile slots: identifier of the key (16 hex digits).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    /// Base64 of the wrapped content key (48 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapped_key: Option<String>,
}

/// A key slot with its binary fields decoded.
pub(crate) enum DecodedSlot {
    Password {
        params: KdfParams,
        salt: [u8; SALT_LEN],
        wrapped: [u8; WRAPPED_KEY_LEN],
    },
    Keyfile {
        key_id: [u8; 8],
        wrapped: [u8; WRAPPED_KEY_LEN],
    },
    Unknown,
}

fn decode_b64<const N: usize>(field: &'static str, text: Option<&String>) -> Result<[u8; N]> {
    let text =
        text.ok_or_else(|| PackageError::prelude(format!("the {field} field is missing")))?;
    let mut out = [0u8; N];
    // Reject before decoding anything that cannot be exactly N bytes.
    if text.len() != N.div_ceil(3) * 4 {
        return Err(PackageError::prelude(format!(
            "the {field} field has the wrong length"
        )));
    }
    match Base64::decode(text.as_bytes(), &mut out) {
        Ok(decoded) if decoded.len() == N => Ok(out),
        _ => Err(PackageError::prelude(format!(
            "the {field} field is not valid base64 of {N} bytes"
        ))),
    }
}

impl KeySlot {
    /// The slot kind as understood by this reader.
    pub fn slot_kind(&self) -> SlotKind {
        match self.kind.as_str() {
            "password" => match &self.kdf {
                Some(kdf) if !kdf.is_supported() => SlotKind::Unknown,
                _ => SlotKind::Password,
            },
            "keyfile" => SlotKind::Keyfile,
            _ => SlotKind::Unknown,
        }
    }

    /// Decodes and validates the slot. Slots of an unknown kind decode to
    /// [`DecodedSlot::Unknown`] so newer minor versions can add kinds.
    pub(crate) fn decode(&self) -> Result<DecodedSlot> {
        match self.slot_kind() {
            SlotKind::Unknown => Ok(DecodedSlot::Unknown),
            SlotKind::Password => {
                let kdf = self.kdf.as_ref().ok_or_else(|| {
                    PackageError::prelude("a password key slot is missing its kdf field")
                })?;
                let params = kdf.params();
                params.validate()?;
                Ok(DecodedSlot::Password {
                    params,
                    salt: decode_b64("salt", self.salt.as_ref())?,
                    wrapped: decode_b64("wrapped_key", self.wrapped_key.as_ref())?,
                })
            }
            SlotKind::Keyfile => {
                let key_id = self
                    .key_id
                    .as_deref()
                    .and_then(hex_decode::<8>)
                    .ok_or_else(|| {
                        PackageError::prelude(
                            "a keyfile key slot has a missing or malformed key_id",
                        )
                    })?;
                Ok(DecodedSlot::Keyfile {
                    key_id,
                    wrapped: decode_b64("wrapped_key", self.wrapped_key.as_ref())?,
                })
            }
        }
    }

    #[cfg(feature = "write")]
    pub(crate) fn new_password(
        params: KdfParams,
        salt: &[u8; SALT_LEN],
        wrapped: &[u8; WRAPPED_KEY_LEN],
    ) -> KeySlot {
        KeySlot {
            kind: SlotKind::Password.as_str().into(),
            kdf: Some(SlotKdf {
                algorithm: KDF_ARGON2ID.into(),
                version: ARGON2_VERSION,
                m_kib: params.m_kib,
                t: params.t,
                p: params.p,
            }),
            salt: Some(Base64::encode_string(salt)),
            key_id: None,
            wrapped_key: Some(Base64::encode_string(wrapped)),
        }
    }

    #[cfg(feature = "write")]
    pub(crate) fn new_keyfile(key_id: &[u8; 8], wrapped: &[u8; WRAPPED_KEY_LEN]) -> KeySlot {
        KeySlot {
            kind: SlotKind::Keyfile.as_str().into(),
            kdf: None,
            salt: None,
            key_id: Some(crate::util::hex_encode(key_id)),
            wrapped_key: Some(Base64::encode_string(wrapped)),
        }
    }
}

/// The `encryption` object of the prelude.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptionInfo {
    /// AEAD used for the manifest and the chunks; `"AES-256-GCM"`.
    pub cipher: String,
    /// How sub-keys are derived from the content key; `"HKDF-SHA256"`.
    pub subkeys: String,
    /// Base64 of the 32-byte commitment to the content key
    /// (`HKDF-SHA256(salt = UUID, ikm = CEK, info = "darm/1/key-commitment")`).
    pub key_commitment: String,
    /// The key slots (1 to 8).
    pub slots: Vec<KeySlot>,
}

impl EncryptionInfo {
    /// Decodes the content-key commitment.
    pub(crate) fn commitment(&self) -> Result<[u8; 32]> {
        decode_b64("key_commitment", Some(&self.key_commitment))
    }
}

/// The clear-text prelude of a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prelude {
    /// Free-form name and version of the tool that wrote the package.
    pub generator: String,
    /// Plaintext chunk size in bytes used by every entry.
    pub chunk_size: u32,
    /// The public card, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<PublicCard>,
    /// Encryption parameters; present exactly when the package is encrypted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionInfo>,
}

/// Checks a chunk size against the format bounds.
pub(crate) fn validate_chunk_size(chunk_size: u32) -> Result<()> {
    if !(MIN_CHUNK_SIZE..=MAX_CHUNK_SIZE).contains(&chunk_size) {
        return Err(PackageError::limit(
            "chunk size",
            u64::from(chunk_size),
            u64::from(MIN_CHUNK_SIZE),
            u64::from(MAX_CHUNK_SIZE),
        ));
    }
    Ok(())
}

/// Checks a short free-form label (`generator`, `created`).
pub(crate) fn validate_label(what: &'static str, text: &str) -> Result<()> {
    if text.len() > MAX_LABEL_LEN {
        return Err(PackageError::limit(
            what,
            text.len() as u64,
            0,
            MAX_LABEL_LEN as u64,
        ));
    }
    if text.chars().any(char::is_control) {
        return Err(PackageError::BadOption(format!(
            "{what} must not contain control characters"
        )));
    }
    Ok(())
}

impl Prelude {
    /// Serialises the prelude to its canonical form: compact JSON with the
    /// fields in declaration order.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self)
            .map_err(|e| PackageError::prelude(format!("cannot serialise the prelude: {e}")))
    }

    /// Parses prelude bytes and validates them against the header flags.
    ///
    /// The caller has already verified the bytes against `prelude_sha256`.
    pub fn parse(bytes: &[u8], header: &Header) -> Result<Prelude> {
        let prelude: Prelude = serde_json::from_slice(bytes)
            .map_err(|e| PackageError::prelude(format!("it is not valid prelude JSON ({e})")))?;
        prelude.validate(header.is_encrypted(), header.has_public_card())?;
        Ok(prelude)
    }

    /// Validates the prelude's invariants.
    pub(crate) fn validate(&self, encrypted: bool, has_card: bool) -> Result<()> {
        validate_chunk_size(self.chunk_size)?;
        validate_label("generator length", &self.generator)
            .map_err(|_| PackageError::prelude("the generator string is too long or malformed"))?;
        if self.card.is_some() != has_card {
            return Err(PackageError::prelude(
                "the HAS_PUBLIC_CARD header flag does not match the prelude contents",
            ));
        }
        if let Some(card) = &self.card {
            card.validate()?;
        }
        if self.encryption.is_some() != encrypted {
            return Err(PackageError::prelude(
                "the ENCRYPTED header flag does not match the prelude contents",
            ));
        }
        if let Some(encryption) = &self.encryption {
            if encryption.cipher != CIPHER_AES_256_GCM {
                return Err(PackageError::UnsupportedFeature(format!(
                    "cipher \"{}\"",
                    crate::error::quote(&encryption.cipher)
                )));
            }
            if encryption.subkeys != SUBKEYS_HKDF_SHA256 {
                return Err(PackageError::UnsupportedFeature(format!(
                    "sub-key derivation \"{}\"",
                    crate::error::quote(&encryption.subkeys)
                )));
            }
            let count = encryption.slots.len();
            if count == 0 || count > MAX_KEY_SLOTS {
                return Err(PackageError::limit(
                    "number of key slots",
                    count as u64,
                    1,
                    MAX_KEY_SLOTS as u64,
                ));
            }
            encryption.commitment()?;
            for slot in &encryption.slots {
                slot.decode()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kdf_bounds() {
        assert!(KdfParams::DEFAULT.validate().is_ok());
        assert!(KdfParams::MIN.validate().is_ok());
        for bad in [
            KdfParams {
                m_kib: 8191,
                t: 1,
                p: 1,
            },
            KdfParams {
                m_kib: 1024 * 1024 + 1,
                t: 1,
                p: 1,
            },
            KdfParams {
                m_kib: 8192,
                t: 0,
                p: 1,
            },
            KdfParams {
                m_kib: 8192,
                t: 17,
                p: 1,
            },
            KdfParams {
                m_kib: 8192,
                t: 1,
                p: 0,
            },
            KdfParams {
                m_kib: 8192,
                t: 1,
                p: 5,
            },
        ] {
            assert_eq!(bad.validate().unwrap_err().code(), "E_PKG_LIMIT", "{bad:?}");
        }
    }

    #[test]
    fn card_color() {
        let mut card = PublicCard {
            title: "T".into(),
            ..PublicCard::default()
        };
        assert!(card.validate().is_ok());
        card.cover_color = Some("#a1B2c3".into());
        assert!(card.validate().is_ok());
        for bad in ["red", "#12345", "#1234567", "#12345g", "url(x)"] {
            card.cover_color = Some(bad.into());
            assert!(card.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn unknown_fields_and_slot_kinds_are_tolerated() {
        let json = br#"{"generator":"x","chunk_size":65536,"future":{"a":1},
            "encryption":{"cipher":"AES-256-GCM","subkeys":"HKDF-SHA256","extra":true,
            "key_commitment":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "slots":[{"kind":"passkey","credential":"abc"}]}}"#;
        let prelude: Prelude = serde_json::from_slice(json).unwrap();
        prelude.validate(true, false).unwrap();
        assert_eq!(
            prelude.encryption.unwrap().slots[0].slot_kind(),
            SlotKind::Unknown
        );
    }

    #[test]
    fn flag_mismatch_is_rejected() {
        let json = br#"{"generator":"x","chunk_size":65536}"#;
        let prelude: Prelude = serde_json::from_slice(json).unwrap();
        assert!(prelude.validate(false, false).is_ok());
        assert!(prelude.validate(true, false).is_err());
        assert!(prelude.validate(false, true).is_err());
    }
}
