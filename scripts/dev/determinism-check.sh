#!/bin/bash
# Cross-platform determinism check: runs `sim-checksums` (fixed bot matches
# in every mode, checksums every 100 ticks) natively and as a Windows exe
# under Wine (scripts/run-windows.sh: sandboxed, private Xvfb) and diffs
# the two outputs. Any difference is a determinism bug.
#
# Usage: scripts/dev/determinism-check.sh [frames]   (default 3000)
# Output: scratch/determinism/{linux,windows}.txt; exit 1 on a mismatch.
set -euo pipefail
source "$(dirname "$0")/../../env.sh"
cd "$TFP_ROOT"
FRAMES=${1:-3000}
OUT=scratch/determinism
mkdir -p "$OUT"
PERF=~/.claude/skills/perf-mode/scripts/perf-mode
run_heavy() {
    if [ -x "$PERF" ]; then "$PERF" run -r "determinism check" -- "$@"; else "$@"; fi
}

echo "== building (linux, windows)"
run_heavy cargo build -q --release -p tfp-sim --bin sim-checksums
[ -d tools/windows/llvm-mingw ] || scripts/fetch-windows-toolchain.sh
(
    source scripts/windows-env.sh
    # The libgcc shims of fetch-windows-toolchain.sh (the game crate adds
    # them in its build.rs; the sim has no build script).
    export RUSTFLAGS="-L native=$TFP_ROOT/tools/windows/gcc-shim"
    run_heavy cargo build -q --release --target x86_64-pc-windows-gnu -p tfp-sim --bin sim-checksums
)

echo "== linux run ($FRAMES ticks per scenario)"
run_heavy target/release/sim-checksums "$FRAMES" > "$OUT/linux.txt"
echo "== windows run under wine"
EXE=target/x86_64-pc-windows-gnu/release/sim-checksums.exe \
    run_heavy scripts/run-windows.sh "$FRAMES" 2> "$OUT/windows.err" | tr -d '\r' > "$OUT/windows.txt"

lines=$(grep -c . "$OUT/linux.txt" || true)
skipped=$(grep -c skipped "$OUT/linux.txt" || true)
if diff -u "$OUT/linux.txt" "$OUT/windows.txt" > "$OUT/diff.txt"; then
    echo "OK: $lines lines identical ($skipped scenario notes: $(grep skipped "$OUT/linux.txt" | tr '\n' ';'))"
else
    first=$(grep -m1 '^[-+][a-z]' "$OUT/diff.txt" || true)
    echo "MISMATCH: first difference: $first (full diff: $OUT/diff.txt)"
    exit 1
fi
