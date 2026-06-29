# justdown — Claude Code plugin

This orphan branch carries **only** the Claude Code plugin packaging for
[justdown](https://github.com/yesitsfebreeze/justdown). The jd tool source
(Rust CLI, library, docs) lives on `main`.

## Contents

- `.claude-plugin/` — `plugin.json` + `marketplace.json`
- `hooks/` — `hooks.json` (SessionStart: ensure the `jd` binary)
- `scripts/` — `ensure-jd.sh`, `install.sh`, `install.ps1`

## Install

```sh
claude plugin marketplace add yesitsfebreeze/justdown@claude-plugin
claude plugin install justdown@justdown
```

The plugin wraps the `jd` binary's read verbs as one MCP server. The
SessionStart hook (`scripts/ensure-jd.sh`) makes sure `jd` is on `PATH`.
