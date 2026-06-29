// Runtime configuration, resolved from the environment with the same defaults
// the original justfile used (JUSTDOWN_*). `index` is now a SQLite store path
// rather than a flat tsv.

use justdown::render::Vars;
use std::path::{Path, PathBuf};

pub struct Config {
    pub root: PathBuf,
    pub lib: String,
    pub index: String,
    pub repo: String,
    pub git_ref: String,
    pub raw_base: String,
    pub format: Format,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Format {
    Text,
    Json,
}

/// One online library to pull. A tool belt is just a list of these — compose by
/// listing URLs. `slug` is the on-disk folder name under `<scope>/remotes/`.
pub struct Remote {
    pub url: String,
    pub git_ref: String,
    pub slug: String,
}

impl Remote {
    /// The raw base for fetching this remote's published toolbelt without a
    /// clone: `https://raw.githubusercontent.com/<owner>/<repo>/<ref>`. None for
    /// non-GitHub URLs (those are clone-only). Per the contract, the consumable
    /// index lives at `<raw_base>/.jd/graph.db`.
    pub fn raw_base(&self) -> Option<String> {
        // strip scheme, require a github.com host, take owner/repo
        let rest = self
            .url
            .split_once("://")
            .map(|(_, r)| r)
            .unwrap_or(&self.url);
        let (host, path) = rest.split_once('/')?;
        if host != "github.com" {
            return None;
        }
        let mut segs = path.trim_end_matches(".git").split('/');
        let owner = segs.next()?;
        let repo = segs.next()?;
        if owner.is_empty() || repo.is_empty() {
            return None;
        }
        Some(format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{}",
            self.git_ref
        ))
    }
}

/// Keep a slug filesystem-safe: alnum / `-` / `_` / `.` survive, the rest fold
/// to `-`. Keeps `remotes/<slug>` flat and predictable.
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Read a `.jdconfig` belt manifest: one repo entry per line, `#` starts an
/// inline comment, blank lines ignored. Missing/unreadable file → empty list.
fn read_jdconfig(path: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Parse one belt entry into a Remote. Accepts `owner/repo`,
/// `owner/repo@ref`, or a full `https://…` URL (with optional `@ref` when no
/// scheme-side `@`). `owner/repo` shorthand expands to a GitHub https clone URL.
fn parse_remote(entry: &str, default_ref: &str) -> Option<Remote> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    // Pull a trailing `@ref` only for the shorthand form — a scheme URL keeps
    // its `@` (e.g. ssh `git@…`), so we don't split those.
    let (spec, git_ref) = match (entry.contains("://"), entry.rsplit_once('@')) {
        (false, Some((s, r))) if !r.is_empty() && !r.contains('/') => (s, r.to_string()),
        _ => (entry, default_ref.to_string()),
    };
    if spec.contains("://") {
        let trimmed = spec.trim_end_matches(".git");
        let tail: Vec<&str> = trimmed.rsplit('/').take(2).collect();
        let slug = tail.into_iter().rev().collect::<Vec<_>>().join("-");
        Some(Remote {
            url: spec.to_string(),
            git_ref,
            slug: slugify(&slug),
        })
    } else {
        Some(Remote {
            url: format!("https://github.com/{spec}.git"),
            git_ref,
            slug: slugify(&spec.replace('/', "-")),
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// True when `dir` is a justdown home — the `.jd` directory itself, holding the
/// authored `<lib>/` tree and/or a built `graph.db`. The home is the resolved
/// root: library, index, and vendored remotes all live directly inside it.
fn is_home(dir: &std::path::Path, lib: &str) -> bool {
    dir.join(lib).is_dir() || dir.join("graph.db").is_file()
}

/// Resolve the root git-style: an explicit `JUSTDOWN_ROOT` always wins (the
/// hooks point it straight at `<project>/.jd`); else walk up from the cwd and
/// return the nearest `.jd` home — an ancestor's `.jd/` subdir, or an ancestor
/// that is itself a `.jd` home (cwd already inside it). The walk stops at `$HOME`
/// so a project under it never escapes into the machine-global `~/.jd` cache
/// (that is the separate Global tier, not a project root). Falls back to
/// `<cwd>/.jd` so a fresh `jd build` still targets the `.jd` dir.
fn resolve_root(lib: &str) -> PathBuf {
    if let Some(r) = std::env::var("JUSTDOWN_ROOT")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(r);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    for ancestor in cwd.ancestors() {
        if home.as_deref() == Some(ancestor) {
            break;
        }
        let jd = ancestor.join(".jd");
        if is_home(&jd, lib) {
            return jd;
        }
        if ancestor.file_name().is_some_and(|n| n == ".jd") && is_home(ancestor, lib) {
            return ancestor.to_path_buf();
        }
    }
    cwd.join(".jd")
}

impl Config {
    pub fn from_env() -> Config {
        let lib = env_or("JUSTDOWN_LIB", "library");
        let root = resolve_root(&lib);
        let index = env_or("JUSTDOWN_INDEX", "remote-graph.db");
        let repo = env_or("JUSTDOWN_REPO", "yesitsfebreeze/justdown");
        let branch = env_or("JUSTDOWN_BRANCH", "main");
        let ref_ = env_or("JUSTDOWN_REF", &branch);
        let raw_base = env_or(
            "JUSTDOWN_RAW_BASE",
            &format!("https://raw.githubusercontent.com/{repo}/{ref_}"),
        );
        // Wire format is set by the global `--json` flag (parsed in `main`), not
        // the environment. Default to text; `main` overrides after flag parse.
        Config {
            root,
            lib,
            index,
            repo,
            git_ref: ref_,
            raw_base,
            format: Format::Text,
        }
    }

    /// Host-injected `<<var>>` values drawn from the environment: every
    /// `JUSTDOWN_VAR_<name>` becomes the variable `<name>`. This keeps with
    /// justdown's `JUSTDOWN_*`-from-env convention; `get` layers per-call
    /// `--var name=value` flags on top (flags win — see `query::get`).
    /// The `<name>` is lowercased to match the `[A-Za-z0-9_]+` escape grammar
    /// authors write (env keys are conventionally upper-case).
    pub fn env_vars() -> Vars {
        let mut vars = Vars::new();
        for (k, v) in std::env::vars() {
            if let Some(name) = k.strip_prefix("JUSTDOWN_VAR_") {
                if !name.is_empty() {
                    vars.insert(name.to_lowercase(), v);
                }
            }
        }
        vars
    }

    pub fn lib_dir(&self) -> PathBuf {
        self.root.join(&self.lib)
    }

    /// The repo-scoped justdown home — the `.jd` dir, which IS the root. Holds
    /// the authored `<lib>/`, the published `remote-graph.db`, and the `.jdconfig`
    /// belt.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.clone()
    }

    /// This repo's published graph (`<root>/.jd/remote-graph.db` by default) — the
    /// single merged store a consumer downloads via `jd refresh`. Committed, not
    /// gitignored. `jd build` writes it; local queries read the repo live and
    /// never touch it. An absolute `JUSTDOWN_INDEX` escapes the cache dir.
    pub fn index_path(&self) -> PathBuf {
        self.cache_dir().join(&self.index)
    }

    /// Whether nested-home composition is enabled (default on). `JUSTDOWN_NESTED=0`
    /// opts out, restoring the exact pre-feature single-home union.
    pub fn nested_enabled() -> bool {
        !matches!(
            std::env::var("JUSTDOWN_NESTED").ok().as_deref(),
            Some("0") | Some("false") | Some("off")
        )
    }

    /// The project directory that owns the repo-LOCAL `.jd` home — its parent.
    /// Nested-home discovery walks down from here. Falls back to the root itself
    /// when it has no parent (e.g. a filesystem-root home).
    pub fn project_dir(&self) -> PathBuf {
        self.root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }

    /// The machine-scoped download cache root for prebuilt belt graphs:
    /// `$XDG_CACHE_HOME/justdown` (else `~/.cache/justdown`). This is a plain
    /// cache, NOT a `.jd` home — `jd refresh` fills it; queries read it offline.
    /// None when neither `$XDG_CACHE_HOME` nor `$HOME` is set.
    pub fn cache_root() -> Option<PathBuf> {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|h| !h.is_empty())
                    .map(|h| PathBuf::from(h).join(".cache"))
            })
            .map(|d| d.join("justdown"))
    }

    /// The cached graph path for one belt remote: `<cache-root>/belt/<slug>.db`.
    /// None when no cache root resolves. The matching ETag sidecar (for the
    /// conditional refresh) is the same path with `.etag` appended.
    pub fn belt_cache_path(slug: &str) -> Option<PathBuf> {
        Self::cache_root().map(|d| d.join("belt").join(format!("{slug}.db")))
    }

    /// The local-graph staleness sidecar dir: `<cache-root>/local`. Holds one
    /// `<project-hash>.fp` per repo — the source fingerprint of the last local
    /// build, so a query can tell in one cheap stat-walk whether to rebuild.
    pub fn local_cache_dir() -> Option<PathBuf> {
        Self::cache_root().map(|d| d.join("local"))
    }

    /// The tool belt: every online library to cache, in precedence order (later
    /// entries win same-slug dedup). Sourced from the repo `<root>/.jd/.jdconfig`;
    /// `JUSTDOWN_REPOS` env, when set, overrides the file. Falls back to the
    /// single `JUSTDOWN_REPO`.
    pub fn remotes(&self) -> Vec<Remote> {
        let env = env_or("JUSTDOWN_REPOS", "");
        let entries: Vec<String> = if !env.is_empty() {
            env.split([',', '\n', ' ', '\t'])
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        } else {
            read_jdconfig(&self.cache_dir().join(".jdconfig"))
        };

        let mut belt: Vec<Remote> = Vec::new();
        for e in &entries {
            if let Some(r) = parse_remote(e, &self.git_ref) {
                belt.retain(|x| x.slug != r.slug); // last wins
                belt.push(r);
            }
        }
        if belt.is_empty() {
            belt.push(Remote {
                url: format!("https://github.com/{}.git", self.repo),
                git_ref: self.git_ref.clone(),
                slug: slugify(&self.repo.replace('/', "-")),
            });
        }
        belt
    }
}
