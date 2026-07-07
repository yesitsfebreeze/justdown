package main

import (
	"fmt"
	"os"

	justdown "github.com/yesitsfebreeze/justdown/src"
)

const cliVersion = "0.12.0"

func main() {

	raw := os.Args[1:]
	json := false
	var argv []string
	for _, a := range raw {
		if a == "--json" {
			json = true
		} else {
			argv = append(argv, a)
		}
	}
	cmd := "help"
	if len(argv) > 0 {
		cmd = argv[0]
	}
	var rest []string
	if len(argv) > 1 {
		rest = argv[1:]
	}
	cfg := configFromEnv()
	if json {
		cfg.format = formatJSON
	}

	var code int
	switch cmd {
	case "build":
		code = cmdBuild(&cfg, rest)
	case "search":
		code = cmdSearch(&cfg, rest)
	case "get":
		code = cmdGet(&cfg, rest)
	case "just":
		code = cmdJust(&cfg, rest)
	case "ls":
		code = cmdLs(&cfg)
	case "links":
		code = cmdLinks(&cfg, rest)
	case "path":
		code = cmdPath(&cfg, rest)
	case "resolve":
		code = cmdResolve(&cfg, rest)
	case "explore":
		code = cmdExplore(rest)
	case "mcp":
		code = cmdMCP(rest)
	case "lint":
		code = cmdLint(&cfg)
	case "version":
		code = cmdVersion(&cfg)
	case "help", "-h", "--help":
		help()
	default:
		fmt.Fprintf(os.Stderr, "jd: unknown command: %s (try `jd help`)\n", cmd)
		code = 3
	}
	os.Exit(code)
}

func cmdVersion(cfg *config) int {
	fmt.Printf("jd %s  ·  store schema justdown.store/%d\n", cliVersion, justdown.StoreSchema)
	v, ok := justdown.SchemaOf(cfg.indexPath())
	switch {
	case !ok:
		fmt.Println("published store: none — run `jd build` to publish")
	case v > justdown.StoreSchema:
		fmt.Fprintf(os.Stderr,
			"jd: warning: published store is schema %d but this CLI supports %d — upgrade the CLI or `jd build`\n",
			v, justdown.StoreSchema)
	default:
		fmt.Printf("published store: schema %d (ok)\n", v)
	}
	return 0
}

func help() {
	fmt.Print(`jd — justdown CLI · build, query, and merge the .jd graph

USAGE  jd <command> [args]

  build                        smart sync, fastest way to the latest state. Does
                               only what changed: rebuilds the merged local graph
                               (<root>/.jd/remote-graph.db, every nested home
                               unioned) iff the .jd sources changed, and rebuilds
                               each belt source's cached graph iff it went stale —
                               a GitHub repo when its ref's latest commit moved,
                               a directory source when its .jd files changed.
                               Everything up to date → nothing rebuilt. Queries
                               run the local step automatically, so edits show.
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
  just   <ref> [recipe] [args] [--var name=value ...]
                               run a tool: render <ref>'s host-resolved justfile
                               and dispatch it through ` + "`just`" + ` (the one-liner wrap
                               of ` + "`jd get <ref> --justfile | just --justfile -" + `
                               <recipe> args` + "`" + `). ref is kind tool|workflow;
                               recipe + args pass to just verbatim; exit code is
                               just's own (127 if ` + "`just`" + ` is not installed).
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
MERGE  queries merge two graphs — the repo-local graph (auto-rebuilt from
       <root>/.jd when its sources change, then read from the cached store)
       shadows the CACHED belt by key (local > cached). NESTED: the local graph
       unions every .jd home found under the project tree (each owns its <lib>/,
       no sources copied); on a key collision the deeper home wins. Disable
       nesting with JUSTDOWN_NESTED=0.
OUTPUT text (default) or machine JSON via the global --json flag (versioned
       schema, e.g. justdown.search/1; errors as justdown.error/1 on stderr).
EXIT   0 ok · 2 no match · 3 bad args · 4 source unreachable
ENV    JUSTDOWN_LIB (default library)  JUSTDOWN_INDEX (default
       remote-graph.db; the publish artifact ` + "`jd build`" + ` writes under .jd/)
       JUSTDOWN_NESTED (default on; =0 to disable nested .jd composition)
       JUSTDOWN_ROOT  JUSTDOWN_REPO  JUSTDOWN_BRANCH  JUSTDOWN_REF
       JUSTDOWN_REPOS (belt override; else read from <root>/.jd/.jdconfig —
       one source per line: owner/repo[@ref], a URL, or a directory path
       (/abs, ./rel, ~/…) whose .jd files build their own cached graph)
       JUSTDOWN_RAW_BASE
       JUSTDOWN_VAR_<NAME>  host value for the <<name>> escape (lower-cased)
GLOBAL --json  machine JSON on any command (replaces JUSTDOWN_FORMAT)
`)
}
