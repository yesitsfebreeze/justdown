//! `@link` token model: classification + name resolution, shared by the store
//! (edge build), `lint` (resolution check), and the `resolve` endpoint, so the
//! three never drift on what a token means or what it points at.
//!
//! `scan_links` emits a token per link in one of three forms, distinguished here:
//!
//! - `dir/name` — a fully-qualified **key** (`@dir/name`). The legacy form.
//! - `name` — a single-segment **direct** link (`@name`). Resolves to a unique
//!   key via the corpus [`NameIndex`]; lint-warns when it can't.
//! - `?term` — a **fuzzy** link (`@?term`). Ranked live by the search ranker,
//!   one-to-many, re-resolved on read; the source text keeps `@?term` verbatim.

use std::collections::HashMap;

/// Marker prefix `scan_links` emits for a fuzzy `@?term` link.
pub const FUZZY: char = '?';

/// A parsed link token, by form.
#[derive(Debug, PartialEq, Eq)]
pub enum Link<'a> {
    /// `@dir/name` — a fully-qualified key.
    Key(&'a str),
    /// `@name` — a single leaf, resolved against the corpus name index.
    Name(&'a str),
    /// `@?term` — fuzzy, ranked live (one-to-many).
    Fuzzy(&'a str),
}

/// Classify a stored link token (as `scan_links` emits it).
pub fn classify(token: &str) -> Link<'_> {
    if let Some(t) = token.strip_prefix(FUZZY) {
        Link::Fuzzy(t)
    } else if token.contains('/') {
        Link::Key(token)
    } else {
        Link::Name(token)
    }
}

/// The leaf of a key — the segment after the last `/` (the bare file name).
pub fn leaf(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

/// The outcome of resolving a single-segment `@name` against the corpus.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolve {
    /// Exactly one node matched — its key.
    Unique(String),
    /// Nothing matched.
    None,
    /// More than one distinct node matched — every candidate key.
    Ambiguous(Vec<String>),
}

/// A corpus index from identifier (key, frontmatter `name`, or leaf) → node
/// keys, so a bare `@name` can resolve to its canonical `dir/name`. Built once
/// from the whole node set; a single name addresses a unique node only when it
/// maps to exactly one key.
pub struct NameIndex {
    map: HashMap<String, Vec<String>>,
}

impl NameIndex {
    /// Build from `(key, name)` pairs. Registers each node under its key, its
    /// frontmatter name, and its leaf, so any of the three resolves to it.
    pub fn build<'a>(nodes: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let mut add = |ident: &str, key: &str| {
            let v = map.entry(ident.to_string()).or_default();
            if !v.iter().any(|k| k == key) {
                v.push(key.to_string());
            }
        };
        for (key, name) in nodes {
            add(key, key);
            if !name.is_empty() {
                add(name, key);
            }
            add(leaf(key), key);
        }
        NameIndex { map }
    }

    /// Resolve a single-segment `@name` token to a unique key, refusing to guess
    /// when zero or many nodes match.
    pub fn resolve(&self, token: &str) -> Resolve {
        match self.map.get(token).map(|v| v.as_slice()) {
            Some([k]) => Resolve::Unique(k.clone()),
            Some(many) if many.len() > 1 => Resolve::Ambiguous(many.to_vec()),
            _ => Resolve::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_distinguishes_the_three_forms() {
        assert_eq!(classify("dir/name"), Link::Key("dir/name"));
        assert_eq!(classify("glassmorphism"), Link::Name("glassmorphism"));
        assert_eq!(classify("?soft"), Link::Fuzzy("soft"));
    }

    #[test]
    fn leaf_is_last_segment() {
        assert_eq!(leaf("soft-ui/glassmorphism"), "glassmorphism");
        assert_eq!(leaf("glassmorphism"), "glassmorphism");
    }

    #[test]
    fn name_resolves_via_key_name_or_leaf() {
        let idx = NameIndex::build([
            ("soft-ui/glassmorphism", "glass"),
            ("vcs/gh/release", "gh_release"),
        ]);
        // by leaf
        assert_eq!(
            idx.resolve("glassmorphism"),
            Resolve::Unique("soft-ui/glassmorphism".into())
        );
        // by frontmatter name
        assert_eq!(
            idx.resolve("glass"),
            Resolve::Unique("soft-ui/glassmorphism".into())
        );
        // by full key
        assert_eq!(
            idx.resolve("vcs/gh/release"),
            Resolve::Unique("vcs/gh/release".into())
        );
        assert_eq!(idx.resolve("nope"), Resolve::None);
    }

    #[test]
    fn collide_on_leaf_is_ambiguous() {
        let idx = NameIndex::build([("meta/release", "a"), ("gh/release", "b")]);
        match idx.resolve("release") {
            Resolve::Ambiguous(keys) => {
                assert_eq!(keys.len(), 2);
                assert!(keys.contains(&"meta/release".to_string()));
                assert!(keys.contains(&"gh/release".to_string()));
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }
}
