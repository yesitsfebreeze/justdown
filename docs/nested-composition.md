# Nested local graph composition — design note

## Problem

Today a repo has exactly one `.jd` home: queries union three tiers — repo-LOCAL
(`<root>/.jd`) ⊕ machine-GLOBAL (`~/.jd`) ⊕ ONLINE belt — and `jd build` indexes
one `<root>/.jd/<lib>/**/*.jd` into one `graph.db`. There is no notion of several
`.jd` homes living at different depths inside one repo.

We want any folder in a tree to own its own `.jd/library`, build its own
self-contained `.jd/graph.db`, and have the root resolve keys across all of them
as one graph — with **no copying** of `.jd` sources between folders.

## Confirmed current behavior (read from the code)

- `jd build` → `build::run` → `build_into(cfg.index_path(), cfg.lib_dir(), cfg.root)`.
  One library dir, one store, keys derived relative to `cfg.root`.
- `key_and_category` uses only the **last two** path segments
  (`library/ui/glass.jd` → key `ui/glass`). So two nested libraries with the same
  `<category>/<name>` leaf collide on key by construction — collisions are the
  expected case, not an edge case.
- `query::gather` unions Local (`cfg.index_path()`) ⊕ Global (`~/.jd`) ⊕ Online,
  dedup keep-first → local > global > online.
- `get` reads a LOCAL file via `cfg.root.join(row.path)` — `row.path` is relative
  to the home the row was indexed under. This is the seam that breaks for nested
  homes: a row from `pkg/x/.jd` has path `library/.../f.jd` but the file is under
  `pkg/x/.jd/`, not `<root>/.jd/`.
- `Row.origin` already exists as "the base a row's files hang off" (used today
  only to point online rows at their remote's raw base). It is a load-time field,
  not a stored column.
- MCP shells out to this same binary, so any change to `gather`/`get` flows to
  the MCP read verbs for free. `jd pull` / remote belts are untouched.

## Decisions (recommended defaults)

### 1. Discovery: recursive build + query-time union  ✅ (matches your lean)

- `jd build --recursive` (alias `-r`) walks the project tree, finds every
  `*/.jd` home that has a `<lib>/` dir, and builds each one's **own**
  self-contained `graph.db` (plus the root's). Each folder's library is the only
  source of truth for its own procedures — nothing is copied.
- At **query** time, `gather` discovers every nested `.jd/<index>` under the
  project tree and unions them — exactly like the online belt merge, one tier
  deeper. The root graph is therefore the union of all nested graphs *without*
  baking a duplicate copy into one file.
- Plain `jd build` (no flag) stays single-home — today's behavior, unchanged.

### 2. Sibling precedence: deeper path wins, ties by path  ✅ (matches your lean)

The 3-tier rule is "nearer scope trumps." For siblings, *more specific* = *nearer
to the code it serves* = **deeper path wins**. Local homes are ordered by depth
descending, ties broken lexicographically by absolute path (deterministic), then
deduped keep-first. The root `.jd` is simply the shallowest local home, so a
deeper package's `ui/glass` shadows the root's `ui/glass`. Every shadowed local
key is logged to stderr (`jd: note: <key> from <deep> shadows <shallow>`), so the
collision is observable, never silent. Local as a whole still beats global beats
online, as today.

### 3. Boundaries

- Walk starts at the project dir (`<root>/.jd`'s parent) and goes **down only** —
  never above the root home.
- Prune well-known heavy / non-source dirs: `.git`, `.git-fs`, `node_modules`,
  `target`, `dist`, `build`, `vendor`, `.worktrees`. Depth cap (8) as a backstop.
- A `.jd` home is not descended into looking for further nested `.jd` (a library
  tree does not itself contain another home).
- We do **not** add a gitignore parser / the `ignore` crate: it would skip hidden
  dirs and break `.voit/.jd`, and jd's pitch is a self-contained binary. The prune
  set + depth cap is the premade-free equivalent. (Open to revisiting — see the
  question put to you.)

### 4. Backward compatibility — additive

A repo whose only home is `<root>/.jd` discovers exactly that one home and behaves
identically to today. Nested graphs only contribute when they have been built.
The 3-tier union, `jd pull`, remote belts, and the MCP verbs are unchanged.
No on-disk schema change: every nested `graph.db` is the same `STORE_SCHEMA`.

### 5. CLI / ENV surface

- Build: gated behind `jd build --recursive` so a bare `jd build` is never
  surprising.
- Query union: automatic (on by default), bounded by the prune rules; opt out
  with `JUSTDOWN_NESTED=0` for the exact pre-feature single-home behavior.
- `JUSTDOWN_LIB`, `JUSTDOWN_ROOT` keep their meaning. `JUSTDOWN_INDEX`: its
  **basename** names each nested graph file inside that home's `.jd`
  (`<home>/.jd/<basename>`); an **absolute** `JUSTDOWN_INDEX` remains a
  ROOT-only publish seam and is *not* propagated to nested homes (that would
  point every nested build at one absolute file and clobber). When `JUSTDOWN_INDEX`
  is absolute, nested discovery falls back to the default `graph.db` basename.

## Path resolution (the implementation seam)

Each nested-local row carries its home dir in `Row.origin` (absolute path). `get`
resolves a local file as `PathBuf(origin).join(row.path)` when `origin` is set,
else `cfg.root` (root home) / `~/.jd` (global) as today. No stored-schema change —
`origin` is set at load time, just as it already is for online rows.
