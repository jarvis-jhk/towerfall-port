# Levels and towers

Behaviour of the entities and level systems: `docs/spec/levels.md`. This is
the file layout only.

## Ogmo levels (`*.oel`)

Ogmo Editor 1 XML. Root `<level width height …>`; size is in pixels and the
grid is 10-px cells: 320×240 → 32×24, 8-player 420×240 → 42×24. Other root
attributes: `Darkness`, `WrapMode`, `CanUnlockMoonstone`, `CanUnlockPurple`,
`MiasmaH` (Versus), `QuestMode`, `Timer` (Trials).

Children, in this order in every shipped file:

| Layer | Encoding | Meaning |
|---|---|---|
| `BG` | `exportMode="Bitstring"` | background wall cells |
| `BGTiles tileset=".."` | `TrimmedCSV` | hand-placed BG tile ids over the autotiled ones |
| `Solids` | `Bitstring` | solid cells (collision + solid tiles) |
| `SolidTiles tileset=".."` | `TrimmedCSV` | hand-placed solid tile ids |
| `Entities` | elements | one element per entity |

* **Bitstring**: characters `0`/`1`, rows separated by `\n`; everything else
  (`\r`, spaces) is ignored; short rows/missing rows are empty.
* **TrimmedCSV**: one text line per row — split on `\n` *only*, so empty lines
  are empty rows. The text normally starts with a line break right after the
  opening tag, i.e. **the first line is row 0 and is usually empty**. Values
  are comma separated tile ids, `-1` = none, trailing `-1`s and rows omitted;
  empty fields are skipped.
* The `tileset` attribute is the editor's choice and is **ignored**: tiles are
  drawn with the theme's `Tileset` / `BGTileset`.
* **Entities**: element name = type; attributes `id`, `x`, `y` (pixels, the
  editor's origin of that type) plus type-specific ones (`width`, `height`,
  `Lit`, `name`, `Group`, …); optional `<node x y/>` children (paths, portal
  targets).

Counts: Steam 296 files (192 Versus, 14 Quest, 42 Dark World, 48 Trials),
8-player 193 (all Versus, 420 wide).

## Tower index

* **Versus**: `Content/Levels/Versus/<NN - Name>/tower.xml` + `*.oel`.
  `tower.xml`: `<theme>`, optional `<fixedFirst/>`, `<procedural/>`
  (Cataclysm: geometry generated from `RandomBlock` entities), `<treasure
  arrows=".4" arrowShuffle="..">Arrows,Shield,…</treasure>` (each name adds one
  lottery ticket; no element = full default mask, arrow rate 0.6). Levels:
  the directory's `*.oel` sorted by file name, then — with Dark World — the
  same-named directory under `DarkWorldContent/Levels/Versus/`. Towers 12–15
  only have `tower.xml` in `Content/`.
* **Quest**: `Content/Levels/Quest/quest.xml` lists `<level name="00"><theme>`;
  level `NN.oel`, waves `NNdata.xml`.
* **Quest data** (`NNdata.xml`): `<data>` with `<normal>` and `<hardcore>`,
  each `<wave dark slow scroll>` → `<group delay coop solo>` (frames, flags)
  with `<spawns>` (Spawner names, CSV), `<enemies>` (CSV of `[a*b]Name` =
  `a` solo / `b` co-op, `[n]Name`, or `Name`), `<treasure>`,
  `<bigTreasure>`, `<reaper>`, `<jester>`, and an optional `<floors>` per
  wave. Unknown attributes appear (`max`, a typo `deay`) and are ignored.
  `<data gauntlet="true">` (tower 12) instead lists, in order, `<group
  max dark>` (enemy CSV as text), `<treasure>` and `<bigTreasure>` steps.
* **Dark World**: `Content/Levels/DarkWorld/<N - Name>/tower.xml` with
  `<theme>`, `<time><base/><add/></time>`, `<enemies><set id><spawn>
  {enemy,type,delay,difficulty,weight}` and `<normal>/<hardcore>/<legendary>`
  lists of `<level>{file, enemySet, difficulty, waves (default 3),
  delayMultiplier, treasure, boss}`. `file` indexes the tower's level list;
  the `.oel`s are in `DarkWorldContent/Levels/DarkWorld/<same dir>/`.
* **Trials**: `Content/Levels/Trials/trials.xml`: 3 `<tier>`s of `<level
  path="X-Y.oel">` with `theme`, `arrows` (default 3), `switchTimer` (200),
  `gold`, `diamond`, `dev` (seconds). A path missing from `Content` is taken
  from `DarkWorldContent/Levels/Trials/`.

## Workshop `.tower`

One XML file per tower (CRLF line ends in the examples). Root `<tower>`:
`title`, `author`, `icon{data (16×16 bitstring), tile}`, `map x y`, `players
ffa teams`, `mode`, `variants`, `treasure arrows` (text = one digit per pickup
type), `tileset`, `bgtileset`, `background`, `music`, `lanterns`,
`darkOpacity`, `cold`, `world`, and `levels` with `<level ffa teams>`
children. Each level has `LoadSeed` (the autotile seed) and the same `Solids`,
`BG`, `BGTiles` (and maybe `SolidTiles`) layers as an `.oel` without
`exportMode` attributes, plus `Entities`; size is always 320×240. A value
`Custom` for tileset/bgtileset/background/music means the data is embedded:
`customtileset`/`custombgtileset` {`data` = base64 PNG, `autotiling` = the
autotile lists}, `custombg` {`data`, `wavy`, `wind`, `fog`, `rain`, `water`,
`snow`}, `custommusic` = base64 WAV.
