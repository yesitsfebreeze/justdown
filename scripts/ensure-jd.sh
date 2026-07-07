#!/bin/sh
# justdown plugin SessionStart hook — keep the managed `jd` binary pinned to the
# plugin's version. Claude Code has no post-install/post-update hook, so this
# runs once per session: a fast no-op when the version already matches, a
# one-time checksum-verified download when the plugin was just installed or
# updated. Failures are non-blocking — a network hiccup must never break the
# session, so we always exit 0 and only warn on stderr.
set -eu

REPO="yesitsfebreeze/justdown"
ROOT="${CLAUDE_PLUGIN_ROOT:-}"
DATA="${CLAUDE_PLUGIN_DATA:-$HOME/.local/share/justdown}"
BIN="$DATA/bin/jd"

warn() { echo "justdown: $*" >&2; }

# desired version = the plugin's declared version
manifest="$ROOT/.claude-plugin/plugin.json"
[ -f "$manifest" ] || { warn "manifest not found ($manifest); skipping jd check"; exit 0; }
want_ver=$(grep -m1 '"version"' "$manifest" | cut -d'"' -f4)
[ -n "$want_ver" ] || { warn "could not read plugin version; skipping"; exit 0; }

# already correct? fast path, no network.
if [ -x "$BIN" ]; then
  have_ver=$("$BIN" version 2>/dev/null | head -n1 | awk '{print $2}')
  [ "$have_ver" = "$want_ver" ] && exit 0
fi

command -v curl >/dev/null 2>&1 || { warn "curl missing; cannot fetch jd $want_ver"; exit 0; }
command -v tar  >/dev/null 2>&1 || { warn "tar missing; cannot unpack jd $want_ver"; exit 0; }

os=$(uname -s); arch=$(uname -m)
case "$os" in
  Linux)  os_t=unknown-linux-gnu ;;
  Darwin) os_t=apple-darwin ;;
  *) warn "auto-install unsupported on '$os'; install jd $want_ver manually (go install github.com/$REPO/src/cmd/jd@latest)"; exit 0 ;;
esac
case "$arch" in
  x86_64|amd64)  arch_t=x86_64 ;;
  aarch64|arm64) arch_t=aarch64 ;;
  *) warn "auto-install unsupported on arch '$arch'; install jd $want_ver manually"; exit 0 ;;
esac

target="${arch_t}-${os_t}"
tag="v${want_ver}"
archive="jd-${tag}-${target}.tar.gz"
base="https://github.com/$REPO/releases/download/$tag"

tmp=$(mktemp -d) || { warn "mktemp failed"; exit 0; }
trap 'rm -rf "$tmp"' EXIT

warn "updating jd → $want_ver ($target)…"
if ! curl -fSL --connect-timeout 10 --max-time 120 "$base/$archive" -o "$tmp/$archive"; then
  warn "download failed: $base/$archive (keeping existing binary)"; exit 0
fi

# verify checksum against the release SHA256SUMS (best-effort)
if curl -fsSL --connect-timeout 10 --max-time 30 "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" 2>/dev/null; then
  want=$(grep " $archive\$" "$tmp/SHA256SUMS" | awk '{print $1}')
  if [ -n "$want" ]; then
    if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$tmp/$archive" | awk '{print $1}')
    elif command -v shasum  >/dev/null 2>&1; then got=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
    else got=""; fi
    if [ -n "$got" ] && [ "$want" != "$got" ]; then
      warn "checksum mismatch for $archive — refusing to install"; exit 0
    fi
  fi
fi

if ! tar -xzf "$tmp/$archive" -C "$tmp" 2>/dev/null || [ ! -f "$tmp/jd" ]; then
  warn "archive did not contain the jd binary"; exit 0
fi
mkdir -p "$DATA/bin"
install -m 0755 "$tmp/jd" "$BIN" || { warn "install to $BIN failed"; exit 0; }
warn "jd $want_ver ready ($BIN)"
exit 0
