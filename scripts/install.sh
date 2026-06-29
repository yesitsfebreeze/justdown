#!/bin/sh
# justdown installer — download a prebuilt `jd` binary for this host from the
# latest GitHub Release, verify its checksum, and install it.
#
#   curl -fsSL https://raw.githubusercontent.com/yesitsfebreeze/justdown/main/scripts/install.sh | sh
#
# Env: JD_INSTALL_DIR (default ~/.local/bin) · JD_VERSION (default: latest tag).
# Windows: run scripts/install.ps1 in PowerShell.
set -eu

REPO="yesitsfebreeze/justdown"
DEST="${JD_INSTALL_DIR:-$HOME/.local/bin}"

err() { echo "install: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || err "missing required tool: $1"; }
need curl
need tar

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)  os_t=unknown-linux-gnu ;;
  Darwin) os_t=apple-darwin ;;
  *) err "unsupported OS: $os — on Windows run scripts/install.ps1 in PowerShell (or 'cargo install --git https://github.com/$REPO jd')" ;;
esac
case "$arch" in
  x86_64|amd64)   arch_t=x86_64 ;;
  aarch64|arm64)  arch_t=aarch64 ;;
  *) err "unsupported architecture: $arch" ;;
esac
target="${arch_t}-${os_t}"

# resolve the version tag (latest unless pinned)
tag="${JD_VERSION:-}"
if [ -z "$tag" ]; then
  tag=$(curl -fsSL --connect-timeout 10 --max-time 30 \
        "https://api.github.com/repos/$REPO/releases/latest" \
        | grep -m1 '"tag_name"' | cut -d'"' -f4)
  [ -n "$tag" ] || err "could not resolve the latest release tag"
fi

archive="jd-${tag}-${target}.tar.gz"
base="https://github.com/$REPO/releases/download/$tag"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "install: fetching $archive ($tag)"
curl -fSL --connect-timeout 10 --max-time 120 "$base/$archive" -o "$tmp/$archive" \
  || err "download failed: $base/$archive"

# verify the checksum against the release's SHA256SUMS (best-effort: skip with a
# warning only if neither a checksum tool nor the sums file is available)
if curl -fsSL --connect-timeout 10 --max-time 30 "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" 2>/dev/null; then
  want=$(grep " $archive\$" "$tmp/SHA256SUMS" | awk '{print $1}')
  if [ -n "$want" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
      got=$(sha256sum "$tmp/$archive" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
      got=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
    else
      got=""
    fi
    if [ -n "$got" ]; then
      [ "$want" = "$got" ] || err "checksum mismatch for $archive"
      echo "install: checksum ok"
    else
      echo "install: warning: no sha256 tool, skipping verification" >&2
    fi
  fi
else
  echo "install: warning: SHA256SUMS not found, skipping verification" >&2
fi

tar -xzf "$tmp/$archive" -C "$tmp"
[ -f "$tmp/jd" ] || err "archive did not contain the jd binary"
mkdir -p "$DEST"
install -m 0755 "$tmp/jd" "$DEST/jd"
echo "install: jd $tag → $DEST/jd"

case ":$PATH:" in
  *":$DEST:"*) ;;
  *) echo "install: note — add $DEST to your PATH" >&2 ;;
esac
