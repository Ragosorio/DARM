//! `darm inspect`, `darm verify` and `darm extract`.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use darm_package::{HEADER_LEN, MAX_PRELUDE_LEN, Package, Probe};

use crate::pack::human_size;
use crate::secrets::Opened;
use crate::{CmdResult, Failure, Format, Unlock};

/// Read the public part of a package (fixed header + prelude).
pub fn probe_file(path: &Path) -> Result<Probe, Failure> {
    let mut head = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| {
            f.take((HEADER_LEN + MAX_PRELUDE_LEN) as u64)
                .read_to_end(&mut head)
        })
        .map_err(|e| Failure::env(format!("cannot read `{}`: {e}", path.display())))?;
    Ok(darm_package::probe(&head)?)
}

/// Open a package, asking for credentials only when it is encrypted.
pub fn open(path: &Path, unlock: &Unlock) -> Result<Package<std::fs::File>, Failure> {
    let probe = probe_file(path)?;
    let actual = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if actual != probe.total_len {
        return Err(Failure::Package(format!(
            "E_PKG_TRUNCATED: `{}` is {actual} bytes but its header declares {}; the file is incomplete or has extra data",
            path.display(),
            probe.total_len
        )));
    }
    let opened = Opened::resolve(unlock, probe.encrypted)?;
    Package::open_path(path, &opened.credentials()).map_err(|e| match e.code() {
        "E_PKG_CREDENTIALS_REQUIRED" => Failure::Package(format!(
            "{}: this book is protected; pass --password-env VAR, --password-file FILE, --password-stdin or --keyfile FILE",
            e.code()
        )),
        _ => Failure::from(e),
    })
}

pub fn inspect(path: &Path, format: Format, ir: bool, unlock: &Unlock) -> CmdResult {
    let probe = probe_file(path)?;
    let explicit = unlock.keyfile.is_some()
        || unlock.password_env.is_some()
        || unlock.password_file.is_some()
        || unlock.password_stdin;
    // A locked book can still be described from its public part.
    let package = if probe.encrypted && !explicit && !ir {
        None
    } else {
        Some(open(path, unlock)?)
    };

    if ir {
        let package = package.expect("opened above");
        let bytes = package.ir_bytes()?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Failure::Package("E_PKG_CORRUPT: the IR is not UTF-8".into()))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| Failure::Package(format!("E_IR_JSON: the IR is not valid JSON: {e}")))?;
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return Ok(());
    }

    let slots: Vec<&str> = probe.slots.iter().map(|s| s.kind.as_str()).collect();
    match format {
        Format::Json => {
            let mut doc = serde_json::json!({
                "file": path.display().to_string(),
                "format": probe.version(),
                "uuid": probe.uuid_string(),
                "bytes": probe.total_len,
                "encrypted": probe.encrypted,
                "locked": package.is_none(),
                "generator": probe.generator,
                "chunk_size": probe.chunk_size,
                "card": probe.card,
                "key_slots": slots,
                "kdf": probe.kdf,
            });
            if let Some(p) = &package {
                doc["book"] = p.manifest().book()?;
                doc["entries"] = p
                    .entries()
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "name": e.name, "media_type": e.media_type, "role": e.role, "size": e.size,
                            "stored_size": e.stored_size, "compression": e.compression, "sha256": e.sha256.to_string(),
                            "chunks": e.chunks.len(),
                        })
                    })
                    .collect();
            }
            println!("{}", serde_json::to_string_pretty(&doc).unwrap());
        }
        Format::Human => {
            println!("{}", path.display());
            println!("  format      DARM {}", probe.version());
            println!("  uuid        {}", probe.uuid_string());
            println!(
                "  size        {} ({} bytes)",
                human_size(probe.total_len),
                probe.total_len
            );
            if let Some(g) = &probe.generator {
                println!("  written by  {g}");
            }
            if probe.encrypted {
                println!("  protection  AES-256-GCM, key slots: {}", slots.join(", "));
                if let Some(k) = &probe.kdf {
                    println!(
                        "  kdf         Argon2id m={} MiB t={} p={}",
                        k.m_kib / 1024,
                        k.t,
                        k.p
                    );
                }
            } else {
                println!("  protection  none (integrity checks only)");
            }
            match &probe.card {
                Some(card) => println!(
                    "  public card \"{}\" by {}",
                    card.title,
                    card.authors.join(", ")
                ),
                None if probe.encrypted => {
                    println!("  public card none (title and author are hidden)")
                }
                None => {}
            }
            match &package {
                None => println!(
                    "  contents    locked — supply the password or key file to list entries"
                ),
                Some(p) => {
                    let book = p.manifest().book()?;
                    if let Some(title) = book.get("title").and_then(|t| t.as_str()) {
                        let authors: Vec<&str> = book
                            .get("authors")
                            .and_then(|a| a.as_array())
                            .into_iter()
                            .flatten()
                            .filter_map(|a| a.as_str())
                            .collect();
                        println!("  book        \"{title}\" by {}", authors.join(", "));
                        println!(
                            "              id {} · language {} · layout {} · IR {}",
                            book["id"].as_str().unwrap_or("?"),
                            book["language"].as_str().unwrap_or("?"),
                            book["layout"].as_str().unwrap_or("?"),
                            book["ir_version"].as_str().unwrap_or("?")
                        );
                    }
                    println!("  entries     {}", p.entries().len());
                    for e in p.entries() {
                        let stored = if e.stored_size == e.size {
                            String::new()
                        } else {
                            format!(" → {}", human_size(e.stored_size))
                        };
                        println!(
                            "    {:<34} {:<18} {:>10}{stored}",
                            e.name,
                            e.media_type,
                            human_size(e.size)
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn verify(path: &Path, unlock: &Unlock) -> CmdResult {
    let package = open(path, unlock)?;
    let report = package.verify_all()?;
    println!(
        "ok: {} — {} entries, {} chunks, {} verified{}",
        path.display(),
        report.entries,
        report.chunks,
        human_size(report.bytes),
        if package.is_encrypted() {
            " (authenticated with AES-256-GCM)"
        } else {
            " (SHA-256)"
        }
    );
    Ok(())
}

/// Entry names are validated by the package layer; this is defence in depth
/// before touching the file system.
fn safe_join(dir: &Path, name: &str) -> Option<PathBuf> {
    let rel = Path::new(name);
    if rel.components().all(|c| matches!(c, Component::Normal(_)))
        && !name.contains('\\')
        && !name.contains(':')
    {
        Some(dir.join(rel))
    } else {
        None
    }
}

pub fn extract(path: &Path, names: &[String], out: &Path, unlock: &Unlock) -> CmdResult {
    let package = open(path, unlock)?;
    let wanted: Vec<u32> = if names.is_empty() {
        package.entries().iter().map(|e| e.index).collect()
    } else {
        let mut v = Vec::new();
        for n in names {
            match package.entry_by_name(n) {
                Some(e) => v.push(e.index),
                None => {
                    return Err(Failure::Package(format!(
                        "E_PKG_NO_ENTRY: no entry named `{n}` (see `darm inspect`)"
                    )));
                }
            }
        }
        v
    };
    std::fs::create_dir_all(out)
        .map_err(|e| Failure::env(format!("cannot create `{}`: {e}", out.display())))?;
    for index in wanted {
        let entry = package.entry(index)?.clone();
        let target = safe_join(out, &entry.name).ok_or_else(|| {
            Failure::Package(format!(
                "E_PKG_BAD_NAME: refusing to write entry `{}`",
                entry.name
            ))
        })?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Failure::env(format!("cannot create `{}`: {e}", parent.display())))?;
        }
        // Stream chunk by chunk: entries can be multi-gigabyte videos.
        let mut reader = package.entry_reader(index)?;
        let mut file = std::fs::File::create(&target)
            .map_err(|e| Failure::env(format!("cannot write `{}`: {e}", target.display())))?;
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| Failure::Package(format!("E_PKG_INTEGRITY: `{}`: {e}", entry.name)))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| Failure::env(format!("cannot write `{}`: {e}", target.display())))?;
        }
        println!("{}", target.display());
    }
    Ok(())
}
