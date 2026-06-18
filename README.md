# justdown

A `.jd` file is a small Markdown file with optional executable or scaffolded
blocks. It composes four things you already know — Markdown, YAML frontmatter,
[just](https://just.systems), and a scaffold dialect (PSAIDO) — so **one file
serves four readers without copies**:

- **humans** read the rendered Markdown
- **indexers** index only the YAML frontmatter
- **agents** read the Markdown body after retrieval
- **runners** extract and execute fenced ```` ```just ```` blocks

> justdown is a **task runner and a tool maker in one file**. Authoring a `.jd`
> tool-shard *is* making the executable thing — no separate tool implementation,
> no MCP server per capability, no hand-written function to keep in sync with
> its docs. The `just` recipe in the fenced block *is* the tool; the prose
> around it is the *why* and *when*; the frontmatter is the retrieval contract
> that decides when it gets pulled.

The runner interface is one stable shape for every tool:

```text
just --justfile - <recipe> -- <args...>
```

Arguments in, **exit code out**. A non-zero exit is a failure. How the result is
delivered (stdout / a live sidecar / a written path) is the recipe's
*invocation mode*, declared once in frontmatter.

## Status

Specification v0.1 — early and intentionally small. The format is designed to
stay thin; the entire execution glue is one parser extension that lifts
```` ```just ```` fences out of a `.jd` file and feeds them to `just`.

## Contents

- [`justdown.md`](justdown.md) — the full language specification (v0.1).
- [`examples/`](examples/) — twenty `.jd` shards exercising every `kind`
  (`tool`, `agent`, `knowledge`, `workflow`) and every invocation mode
  (`run`, `sidecar`, `artifact`). Each is minimal and self-documenting; see
  [`examples/README.md`](examples/README.md) for the index.

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

## A complete tool file

````markdown
---
name: release
description: Cut and publish a release. Use when the user asks to ship a version.
kind: tool
tags: [release, publish, ci]
run: release
---

# Release

Builds, gates, tags, and publishes. Runs the full check first; refuses on a red
gate. For the gate itself see @tools/gate.

```just
release version="patch":
  just gate
  npm version {{version}}
  git push --follow-tags
```
````

## Reading order

1. This README.
2. [`justdown.md`](justdown.md) — the spec, end to end.
3. [`examples/`](examples/) — see it on disk. Start with
   [`examples/tools/gate.jd`](examples/tools/gate.jd) (a plain `run` tool),
   then [`examples/tools/serve.jd`](examples/tools/serve.jd) (`sidecar`) and
   [`examples/tools/report.jd`](examples/tools/report.jd) (`artifact`).

## License

MIT — see [`LICENSE`](LICENSE).
