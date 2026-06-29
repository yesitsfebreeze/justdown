//! justdown — the `.jd` library graph.
//!
//! One crate, one binary: the `jd` CLI (`src/main.rs` + `src/cmd/`) is built on
//! top of this library, and the library is also consumed directly by other
//! runners (e.g. bombshell, which loads `.jd` files as agent tools). The spec
//! lives in exactly one place so consumers never reimplement the parser and
//! `<<var>>` rules and drift apart.
//!
//! The spec and graph engine:
//! - [`jd`] — the `.jd` parser and [`jd::Node`] model (`@link` scanning,
//!   frontmatter ingest, key/category derivation).
//! - [`render`] — single-pass, degrade-never-fail `<<var>>` context injection.
//! - [`graph`]/[`store`]/[`search`]/[`links`] — build, persist, and query the
//!   library graph over a set of source roots.

pub mod cycle;
pub mod graph;
pub mod jd;
pub mod links;
pub mod lint;
pub mod platform;
pub mod render;
pub mod search;
pub mod store;
