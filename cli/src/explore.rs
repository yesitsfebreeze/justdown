//! `jd explore` — the built-in .jd explorer/editor, served from the binary itself.
//!
//! A Rust port of the former Node/Express `justdown-editor`: the CodeMirror 6
//! frontend is embedded into the executable, the file/search API is served by
//! `tiny_http`, and `*.jd` files are discovered natively (no rg/fzf/node).
//!
//! ## One server, many feeders
//!
//! Every Claude instance runs its own `jd` process scoped to its own files. The
//! editor is a single shared website fed by all of them:
//!
//! - The listen port IS the cross-process mutex. The first process to bind
//!   127.0.0.1:PORT *hosts* the website; the rest become *feeders*.
//! - Every process (host included) registers its roots with the host over HTTP
//!   and re-registers on a heartbeat. The host walks the union of all live
//!   roots, so search spans every running `jd` at once.
//! - A feeder that stops heartbeating ages out of the index. If the *host*
//!   dies, the port frees; the next feeder's bind attempt succeeds and it takes
//!   over hosting — the others simply re-register with the new host. The
//!   website survives as long as one `jd` process is alive.

use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

const INDEX_HTML: &str = include_str!("../editor/index.html");
const APP_JS: &str = include_str!("../editor/app.js");
const STYLE_CSS: &str = include_str!("../editor/style.css");

/// Source-tree asset dir, baked in at build time. `--dev` serves the editor
/// files from here (live) instead of the embedded copies, so a save-and-refresh
/// shows up without a rebuild — and the page auto-reloads via `/api/livereload`.
const EDITOR_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/editor");

/// How often a process re-registers its roots with the host.
const HEARTBEAT: Duration = Duration::from_secs(5);
/// A feeder is dropped from the index after this long without a heartbeat.
const FEEDER_TTL: Duration = Duration::from_secs(20);
/// Force a full re-walk at least this often (picks up new files on disk).
const REWALK_EVERY: Duration = Duration::from_secs(30);

/// One running `jd` process's contribution to the search: the roots it offers
/// and when it was last heard from.
struct Feeder {
    roots: Vec<PathBuf>,
    last_seen: Instant,
}

/// Host-side shared state: the live feeder registry plus the cached union index.
struct State {
    feeders: Mutex<HashMap<String, Feeder>>,
    index: Mutex<Vec<PathBuf>>,
    /// Bumped whenever the feeder set changes, so the indexer re-walks promptly.
    gen: AtomicU64,
    /// `--dev`: serve editor assets from disk + inject the live-reload watcher.
    dev: bool,
}

impl State {
    fn new(dev: bool) -> Self {
        State {
            feeders: Mutex::new(HashMap::new()),
            index: Mutex::new(Vec::new()),
            gen: AtomicU64::new(0),
            dev,
        }
    }

    /// Insert/refresh a feeder; bump `gen` only when the root set is new.
    fn register(&self, id: &str, roots: Vec<PathBuf>) {
        let mut feeders = self.feeders.lock().unwrap();
        let changed = feeders.get(id).map(|f| f.roots != roots).unwrap_or(true);
        feeders.insert(
            id.to_string(),
            Feeder {
                roots,
                last_seen: Instant::now(),
            },
        );
        if changed {
            self.gen.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The deduped union of every live feeder's roots.
    fn live_roots(&self) -> Vec<PathBuf> {
        let mut feeders = self.feeders.lock().unwrap();
        let before = feeders.len();
        feeders.retain(|_, f| f.last_seen.elapsed() < FEEDER_TTL);
        if feeders.len() != before {
            self.gen.fetch_add(1, Ordering::Relaxed);
        }
        let mut seen = HashSet::new();
        let mut roots = Vec::new();
        for f in feeders.values() {
            for r in &f.roots {
                if seen.insert(r.clone()) {
                    roots.push(r.clone());
                }
            }
        }
        roots
    }
}

pub fn run(args: &[String]) -> i32 {
    let port: u16 = args
        .iter()
        .find_map(|a| a.strip_prefix("--port=").and_then(|p| p.parse().ok()))
        .or_else(|| std::env::var("JD_PORT").ok().and_then(|p| p.parse().ok()))
        .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(3001);

    let root = std::env::var("JD_ROOT")
        .map(PathBuf::from)
        .ok()
        .or_else(home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let roots = vec![root.clone()];
    let id = format!("pid-{}", std::process::id());
    let url = format!("http://localhost:{port}");
    let dev = args.iter().any(|a| a == "--dev");

    let mut announced = false;
    loop {
        match TcpListener::bind(("127.0.0.1", port)) {
            // We won the port — host the website until we die or it errors.
            Ok(listener) => {
                let server = match tiny_http::Server::from_listener(listener, None) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("jd: server start failed: {e}");
                        return 4;
                    }
                };
                let state = Arc::new(State::new(dev));
                state.register(&id, roots.clone());
                spawn_indexer(state.clone());
                spawn_self_heartbeat(state.clone(), id.clone(), roots.clone());
                if !announced {
                    println!("✺ jd explorer → {url}");
                    println!("✺ hosting; searching every running jd (this one: {})", root.display());
                    if dev {
                        println!("✺ dev: serving editor from {EDITOR_DIR} — live reload on");
                    }
                    open_url(&url);
                    announced = true;
                }
                serve(server, &state); // blocks; only returns if the server stops
            }
            // Someone else hosts — feed them and heartbeat. If the host dies the
            // next bind attempt wins and we take over.
            Err(e) if e.kind() == ErrorKind::AddrInUse => {
                let fed = post_feed(port, &id, &roots);
                if fed && !announced {
                    println!("✺ jd explorer already running → {url}");
                    println!("✺ feeding: {}", root.display());
                    open_url(&url);
                    announced = true;
                }
                std::thread::sleep(HEARTBEAT);
            }
            Err(e) => {
                eprintln!("jd: cannot bind 127.0.0.1:{port}: {e}");
                return 4;
            }
        }
    }
}

/// Re-walk the union of live roots whenever the feeder set changes or the
/// periodic refresh is due, so search always serves a current snapshot.
fn spawn_indexer(state: Arc<State>) {
    std::thread::spawn(move || {
        let mut last_gen = u64::MAX;
        let mut last_walk = Instant::now()
            .checked_sub(REWALK_EVERY)
            .unwrap_or_else(Instant::now);
        loop {
            let roots = state.live_roots(); // also expires dead feeders (may bump gen)
            let gen = state.gen.load(Ordering::Relaxed);
            if gen != last_gen || last_walk.elapsed() >= REWALK_EVERY {
                *state.index.lock().unwrap() = walk_jd(&roots);
                last_gen = gen;
                last_walk = Instant::now();
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// The host keeps its own feeder entry fresh directly (no self HTTP needed).
fn spawn_self_heartbeat(state: Arc<State>, id: String, roots: Vec<PathBuf>) {
    std::thread::spawn(move || loop {
        state.register(&id, roots.clone());
        std::thread::sleep(HEARTBEAT);
    });
}

fn serve(server: tiny_http::Server, state: &Arc<State>) {
    for request in server.incoming_requests() {
        let state = state.clone();
        std::thread::spawn(move || handle(request, &state));
    }
}

fn handle(request: tiny_http::Request, state: &State) {
    use tiny_http::Method;
    let (path, query) = split_url(request.url());
    let method = request.method().clone();

    match (&method, path.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => respond(
            request,
            200,
            "text/html; charset=utf-8",
            index_html(state.dev).as_bytes(),
        ),
        (Method::Get, "/app.js") => respond(
            request,
            200,
            "text/javascript; charset=utf-8",
            asset(state.dev, "app.js", APP_JS).as_bytes(),
        ),
        (Method::Get, "/style.css") => respond(
            request,
            200,
            "text/css; charset=utf-8",
            asset(state.dev, "style.css", STYLE_CSS).as_bytes(),
        ),
        (Method::Get, "/api/livereload") => api_livereload(request),
        (Method::Post, "/api/feed") => api_feed(request, state),
        (Method::Get, "/api/search") => api_search(request, state, &query),
        (Method::Get, "/api/rg") => api_rg(request, state, &query),
        (Method::Get, "/api/file") => api_load(request, state, &query),
        (Method::Post, "/api/file") => api_save(request, state, &query),
        (Method::Post, "/api/reveal") => api_reveal(request, state, &query),
        (Method::Post, "/api/delete") => api_delete(request, state, &query),
        _ => respond(request, 404, "text/plain", b"not found"),
    }
}

/* ------------------------------ dev assets ----------------------------- */

/// In `--dev`, read an asset fresh from the source tree so a save shows up on
/// the next request; otherwise return the copy embedded at build time. A
/// missing dev file falls back to the embedded copy rather than 404ing.
fn asset(dev: bool, name: &str, embedded: &str) -> String {
    if dev {
        std::fs::read_to_string(Path::new(EDITOR_DIR).join(name))
            .unwrap_or_else(|_| embedded.to_string())
    } else {
        embedded.to_string()
    }
}

/// The page HTML, with a tiny poll-and-reload watcher spliced in under `--dev`.
/// It polls `/api/livereload`; when the newest asset mtime changes, it reloads.
fn index_html(dev: bool) -> String {
    let html = asset(dev, "index.html", INDEX_HTML);
    if !dev {
        return html;
    }
    const WATCH: &str = "<script>(async()=>{let last=null;for(;;){try{const j=await(await fetch('/api/livereload')).json();if(last!==null&&j.mtime!==last)location.reload();last=j.mtime;}catch(e){}await new Promise(r=>setTimeout(r,600));}})();</script>";
    match html.rfind("</body>") {
        Some(i) => format!("{}{WATCH}{}", &html[..i], &html[i..]),
        None => format!("{html}{WATCH}"),
    }
}

/// The newest mtime across the on-disk editor assets — the dev reload signal.
/// Always served (harmless in prod; only the dev page polls it).
fn api_livereload(request: tiny_http::Request) {
    let mtime = ["index.html", "app.js", "style.css"]
        .iter()
        .map(|f| mtime_ms(&Path::new(EDITOR_DIR).join(f)))
        .max()
        .unwrap_or(0);
    respond_json(request, 200, &json!({ "mtime": mtime as f64 }));
}

/* ------------------------------ registry ------------------------------- */

fn api_feed(mut request: tiny_http::Request, state: &State) {
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        return respond_json(request, 400, &json!({ "error": "bad body" }));
    }
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(json!({}));
    let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("");
    let roots: Vec<PathBuf> = v
        .get("roots")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|p| p.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    if !id.is_empty() {
        state.register(id, roots);
    }
    respond_json(request, 200, &json!({ "ok": true }));
}

/* ------------------------------- search -------------------------------- */

fn api_search(request: tiny_http::Request, state: &State, query: &str) {
    let q = param(query, "q").unwrap_or_default();
    let q = q.trim();
    let all = state.index.lock().unwrap().clone();

    let (files, snippets): (Vec<PathBuf>, HashMap<PathBuf, String>) = if q.is_empty() {
        (all, HashMap::new())
    } else {
        // Each whitespace term must match somewhere — either the path tail
        // (fuzzy subsequence) or the file's content. So "vim rg" hits rg.jd
        // when `rg` matches the name and `vim` matches the body.
        let terms: Vec<String> = q.to_lowercase().split_whitespace().map(String::from).collect();
        let mut files = Vec::new();
        let mut snippets = HashMap::new();
        for f in &all {
            let label: Vec<char> = display_path(f).to_lowercase().chars().collect();
            let raw = std::fs::read_to_string(f).unwrap_or_default();
            let content = raw.to_lowercase();
            let mut snippet: Option<String> = None;
            let matched = terms.iter().all(|t| {
                if subsequence(&label, t) {
                    return true;
                }
                if content.contains(t.as_str()) {
                    if snippet.is_none() {
                        snippet = raw
                            .lines()
                            .find(|l| l.to_lowercase().contains(t.as_str()))
                            .map(|l| l.trim().chars().take(120).collect());
                    }
                    return true;
                }
                false
            });
            if matched {
                files.push(f.clone());
                if let Some(s) = snippet {
                    snippets.insert(f.clone(), s);
                }
            }
        }
        (files, snippets)
    };

    let total = files.len();
    let mut scored: Vec<(PathBuf, u128)> =
        files.into_iter().map(|f| (f.clone(), mtime_ms(&f))).collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1)); // latest touched first
    scored.truncate(50);

    let results: Vec<_> = scored
        .iter()
        .map(|(f, m)| {
            let display = display_path(f);
            let dir = Path::new(&display)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            json!({
                "path": f.to_string_lossy(),
                "name": f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                "dir": dir,
                "snippet": snippets.get(f),
                "mtime": *m as f64,
            })
        })
        .collect();

    let body = json!({ "results": results, "total": total, "root": "" });
    respond_json(request, 200, &body);
}

/// Subsequence test: are `needle`'s chars present, in order, within `hay`?
/// `needle` is already lowercase; `hay` is a lowercased char slice.
fn subsequence(hay: &[char], needle: &str) -> bool {
    let mut chars = needle.chars();
    let mut want = chars.next();
    for &c in hay {
        if Some(c) == want {
            want = chars.next();
            if want.is_none() {
                return true;
            }
        }
    }
    want.is_none()
}

/* ------------------------------ ripgrep -------------------------------- */

/// Case-sensitive content search across every live root, powered by `rg`.
/// Returns one hit per matching line (file, line number, the line text) so the
/// editor can render a navigable dropdown and jump to the match. `rg` is the
/// canonical tool here — literal (`-F`), case-sensitive (rg's default), `.jd`
/// only. If `rg` isn't installed we say so rather than silently returning empty.
fn api_rg(request: tiny_http::Request, state: &State, query: &str) {
    let q = param(query, "q").unwrap_or_default();
    let q = q.trim().to_string();
    if q.is_empty() {
        return respond_json(request, 200, &json!({ "results": [], "total": 0 }));
    }
    let roots = state.live_roots();
    let mut cmd = Command::new("rg");
    cmd.arg("--json")
        .arg("-F") // literal string, not a regex
        .arg("--max-count")
        .arg("50") // cap hits per file
        .arg("-g")
        .arg("*.jd")
        .arg("--")
        .arg(&q);
    for r in &roots {
        cmd.arg(r);
    }
    let stdout = match cmd.stderr(Stdio::null()).output() {
        Ok(o) => o.stdout,
        Err(_) => {
            return respond_json(
                request,
                200,
                &json!({ "results": [], "total": 0, "error": "ripgrep (rg) not found on PATH" }),
            );
        }
    };

    let text = String::from_utf8_lossy(&stdout);
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("match") {
            continue;
        }
        let data = &v["data"];
        let Some(path) = data["path"]["text"].as_str() else {
            continue;
        };
        let lineno = data["line_number"].as_u64().unwrap_or(0);
        if !seen.insert((path.to_string(), lineno)) {
            continue; // one row per (file, line)
        }
        let snippet: String = data["lines"]["text"]
            .as_str()
            .unwrap_or("")
            .trim_end_matches(['\n', '\r'])
            .trim()
            .chars()
            .take(160)
            .collect();
        let col = data["submatches"]
            .get(0)
            .and_then(|s| s["start"].as_u64())
            .unwrap_or(0);
        let pb = PathBuf::from(path);
        let display = display_path(&pb);
        let dir = Path::new(&display)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        results.push(json!({
            "path": path,
            "name": pb.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "dir": dir,
            "line": lineno,
            "col": col,
            "text": snippet,
        }));
        if results.len() >= 200 {
            break;
        }
    }

    let total = results.len();
    respond_json(request, 200, &json!({ "results": results, "total": total }));
}

/* -------------------------------- files -------------------------------- */

fn api_load(request: tiny_http::Request, state: &State, query: &str) {
    let rel = param(query, "path").unwrap_or_default();
    let Some(p) = safe_path(state, &rel) else {
        return respond_json(request, 404, &json!({ "error": "File not found" }));
    };
    match std::fs::read_to_string(&p) {
        Ok(content) => respond_json(
            request,
            200,
            &json!({ "content": content, "path": p.to_string_lossy() }),
        ),
        Err(_) => respond_json(request, 404, &json!({ "error": "File not found" })),
    }
}

fn api_save(mut request: tiny_http::Request, state: &State, query: &str) {
    let rel = param(query, "path").unwrap_or_default();
    let Some(p) = safe_path(state, &rel) else {
        return respond_json(request, 500, &json!({ "error": "Access denied" }));
    };
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        return respond_json(request, 500, &json!({ "error": "bad body" }));
    }
    let content = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("content").and_then(|c| c.as_str()).map(String::from))
        .unwrap_or_default();

    let is_new = !p.exists();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&p, content) {
        Ok(_) => respond_json(
            request,
            200,
            &json!({ "success": true, "isNew": is_new, "path": p.to_string_lossy() }),
        ),
        Err(e) => respond_json(request, 500, &json!({ "error": e.to_string() })),
    }
}

fn api_reveal(request: tiny_http::Request, state: &State, query: &str) {
    let rel = param(query, "path").unwrap_or_default();
    if let Some(p) = safe_path(state, &rel) {
        reveal_in_file_manager(&p);
        respond_json(request, 200, &json!({ "ok": true }))
    } else {
        respond_json(request, 400, &json!({ "error": "Access denied" }))
    }
}

fn api_delete(request: tiny_http::Request, state: &State, query: &str) {
    let rel = param(query, "path").unwrap_or_default();
    let Some(p) = safe_path(state, &rel) else {
        return respond_json(request, 400, &json!({ "error": "Access denied" }));
    };
    match std::fs::remove_file(&p) {
        Ok(_) => respond_json(request, 200, &json!({ "ok": true })),
        Err(e) => respond_json(request, 400, &json!({ "error": e.to_string() })),
    }
}

/* ------------------------------- helpers ------------------------------- */

/// Walk every root for `*.jd`, deduped, skipping heavy/irrelevant trees.
fn walk_jd(roots: &[PathBuf]) -> Vec<PathBuf> {
    const SKIP: &[&str] = &["node_modules", ".git", "target", ".Trash", ".cache", "Caches"];
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                let name = entry.file_name();
                if SKIP.iter().any(|s| name == *s) {
                    continue;
                }
                stack.push(path);
            } else if ft.is_file() && path.extension().map(|e| e == "jd").unwrap_or(false) {
                if seen.insert(path.clone()) {
                    out.push(path);
                }
            }
        }
    }
    out
}

/// Resolve a client path (absolute, or relative to a root) and require it to
/// live under one of the currently-fed roots.
fn safe_path(state: &State, p: &str) -> Option<PathBuf> {
    let roots = state.live_roots();
    let cand = PathBuf::from(p);
    let joined = if cand.is_absolute() {
        cand
    } else {
        roots.first()?.join(cand)
    };
    let mut clean = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }
    roots
        .iter()
        .any(|r| clean == *r || clean.starts_with(r))
        .then_some(clean)
}

/// Display form of a path: home collapsed to `~` for readable result rows.
fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if let Some(home) = home_dir() {
        let home = home.to_string_lossy().replace('\\', "/");
        if let Some(rest) = s.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    s
}

fn mtime_ms(p: &Path) -> u128 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// Open `url` in the default browser, trying each platform's launcher.
fn open_url(url: &str) {
    for (cmd, args) in [
        ("xdg-open", vec![url]),
        ("wslview", vec![url]),
        ("open", vec![url]),
        ("cmd.exe", vec!["/c", "start", url]),
    ] {
        if Command::new(cmd)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}

/// Reveal a file in the OS file manager (best-effort, cross-platform).
fn reveal_in_file_manager(p: &Path) {
    let path = p.to_string_lossy().into_owned();
    let dir = p
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let attempts: [(&str, Vec<String>); 3] = [
        ("open", vec!["-R".into(), path.clone()]),
        ("explorer.exe", vec![format!("/select,{path}")]),
        ("xdg-open", vec![dir]),
    ];
    for (cmd, args) in attempts {
        if Command::new(cmd)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}

/* ------------------------------ feed client ---------------------------- */

/// Register this process's roots with the host over a one-shot HTTP POST.
/// Returns false if the host is unreachable (its port is free → we take over).
fn post_feed(port: u16, id: &str, roots: &[PathBuf]) -> bool {
    let roots: Vec<String> = roots.iter().map(|r| r.to_string_lossy().into_owned()).collect();
    let body = json!({ "id": id, "roots": roots }).to_string();
    let req = format!(
        "POST /api/feed HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    match stream.read(&mut buf) {
        Ok(n) => std::str::from_utf8(&buf[..n])
            .map(|s| s.contains(" 200"))
            .unwrap_or(false),
        Err(_) => false,
    }
}

/* ------------------------------- http I/O ------------------------------ */

fn split_url(url: &str) -> (String, String) {
    match url.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (url.to_string(), String::new()),
    }
}

/// Pull one `key` out of a `&`-joined query string, percent-decoded.
fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(request: tiny_http::Request, status: u16, content_type: &str, body: &[u8]) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("valid header");
    let response = tiny_http::Response::from_data(body)
        .with_status_code(status)
        .with_header(header);
    let _ = request.respond(response);
}

fn respond_json(request: tiny_http::Request, status: u16, body: &serde_json::Value) {
    respond(
        request,
        status,
        "application/json; charset=utf-8",
        body.to_string().as_bytes(),
    );
}
