# Procedural levels (`tfp_assets::procedural`)

Towers with `<procedural/>` (Versus Cataclysm) regenerate every round from one
seed: `procedural::generate(&level, seed, teams)` returns the new `Solids`, `BG`,
`BGTiles` override grid and the entity list (treasure re-placed, generated
exploding orbs, mud, jump pads, crumble walls, a spike ball or a moving
platform, and wall lanterns appended). `Level::with_generated` applies it.
The algorithm is specified in `docs/spec/levels.md` §13; this implementation
follows it stage by stage with the original's draw order.

Width handling: one implementation for 32 and 42 columns. The 8-player build
kept some 32-wide constants (thin test of bottom-wrap blocks, the crumble-wall
centre test at column 16, the 32-column bound of the moving-platform solid
test, the column-15 skips of two detail stages, the treasure centre blocker
columns 15/16); `Dims` reproduces them.

## Verification against the reference

An oracle harness kept with the private specs (`docs/spec/oracle/procgen/`)
builds the reference generator from `ref/decomp/{steam,8p}` in
`scratch/procgen-oracle/` (never committed) and dumps solids, BG, overrides and the full entity list for
every Cataclysm level × 7 seeds × FFA/teams. The test
`procedural::tests::matches_reference_dumps` compares our output with those
dumps when present: all 406 cases (196 Steam, 210 8-player) are identical.
Set `TFP_DUMP_DIFF=1` to write our output next to a differing dump.
