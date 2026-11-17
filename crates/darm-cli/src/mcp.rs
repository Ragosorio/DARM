//! `darm mcp`: a minimal Model Context Protocol server over stdio.
//!
//! It lets AI agents validate, format and build DARM projects and read the
//! language reference without shelling out. Transport: newline-delimited
//! JSON-RPC 2.0 on stdin/stdout, as specified for MCP stdio servers. Only the
//! `tools` capability is implemented.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::pack::{self, BuildArgs};
use crate::workspace::Project;
use crate::{CmdResult, Failure, Format};

const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

fn tools() -> Value {
    let path = json!({ "type": "string", "description": "Project directory or entry .darml file" });
    json!([
        {
            "name": "darm_check",
            "description": "Validate a DARM project on disk. Returns diagnostics as JSON (code, message, file, line, column, help).",
            "inputSchema": { "type": "object", "properties": { "path": path }, "required": ["path"] }
        },
        {
            "name": "darm_check_source",
            "description": "Validate a single .darml source given as text (no assets on disk). Returns diagnostics as JSON.",
            "inputSchema": { "type": "object", "properties": { "source": { "type": "string" } }, "required": ["source"] }
        },
        {
            "name": "darm_format",
            "description": "Format .darml source text canonically. Returns the formatted text, or diagnostics if it does not parse.",
            "inputSchema": { "type": "object", "properties": { "source": { "type": "string" } }, "required": ["source"] }
        },
        {
            "name": "darm_build",
            "description": "Compile a project to an unencrypted .darm package. Returns the output path and diagnostics.",
            "inputSchema": { "type": "object", "properties": { "path": path, "out": { "type": "string", "description": "Output file (optional)" } }, "required": ["path"] }
        },
        {
            "name": "darm_reference",
            "description": "The DARM language reference as JSON: elements, arguments, properties, asset constructors, diagnostics.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "darm_explain",
            "description": "Explain a diagnostic code such as E0304, with a failing example and its fix.",
            "inputSchema": { "type": "object", "properties": { "code": { "type": "string" } }, "required": ["code"] }
        }
    ])
}

fn compilation_json(c: &darm_compiler::Compilation) -> Value {
    json!({ "ok": !c.has_errors(), "errors": c.error_count(), "warnings": c.warning_count(), "diagnostics": c.to_json() })
}

/// Run one tool. `Err` is a tool-level error shown to the model.
fn call(name: &str, args: &Value) -> Result<String, String> {
    let string = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("missing string argument `{key}`"))
    };
    let pretty = |v: &Value| serde_json::to_string_pretty(v).unwrap();
    let failure = |f: Failure| match f {
        Failure::Source => "the sources have errors".to_string(),
        Failure::Env(m) | Failure::Package(m) => m,
    };
    match name {
        "darm_check" => {
            let project =
                Project::locate(Some(&PathBuf::from(string("path")?))).map_err(failure)?;
            let c = pack::compile(&project).map_err(failure)?;
            Ok(pretty(&compilation_json(&c)))
        }
        "darm_check_source" => Ok(pretty(&compilation_json(&darm_compiler::compile_str(
            string("source")?,
        )))),
        "darm_format" => {
            let source = string("source")?;
            match darm_lang::fmt::format_source(darm_lang::FileId(0), "source.darml", source) {
                Ok(text) => Ok(text),
                Err(diags) => {
                    let mut map = darm_lang::SourceMap::new();
                    map.add("source.darml", source);
                    Err(pretty(&Value::Array(
                        diags.iter().map(|d| d.to_json(&map)).collect(),
                    )))
                }
            }
        }
        "darm_build" => {
            let args = BuildArgs {
                path: Some(PathBuf::from(string("path")?)),
                out: args.get("out").and_then(Value::as_str).map(PathBuf::from),
                // Human format keeps stdout (the JSON-RPC channel) clean: it logs to stderr.
                format: Format::Human,
                ..BuildArgs::default()
            };
            match pack::build(&args) {
                Ok(built) => {
                    let mut doc = compilation_json(&built.compilation);
                    doc["output"] = built.output.display().to_string().into();
                    Ok(pretty(&doc))
                }
                Err(Failure::Source) => {
                    let project = Project::locate(args.path.as_deref()).map_err(failure)?;
                    Err(pretty(&compilation_json(
                        &pack::compile(&project).map_err(failure)?,
                    )))
                }
                Err(other) => Err(failure(other)),
            }
        }
        "darm_reference" => Ok(pretty(&darm_compiler::spec::reference_json())),
        "darm_explain" => darm_compiler::explain::lookup(string("code")?)
            .map(darm_compiler::explain::render)
            .ok_or_else(|| "unknown diagnostic code".to_string()),
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn handle(message: &Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == requested)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "darm", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Tools for the DARM interactive-book language. Call darm_reference first; fix diagnostics by code (darm_explain)."
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (text, is_error) = match call(name, &args) {
                Ok(t) => (t, false),
                Err(t) => (t, true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        m if m.starts_with("notifications/") => return None,
        _ => Err(json!({ "code": -32601, "message": format!("method not found: {method}") })),
    };
    // Requests without an id are notifications and get no response.
    let id = id?;
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

pub fn run() -> CmdResult {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| Failure::env(format!("cannot read stdin: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(&message),
            Err(e) => Some(
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } }),
            ),
        };
        if let Some(r) = response {
            writeln!(stdout, "{r}")
                .and_then(|_| stdout.flush())
                .map_err(|e| Failure::env(format!("cannot write stdout: {e}")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_basics() {
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2024-11-05" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
        assert!(
            handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none()
        );
        let list = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 6);
        let unknown = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" })).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn tools_work() {
        let bad = call(
            "darm_check_source",
            &json!({ "source": "darm 1\nchapter \"A\":\n    text \"x\"\n" }),
        )
        .unwrap();
        assert!(bad.contains("E0313"));
        let formatted = call(
            "darm_format",
            &json!({ "source": "darm 1\nbook \"T\":\n  author=\"A\"\n" }),
        )
        .unwrap();
        assert_eq!(formatted, "darm 1\n\nbook \"T\":\n    author = \"A\"\n");
        assert!(
            call(
                "darm_format",
                &json!({ "source": "darm 1\ntext \"a\" \"b\"\n" })
            )
            .is_err()
        );
        assert!(
            call("darm_explain", &json!({ "code": "E0304" }))
                .unwrap()
                .contains("unknown asset")
        );
        assert!(
            call("darm_reference", &json!({}))
                .unwrap()
                .contains("\"elements\"")
        );
        assert!(call("nope", &json!({})).is_err());
    }
}
