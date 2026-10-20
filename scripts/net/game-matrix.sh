#!/usr/bin/env bash
# Online play with the real game: full Versus matches between headless
# towerfall-port processes (bots) over a local warpzone-server.
#
#   scripts/net/game-matrix.sh [case-filter]
#
# Builds towerfall-port, starts scripts/net/dev-server.sh unless
# WARPZONE_SERVER answers. Logs: scratch/net/game-matrix/<case>.log.
# Wrap in perf-mode: up to 13 game processes for several minutes.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Tools and caches live in the main checkout (also when run from a worktree).
MAIN="${ROOT%%/.claude/worktrees/*}"
[ -f "$MAIN/env.sh" ] || MAIN="$ROOT"
source "$MAIN/env.sh"
export TFP_SDL2_LIB_DIR="${TFP_SDL2_LIB_DIR:-$MAIN/tools/lib}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export TFP_ROOT="$ROOT"
FILTER="${1:-}"
export WARPZONE_SERVER="${WARPZONE_SERVER:-http://127.0.0.1:${TFP_WZ_PORT:-3012}}"
OUT="$ROOT/scratch/net/game-matrix"
mkdir -p "$OUT"
SERVER_PID=""
if ! curl -sf "$WARPZONE_SERVER/api/v1/info" >/dev/null; then
  "$MAIN/scripts/net/dev-server.sh" >"$OUT/server.log" 2>&1 &
  SERVER_PID=$!
  for _ in $(seq 100); do curl -sf "$WARPZONE_SERVER/api/v1/info" >/dev/null && break; sleep 0.5; done
fi
trap '[ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null' EXIT
(cd "$ROOT" && cargo build -q -p towerfall-port) || exit 1
BIN="$CARGO_TARGET_DIR/debug/towerfall-port"

declare -a CASES=(
  "2x1-lms-quick|--locals 2x1 --mode lms --length quick"
  "3x1-hh-rtt80-loss3-code|--locals 3x1 --mode hh --length quick --private --delay 40 --jitter 10 --loss 3"
  "4x1-tdm-rtt100-loss5|--locals 4x1 --mode tdm --length quick --private --delay 50 --jitter 15 --loss 5"
  "4x3-lms-rtt60-loss3-spec|--locals 4x3 --spectators 1 --mode lms --length quick --delay 30 --jitter 10 --loss 3"
  "12x1-lms-rtt60-loss2|--locals 12x1 --mode lms --length quick --delay 30 --jitter 10 --loss 2"
  "3m-cataclysm-lms|--locals 2,1,1 --mode lms --length quick --tower cataclysm --delay 30 --jitter 10 --loss 3 --max-frames 18000"
  "2x1-darkworld-level1|--locals 2x1 --kind darkworld --coop-index 0 --dw-assist 0 --until-level 1 --delay 30 --jitter 10 --loss 2 --max-frames 18000"
  "2x1-darkworld-continue|--locals 2x1 --kind darkworld --coop-index 1 --dw-level 1 --dw-menu continue --delay 30 --loss 2 --max-frames 9000"
  "4x1-lms-leave-crash|--locals 4x1 --mode lms --length quick --delay 30 --jitter 10 --loss 2 --leave 2@1500 --crash 3@1000"
)
FAIL=0
for c in "${CASES[@]}"; do
  name="${c%%|*}"; args="${c#*|}"
  [ -n "$FILTER" ] && [[ "$name" != *$FILTER* ]] && continue
  # shellcheck disable=SC2086
  "$BIN" --net-harness $args --tag "$name" >"$OUT/$name.log" 2>&1 || FAIL=1
  printf '%-28s %s\n' "$name" "$(grep '^SUMMARY' "$OUT/$name.log" || echo 'SUMMARY FAIL (no summary)')"
done
exit $FAIL
