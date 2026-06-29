// Integration test: `jd` must not panic when its stdout pipe closes early
// (the `jd ls | head` / `jd --json ls | jq` case). Before the SIGPIPE reset in
// `main()`, Rust's default SIG_IGN disposition made `println!` to a dead pipe
// panic with "failed printing to stdout: Broken pipe" plus a backtrace — the
// worst failure mode for a tool whose pitch is "pipeable data source".
//
// These run as integration tests so `CARGO_BIN_EXE_jd` (the built binary) is set.

use std::io::Read;
use std::process::{Command, Stdio};

/// Build the repo's own library into a temp store, returning the env vars a
/// child `jd` needs to use it (so the test is hermetic — no reliance on the
/// caller having run `jd build`, and no network).
fn env_with_store() -> Option<Vec<(String, String)>> {
    // The crate manifest dir is the repo root, so build the repo's own library.
    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tmp = std::env::temp_dir().join(format!("jd-sigpipe-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    // build into an absolute index path (escapes the cache dir, per the CLI)
    let status = Command::new(env!("CARGO_BIN_EXE_jd"))
        .arg("build")
        .env("JUSTDOWN_ROOT", &repo_root)
        .env("JUSTDOWN_INDEX", tmp.to_string_lossy().as_ref())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    if !status.success() || !tmp.exists() {
        return None;
    }
    Some(vec![
        (
            "JUSTDOWN_ROOT".to_string(),
            repo_root.to_string_lossy().into_owned(),
        ),
        (
            "JUSTDOWN_INDEX".to_string(),
            tmp.to_string_lossy().into_owned(),
        ),
        // point the online merge at localhost so it 404s fast, not hang
        (
            "JUSTDOWN_RAW_BASE".to_string(),
            "http://127.0.0.1:1".to_string(),
        ),
    ])
}

/// Read exactly one line from `out`, then drop it — the child keeps writing and
/// its next write hits a broken pipe. It must die cleanly (SIGPIPE), not panic.
fn assert_no_panic_on_early_close(args: &[&str]) {
    let Some(env) = env_with_store() else {
        eprintln!("sigpipe test: could not build a store; skipping {args:?}");
        return;
    };
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jd"));
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in &env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn jd");

    let mut out = child.stdout.take().expect("piped stdout");
    // consume up to and including the first newline, then close our end
    let mut byte = [0u8; 1];
    loop {
        if out.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
    }
    drop(out); // break the pipe → child's next write sees EPIPE

    let output = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Broken pipe")
            && !stderr.contains("panicked")
            && !stderr.contains("backtrace"),
        "jd panicked on a closed pipe (args {args:?}):\n{stderr}"
    );
}

#[test]
fn ls_then_close_does_not_panic() {
    assert_no_panic_on_early_close(&["ls"]);
}

#[test]
fn json_ls_then_close_does_not_panic() {
    // --json emits the whole graph as one large println — the most likely to
    // overflow the pipe buffer and trip the broken-pipe path mid-write.
    assert_no_panic_on_early_close(&["--json", "ls"]);
}

#[test]
fn json_search_then_close_does_not_panic() {
    assert_no_panic_on_early_close(&["--json", "search", "the", "tool", "100"]);
}
