#!/usr/bin/env sh
# UserPromptSubmit hook: surface applicable justdown tools for the user's prompt.
# Runs the `jd` CLI's `search` over the prompt and injects any matches as context.
# Fails open: any error exits 0 with no output so the prompt is never blocked.
set -u

root="${CLAUDE_PROJECT_DIR:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)}"
export JUSTDOWN_ROOT="$root/.jd"
marker="$root/.claude/.jd-turn"

# New turn: clear the per-turn markers. `.jd-turn` is rewritten below only if
# candidates surface (presence ⇒ "don't search again"); `.jd-consulted` stays
# cleared until a recipe is fetched this turn (presence ⇒ release the shell gate).
rm -f "$marker" "$root/.claude/.jd-consulted"

# Implementer profile: skip jd surfacing. See .claude/hook-session.sh, glossary `role`.
[ "$(tr -d '[:space:]' <"$root/.claude/role" 2>/dev/null)" = "implementer" ] && exit 0

command -v jd >/dev/null 2>&1 || exit 0

prompt=$(jq -r '.prompt // empty' 2>/dev/null) || exit 0
[ -n "$prompt" ] || exit 0

results=$(jd search "$prompt" "" 5 2>/dev/null) || exit 0
[ -n "$results" ] || exit 0

printf '%s\n' "$results" >"$marker"
printf 'Applicable justdown tools for this request (fetch one with the justdown `get` MCP tool, or `jd get <name>`):\n\n%s\n' "$results"
exit 0
