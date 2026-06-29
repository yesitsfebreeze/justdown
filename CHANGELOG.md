# Changelog

All notable changes to justdown are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/yesitsfebreeze/justdown/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/yesitsfebreeze/justdown/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/yesitsfebreeze/justdown/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/yesitsfebreeze/justdown/releases/tag/v0.5.0
