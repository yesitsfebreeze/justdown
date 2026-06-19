# INSTALL — one repo, four things at once

This repo *is* the package: an **MCP server**, a **tool library**, its
**documentation**, and a **plugin** — distributed as nothing but files in git.
There is no registry and no build artifact to download. You hand your coding
agent one link (or a local path) and it wires itself up.

- spec & overview → [`README.md`](README.md)
- how to author & run a `.jd` tool → [`HELP.md`](HELP.md)
- the files → [`library/`](library/)
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

`mcp.mjs` and `graph.json` sit at the repo root, and each file under
`library/` is reachable at `…/<branch>/library/<dir>/<name>.jd`. So the agent
never needs the whole tree — it fetches exactly the file a query points at.

## Register the MCP — any agent that speaks MCP

`mcp.mjs` is a **single, zero-dependency Node file** (Node 18+). It is both the
server and the graph builder; it needs no `npm install` and no model. Point your
agent's MCP config at it.

**From a URL — nothing on disk (recommended).** You configure *only the raw URL*.
`node` fetches `mcp.mjs` over HTTP and imports it from memory; the server then
loads `graph.json` and every file body straight from the raw git links. No clone,
no copied file, no `npm install`.

```jsonc
{
  "mcpServers": {
    "justdown": {
      "command": "node",
      "args": [
        "--input-type=module",
        "-e",
        "const u='https://raw.githubusercontent.com/yesitsfebreeze/justdown/main/mcp.mjs';await import('data:text/javascript;base64,'+Buffer.from(await (await fetch(u)).text()).toString('base64'))"
      ],
      "env": {
        "JUSTDOWN_REPO": "yesitsfebreeze/justdown",
        "JUSTDOWN_BRANCH": "main"
      }
    }
  }
}
```

The agent flow is: **paste the raw URL → register it → done.** To run a **fork**,
change the URL `u` *and* set `JUSTDOWN_REPO`/`JUSTDOWN_BRANCH` to match — the first
controls where the server code is fetched from, the env vars control where its
`graph.json` and file bodies are fetched from. For the canonical repo the env
block is optional (these are the defaults); it is shown for clarity.

> **Why not `args: ["https://…/mcp.mjs"]`?** Node only runs *local file paths* as
> its entry script — given a URL it tries to open a file literally named `https:/…`
> and fails. And a GitHub `…/blob/…` link is an HTML page, not the source; the
> runnable file is always the **raw** link (`raw.githubusercontent.com/…`). The
> `node -e` one-liner above is the supported, dependency-free way to "just run the
> URL": it fetches the raw source and imports it. Requires Node 18+ (for `fetch`).

**From a local clone** (offline; reads `graph.json` and files from disk):

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

When `mcp.mjs` finds a sibling `graph.json` (or the file file) on disk it uses
it; otherwise it falls back to the raw git link. So the *same* file works whether
the repo is remote or local — that is the "could also be local" case.

### Config knobs (all optional)

| Env var | Default | Meaning |
|---------|---------|---------|
| `JUSTDOWN_REPO` | `yesitsfebreeze/justdown` | `owner/repo` for the raw base URL |
| `JUSTDOWN_BRANCH` | `main` | branch for the raw base URL |
| `JUSTDOWN_RAW_BASE` | derived from repo+branch | override the whole raw base |
| `JUSTDOWN_GRAPH` | sibling file, else `…/graph.json` | explicit graph URL or path |
| `JUSTDOWN_LIB` | `library` | file folder (for `--build`) |

## What the agent gets — a flat graph as a tool

The MCP exposes the library as one **flat, queryable graph**. Every file is a
node carrying its retrieval contract (the frontmatter) plus a **sparse, quantized
term-vector** — a tiny "embed" whose keys are plain words. Scoring is an integer
dot-product, so query is fast and needs no model. Edges are the `@`links between
files. The keys double as human-readable **categories**, so `graph.json` reads
back as named groups with no decoder.

Tools:

| Tool | Does |
|------|------|
| `search` | rank files by a natural-language query; returns names, purposes, and raw git links |
| `get` | fetch a file's full `.jd` body by name (over the raw link, or from disk when local) |
| `categories` | list the named categories and their member files |
| `neighbors` | the inbound/outbound `@`links of a file |

A typical loop: `search` for a need → read the returned purpose → `get` the file
body → run its recipe with the runner interface from [`HELP.md`](HELP.md)
(`just --justfile - <recipe> -- <args...>`).

## Rebuilding the graph

`graph.json` is committed, so consumers never build it. When you add or edit
files, regenerate it from the local library:

```sh
node mcp.mjs --build            # library/ → graph.json
node mcp.mjs --build library out.json   # explicit in/out
```

Commit the refreshed `graph.json` alongside the file change.

## License

MIT — see [`LICENSE`](LICENSE).
