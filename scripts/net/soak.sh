#!/usr/bin/env bash
# Online soak: many random full matches (towers, modes, 2-12 archers on 2-4
# machines, variants, treasure, latency/loss) against the local dev server.
#
#   scripts/net/soak.sh [count] [seed] [parallel]     # default 30 1 2
#
# Logs: scratch/net/soak/<n>.log; summary on stdout. Wrap in perf-mode.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MAIN="${ROOT%%/.claude/worktrees/*}"
[ -f "$MAIN/env.sh" ] || MAIN="$ROOT"
source "$MAIN/env.sh"
export TFP_SDL2_LIB_DIR="${TFP_SDL2_LIB_DIR:-$MAIN/tools/lib}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export TFP_ROOT="$ROOT"
export WARPZONE_SERVER="${WARPZONE_SERVER:-http://127.0.0.1:${TFP_WZ_PORT:-3012}}"
SERVER_PID=""
if ! curl -sf "$WARPZONE_SERVER/api/v1/info" >/dev/null; then
  mkdir -p "$ROOT/scratch/net/soak"
  "$MAIN/scripts/net/dev-server.sh" >"$ROOT/scratch/net/soak/server.log" 2>&1 &
  SERVER_PID=$!
  for _ in $(seq 100); do curl -sf "$WARPZONE_SERVER/api/v1/info" >/dev/null && break; sleep 0.5; done
fi
trap '[ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null' EXIT
(cd "$ROOT" && cargo build -q -p towerfall-port) || exit 1
exec "$CARGO_TARGET_DIR/debug/towerfall-port" --net-soak --count "${1:-30}" --seed "${2:-1}" --parallel "${3:-2}"
