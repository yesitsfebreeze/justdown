//! Platform-guarded recipe variants — the one justdown extension to just's
//! grammar (see `justdown.md`, "Platform-guarded variants").
//!
//! A recipe whose command differs by OS is written once per platform, each
//! variant preceded by a `[unix]` / `[macos]` / `[windows]` / `[wsl]` attribute
//! line (`darwin` is an accepted alias for `macos`; a comma list `[unix, wsl]`
//! guards one body for several hosts). just itself has no `[wsl]` attribute and
//! would reject one, so platform selection is resolved by the **runner**, not by
//! just: detect the host, keep only the matching variant, and strip the
//! attribute lines. What reaches `just --justfile -` is then an ordinary
//! justfile with a single plain definition per recipe.
//!
//! This lives in core so every runner — the `jd` CLI and bombshell — resolves
//! variants identically, instead of each reimplementing the rule and drifting.

/// The platform tokens the runner selects on. `darwin` is accepted as an input
/// alias for `macos` but is not itself a host token.
pub const PLATFORMS: &[&str] = &["unix", "macos", "windows", "wsl"];

/// The host platform token used to select `[os]` recipe variants:
/// `unix` | `macos` | `windows` | `wsl`. `JD_PLATFORM`, if set, wins (test/CI
/// seam). Otherwise inferred from the OS, with Linux refined to `wsl` when
/// running under WSL (`/proc/version` mentions microsoft, or `$WSL_DISTRO_NAME`).
pub fn host_platform() -> String {
    if let Ok(p) = std::env::var("JD_PLATFORM") {
        if !p.is_empty() {
            return p;
        }
    }
    match std::env::consts::OS {
        "macos" => "macos".to_string(),
        "windows" => "windows".to_string(),
        "linux" => {
            let wsl = std::env::var("WSL_DISTRO_NAME")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
                || std::fs::read_to_string("/proc/version")
                    .map(|s| s.to_lowercase().contains("microsoft"))
                    .unwrap_or(false);
            if wsl {
                "wsl".to_string()
            } else {
                "unix".to_string()
            }
        }
        _ => "unix".to_string(),
    }
}

/// Parse a platform-attribute line like `[unix, wsl]` into its tags. Returns
/// `None` for any line that is not exclusively platform tags — so non-platform
/// just attributes (`[private]`, `[confirm]`, …) pass through untouched.
pub fn parse_platform_attr(line: &str) -> Option<Vec<String>> {
    let s = line.trim();
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    let mut tags = Vec::new();
    for part in inner.split(',') {
        match part.trim() {
            t @ ("unix" | "macos" | "darwin" | "windows" | "wsl") => tags.push(t.to_string()),
            _ => return None,
        }
    }
    if tags.is_empty() {
        None
    } else {
        Some(tags)
    }
}

/// Collect the raw lines inside every ```` ```just ```` fence in a .jd body,
/// with NO platform filtering — the unresolved variants. `lint` walks these per
/// platform to check that selection yields a servable (non-duplicated) justfile.
pub fn raw_tools_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut injust = false;
    for line in body.lines() {
        if line.starts_with("```just") {
            injust = true;
            continue;
        }
        if injust && line.starts_with("```") {
            injust = false;
            continue;
        }
        if injust {
            out.push(line.to_string());
        }
    }
    out
}

/// Select the recipe variants matching `plat` and strip the attribute lines.
/// A `[os]` attr guards the recipe header that follows it and that recipe's
/// indented body; untagged lines always pass. `darwin` is an alias for `macos`.
/// Authors keep same-named variants mutually exclusive per platform, so exactly
/// one definition of each recipe survives.
pub fn platsel(lines: &[&str], plat: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut pend = false; // previous line was a platform attr; next is the header
    let mut guarded = false; // inside a guarded recipe's body
    let mut keep = true; // emit the current guarded block?
    for &line in lines {
        if let Some(tags) = parse_platform_attr(line) {
            keep = tags
                .iter()
                .any(|t| (if t == "darwin" { "macos" } else { t.as_str() }) == plat);
            pend = true;
            guarded = false;
            continue;
        }
        if pend {
            pend = false;
            guarded = true;
            if keep {
                out.push(line.to_string());
            }
            continue;
        }
        if guarded {
            if line.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
                if keep {
                    out.push(line.to_string());
                }
                continue;
            }
            guarded = false;
            keep = true;
            // not indented → end of guarded body; fall through to emit normally
        }
        out.push(line.to_string());
    }
    out
}

/// Resolve a justfile's platform-guarded variants for the current host: keep
/// only the matching `[os]` variant of each recipe and strip the attribute
/// lines, so the result is a plain justfile vanilla `just` accepts. A justfile
/// with no platform attributes is returned essentially unchanged. This is the
/// one call a runner needs before feeding text to `just --justfile -`.
pub fn select_for_host(justfile: &str) -> String {
    let lines: Vec<&str> = justfile.lines().collect();
    platsel(&lines, &host_platform()).join("\n")
}

#[cfg(test)]
mod tests {
    use super::{parse_platform_attr, platsel};

    fn sel(src: &str, plat: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        platsel(&lines, plat).join("\n")
    }

    #[test]
    fn picks_one_variant_per_host_and_strips_attrs() {
        let src = "[unix]\nopen t:\n  xdg-open {{t}}\n[macos]\nopen t:\n  open {{t}}\n[windows]\nopen t:\n  start {{t}}\n[wsl]\nopen t:\n  wslview {{t}}";
        assert_eq!(sel(src, "unix"), "open t:\n  xdg-open {{t}}");
        assert_eq!(sel(src, "macos"), "open t:\n  open {{t}}");
        assert_eq!(sel(src, "windows"), "open t:\n  start {{t}}");
        assert_eq!(sel(src, "wsl"), "open t:\n  wslview {{t}}");
    }

    #[test]
    fn comma_list_and_darwin_alias() {
        let src = "[unix, wsl]\nr:\n  a\n[macos]\nr:\n  b";
        assert_eq!(sel(src, "unix"), "r:\n  a");
        assert_eq!(sel(src, "wsl"), "r:\n  a");
        assert_eq!(sel(src, "macos"), "r:\n  b");
        let darwin = "[darwin]\nr:\n  mac";
        assert_eq!(sel(darwin, "macos"), "r:\n  mac");
        assert_eq!(sel(darwin, "unix"), "");
    }

    #[test]
    fn untagged_and_nonplatform_attrs_pass_through() {
        // a leading comment + untagged recipe always survive
        let src = "# desc\nr:\n  body\n[unix]\nr2:\n  ux";
        assert_eq!(sel(src, "macos"), "# desc\nr:\n  body");
        // non-platform just attributes are not platform attrs → untouched
        assert_eq!(parse_platform_attr("[private]"), None);
        assert_eq!(parse_platform_attr("[confirm: \"sure?\"]"), None);
        let keep = "[private]\nr:\n  body";
        assert_eq!(sel(keep, "unix"), "[private]\nr:\n  body");
    }

    #[test]
    fn parses_tag_lists() {
        assert_eq!(
            parse_platform_attr("[unix]"),
            Some(vec!["unix".to_string()])
        );
        assert_eq!(
            parse_platform_attr("[ unix , wsl ]"),
            Some(vec!["unix".to_string(), "wsl".to_string()])
        );
        assert_eq!(parse_platform_attr("not an attr"), None);
        assert_eq!(parse_platform_attr("[unix, bogus]"), None);
    }

    #[test]
    fn select_for_host_respects_jd_platform_override() {
        // `select_for_host` honors the JD_PLATFORM seam so a runner resolves the
        // host variant end-to-end. Guard the env mutation behind the seam only.
        std::env::set_var("JD_PLATFORM", "macos");
        let src = "[unix]\nopen t:\n  xdg-open {{t}}\n[macos]\nopen t:\n  open {{t}}";
        assert_eq!(super::select_for_host(src), "open t:\n  open {{t}}");
        std::env::remove_var("JD_PLATFORM");
    }
}
