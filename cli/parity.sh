#!/usr/bin/env sh
# Parity gate: jd (rust) vs just (awk) must produce identical stdout across
# search/get/ls/links in both text and JSON. Online disabled on both via a
# file:// base so curl fails instantly → clean local-only comparison.
set -u
cd "$(dirname "$0")/.."
export JUSTDOWN_RAW_BASE="file:///nonexistent"; export JD_PLATFORM=wsl
JD=./cli/target/debug/jd
DB=graph.db

just build >/dev/null 2>&1
JUSTDOWN_INDEX=$DB $JD build >/dev/null 2>&1

j()  { just "$@" </dev/null 2>/dev/null; }
d()  { JUSTDOWN_INDEX=$DB $JD "$@" </dev/null 2>/dev/null; }
jj() { JUSTDOWN_FORMAT=json just "$@" </dev/null 2>/dev/null; }
dj() { JUSTDOWN_INDEX=$DB JUSTDOWN_FORMAT=json $JD "$@" </dev/null 2>/dev/null; }

pass=0; fail=0; fails=""
chk() { # label  a  b
  if [ "$2" = "$3" ]; then pass=$((pass+1)); else fail=$((fail+1)); fails="$fails $1"; fi
}

# --- search: every eval query, text + json ---
while IFS="$(printf '\t')" read -r q exp; do
  case "$q" in ''|'#'*) continue;; esac
  chk "search-txt:$q" "$(j search "$q" '' 20)" "$(d search "$q" '' 20)"
  chk "search-json:$q" "$(jj search "$q" '' 20)" "$(dj search "$q" '' 20)"
done < eval/queries.tsv

# --- search: kind + category filters, edge args ---
for k in tool agent knowledge workflow; do
  chk "search-kind:$k" "$(j search file "$k" 10)" "$(d search file "$k" 10)"
done
chk "search-cat" "$(j search config '' 10 vim)" "$(d search config '' 10 vim)"
chk "search-nomatch" "$(j search zzzzqqqq '' 5)" "$(d search zzzzqqqq '' 5)"

# --- ls, text + json ---
chk "ls-txt" "$(j ls)" "$(d ls)"
chk "ls-json" "$(jj ls)" "$(dj ls)"

# --- get + links across a sample of refs (every node key) ---
for key in $(awk -F'\t' '/^#/{next} NF{print $1}' graph.tsv); do
  name=$(awk -F'\t' -v k="$key" '$1==k{print $2; exit}' graph.tsv)
  chk "get:$key"        "$(j get "$key")"             "$(d get "$key")"
  chk "get-fm:$key"     "$(j get "$key" frontmatter)" "$(d get "$key" frontmatter)"
  chk "get-prose:$key"  "$(j get "$key" prose)"       "$(d get "$key" prose)"
  chk "get-tools:$key"  "$(j get "$key" tools)"       "$(d get "$key" tools)"
  chk "get-json:$key"   "$(jj get "$key")"            "$(dj get "$key")"
  chk "links:$key"      "$(j links "$key")"           "$(d links "$key")"
  chk "links-json:$key" "$(jj links "$key")"          "$(dj links "$key")"
  chk "get-byname:$name" "$(j get "$name")"           "$(d get "$name")"
done

echo "PARITY: pass=$pass fail=$fail"
if [ "$fail" -gt 0 ]; then
  echo "FAILS (first 15):"
  printf '%s\n' $fails | head -15
fi
