# Installs

Two read-only installs are used:

* **Steam copy** (app 251470): `steamapps/common/TowerFall` inside any Steam
  library. Libraries are listed in `<steam root>/steamapps/libraryfolders.vdf`
  (Valve KeyValues text: quoted keys and values, nested `{}` blocks, `//`
  comments, backslash escapes). Current files have
  `"libraryfolders" { "0" { "path" "…" "apps" {…} } … }`; old ones list
  `"1" "D:\\Games"` directly. Steam roots tried: `~/.steam/steam`,
  `~/.local/share/Steam`, the Flatpak one, macOS `~/Library/Application
  Support/Steam`, `%ProgramFiles(x86)%\Steam`.
* **8-player build**: no installer convention; we look at `TF8_GAME_DIR` and a
  few home-directory guesses and accept a directory whose first Versus level is
  wider than 320 px (its levels are 420×240).

`TF_GAME_DIR` / `TF8_GAME_DIR` override the search.

An install directory is recognised by `Content/Atlas/atlas.xml` and
`Content/Levels/`. Content sets:

| Set | Present when | Notes |
|---|---|---|
| Ascension | always (`Content/`) | |
| Dark World | `DarkWorldContent/Atlas/atlas.xml` exists | its `atlas`, `bossAtlas` replace the base ones; extra levels are appended to towers of the same directory name |
| Workshop | `Workshop/` contains `*.tower` (any depth) | Steam copy ships 14 example towers |
| wide towers | level width 420 | 8-player build; has only Versus levels |

The Steam copy also has `tf_savedata` (never touched). The game itself only
enables Dark World if Steam reports the DLC owned; we only check for the files.
