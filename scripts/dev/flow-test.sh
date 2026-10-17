#!/bin/bash
# End-to-end app-flow test (crates/game/src/flow.rs): drives the whole app
# headless with virtual pads through every top-level path (title -> Versus
# at 4 / 8 / 12 archers with a full bot match, CO-OP Quest and Dark World,
# TRIALS, ARCHIVES, OPTIONS) and fails if a screen is not reached or
# anything panics. One frame per step goes to scratch/flow/, the sheet to
# scratch/flow-sheet.png.
#
# Usage: scripts/dev/flow-test.sh [path/to/towerfall-port] [--only 4|8|12]
# (default binary: target/release/towerfall-port, built if missing).
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin="${CARGO_TARGET_DIR:-$root/target}/release/towerfall-port"
if [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; then
    bin="$1"
    shift
fi
if [ ! -x "$bin" ]; then
    (cd "$root" && source env.sh && cargo build --release -p towerfall-port)
fi
out="$root/scratch/flow"
status=0
"$bin" --flow-test "$out" "$@" || status=$?
py="${TFP_ROOT:-$root}/tools/venv/bin/python"
"$py" -I "$root/scripts/dev/contact-sheet.py" "$root/scratch/flow-sheet.png" --cols 6 "$out"
if [ "$status" -eq 0 ]; then
    echo "FLOW PASS ($(wc -l < "$out/steps.tsv") steps)"
else
    echo "FLOW FAIL (see the NOT REACHED tiles in scratch/flow-sheet.png)"
fi
exit "$status"
