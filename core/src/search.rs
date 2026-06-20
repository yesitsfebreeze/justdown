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
        }
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
