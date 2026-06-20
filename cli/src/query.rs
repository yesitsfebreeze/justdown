// The query surface: search / get / ls / links. A faithful port of the
// original justfile awk — same field-weighted scoring, not_when veto, kind &
// category narrowing, degrade-not-fail, text + JSON output, exit codes (0/2/3/4).
// Merge is now three tiers: repo-LOCAL ⊕ machine-GLOBAL ⊕ ONLINE belt, nearer
// scope shadowing farther by key (local > global > online).

use crate::config::{Config, Format};
use justdown::render::{self, Vars};
use justdown::search::{degree_map, rank, words, Scored, STOPWORDS};
use justdown::store::{Row, Source, Store};

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Render a comma-joined field as a JSON array of strings.
fn json_arr(csv: &str) -> String {
    if csv.is_empty() {
        return "[]".to_string();
    }
    let parts: Vec<String> = csv.split(',').map(json_str).collect();
    format!("[{}]", parts.join(","))
}

fn emit_err(cfg: &Config, code: &str, msg: &str) {
    match cfg.format {
        Format::Json => {
            eprintln!(
                "{{\"schema\":\"justdown.error/1\",\"error\":\"{}\",\"message\":{}}}",
                code,
                json_str(msg)
            );
        }
        Format::Text => eprintln!("jd: {msg}"),
    }
}

// ---------------------------------------------------------------------------
// loading + merge
// ---------------------------------------------------------------------------

/// Fetch a URL to a file with curl. Best-effort: returns false on any failure
/// (curl absent, unreachable, 404). justdown already requires curl on PATH;
/// the online merge degrades to local-only when it isn't there.
fn curl_to_file(url: &str, dest: &std::path::Path) -> bool {
    std::process::Command::new("curl")
        .args(["-fsSL", url, "-o"])
        .arg(dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Fetch a URL to a string with curl. None on any failure.
fn curl_to_string(url: &str) -> Option<String> {
    let out = std::process::Command::new("curl")
        .args(["-fsSL", url])
        .output()
        .ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

/// Download one online store at `url` to a temp file (tagged `n` so concurrent
/// belt fetches don't collide) and load its rows as Online. Best-effort: any
/// failure (unreachable, 404, unreadable) yields None so callers degrade.
fn fetch_store(url: &str, n: usize) -> Option<Vec<Row>> {
    let tmp = std::env::temp_dir().join(format!("jd-online-{}-{n}.db", std::process::id()));
    if !curl_to_file(url, &tmp) {
        return None;
    }
    let rows = Store::open(&tmp)
        .ok()
        .and_then(|s| s.load_rows(Source::Online).ok());
    let _ = std::fs::remove_file(&tmp);
    rows
}

/// Fetch the whole online belt: every remote's published `.bombshell/jd/graph.db`
/// (the contract location), in belt order, each row tagged with its remote's raw
/// base so `get` fetches files from the right repo. Remotes that are non-GitHub,
/// unreachable, or index-less are silently skipped.
fn fetch_online_belt(cfg: &Config) -> Vec<Row> {
    // Walk the belt last→first so that, with `gather`'s keep-first dedup, a later
    // belt entry shadows an earlier one — matching `build_roots`' later-root-wins
    // rule, so online and built-graph precedence agree ("later entries win").
    let mut out = Vec::new();
    for (i, r) in cfg.remotes().iter().enumerate().rev() {
        let Some(raw) = r.raw_base() else { continue };
        let url = format!("{raw}/.bombshell/jd/graph.db");
        if let Some(mut rows) = fetch_store(&url, i) {
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

/// Gather the merged, deduped row set across the three tiers — repo-LOCAL
/// (`<root>/.bombshell/jd`), machine-GLOBAL (`~/.bombshell/jd`), and ONLINE.
/// Nearer scope shadows farther by key (local > global > online). On the
/// degrade path (no online) a note goes to stderr; only a total absence of
/// sources is a hard error (exit 4).
fn gather(cfg: &Config) -> Result<Vec<Row>, i32> {
    let local = load_store(&cfg.index_path(), Source::Local);
    let global = cfg
        .home_index_path()
        .as_deref()
        .and_then(|p| load_store(p, Source::Global));
    let online = fetch_online_belt(cfg);

    if local.is_none() && global.is_none() && online.is_empty() {
        emit_err(
            cfg,
            "source-unreachable",
            "no local or global store and online belt unreachable",
        );
        return Err(4);
    }
    if online.is_empty() && (local.is_some() || global.is_some()) {
        eprintln!("jd: note: online belt unreachable; using local/global only");
    }

    // Merge order = precedence: local, then global, then the online belt. Dedup
    // by key keeps the first (nearest) tier seen, so local shadows global shadows
    // online — the same rule the old local⊕online merge used, two tiers deeper.
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in local
        .into_iter()
        .flatten()
        .chain(global.into_iter().flatten())
        .chain(online)
    {
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
            let mut out = String::new();
            out.push_str(&format!(
                "{{\"schema\":\"justdown.search/1\",\"query\":{},\"results\":[",
                json_str(&query)
            ));
            for (i, s) in shown.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let r = s.row;
                let raw = if r.source.is_local() {
                    r.path.clone()
                } else {
                    format!("{}/{}", online_base(cfg, r), r.path)
                };
                let danger = if r.danger.is_empty() {
                    "none"
                } else {
                    &r.danger
                };
                out.push_str(&format!(
                    "{{\"name\":{},\"kind\":{},\"score\":{},\"purpose\":{},\"raw\":{},\"source\":{},\"danger\":{},\"side_effects\":{},\"requires\":{}}}",
                    json_str(&r.name),
                    json_str(&r.kind),
                    s.score,
                    json_str(&r.purpose),
                    json_str(&raw),
                    json_str(r.source.label()),
                    json_str(danger),
                    json_arr(&r.side_effects),
                    json_arr(&r.requires),
                ));
            }
            out.push_str("]}");
            println!("{out}");
        }
        Format::Text => {
            for (i, s) in shown.iter().enumerate() {
                let r = s.row;
                let mut raw = if r.source.is_local() {
                    r.path.clone()
                } else {
                    format!("{}/{}", online_base(cfg, r), r.path)
                };
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
            let raw = if r.source.is_local() {
                r.path.clone()
            } else {
                format!("{}/{}", online_base(cfg, r), r.path)
            };
            println!(
                "{{\"schema\":\"justdown.search/1\",\"query\":{},\"results\":[],\"fallback\":{{\"reason\":\"no-match\",\"name\":{},\"kind\":{},\"purpose\":{},\"raw\":{}}}}}",
                json_str(query),
                json_str(&r.name),
                json_str(&r.kind),
                json_str(&r.purpose),
                json_str(&raw),
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
                "{{\"schema\":\"justdown.search/1\",\"query\":{},\"results\":[]}}",
                json_str(query)
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

    // normalize ref: drop leading @, drop #section
    let mut needle = refr.clone();
    if let Some(s) = needle.strip_prefix('@') {
        needle = s.to_string();
    }
    if let Some(i) = needle.find('#') {
        needle.truncate(i);
    }

    let found = rows.iter().find(|r| {
        r.name == needle || r.key == needle || r.path == needle || basename(&r.path) == needle
    });
    let row = match found {
        Some(r) => r,
        None => {
            emit_err(cfg, "not-found", &format!("no file: {refr}"));
            return 2;
        }
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
        // Resolve the path against each plausible base for the tier. Repo-local
        // files may be authored (<root>/library/…) or vendored by `jd pull`
        // (<root>/.bombshell/jd/lib/…); machine-global files live under
        // ~/.bombshell/jd. First readable wins.
        let bases: Vec<std::path::PathBuf> = match row.source {
            Source::Global => Config::home_cache_dir().into_iter().collect(),
            _ => vec![cfg.root.clone(), cfg.cache_dir()],
        };
        match bases
            .iter()
            .find_map(|b| std::fs::read_to_string(b.join(&row.path)).ok())
        {
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
        let url = format!("{}/{}", online_base(cfg, row), row.path);
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
            let mut out = String::new();
            out.push_str(&format!(
                "{{\"schema\":\"justdown.get/1\",\"ref\":{},\"sections\":[",
                json_str(&refr)
            ));
            for (i, (kind, content)) in sections.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{{\"kind\":{},\"content\":{}}}",
                    json_str(kind),
                    json_str(content)
                ));
            }
            out.push_str("]}");
            println!("{out}");
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

/// The platform tokens the runner selects on. `darwin` is accepted as an input
/// alias for `macos` but is not itself a host token.
pub(crate) const PLATFORMS: &[&str] = &["unix", "macos", "windows", "wsl"];

/// Collect the raw lines inside every ```` ```just ```` fence in a .jd body,
/// with NO platform filtering — the unresolved variants. `lint` walks these per
/// platform to check that selection yields a servable (non-duplicated) justfile.
pub(crate) fn raw_tools_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut injust = false;
    for line in body.lines() {
        if line.starts_with("```just") {
            injust = true;
            continue;
        }
        if injust && line.starts_with("```") {
            injust = false;
            continue;
        }
        if injust {
            out.push(line.to_string());
        }
    }
    out
}

/// The host platform token used to select `[os]` recipe variants:
/// `unix` | `macos` | `windows` | `wsl`. `JD_PLATFORM`, if set, wins (test/CI
/// seam). Otherwise inferred from the OS, with Linux refined to `wsl` when
/// running under WSL (`/proc/version` mentions microsoft, or `$WSL_DISTRO_NAME`).
fn host_platform() -> String {
    if let Ok(p) = std::env::var("JD_PLATFORM") {
        if !p.is_empty() {
            return p;
        }
    }
    match std::env::consts::OS {
        "macos" => "macos".to_string(),
        "windows" => "windows".to_string(),
        "linux" => {
            let wsl = std::env::var("WSL_DISTRO_NAME")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
                || std::fs::read_to_string("/proc/version")
                    .map(|s| s.to_lowercase().contains("microsoft"))
                    .unwrap_or(false);
            if wsl {
                "wsl".to_string()
            } else {
                "unix".to_string()
            }
        }
        _ => "unix".to_string(),
    }
}

/// Parse a platform-attribute line like `[unix, wsl]` into its tags. Returns
/// `None` for any line that is not exclusively platform tags — so non-platform
/// just attributes (`[private]`, `[confirm]`, …) pass through untouched.
pub(crate) fn parse_platform_attr(line: &str) -> Option<Vec<String>> {
    let s = line.trim();
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    let mut tags = Vec::new();
    for part in inner.split(',') {
        match part.trim() {
            t @ ("unix" | "macos" | "darwin" | "windows" | "wsl") => tags.push(t.to_string()),
            _ => return None,
        }
    }
    if tags.is_empty() {
        None
    } else {
        Some(tags)
    }
}

#[cfg(test)]
mod platform_tests {
    use super::{parse_platform_attr, platsel};

    fn sel(src: &str, plat: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        platsel(&lines, plat).join("\n")
    }

    #[test]
    fn picks_one_variant_per_host_and_strips_attrs() {
        let src = "[unix]\nopen t:\n  xdg-open {{t}}\n[macos]\nopen t:\n  open {{t}}\n[windows]\nopen t:\n  start {{t}}\n[wsl]\nopen t:\n  wslview {{t}}";
        assert_eq!(sel(src, "unix"), "open t:\n  xdg-open {{t}}");
        assert_eq!(sel(src, "macos"), "open t:\n  open {{t}}");
        assert_eq!(sel(src, "windows"), "open t:\n  start {{t}}");
        assert_eq!(sel(src, "wsl"), "open t:\n  wslview {{t}}");
    }

    #[test]
    fn comma_list_and_darwin_alias() {
        let src = "[unix, wsl]\nr:\n  a\n[macos]\nr:\n  b";
        assert_eq!(sel(src, "unix"), "r:\n  a");
        assert_eq!(sel(src, "wsl"), "r:\n  a");
        assert_eq!(sel(src, "macos"), "r:\n  b");
        let darwin = "[darwin]\nr:\n  mac";
        assert_eq!(sel(darwin, "macos"), "r:\n  mac");
        assert_eq!(sel(darwin, "unix"), "");
    }

    #[test]
    fn untagged_and_nonplatform_attrs_pass_through() {
        // a leading comment + untagged recipe always survive
        let src = "# desc\nr:\n  body\n[unix]\nr2:\n  ux";
        assert_eq!(sel(src, "macos"), "# desc\nr:\n  body");
        // non-platform just attributes are not platform attrs → untouched
        assert_eq!(parse_platform_attr("[private]"), None);
        assert_eq!(parse_platform_attr("[confirm: \"sure?\"]"), None);
        let keep = "[private]\nr:\n  body";
        assert_eq!(sel(keep, "unix"), "[private]\nr:\n  body");
    }

    #[test]
    fn parses_tag_lists() {
        assert_eq!(
            parse_platform_attr("[unix]"),
            Some(vec!["unix".to_string()])
        );
        assert_eq!(
            parse_platform_attr("[ unix , wsl ]"),
            Some(vec!["unix".to_string(), "wsl".to_string()])
        );
        assert_eq!(parse_platform_attr("not an attr"), None);
        assert_eq!(parse_platform_attr("[unix, bogus]"), None);
    }
}

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

/// Select the recipe variants matching `plat` and strip the attribute lines.
/// A `[os]` attr guards the recipe header that follows it and that recipe's
/// indented body; untagged lines always pass. `darwin` is an alias for `macos`.
/// Authors keep same-named variants mutually exclusive per platform, so exactly
/// one definition of each recipe survives. Mirrors the awk `platsel`.
pub(crate) fn platsel(lines: &[&str], plat: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut pend = false; // previous line was a platform attr; next is the header
    let mut guarded = false; // inside a guarded recipe's body
    let mut keep = true; // emit the current guarded block?
    for &line in lines {
        if let Some(tags) = parse_platform_attr(line) {
            keep = tags
                .iter()
                .any(|t| (if t == "darwin" { "macos" } else { t.as_str() }) == plat);
            pend = true;
            guarded = false;
            continue;
        }
        if pend {
            pend = false;
            guarded = true;
            if keep {
                out.push(line.to_string());
            }
            continue;
        }
        if guarded {
            if line.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
                if keep {
                    out.push(line.to_string());
                }
                continue;
            }
            guarded = false;
            keep = true;
            // not indented → end of guarded body; fall through to emit normally
        }
        out.push(line.to_string());
    }
    out
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
            let mut out = String::from("{\"schema\":\"justdown.ls/1\",\"categories\":[");
            for (i, c) in order.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let ms = &members[c];
                let arr: Vec<String> = ms.iter().map(|m| json_str(m)).collect();
                out.push_str(&format!(
                    "{{\"name\":{},\"members\":[{}]}}",
                    json_str(c),
                    arr.join(",")
                ));
            }
            out.push_str("]}");
            println!("{out}");
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

    let mut needle = refr.clone();
    if let Some(s) = needle.strip_prefix('@') {
        needle = s.to_string();
    }
    if let Some(i) = needle.find('#') {
        needle.truncate(i);
    }

    let target = rows.iter().find(|r| {
        r.name == needle || r.key == needle || r.path == needle || basename(&r.path) == needle
    });
    let target = match target {
        Some(r) => r,
        None => {
            emit_err(cfg, "not-found", &format!("no file: {refr}"));
            return 2;
        }
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

    match cfg.format {
        Format::Json => {
            let o: Vec<String> = outbound.iter().map(|s| json_str(s)).collect();
            let i: Vec<String> = inbound.iter().map(|s| json_str(s)).collect();
            println!(
                "{{\"schema\":\"justdown.links/1\",\"ref\":{},\"key\":{},\"outbound\":[{}],\"inbound\":[{}]}}",
                json_str(&refr),
                json_str(key),
                o.join(","),
                i.join(",")
            );
        }
        Format::Text => {
            for o in &outbound {
                println!("out  @{o}");
            }
            for i in &inbound {
                println!("in   {i}  (@{key})");
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------
// path — shortest connection between two tools through the @link graph
// ---------------------------------------------------------------------------

/// Resolve a ref (name | key | path | basename, with optional `@`/`#section`)
/// to a row.
fn resolve<'a>(rows: &'a [Row], refr: &str) -> Option<&'a Row> {
    let mut needle = refr.to_string();
    if let Some(s) = needle.strip_prefix('@') {
        needle = s.to_string();
    }
    if let Some(i) = needle.find('#') {
        needle.truncate(i);
    }
    rows.iter().find(|r| {
        r.name == needle || r.key == needle || r.path == needle || basename(&r.path) == needle
    })
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

    let src = match resolve(&rows, &a) {
        Some(r) => r.key.clone(),
        None => {
            emit_err(cfg, "not-found", &format!("no file: {a}"));
            return 2;
        }
    };
    let dst = match resolve(&rows, &b) {
        Some(r) => r.key.clone(),
        None => {
            emit_err(cfg, "not-found", &format!("no file: {b}"));
            return 2;
        }
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
            let (arr, len) = match &chain {
                Some(p) => (
                    p.iter().map(|k| json_str(k)).collect::<Vec<_>>().join(","),
                    p.len() as i64 - 1,
                ),
                None => (String::new(), -1),
            };
            println!(
                "{{\"schema\":\"justdown.path/1\",\"from\":{},\"to\":{},\"path\":[{}],\"length\":{}}}",
                json_str(&src),
                json_str(&dst),
                arr,
                len
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
