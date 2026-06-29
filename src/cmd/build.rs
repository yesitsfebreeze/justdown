// `jd build` — scan every `.jd` home's <lib>/**/*.jd and write ONE merged store
// at <project>/.jd/remote-graph.db. A thin wrapper over the library's multi-root
// builder.
//
// This is the PUBLISH step: committing <project>/.jd/remote-graph.db is how a repo
// publishes its whole library (every nested home, unioned) — the single file a
// consumer downloads via `jd refresh`. Local queries don't need it; they read the
// repo's .jd files live. Node paths key relative to the repo root (so they carry
// each home's `.jd/…` prefix) and a consumer's `get` fetches the body from there.

use super::config::Config;
use justdown::graph::{self, Root};
use justdown::store::STORE_SCHEMA;
use std::path::Path;

/// Build one index at `out` from many roots — the multi-home/multi-remote merge.
/// Later roots win key collisions. Creates `out`'s parent dir first (SQLite
/// won't). Returns a process exit code.
pub fn build_roots(out: &Path, roots: &[Root]) -> i32 {
    if let Some(parent) = out.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("jd: cannot create cache dir {}: {e}", parent.display());
            return 1;
        }
    }
    match graph::build_index(out, roots, crate::CLI_VERSION) {
        Ok(n) => {
            eprintln!("built {}: {n} entries (schema {STORE_SCHEMA})", out.display());
            0
        }
        Err(e) => {
            eprintln!("jd: failed to write store {}: {e}", out.display());
            1
        }
    }
}

pub fn run(cfg: &Config, _args: &[String]) -> i32 {
    let project = cfg.project_dir();
    // Discovery is deeper-first; reverse to shallow-first so deeper homes are
    // applied LAST and win key collisions (matching the live query precedence).
    let mut homes = graph::find_jd_homes(&project);
    homes.reverse();

    // Every home's library is one root, keyed relative to the repo root so each
    // node's path keeps its `.jd/…` prefix — what a consumer's `get` resolves
    // against `<raw_base>/<path>`.
    let roots: Vec<Root> = homes
        .iter()
        .map(|home| home.join(&cfg.lib))
        .filter(|libdir| libdir.is_dir())
        .map(|libdir| Root::with_base(libdir, project.clone()))
        .collect();

    if roots.is_empty() {
        eprintln!(
            "jd: build found no .jd/{} homes under {}",
            cfg.lib,
            project.display()
        );
        return 1;
    }
    build_roots(&cfg.index_path(), &roots)
}
