// `jd pull` — hydrate a cache scope from a list of online libraries, then index
// them as one merged graph. A tool belt is just a list of repo URLs; pull clones
// each and unions their tools.
//
//   jd pull            clone/refresh into ~/.bombshell/jd  (machine-global, shared)
//   jd pull --local    clone/refresh into <root>/.bombshell/jd  (this repo only)
//
// For each entry in JUSTDOWN_REPOS (default: the single JUSTDOWN_REPO), clones
// it into <scope>/remotes/<slug> (or fast-forwards), then builds <scope>/graph.db
// from every clone's library, keying paths relative to <scope> so `get` resolves
// files at <scope>/remotes/<slug>/…. Later list entries win key collisions.
// Repos consume the merged graph via the local⊕global⊕online tiers. Needs git.

use crate::build;
use crate::config::Config;
use justdown::graph::Root;
use std::path::Path;
use std::process::Command;

pub fn run(cfg: &Config, args: &[String]) -> i32 {
    let local = args.iter().any(|a| a == "--local");
    let scope = if local {
        cfg.cache_dir()
    } else {
        match Config::home_cache_dir() {
            Some(d) => d,
            None => {
                eprintln!("jd: pull: $HOME is unset (use --local for the repo scope)");
                return 1;
            }
        }
    };

    let remotes = cfg.remotes();
    let mut roots: Vec<Root> = Vec::new();
    for r in &remotes {
        let dest = scope.join("remotes").join(&r.slug);
        if let Err(code) = sync_repo(&dest, &r.url, &r.git_ref) {
            return code;
        }
        // Prefer a repo's published toolbelt lib (.bombshell/jd/lib), else its
        // authored source dir (<JUSTDOWN_LIB>). Key relative to scope so the
        // slug disambiguates same-named files across belts.
        let published = dest.join(".bombshell").join("jd").join("lib");
        let authored = dest.join(&cfg.lib);
        let libdir = if published.is_dir() {
            published
        } else if authored.is_dir() {
            authored
        } else {
            eprintln!(
                "jd: pull: {} has no .bombshell/jd/lib or {}/ — skipping",
                r.slug, cfg.lib
            );
            continue;
        };
        roots.push(Root::with_base(libdir, scope.clone()));
    }

    if roots.is_empty() {
        eprintln!("jd: pull: no libraries found across {} remote(s)", remotes.len());
        return 1;
    }
    build::build_roots(&scope.join(&cfg.index), &roots)
}

/// Clone `url`@`git_ref` into `lib`, or fast-forward it if it's already a clone.
/// Best-effort and explicit: any git failure returns a process exit code.
fn sync_repo(lib: &Path, url: &str, git_ref: &str) -> Result<(), i32> {
    if which_git().is_none() {
        eprintln!("jd: pull needs `git` on PATH");
        return Err(4);
    }
    if lib.join(".git").is_dir() {
        eprintln!("jd: pull: refreshing {}", lib.display());
        run_git(&["-C", &lib.to_string_lossy(), "fetch", "--depth", "1", "origin", git_ref])?;
        // reset to the fetched tip so a moved ref / force-push still applies
        run_git(&["-C", &lib.to_string_lossy(), "reset", "--hard", "FETCH_HEAD"])?;
    } else {
        if let Some(parent) = lib.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("jd: cannot create {}: {e}", parent.display());
                return Err(1);
            }
        }
        eprintln!("jd: pull: cloning {url}@{git_ref} → {}", lib.display());
        run_git(&[
            "clone",
            "--depth",
            "1",
            "--branch",
            git_ref,
            url,
            &lib.to_string_lossy(),
        ])?;
    }
    Ok(())
}

fn which_git() -> Option<()> {
    Command::new("git")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| ())
}

fn run_git(args: &[&str]) -> Result<(), i32> {
    match Command::new("git").args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            eprintln!("jd: git {} failed ({s})", args.first().copied().unwrap_or(""));
            Err(4)
        }
        Err(e) => {
            eprintln!("jd: cannot run git: {e}");
            Err(4)
        }
    }
}
