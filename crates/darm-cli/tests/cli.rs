//! End-to-end tests of the `darm` binary: the acceptance flow of the project
//! (create → edit → format → validate → compile → inspect → protect → serve)
//! exercised exactly as a user or an agent would, through the command line.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const DARM: &str = env!("CARGO_BIN_EXE_darm");

struct Workspace(PathBuf);

impl Workspace {
    fn new(name: &str) -> Workspace {
        let dir = std::env::temp_dir().join(format!("darm-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Workspace(dir)
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn darm(args: &[&str]) -> Output {
    Command::new(DARM)
        .args(args)
        .stdin(Stdio::null())
        .env("NO_COLOR", "1")
        .output()
        .expect("run darm")
}

fn darm_env(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(DARM);
    c.args(args).stdin(Stdio::null()).env("NO_COLOR", "1");
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().expect("run darm")
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn create_edit_format_check_build_inspect() {
    let ws = Workspace::new("flow");
    let project = ws.path("my-book");
    let o = darm(&[
        "init",
        s(&project),
        "--author",
        "Test Author",
        "--title",
        "A Test Book",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(project.join("book.darml").is_file() && project.join("darm.toml").is_file());
    // A second init must not overwrite work.
    assert_eq!(code(&darm(&["init", s(&project)])), 3);

    // The fresh project is valid and already formatted.
    assert_eq!(code(&darm(&["check", s(&project)])), 0);
    assert_eq!(code(&darm(&["fmt", "--check", s(&project)])), 0);

    // Edit the source (badly formatted on purpose).
    let source = project.join("book.darml");
    let mut text = std::fs::read_to_string(&source).unwrap();
    text.push_str("\nchapter \"Added later\",id=added:\n  text \"A new chapter written by the test.\"\n  button \"Back\",goto=beginning\n");
    std::fs::write(&source, &text).unwrap();
    assert_eq!(
        code(&darm(&["fmt", "--check", s(&project)])),
        1,
        "unformatted source must be reported"
    );
    assert_eq!(code(&darm(&["fmt", s(&project)])), 0);
    assert_eq!(code(&darm(&["fmt", "--check", s(&project)])), 0);
    assert!(
        std::fs::read_to_string(&source)
            .unwrap()
            .contains("chapter \"Added later\", id=added:\n    text")
    );

    // Validate with machine-readable output.
    let o = darm(&["check", s(&project), "--format", "json"]);
    assert_eq!(code(&o), 0);
    let report: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["errors"], 0);

    // Compile.
    let o = darm(&["build", s(&project), "--format", "json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let built: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    let package = PathBuf::from(built["output"].as_str().unwrap());
    assert_eq!(package.file_name().unwrap(), "a-test-book.darm");
    assert_eq!(built["encrypted"], false);
    let bytes = std::fs::read(&package).unwrap();
    assert_eq!(
        &bytes[..8],
        b"\x89DARM\r\n\x1a",
        "a .darm starts with the DARM magic, it is not JSON or zip"
    );
    assert_eq!(built["bytes"].as_u64().unwrap() as usize, bytes.len());

    // Builds are reproducible.
    let again = ws.path("again.darm");
    assert_eq!(code(&darm(&["build", s(&project), "-o", s(&again)])), 0);
    assert_eq!(
        std::fs::read(&again).unwrap(),
        bytes,
        "same sources must give a byte-identical package"
    );

    // Inspect, verify, extract.
    let o = darm(&["inspect", s(&package), "--format", "json"]);
    assert_eq!(code(&o), 0);
    let info: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(info["format"], "1.0");
    assert_eq!(info["book"]["title"], "A Test Book");
    assert!(
        info["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "res/landscape.svg")
    );
    let o = darm(&["inspect", s(&package), "--ir"]);
    let ir: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(ir["ir_version"], "1.0");
    assert!(ir["anchors"].get("added").is_some());
    let o = darm(&["verify", s(&package)]);
    assert_eq!(code(&o), 0);
    assert!(out(&o).starts_with("ok:"));
    let dest = ws.path("extracted");
    assert_eq!(code(&darm(&["extract", s(&package), "-o", s(&dest)])), 0);
    assert_eq!(
        std::fs::read(dest.join("res/landscape.svg")).unwrap(),
        std::fs::read(project.join("art/landscape.svg")).unwrap()
    );
}

#[test]
fn source_errors_are_reported_with_positions_and_exit_code_1() {
    let ws = Workspace::new("errors");
    let project = ws.path("book");
    assert_eq!(
        code(&darm(&["init", s(&project), "--template", "interactive"])),
        0
    );

    // A missing asset fails the build and names the file and position.
    std::fs::remove_file(project.join("art/landscape.svg")).unwrap();
    let o = darm(&["build", s(&project)]);
    assert_eq!(code(&o), 1);
    let e = err(&o);
    assert!(
        e.contains("error[E0401]: file not found: `art/landscape.svg`"),
        "{e}"
    );
    assert!(e.contains("--> book.darml:"), "{e}");
    assert!(
        !project.join("book.darm").exists(),
        "no package may be written when the build fails"
    );
    let o = darm(&["check", s(&project), "--format", "json"]);
    assert_eq!(code(&o), 1);
    let report: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    let d = &report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "E0401")
        .unwrap();
    assert!(d["line"].as_u64().unwrap() > 0 && d["column"].as_u64().unwrap() > 0);
    assert!(
        d["help"]
            .as_str()
            .unwrap()
            .contains("relative to the source file")
    );

    // Warnings only fail with --deny-warnings.
    let ok = ws.path("warn");
    std::fs::create_dir_all(&ok).unwrap();
    std::fs::write(ok.join("book.darml"), "darm 1\nbook \"W\":\n    author = \"A\"\n    language = \"en\"\nchapter \"One\":\n    text \"x\"\n").unwrap();
    assert_eq!(code(&darm(&["check", s(&ok)])), 0);
    assert_eq!(code(&darm(&["check", s(&ok), "--deny-warnings"])), 1);

    // Environment problems are exit code 3, usage errors 2.
    assert_eq!(code(&darm(&["check", s(&ws.path("nowhere"))])), 3);
    assert_eq!(code(&darm(&["frobnicate"])), 2);
    assert_eq!(code(&darm(&["explain", "E9999"])), 3);
}

#[test]
fn protected_books() {
    let ws = Workspace::new("protect");
    let project = ws.path("book");
    assert_eq!(
        code(&darm(&["init", s(&project), "--title", "Secret Harbour"])),
        0
    );
    let clear = ws.path("clear.darm");
    let locked = ws.path("locked.darm");
    assert_eq!(code(&darm(&["build", s(&project), "-o", s(&clear)])), 0);
    let fast = ["--kdf-memory", "8", "--kdf-passes", "1"];
    let pw = [("BOOK_PW", "correct horse battery staple")];

    let mut args = vec![
        "build",
        s(&project),
        "-o",
        s(&locked),
        "--encrypt",
        "--password-env",
        "BOOK_PW",
    ];
    args.extend(fast);
    let o = darm_env(&args, &pw);
    assert_eq!(code(&o), 0, "{}", err(&o));

    // The encrypted file reveals nothing but the public card.
    let bytes = std::fs::read(&locked).unwrap();
    let hay = String::from_utf8_lossy(&bytes);
    for secret in [
        "A beginning",
        "landscape",
        "drop capital",
        "chapter",
        "res/",
    ] {
        assert!(
            !hay.contains(secret),
            "plaintext `{secret}` leaked into the encrypted package"
        );
    }
    assert!(
        hay.contains("Secret Harbour"),
        "the public card is readable unless --hide-card is given"
    );
    let o = darm(&["inspect", s(&locked), "--format", "json"]);
    assert_eq!(code(&o), 0);
    let info: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(info["encrypted"], true);
    assert_eq!(info["locked"], true);
    assert!(info.get("entries").is_none());

    // Right password opens; wrong or missing one is refused with exit code 4.
    assert_eq!(
        code(&darm_env(
            &["verify", s(&locked), "--password-env", "BOOK_PW"],
            &pw
        )),
        0
    );
    let o = darm_env(
        &["verify", s(&locked), "--password-env", "BOOK_PW"],
        &[("BOOK_PW", "wrong password 123")],
    );
    assert_eq!(code(&o), 4);
    assert!(err(&o).contains("E_PKG_WRONG_CREDENTIALS"));
    let o = darm(&["verify", s(&locked)]);
    assert_eq!(code(&o), 4);
    assert!(err(&o).contains("E_PKG_CREDENTIALS_REQUIRED"));
    let o = darm_env(
        &["inspect", s(&locked), "--ir", "--password-env", "BOOK_PW"],
        &pw,
    );
    assert!(out(&o).contains("\"ir_version\""));

    // Any modification is detected, in clear and encrypted packages.
    for (file, env) in [(&clear, &[][..]), (&locked, &pw[..])] {
        let original = std::fs::read(file).unwrap();
        for at in [original.len() / 3, original.len() / 2, original.len() - 5] {
            let mut bad = original.clone();
            bad[at] ^= 0x40;
            let tampered = ws.path("tampered.darm");
            std::fs::write(&tampered, &bad).unwrap();
            let mut args = vec!["verify", s(&tampered)];
            if !env.is_empty() {
                args.extend(["--password-env", "BOOK_PW"]);
            }
            let o = darm_env(&args, env);
            assert_eq!(
                code(&o),
                4,
                "a flipped byte at {at} went unnoticed: {}",
                out(&o)
            );
        }
        let truncated = ws.path("truncated.darm");
        std::fs::write(&truncated, &original[..original.len() - 64]).unwrap();
        assert_eq!(
            code(&darm_env(
                &["verify", s(&truncated), "--password-env", "BOOK_PW"],
                &pw
            )),
            4
        );
    }
    std::fs::write(
        ws.path("garbage.darm"),
        b"this is not a book at all, just text",
    )
    .unwrap();
    let o = darm(&["inspect", s(&ws.path("garbage.darm"))]);
    assert_eq!(code(&o), 4);

    // Hidden card.
    let hidden = ws.path("hidden.darm");
    let mut args = vec![
        "build",
        s(&project),
        "-o",
        s(&hidden),
        "--encrypt",
        "--password-env",
        "BOOK_PW",
        "--hide-card",
    ];
    args.extend(fast);
    assert_eq!(code(&darm_env(&args, &pw)), 0);
    assert!(!String::from_utf8_lossy(&std::fs::read(&hidden).unwrap()).contains("Secret Harbour"));

    // Key files: generated next to (never inside) the book, and they unlock it.
    let key = ws.path("book.darmkey");
    let keyed = ws.path("keyed.darm");
    assert_eq!(
        code(&darm(&[
            "build",
            s(&project),
            "-o",
            s(&keyed),
            "--new-keyfile",
            s(&key)
        ])),
        0
    );
    assert_eq!(code(&darm(&["verify", s(&keyed), "--keyfile", s(&key)])), 0);
    assert_eq!(code(&darm(&["verify", s(&keyed)])), 4);
    let other = ws.path("other.darmkey");
    assert_eq!(code(&darm(&["keygen", "-o", s(&other)])), 0);
    assert_eq!(
        code(&darm(&["keygen", "-o", s(&other)])),
        3,
        "an existing key must never be overwritten"
    );
    assert_eq!(
        code(&darm(&["verify", s(&keyed), "--keyfile", s(&other)])),
        4
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // Weak passwords and missing secrets are refused.
    let x = ws.path("x.darm");
    let mut args = vec![
        "build",
        s(&project),
        "-o",
        s(&x),
        "--encrypt",
        "--password-env",
        "BOOK_PW",
    ];
    args.extend(fast);
    assert_eq!(code(&darm_env(&args, &[("BOOK_PW", "short")])), 3);
    assert_eq!(
        code(&darm(&["build", s(&project), "-o", s(&x), "--encrypt"])),
        3
    );
}

#[test]
fn reference_schema_and_explain_are_machine_readable() {
    let reference: serde_json::Value =
        serde_json::from_str(&out(&darm(&["reference", "--json"]))).unwrap();
    assert_eq!(reference["grammar_version"], 1);
    assert!(
        reference["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "choice")
    );
    let schema: serde_json::Value = serde_json::from_str(&out(&darm(&["schema"]))).unwrap();
    assert!(schema["properties"].get("chapters").is_some());
    let o = darm(&["explain", "e0304"]);
    assert_eq!(code(&o), 0);
    assert!(out(&o).contains("unknown asset"));
    assert!(out(&darm(&["reference", "--errors-md"])).contains("## E0401"));
    assert!(out(&darm(&["reference"])).contains("ELEMENTS"));
    let help = out(&darm(&["--help"]));
    for cmd in [
        "init",
        "check",
        "fmt",
        "build",
        "inspect",
        "verify",
        "extract",
        "open",
        "dev",
        "keygen",
        "reference",
        "explain",
        "schema",
        "mcp",
    ] {
        assert!(help.contains(cmd), "--help does not mention `{cmd}`");
    }
}

#[test]
fn mcp_server_speaks_json_rpc_over_stdio() {
    let ws = Workspace::new("mcp");
    let project = ws.path("book");
    assert_eq!(code(&darm(&["init", s(&project)])), 0);
    let mut child = Command::new(DARM)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut call = |msg: serde_json::Value| -> serde_json::Value {
        writeln!(stdin, "{msg}").unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };
    let init = call(
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "darm");
    let tools = call(serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "darm_build")
    );
    let check = call(
        serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "darm_check", "arguments": {"path": s(&project)}}}),
    );
    assert_eq!(check["result"]["isError"], false);
    assert!(
        check["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"ok\": true")
    );
    let built = call(
        serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "darm_build", "arguments": {"path": s(&project), "out": s(&ws.path("m.darm"))}}}),
    );
    assert_eq!(built["result"]["isError"], false, "{built}");
    assert!(ws.path("m.darm").is_file());
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

fn http_get(port: u16, path: &str, headers: &[(&str, &str)]) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut req = format!("GET {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http response");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.to_ascii_lowercase(), raw[split + 4..].to_vec())
}

#[test]
fn open_serves_the_reader_and_the_book_locally() {
    let ws = Workspace::new("serve");
    let project = ws.path("book");
    assert_eq!(code(&darm(&["init", s(&project)])), 0);
    let package = ws.path("b.darm");
    assert_eq!(code(&darm(&["build", s(&project), "-o", s(&package)])), 0);
    let reader = ws.path("reader");
    std::fs::create_dir_all(reader.join("assets")).unwrap();
    std::fs::write(
        reader.join("index.html"),
        "<!doctype html><title>reader</title>",
    )
    .unwrap();
    std::fs::write(reader.join("assets/core.wasm"), b"\0asm").unwrap();

    let mut child = Command::new(DARM)
        .args(["open", s(&package), "--no-browser", "--reader", s(&reader)])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    let port: u16 = first
        .split("127.0.0.1:")
        .nth(1)
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(first.contains("?book=/__darm/book.darm"), "{first}");

    let result = std::panic::catch_unwind(|| {
        let (status, head, body) = http_get(port, "/", &[]);
        assert_eq!(status, 200);
        assert!(head.contains("content-type: text/html"));
        assert!(String::from_utf8_lossy(&body).contains("reader"));
        let (_, head, _) = http_get(port, "/assets/core.wasm", &[]);
        assert!(head.contains("content-type: application/wasm"));

        let bytes = std::fs::read(&package).unwrap();
        let (status, _, body) = http_get(port, "/__darm/book.darm", &[]);
        assert_eq!(status, 200);
        assert_eq!(body, bytes);
        let (status, head, body) = http_get(port, "/__darm/book.darm", &[("Range", "bytes=8-23")]);
        assert_eq!(status, 206);
        assert!(
            head.contains(&format!("content-range: bytes 8-23/{}", bytes.len())),
            "{head}"
        );
        assert_eq!(body, &bytes[8..24]);
        assert_eq!(
            http_get(port, "/__darm/book.darm", &[("Range", "bytes=999999999-")]).0,
            416
        );

        // No path traversal, no foreign hosts, read-only.
        assert_eq!(http_get(port, "/../b.darm", &[]).0, 404);
        assert_eq!(http_get(port, "/%2e%2e/b.darm", &[]).0, 404);
        assert_eq!(http_get(port, "/", &[("Host", "evil.example.com")]).0, 403);
        let info: serde_json::Value =
            serde_json::from_slice(&http_get(port, "/__darm/info", &[]).2).unwrap();
        assert_eq!(info["dev"], false);
    });
    child.kill().unwrap();
    let _ = child.wait();
    result.unwrap();
}
