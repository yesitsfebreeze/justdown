// `jd build` — scan <lib>/**/*.jd and write a SQLite store. A thin wrapper over
// the library's multi-root builder. Two scopes share one builder, differing only
// in where the index lands and what base its keys are relative to:
//
//   jd build              repo cache   → <root>/.bombshell/jd/graph.db (base <root>)
//   jd build --global     machine cache→ ~/.bombshell/jd/graph.db     (base ~/.bombshell/jd)
//
// The default output is both this repo's local tier AND its published toolbelt
// index — committing <root>/.bombshell/jd/graph.db is how a repo publishes (the
// contract path consumers fetch). `jd pull` reuses `build_into` for cloned belts.

use crate::config::Config;
use justdown::graph::{self, Root};
use justdown::store::STORE_SCHEMA;
use std::path::Path;

/// Build an index at `out` from `libdir`, keying node paths relative to `base`.
pub fn build_into(out: &Path, libdir: &Path, base: &Path) -> i32 {
    if !libdir.is_dir() {
        eprintln!("jd: no library dir: {}", libdir.display());
        return 1;
    }
    build_roots(
        out,
        &[Root::with_base(libdir.to_path_buf(), base.to_path_buf())],
    )
}

/// Build one index at `out` from many roots — the multi-remote merge. Later
/// roots win key collisions (so `JUSTDOWN_REPOS` order = precedence). Creates
/// `out`'s parent dir first (SQLite won't). Returns a process exit code.
pub fn build_roots(out: &Path, roots: &[Root]) -> i32 {
    if let Some(parent) = out.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("jd: cannot create cache dir {}: {e}", parent.display());
            return 1;
        }
    }
    match graph::build_index(out, roots, crate::CLI_VERSION) {
        Ok(n) => {
            eprintln!(
                "built {}: {n} entries (schema {STORE_SCHEMA})",
                out.display()
            );
            0
        }
        Err(e) => {
            eprintln!("jd: failed to write store {}: {e}", out.display());
            1
        }
    }
}

pub fn run(cfg: &Config, args: &[String]) -> i32 {
    // (out, base) per scope; libdir is always the configured authored lib.
    let (out, base) = if args.iter().any(|a| a == "--global") {
        match Config::home_cache_dir() {
            Some(d) => (d.join(&cfg.index), d),
            None => {
                eprintln!("jd: build --global: $HOME is unset");
                return 1;
            }
        }
    } else {
        (cfg.index_path(), cfg.root.clone())
    };
    build_into(&out, &cfg.lib_dir(), &base)
}
