# towerfall-port

An open-source, clean-room reimplementation of **TowerFall Ascension** (and
its Dark World expansion) for Linux and Windows. It plays the original's
Versus mode with up to **12 archers**: Last Man Standing, Headhunters and Team
Deathmatch with 2–6 teams, every tower, arrow, treasure and variant, instant
replays and the match results.

**You need your own copy of TowerFall Ascension** (Steam; the Windows, Linux
or Mac version all work). towerfall-port contains no game data: every sprite,
sound, song and level is read from your installation at start, and nothing
there is changed. The Dark World expansion is optional (its towers, arrows and
archers appear when it is installed); so is the official **8-player build**,
whose wide 420-pixel arenas are used for matches with more than four archers.

towerfall-port is not made by, affiliated with or endorsed by Maddy Makes
Games. TowerFall is © Matt Makes Games Inc.

## Starting

* **Linux:** unpack the archive anywhere and run `./towerfall-port`. SDL2 is
  included (`lib/`); a newer system SDL2 is used instead when
  `LD_LIBRARY_PATH` points at it.
* **Windows:** unpack the zip and run `towerfall-port.exe` (it is not signed,
  so Windows may ask once whether to run it).

The game finds a Steam installation by itself (all Steam library folders,
Flatpak Steam on Linux, the registry on Windows). If it does not, it says so
on screen; then do one of:

* drag your TowerFall folder (the one that contains `Content/`) onto the
  window; it is remembered,
* start it with `--game-dir PATH`,
* or set `TF_GAME_DIR` (and `TF8_GAME_DIR` for the 8-player build).

On the first start your options and keyboard setup are imported from the
original game's save (read only). Settings are kept in
`~/.local/share/towerfall-port/options.cfg` (Linux) or
`%APPDATA%\towerfall-port\options.cfg` (Windows).

## Controls

Any number of controllers plus up to four keyboard layouts. Press a button on
a controller (or Confirm on a keyboard) in the archer select to join.

| Action | Controller | Keyboard (default) |
|---|---|---|
| Move / aim | D-pad or left stick | Arrow keys |
| Jump / menu confirm | A | C |
| Shoot / menu back | X (B also backs out of menus) | X |
| Dodge | LB, RB, LT or RT | Left Shift |
| Alt shoot (Trigger arrows, ...) | B | Z |
| Arrow toggle | Y | S |
| Pause / start | Start | Enter |
| Menu "alt" functions | RB / RT | Left Shift |
| Fullscreen | | F11 or Alt+Enter |

Keyboard keys can be changed in *Options → Control options → Configure
keyboard* (a "shared keyboard" preset puts two players on one keyboard);
controller buttons in *Configure controller* (per controller model). During
an instant replay, hold Alt (RB/RT) for slow motion; Start skips it. A
controller that is unplugged during a match pauses the game until it (or
another one) is plugged in.

## Online

Online play goes through [Warpzone](https://github.com/jarvis-jhk/warpzone)
(rooms, join codes, invite links) with rollback netcode, and any mix of local
and online archers up to 12. It is still being finished; see the project page
for its state. Everyone in a match needs their own copy of TowerFall.

## Command line

`--fullscreen`, `--scale fit|integer|stretch`, `--no-vsync`, `--game-dir PATH`,
`--import-save` (import the original's options again), `--no-music`;
`--help` lists everything.

## Licence

The code is MIT or Apache-2.0, at your option (`LICENSE-MIT`,
`LICENSE-APACHE`). Third-party components: `THIRD-PARTY/` (SDL2 under the zlib
licence, the Rust crates and their licences). Source code:
https://github.com/jarvis-jhk/towerfall-port
