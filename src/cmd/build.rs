// `jd build` — the one smart sync command. It brings the queryable state up to
// date the fastest way possible, doing only the work that's needed:
//
//   * LOCAL  — if the repo's `.jd` sources changed, rebuild the merged local
//              graph (<root>/.jd/remote-graph.db). Unchanged → skipped. Queries
//              auto-run this same step, so local edits are always reflected.
//   * REMOTE — for each belt remote, re-download its published remote-graph.db
//              only if upstream changed (ETag conditional GET). Unchanged → kept.
//
// The local graph doubles as the PUBLISH artifact: committing it is how a repo
// ships its whole library (every nested home, unioned) for consumers to fetch.

use super::config::Config;
use super::query;
use justdown::graph::{self, Root};

/// (Re)build the merged local graph at `cfg.index_path()` from every `.jd` home
/// under the project. Homes are applied shallow-first so deeper homes win key
/// collisions (matching the live query precedence); nodes key relative to the
/// repo root so each path keeps its `.jd/…` prefix (what a consumer's `get`
/// resolves against `<raw_base>/<path>`). Silent on success (callers report
/// state); returns true on success, false if there were no homes or the write
/// failed. Used by `jd build` and the per-query auto-rebuild.
pub(crate) fn build_local_graph(cfg: &Config) -> bool {
    let project = cfg.project_dir();
    let mut homes = query::local_homes(cfg); // honors JUSTDOWN_NESTED
    homes.reverse(); // discovery is deeper-first; apply deeper LAST so it wins

    let roots: Vec<Root> = homes
        .iter()
        .map(|home| home.join(&cfg.lib))
        .filter(|libdir| libdir.is_dir())
        .map(|libdir| Root::with_base(libdir, project.clone()))
        .collect();
    if roots.is_empty() {
        return false;
    }

    let out = cfg.index_path();
    if let Some(parent) = out.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("jd: cannot create cache dir {}: {e}", parent.display());
            return false;
        }
    }
    match graph::build_index(&out, &roots, crate::CLI_VERSION) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("jd: failed to write store {}: {e}", out.display());
            false
        }
    }
}

pub fn run(cfg: &Config, _args: &[String]) -> i32 {
    // LOCAL: rebuild only if the sources changed.
    match query::ensure_local_graph(cfg) {
        query::LocalState::Rebuilt => eprintln!("jd: local graph rebuilt"),
        query::LocalState::Current => eprintln!("jd: local graph up to date"),
        query::LocalState::None => eprintln!("jd: no local .jd library to build"),
        query::LocalState::Failed => eprintln!("jd: local graph build failed"),
    }

    // REMOTE: refresh each belt remote only if upstream changed.
    let outcomes = query::refresh_belt(cfg);
    for (slug, outcome) in &outcomes {
        match outcome {
            query::Fetch::Updated => eprintln!("jd: refreshed {slug}"),
            query::Fetch::Unchanged => eprintln!("jd: {slug} up to date"),
            query::Fetch::Failed => eprintln!("jd: could not refresh {slug}"),
        }
    }
    0
}
