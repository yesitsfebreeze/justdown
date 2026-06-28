#!/usr/bin/env python3
"""PreToolUse hook (Bash + justdown MCP search/get): search-once, then use.

jd is searched once per turn by the UserPromptSubmit auto-search. From there:

- A second search this turn (Bash `jd search` or the MCP `search` tool) is
  redundant — the candidates are already in context. DENY; `get` one instead.
- A raw shell command a jd recipe covers is DENIED **until a recipe is fetched
  this turn** (`jd get` / the MCP `get` tool), then allowed. One consult per
  turn releases the gate — so covered commands stay runnable (recipes are
  knowledge, not executors) while the consult is still forced.

Trivial navigation/file commands are never gated. Fails open: any error returns
without a decision so a real failure never bricks the tool.
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hook_lib import (  # noqa: E402
    read_event, first_binary, gate_hits, emit_context, emit_deny,
    is_jd_search, is_jd_get, mark_consulted, turn_consulted,
    turn_has_candidates, TRIVIAL, is_implementer,
)

SEARCH_TOOL = "mcp__plugin_justdown_justdown__search"
GET_TOOL = "mcp__plugin_justdown_justdown__get"
ALREADY = ("jd already surfaced candidates this turn (the list above the last "
           "user message). Fetch one with the justdown `get` MCP tool — do not "
           "search again.")


def main():
    if is_implementer():
        return
    ev = read_event()
    tool = ev.get("tool_name")

    if tool == GET_TOOL:
        mark_consulted()
        return
    if tool == SEARCH_TOOL:
        if turn_has_candidates():
            emit_deny("PreToolUse", ALREADY)
        return

    if tool != "Bash":
        return
    command = (ev.get("tool_input") or {}).get("command", "")

    if is_jd_get(command):
        mark_consulted()
        return
    if is_jd_search(command) and turn_has_candidates():
        emit_deny("PreToolUse", ALREADY)
        return

    binary = first_binary(command)
    if not binary or binary in TRIVIAL:
        return

    hits = gate_hits(command)
    if hits and not turn_consulted():
        lines = ["A justdown recipe covers this — fetch it with the justdown "
                 "`get` MCP tool and use it instead of raw shell:"]
        for r in hits[:3]:
            lines.append(f"- {r['name']}: {r.get('purpose', '').split('.')[0]}.")
        lines.append("(Consulting any recipe this turn releases this gate. Run "
                     "shell directly only if jd has no recipe for the command.)")
        emit_deny("PreToolUse", "\n".join(lines))
        return

    if not hits:
        emit_context("PreToolUse",
            f"No jd tool covers `{binary}`. If this is reusable shell work, author one in the "
            f"local library (see the /jd skill): draft a blueprint (name, use case, recipe), "
            f"confirm it's genuinely new, then write "
            f".jd/library/<category>/<name>.jd and run `jd build`.")


if __name__ == "__main__":
    main()
