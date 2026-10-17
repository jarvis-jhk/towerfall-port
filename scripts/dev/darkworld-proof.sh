#!/bin/bash
# Headless proof of the Dark World presentation (M10): bots play the
# Amaranth's first level (title card, level card, waves, GO! portal, level
# 2), each of the four bosses is fought on its level with the proof assist
# (invulnerable bots, boss damage every N ticks so every phase shows up),
# and the game over and complete screens are forced on short runs.
# Contact sheet: scratch/darkworld-sheet.png (all captures in
# scratch/darkworld-proof/).
#
# Usage: scripts/dev/darkworld-proof.sh [path/to/towerfall-port]
set -euo pipefail
shopt -s nullglob
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin="${1:-${CARGO_TARGET_DIR:-$root/target}/debug/towerfall-port}"
out="$root/scratch/darkworld-proof"
rm -rf "$out"
mkdir -p "$out"
run() { "$bin" --headless --play --mode darkworld "$@" | grep -E "capture|music:" || true; }
# A normal level with two bots, through the exit portal into level 2.
run --tower 0 --players 2 --frames 2400 --dw-assist 0 --shots "$out/level"
# Darkfang's first level: the trigger-arrow slide before the title card.
run --tower 2 --players 1 --frames 700 --dw-assist 0 --shots "$out/tips"
# The bosses mid-fight (one archer; damage every N ticks while hurtable).
run --tower 0 --players 1 --dw-level 99 --frames 3000 --dw-assist 40 --shots "$out/amaranth"
run --tower 1 --players 1 --dw-level 99 --frames 4200 --dw-assist 30 --shots "$out/dreadwood"
run --tower 2 --players 1 --dw-level 99 --frames 5200 --dw-assist 25 --shots "$out/darkfang"
run --tower 3 --players 1 --dw-level 99 --frames 12000 --dw-assist 40 --shots "$out/cataclysm"
# Game over on level 2 (a continue is offered) and a Legendary clear
# (no continues: the eye awards).
run --tower 1 --players 1 --dw-level 1 --frames 1300 --quest-end gameover@500 --shots "$out/gameover"
run --tower 0 --players 2 --difficulty 2 --frames 2600 --quest-end complete@200 --shots "$out/complete"
py="${TFP_ROOT:-$root}/tools/venv/bin/python"
"$py" -I "$root/scripts/dev/contact-sheet.py" "$root/scratch/darkworld-sheet.png" --cols 4 \
    "$out/level"/*-title.png "$out/level"/*-L1-card.png "$out/level"/*-L1-play.png \
    "$out/level"/*-L1-exit.png "$out/level"/*-L1-zoom.png \
    "$out/tips"/*-tips.png \
    "$out/amaranth"/*-boss-title.png "$out/amaranth"/*-state1.png "$out/amaranth"/*-state2.png "$out/amaranth"/*-death.png \
    "$out/dreadwood"/*-state1.png "$out/dreadwood"/*-state2.png "$out/dreadwood"/*-state3.png "$out/dreadwood"/*-death.png \
    "$out/darkfang"/*-state1.png "$out/darkfang"/*-state2.png "$out/darkfang"/*-state3.png "$out/darkfang"/*-death.png \
    "$out/cataclysm"/*-state0.png "$out/cataclysm"/*-state1.png "$out/cataclysm"/*-state2.png "$out/cataclysm"/*-state3.png \
    "$out/cataclysm"/*-state4.png "$out/cataclysm"/*-heart.png \
    "$out/gameover"/*-gameover.png "$out/gameover"/*-menu.png \
    "$out/complete"/*-complete.png "$out/complete"/*-complete-cards.png
