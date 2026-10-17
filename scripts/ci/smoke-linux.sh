#!/bin/bash
# Smoke test of the Linux release tarball without a TowerFall install (CI):
# unpacks it into scratch/, checks the bundled SDL2 is the one loaded, then
# runs it headless with an empty HOME. It must start, show the "game data
# not found" screen, write a screenshot and exit cleanly.
# Usage: scripts/ci/smoke-linux.sh [dist/towerfall-port-*-linux-x86_64.tar.gz]
set -euo pipefail
cd "$(dirname "$0")/../.."
TAR=${1:-$(ls dist/towerfall-port-*-linux-x86_64.tar.gz | head -1)}
OUT=scratch/smoke-linux
rm -rf "$OUT"
mkdir -p "$OUT/home"
tar -xzf "$TAR" -C "$OUT"
BIN=$(ls "$OUT"/towerfall-port-*/towerfall-port)
ldd "$BIN" | grep 'libSDL2-2.0.so.0' | grep -q "$(dirname "$BIN")/lib/" \
  || { ldd "$BIN"; echo "bundled libSDL2 not used" >&2; exit 1; }
echo "glibc symbols needed: $(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
env -i PATH=/usr/bin:/bin HOME="$PWD/$OUT/home" WGPU_ADAPTER_NAME="${WGPU_ADAPTER_NAME:-}" \
  "$BIN" --headless --frames 30 --screenshot "$PWD/$OUT/shot.png"
[ -s "$OUT/shot.png" ] || { echo "no screenshot written" >&2; exit 1; }
echo "smoke: OK"
