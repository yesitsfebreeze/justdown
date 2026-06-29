//! The `jd` CLI commands. Each verb is one module; the shared `justdown`
//! library (parser, render, graph, store) lives at the crate root next to
//! `main.rs`. Library modules are flat in `src/`; commands live here.

pub mod build;
pub mod config;
pub mod explore;
pub mod lint;
pub mod mcp;
pub mod query;
