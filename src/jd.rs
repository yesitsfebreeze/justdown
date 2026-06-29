// The .jd parser and node model. Frontmatter (the `---`-delimited YAML head) is
// the retrieval contract, deserialized with serde via `serde_yaml_ng`; the body
// is scanned for @links; the key/category are derived from the path. A node plus
// its link edges is one row in the graph.

use serde::Deserialize;

pub struct Node {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub purpose: String,
    pub tags: Vec<String>,
    pub path: String, // path relative to root, including the lib dir prefix
    pub use_when: Vec<String>,
    pub not_when: Vec<String>,
    pub danger: String,
    pub side_effects: Vec<String>,
    pub requires: Vec<String>,
    pub category: String,
    pub run: String,
    pub has_frontmatter: bool,
    /// Whether `name:` was actually present (before the key fallback). lint
    /// needs the raw state to flag a missing required field.
    pub name_given: bool,
    /// Link targets in key form (no `@`, `#section` stripped), deduped in order.
    pub links: Vec<String>,
}

/// The frontmatter contract as serde sees it. Unknown keys (e.g. `provides`,
/// `invoke`) are ignored — only what the graph models is captured. `name` is an
/// `Option` so we can tell "absent" from "present" for the `name_given` flag
/// lint relies on. serde handles both inline (`[a, b]`) and block YAML arrays,
/// a superset of what the old hand-rolled `arr()` accepted.
#[derive(Deserialize, Default)]
struct Frontmatter {
    name: Option<String>,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    use_when: Vec<String>,
    #[serde(default)]
    not_when: Vec<String>,
    #[serde(default)]
    danger: String,
    #[serde(default)]
    side_effects: Vec<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    run: String,
}

/// Split a `.jd` into its frontmatter YAML (between the leading `---` fences)
/// and the body that follows. Frontmatter exists only when the very first line
/// is exactly `---` and a later line is exactly `---`; otherwise the whole file
/// is body. Handles `\n` and `\r\n` line endings.
fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let Some(rest) = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))
    else {
        return (None, content);
    };
    let mut idx = 0;
    for line in rest.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        let bare = bare.strip_suffix('\r').unwrap_or(bare);
        if bare == "---" {
            return (Some(&rest[..idx]), &rest[idx + line.len()..]);
        }
        idx += line.len();
    }
    // Unclosed fence: malformed, treat as no frontmatter (degrade, never fail).
    (None, content)
}

/// Scan text for `@link`s in three forms, deduped in first-seen order. The
/// stored token preserves the form so the store/lint/resolve can tell them apart
/// (see [`crate::links`]):
///
/// - `@dir/name` → `dir/name` — a fully-qualified key (the legacy form).
/// - `@name` → `name` — a single-segment direct link (resolved to a key later).
/// - `@?term` → `?term` — a fuzzy link (ranked live; never rewritten in source).
///
/// Segments are `[a-z0-9_-]+`. A trailing `#section` is not captured. Inline
/// code spans (`` `…` ``) are skipped — `@`-links are a prose convention, so a
/// backticked `@daily` (cron), `user@host`, or `@echo` (a just recipe) is not a
/// link. Fenced code blocks are skipped one level up, in [`parse`].
fn scan_links(line: &str, out: &mut Vec<String>) {
    let b = line.as_bytes();
    let is_word =
        |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-';
    let mut in_code = false;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            in_code = !in_code;
            i += 1;
            continue;
        }
        if in_code || b[i] != b'@' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let fuzzy = j < b.len() && b[j] == b'?';
        if fuzzy {
            j += 1;
        }
        let s1 = j;
        while j < b.len() && is_word(b[j]) {
            j += 1;
        }
        if j == s1 {
            i += 1; // bare `@` / `@?` with no word — not a link
            continue;
        }
        // A fuzzy link is single-segment; a direct link may take a `/name` tail.
        let mut end = j;
        if !fuzzy && j < b.len() && b[j] == b'/' {
            let s2 = j + 1;
            let mut k = s2;
            while k < b.len() && is_word(b[k]) {
                k += 1;
            }
            if k > s2 {
                end = k; // `dir/name`
            }
            // `dir/` with an empty second segment degrades to the bare `dir` name
        }
        // Token form: `?term` for fuzzy, else `dir/name` or bare `name`.
        let token = if fuzzy {
            format!("?{}", &line[s1..end])
        } else {
            line[i + 1..end].to_string()
        };
        if !out.contains(&token) {
            out.push(token);
        }
        i = end;
    }
}

/// Derive (key, category) from a root-relative path. Mirrors awk:
/// strip `.jd`, split on `/`; key = parent/file (or just file), category =
/// parent.
pub fn key_and_category(rel: &str) -> (String, String) {
    let p = rel.strip_suffix(".jd").unwrap_or(rel);
    let parts: Vec<&str> = p.split('/').collect();
    let n = parts.len();
    if n >= 2 {
        (
            format!("{}/{}", parts[n - 2], parts[n - 1]),
            parts[n - 2].to_string(),
        )
    } else {
        (parts[n - 1].to_string(), String::new())
    }
}

/// Parse one .jd file. `rel` is the path relative to root (includes the lib
/// dir), `content` is the file body. Never fails: malformed frontmatter
/// degrades to empty fields (lint then flags the missing required ones).
pub fn parse(rel: &str, content: &str) -> Node {
    let (key, category) = key_and_category(rel);

    let (fm_text, body) = split_frontmatter(content);
    let has_frontmatter = fm_text.is_some();
    let fm: Frontmatter = match fm_text {
        Some(t) if !t.trim().is_empty() => serde_yaml_ng::from_str(t).unwrap_or_default(),
        _ => Frontmatter::default(),
    };

    // Scan body prose for @links, skipping fenced code blocks (``` … ```): a
    // recipe / code sample is not link prose (e.g. just's `@echo`, `npm i x@latest`).
    let mut links = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            scan_links(line, &mut links);
        }
    }

    // `name:` counts as "given" only when present and non-blank — matches the
    // old `val()`-then-`is_empty()` behaviour the lint depends on.
    let name_given = fm
        .name
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let name = if name_given {
        fm.name.unwrap()
    } else {
        key.clone()
    };
    let purpose = if !fm.description.is_empty() {
        fm.description.clone()
    } else {
        name.clone()
    };

    Node {
        key,
        name,
        kind: fm.kind,
        description: fm.description,
        purpose,
        tags: fm.tags,
        path: rel.to_string(),
        use_when: fm.use_when,
        not_when: fm.not_when,
        danger: fm.danger,
        side_effects: fm.side_effects,
        requires: fm.requires,
        category,
        run: fm.run,
        has_frontmatter,
        name_given,
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_three_level_path() {
        let (k, c) = key_and_category("library/security/crypto/gpg.jd");
        assert_eq!(k, "crypto/gpg");
        assert_eq!(c, "crypto");
    }

    #[test]
    fn parses_frontmatter_and_links() {
        let src = "---\nname: tools_release\nkind: tool\ndescription: Cut a release.\ntags: [release, publish, ci]\nrun: release\n---\n\nUses @tools/gate and @tools/gate again, plus @cert/openssl.\n";
        let n = parse("library/meta/tools/release.jd", src);
        assert_eq!(n.name, "tools_release");
        assert_eq!(n.kind, "tool");
        assert_eq!(n.key, "tools/release");
        assert_eq!(n.category, "tools");
        assert_eq!(n.run, "release");
        assert_eq!(n.tags, vec!["release", "publish", "ci"]);
        assert_eq!(n.links, vec!["tools/gate", "cert/openssl"]);
        assert!(n.has_frontmatter);
    }

    #[test]
    fn scans_direct_name_and_fuzzy_links() {
        // single-segment @name, fuzzy @?term, and legacy @dir/name coexist,
        // deduped in first-seen order. `@?term` keeps its `?` marker.
        let src = "---\nname: t\nkind: knowledge\ndescription: d\n---\nSee @glassmorphism and @?soft, plus @soft-ui/glass and @glassmorphism again.\n";
        let n = parse("library/x/t.jd", src);
        assert_eq!(n.links, vec!["glassmorphism", "?soft", "soft-ui/glass"]);
    }

    #[test]
    fn bare_at_and_trailing_section_do_not_capture() {
        let n = parse(
            "library/x/t.jd",
            "---\nname: t\nkind: knowledge\ndescription: d\n---\nemail a@ b, and @ alone, and @glass#tips here.\n",
        );
        // `a` (from a@), the bare `@`, are dropped; `@glass#tips` captures `glass`.
        assert_eq!(n.links, vec!["glass"]);
    }

    #[test]
    fn skips_links_in_inline_and_fenced_code() {
        // `@daily` in inline code, `@echo`/`x@latest` in a fence, are not links;
        // only the prose `@glass` and `@a/b` are captured.
        let src = "---\nname: t\nkind: knowledge\ndescription: d\n---\nProse @glass and `@daily` cron.\n\n```just\nr:\n  @echo hi\n  npm i x@latest\n```\n\nMore @a/b here.\n";
        let n = parse("library/x/t.jd", src);
        assert_eq!(n.links, vec!["glass", "a/b"]);
    }

    #[test]
    fn name_falls_back_to_key() {
        let n = parse("library/x/foo.jd", "---\nkind: tool\n---\nbody\n");
        assert_eq!(n.name, "x/foo");
        assert_eq!(n.purpose, "x/foo");
        assert!(!n.name_given);
    }

    #[test]
    fn parses_block_style_arrays() {
        // serde accepts block YAML lists too — a superset of the old inline-only
        // `arr()`. Multi-word entries keep their spaces (the old parser stripped
        // them).
        let src = "---\nname: t\nkind: tool\ntags:\n  - alpha\n  - beta\nuse_when:\n  - go to definition\n  - jump to symbol\n---\nbody\n";
        let n = parse("library/x/t.jd", src);
        assert_eq!(n.tags, vec!["alpha", "beta"]);
        assert_eq!(n.use_when, vec!["go to definition", "jump to symbol"]);
    }

    #[test]
    fn quoted_item_with_flow_char_is_preserved() {
        // A bracket inside a quoted inline-array item must survive (the `ctrl-]`
        // case) instead of closing the flow sequence early.
        let src = "---\nname: t\nkind: tool\nuse_when: [tag stack, \"ctrl-]\", more]\n---\nbody\n";
        let n = parse("library/x/t.jd", src);
        assert_eq!(n.use_when, vec!["tag stack", "ctrl-]", "more"]);
    }

    #[test]
    fn malformed_frontmatter_degrades_without_panicking() {
        // Unparseable YAML (a `: ` inside an unquoted plain scalar) must not
        // panic — it degrades to empty fields so lint can flag the missing
        // required ones. has_frontmatter still reflects that a block was present.
        let src = "---\nname: t\ndescription: a tool: that breaks yaml\n---\nbody @a/b\n";
        let n = parse("library/x/t.jd", src);
        assert!(n.has_frontmatter);
        assert!(!n.name_given);
        assert_eq!(n.name, "x/t"); // fell back to key
        assert_eq!(n.links, vec!["a/b"]); // body still scanned
    }
}
