//! justdown — the `.jd` spec as a library.
//!
//! One crate, two consumers, so the spec lives in exactly one place: the `jd`
//! CLI (which builds and queries the library graph on top) and bombshell (which
//! loads `.jd` files as agent tools). This is the GNU-cut boundary moved up one
//! level — the binaries still ship independently, but they no longer each
//! reimplement the parser and `<<var>>` rules and drift apart.
//!
//! Phase 1a exposes the pure spec:
//! - [`jd`] — the `.jd` parser and [`jd::Node`] model (`@link` scanning,
//!   frontmatter ingest, key/category derivation).
//! - [`render`] — single-pass, degrade-never-fail `<<var>>` context injection.
//!
//! The graph engine (build/store/query over a set of source roots) moves into
//! this crate next, so bombshell can build a project-local graph spanning repos.

pub mod cycle;
pub mod graph;
pub mod jd;
pub mod platform;
pub mod render;
pub mod search;
pub mod store;
