//! Field-weighted, graph-aware ranking over store rows.
//!
//! The scoring the original justfile awk did, as a library function so both the
//! `jd` CLI (`search`, and its semantic-mode variant) and an embedding host
//! (bombshell's in-process tool discovery) rank the same way. Pure over
//! [`Row`]: name/use_when (3) > tags (2) > purpose (1), `not_when` vetoes, ties
//! broken by graph connectivity then name.

use crate::store::Row;
use std::collections::HashMap;

/// Query words too generic to carry signal; dropped before scoring.
pub const STOPWORDS: &[&str] = &[
    "a", "an", "and", "or", "the", "of", "to", "in", "on", "at", "is", "it", "its", "be", "as",
    "do", "for", "my", "our", "your", "this", "that", "with", "from", "by",
];

/// Split on runs of characters that are not [a-z0-9+] (lowercase assumed by
/// caller). Mirrors the awk `split(s, w, /[^a-z0-9+]+/)`.
pub fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '+'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// A term hits a field if any whole token in the field contains it. Mirrors
/// awk `fhit`.
fn fhit(field: &str, term: &str) -> bool {
    words(field).iter().any(|w| w.contains(term))
}

/// Inbound+outbound @link degree per node key — the graph-connectivity signal.
/// A tool that composes (or is composed by) many others is more central.
pub fn degree_map(rows: &[Row]) -> HashMap<String, usize> {
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    for row in rows {
        for l in &row.links {
            *indeg.entry(l.as_str()).or_insert(0) += 1;
        }
    }
    let mut deg = HashMap::new();
    for row in rows {
        let d = row.links.len() + indeg.get(row.key.as_str()).copied().unwrap_or(0);
        deg.insert(row.key.clone(), d);
    }
    deg
}

/// A scored row: the relevance `score` and a borrow of the matched [`Row`].
pub struct Scored<'a> {
    pub score: i64,
    pub row: &'a Row,
}

/// The explorer's query split: lowercased whitespace terms, all of which must
/// match (see [`match_name_content`]). No stopword filtering — a term the user
/// typed is a term that must hit.
pub fn search_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Subsequence test: are `needle`'s chars present, in order, within `hay`?
/// `needle` is already lowercase; `hay` is a lowercased char slice.
pub fn subsequence(hay: &[char], needle: &str) -> bool {
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

/// The explorer's name+content match, shared by `jd explore` and `jd search`:
/// every term must match either `label` (fuzzy subsequence, case-insensitive)
/// or `raw` content (case-insensitive substring). Returns whether the file
/// matched and, for a content hit, the first matching line (trimmed, capped at
/// 120 chars) as a snippet.
pub fn match_name_content(label: &str, raw: &str, terms: &[String]) -> (bool, Option<String>) {
    let label: Vec<char> = label.to_lowercase().chars().collect();
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
        (true, snippet)
    } else {
        (false, None)
    }
}

/// Direct-link completion: rows whose key, name, or leaf matches `prefix`,
/// ranked by match quality (exact > prefix > substring), then graph
/// connectivity, then name. This is the `@name` autocomplete source — distinct
/// from [`rank`] (the field-weighted ranker that powers `@?` fuzzy links). An
/// empty prefix returns every row (degree-then-name ordered).
pub fn resolve_prefix<'a>(rows: &'a [Row], prefix: &str) -> Vec<&'a Row> {
    let p = prefix.to_lowercase();
    let deg = degree_map(rows);
    let dg = |k: &str| deg.get(k).copied().unwrap_or(0);

    // 0 = exact, 1 = prefix, 2 = substring; lower is better.
    let quality = |row: &Row| -> Option<u8> {
        if p.is_empty() {
            return Some(3);
        }
        let leaf = crate::links::leaf(&row.key).to_lowercase();
        let fields = [row.key.to_lowercase(), row.name.to_lowercase(), leaf];
        let mut best: Option<u8> = None;
        for f in &fields {
            let q = if *f == p {
                0
            } else if f.starts_with(&p) {
                1
            } else if f.contains(&p) {
                2
            } else {
                continue;
            };
            best = Some(best.map_or(q, |b| b.min(q)));
        }
        best
    };

    let mut hits: Vec<(u8, &Row)> = rows
        .iter()
        .filter_map(|r| quality(r).map(|q| (q, r)))
        .collect();
    hits.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| dg(&b.1.key).cmp(&dg(&a.1.key)))
            .then_with(|| a.1.name.cmp(&b.1.name))
    });
    hits.into_iter().map(|(_, r)| r).collect()
}

/// Field-weighted ranking. Filters by `kind` / `category` (empty = no filter),
/// applies the `not_when` veto, scores name/use_when (3) > tags (2) > purpose
/// (1). Sorts score-desc, then by graph connectivity (a well-connected tool
/// outranks an isolated one on a tie), then name-asc as the deterministic
/// final tie-break.
pub fn rank<'a>(rows: &'a [Row], query: &str, kind: &str, category: &str) -> Vec<Scored<'a>> {
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
            if fhit(&name, t) || fhit(&usew, t) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Source;

    fn row(
        key: &str,
        name: &str,
        kind: &str,
        use_when: &str,
        not_when: &str,
        purpose: &str,
    ) -> Row {
        Row {
            source: Source::Local,
            origin: String::new(),
            key: key.into(),
            name: name.into(),
            kind: kind.into(),
            description: purpose.into(),
            purpose: purpose.into(),
            tags: String::new(),
            path: String::new(),
            use_when: use_when.into(),
            not_when: not_when.into(),
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

    #[test]
    fn resolve_prefix_ranks_exact_then_prefix_then_substring() {
        let rows = vec![
            row("soft-ui/glass", "glass", "tool", "", "", ""),
            row("ui/glassmorphism", "glassmorphism", "tool", "", "", ""),
            row("x/subglass", "subglass", "tool", "", "", ""),
        ];
        let out = resolve_prefix(&rows, "glass");
        let keys: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
        // exact leaf `glass` first, then prefix `glassmorphism`, then substring `subglass`.
        assert_eq!(keys, vec!["soft-ui/glass", "ui/glassmorphism", "x/subglass"]);
    }

    #[test]
    fn name_outscores_purpose_and_not_when_vetoes() {
        let rows = vec![
            row(
                "search/rg",
                "search_rg",
                "tool",
                "grep the repo",
                "",
                "ripgrep search",
            ),
            row(
                "web/fetch",
                "web_fetch",
                "tool",
                "download a url",
                "do not grep",
                "fetch a web page",
            ),
        ];
        let out = rank(&rows, "grep search", "tool", "");
        // rg matches name+use_when strongly; web_fetch is vetoed by not_when "grep".
        assert_eq!(out.len(), 1, "not_when veto drops web_fetch");
        assert_eq!(out[0].row.name, "search_rg");
        assert!(out[0].score >= 3);
    }

    #[test]
    fn fuzzy_name_subsequence_matches() {
        let terms = search_terms("relse");
        let (hit, snippet) = match_name_content("meta/tools/release.jd", "", &terms);
        assert!(hit, "subsequence 'relse' must hit 'release'");
        assert_eq!(snippet, None, "a name-only hit carries no snippet");
        let (miss, _) = match_name_content("meta/tools/gate.jd", "", &terms);
        assert!(!miss);
    }

    #[test]
    fn content_substring_matches_and_snippets() {
        let raw = "---\nname: rg\n---\n\nUse Vim keys to navigate results.\n";
        let terms = search_terms("VIM");
        let (hit, snippet) = match_name_content("search/rg.jd", raw, &terms);
        assert!(hit, "content match is case-insensitive");
        assert_eq!(snippet.as_deref(), Some("Use Vim keys to navigate results."));
    }

    #[test]
    fn every_term_must_match_name_or_content() {
        let raw = "body mentions vim here";
        let terms = search_terms("vim rg");
        // 'rg' hits the name, 'vim' hits the content → match
        let (hit, _) = match_name_content("search/rg.jd", raw, &terms);
        assert!(hit);
        // 'zzz' hits nothing → the whole query misses
        let terms = search_terms("vim zzz");
        let (miss, snippet) = match_name_content("search/rg.jd", raw, &terms);
        assert!(!miss);
        assert_eq!(snippet, None);
    }

    #[test]
    fn kind_filter_excludes_non_tools() {
        let rows = vec![
            row("x/tool", "a_tool", "tool", "do x", "", "does x"),
            row("x/note", "a_note", "knowledge", "do x", "", "about x"),
        ];
        let out = rank(&rows, "x", "tool", "");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].row.kind, "tool");
    }
}
