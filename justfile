# justfile — the justdown CLI and the one entry point.
#
# A small justfile that BEHAVES like a cross-platform CLI and also BUILDS its own
# index — everything the old MCP server + node builder did, in pure POSIX shell +
# awk. No node anywhere.
#
#   just build            scan the local <lib>/ .jd files → write the index (graph.tsv)
#   just search <q>       rank files by purpose
#   just get <ref>        a file as ordered sections: [0] frontmatter, then prose|tools
#   just ls               categories and their members
#   just links <ref>      inbound + outbound @links of a file
#
# The index is a flat, tab-separated file (key, name, kind, purpose, tags, path,
# links) that shell can build, grep, and MERGE trivially. Queries MERGE a LOCAL
# index with the ONLINE one and LOCAL TRUMPS online entries by key — so your repo's
# own .jd files shadow the published library. Build the local index with `just
# build`; the online one is built in CI on every push.
#
# Requires: just, plus a POSIX shell with curl + awk + find on PATH (git-bash or
# WSL on Windows). Install: download to <project>/justfile (see install.jd), then
# run `just <recipe>` from anywhere in the project. Configure the local library
# dir with JUSTDOWN_LIB (default "library").

set shell := ["sh", "-cu"]

root     := justfile_directory()
lib      := env_var_or_default("JUSTDOWN_LIB", "library")
index    := env_var_or_default("JUSTDOWN_INDEX", "graph.tsv")
repo     := env_var_or_default("JUSTDOWN_REPO", "yesitsfebreeze/justdown")
branch   := env_var_or_default("JUSTDOWN_BRANCH", "main")
raw_base := env_var_or_default("JUSTDOWN_RAW_BASE", "https://raw.githubusercontent.com/" + repo + "/" + branch)

# default: show what the CLI can do
_default:
    @just --justfile "{{justfile()}}" help

# build the local index from <lib>/**/*.jd (cross-platform, no node)
build:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; lib={{quote(lib)}}; index={{quote(index)}}
    libdir="$root/$lib"
    out="$root/$index"
    [ -d "$libdir" ] || { echo "jd: no library dir: $libdir" >&2; exit 1; }
    trap 'rm -f "$out.tmp"' EXIT
    : > "$out.tmp"
    find "$libdir" -name '*.jd' | LC_ALL=C sort | while IFS= read -r f; do
      rel=${f#"$root/"}
      JD_REL="$rel" awk '
        BEGIN{ rel=ENVIRON["JD_REL"] }
        # strip the "key:" prefix, neutralize tabs (they delimit the index), trim trailing
        function val(s){ sub(/^[^:]*:[ \t]*/,"",s); gsub(/[\t]/," ",s); sub(/[ \r]+$/,"",s); return s }
        function arr(s){ sub(/^[^:]*:[ \t]*\[/,"",s); sub(/\].*/,"",s); gsub(/[ \t]/,"",s); return s }
        NR==1 && $0=="---" { fm=1; next }
        fm==1 && $0=="---" { fm=2; next }
        fm==1 {
          if ($0 ~ /^name:/)             name=val($0)
          else if ($0 ~ /^kind:/)        kind=val($0)
          else if ($0 ~ /^description:/) desc=val($0)
          else if ($0 ~ /^tags:/)        tags=arr($0)
          next
        }
        fm==2 {
          s=$0
          while (match(s, /@[a-z0-9_]+\/[a-z0-9_]+/)) {
            l=substr(s, RSTART, RLENGTH)
            if (!(l in seen)) { seen[l]=1; links=links (links=="" ? l : "," l) }
            s=substr(s, RSTART+RLENGTH)
          }
        }
        END{
          p=rel; sub(/\.jd$/,"",p); n=split(p,a,"/")
          key=(n>=2 ? a[n-1] "/" a[n] : a[n])
          if (name=="") name=key
          purpose=(desc!="" ? desc : name)
          printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\n", key, name, kind, purpose, tags, rel, links
        }
      ' "$f" >> "$out.tmp"
    done
    LC_ALL=C sort -o "$out.tmp" "$out.tmp"
    mv "$out.tmp" "$out"
    trap - EXIT
    echo "built $out: $(wc -l < "$out" | tr -d ' ') entries" >&2

# rank library files by a natural-language need (optional: <kind> <num>)
search query kind="" num="5":
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}
    # user input crosses into awk via ENVIRON (no -v backslash-escape processing)
    export JD_QUERY={{quote(query)}} JD_KIND={{quote(kind)}} JD_NUM={{quote(num)}} JD_BASE="$raw_base"
    idx() {
      if [ -f "$root/$index" ]; then awk '{print "local\t" $0}' "$root/$index"; fi
      curl -fsSL "$raw_base/$index" 2>/dev/null | awk '{print "online\t" $0}' || true
    }
    idx | awk -F'\t' '!seen[$2]++' | awk -F'\t' '
      BEGIN{ q=tolower(ENVIRON["JD_QUERY"]); kind=ENVIRON["JD_KIND"]; base=ENVIRON["JD_BASE"]
             nq=split(q, qt, /[^a-z0-9+]+/) }
      kind!="" && $4!=kind { next }
      {
        hay=tolower($3 " " $5 " " $6); score=0
        for (i=1;i<=nq;i++) if (qt[i]!="" && index(hay,qt[i])) score++
        if (score>0) {
          raw=($1=="local" ? $7 " (local)" : base "/" $7)
          printf "%d\t%s\t%s\t%s\t%s\n", score, $3, $4, $5, raw
        }
      }
    ' | LC_ALL=C sort -rn | awk -F'\t' '
      BEGIN{ n=ENVIRON["JD_NUM"]+0; if (n<=0) n=5 }
      NR>n { exit }
      { printf "%d. %s  [%s]  score %s\n   %s\n   %s\n", NR, $2, $3, $1, $4, $5 }
    '

# pull one file as ordered sections: [0] frontmatter, then prose | tools
# (optional <only>: frontmatter | prose | tools)
get ref only="":
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}
    export JD_REF={{quote(ref)}} JD_ONLY={{quote(only)}}
    idx() {
      if [ -f "$root/$index" ]; then awk '{print "local\t" $0}' "$root/$index"; fi
      curl -fsSL "$raw_base/$index" 2>/dev/null | awk '{print "online\t" $0}' || true
    }
    row=$(idx | awk -F'\t' '!seen[$2]++' | awk -F'\t' '
      function base(p){ sub(/\.jd$/,"",p); n=split(p,a,"/"); return a[n] }
      BEGIN{ ref=ENVIRON["JD_REF"]; sub(/^@/,"",ref); sub(/#.*$/,"",ref) }
      { if ($3==ref || $2==ref || $7==ref || base($7)==ref){ print $1 "\t" $7; exit } }')
    [ -n "$row" ] || { echo "jd: no file: $JD_REF" >&2; exit 1; }
    src=$(printf '%s' "$row" | cut -f1)
    path=$(printf '%s' "$row" | cut -f2)
    case "$path" in /*|*..*) echo "jd: refusing suspicious path: $path" >&2; exit 1 ;; esac
    if [ "$src" = local ]; then body() { cat "$root/$path"; }; else body() { curl -fsSL "$raw_base/$path"; }; fi
    body | awk '
      BEGIN{ only=ENVIRON["JD_ONLY"] }
      function flush(  i,isjust,injust){
        if (bn==0) return
        isjust=0
        for (i=1;i<=bn;i++) if (blk[i] ~ /^```just/) isjust=1
        if (isjust && (only=="" || only=="tools")) {
          print "# tools"; injust=0
          for (i=1;i<=bn;i++){
            if (blk[i] ~ /^```just/){ injust=1; continue }
            if (injust && blk[i] ~ /^```/){ injust=0; continue }
            if (injust) print blk[i]
          }
          print ""
        } else if (!isjust && (only=="" || only=="prose")) {
          print "# prose"; for (i=1;i<=bn;i++) print blk[i]; print ""
        }
        bn=0; split("", blk)
      }
      NR==1 && $0=="---" { infm=1; if (only==""||only=="frontmatter") print "# frontmatter"; next }
      infm==1 && $0=="---" { infm=2; if (only==""||only=="frontmatter") print ""; next }
      infm==1 { if (only==""||only=="frontmatter") print; next }
      { if ($0 ~ /^```/) fence=!fence
        if (!fence && $0=="---"){ flush(); next }
        if (bn>0 || $0!="") blk[++bn]=$0 }
      END{ flush() }
    '

# list categories (grouped by primary tag) and their member files
ls:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}
    idx() {
      if [ -f "$root/$index" ]; then awk '{print "local\t" $0}' "$root/$index"; fi
      curl -fsSL "$raw_base/$index" 2>/dev/null | awk '{print "online\t" $0}' || true
    }
    idx | awk -F'\t' '!seen[$2]++' | awk -F'\t' '
      { split($6, t, ","); cat=(t[1]!="" ? t[1] : ($4!="" ? $4 : "misc"))
        cats[cat]=cats[cat] " " $3 }
      END{ for (c in cats) print c ":" cats[c] }
    ' | LC_ALL=C sort

# inbound + outbound @links of a file
links ref:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}
    export JD_REF={{quote(ref)}}
    idx() {
      if [ -f "$root/$index" ]; then awk '{print "local\t" $0}' "$root/$index"; fi
      curl -fsSL "$raw_base/$index" 2>/dev/null | awk '{print "online\t" $0}' || true
    }
    m=$(idx | awk -F'\t' '!seen[$2]++')
    key=$(printf '%s\n' "$m" | awk -F'\t' '
      function base(p){ sub(/\.jd$/,"",p); n=split(p,a,"/"); return a[n] }
      BEGIN{ ref=ENVIRON["JD_REF"]; sub(/^@/,"",ref); sub(/#.*$/,"",ref) }
      { if ($3==ref || $2==ref || $7==ref || base($7)==ref){ print $2; exit } }')
    [ -n "$key" ] || { echo "jd: no file: $JD_REF" >&2; exit 1; }
    printf '%s\n' "$m" | JD_KEY="$key" awk -F'\t' '
      BEGIN{ key=ENVIRON["JD_KEY"] }
      { keys[$2]=1; row[NR]=$0 }
      END{
        for (r=1;r<=NR;r++){ split(row[r], c, "\t")
          if (c[2]==key){ nn=split(c[8], o, ","); for (i=1;i<=nn;i++){ t=o[i]; sub(/^@/,"",t)
            if (t!="" && t!=key && (t in keys)) print "out  @" t } } }
        for (r=1;r<=NR;r++){ split(row[r], c, "\t")
          if (c[2]!=key && index(c[8], "@" key)) print "in   " c[3] "  (@" key ")" }
      }
    '

# print this CLI'\''s usage
help:
    #!/usr/bin/env sh
    cat <<'EOF'
    jd — justdown CLI (pure justfile) · build, query, and merge the .jd graph

    USAGE  just <recipe> [args]

    build                        scan <lib>/**/*.jd → write the local index (graph.tsv)
    search <query> [kind] [num]  rank library files by purpose (substring score)
    get    <ref> [only]          file as ordered sections: [0] frontmatter,
                                  then prose | tools  (only: frontmatter|prose|tools)
    ls                           categories and their member files
    links  <ref>                 inbound + outbound @links of a file
    help                        this

    REF    name · path · key(dir/name) · @dir/name
    MERGE  queries union the LOCAL index with the ONLINE one; LOCAL trumps online
           entries by key. Build the local index with `just build`.
    ENV    JUSTDOWN_LIB (default library)  JUSTDOWN_REPO  JUSTDOWN_BRANCH  JUSTDOWN_RAW_BASE
    EOF
