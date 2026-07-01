//! `jd just` — run a tool's recipe, not only emit it. The one-liner wrap of
//! `jd get <ref> --justfile | just --justfile - <recipe> <args>`: render the
//! ref's host-resolved justfile, then hand it to `just` on stdin. This makes
//! `jd` the single entry point for *running* a captured procedure — the same
//! deterministic shell dispatch, one command instead of a pipe.

use super::config::Config;
use super::query;
use std::io::Write;
use std::process::{Command, Stdio};

/// `jd just <ref> [recipe] [args...] [--var name=value ...]`. `--var` (anywhere)
/// feeds `<<var>>` injection like `get`; the first positional is the ref, and
/// everything after it — recipe plus its arguments — is passed to `just`
/// verbatim. Exit code is `just`'s own (127 if `just` is not installed).
pub fn run(cfg: &Config, args: &[String]) -> i32 {
    let mut vars = Config::env_vars();
    let mut rest: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let pair = if a == "--var" {
            i += 1;
            match args.get(i) {
                Some(p) => p.as_str(),
                None => {
                    eprintln!("jd: --var needs name=value");
                    return 3;
                }
            }
        } else if let Some(p) = a.strip_prefix("--var=") {
            p
        } else {
            rest.push(a);
            i += 1;
            continue;
        };
        match pair.split_once('=') {
            Some((name, value)) if !name.is_empty() => {
                vars.insert(name.to_string(), value.to_string());
            }
            _ => {
                eprintln!("jd: --var wants name=value: {pair}");
                return 3;
            }
        }
        i += 1;
    }

    let refr = match rest.first() {
        Some(r) if !r.is_empty() => (*r).clone(),
        _ => {
            eprintln!("jd: just needs a ref (try `jd just <ref> <recipe>`)");
            return 3;
        }
    };
    let passthrough = &rest[1..];

    let justfile = match query::render_justfile(cfg, &refr, &vars) {
        Ok(s) => s,
        Err(c) => return c,
    };

    let mut child = match Command::new("just")
        .arg("--justfile")
        .arg("-")
        .args(passthrough.iter().map(|s| s.as_str()))
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("jd: cannot exec `just` ({e}) — install it: https://just.systems");
            return 127;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(justfile.as_bytes());
    }
    match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("jd: just failed: {e}");
            1
        }
    }
}
