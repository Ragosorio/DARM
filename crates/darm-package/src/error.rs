//! The single error type of this crate.

use std::fmt;
use std::io;

use crate::prelude::PublicCard;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, PackageError>;

/// Everything that can go wrong while building or reading a `.darm` package.
///
/// Each variant has a stable machine-readable [`code`](PackageError::code)
/// (`E_PKG_…`) that front-ends can match on, and a human-readable `Display`
/// message that is specific enough to show to a user.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackageError {
    /// The data does not start with the `.darm` magic bytes.
    BadMagic,
    /// The source is shorter than the format requires.
    Truncated {
        /// Number of bytes that are required.
        needed: u64,
        /// Number of bytes that are available.
        available: u64,
    },
    /// The file uses a format major version this reader does not know.
    UnsupportedVersion {
        /// Major version found in the file.
        major: u16,
        /// Minor version found in the file.
        minor: u16,
    },
    /// The file uses a reserved flag, cipher or other feature unknown to this reader.
    UnsupportedFeature(String),
    /// A declared or requested size is outside the range the format allows.
    Limit {
        /// What was measured.
        what: &'static str,
        /// The offending value.
        value: u64,
        /// Smallest allowed value.
        min: u64,
        /// Largest allowed value.
        max: u64,
    },
    /// A checksum, hash or authentication tag does not match: the file is corrupt or was tampered with.
    Integrity(String),
    /// The fixed header is structurally invalid.
    CorruptHeader(String),
    /// The prelude is not valid for this format.
    CorruptPrelude(String),
    /// The manifest is not valid for this format.
    CorruptManifest(String),
    /// The supplied password or key does not unlock the package.
    WrongCredentials,
    /// The package is encrypted and no credentials were supplied.
    CredentialsRequired {
        /// The public card, when the author chose to expose one.
        card: Option<Box<PublicCard>>,
    },
    /// An entry name is not a safe relative path.
    BadName {
        /// The offending name.
        name: String,
        /// Why it was rejected.
        reason: &'static str,
    },
    /// Two entries share the same name.
    DuplicateName(String),
    /// An entry or chunk index/name does not exist.
    NotFound(String),
    /// A `.darmkey` file could not be parsed.
    BadKeyfile(String),
    /// A builder or reader option is invalid.
    BadOption(String),
    /// The password key-derivation function could not run (usually not enough memory).
    Kdf(String),
    /// A buffer the format legitimately needs could not be allocated.
    OutOfMemory {
        /// Size of the allocation that failed.
        bytes: u64,
    },
    /// An input file changed while the package was being written.
    SourceChanged(String),
    /// The operating system random source failed.
    Random(String),
    /// An I/O error from the underlying source or destination.
    Io {
        /// Kind of the I/O error.
        kind: io::ErrorKind,
        /// Its message.
        message: String,
    },
}

impl PackageError {
    /// Stable machine-readable code of this error, e.g. `E_PKG_BAD_MAGIC`.
    pub fn code(&self) -> &'static str {
        match self {
            PackageError::BadMagic => "E_PKG_BAD_MAGIC",
            PackageError::Truncated { .. } => "E_PKG_TRUNCATED",
            PackageError::UnsupportedVersion { .. } => "E_PKG_UNSUPPORTED_VERSION",
            PackageError::UnsupportedFeature(_) => "E_PKG_UNSUPPORTED_FEATURE",
            PackageError::Limit { .. } => "E_PKG_LIMIT",
            PackageError::Integrity(_) => "E_PKG_INTEGRITY",
            PackageError::CorruptHeader(_) => "E_PKG_CORRUPT_HEADER",
            PackageError::CorruptPrelude(_) => "E_PKG_CORRUPT_PRELUDE",
            PackageError::CorruptManifest(_) => "E_PKG_CORRUPT_MANIFEST",
            PackageError::WrongCredentials => "E_PKG_WRONG_CREDENTIALS",
            PackageError::CredentialsRequired { .. } => "E_PKG_CREDENTIALS_REQUIRED",
            PackageError::BadName { .. } => "E_PKG_BAD_NAME",
            PackageError::DuplicateName(_) => "E_PKG_DUPLICATE_NAME",
            PackageError::NotFound(_) => "E_PKG_NOT_FOUND",
            PackageError::BadKeyfile(_) => "E_PKG_BAD_KEYFILE",
            PackageError::BadOption(_) => "E_PKG_BAD_OPTION",
            PackageError::Kdf(_) => "E_PKG_KDF",
            PackageError::OutOfMemory { .. } => "E_PKG_OUT_OF_MEMORY",
            PackageError::SourceChanged(_) => "E_PKG_SOURCE_CHANGED",
            PackageError::Random(_) => "E_PKG_RANDOM",
            PackageError::Io { .. } => "E_PKG_IO",
        }
    }

    pub(crate) fn limit(what: &'static str, value: u64, min: u64, max: u64) -> Self {
        PackageError::Limit {
            what,
            value,
            min,
            max,
        }
    }

    pub(crate) fn integrity(message: impl Into<String>) -> Self {
        PackageError::Integrity(message.into())
    }

    pub(crate) fn manifest(message: impl Into<String>) -> Self {
        PackageError::CorruptManifest(message.into())
    }

    pub(crate) fn prelude(message: impl Into<String>) -> Self {
        PackageError::CorruptPrelude(message.into())
    }
}

/// Shortens and escapes untrusted text before it is embedded in a message.
pub(crate) fn quote(text: &str) -> String {
    const MAX: usize = 80;
    let mut out = String::new();
    for (count, ch) in text.chars().enumerate() {
        if count == MAX {
            out.push('…');
            break;
        }
        out.extend(ch.escape_debug());
    }
    out
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::BadMagic => {
                write!(
                    f,
                    "this is not a .darm package (the file signature is missing)"
                )
            }
            PackageError::Truncated { needed, available } => write!(
                f,
                "the package is truncated: {needed} bytes are required but only {available} are available"
            ),
            PackageError::UnsupportedVersion { major, minor } => write!(
                f,
                "this package uses .darm format {major}.{minor}, which is newer than this reader \
                 understands (format {}.x); update the reader to open it",
                crate::FORMAT_MAJOR
            ),
            PackageError::UnsupportedFeature(what) => write!(
                f,
                "this package uses a feature this reader does not support: {what}; update the reader to open it"
            ),
            PackageError::Limit {
                what,
                value,
                min,
                max,
            } => write!(
                f,
                "{what} is {value}, outside the allowed range {min}..={max}"
            ),
            PackageError::Integrity(message) => {
                write!(f, "the package is corrupt or was modified: {message}")
            }
            PackageError::CorruptHeader(message) => {
                write!(f, "the package header is invalid: {message}")
            }
            PackageError::CorruptPrelude(message) => {
                write!(f, "the package prelude is invalid: {message}")
            }
            PackageError::CorruptManifest(message) => {
                write!(f, "the package manifest is invalid: {message}")
            }
            PackageError::WrongCredentials => {
                write!(f, "the password or key file does not unlock this package")
            }
            PackageError::CredentialsRequired { .. } => write!(
                f,
                "this package is encrypted; a password or key file is required to open it"
            ),
            PackageError::BadName { name, reason } => {
                write!(f, "invalid entry name \"{}\": {reason}", quote(name))
            }
            PackageError::DuplicateName(name) => {
                write!(f, "duplicate entry name \"{}\"", quote(name))
            }
            PackageError::NotFound(what) => write!(f, "not found: {what}"),
            PackageError::BadKeyfile(message) => write!(f, "invalid key file: {message}"),
            PackageError::BadOption(message) => write!(f, "invalid option: {message}"),
            PackageError::Kdf(message) => write!(f, "password key derivation failed: {message}"),
            PackageError::OutOfMemory { bytes } => {
                write!(f, "not enough memory to allocate {bytes} bytes")
            }
            PackageError::SourceChanged(message) => write!(
                f,
                "an input changed while the package was being written: {message}"
            ),
            PackageError::Random(message) => {
                write!(f, "the system random source failed: {message}")
            }
            PackageError::Io { message, .. } => write!(f, "I/O error: {message}"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<io::Error> for PackageError {
    fn from(error: io::Error) -> Self {
        PackageError::Io {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

impl From<PackageError> for io::Error {
    fn from(error: PackageError) -> Self {
        let kind = match &error {
            PackageError::Io { kind, .. } => *kind,
            PackageError::Truncated { .. } => io::ErrorKind::UnexpectedEof,
            PackageError::NotFound(_) => io::ErrorKind::NotFound,
            PackageError::OutOfMemory { .. } => io::ErrorKind::OutOfMemory,
            _ => io::ErrorKind::InvalidData,
        };
        io::Error::new(kind, error)
    }
}
