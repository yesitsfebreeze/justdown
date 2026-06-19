# justdown library

Each `.jd` file here is a small, single-purpose file. Together they exercise
every `kind` and the main features of the format. They are intentionally minimal
and self-documenting; read the frontmatter for the retrieval contract, the prose
for the *why*, and the fenced blocks for the *how*.

This folder is also the corpus behind [`../graph.tsv`](../graph.tsv) — the flat,
tab-separated index the CLI queries, built from these files by `just build`. See
[`../install.jd`](../install.jd) to download the CLI and wire it into your agent.

| File | kind | Demonstrates |
|------|------|--------------|
| `tools/gate.jd` | tool | a plain `just` tool with one recipe and a red/green gate |
| `tools/release.jd` | tool | a tool that depends on another file's recipe (`@tools/gate`) via a just dependency |
| `tools/fmt.jd` | tool | a tool with **multiple recipes** and a `run` default |
| `tools/commit.jd` | tool | a thin git tool — conventional commits from typed args |
| `tools/db.jd` | tool | a recipe family (migrate/rollback/seed/studio/reset) over a real CLI |
| `tools/docker.jd` | tool | a recipe family (build/up/down/logs/shell) delegating to `docker compose` |
| `tools/new-tool.jd` | tool | a meta tool that scaffolds a new `.jd` file from a heredoc template |
| `tools/serve.jd` | tool | **`invoke: sidecar`** — a long-running dev server watched via `READY`, plus one-shot recipes |
| `tools/report.jd` | tool | **`invoke: artifact`** — large/structured reports written to a file, announced by `ARTIFACT <path>` |
| `tools/screenshot.jd` | tool | **`invoke: artifact`** — binary PNG/PDF output that cannot go on stdout |
| `agents/review.jd` | agent | an agent file that delegates to a CLI (`gh`) from its recipe |
| `agents/summarize.jd` | agent | an agent file with a required arg and a no-default usage error |
| `workflows/ship.jd` | workflow | composes `@tools/gate` + `@tools/release` via just dependencies, passing the version through (`gate (release version)`) |
| `workflows/onboard.jd` | workflow | composes in-file `install`/`env` with `@tools/db` `migrate`/`seed` as `&&` post-dependencies |
| `knowledge/orders.jd` | knowledge | a knowledge file with a `psaido` scaffold and `provides` |
| `knowledge/product.jd` | knowledge | a companion knowledge file referenced via `@knowledge/product#Product` |
| `scaffolds/auth.jd` | knowledge | scaffolds showing `!im ... as`, inline `@` links, nesting, control flow |
| `scaffolds/pagination.jd` | knowledge | a scaffold with generics (`[any]`), slices, and round-up math |
| `scaffolds/result.jd` | knowledge | a convention scaffold (`Ok`/`Err`/`map`/`recover`) using functions as values |
| `scaffolds/cache.jd` | knowledge | a scaffold that `!im @scaffolds/result as r` and builds a TTL cache |

Two tools use non-default invocation modes — the contract for how the runner
spawns a recipe and reads its result back (`invoke` in the spec):

- `tools/serve.jd` — `invoke: sidecar`: a long-running dev server watched via
  `READY`, plus `run`-style one-shot recipes (`reload`, `status`, `stop`).
- `tools/report.jd`, `tools/screenshot.jd` — `invoke: artifact`: the result is a
  file announced by `ARTIFACT <path>`; stdout is logs. Covers large/structured
  and binary outputs that cannot go on stdout.

Conventions used across these files:

- Frontmatter is the **only** thing an index ingests — keep it honest and short.
- `@` links in prose and `psaido` are resolved before the agent sees the file;
  never put `@` inside a `just` recipe body (the runner does not resolve it).
- A `tool` file's `run` field names the default recipe; the runner calls
  `just --justfile - <recipe> <args...>` on the extracted fences.
- Compose across files with a just **dependency** on the bare recipe name
  (`ship: gate (release version)`), not a nested `just gate` in the body. Link
  the other file with `@` so the runner folds its recipes into the same justfile;
  files composed together must not reuse a recipe name.
