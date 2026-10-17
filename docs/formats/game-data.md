# Game data

`Content/Atlas/GameData/*.xml` (no Dark World copies). Ids are matched
case-insensitively. Hex colours are `RRGGBB`.

## tilesetData.xml
`<Tileset id image>` × 34 (a solid and a `…BG` set per theme). `image` is a
main-atlas subtexture holding 10×10 tiles row-major. Children are the
autotile candidate lists, see [autotile.md](autotile.md), plus optional
`<Lights color radius>ids</Lights>` (tiles that emit light; radius default 12).
Formatting is loose (spaces in lists, several elements on one line, a stray
character between elements) — read children by name, not by position.

## themeData.xml
`<Theme id>` × 19 with children `Name`, `Icon` (menu atlas
`towerIcons/<Icon>`), `TowerType` (Normal/Secret/Ascension/Dark/Cataclysm/
Random), `MapPosition x y`, `Music` (cue name), `DarknessColor` (default
black), `DarknessOpacity`, `Wind` (int, default 0), `Lanterns`
(CathedralTorch/DarkHallsTorch/CavesTorch/DayMountainTorch/MoonstoneCrystal),
`World` (Normal/Dark), `Raining`, `Cold`, `Background` (bgData id),
`Tileset`, `BGTileset`, `DrillParticleColor` (default red),
`CrackedBlockColor` (default 4EB1E9), `PlayerInvisibility` (per archer colour
Green…Red an int; invisible opacity = 0.2 + 0.1 × value).

## bgData.xml
`<BG id>` with `<Background bgColor>` and optional `<Foreground>`, each a list
of layer elements whose name is the layer type (`Backdrop`, `ScrollLayer`,
`FadeLayer`, `WavyLayer`, `AnimatedLayer`, `NodeLayer` with `<Node>`s,
`LightningFlash`, theme-specific ones like `SacredGroundMoonLayer`, and
`<Overtime>` groups). Attributes vary by type; the text is a bg-atlas or
bg-sprite name. Kept generic (`BgLayer { kind, attrs, text, children }`);
behaviour belongs to the rendering spec.

## archerData.xml
Root `<Archers>`: `BlueTeam`, `RedTeam`, `Enemies` (ColorA/ColorB/Aimer/
VictoryMusic) and, in order, `<Archer>` × 9 each optionally followed by an
`<AltArcher>` and/or `<SecretArcher>`, which inherit every field of the
preceding archer they don't set. Fields: `Name0`, `Name1`, `ColorA`,
`ColorB`, `LightbarColor`, `Aimer` (main atlas), `Hair`, `SFX` (index into
the character sound sets), `Corpse` (corpse sprite id), `StartNoHat`,
`VictoryMusic` (default `Team`), `PurpleParticles`, `SleepHeadFrame`
(default −1), `Gender` (default Female), `Sprites{Body, HeadNormal,
HeadNoHat, HeadCrown, HeadBack, Bow}` (sprite ids), `Hat{Material, Normal,
Blue, Red}` (main atlas), `Portraits{NotJoined, Joined, Win, Lose}` (menu
atlas), `Statue{Image, Glow}`, `Gems{Menu, Gameplay}`,
`Breathing{Interval, Offset x y, DuckingOffset x y}` (interval −1 = none).

## versusTips.xml / questTips.xml
`<tips><tip>` with `text` (`|` = line break); versus tips add `matches`,
`image`, `dir`.
