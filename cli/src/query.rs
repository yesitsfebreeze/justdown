// The query surface: search / get / ls / links. A faithful port of the
// original justfile awk — same field-weighted scoring, not_when veto, kind &
// category narrowing, local⊕online merge (local shadows online by key),
// degrade-not-fail, text + JSON output, and exit codes (0/2/3/4).

use crate::config::{Config, Format};
use crate::store::{Row, Source, Store};

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "or", "the", "of", "to", "in", "on", "at", "is", "it", "its", "be", "as",
    "do", "for", "my", "our", "your", "this", "that", "with", "from", "by",
];

/// Split on runs of characters that are not [a-z0-9+] (lowercase assumed by
/// caller). Mirrors the awk `split(s, w, /[^a-z0-9+]+/)`.
fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '+'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// A term hits a field if any whole token in the field contains it. Mirrors
/// awk `fhit`.
fn fhit(field: &str, term: &str) -> bool {
    words(field).iter().any(|w| w.contains(term))
}

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

/// Download the online store to a temp file and load its rows. Best-effort:
/// any failure (unreachable, 404, unreadable) yields None so callers degrade.
fn fetch_online(cfg: &Config) -> Option<Vec<Row>> {
    let url = format!("{}/{}", cfg.raw_base, cfg.index);
    let tmp = std::env::temp_dir().join(format!("jd-online-{}.db", std::process::id()));
    if !curl_to_file(&url, &tmp) {
        return None;
    }
    let rows = Store::open(&tmp).ok().and_then(|s| s.load_rows(Source::Online).ok());
    let _ = std::fs::remove_file(&tmp);
    rows
}

/// Gather the merged, deduped row set. Local shadows online by key. On the
/// degrade path (no online) a note goes to stderr; only a total absence of
/// sources is a hard error (exit 4).
fn gather(cfg: &Config) -> Result<Vec<Row>, i32> {
    let local: Option<Vec<Row>> = if cfg.index_path().exists() {
        Store::open(&cfg.index_path())
            .ok()
            .and_then(|s| s.load_rows(Source::Local).ok())
    } else {
        None
    };
    let online = fetch_online(cfg);

    if local.is_none() && online.is_none() {
        emit_err(
            cfg,
            "source-unreachable",
            &format!(
                "no local store and online store unreachable ({}/{})",
                cfg.raw_base, cfg.index
            ),
        );
        return Err(4);
    }
    if online.is_none() && local.is_some() {
        eprintln!("jd: note: online store unreachable; using local only");
    }

    // local first, then online; dedup by key keeps the local entry.
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in local.into_iter().flatten().chain(online.into_iter().flatten()) {
        if seen.insert(row.key.clone()) {
            out.push(row);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

struct Scored<'a> {
    score: i64,
    row: &'a Row,
}

/// Inbound+outbound @link degree per node key — the graph-connectivity signal.
/// A tool that composes (or is composed by) many others is more central.
fn degree_map(rows: &[Row]) -> std::collections::HashMap<String, usize> {
    let mut indeg: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for row in rows {
        for l in &row.links {
            *indeg.entry(l.as_str()).or_insert(0) += 1;
        }
    }
    let mut deg = std::collections::HashMap::new();
    for row in rows {
        let d = row.links.len() + indeg.get(row.key.as_str()).copied().unwrap_or(0);
        deg.insert(row.key.clone(), d);
    }
    deg
}

/// Field-weighted ranking, used by `search`. Filters by kind /
/// category, applies the not_when veto, scores name/use_when (3) > tags (2) >
/// purpose (1). Sorts score-desc, then by graph connectivity (a well-connected
/// tool outranks an isolated one on a tie — the smart-graph signal), then
/// name-asc as the final deterministic tie-break.
fn rank<'a>(rows: &'a [Row], query: &str, kind: &str, category: &str) -> Vec<Scored<'a>> {
    let q = query.to_lowercase();
    let terms: Vec<String> = words(&q)
        .into_iter()
        .filter(|t| !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect();

    let deg = degree_map(rows);
    let mut scored: Vec<Scored> = Vec::new();
    for row in rows {
        if !kind.is_empty() && row.kind != kind {
            continue;
        }
        if !category.is_empty() && row.category != category {
            continue;
        }
        let name = row.name.to_lowercase();
        let purpose = row.purpose.to_lowercase();
        let tags = row.tags.to_lowercase();
        let usew = row.use_when.to_lowercase();
        let notw = row.not_when.to_lowercase();

        let mut score = 0i64;
        let mut vetoed = false;
        for t in &terms {
            if !notw.is_empty() && fhit(&notw, t) {
                vetoed = true;
                break;
            }
            if fhit(&name, t) {
                score += 3;
            } else if fhit(&usew, t) {
                score += 3;
            } else if fhit(&tags, t) {
                score += 2;
            } else if fhit(&purpose, t) {
                score += 1;
            }
        }
        if vetoed || score <= 0 {
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
    if !Config::format_valid() {
        emit_err(cfg, "bad-args", "unknown JUSTDOWN_FORMAT (want text|json)");
        return 3;
    }
    let query = match args.first() {
        Some(q) if !q.is_empty() => q.clone(),
        _ => {
            emit_err(cfg, "bad-args", "search needs a query");
            return 3;
        }
    };
    let kind = args.get(1).cloned().unwrap_or_default();
    let num_s = args.get(2).cloned().unwrap_or_else(|| "5".to_string());
    let category = args.get(3).cloned().unwrap_or_default();

    if !kind.is_empty() && !matches!(kind.as_str(), "tool" | "agent" | "knowledge" | "workflow") {
        emit_err(cfg, "bad-args", &format!("unknown kind: {kind} (want tool|agent|knowledge|workflow)"));
        return 3;
    }
    if num_s.is_empty() || !num_s.bytes().all(|b| b.is_ascii_digit()) {
        emit_err(cfg, "bad-args", &format!("num must be a positive integer: {num_s}"));
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

    let scored = rank(&rows, &query, &kind, &category);

    let take = scored.len().min(num as usize);
    let shown = &scored[..take];

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
                    format!("{}/{}", cfg.raw_base, r.path)
                };
                let danger = if r.danger.is_empty() { "none" } else { &r.danger };
                out.push_str(&format!(
                    "{{\"name\":{},\"kind\":{},\"score\":{},\"purpose\":{},\"raw\":{},\"source\":{},\"danger\":{},\"side_effects\":{},\"requires\":{}}}",
                    json_str(&r.name),
                    json_str(&r.kind),
                    s.score,
                    json_str(&r.purpose),
                    json_str(&raw),
                    json_str(if r.source.is_local() { "local" } else { "online" }),
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
                    format!("{}/{}", cfg.raw_base, r.path)
                };
                if r.source.is_local() {
                    raw.push_str(" (local)");
                }
                println!("{}. {}  [{}]  score {}\n   {}\n   {}", i + 1, r.name, r.kind, s.score, r.purpose, raw);
                // surface safety only when it matters
                if r.danger == "high" || r.danger == "medium" || !r.side_effects.is_empty() {
                    let mut line = format!("   ⚠ danger={}", if r.danger.is_empty() { "none" } else { &r.danger });
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

    if shown.is_empty() {
        2
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// get
// ---------------------------------------------------------------------------

fn basename(path: &str) -> String {
    let p = path.strip_suffix(".jd").unwrap_or(path);
    p.rsplit('/').next().unwrap_or(p).to_string()
}

pub fn get(cfg: &Config, args: &[String]) -> i32 {
    if !Config::format_valid() {
        emit_err(cfg, "bad-args", "unknown JUSTDOWN_FORMAT (want text|json)");
        return 3;
    }
    let refr = match args.first() {
        Some(r) if !r.is_empty() => r.clone(),
        _ => {
            emit_err(cfg, "bad-args", "get needs a ref");
            return 3;
        }
    };
    let only = args.get(1).cloned().unwrap_or_default();
    if !matches!(only.as_str(), "" | "frontmatter" | "prose" | "tools") {
        emit_err(cfg, "bad-args", &format!("unknown only: {only} (want frontmatter|prose|tools)"));
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
        emit_err(cfg, "bad-args", &format!("refusing suspicious path: {}", row.path));
        return 3;
    }

    let body = if row.source.is_local() {
        match std::fs::read_to_string(cfg.root.join(&row.path)) {
            Ok(b) => b,
            Err(_) => {
                emit_err(cfg, "source-unreachable", &format!("cannot read local file: {}", row.path));
                return 4;
            }
        }
    } else {
        let url = format!("{}/{}", cfg.raw_base, row.path);
        match curl_to_string(&url) {
            Some(b) => b,
            None => {
                emit_err(cfg, "source-unreachable", &format!("cannot fetch: {url}"));
                return 4;
            }
        }
    };

    let sections = split_sections(&body, &only);
    match cfg.format {
        Format::Json => {
            let mut out = String::new();
            out.push_str(&format!("{{\"schema\":\"justdown.get/1\",\"ref\":{},\"sections\":[", json_str(&refr)));
            for (i, (kind, content)) in sections.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!("{{\"kind\":{},\"content\":{}}}", json_str(kind), json_str(content)));
            }
            out.push_str("]}");
            println!("{out}");
        }
        Format::Text => {
            for (kind, content) in &sections {
                println!("# {kind}");
                println!("{content}");
                println!();
            }
        }
    }
    0
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
            let wsl = std::env::var("WSL_DISTRO_NAME").map(|v| !v.is_empty()).unwrap_or(false)
                || std::fs::read_to_string("/proc/version")
                    .map(|s| s.to_lowercase().contains("microsoft"))
                    .unwrap_or(false);
            if wsl { "wsl".to_string() } else { "unix".to_string() }
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
    if tags.is_empty() { None } else { Some(tags) }
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
        assert_eq!(parse_platform_attr("[unix]"), Some(vec!["unix".to_string()]));
        assert_eq!(
            parse_platform_attr("[ unix , wsl ]"),
            Some(vec!["unix".to_string(), "wsl".to_string()])
        );
        assert_eq!(parse_platform_attr("not an attr"), None);
        assert_eq!(parse_platform_attr("[unix, bogus]"), None);
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
            keep = tags.iter().any(|t| (if t == "darwin" { "macos" } else { t.as_str() }) == plat);
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
    if !Config::format_valid() {
        emit_err(cfg, "bad-args", "unknown JUSTDOWN_FORMAT (want text|json)");
        return 3;
    }
    let rows = match gather(cfg) {
        Ok(r) => r,
        Err(c) => return c,
    };

    // group by category, fall back to kind, then "misc"; preserve member order
    let mut order: Vec<String> = Vec::new();
    let mut members: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
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
                out.push_str(&format!("{{\"name\":{},\"members\":[{}]}}", json_str(c), arr.join(",")));
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
    if !Config::format_valid() {
        emit_err(cfg, "bad-args", "unknown JUSTDOWN_FORMAT (want text|json)");
        return 3;
    }
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
    rows.iter()
        .find(|r| r.name == needle || r.key == needle || r.path == needle || basename(&r.path) == needle)
}

/// `jd path <a> <b>` — the shortest chain of @links connecting two files,
/// treating links as undirected (the "best connection between tooling"). BFS
/// over the link graph; neighbours visited in sorted order for determinism.
/// Exit 0 with a path, 2 if the two are unconnected, 2 (with an error) if an
/// endpoint doesn't resolve, 3 on bad args.
pub fn path(cfg: &Config, args: &[String]) -> i32 {
    if !Config::format_valid() {
        emit_err(cfg, "bad-args", "unknown JUSTDOWN_FORMAT (want text|json)");
        return 3;
    }
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
