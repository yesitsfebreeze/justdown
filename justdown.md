# Justdown (.jd) Language Specification v0.1

A `.jd` file is a small Markdown file with optional executable or scaffolded
blocks. It is **not a new language** — it composes Markdown, YAML frontmatter,
[just](https://just.systems), and PSAIDO so one file can serve four readers
without copies:

- **humans** read rendered Markdown
- **indexers** index only the YAML frontmatter
- **agents** read the Markdown body after retrieval
- **runners** extract and execute fenced ```` ```just ```` blocks

## Intention

justdown is a **task runner and a tool maker in one file**. Authoring a `.jd`
tool-shard *is* making the executable thing — there is no separate tool
implementation, no MCP server per capability, no hand-written tool function to
keep in sync with its docs. The `just` recipe in the fenced block *is* the tool;
the prose around it is the *why* and *when*; the frontmatter is the retrieval
contract that decides when it gets pulled.

The contract is deliberately flexible: a recipe is ordinary shell, so its
backend can be anything reachable from a process — a script on disk, a CLI
(`gh`, `claude`, `codex`), an HTTP call, a language runtime, or another recipe
via `just` dependencies. Heavy logic lives in real scripts on disk; the recipe
is the thin, named, parameterized entry point that delegates to them. This
keeps the runner's interface stable and identical across every tool:

```text
just --justfile - <recipe> -- <args...>
```

A file may hold several recipes; `run:` names the default, the rest are
callable by name. So one shard can expose a family of related entry points
without becoming several tools.

The result contract is correspondingly thin: arguments in, **exit code out**;
a non-zero exit is a failure. **How** the result is delivered — on stdout, via
a live sidecar, or as a written path — is the recipe's *invocation mode*,
declared once in frontmatter. See [Invocation modes](#invocation-modes).
Anything richer is a convention recipes opt into within a mode, not something
the format imposes.

## The model

A `.jd` file has three regions, and each one is a different surface for a
different reader:

1. **frontmatter — the retrieval contract.** The indexed surface. An index
   ingests only this; it decides *when* the file is pulled.
2. **Markdown body — the instruction manual.** Intent, context, usage notes —
   *why* and *when*. Read by the agent once a query selects the file.
3. **fenced blocks — the payloads.** ```` ```just ```` recipes a runner executes,
   or ```` ```psaido ```` scaffolds a model translates. The *how*.

## Frontmatter (the retrieval contract)

The frontmatter is the **only** part an index ingests, keyed by the file's path.
It is the match surface that decides *when* the file is pulled.

| Field | Required | Meaning |
|-------|----------|---------|
| `name` | yes | File identity; also the just module name when relevant. |
| `description` | yes | One or two lines: what it is and when to use it. **This is the contract** — the agent retrieves on it. |
| `kind` | yes | `tool` \| `agent` \| `knowledge` \| `workflow`. |
| `tags` | no | Free-form labels for grouping and retrieval. |
| `run` | tool only | Default recipe entry point, e.g. `release`. The runner invokes it as `just --justfile - release -- <args...>`. |
| `invoke` | tool only | Invocation mode: `run` (default) \| `sidecar` \| `artifact`. Declares how the runner spawns the recipe and reads its result. See [Invocation modes](#invocation-modes). |
| `provides` | no | Names other files link to via `#Name` (schemas, functions, recipes). |

Everything else (the body, the blocks) stays on disk and is read by path only
once a query selects the file.

## Markdown body

Plain Markdown — headings, prose, lists, links. It tells the agent when to reach
for the file and what it does; it never restates the fenced block's mechanics.
Keep a file small and single-purpose.

## The just blocks (recipes)

> run `just --help` for what we can do.


A ```` ```just ```` block is a normal [justfile](https://just.systems) fragment —
recipes, variables, dependencies, parameters. A tool file's `run` frontmatter
names the default recipe.

````markdown
```just
# build, tag, and publish a release
release version="patch":
  npm run check
  npm version {{version}}
  git push --follow-tags
```
````

Arguments follow just's own conventions and are passed through the runner:

```
runner args: ["minor"]  →  release version="minor"
```

A file may hold several recipes; `run` picks the default, and others are
callable by name.

### Why just, not bash

`.jd` needs named, addressable units of work, not one opaque script. just gives
each recipe a name, declared parameters, defaults, and dependencies while keeping
the recipe body as ordinary shell. That gives the runner one stable interface:

```text
just --justfile - <recipe> -- <args...>
```

Heavy logic should still live in real scripts on disk; a just recipe is the thin,
named entry point that delegates to them. just is cross-platform and the
invocation stays identical across Linux, macOS, and Windows — but recipe bodies
are still shell commands, so portable recipes should delegate to cross-platform
scripts or tools.

### Running

A `.jd` file is **not** a native justfile — Markdown prose lines would be just
syntax errors. The runner does one cheap step: it lifts every ```` ```just ````
fence out, concatenates them, and runs the requested recipe.

```
extract ```just fences  →  feed to `just --justfile - <recipe> -- <args>`
```

That extractor — one small parser extension — is the entire execution glue.
```` ```psaido ```` blocks are never extracted or run.

## Invocation modes

`invoke` (default `run`) tells the runner **how to spawn and read back**. The
recipe is always ordinary shell under `just --justfile - <recipe> -- <args...>`;
the mode changes only the process contract around it.

| Mode | Process | Result | Use when |
|------|---------|--------|----------|
| `run` | run to completion | stdout + exit code | the answer fits on stdout and the recipe finishes. Default. |
| `sidecar` | start and stay alive | status via a control channel, read on demand | the value is the running process (server, watcher, proxy). |
| `artifact` | run to completion | a path the recipe writes; stdout is logs | the result can't go on stdout (binary, large, multi-file, structured). |

### `run` (default)

Result is **stdout + exit code**; non-zero is failure. The runner captures
stdout and returns it.

```just
release version="patch":
  just gate
  npm version {{version}}
  git push --follow-tags
```

### `sidecar`

The recipe **does not exit** while useful. The runner spawns it detached and
reads status through a channel, not an exit code (a healthy sidecar never
produces one).

- **Health** — print `READY <endpoint>` on stdout once up; `ERROR ...` is the
  failure path. The runner treats the first `READY` (or a probe of the
  endpoint) as started.
- **Lifecycle** — the runner sends `SIGTERM`; the recipe traps it and shuts down
  cleanly. Alternatively watch a control file/socket/env var the runner toggles.
- **Output** — logs stream to stdout/stderr; the *result* is "running at this
  address", not a payload.

One long-lived recipe (`run:` names it). One-shot commands (reload, status,
stop) go as **separate named recipes in the same shard**, called by name as
normal `run`-style recipes.

```just
# long-running dev server (invoke: sidecar)
serve port="3000":
  @echo "READY http://localhost:{{port}}"
  vite --port {{port}} --host

# one-shot, run-style, callable by name
reload:
  @touch vite.config.ts

status port="3000":
  @curl -fsS http://localhost:{{port}}/health && echo " up" || (echo "down" && exit 1)
```

### `artifact`

Runs to completion, but the **result is a path it writes** — not stdout. Use
when the result is binary, large, multi-file, or structured. The recipe prints
the path as the **last** stdout line:

```
ARTIFACT <path>
```

The runner returns the *path* (not contents) to the agent, who reads the file
by path. Exit code still governs trust — non-zero means no artifact is safe,
even if the line was printed.

```just
chart out="dist/chart.png":
  @mkdir -p dist
  python scripts/render_chart.py --out {{out}}
  @echo "ARTIFACT {{out}}"
```

### Choosing a mode

- Answer on stdout, recipe finishes → **`run`**.
- Stays alive → **`sidecar`**.
- If the result is a file the agent reads by path → **`artifact`**.

All three share one spawn shape — `just --justfile - <recipe> -- <args...>` —
so the runner's interface stays stable; only the read-back differs.

## Scaffold blocks (`psaido`)

Where a file sketches logic for an agent instead of running code, it uses a
```` ```psaido ```` block — the PSAIDO scaffold dialect. It is **read, not
compiled**: a model translates it into the project's real language. It never
reaches the runner; it is context, the same as prose.

A scaffold describes *what* should happen and how the pieces connect, never *how*
a given language implements it. Rough is fine — leave detail to the translator's
judgment; add a keyword only when its absence causes real ambiguity. The
`provides` frontmatter names the schemas and functions other files link to via
`#Name`.

The grammar uses three sigils — `!im`, `!sc`, `!fn` — plus plain statements and
control flow. The full grammar follows.

### Imports

> `!im <path> [as <alias>]`

Pull another file's provided names into scope; alias to shorten a repeated path.

```psaido
!im @db/users as udb
```

### Schemas

> `!sc <Name>` — fields as `- <name>: <type>`

Data shapes, built from the primitives `string`, `number`, `boolean`, `null`,
`any`:

```psaido
!sc User
- id: number
- name: string
- email: string

!sc Product
- id: number
- name: string
- price: number
```

Schemas nest and hold arrays:

```psaido
!sc Order
- id: number
- customer: User
- items: [Product]
- total: number
```

### Functions

> `!fn <Name> > <return type>` — params as `- <name>: <type>`, body indented,
> `< <expr>` returns

```psaido
!fn getName > string
- input: User
< input.name

!fn addNumbers > number
- a: number
- b: number
  result = a + b
< result
```

### Statements

Assignment, calls, construction, and indexing — all the everyday lines. `=` binds
a variable; `!` binds a constant.

```psaido
x ! 5                                   // constant
name = "hello"                          // variable
flag = true

user  = lookupUser(id)                  // call
first = items[0]                        // index
last  = items[items.length - 1]

user = User{ id: 1, name: "Alice" }     // construct
```

### Control flow

```psaido
if x > 10 then
  y = 1
else
  y = 0

for item in items do
  total = total + item.price

while x < 100 do
  x = x + 1
```

### References inside a scaffold

`@` links work inline, where they are the import (see [References](#references-)
for the resolution model). Use a link where the type or call belongs:

```psaido
!fn login > @auth/user#User
- input: Credentials
  record = @db/users#findByEmail(input.email)
< record
```

…or alias a path once with `!im` and use the short name below:

```psaido
!im @db/users as udb

!fn lookup > udb.User
- id: number
< udb.findById(id)
```

## References (`@`)

`@` is the file-link mechanism used by prose and scaffolds. It links one file to
another.

```
@tools/release            → the whole file
@tools/gate#check         → a named section / provided name inside it
@auth/user#User           → just the `User` schema in that file
```

Paths are relative to the project root. **`@` links are resolved before file
content reaches the agent.** A pre-send filter scans for the `@` pattern,
hydrates each linked file into context, and hands the agent already-linked
content. This is the canonical execution model — the system is deterministic, not
dependent on the model chasing files at read time, and the runner never resolves
`@`.

`@` references are valid in Markdown prose and `psaido` scaffolds. **Do not use
them inside executable `just` recipe bodies** — the runner never resolves `@`,
and a literal `@` must never reach the shell. In a scaffold, translate each link
into the target language's normal import or call (see [Scaffold
blocks](#scaffold-blocks-psaido) for the inline and aliased forms).

## Plugins

A **plugin** is just a folder or a repository containing one or more `.jd`
files. There is no plugin manifest, no build step, no install hook — if a path
holds `.jd` files, it is a plugin.

Install a plugin by **linking** it: point the system at the folder path or git
URL and the `.jd` files inside become available to the index and runner like
any locally authored shard. A plugin is nothing more than the `.jd` files it
contains, linked in.

Plugins are **persisted on disk as plain files** — Markdown, frontmatter, and
fenced blocks, nothing else. That makes a plugin **gitable**: it lives in a repo,
versioned, diffed, branched, and reviewed like any other source. Clone the repo
(or add it as a submodule) and link it; the files are the plugin.

Because the unit is ordinary files in a repo:

- sharing a capability = sharing a folder of `.jd` files
- versioning a capability = git history of that folder
- pinning a capability = a commit, tag, or branch
- composing capabilities = linking several plugins at once

No registry, no packaging format, no separate distribution channel. The link
is the install; the repo is the package; the `.jd` files are the plugin.


## Example: a complete tool file

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

- An index ingests the frontmatter, keyed by the file's path.
- The agent, on a "ship it" query, pulls the file and reads the body.
- The runner shells `just --justfile - release -- <version>` from the extracted
  block.

## Example: a knowledge file with a scaffold

````markdown
---
name: orders/total
description: How an order total is computed and applied. Pull when implementing checkout.
kind: knowledge
tags: [orders, pricing]
provides: [calculateOrderTotal, processOrder]
---

# Order total

Sums each line (`price × quantity`), then writes it back onto the order. An empty
order is a no-op. Product shape comes from @knowledge/product#Product.

```psaido
!sc OrderItem
- product: @knowledge/product#Product
- quantity: number

!sc Order
- id: number
- items: [OrderItem]
- total: number

!fn calculateOrderTotal > number
- input: Order
  total = 0
  for item in input.items do
    total = total + item.product.price * item.quantity
< total

!fn processOrder > Order
- input: Order
  if input.items.length == 0 then
    < input
  input.total = calculateOrderTotal(input)
< input
```
````
