//! Projects, file access and path safety.
//!
//! All paths that appear in sources (`include`, asset paths) are *project
//! relative strings* with `/` separators. They are normalised lexically and
//! may never leave the project root; on disk, symlinks that point outside the
//! root are rejected as well.

use std::collections::BTreeMap;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::assets::ReadSeek;

/// Name of the optional project file.
pub const PROJECT_FILE: &str = "darm.toml";
/// Extension of source files.
pub const SOURCE_EXT: &str = "darml";
/// Maximum size of one source file.
pub const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum include nesting.
pub const MAX_INCLUDE_DEPTH: usize = 16;

/// File access used by the compiler. Paths are project-relative.
pub trait Fs {
    fn read(&self, rel: &str) -> io::Result<Vec<u8>>;
    fn open(&self, rel: &str) -> io::Result<Box<dyn ReadSeek + '_>>;
    /// Absolute location for packaging, if the file exists on a real disk.
    fn disk_path(&self, rel: &str) -> Option<PathBuf>;
}

/// The real file system, rooted at a project directory.
pub struct DiskFs {
    root: PathBuf,
    canonical_root: PathBuf,
}

impl DiskFs {
    pub fn new(root: impl Into<PathBuf>) -> io::Result<DiskFs> {
        let root = root.into();
        let canonical_root = root.canonicalize()?;
        Ok(DiskFs {
            root,
            canonical_root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, rel: &str) -> io::Result<PathBuf> {
        let path = self.root.join(rel);
        // Symlinks must not be a way out of the project.
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(&self.canonical_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path resolves outside the project directory",
            ));
        }
        Ok(canonical)
    }
}

impl Fs for DiskFs {
    fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        let path = self.resolve(rel)?;
        if std::fs::metadata(&path)?.len() > MAX_SOURCE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file is larger than 16 MiB",
            ));
        }
        std::fs::read(path)
    }

    fn open(&self, rel: &str) -> io::Result<Box<dyn ReadSeek + '_>> {
        let path = self.resolve(rel)?;
        if !std::fs::metadata(&path)?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        Ok(Box::new(std::fs::File::open(path)?))
    }

    fn disk_path(&self, rel: &str) -> Option<PathBuf> {
        self.resolve(rel).ok()
    }
}

/// An in-memory file system for tests, fuzzing and embedding.
#[derive(Default, Clone)]
pub struct MemoryFs {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl MemoryFs {
    pub fn new() -> MemoryFs {
        MemoryFs::default()
    }

    pub fn with(mut self, rel: &str, data: impl Into<Vec<u8>>) -> MemoryFs {
        self.files.insert(rel.to_string(), data.into());
        self
    }
}

impl Fs for MemoryFs {
    fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        self.files
            .get(rel)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }

    fn open(&self, rel: &str) -> io::Result<Box<dyn ReadSeek + '_>> {
        let data = self
            .files
            .get(rel)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))?;
        Ok(Box::new(Cursor::new(data.as_slice())))
    }

    fn disk_path(&self, _rel: &str) -> Option<PathBuf> {
        None
    }
}

/// Directory part of a project-relative path (`"a/b/c.darml"` → `"a/b"`).
pub fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Join a path written in a source file onto the directory of that file and
/// normalise it. Fails with a reason when the path is absolute, malformed or
/// leaves the project.
pub fn join_rel(base_dir: &str, path: &str) -> Result<String, &'static str> {
    if path.is_empty() {
        return Err("the path is empty");
    }
    if path.contains('\\') {
        return Err("use `/` as the path separator");
    }
    if path.starts_with('/') || path.starts_with('~') || path.as_bytes().get(1) == Some(&b':') {
        return Err("absolute paths are not allowed; write the path relative to this file");
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("the path contains control characters");
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("the path leaves the project directory");
                }
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err("the path does not name a file");
    }
    Ok(parts.join("/"))
}

/// Contents of `darm.toml`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    #[serde(default)]
    pub book: BookSection,
    #[serde(default)]
    pub protect: ProtectSection,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BookSection {
    /// Entry source file, relative to the project directory.
    #[serde(default = "default_entry")]
    pub entry: String,
    /// Output path, relative to the project directory. `{slug}` is replaced
    /// by a file-name-safe form of the title. Default `{slug}.darm`.
    #[serde(default = "default_output")]
    pub output: String,
}

fn default_entry() -> String {
    "book.darml".into()
}

fn default_output() -> String {
    "{slug}.darm".into()
}

impl Default for BookSection {
    fn default() -> Self {
        BookSection {
            entry: default_entry(),
            output: default_output(),
        }
    }
}

/// Protection defaults. Secrets never live in this file: passwords come from
/// the environment, a file or a prompt, and key files stay outside the book.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProtectSection {
    /// `none` (default), `password` or `keyfile`.
    #[serde(default)]
    pub mode: ProtectMode,
    /// Environment variable that holds the password (mode `password`).
    pub password_env: Option<String>,
    /// Path of the `.darmkey` file (mode `keyfile`), relative to the project.
    pub keyfile: Option<String>,
    /// Keep title/author/language readable without the key (default true).
    #[serde(default = "yes")]
    pub public_card: bool,
}

fn yes() -> bool {
    true
}

impl Default for ProtectSection {
    fn default() -> Self {
        ProtectSection {
            mode: ProtectMode::None,
            password_env: None,
            keyfile: None,
            public_card: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProtectMode {
    #[default]
    None,
    Password,
    Keyfile,
}

impl ProjectFile {
    pub fn parse(text: &str) -> Result<ProjectFile, String> {
        let p: ProjectFile = toml::from_str(text).map_err(|e| e.to_string())?;
        join_rel("", &p.book.entry).map_err(|e| format!("book.entry: {e}"))?;
        if !p.book.entry.ends_with(".darml") {
            return Err("book.entry must be a .darml file".into());
        }
        Ok(p)
    }
}

/// A file-name-safe slug: lower-case ASCII letters, digits and dashes.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        let mapped = match c {
            'a'..='z' | '0'..='9' => Some(c),
            'A'..='Z' => Some(c.to_ascii_lowercase()),
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' | 'Á' | 'À' | 'Ä' | 'Â' | 'Ã' | 'Å' => {
                Some('a')
            }
            'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => Some('e'),
            'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => Some('i'),
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' | 'ø' | 'Ó' | 'Ò' | 'Ö' | 'Ô' | 'Õ' | 'Ø' => {
                Some('o')
            }
            'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => Some('u'),
            'ñ' | 'Ñ' => Some('n'),
            'ç' | 'Ç' => Some('c'),
            _ => None,
        };
        match mapped {
            Some(c) => out.push(c),
            None if !out.ends_with('-') && !out.is_empty() => out.push('-'),
            None => {}
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "book".into()
    } else {
        out.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_is_confined_to_the_project() {
        assert_eq!(join_rel("", "art/a.png").unwrap(), "art/a.png");
        assert_eq!(join_rel("chapters", "../art/a.png").unwrap(), "art/a.png");
        assert_eq!(join_rel("a/b", "./c/../d.png").unwrap(), "a/b/d.png");
        assert!(join_rel("", "../x").is_err());
        assert!(join_rel("a", "../../x").is_err());
        assert!(join_rel("", "/etc/passwd").is_err());
        assert!(join_rel("", "C:/x").is_err());
        assert!(join_rel("", "a\\b").is_err());
        assert!(join_rel("", "~/x").is_err());
        assert!(join_rel("", "").is_err());
        assert!(join_rel("", ".").is_err());
    }

    #[test]
    fn project_file() {
        let p = ProjectFile::parse("").unwrap();
        assert_eq!(p.book.entry, "book.darml");
        assert_eq!(p.protect.mode, ProtectMode::None);
        assert!(
            p.protect.public_card,
            "the card is public unless the author opts out"
        );
        let p = ProjectFile::parse("[book]\nentry = \"src/main.darml\"\noutput = \"dist/{slug}.darm\"\n[protect]\nmode = \"password\"\npassword_env = \"BOOK_PW\"\n").unwrap();
        assert_eq!(p.book.output, "dist/{slug}.darm");
        assert_eq!(p.protect.mode, ProtectMode::Password);
        assert!(p.protect.public_card);
        assert!(ProjectFile::parse("[book]\nentry = \"../x.darml\"").is_err());
        assert!(ProjectFile::parse("[book]\nentri = \"x.darml\"").is_err());
        assert!(
            ProjectFile::parse("[protect]\npassword = \"hunter2\"").is_err(),
            "secrets must not be accepted in darm.toml"
        );
    }

    #[test]
    fn slugs() {
        assert_eq!(
            slug("The Cartographer of Tides"),
            "the-cartographer-of-tides"
        );
        assert_eq!(
            slug("  Atlas de mareas: señales & café  "),
            "atlas-de-mareas-senales-cafe"
        );
        assert_eq!(slug("!!!"), "book");
    }
}
