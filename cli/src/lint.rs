// `jd lint` — validate the local library's .jd frontmatter: required fields,
// kind/danger enums, duplicate name/key, broken @links, tool needs a run
// recipe. A faithful port of the `lint` recipe. Exits 1 on any error
// (CI-gateable); warnings don't fail.
//
// It also validates the platform-variant extension: for a file that uses the
// `[unix]/[macos]/[windows]/[wsl]` convention, resolving any one host must leave
// a servable justfile — i.e. exactly one definition per recipe name. A pair of
// variants that both match a platform would hand `just` a duplicate recipe, so
// that is a lint error.

use crate::config::Config;
use crate::jd::{self, Node};
use crate::query::{self, PLATFORMS};
use std::collections::HashMap;

/// The recipe name a just-block line declares as a header, if any. A header is
/// an unindented `name [params]:` line — not a comment, attribute, assignment
/// (`:=`), or body line. Used to detect duplicate definitions after selection.
fn recipe_name(line: &str) -> Option<String> {
    if line.is_empty() || line.starts_with([' ', '\t', '#', '[']) || line.contains(":=") {
        return None;
    }
    let head = &line[..line.find(':')?];
    let nm = head.split_whitespace().next()?;
    if !nm.is_empty() && nm.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        Some(nm.to_string())
    } else {
        None
    }
}

/// Errors from resolving the platform-variant block of one file. Empty when the
/// file does not use the convention or every host resolves cleanly.
fn platform_errors(body: &str) -> Vec<String> {
    let lines = query::raw_tools_lines(body);
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    // only files that actually use the extension are checked
    if !refs.iter().any(|l| query::parse_platform_attr(l).is_some()) {
        return Vec::new();
    }
    let mut errs = Vec::new();
    for &plat in PLATFORMS {
        let resolved = query::platsel(&refs, plat);
        let mut counts: HashMap<String, usize> = HashMap::new();
        for l in &resolved {
            if let Some(n) = recipe_name(l) {
                *counts.entry(n).or_insert(0) += 1;
            }
        }
        let mut dups: Vec<&String> = counts.iter().filter(|(_, &c)| c > 1).map(|(n, _)| n).collect();
        dups.sort();
        for n in dups {
            errs.push(format!(
                "  error: recipe `{n}` has overlapping platform variants on [{plat}] (would serve a duplicate definition)"
            ));
        }
    }
    errs
}

const KINDS: &[&str] = &["tool", "agent", "knowledge", "workflow"];
const DANGERS: &[&str] = &["none", "low", "medium", "high"];

pub fn run(cfg: &Config) -> i32 {
    let libdir = cfg.lib_dir();
    if !libdir.is_dir() {
        eprintln!("jd: no library dir: {}", libdir.display());
        return 1;
    }

    let mut files = Vec::new();
    crate::build::collect_jd(&libdir, &mut files);
    files.sort_by(|a, b| a.as_os_str().as_encoded_bytes().cmp(b.as_os_str().as_encoded_bytes()));

    let mut nodes: Vec<Node> = Vec::new();
    let mut bodies: Vec<String> = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&cfg.root).unwrap_or(f).to_string_lossy().replace('\\', "/");
        if let Ok(c) = std::fs::read_to_string(f) {
            nodes.push(jd::parse(&rel, &c));
            bodies.push(c);
        }
    }

    // known keys + duplicate counts (raw name, skipping the key fallback)
    let mut keys = std::collections::HashSet::new();
    let mut keycount: HashMap<&str, usize> = HashMap::new();
    let mut namecount: HashMap<&str, usize> = HashMap::new();
    for n in &nodes {
        keys.insert(n.key.as_str());
        *keycount.entry(n.key.as_str()).or_insert(0) += 1;
        if n.name_given {
            *namecount.entry(n.name.as_str()).or_insert(0) += 1;
        }
    }

    let mut errs = 0usize;
    let mut warns = 0usize;
    for (i, n) in nodes.iter().enumerate() {
        let mut msgs: Vec<String> = Vec::new();
        if !n.has_frontmatter {
            msgs.push("  error: no frontmatter block".to_string());
        } else {
            if !n.name_given {
                msgs.push("  error: missing required field: name".to_string());
            }
            if n.description.is_empty() {
                msgs.push("  error: missing required field: description".to_string());
            }
            if n.kind.is_empty() {
                msgs.push("  error: missing required field: kind".to_string());
            } else if !KINDS.contains(&n.kind.as_str()) {
                msgs.push(format!("  error: invalid kind: {} (want tool|agent|knowledge|workflow)", n.kind));
            }
            if n.kind == "tool" && n.run.is_empty() {
                msgs.push("  error: tool has no `run:` recipe".to_string());
            }
            if !n.danger.is_empty() && !DANGERS.contains(&n.danger.as_str()) {
                msgs.push(format!("  error: invalid danger: {} (want none|low|medium|high)", n.danger));
            }
            if n.name_given && namecount.get(n.name.as_str()).copied().unwrap_or(0) > 1 {
                msgs.push(format!("  error: duplicate name: {}", n.name));
            }
            if keycount.get(n.key.as_str()).copied().unwrap_or(0) > 1 {
                msgs.push(format!("  error: duplicate key: {}", n.key));
            }
            for l in &n.links {
                if !keys.contains(l.as_str()) {
                    // knowledge files legitimately reference the user's own
                    // modules; only flag real .jd composition as an error.
                    if n.kind == "knowledge" {
                        msgs.push(format!("  warn: unresolved @link: {l} (external reference?)"));
                    } else {
                        msgs.push(format!("  error: broken @link: {l}"));
                    }
                }
            }
            if (n.kind == "tool" || n.kind == "workflow") && n.use_when.is_empty() {
                msgs.push("  warn: no use_when (retrieval leans on description alone)".to_string());
            }
            msgs.extend(platform_errors(&bodies[i]));
        }
        if !msgs.is_empty() {
            println!("lint: {}", n.path);
            for m in &msgs {
                println!("{m}");
                if m.starts_with("  error:") {
                    errs += 1;
                } else if m.starts_with("  warn:") {
                    warns += 1;
                }
            }
        }
    }

    println!("\n{} error(s), {} warning(s) across {} file(s)", errs, warns, nodes.len());
    if errs > 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::{platform_errors, recipe_name};

    fn block(inner: &str) -> String {
        format!("---\nkind: tool\n---\n\n```just\n{inner}\n```\n")
    }

    #[test]
    fn flags_overlapping_variants() {
        // [unix, wsl] and [wsl] both match a wsl host → duplicate `r`
        let errs = platform_errors(&block("[unix, wsl]\nr:\n  a\n[wsl]\nr:\n  b"));
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("recipe `r`") && errs[0].contains("[wsl]"));
    }

    #[test]
    fn accepts_mutually_exclusive_variants() {
        let errs = platform_errors(&block("[unix, wsl]\nr:\n  a\n[macos]\nr:\n  b\n[windows]\nr:\n  c"));
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn ignores_files_without_the_convention() {
        assert!(platform_errors(&block("a:\n  one\nb:\n  two")).is_empty());
    }

    #[test]
    fn recipe_name_detects_headers_only() {
        assert_eq!(recipe_name("open target:").as_deref(), Some("open"));
        assert_eq!(recipe_name("check host count=\"5\":").as_deref(), Some("check"));
        assert_eq!(recipe_name("  xdg-open x"), None); // indented body
        assert_eq!(recipe_name("# comment"), None);
        assert_eq!(recipe_name("[unix]"), None);
        assert_eq!(recipe_name("x := 1"), None); // assignment
    }
}
