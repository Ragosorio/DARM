//! `darm open` and `darm dev`: a small local server that hosts the reader
//! and the book, so a `.darm` file can be read in any modern browser.
//!
//! The server only listens on 127.0.0.1, only answers GET/HEAD, checks the
//! `Host` header (DNS-rebinding defence) and never lists directories.

use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use clap::Args;

use crate::pack::{self, BuildArgs};
use crate::workspace::Project;
use crate::{CmdResult, Failure, Format};

#[derive(Args, Clone)]
pub struct OpenArgs {
    /// A .darm file, a project directory or an entry .darml file.
    pub target: PathBuf,
    #[command(flatten)]
    pub server: ServerArgs,
}

#[derive(Args, Clone)]
pub struct DevArgs {
    /// Project directory or entry .darml file (default: current directory).
    pub path: Option<PathBuf>,
    #[command(flatten)]
    pub server: ServerArgs,
}

#[derive(Args, Clone)]
pub struct ServerArgs {
    /// Port to listen on (default: pick a free one).
    #[arg(long, default_value_t = 0)]
    pub port: u16,
    /// Do not launch a browser; just print the address.
    #[arg(long)]
    pub no_browser: bool,
    /// Directory of a built reader (default: bundled, $DARM_READER_DIR, or apps/reader/dist).
    #[arg(long, value_name = "DIR")]
    pub reader: Option<PathBuf>,
}

#[cfg(feature = "embed-reader")]
static EMBEDDED: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../apps/reader/dist");

enum ReaderFiles {
    Dir(PathBuf),
    #[cfg(feature = "embed-reader")]
    Embedded,
}

impl ReaderFiles {
    fn locate(explicit: Option<&Path>) -> Result<ReaderFiles, Failure> {
        let is_reader = |dir: &Path| dir.join("index.html").is_file();
        if let Some(dir) = explicit {
            return if is_reader(dir) {
                Ok(ReaderFiles::Dir(dir.to_path_buf()))
            } else {
                Err(Failure::env(format!(
                    "`{}` does not contain a built reader (no index.html)",
                    dir.display()
                )))
            };
        }
        if let Some(dir) = std::env::var_os("DARM_READER_DIR").map(PathBuf::from)
            && is_reader(&dir)
        {
            return Ok(ReaderFiles::Dir(dir));
        }
        #[cfg(feature = "embed-reader")]
        {
            return Ok(ReaderFiles::Embedded);
        }
        #[cfg(not(feature = "embed-reader"))]
        {
            let mut candidates = Vec::new();
            if let Ok(exe) = std::env::current_exe()
                && let Some(dir) = exe.parent()
            {
                candidates.push(dir.join("reader"));
                candidates.push(dir.join("../share/darm/reader"));
                // Running from a source checkout: target/<profile>/darm.
                candidates.push(dir.join("../../apps/reader/dist"));
            }
            if let Ok(cwd) = std::env::current_dir() {
                for up in cwd.ancestors().take(6) {
                    candidates.push(up.join("apps/reader/dist"));
                }
            }
            candidates.into_iter().find(|d| is_reader(d)).map(ReaderFiles::Dir).ok_or_else(|| {
                Failure::env(
                    "no reader build found. Build it with `npm --prefix apps/reader ci && npm --prefix apps/reader run build`, \
                     then pass --reader apps/reader/dist or set DARM_READER_DIR (or build darm with `--features embed-reader`)",
                )
            })
        }
    }

    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        let safe = Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
        if !safe {
            return None;
        }
        match self {
            ReaderFiles::Dir(dir) => std::fs::read(dir.join(rel)).ok(),
            #[cfg(feature = "embed-reader")]
            ReaderFiles::Embedded => EMBEDDED.get_file(rel).map(|f| f.contents().to_vec()),
        }
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// State shared between the server and the rebuild loop.
struct Shared {
    book: PathBuf,
    name: String,
    dev: bool,
    /// Bumped after every rebuild.
    version: Mutex<(u64, serde_json::Value)>,
    changed: Condvar,
}

/// A parsed request: method, target and the two headers the server cares about.
struct HttpRequest {
    method: String,
    target: String,
    host: Option<String>,
    range: Option<String>,
}

/// What to send back.
struct Reply {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    extra: Vec<(&'static str, String)>,
}

impl Reply {
    fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Reply {
        Reply {
            status,
            content_type,
            body: body.into(),
            extra: Vec::new(),
        }
    }

    fn text(status: u16, message: &str) -> Reply {
        Reply::new(status, "text/plain; charset=utf-8", message.as_bytes())
    }
}

/// Largest request head accepted.
const MAX_HEAD: usize = 16 * 1024;

/// Read one request head. The server only ever answers GET and HEAD, so there
/// is no body to read.
fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<HttpRequest>> {
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut head = Vec::new();
    let mut buf = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(None); // the client connected and went away
        }
        head.extend_from_slice(&buf[..n]);
        if head.len() > MAX_HEAD {
            return Ok(None);
        }
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(target)) = (first.next(), first.next()) else {
        return Ok(None);
    };
    let mut request = HttpRequest {
        method: method.to_string(),
        target: target.to_string(),
        host: None,
        range: None,
    };
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "host" => request.host = Some(value.trim().to_string()),
                "range" => request.range = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }
    Ok(Some(request))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        _ => "Internal Server Error",
    }
}

/// Write the reply and close. One request per connection keeps the server
/// trivially correct: there is no connection state to get wrong.
fn write_reply(stream: &mut TcpStream, reply: &Reply, head_only: bool) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\
         X-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\nCache-Control: no-store\r\n",
        reply.status,
        reason(reply.status),
        reply.content_type,
        reply.body.len()
    );
    for (name, value) in &reply.extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    stream.write_all(head.as_bytes())?;
    if !head_only {
        stream.write_all(&reply.body)?;
    }
    stream.flush()?;
    stream.shutdown(std::net::Shutdown::Write)
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n` against a file of `len` bytes.
fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    let spec = value.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (len.saturating_sub(n), len.checked_sub(1)?)
        }
        (s, "") => (s.parse().ok()?, len.checked_sub(1)?),
        (s, e) => (
            s.parse().ok()?,
            e.parse::<u64>().ok()?.min(len.checked_sub(1)?),
        ),
    };
    (start <= end && start < len).then_some((start, end))
}

fn serve_book(request: &HttpRequest, shared: &Shared) -> Reply {
    let mut file = match std::fs::File::open(&shared.book) {
        Ok(f) => f,
        Err(e) => return Reply::text(404, &format!("book not available: {e}")),
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let (status, start, end) = match request.range.as_deref().map(|r| parse_range(r, len)) {
        None => (200, 0, len.saturating_sub(1)),
        Some(Some((s, e))) => (206, s, e),
        Some(None) => {
            let mut reply = Reply::text(416, "invalid range");
            reply
                .extra
                .push(("Content-Range", format!("bytes */{len}")));
            return reply;
        }
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    // Cap one response; the reader asks for what it needs in pieces.
    let count = count.min(64 * 1024 * 1024);
    let mut body = vec![0u8; count as usize];
    if file
        .seek(SeekFrom::Start(start))
        .and_then(|_| file.read_exact(&mut body))
        .is_err()
    {
        return Reply::text(500, "read error");
    }
    let mut reply = Reply::new(status, "application/vnd.darm", body);
    reply.extra.push(("Accept-Ranges", "bytes".to_string()));
    if status == 206 {
        reply.extra.push((
            "Content-Range",
            format!("bytes {start}-{}/{len}", start + count.saturating_sub(1)),
        ));
    }
    reply
}

fn handle(
    request: &HttpRequest,
    reader: &ReaderFiles,
    shared: &Shared,
    authority: &[String],
) -> Reply {
    // Only the loopback names this server was started on: a page on another
    // site cannot reach it through a DNS name that resolves to 127.0.0.1.
    if !request
        .host
        .as_ref()
        .is_some_and(|h| authority.iter().any(|a| a == h))
    {
        return Reply::text(403, "forbidden host");
    }
    if request.method != "GET" && request.method != "HEAD" {
        return Reply::text(405, "method not allowed");
    }
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((&request.target, ""));
    match path {
        "/__darm/book.darm" => serve_book(request, shared),
        "/__darm/info" => {
            let (version, diagnostics) = shared.version.lock().unwrap().clone();
            let doc = serde_json::json!({ "name": shared.name, "dev": shared.dev, "version": version, "build": diagnostics });
            Reply::new(200, "application/json", doc.to_string())
        }
        "/__darm/wait" => {
            // Long poll: answer when the build version differs from `v`, or after 25 s.
            let seen: u64 = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("v="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let guard = shared.version.lock().unwrap();
            let (guard, _) = shared
                .changed
                .wait_timeout_while(guard, Duration::from_secs(25), |(v, _)| *v == seen)
                .unwrap();
            let doc = serde_json::json!({ "version": guard.0, "build": guard.1 });
            drop(guard);
            Reply::new(200, "application/json", doc.to_string())
        }
        _ => {
            let rel = path.trim_start_matches('/');
            let rel = if rel.is_empty() { "index.html" } else { rel };
            match reader.read(rel) {
                Some(body) => Reply::new(200, content_type(rel), body),
                None => Reply::text(404, "not found"),
            }
        }
    }
}

fn connection(mut stream: TcpStream, reader: &ReaderFiles, shared: &Shared, authority: &[String]) {
    let reply = match read_request(&mut stream) {
        Ok(Some(request)) => {
            let head_only = request.method == "HEAD";
            let reply = handle(&request, reader, shared, authority);
            let _ = write_reply(&mut stream, &reply, head_only);
            return;
        }
        Ok(None) => Reply::text(400, "bad request"),
        Err(_) => return,
    };
    let _ = write_reply(&mut stream, &reply, false);
}

fn launch_browser(url: &str) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
    } else {
        std::process::Command::new("xdg-open").arg(url).status()
    };
    if !result.is_ok_and(|s| s.success()) {
        eprintln!("could not launch a browser; open the address above manually");
    }
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn serve(args: &ServerArgs, shared: Arc<Shared>) -> CmdResult {
    let reader = Arc::new(ReaderFiles::locate(args.reader.as_deref())?);
    let listener = TcpListener::bind(("127.0.0.1", args.port))
        .map_err(|e| Failure::env(format!("cannot listen on 127.0.0.1:{}: {e}", args.port)))?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(args.port);
    let authority = Arc::new(vec![
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
    ]);
    let url = format!(
        "http://127.0.0.1:{port}/?book=/__darm/book.darm&name={}{}",
        percent_encode(&shared.name),
        if shared.dev { "&dev=1" } else { "" }
    );
    println!("reader: {url}");
    println!("press Ctrl+C to stop");
    if !args.no_browser {
        launch_browser(&url);
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let (reader, shared, authority) = (reader.clone(), shared.clone(), authority.clone());
        std::thread::spawn(move || connection(stream, &reader, &shared, &authority));
    }
    Ok(())
}

pub fn open(args: &OpenArgs) -> CmdResult {
    let is_package =
        args.target.is_file() && args.target.extension().and_then(|e| e.to_str()) == Some("darm");
    let book = if is_package {
        // Fail early with a precise message if the file is not a DARM package.
        crate::inspect::probe_file(&args.target)?;
        args.target.clone()
    } else {
        pack::build(&BuildArgs {
            path: Some(args.target.clone()),
            format: Format::Human,
            ..BuildArgs::default()
        })?
        .output
    };
    let name = book
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("book.darm")
        .to_string();
    let shared = Arc::new(Shared {
        book,
        name,
        dev: false,
        version: Mutex::new((1, serde_json::json!({ "ok": true }))),
        changed: Condvar::new(),
    });
    serve(&args.server, shared)
}

/// Newest modification time under `root`, ignoring hidden directories and build output.
fn newest_mtime(root: &Path, skip: &Path) -> SystemTime {
    fn walk(dir: &Path, skip: &Path, newest: &mut SystemTime, budget: &mut u32) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" || name == "target" || path == skip {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&path, skip, newest, budget);
            } else if name.ends_with(".darm") {
                continue;
            } else if let Ok(m) = meta.modified()
                && m > *newest
            {
                *newest = m;
            }
        }
    }
    let mut newest = SystemTime::UNIX_EPOCH;
    walk(root, skip, &mut newest, &mut 50_000);
    newest
}

pub fn dev(args: &DevArgs) -> CmdResult {
    let project = Project::locate(args.path.as_deref())?;
    let cache = project.root.join(".darm-cache");
    std::fs::create_dir_all(&cache)
        .map_err(|e| Failure::env(format!("cannot create `{}`: {e}", cache.display())))?;
    let output = cache.join("preview.darm");
    // The preview is always unencrypted: it never leaves this machine.
    let build_args = BuildArgs {
        path: args.path.clone(),
        out: Some(output.clone()),
        format: Format::Human,
        ..BuildArgs::default()
    };
    let build_once = |args: &BuildArgs| -> serde_json::Value {
        match pack::build(args) {
            Ok(built) => {
                serde_json::json!({ "ok": true, "diagnostics": built.compilation.to_json() })
            }
            Err(_) => {
                // Re-run the front end only, to hand the diagnostics to the reader overlay.
                let diagnostics = Project::locate(args.path.as_deref())
                    .ok()
                    .and_then(|p| pack::compile(&p).ok())
                    .map(|c| serde_json::json!({ "text": c.render(false), "list": c.to_json() }))
                    .unwrap_or_else(|| serde_json::json!({ "text": "the project could not be read", "list": [] }));
                serde_json::json!({ "ok": false, "diagnostics": diagnostics["list"], "text": diagnostics["text"] })
            }
        }
    };
    let first = build_once(&build_args);
    if !output.is_file() {
        return Err(Failure::Source);
    }
    let name = format!(
        "{} (live preview)",
        project
            .root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("book")
    );
    let shared = Arc::new(Shared {
        book: output.clone(),
        name,
        dev: true,
        version: Mutex::new((1, first)),
        changed: Condvar::new(),
    });

    let watcher = shared.clone();
    let root = project.root.clone();
    std::thread::spawn(move || {
        let mut last = newest_mtime(&root, &cache);
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let now = newest_mtime(&root, &cache);
            if now > last {
                last = now;
                eprintln!("\n— change detected, rebuilding —");
                let result = build_once(&build_args);
                let mut guard = watcher.version.lock().unwrap();
                guard.0 += 1;
                guard.1 = result;
                watcher.changed.notify_all();
            }
        }
    });
    serve(&args.server, shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=0-5000", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=5-2", 1000), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
    }

    #[test]
    fn reader_paths_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("darm-serve-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "x").unwrap();
        let files = ReaderFiles::Dir(dir.clone());
        assert!(files.read("index.html").is_some());
        assert!(files.read("../index.html").is_none());
        assert!(files.read("/etc/passwd").is_none());
        assert!(files.read("a/../../x").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
