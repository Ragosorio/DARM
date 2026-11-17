//! Key hierarchy and AEAD sealing. Only audited RustCrypto primitives are used
//! (`aes-gcm`, `hkdf`, `sha2`, `argon2`); this module merely wires them together.
//!
//! ```text
//! password ──Argon2id(salt, m, t, p)──► KEK ─┐
//!                                            ├─AES-256-GCM unwrap──► CEK (32 random bytes)
//! .darmkey ──HKDF-SHA256(salt = UUID)──► KEK ─┘                        │
//!                                         HKDF-SHA256(salt = UUID) ────┤
//!                                            info "darm/1/manifest-key"   ──► manifest key
//!                                            info "darm/1/data-key"       ──► data key
//!                                            info "darm/1/key-commitment" ──► public commitment
//! ```

use aes_gcm::aead::AeadInOut;
use aes_gcm::{Aes256Gcm, KeyInit};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::{PackageError, Result};
use crate::header::HEADER_AAD_LEN;
use crate::limits::GCM_TAG_LEN;
use crate::prelude::{KdfParams, SALT_LEN, WRAPPED_KEY_LEN};
use crate::util::sha256_parts;

/// A 32-byte secret that is wiped when dropped.
pub(crate) type Secret = Zeroizing<[u8; 32]>;

const INFO_MANIFEST_KEY: &[u8] = b"darm/1/manifest-key";
const INFO_DATA_KEY: &[u8] = b"darm/1/data-key";
const INFO_KEYFILE_KEK: &[u8] = b"darm/1/keyfile-kek";
const INFO_KEY_COMMITMENT: &[u8] = b"darm/1/key-commitment";
const AAD_CHUNK: &[u8] = b"darm/1/chunk\0";
const AAD_MANIFEST: &[u8] = b"darm/1/manifest\0";
const AAD_SLOT: &[u8] = b"darm/1/slot\0";
const KEY_ID_DOMAIN: &[u8] = b"darm/1/key-id\0";

/// Slot kind byte bound into the slot AAD.
const SLOT_KIND_PASSWORD: u8 = 1;
const SLOT_KIND_KEYFILE: u8 = 2;

/// Compression byte bound into the chunk AAD.
pub(crate) const AAD_COMPRESSION_NONE: u8 = 0;
pub(crate) const AAD_COMPRESSION_DEFLATE: u8 = 1;

fn cipher(key: &[u8; 32]) -> Aes256Gcm {
    Aes256Gcm::new(key.into())
}

fn hkdf_expand(salt: &[u8], ikm: &[u8; 32], info: &[u8]) -> Secret {
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info, out.as_mut_slice())
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

/// Seals `buffer` in place and appends the 16-byte tag.
#[cfg(feature = "write")]
fn seal(cipher: &Aes256Gcm, nonce: &[u8; 12], aad: &[u8], buffer: &mut Vec<u8>) -> Result<()> {
    let tag = cipher
        .encrypt_inout_detached(nonce.into(), aad, buffer.as_mut_slice().into())
        .map_err(|_| PackageError::BadOption("data is too large to encrypt".into()))?;
    buffer.extend_from_slice(&tag);
    Ok(())
}

/// Verifies the trailing tag of `buffer` and decrypts it in place, removing
/// the tag. Returns `false` (leaving the buffer untouched) on failure.
#[must_use]
fn open(cipher: &Aes256Gcm, nonce: &[u8; 12], aad: &[u8], buffer: &mut Vec<u8>) -> bool {
    let Some(body_len) = buffer.len().checked_sub(GCM_TAG_LEN) else {
        return false;
    };
    let (body, tag) = buffer.split_at_mut(body_len);
    let mut tag_bytes = [0u8; GCM_TAG_LEN];
    tag_bytes.copy_from_slice(tag);
    let ok = cipher
        .decrypt_inout_detached(nonce.into(), aad, body.into(), &tag_bytes.into())
        .is_ok();
    if ok {
        buffer.truncate(body_len);
    }
    ok
}

/// Nonce of a data chunk: entry index (u32 BE) followed by chunk index (u64 BE).
fn chunk_nonce(entry: u32, chunk: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&entry.to_be_bytes());
    nonce[4..].copy_from_slice(&chunk.to_be_bytes());
    nonce
}

/// Position of a chunk, bound into its AEAD associated data.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChunkContext<'a> {
    pub uuid: &'a [u8; 16],
    pub entry: u32,
    pub chunk: u64,
    pub is_final: bool,
    pub plain_len: u32,
    pub compression: u8,
}

impl ChunkContext<'_> {
    fn aad(&self) -> [u8; AAD_CHUNK.len() + 16 + 4 + 8 + 1 + 4 + 1] {
        let mut aad = [0u8; AAD_CHUNK.len() + 16 + 4 + 8 + 1 + 4 + 1];
        let mut at = 0;
        let mut put = |bytes: &[u8]| {
            aad[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        };
        put(AAD_CHUNK);
        put(self.uuid);
        put(&self.entry.to_be_bytes());
        put(&self.chunk.to_be_bytes());
        put(&[u8::from(self.is_final)]);
        put(&self.plain_len.to_be_bytes());
        put(&[self.compression]);
        aad
    }
}

/// The keys of an unlocked package.
pub(crate) struct ContentKeys {
    data: Aes256Gcm,
    manifest: Aes256Gcm,
}

impl ContentKeys {
    /// Derives the manifest and data sub-keys from the content key.
    pub(crate) fn derive(cek: &Secret, uuid: &[u8; 16]) -> ContentKeys {
        let data = hkdf_expand(uuid, cek, INFO_DATA_KEY);
        let manifest = hkdf_expand(uuid, cek, INFO_MANIFEST_KEY);
        ContentKeys {
            data: cipher(&data),
            manifest: cipher(&manifest),
        }
    }

    fn manifest_aad(header_bytes: &[u8]) -> Vec<u8> {
        let mut aad = Vec::with_capacity(AAD_MANIFEST.len() + HEADER_AAD_LEN);
        aad.extend_from_slice(AAD_MANIFEST);
        aad.extend_from_slice(&header_bytes[..HEADER_AAD_LEN]);
        aad
    }

    /// Seals the (deflated) manifest. `header_bytes` is the encoded header;
    /// its first 80 bytes (everything through `prelude_sha256`) are bound.
    #[cfg(feature = "write")]
    pub(crate) fn seal_manifest(&self, header_bytes: &[u8], manifest: &mut Vec<u8>) -> Result<()> {
        seal(
            &self.manifest,
            &[0u8; 12],
            &Self::manifest_aad(header_bytes),
            manifest,
        )
    }

    /// Authenticates and decrypts the stored manifest in place.
    #[must_use]
    pub(crate) fn open_manifest(&self, header_bytes: &[u8], manifest: &mut Vec<u8>) -> bool {
        open(
            &self.manifest,
            &[0u8; 12],
            &Self::manifest_aad(header_bytes),
            manifest,
        )
    }

    /// Seals one data chunk in place.
    #[cfg(feature = "write")]
    pub(crate) fn seal_chunk(&self, context: &ChunkContext<'_>, chunk: &mut Vec<u8>) -> Result<()> {
        seal(
            &self.data,
            &chunk_nonce(context.entry, context.chunk),
            &context.aad(),
            chunk,
        )
    }

    /// Authenticates and decrypts one data chunk in place.
    #[must_use]
    pub(crate) fn open_chunk(&self, context: &ChunkContext<'_>, chunk: &mut Vec<u8>) -> bool {
        open(
            &self.data,
            &chunk_nonce(context.entry, context.chunk),
            &context.aad(),
            chunk,
        )
    }
}

/// Derives the key-encryption key of a password slot with Argon2id.
pub(crate) fn password_kek(
    password: &[u8],
    salt: &[u8; SALT_LEN],
    params: KdfParams,
) -> Result<Secret> {
    params.validate()?;
    let argon_params = Params::new(params.m_kib, params.t, params.p, Some(32))
        .map_err(|e| PackageError::Kdf(format!("invalid Argon2id parameters: {e}")))?;
    let mut kek = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params)
        .hash_password_into(password, salt, kek.as_mut_slice())
        .map_err(|e| match e {
            argon2::Error::OutOfMemory => PackageError::Kdf(format!(
                "not enough memory for Argon2id with a memory cost of {} MiB",
                params.m_kib / 1024
            )),
            other => PackageError::Kdf(other.to_string()),
        })?;
    Ok(kek)
}

/// Derives the key-encryption key of a keyfile slot:
/// `HKDF-SHA256(salt = UUID, ikm = key, info = "darm/1/keyfile-kek" || slot index u32 BE)`.
///
/// The slot index is part of the derivation so that every slot wraps the
/// content key under its own key even if the same key file is listed twice.
pub(crate) fn keyfile_kek(key: &[u8; 32], uuid: &[u8; 16], slot_index: u32) -> Secret {
    let mut info = [0u8; INFO_KEYFILE_KEK.len() + 4];
    info[..INFO_KEYFILE_KEK.len()].copy_from_slice(INFO_KEYFILE_KEK);
    info[INFO_KEYFILE_KEK.len()..].copy_from_slice(&slot_index.to_be_bytes());
    hkdf_expand(uuid, key, &info)
}

/// Public commitment to the content key, stored in the prelude:
/// `HKDF-SHA256(salt = UUID, ikm = CEK, info = "darm/1/key-commitment")`.
///
/// AES-GCM does not commit to its key, so without this value a malicious
/// author could craft one file that decrypts to different books under
/// different slots. Every slot must unwrap to the one key committed here.
pub(crate) fn key_commitment(cek: &Secret, uuid: &[u8; 16]) -> [u8; 32] {
    *hkdf_expand(uuid, cek, INFO_KEY_COMMITMENT)
}

/// Public identifier of a keyfile key: first 8 bytes of
/// `SHA-256("darm/1/key-id\0" || key)`.
pub(crate) fn key_id_bytes(key: &[u8; 32]) -> [u8; 8] {
    let digest = sha256_parts(&[KEY_ID_DOMAIN, key]);
    let mut id = [0u8; 8];
    id.copy_from_slice(&digest[..8]);
    id
}

/// What a slot's wrap is bound to besides the package UUID and slot index.
pub(crate) enum SlotBinding<'a> {
    Password {
        params: KdfParams,
        salt: &'a [u8; SALT_LEN],
    },
    Keyfile {
        key_id: &'a [u8; 8],
    },
}

fn slot_aad(uuid: &[u8; 16], slot_index: u32, binding: &SlotBinding<'_>) -> Vec<u8> {
    let mut aad = Vec::with_capacity(80);
    aad.extend_from_slice(AAD_SLOT);
    aad.extend_from_slice(uuid);
    aad.extend_from_slice(&slot_index.to_be_bytes());
    match binding {
        SlotBinding::Password { params, salt } => {
            aad.push(SLOT_KIND_PASSWORD);
            aad.extend_from_slice(&crate::prelude::ARGON2_VERSION.to_be_bytes());
            aad.extend_from_slice(&params.m_kib.to_be_bytes());
            aad.extend_from_slice(&params.t.to_be_bytes());
            aad.extend_from_slice(&params.p.to_be_bytes());
            aad.extend_from_slice(*salt);
        }
        SlotBinding::Keyfile { key_id } => {
            aad.push(SLOT_KIND_KEYFILE);
            aad.extend_from_slice(*key_id);
        }
    }
    aad
}

/// Wraps the content key under a slot's key-encryption key.
#[cfg(feature = "write")]
pub(crate) fn wrap_cek(
    kek: &Secret,
    cek: &Secret,
    uuid: &[u8; 16],
    slot_index: u32,
    binding: &SlotBinding<'_>,
) -> Result<[u8; WRAPPED_KEY_LEN]> {
    let mut buffer = Zeroizing::new(cek.to_vec());
    seal(
        &cipher(kek),
        &[0u8; 12],
        &slot_aad(uuid, slot_index, binding),
        &mut buffer,
    )?;
    let mut out = [0u8; WRAPPED_KEY_LEN];
    out.copy_from_slice(&buffer);
    Ok(out)
}

/// Tries to unwrap the content key; `None` means the key-encryption key is wrong
/// (or the slot was tampered with).
pub(crate) fn unwrap_cek(
    kek: &Secret,
    wrapped: &[u8; WRAPPED_KEY_LEN],
    uuid: &[u8; 16],
    slot_index: u32,
    binding: &SlotBinding<'_>,
) -> Option<Secret> {
    let mut buffer = Zeroizing::new(wrapped.to_vec());
    if !open(
        &cipher(kek),
        &[0u8; 12],
        &slot_aad(uuid, slot_index, binding),
        &mut buffer,
    ) {
        return None;
    }
    let mut cek = Zeroizing::new([0u8; 32]);
    cek.copy_from_slice(&buffer);
    Some(cek)
}

#[cfg(all(test, feature = "write"))]
mod tests {
    use super::*;

    fn keys() -> ContentKeys {
        ContentKeys::derive(&Zeroizing::new([9u8; 32]), &[1u8; 16])
    }

    fn context(uuid: &[u8; 16]) -> ChunkContext<'_> {
        ChunkContext {
            uuid,
            entry: 3,
            chunk: 5,
            is_final: false,
            plain_len: 11,
            compression: AAD_COMPRESSION_NONE,
        }
    }

    #[test]
    fn chunk_round_trip_and_binding() {
        let keys = keys();
        let uuid = [1u8; 16];
        let base = context(&uuid);
        let mut sealed = b"hello world".to_vec();
        keys.seal_chunk(&base, &mut sealed).unwrap();
        assert_eq!(sealed.len(), 11 + GCM_TAG_LEN);

        let mut copy = sealed.clone();
        assert!(keys.open_chunk(&base, &mut copy));
        assert_eq!(copy, b"hello world");

        let other_uuid = [2u8; 16];
        let variants = [
            ChunkContext { entry: 4, ..base },
            ChunkContext { chunk: 6, ..base },
            ChunkContext {
                is_final: true,
                ..base
            },
            ChunkContext {
                plain_len: 12,
                ..base
            },
            ChunkContext {
                compression: AAD_COMPRESSION_DEFLATE,
                ..base
            },
            ChunkContext {
                uuid: &other_uuid,
                ..base
            },
        ];
        for variant in variants {
            let mut copy = sealed.clone();
            assert!(!keys.open_chunk(&variant, &mut copy), "{variant:?}");
            assert_eq!(copy, sealed, "a failed open must not expose plaintext");
        }

        let mut short = sealed[..10].to_vec();
        assert!(!keys.open_chunk(&base, &mut short));
    }

    #[test]
    fn manifest_is_bound_to_header() {
        let keys = keys();
        let header = [5u8; 128];
        let mut sealed = b"{}".to_vec();
        keys.seal_manifest(&header, &mut sealed).unwrap();
        let mut ok = sealed.clone();
        assert!(keys.open_manifest(&header, &mut ok));
        assert_eq!(ok, b"{}");
        for at in 0..HEADER_AAD_LEN {
            let mut other = header;
            other[at] ^= 1;
            let mut copy = sealed.clone();
            assert!(!keys.open_manifest(&other, &mut copy), "header byte {at}");
        }
    }

    #[test]
    fn wrap_round_trip_and_binding() {
        let uuid = [3u8; 16];
        let cek: Secret = Zeroizing::new([0xabu8; 32]);
        let kek = keyfile_kek(&[4u8; 32], &uuid, 0);
        assert_ne!(kek.as_slice(), keyfile_kek(&[4u8; 32], &uuid, 1).as_slice());
        assert_ne!(
            key_commitment(&cek, &uuid),
            key_commitment(&cek, &[0u8; 16])
        );
        assert_ne!(key_commitment(&cek, &uuid).as_slice(), cek.as_slice());
        let key_id = key_id_bytes(&[4u8; 32]);
        let binding = SlotBinding::Keyfile { key_id: &key_id };
        let wrapped = wrap_cek(&kek, &cek, &uuid, 0, &binding).unwrap();
        assert_eq!(
            unwrap_cek(&kek, &wrapped, &uuid, 0, &binding)
                .unwrap()
                .as_slice(),
            cek.as_slice()
        );
        assert!(unwrap_cek(&kek, &wrapped, &uuid, 1, &binding).is_none());
        assert!(unwrap_cek(&kek, &wrapped, &[0u8; 16], 0, &binding).is_none());
        let wrong = keyfile_kek(&[5u8; 32], &uuid, 0);
        assert!(unwrap_cek(&wrong, &wrapped, &uuid, 0, &binding).is_none());
    }

    /// Argon2id test vector from RFC 9106 §5.3 cannot be reproduced here
    /// (it uses a secret and associated data), so this checks determinism,
    /// salt/parameter sensitivity and the parameter bounds instead.
    #[test]
    fn password_kdf() {
        let salt = [7u8; SALT_LEN];
        let a = password_kek(b"correct horse", &salt, KdfParams::MIN).unwrap();
        let b = password_kek(b"correct horse", &salt, KdfParams::MIN).unwrap();
        assert_eq!(a.as_slice(), b.as_slice());
        let c = password_kek(b"correct horsf", &salt, KdfParams::MIN).unwrap();
        assert_ne!(a.as_slice(), c.as_slice());
        let d = password_kek(b"correct horse", &[8u8; SALT_LEN], KdfParams::MIN).unwrap();
        assert_ne!(a.as_slice(), d.as_slice());
        let weak = KdfParams {
            m_kib: 1024,
            t: 1,
            p: 1,
        };
        assert_eq!(
            password_kek(b"x", &salt, weak).unwrap_err().code(),
            "E_PKG_LIMIT"
        );
    }
}
