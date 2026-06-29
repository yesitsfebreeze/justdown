// `jd refresh` — rebuild the cached graph. Everything that changes rarely (the
// online belt of published remotes) is downloaded once into the local cache, so
// queries read it offline. The repo-local `.jd` files are NOT cached — they're
// parsed live on every query (see `query::live_cwd_rows`).
//
// For each belt entry (JUSTDOWN_REPOS, else <root>/.jd/.jdconfig, else the single
// JUSTDOWN_REPO) we fetch the remote's published `<raw_base>/.jd/remote-graph.db`
// into `<cache-root>/belt/<slug>.db`. Best-effort: an unreachable remote is warned
// and skipped, not fatal. Needs curl.

use super::config::Config;
use super::query::curl_to_file;

pub fn run(cfg: &Config) -> i32 {
    let remotes = cfg.remotes();
    let mut ok = 0usize;
    let mut failed = 0usize;
    for r in &remotes {
        let Some(raw) = r.raw_base() else {
            eprintln!("jd: refresh: {} is not a github.com remote — skipping", r.url);
            continue;
        };
        let Some(dest) = Config::belt_cache_path(&r.slug) else {
            eprintln!("jd: refresh: no cache dir ($XDG_CACHE_HOME/$HOME unset)");
            return 1;
        };
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("jd: refresh: cannot create {}: {e}", parent.display());
                return 1;
            }
        }
        let url = format!("{raw}/.jd/{}", cfg.index);
        if curl_to_file(&url, &dest) {
            eprintln!("jd: refresh: cached {} → {}", r.slug, dest.display());
            ok += 1;
        } else {
            eprintln!("jd: refresh: failed to fetch {url}");
            failed += 1;
        }
    }
    if ok == 0 && failed > 0 {
        return 4;
    }
    eprintln!("refreshed {ok} belt remote(s)");
    0
}
