// jd — the justdown CLI. Single entry point: build, query, and merge the .jd
// library graph. A Rust port of the original pure-POSIX justfile, backed by a
// real graph in a SQLite store instead of a flat graph.tsv + awk.
//
//   jd build          index <lib>/**/*.jd into the graph store (publish)
//   jd refresh        download the online belt's prebuilt graphs into the cache
//   jd search <q>     rank files by purpose (graph-aware)
//   jd get <ref>      a file as ordered sections, or one output profile
//                     (--human|--agent|--frontmatter|--justfile)
//   jd ls             categories and their members
//   jd links <ref>    inbound + outbound @links of a file (graph traversal)
//   jd resolve <term> live @link completion (direct prefix / --fuzzy ranker)
//   jd lint           validate library .jd frontmatter (CI-gateable)
//   jd version        CLI + store-schema versions
//
// Exit codes (the machine contract): 0 ok · 2 no match / no file · 3 bad args
// · 4 source unreachable. `lint` exits 1 on validation errors.

mod cmd;

use cmd::{build, config, explore, lint, mcp, query, refresh};
use config::Format;
use justdown::store::STORE_SCHEMA;
use std::process::exit;

pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    // Restore the default SIGPIPE disposition. The Rust runtime sets SIGPIPE to
    // SIG_IGN, so a write to a pipe whose reader has exited (e.g. `jd ls | head`)
    // returns EPIPE and `println!` panics with "failed printing to stdout:
    // Broken pipe" plus a backtrace. `jd` is a pipeable data source — `--json`
    // output is meant to feed `jq`/`rg`/`fzf`/`head`, which routinely close early
    // — so a closed downstream pipe must terminate the process cleanly, not
    // crash it. Resetting to SIG_DFL makes the kernel kill us on the next write
    // to the dead pipe (exit 141, the conventional `| head` status). Windows
    // has no SIGPIPE and is unaffected.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    // `--json` is a global wire-format switch valid on every command; pull it out
    // of argv before dispatch so subcommands see only their own args. It replaces
    // the old JUSTDOWN_FORMAT env var.
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let json = raw.iter().any(|a| a == "--json");
    let argv: Vec<String> = raw.into_iter().filter(|a| a != "--json").collect();
    let cmd = argv.first().map(String::as_str).unwrap_or("help");
    let rest: &[String] = if argv.is_empty() { &[] } else { &argv[1..] };
    let mut cfg = config::Config::from_env();
    cfg.format = if json { Format::Json } else { Format::Text };

    let code = match cmd {
        "build" => build::run(&cfg, rest),
        "refresh" => refresh::run(&cfg),
        "search" => query::search(&cfg, rest),
        "get" => query::get(&cfg, rest),
        "ls" => query::ls(&cfg),
        "links" => query::links(&cfg, rest),
        "path" => query::path(&cfg, rest),
        "resolve" => query::resolve(&cfg, rest),
        "explore" => explore::run(rest),
        "mcp" => mcp::run(rest),
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
    match justdown::store::Store::schema_of(&cfg.index_path()) {
        None => println!("published store: none — run `jd build` to publish"),
        Some(v) if v > STORE_SCHEMA => {
            eprintln!(
                "jd: warning: published store is schema {v} but this CLI supports {STORE_SCHEMA} — upgrade the CLI or `jd build`"
            );
        }
        Some(v) => println!("published store: schema {v} (ok)"),
    }
    0
}

fn help() {
    print!(
        r#"jd — justdown CLI · build, query, and merge the .jd graph

USAGE  jd <command> [args]

  build                        scan <lib>/**/*.jd → write the graph store(s).
                               This is how a repo PUBLISHES its library (consumers
                               fetch it via `jd refresh`); queries here read the
                               repo live, no build needed. Always recursive: every
                               nested .jd/<lib> home builds its own store.
  refresh                      download every belt remote's prebuilt graph
                               (<raw_base>/.jd/graph.db) into the local cache
                               (<cache>/belt/<slug>.db). Queries read it offline;
                               re-run to update. needs curl.
  search <query> [kind] [num] [category]
                               rank library files by need (graph-aware:
                               name/use_when > tags > prose; not_when vetoes)
  get    <ref> [profile] [--var name=value ...]
                               file as ordered sections (default), or one output
                               profile selected by the file's kind:
                                 --frontmatter  the retrieval contract only
                                 --human        prose + fenced blocks, no yaml
                                 --agent        contract + prose, no raw recipe
                                 --justfile     vanilla just recipes, host-resolved
                               --justfile needs kind tool|workflow; on any other
                               kind (agent/knowledge/types) it refuses (exit 3) —
                               those .jd files are not executable as scripts.
                               Resolves <<var>> context injection before output:
                               values come from JUSTDOWN_VAR_<NAME> env and
                               --var flags (flags win). One pass, non-recursive.
  ls                           categories and their member files
  links  <ref>                 inbound + outbound @links of a file
  path   <a> <b>               shortest @link connection between two files
  resolve <term> [num] [--fuzzy]
                               live @link completion: ranked key/name/leaf prefix
                               matches (direct), or the field-weighted ranker
                               (--fuzzy, for @?term). Feeds the editor popup.
  explore [--port=N] [--dev]   serve the built-in .jd explorer and open it in the
                               browser. One shared website per port (default
                               3001): the first process hosts, every later one
                               feeds its JD_ROOT (default $HOME) into the search
                               and reuses the running site. Search spans the
                               union of all live jd processes; if the host dies a
                               feeder takes over. --dev serves the editor assets
                               from disk with live reload (edit, save, refresh).
  mcp                          serve jd's read verbs (search/get/ls/links/path)
                               as a stdio MCP server — one library-lookup server,
                               not one per capability. Newline-delimited
                               JSON-RPC 2.0 on stdin/stdout.
  lint                         validate library .jd frontmatter (CI-gateable)
  version                      CLI + store-schema versions
  help                         this

REF    name · path · key(dir/name) · @dir/name
MERGE  queries merge two graphs — the LIVE repo-local <root>/.jd files (parsed
       fresh every call) shadow the CACHED belt (`jd refresh`) by key
       (local > cached). NESTED: the live graph is itself every .jd home found
       under the project tree (each owns its <lib>/, no sources copied); on a key
       collision the deeper home wins (shadowed keys logged). Disable nesting
       with JUSTDOWN_NESTED=0.
OUTPUT text (default) or machine JSON via the global --json flag (versioned
       schema, e.g. justdown.search/1; errors as justdown.error/1 on stderr).
EXIT   0 ok · 2 no match · 3 bad args · 4 source unreachable
ENV    JUSTDOWN_LIB (default library)  JUSTDOWN_INDEX (default
       .jd/graph.db; the publish artifact `jd build` writes)
       JUSTDOWN_NESTED (default on; =0 to disable nested .jd composition)
       JUSTDOWN_ROOT  JUSTDOWN_REPO  JUSTDOWN_BRANCH  JUSTDOWN_REF
       JUSTDOWN_REPOS (belt override; else read from <root>/.jd/.jdconfig —
       one owner/repo[@ref] or URL per line)
       JUSTDOWN_RAW_BASE
       JUSTDOWN_VAR_<NAME>  host value for the <<name>> escape (lower-cased)
GLOBAL --json  machine JSON on any command (replaces JUSTDOWN_FORMAT)
"#
    );
}
