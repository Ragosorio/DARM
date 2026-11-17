//! Passwords and key files. Secrets are never taken from command-line
//! arguments (they would leak through shell history and process lists).

use std::io::{BufRead, IsTerminal, Read};
use std::path::Path;

use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::{CmdResult, Failure, Unlock};

/// Minimum password length accepted when *creating* a protected book.
pub const MIN_PASSWORD_CHARS: usize = 8;

/// Passwords are NFC-normalised so that the same typed text unlocks the book
/// on every platform (the reader does the same).
fn normalise(raw: &str) -> Zeroizing<String> {
    Zeroizing::new(raw.trim_end_matches(['\n', '\r']).nfc().collect())
}

/// Read a password from the configured source, or prompt for it.
/// `confirm` asks twice (when creating a book).
pub fn password(unlock: &Unlock, confirm: bool) -> Result<Option<Zeroizing<String>>, Failure> {
    if let Some(var) = &unlock.password_env {
        let value = std::env::var(var).map_err(|_| {
            Failure::env(format!(
                "error[E0504]: the environment variable `{var}` is not set"
            ))
        })?;
        return Ok(Some(normalise(&value)));
    }
    if let Some(path) = &unlock.password_file {
        let text = std::fs::read_to_string(path).map_err(|e| {
            Failure::env(format!(
                "cannot read the password file `{}`: {e}",
                path.display()
            ))
        })?;
        return Ok(Some(normalise(text.lines().next().unwrap_or(""))));
    }
    if unlock.password_stdin {
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| Failure::env(format!("cannot read stdin: {e}")))?;
        let out = normalise(&line);
        zeroize::Zeroize::zeroize(&mut line);
        return Ok(Some(out));
    }
    if unlock.keyfile.is_some() {
        return Ok(None);
    }
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let first = rpassword::prompt_password("Password: ")
        .map_err(|e| Failure::env(format!("cannot read the password: {e}")))?;
    let first = normalise(&first);
    if confirm {
        let second = rpassword::prompt_password("Repeat password: ")
            .map_err(|e| Failure::env(format!("cannot read the password: {e}")))?;
        if *normalise(&second) != *first {
            return Err(Failure::env("error[E0504]: the two passwords differ"));
        }
    }
    Ok(Some(first))
}

/// Load the 32-byte key of a `.darmkey` file.
pub fn keyfile(path: &Path) -> Result<darm_package::KeyFile, Failure> {
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(64 * 1024).read_to_string(&mut text))
        .map_err(|e| {
            Failure::env(format!(
                "cannot read the key file `{}`: {e}",
                path.display()
            ))
        })?;
    let key = darm_package::KeyFile::parse(&text)
        .map_err(|e| Failure::Package(format!("{}: `{}`: {e}", e.code(), path.display())));
    zeroize::Zeroize::zeroize(&mut text);
    key
}

/// Write a key file that only the owner can read.
pub fn write_keyfile(path: &Path, key: &darm_package::KeyFile) -> CmdResult {
    if path.exists() {
        return Err(Failure::env(format!(
            "`{}` already exists; refusing to overwrite a key (books locked with it would become unreadable)",
            path.display()
        )));
    }
    let text = Zeroizing::new(key.to_text());
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .map_err(|e| Failure::env(format!("cannot write `{}`: {e}", path.display())))?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, text.as_bytes())
        .map_err(|e| Failure::env(format!("cannot write `{}`: {e}", path.display())))?;
    Ok(())
}

pub fn keygen(out: &Path) -> CmdResult {
    let key = darm_package::generate_keyfile()?;
    write_keyfile(out, &key)?;
    println!("wrote {} (key id {})", out.display(), key.key_id());
    println!(
        "Keep this file private and backed up: a book locked with it cannot be opened without it."
    );
    Ok(())
}

/// Credentials for opening a package, resolved from the command line.
pub enum Opened {
    None,
    Password(Zeroizing<String>),
    Key(darm_package::KeyFile),
}

impl Opened {
    pub fn resolve(unlock: &Unlock, needed: bool) -> Result<Opened, Failure> {
        if let Some(path) = &unlock.keyfile {
            return Ok(Opened::Key(keyfile(path)?));
        }
        let explicit = unlock.password_env.is_some()
            || unlock.password_file.is_some()
            || unlock.password_stdin;
        if !needed && !explicit {
            return Ok(Opened::None);
        }
        Ok(match password(unlock, false)? {
            Some(p) => Opened::Password(p),
            None => Opened::None,
        })
    }

    pub fn credentials(&self) -> darm_package::Credentials<'_> {
        match self {
            Opened::None => darm_package::Credentials::None,
            Opened::Password(p) => darm_package::Credentials::Password(p.as_str()),
            Opened::Key(k) => darm_package::Credentials::Key(k.key()),
        }
    }
}
