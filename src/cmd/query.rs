// The query surface: search / get / ls / links. A faithful port of the
// original justfile awk — same field-weighted scoring, not_when veto, kind &
// category narrowing, degrade-not-fail, text + JSON output, exit codes (0/2/3/4).
// Merge is two graphs: the repo-LOCAL `.jd` files, parsed LIVE on every query
// (they change often), shadow a CACHED belt of prebuilt remote graphs that
// `jd refresh` downloads (slow-changing, queried offline).

use super::config::{Config, Format};
use justdown::store::{rows_from_nodes, Row, Source, Store};
use justdown::{graph, jd, links};
use justdown::render::{self, Vars};
use justdown::search::{degree_map, rank, words, Scored, STOPWORDS};
use serde::Serialize;

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/// Serialize a `--json` envelope to a single line. Our output types are plain
/// structs of strings/ints/string-vecs — `serde_json` cannot fail on them — so
/// an error here is a bug, not bad input; surface it loudly rather than emit a
/// half-formed envelope.
fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("jd json output is always serializable")
}

/// Split a comma-joined store field (`Row::side_effects`, `Row::requires`) into
/// the vec the JSON arrays carry. Empty string → empty vec (matches the old
/// `json_arr` which emitted `[]` for an empty field).
fn csv_vec(csv: &str) -> Vec<String> {
    if csv.is_empty() {
        Vec::new()
    } else {
        csv.split(',').map(str::to_string).collect()
    }
}

#[derive(Serialize)]
struct ErrorOut<'a> {
    schema: &'a str,
    error: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct SearchOut<'a> {
    schema: &'a str,
    query: &'a str,
    results: Vec<SearchResult<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback: Option<Fallback<'a>>,
}

#[derive(Serialize)]
struct SearchResult<'a> {
    name: &'a str,
    kind: &'a str,
    score: i64,
    purpose: &'a str,
    raw: String,
    source: &'a str,
    danger: &'a str,
    side_effects: Vec<String>,
    requires: Vec<String>,
}

#[derive(Serialize)]
struct Fallback<'a> {
    reason: &'a str,
    name: &'a str,
    kind: &'a str,
    purpose: &'a str,
    raw: String,
}

#[derive(Serialize)]
struct GetOut<'a> {
    schema: &'a str,
    #[serde(rename = "ref")]
    refr: &'a str,
    sections: Vec<Section<'a>>,
}

#[derive(Serialize)]
struct Section<'a> {
    kind: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct LsOut<'a> {
    schema: &'a str,
    categories: Vec<LsCategory<'a>>,
}

#[derive(Serialize)]
struct LsCategory<'a> {
    name: &'a str,
    members: &'a [String],
}

#[derive(Serialize)]
struct LinksOut<'a> {
    schema: &'a str,
    #[serde(rename = "ref")]
    refr: &'a str,
    key: &'a str,
    outbound: Vec<String>,
    inbound: Vec<String>,
    /// Fuzzy (`@?term`) link terms — ranked live, not fixed graph edges.
    fuzzy: Vec<String>,
}

#[derive(Serialize)]
struct ResolveOut<'a> {
    schema: &'a str,
    query: &'a str,
    fuzzy: bool,
    /// The canonical key a direct `@query` resolves to uniquely, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved: Option<String>,
    matches: Vec<ResolveMatch<'a>>,
}

/// A resolve hit, in the one shape both the CLI and the editor's `/api/resolve`
/// emit: the editor consumes only `key`/`kind`/`path`.
#[derive(Serialize)]
struct ResolveMatch<'a> {
    key: &'a str,
    kind: &'a str,
    path: &'a str,
}

#[derive(Serialize)]
struct PathOut<'a> {
    schema: &'a str,
    from: &'a str,
    to: &'a str,
    path: Vec<String>,
    length: i64,
}

fn emit_err(cfg: &Config, code: &str, msg: &str) {
    match cfg.format {
        Format::Json => eprintln!(
            "{}",
            to_json(&ErrorOut {
                schema: "justdown.error/1",
                error: code,
                message: msg,
            })
        ),
        Format::Text => eprintln!("jd: {msg}"),
    }
}

// ---------------------------------------------------------------------------
// loading + merge
// ---------------------------------------------------------------------------

/// Network deadline for an online-merge fetch: connect in ≤5s, finish in ≤15s.
/// The online tier is best-effort — queries degrade to local/global when it's
/// unreachable — so no fetch may block the tool indefinitely. Without this, a
/// slow or black-holed host hangs every `jd search`/`get`/`ls`/`links`/`path`.
const NET_MAX_TIME: &str = "15";
const NET_CONNECT_TIMEOUT: &str = "5";

/// Fetch a URL to a file with curl. Best-effort: returns false on any failure
/// (curl absent, unreachable, 404, or a network timeout). justdown already
/// requires curl on PATH; the online merge degrades to local-only when it
/// isn't there or can't answer in time.
pub(crate) fn curl_to_file(url: &str, dest: &std::path::Path) -> bool {
    std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            NET_CONNECT_TIMEOUT,
            "--max-time",
            NET_MAX_TIME,
            url,
            "-o",
        ])
        .arg(dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Fetch a URL to a string with curl. None on any failure (incl. timeout).
fn curl_to_string(url: &str) -> Option<String> {
    let out = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            NET_CONNECT_TIMEOUT,
            "--max-time",
            NET_MAX_TIME,
            url,
        ])
        .output()
        .ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

/// Load the cached belt: every remote's prebuilt `graph.db` as downloaded into
/// the local cache by `jd refresh`. Read offline — no network here. Each row is
/// tagged with its remote's raw base so `get` fetches that file's body from the
/// right repo. Remotes that are non-GitHub or not yet cached are silently
/// skipped (run `jd refresh`).
fn cached_belt_rows(cfg: &Config) -> Vec<Row> {
    // Walk the belt last→first so that, with `gather`'s keep-first dedup, a later
    // belt entry shadows an earlier one — matching `build_roots`' later-root-wins
    // rule, so cached and built-graph precedence agree ("later entries win").
    let mut out = Vec::new();
    for r in cfg.remotes().iter().rev() {
        let Some(raw) = r.raw_base() else { continue };
        let Some(cache) = Config::belt_cache_path(&r.slug) else {
            continue;
        };
        if let Some(mut rows) = load_store(&cache, Source::Online) {
            for row in &mut rows {
                row.origin = raw.clone();
            }
            out.extend(rows);
        }
    }
    out
}

/// The raw base a given online row's files hang off — its remote's, or the
/// configured default when untagged (single-repo / legacy).
fn online_base<'a>(cfg: &'a Config, r: &'a Row) -> &'a str {
    if r.origin.is_empty() {
        &cfg.raw_base
    } else {
        &r.origin
    }
}

/// The display path for a row's source file: the remote URL for online rows, the
/// nested-home-qualified path for a nested local row (so it points at the real
/// file, not an ambiguous root-relative path), or the bare relative path for the
/// root/global home.
fn raw_display(cfg: &Config, r: &Row) -> String {
    if r.source.is_local() {
        if r.origin.is_empty() {
            r.path.clone()
        } else {
            format!("{}/{}", r.origin, r.path)
        }
    } else {
        format!("{}/{}", online_base(cfg, r), r.path)
    }
}

/// Load a store's rows from `path`, tagged `source`. None if the file is
/// absent, unopenable, or unreadable — callers treat each tier as best-effort.
fn load_store(path: &std::path::Path, source: Source) -> Option<Vec<Row>> {
    if !path.exists() {
        return None;
    }
    Store::open(path)
        .ok()
        .and_then(|s| s.load_rows(source).ok())
}

/// Canonicalize a path for identity comparison, falling back to the path itself
/// when it can't be resolved (e.g. it doesn't exist yet).
fn canon(p: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Parse one `.jd` home's `<lib>/` tree into live query rows. Files key relative
/// to the home (`library/foo/bar.jd`), matching `jd build`, so `get` reads each
/// body back under the home dir. Edges resolve within the home — self-contained,
/// like a nested home's own built store. `origin` is stamped by the caller.
fn parse_home(home: &std::path::Path, lib: &str) -> Vec<Row> {
    let libdir = home.join(lib);
    if !libdir.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    graph::collect_jd(&libdir, &mut files);
    // LC_ALL=C byte-order sort, matching `jd build`'s reproducible walk order.
    files.sort_by(|a, b| {
        a.as_os_str()
            .as_encoded_bytes()
            .cmp(b.as_os_str().as_encoded_bytes())
    });
    let nodes: Vec<jd::Node> = files
        .iter()
        .filter_map(|f| {
            let rel = f.strip_prefix(home).unwrap_or(f);
            let rel = rel.to_string_lossy().replace('\\', "/");
            let content = std::fs::read_to_string(f).ok()?;
            Some(jd::parse(&rel, &content))
        })
        .collect();
    rows_from_nodes(&nodes, Source::Local)
}

/// The LIVE repo graph: every repo-local `.jd` home parsed fresh on this query
/// (never a cached store — these files change too often). Homes come back
/// **deeper-first** so a deeper home's key wins (the keep-first dedup in
/// [`gather`] applies it). With nesting off (`JUSTDOWN_NESTED=0`) it's just the
/// root home. Each row carries the home dir as `origin` (empty for the root, so
/// `get` reads it under `cfg.root`).
fn live_cwd_rows(cfg: &Config) -> Vec<Row> {
    let root_canon = canon(&cfg.root);
    let homes = if Config::nested_enabled() {
        graph::find_jd_homes(&cfg.project_dir())
    } else {
        Vec::new()
    };
    let mut out: Vec<Row> = Vec::new();
    let mut saw_root = false;
    for home in &homes {
        let is_root = canon(home) == root_canon;
        saw_root |= is_root;
        let origin = if is_root {
            String::new()
        } else {
            home.to_string_lossy().into_owned()
        };
        for mut row in parse_home(home, &cfg.lib) {
            row.origin = origin.clone();
            out.push(row);
        }
    }
    // The root home is the precedence floor: ensure it's parsed (and last, so it
    // loses collisions to deeper homes) even if discovery missed it.
    if !saw_root {
        out.extend(parse_home(&cfg.root, &cfg.lib));
    }
    out
}

/// Gather the merged, deduped row set from the two graphs: the LIVE repo-local
/// `.jd` files (deeper homes first) shadowing the CACHED belt (`jd refresh`).
/// Local always wins by key; among local homes the deeper one wins and each
/// shadowed key is logged. Only a total absence of both is a hard error (exit 4).
fn gather(cfg: &Config) -> Result<Vec<Row>, i32> {
    let local = live_cwd_rows(cfg);
    let cached = cached_belt_rows(cfg);

    if local.is_empty() && cached.is_empty() {
        emit_err(
            cfg,
            "source-unreachable",
            "no repo-local .jd files and no cached belt (run `jd refresh`)",
        );
        return Err(4);
    }

    // Merge order = precedence: live local (deeper-first), then the cached belt.
    // Keep-first dedup means a deeper local home shadows a shallower one, and any
    // local row shadows the cached belt.
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in local {
        if seen.insert(row.key.clone()) {
            out.push(row);
        } else {
            let where_ = if row.origin.is_empty() {
                "<root>".to_string()
            } else {
                row.origin.clone()
            };
            eprintln!(
                "jd: note: '{}' in {where_} shadowed by a deeper .jd home",
                row.key
            );
        }
    }
    for row in cached {
        if seen.insert(row.key.clone()) {
            out.push(row);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// semantic-lite ranking (--mode semantic)
//
// No model, no deps — `jd` stays a self-contained binary. Three pure-Rust
// signals approximate meaning: a small synonym map widens the query
// ("smaller" → resize/scale/compress), a light suffix stemmer collapses
// inflections ("logs" → "log"), and character-trigram cosine rewards near
// wording. Recall-boosting, NOT true embeddings — regex/fuzzy stay on the pipe.
// ---------------------------------------------------------------------------

/// Light suffix-stripping stemmer (not full Porter): collapses common plural /
/// verb inflections so their forms share a stem. Longest suffix wins; a 2-char
/// floor keeps short stems from being gutted.
fn stem(w: &str) -> String {
    const SUF: &[&str] = &[
        "izations", "ization", "ations", "ation", "ings", "ing", "ers", "er", "ed", "es", "ly", "s",
    ];
    for suf in SUF {
        if w.len() >= suf.len() + 2 && w.ends_with(suf) {
            return w[..w.len() - suf.len()].to_string();
        }
    }
    w.to_string()
}

/// Hand-curated synonym widening for the intent verbs/nouns common in tool
/// queries. Returns related terms (including the input's neighbours) or an empty
/// slice. Small on purpose — a lexicon, not a thesaurus.
fn synonyms(term: &str) -> &'static [&'static str] {
    match term {
        "smaller" | "shrink" | "compress" | "reduce" | "size" => {
            &["resize", "scale", "compress", "smaller", "shrink"]
        }
        "bigger" | "enlarge" | "grow" | "upscale" => &["resize", "scale", "bigger", "enlarge"],
        "make" | "create" | "new" | "generate" | "init" => {
            &["create", "make", "build", "init", "generate"]
        }
        "remove" | "delete" | "erase" | "clean" | "purge" => {
            &["remove", "delete", "prune", "clean", "rm"]
        }
        "find" | "locate" | "lookup" | "grep" => &["search", "find", "grep", "locate"],
        "show" | "display" | "view" | "print" => &["show", "list", "display", "print"],
        "convert" | "transform" | "change" | "transcode" => {
            &["convert", "transform", "transcode", "change"]
        }
        "video" | "movie" | "clip" | "film" => &["video", "media", "ffmpeg", "movie"],
        "image" | "picture" | "photo" | "img" => &["image", "picture", "magick", "photo"],
        "folder" | "directory" | "dir" => &["directory", "folder", "dir"],
        "fast" | "speed" | "quick" | "benchmark" => &["fast", "speed", "bench", "benchmark"],
        "secret" | "password" | "credential" | "key" => &["secret", "credential", "key", "token"],
        _ => &[],
    }
}

/// Character-trigram bag of a string, padded so word edges form trigrams.
fn trigrams(s: &str) -> std::collections::HashMap<[char; 3], f64> {
    let chars: Vec<char> = format!("  {s}  ").chars().collect();
    let mut m = std::collections::HashMap::new();
    for w in chars.windows(3) {
        *m.entry([w[0], w[1], w[2]]).or_insert(0.0) += 1.0;
    }
    m
}

/// Cosine similarity of two trigram bags, in [0, 1].
fn cosine(
    a: &std::collections::HashMap<[char; 3], f64>,
    b: &std::collections::HashMap<[char; 3], f64>,
) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let dot: f64 = a
        .iter()
        .map(|(k, va)| b.get(k).map_or(0.0, |vb| va * vb))
        .sum();
    let na: f64 = a.values().map(|v| v * v).sum::<f64>().sqrt();
    let nb: f64 = b.values().map(|v| v * v).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Stemmed token set of a field.
fn stem_tokens(field: &str) -> std::collections::HashSet<String> {
    words(field).into_iter().map(stem).collect()
}

/// Semantic-lite rank. Same field weights and tie-breaks as `rank`, but matches
/// on synonym-widened, stemmed terms and adds a trigram-cosine bonus (0..=6)
/// over the row's combined text — so a meaning-shaped query can surface the
/// right tool even with no shared surface word.
fn rank_semantic<'a>(rows: &'a [Row], query: &str, kind: &str, category: &str) -> Vec<Scored<'a>> {
    let q = query.to_lowercase();
    let base: Vec<String> = words(&q)
        .into_iter()
        .filter(|t| !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect();

    // widen with synonyms, stem the lot, dedup preserving order
    let mut expanded: Vec<String> = Vec::new();
    for t in &base {
        for cand in std::iter::once(t.as_str()).chain(synonyms(t).iter().copied()) {
            let s = stem(cand);
            if !expanded.contains(&s) {
                expanded.push(s);
            }
        }
    }
    let qtri = trigrams(&q);

    let deg = degree_map(rows);
    let mut scored: Vec<Scored> = Vec::new();
    for row in rows {
        if !kind.is_empty() && row.kind != kind {
            continue;
        }
        if !category.is_empty() && row.category != category {
            continue;
        }
        // veto on stemmed not_when against the original query terms
        let notw = stem_tokens(&row.not_when.to_lowercase());
        if base.iter().any(|t| notw.contains(&stem(t))) {
            continue;
        }
        let name = stem_tokens(&row.name.to_lowercase());
        let usew = stem_tokens(&row.use_when.to_lowercase());
        let tags = stem_tokens(&row.tags.to_lowercase());
        let purpose = stem_tokens(&row.purpose.to_lowercase());

        let mut score = 0i64;
        for t in &expanded {
            if name.contains(t) || usew.contains(t) {
                score += 3;
            } else if tags.contains(t) {
                score += 2;
            } else if purpose.contains(t) {
                score += 1;
            }
        }
        // trigram-cosine bonus over the row's combined text
        let doc =
            format!("{} {} {} {}", row.name, row.tags, row.use_when, row.purpose).to_lowercase();
        let sim = cosine(&qtri, &trigrams(&doc));
        score += (sim * 6.0).round() as i64;

        if score <= 0 {
            continue;
        }
        scored.push(Scored { score, row });
    }
    let dg = |k: &str| deg.get(k).copied().unwrap_or(0);
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| dg(&b.row.key).cmp(&dg(&a.row.key)))
            .then_with(|| a.row.name.cmp(&b.row.name))
    });
    scored
}

pub fn search(cfg: &Config, args: &[String]) -> i32 {
    // Pull the optional `--mode <exact|semantic>` flag out of the positionals.
    // exact (default) is the field-weighted substring rank; semantic widens the
    // query with synonyms + stemming and adds a trigram-cosine signal for
    // meaning-shaped queries ("make video smaller"). regex/fuzzy are
    // deliberately NOT modes — pipe the JSON to rg/fzf instead.
    let mut mode = String::new();
    let mut pos: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--mode" {
            i += 1;
            match args.get(i) {
                Some(m) => mode = m.clone(),
                None => {
                    emit_err(cfg, "bad-args", "--mode needs a value");
                    return 3;
                }
            }
        } else if let Some(m) = a.strip_prefix("--mode=") {
            mode = m.to_string();
        } else {
            pos.push(a.clone());
        }
        i += 1;
    }
    if mode.is_empty() {
        mode = "exact".to_string();
    }
    if !matches!(mode.as_str(), "exact" | "semantic") {
        emit_err(
            cfg,
            "bad-args",
            &format!("unknown mode: {mode} (want exact|semantic)"),
        );
        return 3;
    }

    let query = match pos.first() {
        Some(q) if !q.is_empty() => q.clone(),
        _ => {
            emit_err(cfg, "bad-args", "search needs a query");
            return 3;
        }
    };
    let kind = pos.get(1).cloned().unwrap_or_default();
    let num_s = pos.get(2).cloned().unwrap_or_else(|| "5".to_string());
    let category = pos.get(3).cloned().unwrap_or_default();

    if !kind.is_empty() && !matches!(kind.as_str(), "tool" | "agent" | "knowledge" | "workflow") {
        emit_err(
            cfg,
            "bad-args",
            &format!("unknown kind: {kind} (want tool|agent|knowledge|workflow)"),
        );
        return 3;
    }
    if num_s.is_empty() || !num_s.bytes().all(|b| b.is_ascii_digit()) {
        emit_err(
            cfg,
            "bad-args",
            &format!("num must be a positive integer: {num_s}"),
        );
        return 3;
    }
    let mut num: i64 = num_s.parse().unwrap_or(5);
    if num <= 0 {
        num = 5;
    }

    let rows = match gather(cfg) {
        Ok(r) => r,
        Err(c) => return c,
    };

    let scored = if mode == "semantic" {
        rank_semantic(&rows, &query, &kind, &category)
    } else {
        rank(&rows, &query, &kind, &category)
    };

    let take = scored.len().min(num as usize);
    let shown = &scored[..take];

    // Universal fallback: the curated graph matched nothing, so point at the
    // cht.sh cheat-sheet tool — it answers for any command or language live.
    // Advisory only: exit stays 2 (the library itself had no hit) so callers
    // can still tell a real graph hit from the fallback.
    if shown.is_empty() {
        return emit_fallback(cfg, &query, &rows);
    }

    match cfg.format {
        Format::Json => {
            let results = shown
                .iter()
                .map(|s| {
                    let r = s.row;
                    let raw = raw_display(cfg, r);
                    SearchResult {
                        name: &r.name,
                        kind: &r.kind,
                        score: s.score,
                        purpose: &r.purpose,
                        raw,
                        source: r.source.label(),
                        danger: if r.danger.is_empty() {
                            "none"
                        } else {
                            &r.danger
                        },
                        side_effects: csv_vec(&r.side_effects),
                        requires: csv_vec(&r.requires),
                    }
                })
                .collect();
            println!(
                "{}",
                to_json(&SearchOut {
                    schema: "justdown.search/1",
                    query: &query,
                    results,
                    fallback: None,
                })
            );
        }
        Format::Text => {
            for (i, s) in shown.iter().enumerate() {
                let r = s.row;
                let mut raw = raw_display(cfg, r);
                if r.source.is_local() {
                    raw.push_str(&format!(" ({})", r.source.label()));
                }
                println!(
                    "{}. {}  [{}]  score {}\n   {}\n   {}",
                    i + 1,
                    r.name,
                    r.kind,
                    s.score,
                    r.purpose,
                    raw
                );
                // surface safety only when it matters
                if r.danger == "high" || r.danger == "medium" || !r.side_effects.is_empty() {
                    let mut line = format!(
                        "   ⚠ danger={}",
                        if r.danger.is_empty() {
                            "none"
                        } else {
                            &r.danger
                        }
                    );
                    if !r.side_effects.is_empty() {
                        line.push_str(&format!("  effects={}", r.side_effects));
                    }
                    if !r.requires.is_empty() {
                        line.push_str(&format!("  requires={}", r.requires));
                    }
                    println!("{line}");
                }
            }
        }
    }

    0
}

/// The graph node surfaced when a search matches nothing in the curated
/// library: the cht.sh cheat-sheet tool, which answers for any command or
/// language. Key is `<category>/<name>` per `key_and_category`.
const FALLBACK_KEY: &str = "help/cht";

/// Emit the cht.sh fallback pointer on a zero-hit search. Keeps the JSON
/// envelope (empty `results`, plus a `fallback` object) and prints a one-line
/// pointer in text mode. Returns exit 2 — the library had no match; the
/// fallback is advisory, not a graph hit. If the fallback node isn't present
/// (a library without it), behaves like the old empty result.
fn emit_fallback(cfg: &Config, query: &str, rows: &[Row]) -> i32 {
    let row = rows.iter().find(|r| r.key == FALLBACK_KEY);
    match (row, cfg.format) {
        (Some(r), Format::Json) => {
            let raw = raw_display(cfg, r);
            println!(
                "{}",
                to_json(&SearchOut {
                    schema: "justdown.search/1",
                    query,
                    results: Vec::new(),
                    fallback: Some(Fallback {
                        reason: "no-match",
                        name: &r.name,
                        kind: &r.kind,
                        purpose: &r.purpose,
                        raw,
                    }),
                })
            );
        }
        (Some(r), Format::Text) => {
            eprintln!(
                "jd: no library file matched '{query}'; cht.sh covers any command or language"
            );
            println!(
                "↳ fallback: {}  [{}]\n   {}\n   get @{} — then run its lang/sheet recipe via just",
                r.name, r.kind, r.purpose, r.key
            );
        }
        (None, Format::Json) => {
            println!(
                "{}",
                to_json(&SearchOut {
                    schema: "justdown.search/1",
                    query,
                    results: Vec::new(),
                    fallback: None,
                })
            );
        }
        (None, Format::Text) => {}
    }
    2
}

// ---------------------------------------------------------------------------
// get
// ---------------------------------------------------------------------------

fn basename(path: &str) -> String {
    let p = path.strip_suffix(".jd").unwrap_or(path);
    p.rsplit('/').next().unwrap_or(p).to_string()
}

pub fn get(cfg: &Config, args: &[String]) -> i32 {
    // Split args into: `--var name=value` host vars, `--<profile>` output flags,
    // and the single positional ref. Env-sourced vars seed the map; --var flags
    // layer on top so a per-call flag overrides the environment.
    let mut vars = Config::env_vars();
    let mut positional: Vec<&String> = Vec::new();
    let mut flags: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let pair = if a == "--var" {
            i += 1;
            match args.get(i) {
                Some(p) => p.as_str(),
                None => {
                    emit_err(cfg, "bad-args", "--var needs name=value");
                    return 3;
                }
            }
        } else if let Some(p) = a.strip_prefix("--var=") {
            p
        } else if a.starts_with("--") {
            flags.push(a.as_str());
            i += 1;
            continue;
        } else {
            positional.push(a);
            i += 1;
            continue;
        };
        match pair.split_once('=') {
            Some((name, value)) if !name.is_empty() => {
                vars.insert(name.to_string(), value.to_string());
            }
            _ => {
                emit_err(cfg, "bad-args", &format!("--var wants name=value: {pair}"));
                return 3;
            }
        }
        i += 1;
    }

    let profile = match parse_profile(&flags) {
        Ok(p) => p,
        Err(msg) => {
            emit_err(cfg, "bad-args", &msg);
            return 3;
        }
    };

    let refr = match positional.first() {
        Some(r) if !r.is_empty() => (*r).clone(),
        _ => {
            emit_err(cfg, "bad-args", "get needs a ref");
            return 3;
        }
    };
    if positional.len() > 1 {
        emit_err(
            cfg,
            "bad-args",
            "get takes one ref; select output with --human|--agent|--frontmatter|--justfile",
        );
        return 3;
    }

    let rows = match gather(cfg) {
        Ok(r) => r,
        Err(c) => return c,
    };

    let row = match resolved_or_err(cfg, &rows, &refr) {
        Ok(r) => r,
        Err(c) => return c,
    };

    // refuse suspicious paths (absolute or traversal)
    if row.path.starts_with('/') || row.path.contains("..") {
        emit_err(
            cfg,
            "bad-args",
            &format!("refusing suspicious path: {}", row.path),
        );
        return 3;
    }

    let body = if row.source.is_local() {
        // Repo-local paths key relative to the `.jd` home (`library/…`); a nested
        // home stamps its dir on `origin`, so its files resolve there rather than
        // under the root home.
        let base = if row.origin.is_empty() {
            cfg.root.clone()
        } else {
            std::path::PathBuf::from(&row.origin)
        };
        match std::fs::read_to_string(base.join(&row.path)).ok() {
            Some(b) => b,
            None => {
                emit_err(
                    cfg,
                    "source-unreachable",
                    &format!("cannot read {} file: {}", row.source.label(), row.path),
                );
                return 4;
            }
        }
    } else {
        // Online paths key relative to the remote's `.jd` home, so the file
        // lives under `<raw_base>/.jd/<path>`.
        let url = format!("{}/.jd/{}", online_base(cfg, row), row.path);
        match curl_to_string(&url) {
            Some(b) => b,
            None => {
                emit_err(cfg, "source-unreachable", &format!("cannot fetch: {url}"));
                return 4;
            }
        }
    };

    // Resolve the requested profile to the sections it emits, gating --justfile
    // on the file's kind: only runnable kinds (tool|workflow) yield an executable
    // justfile — agent/knowledge/type/event .jd files are not scripts (exit 3).
    // `headers` is false for the raw single-payload views (justfile, human),
    // which emit their content verbatim with no `# kind` banner.
    let (sections, headers): (Vec<(String, String)>, bool) = match profile {
        Profile::Justfile => {
            if !justfile_kind(&row.kind) {
                emit_err(
                    cfg,
                    "bad-args",
                    &format!(
                        "no executable payload: kind '{}' defines types/events, not a recipe — --justfile needs kind tool|workflow",
                        row.kind
                    ),
                );
                return 3;
            }
            let joined = split_sections(&body, "tools")
                .into_iter()
                .map(|(_, c)| c)
                .collect::<Vec<_>>()
                .join("\n");
            (vec![("justfile".to_string(), joined)], false)
        }
        Profile::Human => (vec![("human".to_string(), strip_frontmatter(&body))], false),
        Profile::Frontmatter => (split_sections(&body, "frontmatter"), true),
        Profile::Agent => {
            // Contract + prose, no raw recipe. `split_sections` folds prose into
            // a recipe-bearing block, so build prose directly (fences stripped)
            // rather than filtering its sections.
            let mut secs = split_sections(&body, "frontmatter");
            let prose = prose_only(&body);
            if !prose.is_empty() {
                secs.push(("prose".to_string(), prose));
            }
            (secs, true)
        }
        Profile::Default => (split_sections(&body, ""), true),
    };

    // Context injection: resolve `<<var>>` escapes against host-supplied values
    // (env + --var) just before output — the consume point the jd spec names
    // ("before a file is consumed"). One pass, non-recursive, so a spliced value
    // can't smuggle in further escapes.
    let sections = inject_vars(sections, &vars);
    match cfg.format {
        Format::Json => {
            let secs = sections
                .iter()
                .map(|(kind, content)| Section { kind, content })
                .collect();
            println!(
                "{}",
                to_json(&GetOut {
                    schema: "justdown.get/1",
                    refr: &refr,
                    sections: secs,
                })
            );
        }
        Format::Text => {
            for (kind, content) in &sections {
                if headers {
                    println!("# {kind}");
                    println!("{content}");
                    println!();
                } else {
                    println!("{content}");
                }
            }
        }
    }
    0
}

/// Output profile for `get`: a kind-gated view of one `.jd` file, selected by a
/// single `--<profile>` flag. With no flag the default emits all sections.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Profile {
    /// All sections in document order (frontmatter, prose, tools) — the default.
    Default,
    /// The retrieval contract only (frontmatter / yaml).
    Frontmatter,
    /// What a person reads: prose + fenced blocks, no yaml.
    Human,
    /// What an agent reasons over: the contract + prose, no raw recipe.
    Agent,
    /// Vanilla just recipes only, host-resolved — ready for `just --justfile -`.
    /// Refused unless the file's kind is executable (see [`justfile_kind`]).
    Justfile,
}

/// Map the `--<profile>` output flags to a single [`Profile`]. At most one
/// profile flag is allowed; an unknown `--flag` or a second profile flag is an
/// error (the caller maps it to exit 3). `--var`/`--json` are handled elsewhere
/// and never reach here.
fn parse_profile(flags: &[&str]) -> Result<Profile, String> {
    let mut prof = Profile::Default;
    for f in flags {
        let p = match *f {
            "--frontmatter" => Profile::Frontmatter,
            "--human" => Profile::Human,
            "--agent" => Profile::Agent,
            "--justfile" => Profile::Justfile,
            other => return Err(format!("unknown flag: {other}")),
        };
        if prof != Profile::Default {
            return Err(
                "only one output profile (--human|--agent|--frontmatter|--justfile)".to_string(),
            );
        }
        prof = p;
    }
    Ok(prof)
}

/// Whether a file of the given `kind` may emit an executable `--justfile`. Only
/// runnable kinds qualify; agent/knowledge/type/event `.jd` files are not
/// scripts and are refused. The contract is an allowlist, so any future
/// non-runnable kind is refused by default.
fn justfile_kind(kind: &str) -> bool {
    matches!(kind, "tool" | "workflow")
}

/// The body with its leading `---`…`---` frontmatter block removed — what a
/// human reads (the rendered markdown, yaml stripped). No frontmatter, or an
/// unterminated one, returns the text verbatim. Blank lines immediately after
/// the block are trimmed.
fn strip_frontmatter(body: &str) -> String {
    let mut lines = body.lines();
    if lines.next() != Some("---") {
        return body.to_string();
    }
    let mut closed = false;
    let mut out: Vec<&str> = Vec::new();
    for l in lines.by_ref() {
        if !closed {
            if l == "---" {
                closed = true;
            }
            continue;
        }
        out.push(l);
    }
    if !closed {
        return body.to_string();
    }
    while out.first() == Some(&"") {
        out.remove(0);
    }
    out.join("\n")
}

/// The body's prose only — frontmatter and every fenced block (```just /
/// ```psaido / any ```) removed — what an agent reasons over without the raw
/// recipe. Leading and trailing blank lines are trimmed.
fn prose_only(body: &str) -> String {
    let stripped = strip_frontmatter(body);
    let mut out: Vec<&str> = Vec::new();
    let mut fence = false;
    for l in stripped.lines() {
        if l.starts_with("```") {
            fence = !fence;
            continue;
        }
        if !fence {
            out.push(l);
        }
    }
    while out.first() == Some(&"") {
        out.remove(0);
    }
    while out.last() == Some(&"") {
        out.pop();
    }
    out.join("\n")
}

/// Apply the `<<var>>` render pass to every section's content. Frontmatter,
/// prose, and tool bodies all pass through the same single-pass renderer.
fn inject_vars(sections: Vec<(String, String)>, vars: &Vars) -> Vec<(String, String)> {
    if vars.is_empty() {
        return sections;
    }
    sections
        .into_iter()
        .map(|(kind, content)| (kind, render::render(&content, vars)))
        .collect()
}

/// Split a .jd body into ordered sections: [0] frontmatter, then prose | tools
/// blocks separated by top-level `---`. Mirrors the awk section splitter.
fn split_sections(body: &str, only: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let collect_fm = only.is_empty() || only == "frontmatter";

    let mut infm = 0; // 0 before, 1 inside frontmatter, 2 after
    let mut fmbuf: Vec<&str> = Vec::new();
    let mut fence = false;
    let mut blk: Vec<&str> = Vec::new();
    let plat = host_platform();

    let flush = |blk: &mut Vec<&str>, sections: &mut Vec<(String, String)>| {
        if blk.is_empty() {
            return;
        }
        let isjust = blk.iter().any(|l| l.starts_with("```just"));
        if isjust && (only.is_empty() || only == "tools") {
            let mut buf: Vec<&str> = Vec::new();
            let mut injust = false;
            for l in blk.iter() {
                if l.starts_with("```just") {
                    injust = true;
                    continue;
                }
                if injust && l.starts_with("```") {
                    injust = false;
                    continue;
                }
                if injust {
                    buf.push(l);
                }
            }
            // justdown extension: resolve [unix]/[macos]/[windows]/[wsl] recipe
            // variants for this host and strip the attr lines, so plain `just`
            // downstream never sees them (it has no [wsl] of its own).
            let buf = platsel(&buf, &plat);
            sections.push(("tools".to_string(), buf.join("\n")));
        } else if !isjust && (only.is_empty() || only == "prose") {
            sections.push(("prose".to_string(), blk.join("\n")));
        }
        blk.clear();
    };

    for (idx, line) in body.lines().enumerate() {
        if idx == 0 && line == "---" {
            infm = 1;
            continue;
        }
        if infm == 1 && line == "---" {
            infm = 2;
            if collect_fm {
                sections.push(("frontmatter".to_string(), fmbuf.join("\n")));
            }
            continue;
        }
        if infm == 1 {
            if collect_fm {
                fmbuf.push(line);
            }
            continue;
        }
        // body
        if line.starts_with("```") {
            fence = !fence;
        }
        if !fence && line == "---" {
            flush(&mut blk, &mut sections);
            continue;
        }
        if !blk.is_empty() || !line.is_empty() {
            blk.push(line);
        }
    }
    flush(&mut blk, &mut sections);
    sections
}

// ---------------------------------------------------------------------------
// platform-guarded recipe variants (justdown extension)
// ---------------------------------------------------------------------------

// Platform-variant resolution lives in the shared `justdown` core crate
// (`justdown::platform`) so the `jd` CLI and bombshell resolve `[os]` variants
// identically instead of each carrying a copy that drifts. `get` still selects
// the host variant when emitting a justfile, so re-export just what this module
// uses; the lint-side helpers are consumed directly from core by `justdown::lint`.
pub(crate) use justdown::platform::{host_platform, platsel};

#[cfg(test)]
mod inject_tests {
    use super::{inject_vars, Vars};

    fn vars(pairs: &[(&str, &str)]) -> Vars {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn resolves_escapes_per_section() {
        let secs = vec![
            ("prose".to_string(), "cwd: <<cwd>>".to_string()),
            ("tools".to_string(), "open:\n  echo <<shell>>".to_string()),
        ];
        let out = inject_vars(secs, &vars(&[("cwd", "/tmp"), ("shell", "nu")]));
        assert_eq!(out[0].1, "cwd: /tmp");
        assert_eq!(out[1].1, "open:\n  echo nu");
    }

    #[test]
    fn no_vars_is_passthrough() {
        let secs = vec![("prose".to_string(), "cwd: <<cwd>>".to_string())];
        let out = inject_vars(secs.clone(), &Vars::new());
        assert_eq!(out, secs);
    }
}

#[cfg(test)]
mod semantic_tests {
    use super::{cosine, stem, synonyms, trigrams};

    #[test]
    fn stemmer_collapses_simple_inflections() {
        assert_eq!(stem("logs"), stem("log"));
        assert_eq!(stem("converts"), stem("convert"));
        assert_eq!(stem("removing"), "remov");
        // 2-char floor: short words are left intact
        assert_eq!(stem("id"), "id");
        assert_eq!(stem("is"), "is");
    }

    #[test]
    fn synonyms_widen_intent() {
        assert!(synonyms("smaller").contains(&"resize"));
        assert!(synonyms("delete").contains(&"prune"));
        assert!(synonyms("video").contains(&"ffmpeg"));
        assert!(synonyms("zzz").is_empty());
    }

    #[test]
    fn cosine_is_bounded() {
        let a = trigrams("docker compose");
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-9);
        // no shared trigrams → orthogonal
        assert_eq!(cosine(&trigrams("abc"), &trigrams("xyz")), 0.0);
        // near wording scores between 0 and 1
        let sim = cosine(&trigrams("resize image"), &trigrams("resizing images"));
        assert!(sim > 0.3 && sim < 1.0);
    }
}

// ---------------------------------------------------------------------------
// ls
// ---------------------------------------------------------------------------

pub fn ls(cfg: &Config) -> i32 {
    let rows = match gather(cfg) {
        Ok(r) => r,
        Err(c) => return c,
    };

    // group by category, fall back to kind, then "misc"; preserve member order
    let mut order: Vec<String> = Vec::new();
    let mut members: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in &rows {
        let cat = if !r.category.is_empty() {
            r.category.clone()
        } else if !r.kind.is_empty() {
            r.kind.clone()
        } else {
            "misc".to_string()
        };
        if !members.contains_key(&cat) {
            order.push(cat.clone());
        }
        members.entry(cat).or_default().push(r.name.clone());
    }
    order.sort();

    match cfg.format {
        Format::Json => {
            let categories = order
                .iter()
                .map(|c| LsCategory {
                    name: c,
                    members: &members[c],
                })
                .collect();
            println!(
                "{}",
                to_json(&LsOut {
                    schema: "justdown.ls/1",
                    categories,
                })
            );
        }
        Format::Text => {
            for c in &order {
                // original joins members with a leading space per item
                let line: String = members[c].iter().map(|m| format!(" {m}")).collect();
                println!("{c}:{line}");
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------
// links
// ---------------------------------------------------------------------------

pub fn links(cfg: &Config, args: &[String]) -> i32 {
    let refr = match args.first() {
        Some(r) if !r.is_empty() => r.clone(),
        _ => {
            emit_err(cfg, "bad-args", "links needs a ref");
            return 3;
        }
    };
    let rows = match gather(cfg) {
        Ok(r) => r,
        Err(c) => return c,
    };

    let target = match resolved_or_err(cfg, &rows, &refr) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let key = &target.key;
    let known: std::collections::HashSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();

    // outbound: target's links that resolve to a known node (no self-loop)
    let outbound: Vec<String> = target
        .links
        .iter()
        .filter(|t| t.as_str() != key && known.contains(t.as_str()))
        .cloned()
        .collect();
    // inbound: other nodes that link to key
    let inbound: Vec<String> = rows
        .iter()
        .filter(|r| &r.key != key && r.links.iter().any(|d| d == key))
        .map(|r| r.name.clone())
        .collect();

    let fuzzy = target.fuzzy.clone();

    match cfg.format {
        Format::Json => {
            println!(
                "{}",
                to_json(&LinksOut {
                    schema: "justdown.links/1",
                    refr: &refr,
                    key,
                    outbound,
                    inbound,
                    fuzzy,
                })
            );
        }
        Format::Text => {
            for o in &outbound {
                println!("out  @{o}");
            }
            for i in &inbound {
                println!("in   {i}  (@{key})");
            }
            for f in &fuzzy {
                println!("fuzz @?{f}");
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------
// path — shortest connection between two tools through the @link graph
// ---------------------------------------------------------------------------

/// Strip a leading `@` and a trailing `#section` from a ref, leaving the bare
/// name | key | path | basename needle the resolvers match on.
fn normalize_ref(refr: &str) -> String {
    let mut needle = refr.to_string();
    if let Some(s) = needle.strip_prefix('@') {
        needle = s.to_string();
    }
    if let Some(i) = needle.find('#') {
        needle.truncate(i);
    }
    needle
}

/// The outcome of resolving a ref against the merged row set.
enum Resolution<'a> {
    /// Exactly one file matched.
    Unique(&'a Row),
    /// Nothing matched.
    None,
    /// The ref matched more than one distinct file. Carries each candidate's
    /// key — the fully-qualified ref that selects it — so the caller can tell
    /// the user how to disambiguate.
    Ambiguous(Vec<String>),
}

/// Resolve a ref to a single row, refusing rather than guessing when it is
/// ambiguous. An exact identifier — name, key, or path — is a unique address
/// and wins outright; only the convenience *basename* match can collide (two
/// files share a leaf name in different categories, e.g. `meta/tools/release`
/// and `vcs/gh/release` both basename `release`). When the basename matches
/// more than one file we report every candidate instead of silently taking the
/// first, so a bare ref can never resolve to the wrong file undetected. Rows
/// are deduped by key so reaching one file via two of its identifiers, or the
/// same key across merge tiers, still counts once.
fn resolve_ref<'a>(rows: &'a [Row], refr: &str) -> Resolution<'a> {
    let needle = normalize_ref(refr);

    let dedup_keys = |matches: Vec<&'a Row>| -> Vec<&'a Row> {
        let mut seen = std::collections::HashSet::new();
        matches
            .into_iter()
            .filter(|r| seen.insert(r.key.clone()))
            .collect()
    };

    // Tier 1: exact identifier. A name/key/path hit is a qualified address.
    let exact = dedup_keys(
        rows.iter()
            .filter(|r| r.name == needle || r.key == needle || r.path == needle)
            .collect(),
    );
    match exact.as_slice() {
        [r] => return Resolution::Unique(r),
        [] => {}
        many => return Resolution::Ambiguous(many.iter().map(|r| r.key.clone()).collect()),
    }

    // Tier 2: convenience basename. The one collision-prone match — refuse when
    // it is not unique.
    let by_base = dedup_keys(
        rows.iter()
            .filter(|r| basename(&r.path) == needle)
            .collect(),
    );
    match by_base.as_slice() {
        [r] => Resolution::Unique(r),
        [] => Resolution::None,
        many => Resolution::Ambiguous(many.iter().map(|r| r.key.clone()).collect()),
    }
}

/// Resolve a ref to a row for a caller that returns an exit code, emitting the
/// right error on the None / Ambiguous arms. The ambiguity message lists every
/// candidate key and suggests qualifying with one, so the fix is in the output.
fn resolved_or_err<'a>(cfg: &Config, rows: &'a [Row], refr: &str) -> Result<&'a Row, i32> {
    match resolve_ref(rows, refr) {
        Resolution::Unique(r) => Ok(r),
        Resolution::None => {
            emit_err(cfg, "not-found", &format!("no file: {refr}"));
            Err(2)
        }
        Resolution::Ambiguous(keys) => {
            emit_err(
                cfg,
                "ambiguous-ref",
                &format!(
                    "'{}' matches {} files: {} — qualify it (e.g. `{}`)",
                    refr,
                    keys.len(),
                    keys.join(", "),
                    keys[0]
                ),
            );
            Err(2)
        }
    }
}

/// `jd path <a> <b>` — the shortest chain of @links connecting two files,
/// treating links as undirected (the "best connection between tooling"). BFS
/// over the link graph; neighbours visited in sorted order for determinism.
/// Exit 0 with a path, 2 if the two are unconnected, 2 (with an error) if an
/// endpoint doesn't resolve, 3 on bad args.
pub fn path(cfg: &Config, args: &[String]) -> i32 {
    let (a, b) = match (args.first(), args.get(1)) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => (a.clone(), b.clone()),
        _ => {
            emit_err(cfg, "bad-args", "path needs two refs: jd path <a> <b>");
            return 3;
        }
    };
    let rows = match gather(cfg) {
        Ok(r) => r,
        Err(c) => return c,
    };

    let src = match resolved_or_err(cfg, &rows, &a) {
        Ok(r) => r.key.clone(),
        Err(c) => return c,
    };
    let dst = match resolved_or_err(cfg, &rows, &b) {
        Ok(r) => r.key.clone(),
        Err(c) => return c,
    };

    // undirected adjacency among known keys (sorted for deterministic BFS)
    let known: std::collections::HashSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    let mut adj: std::collections::HashMap<&str, std::collections::BTreeSet<&str>> =
        std::collections::HashMap::new();
    for r in &rows {
        for l in &r.links {
            if l.as_str() != r.key && known.contains(l.as_str()) {
                adj.entry(r.key.as_str()).or_default().insert(l.as_str());
                adj.entry(l.as_str()).or_default().insert(r.key.as_str());
            }
        }
    }

    // BFS from src to dst
    let chain: Option<Vec<String>> = {
        let mut prev: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
        let s: &str = &src;
        let d: &str = &dst;
        seen.insert(s);
        queue.push_back(s);
        let mut hit = s == d;
        while let Some(cur) = queue.pop_front() {
            if cur == d {
                hit = true;
                break;
            }
            if let Some(ns) = adj.get(cur) {
                for &n in ns {
                    if seen.insert(n) {
                        prev.insert(n, cur);
                        queue.push_back(n);
                    }
                }
            }
        }
        if hit {
            let mut path = vec![d.to_string()];
            let mut cur = d;
            while cur != s {
                let p = prev[cur];
                path.push(p.to_string());
                cur = p;
            }
            path.reverse();
            Some(path)
        } else {
            None
        }
    };

    match cfg.format {
        Format::Json => {
            let (path, length) = match &chain {
                Some(p) => (p.clone(), p.len() as i64 - 1),
                None => (Vec::new(), -1),
            };
            println!(
                "{}",
                to_json(&PathOut {
                    schema: "justdown.path/1",
                    from: &src,
                    to: &dst,
                    path,
                    length,
                })
            );
        }
        Format::Text => {
            if let Some(p) = &chain {
                println!("{}", p.join(" → "));
            }
        }
    }

    match chain {
        Some(_) => 0,
        None => 2,
    }
}

// ---------------------------------------------------------------------------
// resolve — link-target completion for the editor popup
// ---------------------------------------------------------------------------

/// `jd resolve <term> [num] [--fuzzy]` — the live `@link` autocomplete source.
///
/// Direct (default): ranked prefix completion over node key/name/leaf — what a
/// `@name` link offers as you type. Reports the unique canonical key when the
/// term resolves to exactly one node (`resolved`). Fuzzy (`--fuzzy`): runs the
/// shared field-weighted ranker (the same one `search` uses) — what a `@?term`
/// link matches, one-to-many. Always exits 0 (an empty match set is valid).
pub fn resolve(cfg: &Config, args: &[String]) -> i32 {
    let mut fuzzy = false;
    let mut pos: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--fuzzy" {
            fuzzy = true;
        } else {
            pos.push(a.clone());
        }
        i += 1;
    }

    let term = match pos.first() {
        Some(t) if !t.is_empty() => t.clone(),
        _ => {
            emit_err(cfg, "bad-args", "resolve needs a term");
            return 3;
        }
    };
    // strip a leading `@` then a `?` a caller may pass through verbatim; a `?`
    // prefix forces fuzzy mode (so `jd resolve @?soft` works like `--fuzzy soft`).
    let mut t = term.strip_prefix('@').unwrap_or(&term);
    if let Some(rest) = t.strip_prefix('?') {
        fuzzy = true;
        t = rest;
    }
    let term = t.to_string();
    let num: usize = pos
        .get(1)
        .and_then(|n| n.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(10);

    let rows = match gather(cfg) {
        Ok(r) => r,
        Err(c) => return c,
    };

    let (rows_matched, resolved) = links::resolve_term(&rows, &term, fuzzy, num);
    let matches: Vec<ResolveMatch> = rows_matched
        .iter()
        .map(|r| ResolveMatch {
            key: &r.key,
            kind: &r.kind,
            path: &r.path,
        })
        .collect();

    match cfg.format {
        Format::Json => {
            println!(
                "{}",
                to_json(&ResolveOut {
                    schema: "justdown.resolve/1",
                    query: &term,
                    fuzzy,
                    resolved,
                    matches,
                })
            );
        }
        Format::Text => {
            for m in &matches {
                let leaf = links::leaf(m.key);
                if fuzzy {
                    println!("{}  [{}]  @?{}  ({})", leaf, m.kind, term, m.key);
                } else {
                    println!("{}  [{}]  @{}", leaf, m.kind, m.key);
                }
            }
        }
    }
    0
}

#[cfg(test)]
mod resolve_tests {
    use super::{resolve_ref, Resolution};
    use justdown::store::{Row, Source};

    /// A minimal row carrying just the fields the resolver matches on.
    fn row(key: &str, name: &str, path: &str) -> Row {
        Row {
            source: Source::Local,
            origin: String::new(),
            key: key.to_string(),
            name: name.to_string(),
            kind: "tool".to_string(),
            description: String::new(),
            purpose: String::new(),
            tags: String::new(),
            path: path.to_string(),
            use_when: String::new(),
            not_when: String::new(),
            danger: String::new(),
            side_effects: String::new(),
            requires: String::new(),
            category: String::new(),
            run: String::new(),
            has_fm: true,
            links: Vec::new(),
            fuzzy: Vec::new(),
        }
    }

    /// The two `release` files that motivated the guard: same basename, distinct
    /// keys and names.
    fn release_pair() -> Vec<Row> {
        vec![
            row(
                "tools/release",
                "tools_release",
                "library/meta/tools/release.jd",
            ),
            row("gh/release", "gh_release", "library/vcs/gh/release.jd"),
        ]
    }

    #[test]
    fn bare_basename_collision_is_ambiguous() {
        let rows = release_pair();
        match resolve_ref(&rows, "release") {
            Resolution::Ambiguous(keys) => {
                assert_eq!(keys.len(), 2);
                assert!(keys.contains(&"tools/release".to_string()));
                assert!(keys.contains(&"gh/release".to_string()));
            }
            _ => panic!("bare ambiguous basename must refuse, not guess"),
        }
    }

    #[test]
    fn qualified_ref_resolves_uniquely() {
        let rows = release_pair();
        // by name, by key, and by full path each pin one file
        for (refr, want) in [
            ("tools_release", "tools/release"),
            ("gh/release", "gh/release"),
            ("library/meta/tools/release.jd", "tools/release"),
        ] {
            match resolve_ref(&rows, refr) {
                Resolution::Unique(r) => assert_eq!(r.key, want, "ref {refr}"),
                _ => panic!("qualified ref {refr} must resolve uniquely"),
            }
        }
    }

    #[test]
    fn at_prefix_and_section_suffix_are_stripped() {
        let rows = release_pair();
        match resolve_ref(&rows, "@tools/release#tools") {
            Resolution::Unique(r) => assert_eq!(r.key, "tools/release"),
            _ => panic!("@ref#section must normalize before matching"),
        }
    }

    #[test]
    fn unique_basename_still_resolves() {
        let rows = vec![row("gh/pr", "gh_pr", "library/vcs/gh/pr.jd")];
        match resolve_ref(&rows, "pr") {
            Resolution::Unique(r) => assert_eq!(r.key, "gh/pr"),
            _ => panic!("a basename matching one file must resolve"),
        }
    }

    #[test]
    fn no_match_is_none() {
        let rows = release_pair();
        assert!(matches!(resolve_ref(&rows, "nope"), Resolution::None));
    }

    #[test]
    fn same_file_via_two_identifiers_counts_once() {
        // A single row reached by both its name and basename must not look like
        // two candidates.
        let rows = vec![row("gh/release", "gh_release", "library/vcs/gh/release.jd")];
        assert!(matches!(
            resolve_ref(&rows, "release"),
            Resolution::Unique(_)
        ));
    }
}

#[cfg(test)]
mod get_profile_tests {
    use super::{
        justfile_kind, parse_profile, prose_only, split_sections, strip_frontmatter, Profile,
    };

    const DOC: &str = "---\nname: demo\nkind: tool\n---\n\n# Demo\n\nprose line\n\n```just\nrun:\n  echo hi\n```\n";

    #[test]
    fn no_flag_is_default() {
        assert_eq!(parse_profile(&[]), Ok(Profile::Default));
    }

    #[test]
    fn each_flag_maps_to_its_profile() {
        assert_eq!(parse_profile(&["--frontmatter"]), Ok(Profile::Frontmatter));
        assert_eq!(parse_profile(&["--human"]), Ok(Profile::Human));
        assert_eq!(parse_profile(&["--agent"]), Ok(Profile::Agent));
        assert_eq!(parse_profile(&["--justfile"]), Ok(Profile::Justfile));
    }

    #[test]
    fn two_profiles_is_an_error() {
        assert!(parse_profile(&["--human", "--justfile"]).is_err());
    }

    #[test]
    fn unknown_flag_is_an_error() {
        assert!(parse_profile(&["--nope"]).is_err());
    }

    #[test]
    fn justfile_kind_allowlist() {
        assert!(justfile_kind("tool"));
        assert!(justfile_kind("workflow"));
        // non-runnable kinds — and any future type/event kind — are refused.
        assert!(!justfile_kind("agent"));
        assert!(!justfile_kind("knowledge"));
        assert!(!justfile_kind("type"));
        assert!(!justfile_kind("event"));
        assert!(!justfile_kind(""));
    }

    #[test]
    fn strip_frontmatter_drops_yaml_and_leading_blanks() {
        let out = strip_frontmatter(DOC);
        assert!(!out.contains("name: demo"), "yaml must be gone: {out:?}");
        assert!(out.starts_with("# Demo"), "leading blanks trimmed: {out:?}");
        assert!(out.contains("```just"), "fenced blocks kept for human view");
    }

    #[test]
    fn strip_frontmatter_passthrough_without_block() {
        let plain = "# Title\n\nbody";
        assert_eq!(strip_frontmatter(plain), plain);
    }

    #[test]
    fn agent_prose_keeps_prose_drops_recipe() {
        // The agent view is contract + prose with the raw recipe removed —
        // prose survives even when it shares a block with a ```just recipe.
        let prose = prose_only(DOC);
        assert!(prose.contains("# Demo"), "heading kept: {prose:?}");
        assert!(prose.contains("prose line"), "prose kept: {prose:?}");
        assert!(!prose.contains("echo hi"), "recipe body dropped: {prose:?}");
        assert!(!prose.contains("```"), "fence markers dropped: {prose:?}");
        assert!(!prose.contains("name: demo"), "yaml dropped: {prose:?}");
    }

    #[test]
    fn justfile_selection_emits_recipe_only() {
        let joined = split_sections(DOC, "tools")
            .into_iter()
            .map(|(_, c)| c)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("run:"));
        assert!(joined.contains("echo hi"));
        // raw recipe — no fence markers, no yaml.
        assert!(!joined.contains("```"));
        assert!(!joined.contains("name: demo"));
    }
}
