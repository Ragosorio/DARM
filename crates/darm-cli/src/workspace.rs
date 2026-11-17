//! Locating a project on disk.

use std::path::{Path, PathBuf};

use darm_compiler::project::{PROJECT_FILE, ProjectFile};

use crate::Failure;

/// A resolved project: root directory, entry file and `darm.toml` contents.
pub struct Project {
    pub root: PathBuf,
    /// Entry source, relative to `root`.
    pub entry: String,
    pub file: ProjectFile,
}

impl Project {
    /// `path` may be a project directory, an entry `.darml` file, or `None`
    /// for the current directory.
    pub fn locate(path: Option<&Path>) -> Result<Project, Failure> {
        let path = path.map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let meta = std::fs::metadata(&path)
            .map_err(|e| Failure::env(format!("cannot open `{}`: {e}", path.display())))?;
        let (root, explicit_entry) = if meta.is_dir() {
            (path, None)
        } else {
            if path.extension().and_then(|e| e.to_str()) != Some("darml") {
                return Err(Failure::env(format!(
                    "`{}` is not a .darml source file or a project directory",
                    path.display()
                )));
            }
            let root = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string);
            (root, name)
        };
        let toml_path = root.join(PROJECT_FILE);
        let file = match std::fs::read_to_string(&toml_path) {
            Ok(text) => ProjectFile::parse(&text)
                .map_err(|e| Failure::env(format!("error[E0502]: {}: {e}", toml_path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProjectFile::default(),
            Err(e) => {
                return Err(Failure::env(format!(
                    "cannot read `{}`: {e}",
                    toml_path.display()
                )));
            }
        };
        let entry = explicit_entry.unwrap_or_else(|| file.book.entry.clone());
        if !root.join(&entry).is_file() {
            return Err(Failure::env(format!(
                "no entry file `{}` in `{}`; create one with `darm init`, or set `entry` in {PROJECT_FILE}",
                entry,
                root.display()
            )));
        }
        Ok(Project { root, entry, file })
    }
}
