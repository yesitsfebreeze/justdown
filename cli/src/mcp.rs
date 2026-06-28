//! `jd mcp` — a stdio MCP server that wraps jd's query verbs as tools.
//!
//! The README's thesis: one MCP server doing *library lookup*, not an MCP
//! server per capability. This is that server. Each tool re-invokes the `jd`
//! binary itself with `--json`, so the MCP surface is a perfect mirror of the
//! CLI — same three-tier merge, same scoring, same versioned output schemas —
//! with no logic duplicated here. The `.jd` library stays the single contract.
//!
//! Transport is newline-delimited JSON-RPC 2.0 over stdio (the MCP stdio
//! convention): one JSON object per line in, one per line out. Requests carry
//! an `id` and get a reply; notifications have none and are processed silently.

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::Command;

/// MCP protocol revision we default to when a client doesn't pin one.
const PROTOCOL: &str = "2024-11-05";

pub fn run(_args: &[String]) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue; // not JSON — ignore rather than crash the stream
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        match method {
            "initialize" => respond(&mut out, id, Ok(initialize(&params))),
            "tools/list" => respond(&mut out, id, Ok(json!({ "tools": tools() }))),
            "tools/call" => respond(&mut out, id, Ok(tool_call(&params))),
            "ping" => respond(&mut out, id, Ok(json!({}))),
            // notifications (no id) — nothing to reply
            m if m.starts_with("notifications/") => {}
            other => {
                if id.is_some() {
                    respond(
                        &mut out,
                        id,
                        Err((-32601, format!("method not found: {other}"))),
                    );
                }
            }
        }
    }
    0
}

/// Write one JSON-RPC reply line, or nothing for a notification (no `id`).
fn respond(out: &mut impl Write, id: Option<Value>, result: Result<Value, (i64, String)>) {
    let Some(id) = id else { return };
    let body = match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, msg)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
        }
    };
    let _ = writeln!(out, "{body}");
    let _ = out.flush();
}

fn initialize(params: &Value) -> Value {
    let pv = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL);
    json!({
        "protocolVersion": pv,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "justdown", "version": crate::CLI_VERSION },
    })
}

/// The tool catalogue — jd's read verbs, each with a JSON-Schema for its args.
fn tools() -> Value {
    json!([
        {
            "name": "search",
            "description": "Rank library .jd files by need (graph-aware: name/use_when > tags > prose; not_when vetoes). Returns the best matches with purpose, kind, source tier and safety.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "what you need to do, in plain words" },
                    "kind": { "type": "string", "enum": ["tool", "agent", "knowledge", "workflow"], "description": "narrow to one kind" },
                    "limit": { "type": "integer", "minimum": 1, "description": "max results (default 5)" },
                    "category": { "type": "string", "description": "narrow to one category" },
                    "mode": { "type": "string", "enum": ["exact", "semantic"], "description": "exact substring rank (default) or synonym/stem-widened semantic rank" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "get",
            "description": "Read one .jd file as ordered sections, or a single output profile. ref = name | key | path | @dir/name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ref": { "type": "string", "description": "name, key (dir/name), path, or @dir/name" },
                    "profile": { "type": "string", "enum": ["default", "frontmatter", "human", "agent", "justfile"], "description": "default = all sections; justfile needs kind tool|workflow" },
                    "vars": { "type": "object", "description": "host values for <<var>> injection, name->value" }
                },
                "required": ["ref"]
            }
        },
        {
            "name": "ls",
            "description": "List every category and its member files.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "links",
            "description": "Inbound + outbound @links of a file (one hop of the graph).",
            "inputSchema": {
                "type": "object",
                "properties": { "ref": { "type": "string", "description": "name, key, path, or @dir/name" } },
                "required": ["ref"]
            }
        },
        {
            "name": "resolve",
            "description": "Live @link completion. Direct: ranked key/name/leaf prefix matches for a @name link (reports the unique canonical key when one resolves). Fuzzy: the field-weighted ranker for a @?term link (one-to-many).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "the term after @ (or @?)" },
                    "fuzzy": { "type": "boolean", "description": "true for @?term ranking; false (default) for @name prefix" },
                    "limit": { "type": "integer", "minimum": 1, "description": "max matches (default 10)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "path",
            "description": "Shortest @link connection between two files (undirected BFS).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string" },
                    "to": { "type": "string" }
                },
                "required": ["from", "to"]
            }
        }
    ])
}

/// Run the requested tool by shelling back into `jd … --json`, wrapping stdout
/// as MCP text content. A non-zero exit becomes an `isError` result carrying
/// stderr (jd's `justdown.error/1` envelope), so the model sees the reason.
fn tool_call(params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let a = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    let str_arg = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();

    let argv: Vec<String> = match name {
        "search" => {
            let limit = a
                .get("limit")
                .and_then(Value::as_i64)
                .filter(|n| *n > 0)
                .unwrap_or(5)
                .to_string();
            let mode = a.get("mode").and_then(Value::as_str).unwrap_or("exact");
            // positional slots: query, kind, num, category — empty strings hold
            // a slot open so `limit` can be passed without naming a kind.
            vec![
                "search".into(),
                str_arg("query"),
                str_arg("kind"),
                limit,
                str_arg("category"),
                "--mode".into(),
                mode.into(),
                "--json".into(),
            ]
        }
        "get" => {
            let mut v = vec!["get".into(), str_arg("ref")];
            match a.get("profile").and_then(Value::as_str) {
                Some(p) if !p.is_empty() && p != "default" => v.push(format!("--{p}")),
                _ => {}
            }
            if let Some(vars) = a.get("vars").and_then(Value::as_object) {
                for (k, val) in vars {
                    let val = val.as_str().map(str::to_string).unwrap_or_else(|| val.to_string());
                    v.push("--var".into());
                    v.push(format!("{k}={val}"));
                }
            }
            v.push("--json".into());
            v
        }
        "ls" => vec!["ls".into(), "--json".into()],
        "links" => vec!["links".into(), str_arg("ref"), "--json".into()],
        "resolve" => {
            let limit = a
                .get("limit")
                .and_then(Value::as_i64)
                .filter(|n| *n > 0)
                .unwrap_or(10)
                .to_string();
            let mut v = vec!["resolve".into(), str_arg("query"), limit];
            if a.get("fuzzy").and_then(Value::as_bool).unwrap_or(false) {
                v.push("--fuzzy".into());
            }
            v.push("--json".into());
            v
        }
        "path" => vec![
            "path".into(),
            str_arg("from"),
            str_arg("to"),
            "--json".into(),
        ],
        other => {
            return json!({
                "content": [{ "type": "text", "text": format!("unknown tool: {other}") }],
                "isError": true,
            });
        }
    };

    let (code, stdout, stderr) = call_jd(&argv);
    if code == 0 {
        json!({ "content": [{ "type": "text", "text": stdout }] })
    } else {
        let text = if stderr.trim().is_empty() { stdout } else { stderr };
        json!({ "content": [{ "type": "text", "text": text }], "isError": true })
    }
}

/// Invoke this very `jd` executable with `args`, capturing (exit, stdout,
/// stderr). Using `current_exe` keeps the tool surface and the CLI the same
/// binary — dogfooding in the literal sense.
fn call_jd(args: &[String]) -> (i32, String, String) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("jd"));
    match Command::new(exe).args(args).output() {
        Ok(o) => (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}
