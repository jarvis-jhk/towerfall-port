#!/bin/bash
# Headless proof of the Quest / Trials presentation (M9): bots play the
# first Quest tower past wave 3, the game over and complete screens are
# forced on short runs, a bot clears a Trials level; contact sheets go to
# scratch/quest-sheet.png and scratch/trials-sheet.png.
#
# Usage: scripts/dev/coop-proof.sh [path/to/towerfall-port]
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin="${1:-${CARGO_TARGET_DIR:-$root/target}/debug/towerfall-port}"
out="$root/scratch/coop-proof"
rm -rf "$out"
mkdir -p "$out"
run() { "$bin" --headless "$@" | grep -E "capture|coop:" || true; }
run --play --mode quest --tower 0 --players 2 --frames 7000 --shots "$out/quest"
run --play --mode quest --tower 0 --players 2 --frames 1900 --quest-end gameover@900 --shots "$out/quest-gameover"
run --play --mode quest --tower 0 --players 2 --frames 2400 --quest-end complete@900 --shots "$out/quest-complete"
run --play --mode trials --tower 1 --level 0 --frames 2200 --shots "$out/trials"
# Tools live in the main checkout (env.sh sets TFP_ROOT).
py="${TFP_ROOT:-$root}/tools/venv/bin/python"
# King's Court Reaper intro / fight / death, if the game crate test
# `kings_court_reaper_intro_and_death` has saved them.
reaper=()
for f in "$root"/scratch/quest-reaper-transform.png "$root"/scratch/quest-reaper-intro.png "$root"/scratch/quest-reaper-fight.png \
    "$root"/scratch/quest-reaper-death.png; do
    if [ -e "$f" ]; then reaper+=("$f"); fi
done
"$py" -I "$root/scripts/dev/contact-sheet.py" "$root/scratch/quest-sheet.png" --cols 4 \
    "$out/quest" "$out/quest-gameover"/*-gameover.png "$out/quest-gameover"/*-menu.png \
    "$out/quest-complete"/*-complete*.png "$out/quest-complete"/*-menu.png "${reaper[@]}"
"$py" -I "$root/scripts/dev/contact-sheet.py" "$root/scratch/trials-sheet.png" --cols 3 "$out/trials"
