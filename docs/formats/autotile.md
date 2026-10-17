# Autotiling

The rules are specified in `docs/spec/levels.md` §8; this note records how
`tfp-assets` applies them (`autotile`, `level_tiles`, `dotnet_random`).

* A tileset gives, per neighbourhood class, a list of candidate tile ids:
  Center, Single, SingleHorizontal{Left,Center,Right},
  SingleVertical{Top,Center,Bottom}, Top, Bottom, Left, Right, the four outer
  corners, the four inside corners, and Below. Duplicates weight a choice;
  `-1` entries mean "nothing".
* For each cell the class is decided from the cell and its 8 neighbours. A
  filled cell: all 8 neighbours filled → Center; no vertical neighbour → a
  horizontal single by its left/right neighbours; no horizontal neighbour →
  a vertical single by up/down; then the edges and outer corners from the four
  orthogonal neighbours; then inside corners when both vertical neighbours are
  filled and exactly one of a lower (checked first) or upper diagonal pair is
  empty; finally Bottom if nothing is below, else Top. An empty cell with a
  filled cell above takes Below.
* Edges wrap (levels are toroidal), for both layers.
* The BG layer counts solid cells as filled *neighbours* (so walls don't show
  edges against blocks) but only BG cells get BG tiles.
* Choosing: one candidate → used directly; several → index
  `Random.Next(count)` from a `System.Random` seeded with the level's load
  seed. Each layer starts a fresh generator from the same seed. Cells are
  visited column by column (x outer, y inner) — this order decides which
  random draw each cell gets.
* After autotiling, the level's `SolidTiles` / `BGTiles` ids (≠ −1) replace
  the computed tiles. Tiles are copied opaquely (alpha included) into a
  transparent layer image, which is then alpha-blended over the scene: BG
  layer first, solids on top. Every `Orb` entity also stamps
  `details/orbHolder` into the BG layer at (x − 8, y + 6).
* Tile-id lights use the solid ids *after* overrides but the BG ids *before*
  overrides (`LevelTiles::bg_autotiled`).

## Load seed

* Versus: the sum of the UTF-16 code units of the level's path as the game
  spells it, relative to the install: `Content/Levels/Versus/00 - Sacred
  Ground/00.oel` or `DarkWorldContent/Levels/Versus/…`, with `/` separators
  on Linux/macOS (the Windows build uses `\`, so its tiles differ).
* Quest: the quest level index; Trials `X-Y.oel` (level X of tier Y):
  `X × 10 + Y`; Dark World: the level's `file` index; Workshop: the level's
  `LoadSeed`.

## System.Random

The seeded generator is Knuth's subtractive generator as implemented by
.NET/Mono (`docs/spec/engine.md` §5.3). `DotNetRandom` reproduces it bit for
bit; the unit test holds values printed by a real .NET 8 runtime for several
seeds (`Next`, `Next(max)`, `Next(min,max)`, `NextDouble`).
