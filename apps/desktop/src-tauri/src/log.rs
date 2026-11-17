//! Minimal diagnostics for the shell. Never logs book content.
//!
//! * Every line goes to stderr (invisible when launched from Finder/Explorer,
//!   visible from a terminal or `open --stderr <file>`).
//! * `DARM_SHELL_LOG=<path>` additionally appends to that file. This is the
//!   only way to see the log of a release build on Windows, which has no console.
//! * `DARM_SHELL_TRACE=1` adds one line per webview command (numbers only).

use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn log(message: impl AsRef<str>) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!(
        "[darm-shell {}.{:03} pid {}] {}\n",
        now / 1000,
        now % 1000,
        std::process::id(),
        message.as_ref()
    );
    // `eprintln!` would panic if stderr is closed; a log line is never worth that.
    let _ = std::io::stderr().write_all(line.as_bytes());
    if let Some(path) = std::env::var_os("DARM_SHELL_LOG").filter(|p| !p.is_empty())
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("DARM_SHELL_TRACE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

pub fn trace(message: impl FnOnce() -> String) {
    if trace_enabled() {
        log(message());
    }
}
