"""Shared helpers for the justdown PreToolUse hook.

Talks to the project-local jd store (JUSTDOWN_ROOT = <project>/.jd), which jd
merges over the online library (local > online). Everything fails open: any
error returns empty so a hook never blocks a tool call.
"""
import json
import os
import shutil
import subprocess
import sys

ROOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), ".jd")
JD = shutil.which("jd") or os.path.expanduser("~/.cargo/bin/jd")


def _project_root():
    return os.environ.get("CLAUDE_PROJECT_DIR") or os.path.dirname(ROOT)


# Per-turn markers under .claude/ (both reset each prompt by the auto-search):
#   .jd-turn      — auto-search surfaced candidates → a 2nd search is redundant.
#   .jd-consulted — a recipe was fetched this turn → release the covered-shell gate.
def turn_has_candidates():
    return os.path.exists(os.path.join(_project_root(), ".claude", ".jd-turn"))


def turn_consulted():
    return os.path.exists(os.path.join(_project_root(), ".claude", ".jd-consulted"))


def mark_consulted():
    try:
        open(os.path.join(_project_root(), ".claude", ".jd-consulted"), "w").close()
    except OSError:
        pass

# Interactive / navigation / inspection commands not worth a jd tool.
TRIVIAL = {
    "cd", "ls", "ll", "pwd", "echo", "cat", "head", "tail", "less", "more",
    "which", "type", "man", "clear", "export", "set", "env", "true", "false",
    "jd", "just", "code", "touch", "sleep", "exit", "printf", "test",
    "rm", "mv", "cp", "mkdir", "rmdir", "ln", "chmod", "chown", "grep", "find",
}


def read_event():
    try:
        return json.load(sys.stdin)
    except Exception:
        return {}


def is_implementer():
    """True if `.claude/role` is `implementer` (written by hook-session.sh)."""
    root = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    try:
        with open(os.path.join(root, ".claude", "role")) as f:
            return f.read().strip() == "implementer"
    except OSError:
        return False


def jd_env():
    e = dict(os.environ)
    e.setdefault("JUSTDOWN_ROOT", ROOT)
    return e


def jd_search(query, num=5):
    """Parsed jd search results (local merged over online), or [] on failure."""
    if not query.strip():
        return []
    try:
        p = subprocess.run(
            [JD, "search", query, "", str(num), "--json"],
            capture_output=True, text=True, timeout=10, env=jd_env(),
        )
        return (json.loads(p.stdout or "{}").get("results") or [])
    except Exception:
        return []


def _payload_tokens(command):
    """Command tokens with leading VAR=val assignments and wrappers stripped."""
    out = []
    for tok in command.split():
        if "=" in tok and not tok.startswith("-"):
            continue
        if tok in ("sudo", "env", "command", "nohup", "time"):
            continue
        out.append(tok)
    return out


def first_binary(command):
    """The invoked binary, basename only."""
    toks = _payload_tokens(command)
    return toks[0].rsplit("/", 1)[-1] if toks else ""


# Tools whose first subcommand meaningfully partitions usage (git commit vs
# git push). For everything else the binary alone is the churn signature.
SUBCOMMAND_TOOLS = {
    "git", "docker", "podman", "cargo", "npm", "pnpm", "yarn", "go", "gh",
    "kubectl", "pip", "pip3", "apt", "apt-get", "dnf", "pacman", "brew",
    "systemctl", "just", "make", "terraform", "helm", "nix", "deno", "bun",
}


def signature(command):
    """A stable churn signature: the binary (basename), plus its subcommand for
    known multiplexer tools. Arguments and values are dropped so repeat runs of
    the same operation collapse onto one signature."""
    toks = _payload_tokens(command)
    if not toks:
        return ""
    binary = toks[0].rsplit("/", 1)[-1]
    if binary in SUBCOMMAND_TOOLS:
        for tok in toks[1:]:
            if tok.startswith("-"):
                continue
            if tok.replace("-", "").replace(":", "").isalnum():
                return f"{binary} {tok}"
            break
    return binary


def covered(command):
    """True if a jd recipe genuinely covers this command (subcommand-aware)."""
    return bool(gate_hits(command))


def gate_hits(command):
    """Recipes that genuinely cover THIS command. For multiplexer tools the
    recipe must also match the subcommand, so `git diff` is not blocked by an
    unrelated `git clone` recipe (bare-binary match is too coarse for them)."""
    binary = first_binary(command)
    if not binary:
        return []
    hits = relevant(jd_search(command), binary)
    if binary not in SUBCOMMAND_TOOLS:
        return hits
    sig = signature(command)
    sub = sig.split(" ", 1)[1] if " " in sig else ""
    if not sub:
        return []
    out = []
    for r in hits:
        hay = " ".join([
            r.get("name", ""), r.get("purpose", ""),
            " ".join(r.get("requires", [])), " ".join(r.get("tags", [])),
        ]).lower()
        if sub in hay:
            out.append(r)
    return out


def relevant(results, binary):
    """Results whose name/tags/requires mention the command's binary — the
    precise 'this tool is for this command' signal, straight off jd's output."""
    b = binary.lower()
    out = []
    for r in results:
        hay = " ".join([
            r.get("name", ""),
            " ".join(r.get("requires", [])),
            " ".join(r.get("tags", [])),
        ]).lower()
        if b and b in hay:
            out.append(r)
    return out


def emit_context(event_name, text):
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": event_name, "additionalContext": text,
    }}))


def emit_deny(event_name, reason):
    """Hard-block the tool call. The agent sees `reason` and must change course."""
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": event_name,
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }}))


def _jd_subcommand(command, sub):
    toks = _payload_tokens(command)
    return len(toks) >= 2 and toks[0].rsplit("/", 1)[-1] == "jd" and toks[1] == sub


def is_jd_search(command):
    """True if the shell command shells out to `jd search`."""
    return _jd_subcommand(command, "search")


def is_jd_get(command):
    """True if the shell command shells out to `jd get`."""
    return _jd_subcommand(command, "get")
