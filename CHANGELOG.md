# Changelog

All notable changes to justdown are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0] - 2026-06-29

### Changed
- `jd build` is now one smart incremental sync — the fastest path to the latest
  state. It rebuilds the merged local graph only when the repo's `.jd` sources
  changed (a cheap mtime/size fingerprint, cached in a sidecar) and refreshes
  each belt remote only when upstream changed (ETag conditional GET).
- The local layer moved from per-query live parsing to a cached store that
  queries auto-rebuild on staleness — fast when unchanged, always current after
  an edit, with no manual build step.

### Removed
- `jd refresh` — folded into `jd build` (which now does both local rebuild and
  remote refresh, each incrementally).

## [0.8.0] - 2026-06-29

### Added
- Two-graph query model. Queries now merge a **live** repo-local graph with a
  **cached** belt:
  - Repo-local `.jd` files are parsed fresh on every query — no `jd build` is
    needed to query locally, so local edits are reflected instantly.
  - `jd refresh` downloads each belt remote's prebuilt graph into a local cache
    (`<cache>/belt/<slug>.db`, under `$XDG_CACHE_HOME` or `~/.cache`), which
    queries then read offline. Local always shadows the cached belt by key.
- `jd build` now publishes a single merged `remote-graph.db` under the `.jd`
  home, unioning every nested home (deeper wins) and keyed repo-root-relative —
  the one file a consumer downloads to get the whole library.

### Changed
- `jd build` is publish-only and always merges every nested home; the
  `--recursive` / `-r` flag is gone (queries read the repo live, so a build only
  exists to publish). The committed merged graph is how a repo publishes.
- Cached-belt file bodies are fetched at `<raw_base>/<path>` — published paths
  now carry each home's `.jd/…` prefix, so nested-home files resolve correctly.
- `JUSTDOWN_INDEX` defaults to `remote-graph.db`.

### Removed
- The machine-global `~/.jd` tier and all of its build/query paths.
- Per-query online belt fetching — superseded by the `jd refresh` cache.
- `jd pull` (git-clone belt hydration) — replaced by `jd refresh`.
- Per-home `graph.db` stores — local queries are live and never read a built
  store.

## [0.7.0] - 2026-06-29

### Added
- Nested local graph composition — any folder in a tree can own its own
  `.jd/library` and `graph.db`, and the repo root resolves keys across all of
  them as one graph, with no `.jd` sources copied between folders.
  - `jd build --recursive` (`-r`) discovers every nested `.jd/<lib>` home under
    the project tree and builds each its own self-contained store.
  - Queries (`get`/`search`/`ls`/`links`/`path`, and the MCP read verbs) union
    every discovered local home; on a key collision the deeper home wins and the
    shadowed key is logged. Local still trumps machine-global trumps online.
  - Discovery prunes heavy dirs (`node_modules`, `target`, …), is depth-capped,
    and scans hidden dirs (so `.voit/.jd` is found). Opt out with
    `JUSTDOWN_NESTED=0`.

### Changed
- `JUSTDOWN_INDEX` basename now names each nested home's store; its absolute
  (publish-seam) form stays root-only so nested homes never clobber one another.
  No on-disk graph-schema change.

## [0.6.0] - 2026-06-28

### Added
- `@`links across the stack — `@name` direct and `@?term` fuzzy `.jd` links,
  resolved in core, CLI, MCP, and the editor.
- Windows PowerShell installer (`scripts/install.ps1`) with checksum verification
  and PATH wiring; sh/ps one-liners double as updaters.
- `tools_version` (`@tools/version`) — set the project version across
  `cli/Cargo.toml`, `core/Cargo.toml`, the plugin manifest, and `Cargo.lock` from
  one `MAJOR.MINOR.PATCH` argument.
- Frontend design-style reference library — flat, minimalism, dark-mode, memphis,
  art-deco, skeuomorphism, aurora-gradient, bauhaus, swiss-international,
  constructivism, soft-UI (glass/neu/clay), neo-brutalism, bento-grid, material,
  and retro-digital (y2k, vaporwave, tui, pixel).
- Full Onshape REST API surface documented in the library.

### Changed
- Release chain moved from npm to cargo; the gate now builds and tests with cargo.
- Editor: text-hugging rounded selection, frontmatter key column, unified focus
  slider, theme light/dark/auto with uniform crossfade.
- Standardized terminology on `.jd` / `.jds` (dropped "procedure").

## [0.5.0] - 2026-06-27

- Baseline release prior to this changelog.

[Unreleased]: https://github.com/yesitsfebreeze/justdown/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/yesitsfebreeze/justdown/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/yesitsfebreeze/justdown/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/yesitsfebreeze/justdown/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/yesitsfebreeze/justdown/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/yesitsfebreeze/justdown/releases/tag/v0.5.0
