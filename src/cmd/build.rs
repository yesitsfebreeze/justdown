// `jd build` — scan <lib>/**/*.jd and write a SQLite store. A thin wrapper over
// the library's multi-root builder. Two scopes share one builder, differing only
// in where the index lands and what base its keys are relative to:
//
//   jd build              repo home    → <project>/.jd/graph.db (base = the .jd home)
//   jd build --global     machine home → ~/.jd/graph.db         (base ~/.jd)
//
// The default output is both this repo's local tier AND its published toolbelt
// index — committing <project>/.jd/graph.db is how a repo publishes (the
// contract path consumers fetch). `jd pull` reuses `build_into` for cloned belts.

use super::config::Config;
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
        if args.iter().any(|a| a == "--recursive" || a == "-r") {
            return build_recursive(cfg);
        }
        (cfg.index_path(), cfg.root.clone())
    };
    build_into(&out, &cfg.lib_dir(), &base)
}

/// `jd build --recursive` — discover every nested `.jd/<lib>` home under the
/// project tree and build each one's own self-contained store. Each folder's
/// library is the single source of truth for its own procedures (nothing is
/// copied); the root resolves the union at query time. The root home honours an
/// absolute `JUSTDOWN_INDEX` (the publish seam); nested homes always write
/// `<home>/<index-basename>`.
fn build_recursive(cfg: &Config) -> i32 {
    let basename = cfg.index_basename();
    let root_canon = canon(&cfg.root);
    let homes = graph::find_jd_homes(&cfg.project_dir());

    let mut code = 0;
    let mut built = 0usize;
    for home in &homes {
        let libdir = home.join(&cfg.lib);
        if !libdir.is_dir() {
            continue; // a `.jd` with no authored library — nothing to index
        }
        let is_root = canon(home) == root_canon;
        let out = if is_root {
            cfg.index_path()
        } else {
            home.join(&basename)
        };
        let c = build_into(&out, &libdir, home);
        if c == 0 {
            built += 1;
        } else {
            code = c;
        }
    }
    if built == 0 {
        eprintln!(
            "jd: --recursive found no .jd/{} homes under {}",
            cfg.lib,
            cfg.project_dir().display()
        );
        return 1;
    }
    eprintln!("built {built} nested .jd home(s)");
    code
}

/// Canonicalize for identity comparison, falling back to the path itself when it
/// doesn't exist yet (so a not-yet-built home still compares sanely).
fn canon(p: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}
