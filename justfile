# justfile — the justdown CLI. The single entry point: build, query, and merge
# the .jd library graph in pure POSIX shell (no node, no extra binary).
#
#   just build          index <lib>/**/*.jd into graph.tsv
#   just search <q>     rank files by purpose
#   just get <ref>      a file as ordered sections: [0] frontmatter, then prose|tools
#   just ls             categories and their members
#   just links <ref>    inbound + outbound @links of a file
#
# graph.tsv is a flat, tab-separated index (key, name, kind, purpose, tags, path,
# links, use_when, not_when, danger, side_effects, requires, category). category is
# the parent folder, inferred at build. Queries merge the local index over the
# online one — local entries win by
# key, so a project's own .jd files shadow the published library. `just build`
# writes the local index; CI builds the online one on every push.
#
# Requires just, plus curl, awk, and find on PATH (git-bash or WSL on Windows).
# Install: download to <project>/justfile (see install.jd), then run `just <recipe>`
# from anywhere in the project. Set JUSTDOWN_LIB to change the library dir.

set shell := ["sh", "-cu"]

root     := justfile_directory()
lib      := env_var_or_default("JUSTDOWN_LIB", "library")
index    := env_var_or_default("JUSTDOWN_INDEX", "graph.tsv")
repo     := env_var_or_default("JUSTDOWN_REPO", "yesitsfebreeze/justdown")
branch   := env_var_or_default("JUSTDOWN_BRANCH", "main")
# JUSTDOWN_REF pins to any git ref — a commit SHA for a reproducible, churn-proof
# install, or a branch/tag for "latest". Defaults to the branch.
ref      := env_var_or_default("JUSTDOWN_REF", branch)
raw_base := env_var_or_default("JUSTDOWN_RAW_BASE", "https://raw.githubusercontent.com/" + repo + "/" + ref)
format   := env_var_or_default("JUSTDOWN_FORMAT", "text")
idx_schema  := "2"
cli_version := "0.1.0"

# default: show what the CLI can do
_default:
    @just --justfile "{{justfile()}}" help

# build the local index from <lib>/**/*.jd (cross-platform, no node)
build:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; lib={{quote(lib)}}; index={{quote(index)}}; schema={{quote(idx_schema)}}
    libdir="$root/$lib"
    out="$root/$index"
    [ -d "$libdir" ] || { echo "jd: no library dir: $libdir" >&2; exit 1; }
    trap 'rm -f "$out.tmp" "$out.hdr"' EXIT
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
          else if ($0 ~ /^use_when:/)    usew=arr($0)
          else if ($0 ~ /^not_when:/)    notw=arr($0)
          else if ($0 ~ /^danger:/)        danger=val($0)
          else if ($0 ~ /^side_effects:/)  effects=arr($0)
          else if ($0 ~ /^requires:/)      requires=arr($0)
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
          cat=(n>=2 ? a[n-1] : "")          # category = the parent folder, inferred (no authoring cost)
          if (name=="") name=key
          purpose=(desc!="" ? desc : name)
          printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", key, name, kind, purpose, tags, rel, links, usew, notw, danger, effects, requires, cat
        }
      ' "$f" >> "$out.tmp"
    done
    LC_ALL=C sort -o "$out.tmp" "$out.tmp"
    # stamp a versioned header line so a consumer can detect a format it predates
    # (write it AFTER the sort so it always stays line 1)
    { printf '#jdschema\t%s\n' "$schema"; cat "$out.tmp"; } > "$out.hdr"
    mv "$out.hdr" "$out"
    trap - EXIT; rm -f "$out.tmp"
    echo "built $out: $(grep -cv '^#' "$out" | tr -d ' ') entries (schema $schema)" >&2

# rank library files by a natural-language need (optional: <kind> <num> <category>)
# category narrows to one folder (e.g. docker, git) before scoring — like <kind>
# output: text (default) or JSON when JUSTDOWN_FORMAT=json (schema justdown.search/1)
# exit:   0 ok · 2 no matches · 3 bad args · 4 sources unreachable
search query kind="" num="5" category="":
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}; format={{quote(format)}}
    kind={{quote(kind)}}; num={{quote(num)}}; category={{quote(category)}}
    # user input crosses into awk via ENVIRON (no -v backslash-escape processing)
    export JD_QUERY={{quote(query)}} JD_KIND="$kind" JD_NUM="$num" JD_BASE="$raw_base" JD_CAT="$category"
    # machine-distinguishable errors: JSON envelope on stderr in json mode, prose otherwise
    emit_err() {
      if [ "$format" = json ]; then
        msg=$(printf '%s' "$2" | sed 's/\\/\\\\/g; s/"/\\"/g')
        printf '{"schema":"justdown.error/1","error":"%s","message":"%s"}\n' "$1" "$msg" >&2
      else
        echo "jd: $2" >&2
      fi
    }
    # validate args up front (exit 3)
    case "$format" in text|json) ;; *) emit_err bad-args "unknown JUSTDOWN_FORMAT: $format (want text|json)"; exit 3 ;; esac
    if [ -n "$kind" ]; then
      case "$kind" in tool|agent|knowledge|workflow) ;; *) emit_err bad-args "unknown kind: $kind (want tool|agent|knowledge|workflow)"; exit 3 ;; esac
    fi
    case "$num" in ''|*[!0-9]*) emit_err bad-args "num must be a positive integer: $num"; exit 3 ;; esac
    # gather the merged index; track which sources answered so we can degrade, not fail
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    have_local=0; have_online=0
    if [ -f "$root/$index" ]; then have_local=1; awk '/^#/{next} NF{print "local\t" $0}' "$root/$index" > "$tmp"; fi
    if online=$(curl -fsSL "$raw_base/$index" 2>/dev/null); then
      have_online=1; printf '%s\n' "$online" | awk '/^#/{next} NF{print "online\t" $0}' >> "$tmp"
    fi
    if [ "$have_local" -eq 0 ] && [ "$have_online" -eq 0 ]; then
      emit_err source-unreachable "no local index and online index unreachable ($raw_base/$index)"; exit 4
    fi
    [ "$have_online" -eq 0 ] && [ "$have_local" -eq 1 ] && echo "jd: note: online index unreachable; using local only" >&2 || true
    # score, then sort by score desc with a deterministic name tie-break (reproducible output)
    tab=$(printf '\t')
    scored=$(awk -F'\t' '!seen[$2]++' "$tmp" | awk -F'\t' '
      # a term hits a field only inside a token boundary, not across the whole
      # string — stops short terms ("on") matching unrelated words ("production")
      function fhit(field, term,   n,w,i){ n=split(field, w, /[^a-z0-9+]+/)
        for (i=1;i<=n;i++) if (w[i]!="" && index(w[i],term)) return 1; return 0 }
      BEGIN{ q=tolower(ENVIRON["JD_QUERY"]); kind=ENVIRON["JD_KIND"]; cat=ENVIRON["JD_CAT"]; base=ENVIRON["JD_BASE"]
             m=split(q, raw, /[^a-z0-9+]+/)
             split("a an and or the of to in on at is it its be as do for my our your this that with from by", sl, " ")
             for (i in sl) stop[sl[i]]=1
             nt=0; for (i=1;i<=m;i++) if (raw[i]!="" && !(raw[i] in stop)) qt[++nt]=raw[i] }
      kind!="" && $4!=kind { next }
      cat!=""  && $14!=cat { next }
      {
        # field-weighted scoring; each term counts once at its strongest field.
        # name / use_when outrank tags, tags outrank free prose. not_when vetoes.
        name=tolower($3); purpose=tolower($5); tags=tolower($6); usew=tolower($9); notw=tolower($10)
        score=0; vetoed=0
        for (i=1;i<=nt;i++) {
          t=qt[i]
          if (notw!="" && fhit(notw,t)) { vetoed=1; break }
          if      (fhit(name,t))    score+=3
          else if (fhit(usew,t))    score+=3
          else if (fhit(tags,t))    score+=2
          else if (fhit(purpose,t)) score+=1
        }
        if (vetoed) next
        if (score>0) {
          rawclean=($1=="local" ? $7 : base "/" $7)
          # carry safety metadata through so a gate can read it without get/prose
          printf "%d\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", score, $3, $4, $5, rawclean, $1, $11, $12, $13
        }
      }
    ' | LC_ALL=C sort -t"$tab" -k1,1nr -k2,2)
    # one formatter for both modes; empty input still yields a valid empty JSON envelope
    printf '%s' "$scored" | awk -F'\t' -v fmt="$format" '
      function jstr(s){ gsub(/\\/,"\\\\",s); gsub(/"/,"\\\"",s); gsub(/\t/,"\\t",s); gsub(/\r/,"\\r",s); return "\"" s "\"" }
      function jarr(s,   n,a,i,o){ if(s=="")return "[]"; n=split(s,a,","); o="["
        for(i=1;i<=n;i++){ if(i>1)o=o","; o=o jstr(a[i]) } return o "]" }
      BEGIN{ n=ENVIRON["JD_NUM"]+0; if (n<=0) n=5
             if (fmt=="json") printf "{\"schema\":\"justdown.search/1\",\"query\":%s,\"results\":[", jstr(ENVIRON["JD_QUERY"]) }
      NF==0 { next }
      NR>n  { next }
      fmt=="json" {
        if (jn++) printf ","
        dang=($7==""?"none":$7)
        printf "{\"name\":%s,\"kind\":%s,\"score\":%d,\"purpose\":%s,\"raw\":%s,\"source\":%s,\"danger\":%s,\"side_effects\":%s,\"requires\":%s}", \
          jstr($2), jstr($3), $1, jstr($4), jstr($5), jstr($6), jstr(dang), jarr($8), jarr($9)
        next
      }
      { raw=$5; if ($6=="local") raw=raw " (local)"
        printf "%d. %s  [%s]  score %s\n   %s\n   %s\n", NR, $2, $3, $1, $4, raw
        # only surface a safety line when it matters — destructive or has effects
        if ($7=="high" || $7=="medium" || $8!="") {
          line="   ⚠ danger=" ($7==""?"none":$7)
          if ($8!="") line=line "  effects=" $8
          if ($9!="") line=line "  requires=" $9
          print line
        } }
      END{ if (fmt=="json") print "]}" }
    '
    [ -n "$scored" ] || exit 2

# pull one file as ordered sections: [0] frontmatter, then prose | tools
# (optional <only>: frontmatter | prose | tools)
# output: text (default) or JSON (schema justdown.get/1) when JUSTDOWN_FORMAT=json
# exit:   0 ok · 2 no such file · 3 bad args · 4 source unreachable
get ref only="":
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}; format={{quote(format)}}
    only={{quote(only)}}
    export JD_REF={{quote(ref)}} JD_ONLY="$only"
    # resolve the host platform for recipe-variant selection (justdown extension):
    # [unix]/[macos]/[windows]/[wsl] attrs are picked here and stripped before the
    # block reaches `just`, which has no [wsl] of its own. uname + /proc/version,
    # per spec. JD_PLATFORM, if preset, wins — the test/CI seam, not a public knob.
    if [ -z "${JD_PLATFORM:-}" ]; then
      case "$(uname -s 2>/dev/null)" in
        Darwin) JD_PLATFORM=macos ;;
        Linux)  if grep -qi microsoft /proc/version 2>/dev/null || [ -n "${WSL_DISTRO_NAME:-}" ]; then JD_PLATFORM=wsl; else JD_PLATFORM=unix; fi ;;
        *MINGW*|*MSYS*|*CYGWIN*|*Windows_NT*) JD_PLATFORM=windows ;;
        *) JD_PLATFORM=unix ;;
      esac
    fi
    export JD_PLATFORM
    emit_err() {
      if [ "$format" = json ]; then
        msg=$(printf '%s' "$2" | sed 's/\\/\\\\/g; s/"/\\"/g')
        printf '{"schema":"justdown.error/1","error":"%s","message":"%s"}\n' "$1" "$msg" >&2
      else echo "jd: $2" >&2; fi
    }
    case "$format" in text|json) ;; *) emit_err bad-args "unknown JUSTDOWN_FORMAT: $format (want text|json)"; exit 3 ;; esac
    case "$only" in ''|frontmatter|prose|tools) ;; *) emit_err bad-args "unknown only: $only (want frontmatter|prose|tools)"; exit 3 ;; esac
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    have_local=0; have_online=0
    if [ -f "$root/$index" ]; then have_local=1; awk '/^#/{next} NF{print "local\t" $0}' "$root/$index" > "$tmp"; fi
    if online=$(curl -fsSL "$raw_base/$index" 2>/dev/null); then
      have_online=1; printf '%s\n' "$online" | awk '/^#/{next} NF{print "online\t" $0}' >> "$tmp"
    fi
    if [ "$have_local" -eq 0 ] && [ "$have_online" -eq 0 ]; then
      emit_err source-unreachable "no local index and online index unreachable ($raw_base/$index)"; exit 4
    fi
    [ "$have_online" -eq 0 ] && [ "$have_local" -eq 1 ] && echo "jd: note: online index unreachable; using local only" >&2 || true
    row=$(awk -F'\t' '!seen[$2]++' "$tmp" | awk -F'\t' '
      function base(p){ sub(/\.jd$/,"",p); n=split(p,a,"/"); return a[n] }
      BEGIN{ ref=ENVIRON["JD_REF"]; sub(/^@/,"",ref); sub(/#.*$/,"",ref) }
      { if ($3==ref || $2==ref || $7==ref || base($7)==ref){ print $1 "\t" $7; exit } }')
    [ -n "$row" ] || { emit_err not-found "no file: $JD_REF"; exit 2; }
    src=$(printf '%s' "$row" | cut -f1)
    path=$(printf '%s' "$row" | cut -f2)
    case "$path" in /*|*..*) emit_err bad-args "refusing suspicious path: $path"; exit 3 ;; esac
    if [ "$src" = local ]; then
      body=$(cat "$root/$path") || { emit_err source-unreachable "cannot read local file: $path"; exit 4; }
    else
      body=$(curl -fsSL "$raw_base/$path") || { emit_err source-unreachable "cannot fetch: $raw_base/$path"; exit 4; }
    fi
    printf '%s\n' "$body" | awk -v fmt="$format" '
      function jstr(s){ gsub(/\\/,"\\\\",s); gsub(/"/,"\\\"",s); gsub(/\n/,"\\n",s); gsub(/\t/,"\\t",s); gsub(/\r/,"\\r",s); return "\"" s "\"" }
      function addsec(kind, content){ sk[++ns]=kind; sc[ns]=content }
      BEGIN{ only=ENVIRON["JD_ONLY"]; plat=ENVIRON["JD_PLATFORM"]; if(plat=="")plat="unix" }
      # justdown extension: select platform-guarded recipe variants and strip the
      # [unix]/[macos]/[windows]/[wsl] attr lines so plain `just` never sees them.
      # A guard accepts a comma list ([unix, wsl]); darwin is an alias for macos.
      # An attr line guards the recipe header that follows and its indented body;
      # untagged lines always pass through. Authors keep variants of one recipe
      # mutually exclusive per platform so `just` gets exactly one definition.
      function platsel(n,   i,line,tg,parts,np,j,t,want,out,pend,guarded,pmode){
        pend=0; guarded=0; pmode="emit"; out=""
        for(i=1;i<=n;i++){
          line=jl[i]
          if(line ~ /^[ \t]*\[[ \t]*(unix|macos|darwin|windows|wsl)([ \t]*,[ \t]*(unix|macos|darwin|windows|wsl))*[ \t]*\][ \t]*$/){
            tg=line; sub(/^[ \t]*\[/,"",tg); sub(/\][ \t]*$/,"",tg); gsub(/[ \t]/,"",tg)
            np=split(tg,parts,","); want=0
            for(j=1;j<=np;j++){ t=parts[j]; if(t=="darwin")t="macos"; if(t==plat)want=1 }
            pmode=(want?"emit":"drop"); pend=1; guarded=0; continue
          }
          if(pend){ pend=0; guarded=1; if(pmode=="emit")out=out (out==""?"":"\n") line; continue }
          if(guarded){
            if(line ~ /^[ \t]/ || line==""){ if(pmode=="emit")out=out (out==""?"":"\n") line; continue }
            guarded=0; pmode="emit"
          }
          out=out (out==""?"":"\n") line
        }
        return out
      }
      function flush(  i,isjust,injust,njl,buf){
        if (bn==0) return
        isjust=0
        for (i=1;i<=bn;i++) if (blk[i] ~ /^```just/) isjust=1
        if (isjust && (only=="" || only=="tools")) {
          njl=0; injust=0; split("", jl)
          for (i=1;i<=bn;i++){
            if (blk[i] ~ /^```just/){ injust=1; continue }
            if (injust && blk[i] ~ /^```/){ injust=0; continue }
            if (injust) jl[++njl]=blk[i]
          }
          addsec("tools", platsel(njl))
        } else if (!isjust && (only=="" || only=="prose")) {
          buf=""; for (i=1;i<=bn;i++) buf=buf (buf==""?"":"\n") blk[i]
          addsec("prose", buf)
        }
        bn=0; split("", blk)
      }
      NR==1 && $0=="---" { infm=1; if (only==""||only=="frontmatter") collectfm=1; next }
      infm==1 && $0=="---" { infm=2; if (collectfm) addsec("frontmatter", fmbuf); next }
      infm==1 { if (collectfm) fmbuf=fmbuf (fmbuf==""?"":"\n") $0; next }
      { if ($0 ~ /^```/) fence=!fence
        if (!fence && $0=="---"){ flush(); next }
        if (bn>0 || $0!="") blk[++bn]=$0 }
      END{
        flush()
        if (fmt=="json"){
          printf "{\"schema\":\"justdown.get/1\",\"ref\":%s,\"sections\":[", jstr(ENVIRON["JD_REF"])
          for (i=1;i<=ns;i++){ if(i>1)printf ","; printf "{\"kind\":%s,\"content\":%s}", jstr(sk[i]), jstr(sc[i]) }
          print "]}"
        } else {
          for (i=1;i<=ns;i++){ print "# " sk[i]; print sc[i]; print "" }
        }
      }
    '

# list categories (grouped by parent folder) and their member files
# output: text (default) or JSON (schema justdown.ls/1) when JUSTDOWN_FORMAT=json
# exit:   0 ok · 3 bad args · 4 sources unreachable
ls:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}; format={{quote(format)}}
    emit_err() {
      if [ "$format" = json ]; then
        msg=$(printf '%s' "$2" | sed 's/\\/\\\\/g; s/"/\\"/g')
        printf '{"schema":"justdown.error/1","error":"%s","message":"%s"}\n' "$1" "$msg" >&2
      else echo "jd: $2" >&2; fi
    }
    case "$format" in text|json) ;; *) emit_err bad-args "unknown JUSTDOWN_FORMAT: $format (want text|json)"; exit 3 ;; esac
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    have_local=0; have_online=0
    if [ -f "$root/$index" ]; then have_local=1; awk '/^#/{next} NF{print "local\t" $0}' "$root/$index" > "$tmp"; fi
    if online=$(curl -fsSL "$raw_base/$index" 2>/dev/null); then
      have_online=1; printf '%s\n' "$online" | awk '/^#/{next} NF{print "online\t" $0}' >> "$tmp"
    fi
    if [ "$have_local" -eq 0 ] && [ "$have_online" -eq 0 ]; then
      emit_err source-unreachable "no local index and online index unreachable ($raw_base/$index)"; exit 4
    fi
    [ "$have_online" -eq 0 ] && [ "$have_local" -eq 1 ] && echo "jd: note: online index unreachable; using local only" >&2 || true
    awk -F'\t' '!seen[$2]++' "$tmp" | awk -F'\t' -v fmt="$format" '
      function jstr(s){ gsub(/\\/,"\\\\",s); gsub(/"/,"\\\"",s); gsub(/\t/,"\\t",s); gsub(/\r/,"\\r",s); return "\"" s "\"" }
      { cat=($14!="" ? $14 : ($4!="" ? $4 : "misc"))   # group by parent folder, fall back to kind
        cats[cat]=cats[cat] " " $3 }
      END{
        nc=0; for (c in cats) ord[++nc]=c                         # deterministic category order
        for (i=2;i<=nc;i++){ k=ord[i]; j=i-1; while(j>=1 && ord[j]>k){ord[j+1]=ord[j];j--}; ord[j+1]=k }
        if (fmt=="json"){
          printf "{\"schema\":\"justdown.ls/1\",\"categories\":["
          for (i=1;i<=nc;i++){ c=ord[i]; if(i>1)printf ","
            printf "{\"name\":%s,\"members\":[", jstr(c)
            m=split(cats[c], mm, " "); first=1
            for (x=1;x<=m;x++){ if(mm[x]=="")continue; if(!first)printf ","; first=0; printf "%s", jstr(mm[x]) }
            printf "]}" }
          print "]}"
        } else { for (i=1;i<=nc;i++){ c=ord[i]; print c ":" cats[c] } }
      }'

# inbound + outbound @links of a file
# output: text (default) or JSON (schema justdown.links/1) when JUSTDOWN_FORMAT=json
# exit:   0 ok · 2 no such file · 3 bad args · 4 sources unreachable
links ref:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; raw_base={{quote(raw_base)}}; format={{quote(format)}}
    export JD_REF={{quote(ref)}}
    emit_err() {
      if [ "$format" = json ]; then
        msg=$(printf '%s' "$2" | sed 's/\\/\\\\/g; s/"/\\"/g')
        printf '{"schema":"justdown.error/1","error":"%s","message":"%s"}\n' "$1" "$msg" >&2
      else echo "jd: $2" >&2; fi
    }
    case "$format" in text|json) ;; *) emit_err bad-args "unknown JUSTDOWN_FORMAT: $format (want text|json)"; exit 3 ;; esac
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    have_local=0; have_online=0
    if [ -f "$root/$index" ]; then have_local=1; awk '/^#/{next} NF{print "local\t" $0}' "$root/$index" > "$tmp"; fi
    if online=$(curl -fsSL "$raw_base/$index" 2>/dev/null); then
      have_online=1; printf '%s\n' "$online" | awk '/^#/{next} NF{print "online\t" $0}' >> "$tmp"
    fi
    if [ "$have_local" -eq 0 ] && [ "$have_online" -eq 0 ]; then
      emit_err source-unreachable "no local index and online index unreachable ($raw_base/$index)"; exit 4
    fi
    [ "$have_online" -eq 0 ] && [ "$have_local" -eq 1 ] && echo "jd: note: online index unreachable; using local only" >&2 || true
    m=$(awk -F'\t' '!seen[$2]++' "$tmp")
    key=$(printf '%s\n' "$m" | awk -F'\t' '
      function base(p){ sub(/\.jd$/,"",p); n=split(p,a,"/"); return a[n] }
      BEGIN{ ref=ENVIRON["JD_REF"]; sub(/^@/,"",ref); sub(/#.*$/,"",ref) }
      { if ($3==ref || $2==ref || $7==ref || base($7)==ref){ print $2; exit } }')
    [ -n "$key" ] || { emit_err not-found "no file: $JD_REF"; exit 2; }
    printf '%s\n' "$m" | JD_KEY="$key" awk -F'\t' -v fmt="$format" '
      function jstr(s){ gsub(/\\/,"\\\\",s); gsub(/"/,"\\\"",s); gsub(/\t/,"\\t",s); gsub(/\r/,"\\r",s); return "\"" s "\"" }
      BEGIN{ key=ENVIRON["JD_KEY"] }
      { keys[$2]=1; row[NR]=$0 }
      END{
        no=0; ni=0
        for (r=1;r<=NR;r++){ split(row[r], c, "\t")
          if (c[2]==key){ nn=split(c[8], o, ","); for (i=1;i<=nn;i++){ t=o[i]; sub(/^@/,"",t)
            if (t!="" && t!=key && (t in keys)) outl[++no]=t } } }
        for (r=1;r<=NR;r++){ split(row[r], c, "\t")
          if (c[2]!=key && index(c[8], "@" key)) inl[++ni]=c[3] }
        if (fmt=="json"){
          printf "{\"schema\":\"justdown.links/1\",\"ref\":%s,\"key\":%s,\"outbound\":[", jstr(ENVIRON["JD_REF"]), jstr(key)
          for (i=1;i<=no;i++){ if(i>1)printf ","; printf "%s", jstr(outl[i]) }
          printf "],\"inbound\":["
          for (i=1;i<=ni;i++){ if(i>1)printf ","; printf "%s", jstr(inl[i]) }
          print "]}"
        } else {
          for (i=1;i<=no;i++) print "out  @" outl[i]
          for (i=1;i<=ni;i++) print "in   " inl[i] "  (@" key ")"
        }
      }'

# score retrieval quality against eval/queries.tsv — precision@1 + MRR
# dev tooling: proves a ranking change helped instead of guessing it
eval:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; jf={{quote(justfile())}}
    qfile="$root/eval/queries.tsv"
    [ -f "$qfile" ] || { echo "jd: no eval file: $qfile" >&2; exit 1; }
    tab=$(printf '\t')
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    while IFS="$tab" read -r query expected; do
      case "$query" in ''|'#'*) continue ;; esac
      [ -n "$expected" ] || continue
      res=$(just --justfile "$jf" search "$query" "" 20 </dev/null 2>/dev/null || true)
      rank=$(printf '%s\n' "$res" | awk -v want="$expected" '
        /^[0-9]+\./ { n=$1; sub(/\./,"",n); if ($2==want){ print n+0; exit } }')
      printf '%s\t%s\t%s\n' "$query" "$expected" "${rank:-0}" >> "$tmp"
    done < "$qfile"
    awk -F'\t' '
      BEGIN{ printf "%-5s %-40s %s\n", "RANK", "QUERY", "EXPECTED" }
      NF<3 { next }
      { total++; r=$3+0
        if (r==1) h1++
        if (r>0) mrr+=1/r
        printf "%-5s %-40.40s %s\n", (r>0?r:"MISS"), $1, $2 }
      END{ if (total>0) printf "\nprecision@1=%.3f   MRR=%.3f   (n=%d, top-1 hits=%d)\n", h1/total, mrr/total, total, h1+0 }
    ' "$tmp"

# validate the local library's .jd frontmatter — required fields, enums,
# duplicate name/key, broken @links. Exit non-zero on errors (CI-gateable).
lint:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; lib={{quote(lib)}}
    libdir="$root/$lib"
    [ -d "$libdir" ] || { echo "jd: no library dir: $libdir" >&2; exit 1; }
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    find "$libdir" -name '*.jd' | LC_ALL=C sort | while IFS= read -r f; do
      rel=${f#"$root/"}
      JD_REL="$rel" awk '
        BEGIN{ rel=ENVIRON["JD_REL"] }
        function val(s){ sub(/^[^:]*:[ \t]*/,"",s); gsub(/[\t]/," ",s); sub(/[ \r]+$/,"",s); return s }
        NR==1 && $0=="---" { fm=1; next }
        fm==1 && $0=="---" { fm=2; next }
        fm==1 {
          if      ($0 ~ /^name:/)        name=val($0)
          else if ($0 ~ /^kind:/)        kind=val($0)
          else if ($0 ~ /^description:/) desc=val($0)
          else if ($0 ~ /^run:/)         run=val($0)
          else if ($0 ~ /^danger:/)      danger=val($0)
          else if ($0 ~ /^use_when:/)    usew=1
          next
        }
        fm==2 {
          s=$0
          while (match(s, /@[a-z0-9_]+\/[a-z0-9_]+/)) {
            l=substr(s, RSTART, RLENGTH); sub(/^@/,"",l)
            if (!(l in seen)) { seen[l]=1; links=links (links==""?l:","l) }
            s=substr(s, RSTART+RLENGTH) }
        }
        END{
          p=rel; sub(/\.jd$/,"",p); n=split(p,a,"/"); key=(n>=2?a[n-1]"/"a[n]:a[n])
          printf "%s\t%s\t%s\t%s\t%d\t%d\t%s\t%d\t%s\t%d\n", \
            rel, key, name, kind, (desc!=""), (run!=""), danger, usew, links, (fm>=2)
        }
      ' "$f" >> "$tmp"
    done
    awk -F'\t' '
      { recs[NR]=$0; keys[$2]=1; keycount[$2]++; if($3!="") namecount[$3]++ }
      END{
        errs=0; warns=0
        for (i=1;i<=NR;i++){
          split(recs[i], c, "\t")
          rel=c[1];key=c[2];name=c[3];kind=c[4];hasdesc=c[5]+0;hasrun=c[6]+0;danger=c[7];usew=c[8]+0;links=c[9];hasfm=c[10]+0
          msg=""
          if (!hasfm) { msg="  error: no frontmatter block\n" }
          else {
            if (name=="")    msg=msg "  error: missing required field: name\n"
            if (!hasdesc)    msg=msg "  error: missing required field: description\n"
            if (kind=="")    msg=msg "  error: missing required field: kind\n"
            else if (kind !~ /^(tool|agent|knowledge|workflow)$/) msg=msg "  error: invalid kind: " kind " (want tool|agent|knowledge|workflow)\n"
            if (kind=="tool" && !hasrun) msg=msg "  error: tool has no `run:` recipe\n"
            if (danger!="" && danger !~ /^(none|low|medium|high)$/) msg=msg "  error: invalid danger: " danger " (want none|low|medium|high)\n"
            if (name!="" && namecount[name]>1) msg=msg "  error: duplicate name: " name "\n"
            if (keycount[key]>1)               msg=msg "  error: duplicate key: " key "\n"
            if (links!=""){ m=split(links, ll, ","); for(j=1;j<=m;j++) if(!(ll[j] in keys)){
              # scaffolds/knowledge legitimately reference the user'\''s own modules;
              # only flag real .jd-to-.jd composition (tool/agent/workflow) as an error.
              if (kind=="knowledge") msg=msg "  warn: unresolved @link: " ll[j] " (external reference?)\n"
              else                   msg=msg "  error: broken @link: " ll[j] "\n" } }
            if ((kind=="tool"||kind=="workflow") && !usew) msg=msg "  warn: no use_when (retrieval leans on description alone)\n"
          }
          if (msg!=""){ print "lint: " rel; printf "%s", msg
            errs+=gsub(/  error:/,"\\&",msg); warns+=gsub(/  warn:/,"\\&",msg) }
        }
        printf "\n%d error(s), %d warning(s) across %d file(s)\n", errs, warns, NR
        if (errs>0) exit 1
      }
    ' "$tmp"

# print CLI + index-schema versions; warn if the local index predates this CLI
version:
    #!/usr/bin/env sh
    set -eu
    root={{quote(root)}}; index={{quote(index)}}; schema={{quote(idx_schema)}}; ver={{quote(cli_version)}}
    printf 'justdown-cli %s  ·  index schema justdown.index/%s\n' "$ver" "$schema"
    f="$root/$index"
    if [ ! -f "$f" ]; then printf 'local index: none — run `just build`\n'; exit 0; fi
    hv=$(awk -F'\t' 'NR==1 && $1=="#jdschema"{print $2+0; exit}' "$f")
    if [ -z "$hv" ]; then
      printf 'local index: unversioned (legacy) — run `just build` to stamp it\n'
    elif [ "$hv" -gt "$schema" ]; then
      printf 'jd: warning: local index is schema %s but this CLI supports %s — upgrade the CLI or `just build`\n' "$hv" "$schema" >&2
    else
      printf 'local index: schema %s (ok)\n' "$hv"
    fi

# print this CLI'\''s usage
help:
    #!/usr/bin/env sh
    cat <<'EOF'
    jd — justdown CLI (pure justfile) · build, query, and merge the .jd graph

    USAGE  just <recipe> [args]

    build                        scan <lib>/**/*.jd → write the local index (graph.tsv)
    search <query> [kind] [num]  rank library files by need (field-weighted:
                                  name/use_when > tags > prose; not_when vetoes)
    get    <ref> [only]          file as ordered sections: [0] frontmatter,
                                  then prose | tools  (only: frontmatter|prose|tools)
    ls                           categories and their member files
    links  <ref>                 inbound + outbound @links of a file
    eval                         score retrieval vs eval/queries.tsv (P@1 + MRR)
    lint                         validate library .jd frontmatter (CI-gateable)
    version                      CLI + index-schema versions; warn on index drift
    help                        this

    REF    name · path · key(dir/name) · @dir/name
    MERGE  queries union the LOCAL index with the ONLINE one; LOCAL trumps online
           entries by key. Build the local index with `just build`.
    OUTPUT text (default) or machine JSON via JUSTDOWN_FORMAT=json (versioned
           schema, e.g. justdown.search/1; errors as justdown.error/1 on stderr).
    EXIT   0 ok · 2 no matches · 3 bad args · 4 sources unreachable
    ENV    JUSTDOWN_LIB (default library)  JUSTDOWN_REPO  JUSTDOWN_BRANCH
           JUSTDOWN_REF (pin to a commit SHA for a reproducible install)
           JUSTDOWN_RAW_BASE  JUSTDOWN_FORMAT (text|json)
    EOF
