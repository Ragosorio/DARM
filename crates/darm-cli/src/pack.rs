//! `darm check`, `darm fmt`, `darm build` and `darm reference`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use clap::Args;
use darm_compiler::project::{ProtectMode, slug};
use darm_compiler::{Compilation, DiskFs, spec};
use darm_package::{EncryptOptions, KdfParams, PackageBuilder, PublicCard};

use crate::secrets::{self, MIN_PASSWORD_CHARS};
use crate::workspace::Project;
use crate::{CmdResult, Failure, Format, Unlock};

fn color() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub fn compile(project: &Project) -> Result<Compilation, Failure> {
    let fs = DiskFs::new(&project.root)
        .map_err(|e| Failure::env(format!("cannot open `{}`: {e}", project.root.display())))?;
    Ok(darm_compiler::compile(&fs, &project.entry))
}

/// Print diagnostics. Returns `Err(Failure::Source)` when the build must stop.
pub fn report(
    c: &Compilation,
    format: Format,
    deny_warnings: bool,
    extra: serde_json::Value,
) -> CmdResult {
    let failed = c.has_errors() || (deny_warnings && c.warning_count() > 0);
    match format {
        Format::Human => {
            let text = c.render(color());
            if !text.is_empty() {
                eprintln!("{text}");
            }
            let (e, w) = (c.error_count(), c.warning_count());
            if e > 0 || w > 0 {
                let plural =
                    |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
                eprintln!("{}, {}", plural(e, "error"), plural(w, "warning"));
            }
            if deny_warnings && !c.has_errors() && w > 0 {
                eprintln!("warnings are denied (--deny-warnings)");
            }
        }
        Format::Json => {
            let mut doc = serde_json::json!({
                "ok": !failed,
                "errors": c.error_count(),
                "warnings": c.warning_count(),
                "diagnostics": c.to_json(),
            });
            if let (Some(map), serde_json::Value::Object(extra)) = (doc.as_object_mut(), extra) {
                map.extend(extra);
            }
            println!("{}", serde_json::to_string_pretty(&doc).unwrap());
        }
    }
    if failed { Err(Failure::Source) } else { Ok(()) }
}

pub fn check(path: Option<&Path>, format: Format, deny_warnings: bool) -> CmdResult {
    let project = Project::locate(path)?;
    let c = compile(&project)?;
    let summary = match &c.book {
        Some(book) => serde_json::json!({
            // `chapters` counts what the author wrote; the title page, colophon
            // and contents that the compiler generates are counted apart.
            "book": {
                "id": book.meta.id,
                "title": book.meta.title,
                "chapters": book.chapters.iter().filter(|c| c.kind == darm_ir::ChapterKind::Chapter).count(),
                "generated_front_matter": book.chapters.iter().filter(|c| c.kind != darm_ir::ChapterKind::Chapter).count(),
                "resources": book.resources.len()
            }
        }),
        None => serde_json::json!({}),
    };
    report(&c, format, deny_warnings, summary)?;
    if format == Format::Human
        && let Some(book) = &c.book
    {
        eprintln!(
            "ok: \"{}\" ({} chapters, {} resources)",
            book.meta.title,
            book.chapters.len(),
            book.resources.len()
        );
    }
    Ok(())
}

fn collect_sources(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), Failure> {
    let meta = std::fs::metadata(path)
        .map_err(|e| Failure::env(format!("cannot open `{}`: {e}", path.display())))?;
    if meta.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(path)
        .map_err(|e| Failure::env(format!("cannot list `{}`: {e}", path.display())))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if !name.starts_with('.') && name != "node_modules" && name != "target" {
                collect_sources(&p, out)?;
            }
        } else if p.extension().and_then(|e| e.to_str()) == Some("darml") {
            out.push(p);
        }
    }
    Ok(())
}

pub fn fmt(paths: &[PathBuf], check: bool, stdout: bool) -> CmdResult {
    let mut files = Vec::new();
    if paths.is_empty() {
        collect_sources(Path::new("."), &mut files)?;
    }
    for p in paths {
        collect_sources(p, &mut files)?;
    }
    if files.is_empty() {
        return Err(Failure::env("no .darml files found"));
    }
    if stdout && files.len() != 1 {
        return Err(Failure::env("--stdout formats exactly one file"));
    }
    let (mut changed, mut failed) = (0usize, false);
    for file in &files {
        let name = file.display().to_string();
        let text = std::fs::read_to_string(file)
            .map_err(|e| Failure::env(format!("cannot read `{name}`: {e}")))?;
        match darm_lang::fmt::format_source(darm_lang::FileId(0), &name, &text) {
            Ok(formatted) => {
                if stdout {
                    print!("{formatted}");
                } else if formatted != text {
                    changed += 1;
                    if check {
                        eprintln!("would reformat {name}");
                    } else {
                        std::fs::write(file, formatted)
                            .map_err(|e| Failure::env(format!("cannot write `{name}`: {e}")))?;
                        eprintln!("formatted {name}");
                    }
                }
            }
            Err(diags) => {
                failed = true;
                let mut map = darm_lang::SourceMap::new();
                map.add(name.clone(), text);
                for d in diags {
                    eprintln!("{}", d.render(&map, color()));
                }
                eprintln!("{name}: not formatted because it has syntax errors");
            }
        }
    }
    if failed || (check && changed > 0) {
        return Err(Failure::Source);
    }
    if !stdout {
        eprintln!(
            "{} file(s) checked, {changed} {}",
            files.len(),
            if check { "would change" } else { "changed" }
        );
    }
    Ok(())
}

#[derive(Args, Clone, Default)]
pub struct BuildArgs {
    /// Project directory or entry .darml file (default: current directory).
    pub path: Option<PathBuf>,
    /// Output file (default: `<title-slug>.darm` in the project, or `output` from darm.toml).
    #[arg(short, long, value_name = "FILE")]
    pub out: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "human")]
    pub format: Format,
    /// Treat warnings as errors.
    #[arg(long)]
    pub deny_warnings: bool,
    /// Also write the IR as pretty JSON to this file.
    #[arg(long, value_name = "FILE")]
    pub emit_ir: Option<PathBuf>,
    /// Encrypt the book (AES-256-GCM). Needs a password source and/or a key file.
    #[arg(long)]
    pub encrypt: bool,
    /// Generate a new key file at this path and lock the book with it.
    #[arg(long, value_name = "FILE")]
    pub new_keyfile: Option<PathBuf>,
    /// Do not leave title/author/language readable in an encrypted book.
    #[arg(long)]
    pub hide_card: bool,
    /// Argon2id memory in MiB for password-protected books (8..=1024).
    #[arg(long, value_name = "MIB", default_value_t = 64)]
    pub kdf_memory: u32,
    /// Argon2id passes (1..=16).
    #[arg(long, value_name = "N", default_value_t = 3)]
    pub kdf_passes: u32,
    #[command(flatten)]
    pub unlock: Unlock,
}

/// What `build` produced.
pub struct Built {
    pub output: PathBuf,
    pub compilation: Compilation,
}

fn encryption(args: &BuildArgs, project: &Project) -> Result<Option<EncryptOptions>, Failure> {
    let protect = &project.file.protect;
    let from_flags = args.encrypt
        || args.new_keyfile.is_some()
        || args.unlock.keyfile.is_some()
        || args.unlock.password_env.is_some()
        || args.unlock.password_file.is_some()
        || args.unlock.password_stdin;
    if !from_flags && protect.mode == ProtectMode::None {
        return Ok(None);
    }
    // Command-line flags win; darm.toml supplies defaults.
    let mut unlock = args.unlock.clone();
    if !from_flags
        || (args.encrypt
            && unlock.password_env.is_none()
            && unlock.keyfile.is_none()
            && args.new_keyfile.is_none())
    {
        match protect.mode {
            ProtectMode::Password => {
                unlock.password_env = unlock.password_env.or(protect.password_env.clone())
            }
            ProtectMode::Keyfile => {
                unlock.keyfile = unlock
                    .keyfile
                    .or(protect.keyfile.as_ref().map(|k| project.root.join(k)));
                if unlock.keyfile.is_none() {
                    return Err(Failure::env(
                        "error[E0504]: darm.toml has `mode = \"keyfile\"` but no `keyfile` path",
                    ));
                }
            }
            ProtectMode::None => {}
        }
    }
    let kdf = KdfParams {
        m_kib: args.kdf_memory.saturating_mul(1024),
        t: args.kdf_passes,
        p: 1,
    };
    kdf.validate()
        .map_err(|e| Failure::env(format!("error[E0504]: {e}")))?;
    let mut options = EncryptOptions::new()
        .kdf(kdf)
        .expose_card(!args.hide_card && protect.public_card);
    let mut slots = 0;
    if let Some(path) = &args.new_keyfile {
        let key = darm_package::generate_keyfile()?;
        secrets::write_keyfile(path, &key)?;
        eprintln!(
            "wrote key file {} (key id {}) — keep it private and backed up",
            path.display(),
            key.key_id()
        );
        options = options.keyfile_key(key.key());
        slots += 1;
    }
    if let Some(path) = &unlock.keyfile {
        let key = secrets::keyfile(path)?;
        options = options.keyfile_key(key.key());
        slots += 1;
    }
    let wants_password = unlock.password_env.is_some()
        || unlock.password_file.is_some()
        || unlock.password_stdin
        || slots == 0;
    if wants_password && let Some(pw) = secrets::password(&unlock, true)? {
        if pw.chars().count() < MIN_PASSWORD_CHARS {
            return Err(Failure::env(format!(
                "error[E0504]: the password is shorter than {MIN_PASSWORD_CHARS} characters; a longer passphrase is the main protection of the book"
            )));
        }
        options = options.password(pw.as_str());
        slots += 1;
    }
    if slots == 0 {
        return Err(Failure::env(
            "error[E0504]: --encrypt needs a secret: use --password-env VAR, --password-file FILE, --password-stdin, --keyfile FILE or --new-keyfile FILE (or run in a terminal to be prompted)",
        ));
    }
    Ok(Some(options))
}

pub fn build(args: &BuildArgs) -> Result<Built, Failure> {
    let project = Project::locate(args.path.as_deref())?;
    let c = compile(&project)?;
    let Some(book) = c
        .book
        .as_ref()
        .filter(|_| !(args.deny_warnings && c.warning_count() > 0))
    else {
        report(&c, args.format, args.deny_warnings, serde_json::json!({}))?;
        return Err(Failure::Source);
    };

    let output = match &args.out {
        Some(o) => o.clone(),
        None => project.root.join(
            project
                .file
                .book
                .output
                .replace("{slug}", &slug(&book.meta.title)),
        ),
    };
    if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| {
            Failure::env(format!(
                "error[E0503]: cannot create `{}`: {e}",
                dir.display()
            ))
        })?;
    }
    let encrypt = encryption(args, &project)?;
    let encrypted = encrypt.is_some();

    let mut builder = PackageBuilder::new();
    builder.generator(book.meta.generator.clone());
    builder.card(PublicCard {
        title: book.meta.title.clone(),
        authors: book.meta.authors.clone(),
        language: Some(book.meta.language.clone()),
        cover_color: Some(book.cover.front.color.clone()),
    });
    builder.book_summary(serde_json::json!({
        "id": book.meta.id,
        "title": book.meta.title,
        "subtitle": book.meta.subtitle,
        "authors": book.meta.authors,
        "language": book.meta.language,
        "layout": book.meta.layout,
        "ir_version": book.ir_version,
        "cover": { "style": book.cover.front.style, "color": book.cover.front.color, "ink": book.cover.front.ink, "accent": book.cover.front.accent },
    }))?;
    builder.ir(book.to_json().into_bytes())?;
    for f in &c.files {
        let path = f
            .path
            .clone()
            .ok_or_else(|| Failure::env(format!("`{}` is not a file on disk", f.source)))?;
        builder.add_file(f.entry.clone(), f.media_type.clone(), path)?;
    }
    if let Some(options) = encrypt {
        builder.encrypt(options);
    }
    let summary = builder.write_to_path(&output).map_err(|e| match e.code() {
        "E_PKG_IO" => Failure::env(format!(
            "error[E0503]: cannot write `{}`: {e}",
            output.display()
        )),
        _ => Failure::from(e),
    })?;

    // The package must hash exactly what the compiler inspected.
    if let Err(Failure::Package(msg) | Failure::Env(msg)) = crate::inspect::probe_file(&output) {
        return Err(Failure::Package(format!(
            "the package that was just written does not open: {msg}"
        )));
    }

    if let Some(path) = &args.emit_ir {
        std::fs::write(path, book.to_json_pretty()).map_err(|e| {
            Failure::env(format!(
                "error[E0503]: cannot write `{}`: {e}",
                path.display()
            ))
        })?;
    }
    let extra = serde_json::json!({
        "output": output.display().to_string(),
        "bytes": summary.total_len,
        "entries": summary.entry_count,
        "encrypted": encrypted,
        "uuid": summary.uuid_string(),
        "book": { "id": book.meta.id, "title": book.meta.title },
    });
    report(&c, args.format, false, extra)?;
    if args.format == Format::Human {
        eprintln!(
            "built {} ({}, {} entries{})",
            output.display(),
            human_size(summary.total_len),
            summary.entry_count,
            if encrypted { ", encrypted" } else { "" }
        );
    }
    Ok(Built {
        output,
        compilation: c,
    })
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Plain-text rendering of the language tables.
/// The complete language guide, carried in the binary so that the tool
/// documents itself wherever it is installed.
pub const LANGUAGE_GUIDE: &str = include_str!("../../../docs/language.md");

pub fn reference_text() -> String {
    let mut s = String::from(
        "DARM language reference (grammar 1). Complete guide with examples: `darm reference --guide`\n\nELEMENTS\n",
    );
    let params = |list: &[spec::Param], named: bool| -> String {
        list.iter()
            .map(|p| {
                let n = if named {
                    format!("{}=", p.name)
                } else {
                    p.name.to_string()
                };
                if p.required { n } else { format!("[{n}]") }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    for e in spec::ELEMENTS {
        let place = match e.place {
            spec::Place::Top => "top level".to_string(),
            spec::Place::Block => "content".to_string(),
            spec::Place::Chapter => "inside chapter".to_string(),
            spec::Place::Child => format!("inside {}", e.parent),
            spec::Place::Action => "action".to_string(),
        };
        let body = if e.body == spec::Body::None { "" } else { ":" };
        s.push_str(&format!(
            "  {:<11} {} {}{}{}\n              {} ({place})\n",
            e.name,
            params(e.positional, false),
            if e.variadic { "... " } else { "" },
            params(e.named, true),
            body,
            e.summary
        ));
    }
    s.push_str("\nCOMMON BLOCK ARGUMENTS\n");
    for p in spec::COMMON {
        s.push_str(&format!("  {:<10} {:<44} {}\n", p.name, p.ty, p.doc));
    }
    for (title, table) in [
        ("book", spec::BOOK_PROPS),
        ("theme", spec::THEME_PROPS),
        ("cover", spec::COVER_PROPS),
        ("back_cover", spec::BACK_COVER_PROPS),
    ] {
        s.push_str(&format!("\nPROPERTIES OF `{title}`\n"));
        for p in table {
            s.push_str(&format!(
                "  {:<13} {:<32} {}{}\n",
                p.name,
                p.ty,
                if p.required { "(required) " } else { "" },
                p.doc
            ));
        }
    }
    s.push_str("\nASSET CONSTRUCTORS (inside `assets:`)\n");
    for c in spec::CONSTRUCTORS {
        s.push_str(&format!(
            "  {:<6} {} {}\n         {}\n",
            c.name,
            params(c.positional, false),
            params(c.named, true),
            c.summary
        ));
    }
    s.push_str(
        "\nSTATE (inside `state:`), one declaration per line\n\
         \x20 count = 0 in 0..5                  whole number with its range\n\
         \x20 seen = false                       boolean\n\
         \x20 route = \"a\" in [\"a\", \"b\"]         text from a closed set\n\
         \nEXPRESSIONS\n\
         \x20 + - *   == != < <= > >=   and or not   ( )     no division, no calls, no loops\n\
         \nACTIONS (only under `button`, `option` and `on_correct`)\n\
         \x20 name = expr | name += expr | name -= expr | goto target | reset | reset name\n\
         \nCONDITIONS\n\
         \x20 when=EXPR on a chapter, section or option; or a block:\n\
         \x20   if count > 2 and route == \"a\":\n\
         \x20       text \"...\"\n\
         \x20   elif seen:\n\
         \x20       text \"...\"\n\
         \x20   else:\n\
         \x20       text \"...\"\n\
         \nCOMPONENTS (declared at the top level, expanded at compile time)\n\
         \x20 component note(line, tone=\"plain\"):     parameters, optionally with defaults\n\
         \x20     text \"{line}\", style=small          `{name}` inserts a parameter into text\n\
         \x20 note \"Mind the tide.\"                  used like an element: positional, then name=value\n\
         \x20 note \"Twice.\", tone=\"loud\"\n\
         \x20 No recursion, no state of their own; the name must not be a built-in element.\n\
         \nTEXT MARKUP\n\
         \x20 *italic*  **bold**  `code`  [label](target)  {variable}  \\{ for a literal brace\n",
    );
    s.push_str("\nRun `darm reference --guide` for the complete guide, `darm reference --json` for the machine-readable form and `darm explain CODE` for diagnostics.\n");
    s
}
