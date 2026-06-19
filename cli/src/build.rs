// `jd build` — scan <lib>/**/*.jd, parse each into a graph node, and write the
// local SQLite store. The Rust counterpart of the original `build` recipe.

use crate::config::Config;
use crate::jd::{self, Node};
use crate::store::Store;
use std::path::{Path, PathBuf};

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

pub fn run(cfg: &Config) -> i32 {
    let libdir = cfg.lib_dir();
    if !libdir.is_dir() {
        eprintln!("jd: no library dir: {}", libdir.display());
        return 1;
    }

    let mut files = Vec::new();
    collect_jd(&libdir, &mut files);
    // LC_ALL=C byte-order sort on the path, matching the original `find | sort`.
    files.sort_by(|a, b| a.as_os_str().as_encoded_bytes().cmp(b.as_os_str().as_encoded_bytes()));

    let root = &cfg.root;
    let mut nodes: Vec<Node> = Vec::with_capacity(files.len());
    for f in &files {
        let rel = f.strip_prefix(root).unwrap_or(f);
        let rel = rel.to_string_lossy().replace('\\', "/");
        let content = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("jd: cannot read {}: {e}", f.display());
                continue;
            }
        };
        nodes.push(jd::parse(&rel, &content));
    }
    // Stable key order so the store is reproducible regardless of fs walk order.
    nodes.sort_by(|a, b| a.key.cmp(&b.key));

    let out = cfg.index_path();
    if let Err(e) = Store::build(&out, &nodes) {
        eprintln!("jd: failed to write store {}: {e}", out.display());
        return 1;
    }

    eprintln!(
        "built {}: {} entries (schema {})",
        out.display(),
        nodes.len(),
        crate::STORE_SCHEMA
    );
    0
}
