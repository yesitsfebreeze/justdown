// The .jd parser and node model. Mirrors the original awk in `just build`:
// frontmatter is ingested as the retrieval contract, the body is scanned for
// @links, and the key/category are derived from the path. A node plus its link
// edges is one row in the graph.

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

/// Strip the `key:` prefix, neutralize tabs, trim trailing whitespace/CR.
/// Mirrors awk `val()`.
fn val(s: &str) -> String {
    let after = match s.find(':') {
        Some(i) => &s[i + 1..],
        None => s,
    };
    after
        .trim_start_matches([' ', '\t'])
        .replace('\t', " ")
        .trim_end_matches([' ', '\r'])
        .to_string()
}

/// Parse an inline YAML array `key: [a, b, c]` into `["a","b","c"]`. Mirrors
/// awk `arr()`: take between the first `[` and the next `]`, drop whitespace,
/// split on commas. Only the inline bracket form is supported (as in the awk).
fn arr(s: &str) -> Vec<String> {
    let open = match s.find('[') {
        Some(i) => i + 1,
        None => return Vec::new(),
    };
    let inner = &s[open..];
    let inner = match inner.find(']') {
        Some(i) => &inner[..i],
        None => inner,
    };
    inner
        .split(',')
        .map(|t| t.replace([' ', '\t'], ""))
        .filter(|t| !t.is_empty())
        .collect()
}

/// Scan text for `@dir/name` links: `@[a-z0-9_]+/[a-z0-9_]+`. Returns the key
/// form (`dir/name`, no `@`, `#section` not captured), deduped in first-seen
/// order. Mirrors the awk body regex.
fn scan_links(body: &str, out: &mut Vec<String>) {
    let b = body.as_bytes();
    let is_word = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'@' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let s1 = j;
        while j < b.len() && is_word(b[j]) {
            j += 1;
        }
        if j == s1 || j >= b.len() || b[j] != b'/' {
            i += 1;
            continue;
        }
        j += 1; // consume '/'
        let s2 = j;
        while j < b.len() && is_word(b[j]) {
            j += 1;
        }
        if j == s2 {
            i += 1;
            continue;
        }
        // key form: between the @ and the end of the second segment, minus '@'
        let key = &body[i + 1..j];
        if !out.iter().any(|k| k == key) {
            out.push(key.to_string());
        }
        i = j;
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
/// dir), `content` is the file body.
pub fn parse(rel: &str, content: &str) -> Node {
    let (key, category) = key_and_category(rel);

    let mut name = String::new();
    let mut kind = String::new();
    let mut description = String::new();
    let mut tags = Vec::new();
    let mut use_when = Vec::new();
    let mut not_when = Vec::new();
    let mut danger = String::new();
    let mut side_effects = Vec::new();
    let mut requires = Vec::new();
    let mut run = String::new();
    let mut links = Vec::new();

    // fm: 0 = before, 1 = inside frontmatter, 2 = after (body)
    let mut fm = 0;
    for (idx, line) in content.lines().enumerate() {
        if idx == 0 && line == "---" {
            fm = 1;
            continue;
        }
        if fm == 1 && line == "---" {
            fm = 2;
            continue;
        }
        if fm == 1 {
            if let Some(rest) = line.strip_prefix("name:") {
                name = val(&format!("name:{rest}"));
            } else if line.starts_with("kind:") {
                kind = val(line);
            } else if line.starts_with("description:") {
                description = val(line);
            } else if line.starts_with("tags:") {
                tags = arr(line);
            } else if line.starts_with("use_when:") {
                use_when = arr(line);
            } else if line.starts_with("not_when:") {
                not_when = arr(line);
            } else if line.starts_with("danger:") {
                danger = val(line);
            } else if line.starts_with("side_effects:") {
                side_effects = arr(line);
            } else if line.starts_with("requires:") {
                requires = arr(line);
            } else if line.starts_with("run:") {
                run = val(line);
            }
        } else if fm == 2 {
            scan_links(line, &mut links);
        }
    }

    let name_given = !name.is_empty();
    if name.is_empty() {
        name = key.clone();
    }
    let purpose = if !description.is_empty() {
        description.clone()
    } else {
        name.clone()
    };

    Node {
        key,
        name,
        kind,
        description,
        purpose,
        tags,
        path: rel.to_string(),
        use_when,
        not_when,
        danger,
        side_effects,
        requires,
        category,
        run,
        has_frontmatter: fm >= 2,
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
    fn name_falls_back_to_key() {
        let n = parse("library/x/foo.jd", "---\nkind: tool\n---\nbody\n");
        assert_eq!(n.name, "x/foo");
        assert_eq!(n.purpose, "x/foo");
    }
}
