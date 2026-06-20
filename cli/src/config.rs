// Runtime configuration, resolved from the environment with the same defaults
// the original justfile used (JUSTDOWN_*). `index` is now a SQLite store path
// rather than a flat tsv.

use justdown::render::Vars;
use std::path::PathBuf;

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
    /// index lives at `<raw_base>/.bombshell/jd/graph.db`.
    pub fn raw_base(&self) -> Option<String> {
        // strip scheme, require a github.com host, take owner/repo
        let rest = self.url.split_once("://").map(|(_, r)| r).unwrap_or(&self.url);
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

impl Config {
    pub fn from_env() -> Config {
        // root defaults to the current dir (where `just` used justfile_directory).
        let root = std::env::var("JUSTDOWN_ROOT")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let lib = env_or("JUSTDOWN_LIB", "library");
        let index = env_or("JUSTDOWN_INDEX", "graph.db");
        let repo = env_or("JUSTDOWN_REPO", "yesitsfebreeze/justdown");
        let branch = env_or("JUSTDOWN_BRANCH", "main");
        let ref_ = env_or("JUSTDOWN_REF", &branch);
        let raw_base = env_or(
            "JUSTDOWN_RAW_BASE",
            &format!("https://raw.githubusercontent.com/{repo}/{ref_}"),
        );
        let format = match env_or("JUSTDOWN_FORMAT", "text").as_str() {
            "json" => Format::Json,
            _ => Format::Text,
        };
        Config {
            root,
            lib,
            index,
            repo,
            git_ref: ref_,
            raw_base,
            format,
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

    /// The repo-scoped cache root: `<root>/.bombshell/jd`. Holds this repo's
    /// derived layer — built index, vendored lib, spec. Gitignored; rebuildable.
    /// The machine-scoped `~/.bombshell/jd` (see `home_cache_dir`) shares this
    /// exact layout, so one resolver walks both; nearer scope wins.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(".bombshell").join("jd")
    }

    /// Repo-scoped index path — also this repo's published toolbelt index (the
    /// contract path consumers fetch). An absolute `JUSTDOWN_INDEX` escapes the
    /// cache dir.
    pub fn index_path(&self) -> PathBuf {
        self.cache_dir().join(&self.index)
    }

    /// The machine-scoped cache root: `~/.bombshell/jd`, shared across repos.
    /// None when `$HOME` is unset/empty.
    pub fn home_cache_dir() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(".bombshell").join("jd"))
    }

    /// Machine-scoped index path (`~/.bombshell/jd/<index>`), if `$HOME` is set.
    pub fn home_index_path(&self) -> Option<PathBuf> {
        Self::home_cache_dir().map(|d| d.join(&self.index))
    }

    /// The machine-scoped bombshell dir, `~/.bombshell` (parent of `jd/`), or
    /// None when `$HOME` is unset. Home of the global `.jdconfig`.
    fn home_bombshell() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(".bombshell"))
    }

    /// The tool belt: every online library to pull, in precedence order (later
    /// entries win — both key collisions at build time and same-slug dedup).
    /// Sourced from `.bombshell/.jdconfig` (global `~/.bombshell` then repo
    /// `<root>/.bombshell`, so the repo belt wins). `JUSTDOWN_REPOS` env, when
    /// set, overrides the files. Falls back to the single `JUSTDOWN_REPO`.
    pub fn remotes(&self) -> Vec<Remote> {
        let env = env_or("JUSTDOWN_REPOS", "");
        let entries: Vec<String> = if !env.is_empty() {
            env.split([',', '\n', ' ', '\t'])
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        } else {
            let mut v = Vec::new();
            if let Some(h) = Self::home_bombshell() {
                v.extend(read_jdconfig(&h.join(".jdconfig")));
            }
            v.extend(read_jdconfig(&self.root.join(".bombshell").join(".jdconfig")));
            v
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

    /// True when JUSTDOWN_FORMAT held an unknown value (anything but text/json).
    /// We resolve to Text but a strict caller can still reject it.
    pub fn format_valid() -> bool {
        match std::env::var("JUSTDOWN_FORMAT") {
            Ok(v) if !v.is_empty() => matches!(v.as_str(), "text" | "json"),
            _ => true,
        }
    }
}
