# INSTALL — one repo, four things at once

This repo *is* the package: an **MCP server**, a **tool library**, its
**documentation**, and a **plugin** — distributed as nothing but files in git.
There is no registry and no build artifact to download. You hand your coding
agent one link (or a local path) and it wires itself up.

- spec & overview → [`README.md`](README.md)
- how to author & run a `.jd` tool → [`HELP.md`](HELP.md)
- the shards → [`library/`](library/)
- the queryable graph → [`graph.json`](graph.json)
- the server → [`mcp.mjs`](mcp.mjs)

## Prelude — what your agent needs to know

The only input is **where this repo lives**. That is either:

- a **git URL** you paste into your agent (e.g.
  `https://github.com/yesitsfebreeze/justdown`), or
- a **local path** to a clone (`/path/to/justdown`).

From that one location everything else is derivable, because every file is
addressable by its **raw git link**:

```
https://raw.githubusercontent.com/<owner>/<repo>/<branch>/<path>
```

`mcp.mjs` and `graph.json` sit at the repo root, and each shard under
`library/` is reachable at `…/<branch>/library/<dir>/<name>.jd`. So the agent
never needs the whole tree — it fetches exactly the file a query points at.

## Register the MCP — any agent that speaks MCP

`mcp.mjs` is a **single, zero-dependency Node file** (Node 18+). It is both the
server and the graph builder; it needs no `npm install` and no model. Point your
agent's MCP config at it.

**From a URL** (fetch the file once, then run it locally over stdio):

```jsonc
{
  "mcpServers": {
    "justdown": {
      "command": "node",
      "args": ["mcp.mjs"],
      "env": {
        "JUSTDOWN_REPO": "yesitsfebreeze/justdown",
        "JUSTDOWN_BRANCH": "main"
      }
    }
  }
}
```

The agent flow is: **grab the repo location → fetch `mcp.mjs` → register it as an
MCP server → done.** With only `JUSTDOWN_REPO`/`JUSTDOWN_BRANCH` set, the server
loads `graph.json` and every shard body straight from the raw git links — no
clone required.

**From a local clone** (offline; reads `graph.json` and shards from disk):

```jsonc
{
  "mcpServers": {
    "justdown": {
      "command": "node",
      "args": ["/path/to/justdown/mcp.mjs"]
    }
  }
}
```

When `mcp.mjs` finds a sibling `graph.json` (or the shard file) on disk it uses
it; otherwise it falls back to the raw git link. So the *same* file works whether
the repo is remote or local — that is the "could also be local" case.

### Config knobs (all optional)

| Env var | Default | Meaning |
|---------|---------|---------|
| `JUSTDOWN_REPO` | `yesitsfebreeze/justdown` | `owner/repo` for the raw base URL |
| `JUSTDOWN_BRANCH` | `main` | branch for the raw base URL |
| `JUSTDOWN_RAW_BASE` | derived from repo+branch | override the whole raw base |
| `JUSTDOWN_GRAPH` | sibling file, else `…/graph.json` | explicit graph URL or path |
| `JUSTDOWN_LIB` | `library` | shard folder (for `--build`) |

## What the agent gets — a flat graph as a tool

The MCP exposes the library as one **flat, queryable graph**. Every shard is a
node carrying its retrieval contract (the frontmatter) plus a **sparse, quantized
term-vector** — a tiny "embed" whose keys are plain words. Scoring is an integer
dot-product, so query is fast and needs no model. Edges are the `@`links between
shards. The keys double as human-readable **categories**, so `graph.json` reads
back as named groups with no decoder.

Tools:

| Tool | Does |
|------|------|
| `search` | rank shards by a natural-language query; returns names, purposes, and raw git links |
| `get` | fetch a shard's full `.jd` body by name (over the raw link, or from disk when local) |
| `categories` | list the named categories and their member shards |
| `neighbors` | the inbound/outbound `@`links of a shard |

A typical loop: `search` for a need → read the returned purpose → `get` the shard
body → run its recipe with the runner interface from [`HELP.md`](HELP.md)
(`just --justfile - <recipe> -- <args...>`).

## Rebuilding the graph

`graph.json` is committed, so consumers never build it. When you add or edit
shards, regenerate it from the local library:

```sh
node mcp.mjs --build            # library/ → graph.json
node mcp.mjs --build library out.json   # explicit in/out
```

Commit the refreshed `graph.json` alongside the shard change.

## License

MIT — see [`LICENSE`](LICENSE).
