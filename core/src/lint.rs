//! Single-file `.jd` validation, shared by the `jd` CLI's `lint` and by any
//! runner that authors `.jd` files (e.g. bombshell's `/learn` synthesis) so the
//! spec's required-field and platform-variant rules live in one place instead of
//! being re-derived per consumer.
//!
//! [`lint_node`] covers everything checkable from a single file: a frontmatter
//! block, the required `name`/`description`/`kind` fields, the `kind`/`danger`
//! enums, a tool's `run:` recipe, a `use_when` retrieval hint, and that the
//! platform-variant block resolves to a servable justfile on every host.
//!
//! Cross-file checks — duplicate `name`/`key` and `@link` resolution — need the
//! whole corpus and stay with the caller (the CLI walks the library; a
//! single-file author has no corpus to resolve against).

use crate::jd::Node;
use crate::platform::{parse_platform_attr, platsel, raw_tools_lines, PLATFORMS};
use std::collections::HashMap;

/// The valid `kind` values (the closed set the graph models).
pub const KINDS: &[&str] = &["tool", "agent", "knowledge", "workflow"];
/// The valid `danger` levels.
pub const DANGERS: &[&str] = &["none", "low", "medium", "high"];

/// Severity of a lint [`Finding`]. Errors fail a gate; warnings don't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
}

/// One lint finding: a severity and a bare message (no `error:`/`warn:` prefix —
/// the caller renders that from [`Severity`]).
#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

impl Finding {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
        }
    }
    pub fn warn(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warn,
            message: message.into(),
        }
    }
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// The recipe name a just-block line declares as a header, if any. A header is
/// an unindented `name [params]:` line — not a comment, attribute, assignment
/// (`:=`), or body line. Used to detect duplicate definitions after platform
/// selection.
pub fn recipe_name(line: &str) -> Option<String> {
    if line.is_empty() || line.starts_with([' ', '\t', '#', '[']) || line.contains(":=") {
        return None;
    }
    let head = &line[..line.find(':')?];
    let nm = head.split_whitespace().next()?;
    if !nm.is_empty()
        && nm
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(nm.to_string())
    } else {
        None
    }
}

/// Bare-message platform-variant errors for one file's body. Empty when the file
/// does not use the `[os]` convention or every host resolves to a servable
/// (non-duplicated) justfile. A pair of variants that both match one platform
/// would hand `just` a duplicate recipe — that is the error this catches.
pub fn platform_errors(body: &str) -> Vec<String> {
    let lines = raw_tools_lines(body);
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    // only files that actually use the extension are checked
    if !refs.iter().any(|l| parse_platform_attr(l).is_some()) {
        return Vec::new();
    }
    let mut errs = Vec::new();
    for &plat in PLATFORMS {
        let resolved = platsel(&refs, plat);
        let mut counts: HashMap<String, usize> = HashMap::new();
        for l in &resolved {
            if let Some(n) = recipe_name(l) {
                *counts.entry(n).or_insert(0) += 1;
            }
        }
        let mut dups: Vec<&String> = counts
            .iter()
            .filter(|(_, &c)| c > 1)
            .map(|(n, _)| n)
            .collect();
        dups.sort();
        for n in dups {
            errs.push(format!(
                "recipe `{n}` has overlapping platform variants on [{plat}] (would serve a duplicate definition)"
            ));
        }
    }
    errs
}

/// Validate everything checkable from a single parsed [`Node`] plus its raw
/// `body`. Returns findings in report order. Does NOT include cross-file checks
/// (duplicate name/key, `@link` resolution) — those need the whole corpus and
/// belong to the caller. When the file has no frontmatter, that is the only
/// finding (mirrors the CLI: the rest of the checks are skipped).
pub fn lint_node(node: &Node, body: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if !node.has_frontmatter {
        out.push(Finding::error("no frontmatter block"));
        return out;
    }
    if !node.name_given {
        out.push(Finding::error("missing required field: name"));
    }
    if node.description.is_empty() {
        out.push(Finding::error("missing required field: description"));
    }
    if node.kind.is_empty() {
        out.push(Finding::error("missing required field: kind"));
    } else if !KINDS.contains(&node.kind.as_str()) {
        out.push(Finding::error(format!(
            "invalid kind: {} (want tool|agent|knowledge|workflow)",
            node.kind
        )));
    }
    if node.kind == "tool" && node.run.is_empty() {
        out.push(Finding::error("tool has no `run:` recipe"));
    }
    if !node.danger.is_empty() && !DANGERS.contains(&node.danger.as_str()) {
        out.push(Finding::error(format!(
            "invalid danger: {} (want none|low|medium|high)",
            node.danger
        )));
    }
    if (node.kind == "tool" || node.kind == "workflow") && node.use_when.is_empty() {
        out.push(Finding::warn(
            "no use_when (retrieval leans on description alone)",
        ));
    }
    for m in platform_errors(body) {
        out.push(Finding::error(m));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jd;

    fn block(inner: &str) -> String {
        format!("---\nkind: tool\n---\n\n```just\n{inner}\n```\n")
    }

    #[test]
    fn flags_overlapping_variants() {
        // [unix, wsl] and [wsl] both match a wsl host → duplicate `r`
        let errs = platform_errors(&block("[unix, wsl]\nr:\n  a\n[wsl]\nr:\n  b"));
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("recipe `r`") && errs[0].contains("[wsl]"));
    }

    #[test]
    fn accepts_mutually_exclusive_variants() {
        let errs = platform_errors(&block(
            "[unix, wsl]\nr:\n  a\n[macos]\nr:\n  b\n[windows]\nr:\n  c",
        ));
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn ignores_files_without_the_convention() {
        assert!(platform_errors(&block("a:\n  one\nb:\n  two")).is_empty());
    }

    #[test]
    fn recipe_name_detects_headers_only() {
        assert_eq!(recipe_name("open target:").as_deref(), Some("open"));
        assert_eq!(
            recipe_name("check host count=\"5\":").as_deref(),
            Some("check")
        );
        assert_eq!(recipe_name("  xdg-open x"), None); // indented body
        assert_eq!(recipe_name("# comment"), None);
        assert_eq!(recipe_name("[unix]"), None);
        assert_eq!(recipe_name("x := 1"), None); // assignment
    }

    #[test]
    fn lint_node_flags_missing_required_fields() {
        let n = jd::parse("library/x/foo.jd", "---\nkind: tool\n---\nbody\n");
        let findings = lint_node(&n, "body\n");
        let msgs: Vec<&str> = findings.iter().map(|f| f.message.as_str()).collect();
        // name missing (key fallback means name_given=false), description missing,
        // and a tool with no run recipe.
        assert!(msgs.iter().any(|m| m.contains("missing required field: name")));
        assert!(msgs
            .iter()
            .any(|m| m.contains("missing required field: description")));
        assert!(msgs.iter().any(|m| m.contains("tool has no `run:` recipe")));
    }

    #[test]
    fn lint_node_flags_bad_enums() {
        let n = jd::parse(
            "library/x/foo.jd",
            "---\nname: foo\ndescription: d\nkind: gadget\ndanger: spicy\nrun: go\n---\nbody\n",
        );
        let msgs: Vec<String> = lint_node(&n, "body\n")
            .iter()
            .map(|f| f.message.clone())
            .collect();
        assert!(msgs.iter().any(|m| m.contains("invalid kind: gadget")));
        assert!(msgs.iter().any(|m| m.contains("invalid danger: spicy")));
    }

    #[test]
    fn lint_node_no_frontmatter_is_only_finding() {
        let n = jd::parse("library/x/foo.jd", "no frontmatter here\n");
        let f = lint_node(&n, "no frontmatter here\n");
        assert_eq!(f.len(), 1);
        assert!(f[0].is_error() && f[0].message.contains("no frontmatter"));
    }

    #[test]
    fn lint_node_clean_tool_has_no_errors() {
        let n = jd::parse(
            "library/x/foo.jd",
            "---\nname: foo\ndescription: does a thing\nkind: tool\nrun: go\nuse_when: [do a thing]\n---\nbody\n",
        );
        assert!(lint_node(&n, "body\n").iter().all(|f| !f.is_error()));
    }
}
