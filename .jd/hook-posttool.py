#!/usr/bin/env python3
"""PostToolUse hook: after a Bash command runs, count its signature. When a
non-trivial command recurs (churn) and no jd recipe covers it, suggest once that
it be crystallized into a .jd tool — the justdown flywheel, done deterministically.

Counts persist in .jd/candidates.tsv (signature<TAB>count). The suggestion fires
exactly once, when the count first reaches THRESHOLD.
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hook_lib import read_event, signature, covered, emit_context, TRIVIAL, is_implementer  # noqa: E402

THRESHOLD = 3


def load(path):
    counts = {}
    try:
        with open(path) as f:
            for line in f:
                sig, _, n = line.rstrip("\n").rpartition("\t")
                if sig:
                    counts[sig] = int(n)
    except FileNotFoundError:
        pass
    except Exception:
        return None
    return counts


def save(path, counts):
    try:
        with open(path, "w") as f:
            for sig, n in sorted(counts.items(), key=lambda kv: -kv[1]):
                f.write(f"{sig}\t{n}\n")
    except Exception:
        pass


def main():
    if is_implementer():
        return
    ev = read_event()
    if ev.get("tool_name") != "Bash":
        return
    command = (ev.get("tool_input") or {}).get("command", "")
    sig = signature(command)
    if not sig or sig.split()[0] in TRIVIAL:
        return

    cwd = ev.get("cwd") or os.getcwd()
    path = os.path.join(cwd, ".jd", "candidates.tsv")
    counts = load(path)
    if counts is None:
        return

    counts[sig] = counts.get(sig, 0) + 1
    n = counts[sig]
    save(path, counts)

    if n == THRESHOLD and not covered(command):
        emit_context("PostToolUse",
            f"Churn: `{sig}` has run {n}× with no jd recipe covering it. "
            f"Consider crystallizing it into a .jd tool (see .claude/skills/jd/, "
            f"ref-justdown.md) so future runs skip the model. Logged in .jd/candidates.tsv.")


if __name__ == "__main__":
    main()
