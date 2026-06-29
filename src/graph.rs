//! Building a graph store from one or more `.jd` source roots.
//!
//! The single-root case is `jd build` over a library dir. The multi-root case
//! is the point of the library split: a host layers several sources — the
//! justdown library, the user's `~/.jd`, the project repo — into one local
//! graph. Roots are applied in order and **later roots override earlier ones by
//! node key** (most-specific-wins), so `project` shadows `~/.jd` shadows the
//! shared library.

use crate::jd::{self, Node};
use crate::store::Store;
use std::path::{Path, PathBuf};

/// A source root to index. Every `*.jd` under `dir` becomes a node; `rel_base`
/// is the prefix stripped to derive each node's key/category (usually the repo
/// root, so a file `<rel_base>/library/foo/bar.jd` keys as `foo/bar`).
pub struct Root {
    pub dir: PathBuf,
    pub rel_base: PathBuf,
}

impl Root {
    /// A root whose keys are derived relative to the dir itself.
    pub fn new(dir: impl Into<PathBuf>) -> Root {
        let dir = dir.into();
        Root {
            rel_base: dir.clone(),
            dir,
        }
    }

    /// A root with an explicit `rel_base` (e.g. the repo root, so the lib-dir
    /// prefix is kept in the key path — matching `jd build`).
    pub fn with_base(dir: impl Into<PathBuf>, rel_base: impl Into<PathBuf>) -> Root {
        Root {
            dir: dir.into(),
            rel_base: rel_base.into(),
        }
    }
}

/// Directory names never descended into during nested-home discovery: VCS
/// metadata, dependency installs, build output, and worktrees. Keeps the walk
/// off node_modules-scale trees without pulling in a gitignore parser.
const PRUNE_DIRS: &[&str] = &[
    ".git",
    ".git-fs",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    ".worktrees",
];

/// How deep below the start dir nested-home discovery descends — a backstop so a
/// pathological tree can't make the walk unbounded.
const MAX_HOME_DEPTH: usize = 8;

/// Find every `.jd` home directory at or below `start` — the nested-composition
/// discovery walk. A home is any directory literally named `.jd`; its own subtree
/// is not descended (a library tree holds no further homes). Heavy dirs
/// ([`PRUNE_DIRS`]) and symlinks are skipped, and the descent is depth-capped.
/// Results are sorted **deeper-first**, ties broken by path, so a caller can
/// apply deeper-wins precedence by keeping the first row seen per key.
pub fn find_jd_homes(start: &Path) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    walk_homes(start, 0, &mut homes);
    homes.sort_by(|a, b| {
        b.components()
            .count()
            .cmp(&a.components().count())
            .then_with(|| a.cmp(b))
    });
    homes
}

fn walk_homes(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_HOME_DEPTH {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        // Don't follow symlinks (avoids cycles); only real dirs are walked.
        match entry.file_type() {
            Ok(t) if t.is_dir() => {}
            _ => continue,
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == ".jd" {
            out.push(path); // a home — record it, don't descend into it
            continue;
        }
        if PRUNE_DIRS.contains(&name) {
            continue;
        }
        walk_homes(&path, depth + 1, out);
    }
}

/// Recursively collect every `*.jd` file under `dir`.
pub fn collect_jd(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jd(&path, out);
        } else if path.extension().map(|e| e == "jd").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// Parse every `.jd` under `roots` and write a fresh graph store at
/// `store_path`. Later roots override earlier ones by key. `producer` is
/// stamped into the store's `meta` (the building tool's id/version). Returns
/// the merged node count.
pub fn build_index(store_path: &Path, roots: &[Root], producer: &str) -> rusqlite::Result<usize> {
    // BTreeMap: dedup by key with later-root-wins, and emit in stable key order
    // so the store is reproducible regardless of filesystem walk order.
    let mut by_key: std::collections::BTreeMap<String, Node> = std::collections::BTreeMap::new();

    for root in roots {
        let mut files = Vec::new();
        collect_jd(&root.dir, &mut files);
        // LC_ALL=C byte-order sort, matching the original `find | sort`.
        files.sort_by(|a, b| {
            a.as_os_str()
                .as_encoded_bytes()
                .cmp(b.as_os_str().as_encoded_bytes())
        });
        for f in &files {
            let rel = f.strip_prefix(&root.rel_base).unwrap_or(f);
            let rel = rel.to_string_lossy().replace('\\', "/");
            let content = match std::fs::read_to_string(f) {
                Ok(c) => c,
                Err(_) => continue, // unreadable file: skip (host can pre-validate)
            };
            let node = jd::parse(&rel, &content);
            by_key.insert(node.key.clone(), node); // later root overrides earlier
        }
    }

    let nodes: Vec<Node> = by_key.into_values().collect();
    Store::build(store_path, &nodes, producer)?;
    Ok(nodes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_root_overrides_by_key() {
        let dir = std::env::temp_dir().join("jd_graph_test_prec");
        let base = dir.join("base/cat");
        let over = dir.join("over/cat");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&over).unwrap();
        // same key `cat/tool` in both roots; the second root should win.
        std::fs::write(
            base.join("tool.jd"),
            "---\nname: from_base\nkind: tool\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(
            over.join("tool.jd"),
            "---\nname: from_over\nkind: tool\n---\nbody\n",
        )
        .unwrap();

        let store = dir.join("graph.db");
        let n = build_index(
            &store,
            &[Root::new(dir.join("base")), Root::new(dir.join("over"))],
            "test",
        )
        .unwrap();
        assert_eq!(n, 1, "same key collapses to one node");

        let rows = Store::open(&store)
            .unwrap()
            .load_rows(crate::store::Source::Local)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "from_over", "later root wins");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_homes_discovers_nested_prunes_heavy_and_orders_deeper_first() {
        let root = std::env::temp_dir().join("jd_find_homes_test");
        let _ = std::fs::remove_dir_all(&root);
        // a root home, a nested home two levels down, one inside node_modules
        // (must be pruned), and a `.jd` inside another `.jd` (must not recurse).
        std::fs::create_dir_all(root.join(".jd/library")).unwrap();
        std::fs::create_dir_all(root.join("packages/x/.jd/library")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/dep/.jd/library")).unwrap();
        std::fs::create_dir_all(root.join(".jd/library/.jd")).unwrap();

        let homes = find_jd_homes(&root);
        assert!(homes.contains(&root.join(".jd")), "root home found");
        assert!(
            homes.contains(&root.join("packages/x/.jd")),
            "nested home found"
        );
        assert!(
            !homes.iter().any(|h| h.starts_with(root.join("node_modules"))),
            "node_modules pruned: {homes:?}"
        );
        assert!(
            !homes.iter().any(|h| h.ends_with("library/.jd")),
            "a home's own subtree is not descended: {homes:?}"
        );
        // deeper-first: packages/x/.jd (more components) precedes root .jd
        let deep = homes.iter().position(|h| h == &root.join("packages/x/.jd"));
        let shallow = homes.iter().position(|h| h == &root.join(".jd"));
        assert!(deep < shallow, "deeper home sorts first: {homes:?}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
