// `jd lint` — validate the local library's .jd frontmatter. Per-file rules
// (required fields, kind/danger enums, tool needs a run recipe, platform-variant
// servability) live in the shared core (`justdown::lint`) so the CLI and any
// `.jd` author validate identically; this file adds the cross-file checks that
// need the whole corpus — duplicate name/key and `@link` resolution — and the
// walk + reporting. Exits 1 on any error (CI-gateable); warnings don't fail.

use crate::config::Config;
use justdown::jd::{self, Node};
use justdown::links::{classify, Link, NameIndex, Resolve};
use justdown::lint::{lint_node, Finding};
use std::collections::HashMap;

pub fn run(cfg: &Config) -> i32 {
    let libdir = cfg.lib_dir();
    if !libdir.is_dir() {
        eprintln!("jd: no library dir: {}", libdir.display());
        return 1;
    }

    let mut files = Vec::new();
    justdown::graph::collect_jd(&libdir, &mut files);
    files.sort_by(|a, b| {
        a.as_os_str()
            .as_encoded_bytes()
            .cmp(b.as_os_str().as_encoded_bytes())
    });

    let mut nodes: Vec<Node> = Vec::new();
    let mut bodies: Vec<String> = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(&cfg.root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
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
    // corpus name index, so a bare `@name` is checked the same way the store
    // resolves it (shared core, no drift).
    let idx = NameIndex::build(nodes.iter().map(|n| (n.key.as_str(), n.name.as_str())));

    let mut errs = 0usize;
    let mut warns = 0usize;
    for (i, n) in nodes.iter().enumerate() {
        // Per-file checks (shared core). When a file has no frontmatter that is
        // the sole finding, so the cross-file checks below are also skipped.
        let mut findings: Vec<Finding> = lint_node(n, &bodies[i]);
        if n.has_frontmatter {
            // Cross-file checks: need the whole corpus, so they live here.
            if n.name_given && namecount.get(n.name.as_str()).copied().unwrap_or(0) > 1 {
                findings.push(Finding::error(format!("duplicate name: {}", n.name)));
            }
            if keycount.get(n.key.as_str()).copied().unwrap_or(0) > 1 {
                findings.push(Finding::error(format!("duplicate key: {}", n.key)));
            }
            for l in &n.links {
                match classify(l) {
                    // fuzzy `@?term` is inherently one-to-many and re-resolved on
                    // read — not a fixed edge, so nothing to validate here.
                    Link::Fuzzy(_) => {}
                    // `@dir/name` names an exact key: a miss is a real broken edge
                    // (knowledge files may reference external modules → warn).
                    Link::Key(k) => {
                        if !keys.contains(k) {
                            if n.kind == "knowledge" {
                                findings.push(Finding::warn(format!(
                                    "unresolved @link: {k} (external reference?)"
                                )));
                            } else {
                                findings.push(Finding::error(format!("broken @link: {k}")));
                            }
                        }
                    }
                    // bare `@name` resolves via the corpus; unresolved/ambiguous
                    // is a warning (the spec says direct links lint-warn).
                    Link::Name(name) => match idx.resolve(name) {
                        Resolve::Unique(_) => {}
                        Resolve::None => findings.push(Finding::warn(format!(
                            "unresolved @link: {name}"
                        ))),
                        Resolve::Ambiguous(keys) => findings.push(Finding::warn(format!(
                            "ambiguous @link: {name} (matches {})",
                            keys.join(", ")
                        ))),
                    },
                }
            }
        }
        if !findings.is_empty() {
            println!("lint: {}", n.path);
            for f in &findings {
                let word = if f.is_error() { "error" } else { "warn" };
                println!("  {word}: {}", f.message);
                if f.is_error() {
                    errs += 1;
                } else {
                    warns += 1;
                }
            }
        }
    }

    println!(
        "\n{} error(s), {} warning(s) across {} file(s)",
        errs,
        warns,
        nodes.len()
    );
    if errs > 0 {
        1
    } else {
        0
    }
}
