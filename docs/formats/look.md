# Level look (`tfp_assets::look`)

`LevelLook::build(&LookAssets, &Level, &ThemeDef, load_seed, &LookOptions)`
turns a level into renderer-ready data (no GPU types). `LookAssets::load(inst)`
loads what it needs once per install (game data, main + bg atlases, main + bg
sprite data). Behaviour of every element: `docs/spec/rendering.md` §5–7.

| Field | Content |
|---|---|
| `bg_tiles`, `solid_tiles` | `TileLayer`: tileset id, its main-atlas subtexture and rect, the final id grid, every non-empty tile as (pixel x, y, id, atlas src rect), plus `stamps` (orb holders baked into the BG layer). Draw each layer into its own transparent W×240 image (tiles copied opaquely, stamps alpha-blended) and composite it as one picture: BG layer at depth 10000 of layer −3, solids at depth −150 of layer 0. |
| `background` | `BgElement`s drawn on the screen first, never darkened. |
| `foreground` | `BgElement`s drawn last inside the darkened foreground image. |
| `decorations` | Background entities from the level: torches/crystals/mushrooms (layer −3, with lights), big mushrooms, skeletons, cobwebs, snow, rain drops, water drops, chains, lanterns, pirate banners, ship windows, Ogmo — kind, position, layer, depth, drawable `parts` (sprite id + animation + start index, or atlas image + origin + flip + alpha, or the special mushroom-strip / banner forms) and an optional light. |
| `lighting` | Light-canvas clear colour (theme darkness colour inverted), base `alpha` (`DarknessOpacity`), and the static lights: tile-id lights and the decorations' lights (colour as stored: black erases darkness, coloured = inverted colour). Dynamic lights (players, arrows, enemies) belong to the simulation. |
| `entities` / `solids` | The level's entities and solid grid after procedural generation (Cataclysm), for the gameplay side. |
| `missing` | Background art the data names but the install lacks (the element is left out). |

`BgElement` variants carry their resolved parameters (defaults applied): `Backdrop`,
`Scroll`, `Wavy`, `Animated`, `Fade`, `FadeMove`, `Node`, `LightningFlash`, `Ocean`,
`SacredGroundMoon`/`FlightMoon` (the 8-player build only has the broken moons),
`AscensionLightning`, `GhostShip`, `Vortex`. Images are `ImageRef { atlas, name,
src }`; sprites are bg-sprite-data ids.

**Seeding.** As in the original, a `System.Random(load_seed)` builds the
background, then the foreground, then the decorations. Elements that randomise
at construction record what they drew (`init_*`: start frame, sine phase,
position, rate, first delay); `<Random chance>` groups are resolved from the
same stream, so a level always gets the original's layer selection. Decoration
picks (skeleton frame, cobweb corner frame, chain links, snow cell, banner
variant) continue the stream but can differ from the original wherever gameplay
entities between them consume draws we don't model; they are stable per seed.
`<Overtime>` groups are read when `LookOptions::overtime` is set;
`no_flashes` drops `LightningFlash`.

**Procedural towers.** `LookOptions::procedural = Some((seed, teams))` runs
the generator (`procedural.md`) before tiling.

The `tower-sheet` binary is a CPU reference renderer of the first frame of a
look (background, layer −3, layer 0 by depth, foreground, then the subtractive
darkness composite with a sine-0 light canvas).
