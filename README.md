# justdown

[![GitHub stars](https://img.shields.io/github/stars/yesitsfebreeze/justdown?style=social)](https://github.com/yesitsfebreeze/justdown/stargazers) [![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![spec](https://img.shields.io/badge/spec-v0.1-blue.svg)](justdown.md)

A `.jd` file is a small Markdown file with optional executable or scaffolded
blocks. It composes four things you already know — Markdown, YAML frontmatter,
[just](https://just.systems), and a scaffold dialect (PSAIDO) — so **one file
serves four readers without copies**:

- **humans** read the rendered Markdown
- **indexers** index only the YAML frontmatter
- **agents** read the Markdown body after retrieval
- **runners** extract and execute fenced ```` ```just ```` blocks

> justdown is a **task runner and a tool maker in one file**. Authoring a `.jd`
> tool-file *is* making the executable thing — no separate tool implementation,
> no MCP server per capability, no hand-written function to keep in sync with
> its docs. The `just` recipe in the fenced block *is* the tool; the prose
> around it is the *why* and *when*; the frontmatter is the retrieval contract
> that decides when it gets pulled.

The runner interface is one stable shape for every tool:

```text
just --justfile - <recipe> <args...>
```

Arguments are **positional** (mapped to the recipe's parameters in order; no `--`
separator). Arguments in, **exit code out**. A non-zero exit is a failure. How the result is
delivered (stdout / a live sidecar / a written path) is the recipe's
*invocation mode*, declared once in frontmatter.

## Status

Specification v0.1 — early and intentionally small. The format is designed to
stay thin; the entire execution glue is one parser extension that lifts
```` ```just ```` fences out of a `.jd` file and feeds them to `just`.

## Contents

- [`justdown.md`](justdown.md) — the full language specification (v0.1).
- [`install.jd`](install.jd) — install and use justdown: install the `jd`
  binary, what tools it gives, and how to wire it into an agent.
- [`cli/`](cli/) — the CLI itself: **`jd`**, a small self-contained Rust binary
  (`search`, `get`, `ls`, `links`, `path`, `build`, `lint`) over the library graph.
- [`library/`](library/) — `.jd` files exercising every `kind`
  (`tool`, `agent`, `knowledge`, `workflow`) and every invocation mode
  (`run`, `sidecar`, `artifact`). Each is minimal and self-documenting.
- [`graph.db`](graph.db) — the SQLite graph store the CLI queries: nodes carrying
  the retrieval contract + safety metadata, and resolved `@`link edges (category
  inferred from the parent folder). Built by `jd build`; CI keeps it fresh on
  every push. No node anywhere.

## Use it as a CLI

The repo *is* the package — a CLI, a tool library, and docs in git. Install one
binary, **`jd`**, and it becomes your lookup over the library. `jd` **builds** a
SQLite graph store ([`graph.db`](graph.db)) from `.jd` files and **queries** it,
**merging your local store over the online one** (local trumps by key). No clone,
no `npm install`, no node, no model.

```sh
# install: prebuilt binary (Linux/macOS) — verifies checksum, drops jd in ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/yesitsfebreeze/justdown/main/scripts/install.sh | sh
# …or from source, any platform (Rust toolchain)
cargo install --git https://github.com/yesitsfebreeze/justdown jd

# use it
jd search "cut a release"   # find a tool
jd get release              # read it as sections
jd get release tools        # just the runnable steps
```

`jd` finds tools; [`just`](https://just.systems) runs them (a tool's recipe is
just-syntax, executed as `just --justfile - <recipe>`). `jd` **does not define how
it is used** — an agent can call the verbs directly, or wrap them (`search`,
`get`, `ls`, `links`, `path`) as an MCP tool lookup. The library is the contract;
the wiring is yours. See [`install.jd`](install.jd).

## Why

MCP servers, hand-written tool functions, and copy-pasted docs all drift from
the code they describe. A `.jd` file collapses the tool, its docs, and its
retrieval contract into one source of truth:

- the **frontmatter** is the only thing an index ingests (the *when*)
- the **Markdown body** is the manual an agent reads once pulled (the *why*)
- the **fenced `just` block** is the executable (the *how*)

Heavy logic still lives in real scripts on disk; a `just` recipe is the thin,
named, parameterized entry point that delegates to them. Swap the backend
(`gh`, `claude`, `prisma`, `docker compose`, an HTTP call, another recipe) and
the runner's interface stays identical.

## The model in one breath

A `.jd` file has three regions, each a different surface for a different reader:

1. **frontmatter** — the retrieval contract (indexed surface)
2. **Markdown body** — the instruction manual (agent reads on selection)
3. **fenced blocks** — the payloads: ```` ```just ```` recipes a runner executes,
   or ```` ```psaido ```` scaffolds a model translates (never run)

`@` links in prose and scaffolds resolve to other files **before** content
reaches the agent, so references are hydrated deterministically — the runner
never resolves `@`, and `@` never appears inside a `just` recipe body.

`<<var>>` escapes are the inbound counterpart: the host splices in live state —
the wrapping shell, cwd, the last command — **before** the text reaches the agent
or shell. One pass, no re-scanning, so an injected value can't smuggle in more
escapes; unknown names pass through untouched and `<<<<` is a literal `<<`. It
won't collide with just's `{{var}}` — different delimiters, resolved at different
times.

## A complete tool file

````markdown
---
name: tools_release
description: Cut and publish a release. Use when the user asks to ship a version.
kind: tool
tags: [release, publish, ci]
run: release
---

# Release

Builds, gates, tags, and publishes. `gate` runs first as a just dependency, so a
red gate aborts before `npm version`. The runner folds the linked file's recipe
into the justfile, so the dependency resolves. For the gate itself see @tools/gate.

```just
release version="patch": gate
  npm version {{version}}
  git push --follow-tags
```
````

## Reading order

1. This README.
2. [`justdown.md`](justdown.md) — the spec, end to end.
3. [`library/`](library/) — see it on disk. Start with
   [`library/tools/gate.jd`](library/tools/gate.jd) (a plain `run` tool),
   then [`library/tools/serve.jd`](library/tools/serve.jd) (`sidecar`) and
   [`library/tools/report.jd`](library/tools/report.jd) (`artifact`).

## Star history

[![Star History Chart](https://api.star-history.com/svg?repos=yesitsfebreeze/justdown&type=Date)](https://star-history.com/#yesitsfebreeze/justdown&Date)

## License

MIT — see [`LICENSE`](LICENSE).
