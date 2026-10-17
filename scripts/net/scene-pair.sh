#!/usr/bin/env bash
# The real online scene (menus, lobby, match, rendering, audio path) in two
# headless towerfall-port processes over the local dev server: the host
# (2 bot archers) opens a room and starts when the guest (1 bot) is in;
# both quit after the match. PASS = both logged "match ended, agreed=true".
#
#   scripts/net/scene-pair.sh            # full match, screenshots at the end (lobby)
#   scripts/net/scene-pair.sh --mid 1800 # guest stops at tick 1800 (mid-match
#                                        # screenshot; the host sees it drop)
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MAIN="${ROOT%%/.claude/worktrees/*}"
[ -f "$MAIN/env.sh" ] || MAIN="$ROOT"
source "$MAIN/env.sh"
export TFP_SDL2_LIB_DIR="${TFP_SDL2_LIB_DIR:-$MAIN/tools/lib}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export WARPZONE_SERVER="${WARPZONE_SERVER:-http://127.0.0.1:${TFP_WZ_PORT:-3012}}"
OUT="$ROOT/scratch/net/scene"
mkdir -p "$OUT/homes/host" "$OUT/homes/guest"
MID=""
[ "${1:-}" = "--mid" ] && MID="${2:-1800}"
curl -sf "$WARPZONE_SERVER/api/v1/info" >/dev/null || { echo "no server at $WARPZONE_SERVER (scripts/net/dev-server.sh)"; exit 1; }
(cd "$ROOT" && cargo build -q -p towerfall-port) || exit 1
BIN="$CARGO_TARGET_DIR/debug/towerfall-port"
TAG="${MID:+mid}"; TAG="${TAG:-full}"
WARPZONE_HOME="$OUT/homes/host" TFP_PLAYER_NAME=hostbot "$BIN" --headless --frames 40000 \
  --online --online-host --online-bots 2 --online-autostart 2 --mode hh --length instant \
  --online-exit --native-screenshot "$OUT/host-$TAG.png" >"$OUT/host-$TAG.log" 2>&1 &
HOST=$!
ROOM=""
for _ in $(seq 120); do
  ROOM="$(sed -n 's/^online: hosting room \([a-z0-9]*\) .*/\1/p' "$OUT/host-$TAG.log" | head -1)"
  [ -n "$ROOM" ] && break
  sleep 0.5
done
[ -n "$ROOM" ] || { echo "host opened no room"; cat "$OUT/host-$TAG.log"; kill $HOST; exit 1; }
FRAMES=40000; [ -n "$MID" ] && FRAMES="$MID"
WARPZONE_HOME="$OUT/homes/guest" TFP_PLAYER_NAME=guestbot "$BIN" --headless --frames "$FRAMES" \
  --warpzone-join "$ROOM" --online-bots 1 --online-exit \
  --native-screenshot "$OUT/guest-$TAG.png" >"$OUT/guest-$TAG.log" 2>&1
wait $HOST
for w in host guest; do
  printf '%-6s %s\n' "$w" "$(grep -E '^online: match (started|ended)' "$OUT/$w-$TAG.log" | tr '\n' ' ')"
done
if [ -z "$MID" ]; then
  grep -q "match ended, agreed=true" "$OUT/host-$TAG.log" && grep -q "match ended, agreed=true" "$OUT/guest-$TAG.log" \
    && echo "SCENE PASS" || { echo "SCENE FAIL"; exit 1; }
else
  grep -q "match ended" "$OUT/host-$TAG.log" && echo "SCENE PASS (guest left at tick $MID, host finished)" || { echo "SCENE FAIL"; exit 1; }
fi
