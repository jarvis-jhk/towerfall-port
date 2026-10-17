# Data formats of the original game

Short notes on every file type `tfp-assets` (`crates/assets`) reads, in our own
words. Behaviour (what the game *does* with the data) lives in `docs/spec/`;
these files only describe layout and parsing rules.

| File | Covers | Reader |
|---|---|---|
| [install.md](install.md) | finding the Steam copy and the 8-player build | `install` |
| [atlas.md](atlas.md) | `Atlas/*.xml` + `*.png` | `atlas` |
| [sprite-data.md](sprite-data.md) | `Atlas/SpriteData/*.xml` | `sprite_data` |
| [game-data.md](game-data.md) | `Atlas/GameData/*.xml` | `game_data` |
| [levels.md](levels.md) | `.oel`, `tower.xml`, quest/trials/dark world XML, Workshop `.tower` | `level`, `towers`, `quest_data`, `workshop` |
| [autotile.md](autotile.md) | tilesets → tile ids, the level seed, `System.Random` | `autotile`, `level_tiles`, `dotnet_random` |
| [audio.md](audio.md) | `SFX/*.wav`, XACT `.xwb`/`.xsb`/`.xgs`, MS-ADPCM, effect catalogue, music streaming | `audio` |
| [xnb.md](xnb.md) | `Misc/*.xnb` sprite fonts and effects | `xnb` |
| [look.md](look.md) | presentation data of a level (tiles, background, decorations, lighting) | `look` |
| [procedural.md](procedural.md) | Cataclysm's generated geometry and its verification | `procedural` |
| [savedata.md](savedata.md) | the original's `tf_saveData` (read-only import source) | `savedata` |

Tools: `cargo run -p tfp-assets --bin assets-info [-- --decode-music]` prints an
inventory of both installs; `--bin tower-sheet [-- --all]` renders contact
sheets of every Versus tower into `scratch/`.
