#!/bin/sh
# MCP entrypoint for the justdown plugin. The real `jd` binary lives in the
# plugin's persistent data dir, downloaded by scripts/ensure-jd.sh (run from the
# SessionStart hook). This wrapper closes the cold-start race: if MCP is spawned
# before that hook has fetched the binary — e.g. the very first session after a
# fresh install — it runs the installer itself, then execs. ROOT/DATA arrive via
# the mcpServers `env` block in plugin.json.
set -eu

DATA="${CLAUDE_PLUGIN_DATA:-$HOME/.local/share/justdown}"
BIN="$DATA/bin/jd"

if [ ! -x "$BIN" ] && [ -n "${CLAUDE_PLUGIN_ROOT:-}" ] && [ -x "$CLAUDE_PLUGIN_ROOT/scripts/ensure-jd.sh" ]; then
	"$CLAUDE_PLUGIN_ROOT/scripts/ensure-jd.sh" >&2 || true
fi

[ -x "$BIN" ] || {
	echo "justdown: jd binary not available at $BIN — the SessionStart hook (scripts/ensure-jd.sh) could not install it; check network or install jd manually." >&2
	exit 1
}

exec "$BIN" "$@"
