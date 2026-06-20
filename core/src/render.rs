// Context injection for .jd files: `<<name>>` escapes are replaced with values
// the host supplies (the wrapping shell, cwd, the last command, …) before the
// file is consumed. The inbound counterpart to `@`: where `@` pulls in another
// file, `<<var>>` pulls in live host state. The jd spec (justdown.md, "Context
// injection") fixes only the syntax; the variable namespace is the host's.
//
// Three properties matter, straight from the spec:
//   1. Single-pass, non-recursive. Substitution runs once over the authored
//      text; injected values are spliced verbatim and NEVER re-scanned. A value
//      that itself contains `<<…>>` (e.g. captured terminal output) does not
//      trigger a second substitution — the safety property for untrusted state.
//   2. Degrade, never fail. Unknown names and malformed escapes are left exactly
//      as written; bad input is a no-op.
//   3. A literal `<<` is writable: `<<<<` emits a single `<<`.
//
// `<<var>>` never collides with just's own `{{var}}` — different delimiters,
// resolved at different times — so `{{ }}` is left untouched.

use std::collections::BTreeMap;

/// A variable map for [`render`]. Keys are `[A-Za-z0-9_]+`; values are spliced
/// in verbatim.
pub type Vars = BTreeMap<String, String>;

/// True for a syntactically valid escape name: non-empty, ASCII alphanumeric or
/// underscore. Keeps `<< foo >>`, `<<a b>>`, and `<<>>` from being treated as
/// substitutions (they pass through untouched).
fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Replace every `<<name>>` escape in `template` with `vars[name]`. Unknown
/// names and malformed escapes pass through verbatim; `<<<<` emits a literal
/// `<<`. All delimiters are ASCII, so slicing stays on UTF-8 boundaries.
pub fn render(template: &str, vars: &Vars) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(pos) = rest.find("<<") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        // `<<<<` → a literal `<<`. Consume one extra `<<` and emit it raw; the
        // splice is never re-examined, so the emitted `<<` can't start a new
        // escape on this pass.
        if let Some(tail) = after.strip_prefix("<<") {
            out.push_str("<<");
            rest = tail;
            continue;
        }
        // Try to read `<<name>>`.
        if let Some(close) = after.find(">>") {
            let name = &after[..close];
            if is_valid_name(name) {
                match vars.get(name) {
                    Some(val) => out.push_str(val),
                    // Unknown var: leave the whole escape exactly as authored.
                    None => {
                        out.push_str("<<");
                        out.push_str(name);
                        out.push_str(">>");
                    }
                }
                rest = &after[close + 2..];
                continue;
            }
        }
        // Not a well-formed escape: emit the `<<` literally and walk on.
        out.push_str("<<");
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vars {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn substitutes_known_vars() {
        let v = vars(&[("shell", "nu"), ("cwd", "/tmp")]);
        assert_eq!(
            render("shell=<<shell>> cwd=<<cwd>>", &v),
            "shell=nu cwd=/tmp"
        );
    }

    #[test]
    fn unknown_var_left_verbatim() {
        let v = vars(&[("shell", "nu")]);
        assert_eq!(render("<<shell>> <<missing>>", &v), "nu <<missing>>");
    }

    #[test]
    fn literal_double_angle_via_quad() {
        let v = vars(&[("shell", "nu")]);
        // `<<<<shell>>` → literal `<<` + `shell>>` text = `<<shell>>`.
        assert_eq!(render("<<<<shell>>", &v), "<<shell>>");
    }

    #[test]
    fn injected_value_is_not_rescanned() {
        // Untrusted terminal content that itself contains an escape must NOT be
        // expanded a second time — the safety property for injected host state.
        let v = vars(&[("screen", "danger <<shell>>"), ("shell", "nu")]);
        assert_eq!(render("<<screen>>", &v), "danger <<shell>>");
    }

    #[test]
    fn malformed_escapes_pass_through() {
        let v = vars(&[("a", "X")]);
        assert_eq!(
            render("<< a >> <<a b>> <<>> <<a", &v),
            "<< a >> <<a b>> <<>> <<a"
        );
    }

    #[test]
    fn leaves_just_interpolation_untouched() {
        // just's own `{{var}}` is a different delimiter and must survive verbatim.
        let v = vars(&[("t", "file.txt")]);
        assert_eq!(render("open {{t}} <<t>>", &v), "open {{t}} file.txt");
    }

    #[test]
    fn no_escapes_is_identity() {
        assert_eq!(
            render("plain text {curly} ${sh}", &Vars::new()),
            "plain text {curly} ${sh}"
        );
    }
}
