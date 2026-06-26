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
}

impl State {
    fn new() -> Self {
        State {
            feeders: Mutex::new(HashMap::new()),
            index: Mutex::new(Vec::new()),
            gen: AtomicU64::new(0),
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
                let state = Arc::new(State::new());
                state.register(&id, roots.clone());
                spawn_indexer(state.clone());
                spawn_self_heartbeat(state.clone(), id.clone(), roots.clone());
                if !announced {
                    println!("✺ jd explorer → {url}");
                    println!("✺ hosting; searching every running jd (this one: {})", root.display());
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
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            respond(request, 200, "text/html; charset=utf-8", INDEX_HTML.as_bytes())
        }
        (Method::Get, "/app.js") => respond(
            request,
            200,
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
        ),
        (Method::Get, "/style.css") => {
            respond(request, 200, "text/css; charset=utf-8", STYLE_CSS.as_bytes())
        }
        (Method::Post, "/api/feed") => api_feed(request, state),
        (Method::Get, "/api/search") => api_search(request, state, &query),
        (Method::Get, "/api/file") => api_load(request, state, &query),
        (Method::Post, "/api/file") => api_save(request, state, &query),
        (Method::Post, "/api/reveal") => api_reveal(request, state, &query),
        (Method::Post, "/api/delete") => api_delete(request, state, &query),
        _ => respond(request, 404, "text/plain", b"not found"),
    }
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
        // Filename fuzzy match on the path tail (not the absolute prefix, which
        // would fuzzy-match everything), unioned with a content match.
        let labels: Vec<(String, &PathBuf)> = all.iter().map(|f| (display_path(f), f)).collect();
        let ranked = fuzzy_rank(labels.iter().map(|(l, _)| l.as_str()), q);
        let by_label: HashMap<&str, &PathBuf> =
            labels.iter().map(|(l, f)| (l.as_str(), *f)).collect();
        let name_matches: Vec<PathBuf> = ranked
            .iter()
            .filter_map(|l| by_label.get(l.as_str()).map(|p| (*p).clone()))
            .collect();

        let content = content_matches(&all, q);
        let seen: HashSet<&PathBuf> = name_matches.iter().collect();
        let content_only: Vec<PathBuf> = content
            .keys()
            .filter(|f| !seen.contains(*f))
            .cloned()
            .collect();

        let mut files = name_matches;
        files.extend(content_only);
        (files, content)
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

/// Subsequence fuzzy match ranked by how tightly the query packs (smaller gap
/// sum = tighter). Mirrors the editor's `jsFuzzy`.
fn fuzzy_rank<'a>(files: impl Iterator<Item = &'a str>, q: &str) -> Vec<String> {
    let needle: Vec<char> = q.to_lowercase().chars().collect();
    let mut scored: Vec<(String, i64)> = Vec::new();
    for f in files {
        let hay: Vec<char> = f.to_lowercase().chars().collect();
        let (mut i, mut score, mut last) = (0usize, 0i64, -1i64);
        for (j, c) in hay.iter().enumerate() {
            if i < needle.len() && *c == needle[i] {
                if last >= 0 {
                    score += j as i64 - last;
                }
                last = j as i64;
                i += 1;
            }
        }
        if i == needle.len() {
            scored.push((f.to_string(), score));
        }
    }
    scored.sort_by(|a, b| a.1.cmp(&b.1));
    scored.into_iter().map(|(f, _)| f).collect()
}

/// First line of each file containing `q` (case-insensitive), trimmed to a
/// 120-char snippet. Lets you find a `.jd` by what it *says*.
fn content_matches(files: &[PathBuf], q: &str) -> HashMap<PathBuf, String> {
    let needle = q.to_lowercase();
    let mut map = HashMap::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        for line in text.lines() {
            if line.to_lowercase().contains(&needle) {
                let snip: String = line.trim().chars().take(120).collect();
                map.insert(f.clone(), snip);
                break;
            }
        }
    }
    map
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
