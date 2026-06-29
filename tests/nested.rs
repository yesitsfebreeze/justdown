// Integration test: nested local graph composition. A repo can hold several
// `.jd` homes at different depths; `jd build --recursive` builds each its own
// self-contained store, and queries from the root union them all — with deeper
// homes winning key collisions, and the legacy single-home behavior intact when
// nested composition is switched off.
//
// Hermetic: every child `jd` is pointed at the fixture as JUSTDOWN_ROOT and at a
// non-GitHub belt so the online tier is skipped (no network).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A unique fixture dir for this test run, cleaned and recreated.
fn fixture_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jd-nested-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn jd_file(name: &str) -> String {
    format!("---\nname: {name}\nkind: tool\ndescription: the {name} tool\n---\nbody of {name}\n")
}

/// Lay down a fixture with three `.jd` homes:
///   <root>/.jd                  → core/root-tool, shared/dup (name from_root)
///   <root>/packages/a/.jd       → pkg/a-tool,    shared/dup (name from_a)  [deeper]
///   <root>/.voit/.jd            → voit/v-tool
/// plus a node_modules home that must be pruned (never discovered).
fn build_fixture(tag: &str) -> PathBuf {
    let root = fixture_root(tag);
    let lib = |home: &str, rel: &str| root.join(home).join("library").join(rel);

    write(&lib(".jd", "core/root-tool.jd"), &jd_file("root_tool"));
    write(&lib(".jd", "shared/dup.jd"), &jd_file("from_root"));
    write(
        &lib("packages/a/.jd", "pkg/a-tool.jd"),
        &jd_file("a_tool"),
    );
    write(
        &lib("packages/a/.jd", "shared/dup.jd"),
        &jd_file("from_a"),
    );
    write(&lib(".voit/.jd", "voit/v-tool.jd"), &jd_file("v_tool"));
    // pruned home: discovery must never reach it
    write(
        &lib("node_modules/dep/.jd", "junk/nope.jd"),
        &jd_file("nope"),
    );
    root
}

/// Run `jd` against the fixture. `nested` toggles JUSTDOWN_NESTED.
fn run(root: &Path, nested: bool, args: &[&str]) -> (i32, String, String) {
    let jd_home = root.join(".jd");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_jd"));
    cmd.args(args)
        .env("JUSTDOWN_ROOT", &jd_home)
        // isolate HOME and the OS cache so the run touches only the fixture (no
        // real ~/.cache sidecars or belt), exercising just the local homes
        .env("HOME", root.join("xhome"))
        .env("XDG_CACHE_HOME", root.join("xcache"))
        .env("JUSTDOWN_NESTED", if nested { "1" } else { "0" })
        // a non-GitHub belt → raw_base() is None → online tier skipped, no network
        .env("JUSTDOWN_REPOS", "https://example.invalid/none/none");
    let out = cmd.output().expect("spawn jd");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn recursive_build_and_root_union_resolve_all_homes() {
    let root = build_fixture("union");

    // (a) build writes ONE merged publish artifact at the root home; the pruned
    // home never contributes (and no stray per-home stores are written).
    let (code, _out, err) = run(&root, true, &["build"]);
    assert_eq!(code, 0, "build failed: {err}");
    assert!(
        root.join(".jd/remote-graph.db").exists(),
        "merged remote-graph.db not built"
    );
    assert!(
        !root.join("node_modules/dep/.jd/remote-graph.db").exists(),
        "pruned home must not be built"
    );

    // (b) the root resolves keys from every home at once (live, not from build)
    for key in ["core/root-tool", "pkg/a-tool", "voit/v-tool"] {
        let (code, out, err) = run(&root, true, &["get", key, "--frontmatter"]);
        assert_eq!(code, 0, "get {key} failed: {err}\n{out}");
    }

    // ls spans all homes' categories
    let (code, out, _err) = run(&root, true, &["ls"]);
    assert_eq!(code, 0);
    for cat in ["core", "pkg", "voit", "shared"] {
        assert!(out.contains(cat), "ls missing category {cat}:\n{out}");
    }
    assert!(!out.contains("junk"), "pruned home leaked into ls:\n{out}");

    // (c) deliberate collision: the deeper home (packages/a) wins shared/dup
    let (code, out, err) = run(&root, true, &["get", "shared/dup", "--human"]);
    assert_eq!(code, 0, "get shared/dup failed: {err}");
    assert!(
        out.contains("from_a"),
        "deeper home must win the key collision, got:\n{out}"
    );
}

#[test]
fn nested_disabled_is_legacy_single_home() {
    let root = build_fixture("legacy");
    // Build only the root home (plain build), as a legacy repo would.
    let (code, _o, err) = run(&root, false, &["build"]);
    assert_eq!(code, 0, "root build failed: {err}");

    // With nesting off, the root sees only its own keys.
    let (code, _o, _e) = run(&root, false, &["get", "core/root-tool", "--frontmatter"]);
    assert_eq!(code, 0, "root key must resolve in legacy mode");

    let (code, _o, _e) = run(&root, false, &["get", "pkg/a-tool", "--frontmatter"]);
    assert_eq!(code, 2, "nested key must NOT resolve when nesting is disabled");

    // shared/dup resolves to the root's copy (no nested home in the union)
    let (code, out, _e) = run(&root, false, &["get", "shared/dup", "--human"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("from_root"),
        "legacy union must use the root's shared/dup, got:\n{out}"
    );
}
