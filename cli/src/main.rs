// jd — the justdown CLI. Single entry point: build, query, and merge the .jd
// library graph. A Rust port of the original pure-POSIX justfile, backed by a
// real graph in a SQLite store instead of a flat graph.tsv + awk.
//
//   jd build          index <lib>/**/*.jd into the graph store
//   jd search <q>     rank files by purpose (graph-aware)
//   jd get <ref>      a file as ordered sections: frontmatter, then prose|tools
//   jd ls             categories and their members
//   jd links <ref>    inbound + outbound @links of a file (graph traversal)
//   jd eval           score retrieval vs eval/queries.tsv (P@1 + MRR)
//   jd lint           validate library .jd frontmatter (CI-gateable)
//   jd version        CLI + store-schema versions
//
// Exit codes (the machine contract): 0 ok · 2 no match / no file · 3 bad args
// · 4 source unreachable. `lint` exits 1 on validation errors.

mod build;
mod config;
mod eval;
mod jd;
mod lint;
mod query;
mod store;

use std::process::exit;

pub const CLI_VERSION: &str = "0.2.0";
// Store schema. Bumped past the tsv index schema (2) — this is a new format.
pub const STORE_SCHEMA: i64 = 3;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cmd = argv.first().map(String::as_str).unwrap_or("help");
    let rest: &[String] = if argv.is_empty() { &[] } else { &argv[1..] };
    let cfg = config::Config::from_env();

    let code = match cmd {
        "build" => build::run(&cfg),
        "search" => query::search(&cfg, rest),
        "get" => query::get(&cfg, rest),
        "ls" => query::ls(&cfg),
        "links" => query::links(&cfg, rest),
        "eval" => eval::run(&cfg),
        "lint" => lint::run(&cfg),
        "version" => version(&cfg),
        "help" | "-h" | "--help" => {
            help();
            0
        }
        other => {
            eprintln!("jd: unknown command: {other} (try `jd help`)");
            3
        }
    };
    exit(code);
}

fn version(cfg: &config::Config) -> i32 {
    println!("jd {CLI_VERSION}  ·  store schema justdown.store/{STORE_SCHEMA}");
    match store::Store::schema_of(&cfg.index_path()) {
        None => println!("local store: none — run `jd build`"),
        Some(v) if v > STORE_SCHEMA => {
            eprintln!(
                "jd: warning: local store is schema {v} but this CLI supports {STORE_SCHEMA} — upgrade the CLI or `jd build`"
            );
        }
        Some(v) => println!("local store: schema {v} (ok)"),
    }
    0
}

fn help() {
    print!(
        r#"jd — justdown CLI · build, query, and merge the .jd graph

USAGE  jd <command> [args]

  build                        scan <lib>/**/*.jd → write the local graph store
  search <query> [kind] [num] [category]
                               rank library files by need (graph-aware:
                               name/use_when > tags > prose; not_when vetoes)
  get    <ref> [only]          file as ordered sections: frontmatter,
                               then prose | tools  (only: frontmatter|prose|tools)
  ls                           categories and their member files
  links  <ref>                 inbound + outbound @links of a file
  eval                         score retrieval vs eval/queries.tsv (P@1 + MRR)
  lint                         validate library .jd frontmatter (CI-gateable)
  version                      CLI + store-schema versions
  help                         this

REF    name · path · key(dir/name) · @dir/name
MERGE  queries union the LOCAL store with the ONLINE one; LOCAL trumps online
       by key. Build the local store with `jd build`.
OUTPUT text (default) or machine JSON via JUSTDOWN_FORMAT=json (versioned
       schema, e.g. justdown.search/1; errors as justdown.error/1 on stderr).
EXIT   0 ok · 2 no match · 3 bad args · 4 source unreachable
ENV    JUSTDOWN_LIB (default library)  JUSTDOWN_INDEX (default graph.db)
       JUSTDOWN_ROOT  JUSTDOWN_REPO  JUSTDOWN_BRANCH  JUSTDOWN_REF
       JUSTDOWN_RAW_BASE  JUSTDOWN_FORMAT (text|json)
"#
    );
}
