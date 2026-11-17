//! `darm` — the command line of the DARM book toolchain.
//!
//! Exit codes (stable, for scripts and agents):
//!
//! | code | meaning |
//! |---|---|
//! | 0 | success |
//! | 1 | the sources have errors (or warnings with `--deny-warnings`; or `fmt --check` found differences) |
//! | 2 | invalid command line |
//! | 3 | environment problem: a file cannot be read or written, no reader build, bad `darm.toml` |
//! | 4 | the `.darm` package is invalid, damaged, locked, or the credentials are wrong |

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

mod init;
mod inspect;
mod mcp;
mod pack;
mod secrets;
mod serve;
mod workspace;

/// How a command failed. Carries the exit code.
#[derive(Debug)]
pub enum Failure {
    /// Diagnostics were already printed.
    Source,
    /// Environment / I/O problem.
    Env(String),
    /// Package problem.
    Package(String),
}

impl Failure {
    pub fn env(msg: impl Into<String>) -> Failure {
        Failure::Env(msg.into())
    }
}

impl From<darm_package::PackageError> for Failure {
    fn from(e: darm_package::PackageError) -> Self {
        Failure::Package(format!("{}: {e}", e.code()))
    }
}

pub type CmdResult = Result<(), Failure>;

#[derive(Parser)]
#[command(
    name = "darm",
    version,
    about = "DARM: write interactive books in .darml, compile them to .darm, read them anywhere.",
    long_about = "DARM: write interactive books in .darml, compile them to .darm, read them anywhere.\n\n\
        Typical flow:\n  darm init my-book\n  darm check my-book\n  darm build my-book\n  darm open my-book/my-book.darm\n\n\
        Exit codes: 0 ok · 1 source errors · 2 bad command line · 3 I/O or environment · 4 invalid/locked package",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub enum Format {
    /// Text for people, with source excerpts.
    #[default]
    Human,
    /// One JSON document on stdout, for tools and AI agents.
    Json,
}

#[derive(Args, Clone, Default)]
pub struct Unlock {
    /// Read the password from this environment variable.
    #[arg(long, value_name = "VAR")]
    pub password_env: Option<String>,
    /// Read the password from the first line of this file.
    #[arg(long, value_name = "FILE")]
    pub password_file: Option<PathBuf>,
    /// Read the password from standard input.
    #[arg(long)]
    pub password_stdin: bool,
    /// Unlock with a .darmkey key file.
    #[arg(long, value_name = "FILE")]
    pub keyfile: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new book project.
    Init {
        /// Directory to create (must be empty or absent).
        dir: PathBuf,
        /// Book title (default: derived from the directory name).
        #[arg(long)]
        title: Option<String>,
        /// Author name.
        #[arg(long, default_value = "Anonymous")]
        author: String,
        /// BCP 47 language tag.
        #[arg(long, default_value = "en")]
        language: String,
        /// Starting point.
        #[arg(long, value_enum, default_value = "essay")]
        template: init::Template,
    },
    /// Validate a project without writing anything.
    Check {
        /// Project directory or entry .darml file (default: current directory).
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "human")]
        format: Format,
        /// Treat warnings as errors.
        #[arg(long)]
        deny_warnings: bool,
    },
    /// Format .darml files in place.
    Fmt {
        /// Files or directories (default: current directory).
        paths: Vec<PathBuf>,
        /// Do not write; exit 1 if any file would change.
        #[arg(long)]
        check: bool,
        /// Print the formatted text of a single file to stdout instead of writing it.
        #[arg(long)]
        stdout: bool,
    },
    /// Compile a project into a .darm package.
    Build(pack::BuildArgs),
    /// Show what is inside a .darm file.
    Inspect {
        file: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: Format,
        /// Print the book IR as JSON (needs the key for protected books).
        #[arg(long)]
        ir: bool,
        #[command(flatten)]
        unlock: Unlock,
    },
    /// Check every byte of a .darm file against its integrity data.
    Verify {
        file: PathBuf,
        #[command(flatten)]
        unlock: Unlock,
    },
    /// Copy entries out of a .darm file.
    Extract {
        file: PathBuf,
        /// Entry name to extract (see `darm inspect`); repeatable. Default: all.
        #[arg(long = "entry", value_name = "NAME")]
        entries: Vec<String>,
        /// Output directory.
        #[arg(short, long, default_value = "extracted")]
        out: PathBuf,
        #[command(flatten)]
        unlock: Unlock,
    },
    /// Open a .darm file (or build and open a project) in the reader.
    Open(serve::OpenArgs),
    /// Live preview: rebuild on every change and reload the reader.
    Dev(serve::DevArgs),
    /// Generate a new .darmkey key file for protected books.
    Keygen {
        /// Where to write the key file.
        #[arg(short, long, value_name = "FILE")]
        out: PathBuf,
    },
    /// Print the language reference tables.
    Reference {
        /// Machine-readable JSON instead of text.
        #[arg(long)]
        json: bool,
        /// Print docs/errors.md (the diagnostics catalogue) as Markdown.
        #[arg(long, conflicts_with = "json")]
        errors_md: bool,
        /// Print the complete language guide (docs/language.md): lexical rules,
        /// grammar, every element with examples, state, components, covers.
        #[arg(long, conflicts_with_all = ["json", "errors_md"])]
        guide: bool,
    },
    /// Explain a diagnostic code, e.g. `darm explain E0304`.
    Explain { code: String },
    /// Print the JSON Schema of the IR stored in .darm packages.
    Schema,
    /// Run a Model Context Protocol server on stdio (tools: check, check_source, format, build, reference, explain).
    Mcp,
}

fn run(cli: Cli) -> CmdResult {
    match cli.command {
        Command::Init {
            dir,
            title,
            author,
            language,
            template,
        } => init::run(&dir, title, &author, &language, template),
        Command::Check {
            path,
            format,
            deny_warnings,
        } => pack::check(path.as_deref(), format, deny_warnings),
        Command::Fmt {
            paths,
            check,
            stdout,
        } => pack::fmt(&paths, check, stdout),
        Command::Build(args) => pack::build(&args).map(|_| ()),
        Command::Inspect {
            file,
            format,
            ir,
            unlock,
        } => inspect::inspect(&file, format, ir, &unlock),
        Command::Verify { file, unlock } => inspect::verify(&file, &unlock),
        Command::Extract {
            file,
            entries,
            out,
            unlock,
        } => inspect::extract(&file, &entries, &out, &unlock),
        Command::Open(args) => serve::open(&args),
        Command::Dev(args) => serve::dev(&args),
        Command::Keygen { out } => secrets::keygen(&out),
        Command::Reference {
            json,
            errors_md,
            guide,
        } => {
            if guide {
                print!("{}", pack::LANGUAGE_GUIDE);
            } else if errors_md {
                print!("{}", darm_compiler::explain::markdown());
            } else if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&darm_compiler::spec::reference_json()).unwrap()
                );
            } else {
                print!("{}", pack::reference_text());
            }
            Ok(())
        }
        Command::Explain { code } => match darm_compiler::explain::lookup(&code) {
            Some(entry) => {
                print!("{}", darm_compiler::explain::render(entry));
                Ok(())
            }
            None => Err(Failure::env(format!(
                "unknown diagnostic code `{code}`; codes look like E0304 or W0601 (`darm reference --errors-md` lists them all)"
            ))),
        },
        Command::Schema => {
            let schema = schemars::schema_for!(darm_ir::Book);
            println!("{}", serde_json::to_string_pretty(&schema).unwrap());
            Ok(())
        }
        Command::Mcp => mcp::run(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Source) => ExitCode::from(1),
        Err(Failure::Env(msg)) => {
            eprintln!("darm: {msg}");
            ExitCode::from(3)
        }
        Err(Failure::Package(msg)) => {
            eprintln!("darm: {msg}");
            ExitCode::from(4)
        }
    }
}
