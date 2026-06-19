# Justdown (.jd) Language Specification v0.2

> justdown is not a scripting language. It is a retrieval-friendly wrapper
> around existing scripts, where the docs, metadata, and recipe live together
> in one file.

A `.jd` file is a small Markdown file with optional executable or scaffolded
blocks. It is **not a new language** — it composes Markdown, YAML frontmatter,
[just](https://just.systems) recipes, and PSAIDO scaffolds so one file can serve
several readers without copies:

- **humans** read the rendered Markdown
- **indexers** index only the YAML frontmatter
- **agents** read the Markdown body after retrieval
- **fenced blocks** carry the payload: a ```` ```just ```` recipe or a
  ```` ```psaido ```` scaffold

This document specifies the **format** — the regions of a `.jd` file and their
syntax. It says nothing about how a host runs, indexes, or distributes the files;
those are the consuming tool's concern, not the language's.

## Intention

Authoring a `.jd` tool-file *is* making the thing — there is no separate tool
implementation and no hand-written function to keep in sync with its docs. The
recipe in the fenced block *is* the tool; the prose around it is the *why* and
*when*; the frontmatter is the retrieval contract that decides when it gets
pulled.

The contract is deliberately flexible: a recipe is ordinary shell, so its backend
can be anything reachable from a process — a script on disk, a CLI (`gh`,
`claude`, `codex`), an HTTP call, a language runtime, or another recipe via `just`
dependencies. Heavy logic lives in real scripts on disk; the recipe is the thin,
named, parameterized entry point that delegates to them.

A file may hold several recipes; `run` names the default, and the rest are
callable by name. So one file can expose a family of related entry points without
becoming several tools.

## The model

A `.jd` file has three regions, and each one is a different surface for a
different reader:

1. **frontmatter — the retrieval contract.** The indexed surface. An index
   ingests only this; it decides *when* the file is pulled.
2. **Markdown body — the instruction manual.** Intent, context, usage notes —
   *why* and *when*. Read by the agent once a query selects the file.
3. **fenced blocks — the payloads.** ```` ```just ```` recipes are the executable
   payload; ```` ```psaido ```` scaffolds are read and translated, never run. The
   *how*.

## Frontmatter (the retrieval contract)

The frontmatter is the **only** part an index ingests, keyed by the file's path.
It is the match surface that decides *when* the file is pulled.

| Field | Required | Meaning |
|-------|----------|---------|
| `name` | yes | File identity. |
| `description` | yes | One or two lines: what it is and when to use it. **This is the contract** — the agent retrieves on it. |
| `kind` | yes | `tool` \| `agent` \| `knowledge` \| `workflow`. |
| `tags` | no | Free-form labels for grouping and retrieval. |
| `use_when` | no | Trigger phrases (positive signals). Scored above prose, so a hit here outranks a stray match in the description. |
| `not_when` | no | Anti-triggers. A query term that hits one **vetoes** the file from results — the cheapest way to cut false positives. |
| `danger` | no | `none` \| `low` \| `medium` \| `high` — how destructive running it is. |
| `side_effects` | no | What it changes outside the process, e.g. `[network, git-push, publish]`. |
| `requires` | no | Host capabilities/binaries needed, e.g. `[npm, git]`. |
| `run` | tool only | Names the default recipe, e.g. `release`. |
| `provides` | no | Names other files link to via `#Name` (schemas, functions, recipes). |

`use_when`/`not_when` are YAML arrays of short phrases, e.g.
`use_when: [cut a release, publish a version]`. They tune *retrieval*, not
behavior: an index that ignores them still resolves the file correctly by
`name`/`description`/`tags` — they only sharpen ranking.

`danger`/`side_effects`/`requires` are **safety metadata**, carried on every
search result so a policy layer (or an unattended agent) can gate a destructive
call *before* running it — without reading the prose. They never affect ranking.

Everything else (the body, the blocks) stays on disk and is read by path only
once a query selects the file.

## Markdown body

Plain Markdown — headings, prose, lists, links. It tells the agent when to reach
for the file and what it does; it never restates the fenced block's mechanics.
Keep a file small and single-purpose.

## The just blocks (recipes)

A ```` ```just ```` block is a normal [justfile](https://just.systems) fragment —
recipes, variables, dependencies, parameters. A tool file's `run` frontmatter
names the default recipe; a file may hold several, the rest callable by name.

````markdown
```just
# build, tag, and publish a release
release version="patch":
  npm run check
  npm version {{version}}
  git push --follow-tags
```
````

### Why just, not bash

`.jd` needs named, addressable units of work, not one opaque script. just gives
each recipe a name, declared parameters, defaults, and dependencies while keeping
the recipe body as ordinary shell. Heavy logic should still live in real scripts
on disk; a just recipe is the thin, named entry point that delegates to them.
just is cross-platform, but recipe bodies are shell commands, so portable recipes
should delegate to cross-platform scripts or tools.

### Platform-guarded variants (a justdown extension)

This is the **one** place `.jd` extends just's grammar. A recipe whose command
differs by operating system is written once per platform, each variant preceded by
a platform attribute on its own line:

````markdown
```just
[unix]
open target:
  xdg-open {{target}}

[macos]
open target:
  open {{target}}

[windows]
open target:
  cmd /c start "" {{target}}

[wsl]
open target:
  wslview {{target}}
```
````

The tags are `unix`, `macos`, `windows`, and `wsl`; `darwin` is an accepted alias
for `macos`. A tag may be a comma list — `[unix, wsl]` guards one body for both.
An attribute guards the recipe header that follows it and that recipe's indented
body; untagged lines (variables, settings, plain recipes) always apply. Variants of
the same recipe name must be mutually exclusive for any one platform, so exactly one
definition survives selection.

`just` itself has no `[wsl]` attribute and would reject one — so platform selection
is **resolved by the runner, not by just**. When the runner extracts a file's
recipes it detects the host (`uname -s`, refined to `wsl` via `/proc/version`),
keeps only the matching variant, and **strips the attribute lines**. What reaches
`just --justfile -` is therefore an ordinary justfile with a single, plain
definition per recipe — vanilla just never sees the extension. The divergence from a
real justfile is exactly these attribute lines, and the binary normalizes them away.

## Scaffold blocks (`psaido`)

Where a file sketches logic for an agent instead of running code, it uses a
```` ```psaido ```` block — the PSAIDO scaffold dialect. It is **read, not
compiled**: a model translates it into the project's real language. It is context,
the same as prose.

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

Paths are relative to the project root. How a host resolves a link — when it
hydrates the target and how much it pulls in — is outside this spec; within the
format, `@` is purely the link syntax.

`@` references are valid in Markdown prose and `psaido` scaffolds. **Do not use
them inside executable `just` recipe bodies** — a literal `@` must never reach the
shell. In a scaffold, translate each link into the target language's normal import
or call (see [Scaffold blocks](#scaffold-blocks-psaido) for the inline and aliased
forms).

## Context injection (`<<var>>`)

`<<name>>` is a host-injected variable. Before a file is consumed, the host
replaces each escape with a value it supplies — the wrapping shell, the working
directory, the last command, the current selection, and so on. It is the inbound
counterpart to `@`: where `@` pulls in *another file*, `<<var>>` pulls in *live
host state*, and both resolve **before** the content reaches the agent or the
shell.

```
You are wrapping the user's `<<shell>>` shell.
cwd: <<cwd>>   last command: <<last_command>>
```

As with `@`, the format fixes only the **syntax**. The variable namespace and the
moment of substitution are the host's concern, not the language's.

Rules:

- **Name grammar.** `<<` + `[A-Za-z0-9_]+` + `>>`. Anything else — `<< x >>`,
  `<<a b>>`, `<<>>`, an unclosed `<<` — is not an escape and passes through
  verbatim.
- **Unknown names pass through.** A name the host does not supply is left exactly
  as written — never an error, never silently emptied. Bad input degrades to a
  no-op.
- **Single-pass, non-recursive.** Substitution runs once over the authored text;
  injected values are spliced verbatim and **never re-scanned**. A value that
  itself contains `<<…>>` (e.g. captured terminal output) does not trigger a
  second substitution. This is the safety property that lets *untrusted* host
  state be injected without it smuggling in further escapes.
- **Literal `<<`.** Write `<<<<` to emit a single literal `<<`.

### Injection vs. just interpolation

`<<var>>` and just's own `{{var}}` never collide: different delimiters, different
times. `<<var>>` is resolved by the host *before* the recipe is handed to just;
`{{var}}` is resolved by just *while it runs*. A host may apply `<<var>>` inside a
`just` recipe body, but a spliced value lands as raw shell text — so inject only
trusted values there, or quote them. For ordinary recipe *inputs*, prefer just
parameters and `{{ }}`; reserve `<<var>>` for host state just cannot know (the
wrapping shell, the caller's cwd, a live selection).

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
- The `release` recipe in the fenced block is the executable payload.

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
