// Runtime configuration, resolved from the environment with the same defaults
// the original justfile used (JUSTDOWN_*). `index` is now a SQLite store path
// rather than a flat tsv.

use std::path::PathBuf;

pub struct Config {
    pub root: PathBuf,
    pub lib: String,
    pub index: String,
    pub repo: String,
    pub branch: String,
    pub ref_: String,
    pub raw_base: String,
    pub format: Format,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Format {
    Text,
    Json,
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
            branch,
            ref_,
            raw_base,
            format,
        }
    }

    pub fn lib_dir(&self) -> PathBuf {
        self.root.join(&self.lib)
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join(&self.index)
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
