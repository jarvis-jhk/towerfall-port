# Roadmap

towerfall-port is feature-complete as a pre-release (v0.1.0): Versus for 2-12
archers, Quest, Trials, Dark World, options, remapping, records and online
play over Warpzone with rollback. What is done in detail, and how it was
checked, is in [docs/STATUS.md](docs/STATUS.md). This file is the order of
the work still ahead.

## Current priorities

1. **CI** on GitHub: formatting, clippy, the test suite (tests that need a
   TowerFall install skip without one), the Linux tarball with a headless
   smoke run and the cross-built Windows zip on every push; releases built by
   a `v*` tag on an older glibc than v0.1.0's (2.39) so more Linux systems
   and the Steam Deck run them. Prereleases until the desktop check below
   has passed.
2. **Warpzone end to end** against the live server: install, data detection,
   host/join, co-op rooms. Offer the official 8-player build to the launcher
   as optional data if the game can use it from there.
3. **Steam Deck**: fullscreen and pad-friendly defaults when started in
   Gaming Mode.
4. **Real hardware**: the desktop check (`docs/desktop-check.md`) with real
   controllers, windowed vsync pacing, fullscreen and audio; a real Windows
   machine.
5. **Crash reports**: panics reported to an issue automatically (opt-in, no
   personal data), so failures on players' machines are not lost.

## macOS support

After the current priorities, without changing their order.

- **Build**: a macOS binary for Apple Silicon and Intel (one universal
  binary via `lipo`, or two archives), built on a GitHub `macos` runner in
  CI with a headless smoke run, and attached to every release.
- **Player docs**: install and start notes for macOS, including where the
  game data is found. Unsigned is fine; document the Gatekeeper workaround
  (right-click > Open, or `xattr -dr com.apple.quarantine`).
- **Known risks**:
  - SDL2 must be bundled (as in the Linux tarball) or linked statically;
    a `.app` bundle needs `@rpath`/`@executable_path` set up for it.
  - Rendering: wgpu's Metal backend is already enabled but untested; check
    Retina/HiDPI scaling of the pixel-perfect presentation, fullscreen
    Spaces and vsync pacing (ProMotion displays).
  - Controllers: SDL's macOS HIDAPI/GameController paths behave differently
    (Xbox/PlayStation/Switch pads, hot-plug, mapping database), and 12 pads
    at once is untested on macOS.
  - File paths: the data dir falls back to `~/.local/share` today; macOS
    wants `~/Library/Application Support/towerfall-port`. Game detection
    must find the Mac Steam install (`~/Library/Application Support/Steam/
    steamapps/common/TowerFall`, content inside `TowerFall.app/Contents/
    Resources`) and the original's macOS save location.
  - Warpzone: the launcher/client and its game descriptor need a macOS
    build target; whether the Warpzone client runs on macOS at all is open.
  - Audio and keyboard: Cmd-key shortcuts (Cmd+Q, Cmd+F) and the audio
    device switching on macOS.

## New game modes

Ten modes of our own that the original does not have (Jan, 2026-10-19).
They are new designs, not reimplementations, so each gets a design note in
`docs/modes/` (public, our own text) instead of a spec. After the current
priorities and macOS, without changing their order. Every mode must:

- run entirely in `tfp-sim`, deterministic and rollback-safe (seeded PRNG,
  no new state outside `World`), so it works online and mixed local+online;
- work for 2-12 archers on the normal and wide towers, with bots, and with
  the existing match settings (length, variants, teams where it makes sense);
- show everything on the one shared screen (no secret per-player
  information);
- get a menu entry, a short in-game explanation card, results/awards and a
  headless proof run (`scripts/dev/`), like the existing modes.

The modes, roughly in the order they should be built (the first ones reuse
the most existing code):

1. **Capture the Flag** (teams): each team has a flag on a pedestal in its
   half of the tower. Grab the enemy flag and bring it to your own pedestal
   while yours is at home. A carrier who dies drops the flag; touching your
   own dropped flag returns it. Carriers can shoot but not dodge-catch.
   Needs team bases on the versus towers (a small per-tower table).
2. **Siege** (co-op wave defense): defend a crystal in the middle of the
   tower against waves of Quest enemies that go for the crystal, not the
   archers. Between waves the team spends gems from kills on barricades,
   arrow racks or a turret at fixed build spots. Lose when the crystal
   breaks; score is waves survived. Different from Quest because the
   objective, not the archers, is under attack.
3. **King of the Hill**: a glowing zone sits on one platform and moves to
   another every 20 s. Standing in it alone scores a point per second;
   contested zones score nothing. First to the target score wins. Archers
   respawn after a short delay.
4. **Hot Potato**: one archer carries a lit bomb with a hidden-length fuse
   (5-15 s, from the seeded PRNG; a ticking sound speeds up). Stomping or
   hitting another archer with an arrow passes it on. When it goes off, the
   carrier (and anyone close) is out for the round. Last archer standing
   wins the round.
5. **Juggernaut**: one archer is the Juggernaut: shield that regrows, wings,
   twice the arrows, bigger hitbox. Only the Juggernaut scores (per kill);
   whoever kills the Juggernaut becomes the next one. First to the target
   score wins.
6. **Infection**: one random archer starts infected (a dark tint and
   unlimited arrows, no shield). Anyone an infected archer kills respawns
   infected. Survivors win the round if one is still alive when the timer
   runs out; the infected win if they get everyone. Points to the survivors,
   and to the first infected for every conversion.
7. **Gem Rush**: chests and kills drop gems; each archer carries what they
   pick up and loses all of it on death, spilled for others to grab. Gems
   only count once banked at an altar that moves between rounds. Most banked
   gems at the end of the timer wins.
8. **Arms Race**: everyone starts with normal arrows. Each kill moves you on
   to the next arrow type (bomb, laser, drill, bramble, trigger, prism, ...,
   the last one a single plain arrow). The first archer to score a kill with
   the last step wins the match. Being stomped sends you one step back.
9. **Orb Ball**: a heavy ball (the Dark World orb physics) and a goal at each
   side of the tower. Arrows, stomps and explosions knock the ball; no
   archer can be killed, hits only stun. First team to the target goals
   wins. Needs goal tiles on a few versus towers or a set of new arenas.
10. **Target Race**: Trials for several archers at once. Every archer has
    their own coloured targets spread over the same tower; your arrows
    break only yours, and hitting another archer only knocks them back.
    First to break all their targets wins the round. Uses the Trials
    targets and timing.

Open for each mode: the exact numbers (scores, timers, wave sizes) are set
by bot playtests and then real ones, as for 9-12 player Versus.

## Big arenas

Optional bigger levels where the whole game is shown smaller (Jan,
2026-10-19): more room for 9-12 archers, but selectable in every mode and
at every player count. After the current priorities, macOS and the new
modes, without changing their order.

- **Setting**: an arena size option (Normal / Wide / Big, maybe Huge) in
  the versus settings and the co-op menus; Normal keeps 320×240, Wide the
  existing 420×240, Big about 1.5× the area in each direction (e.g. 480×360
  or 640×360), Huge about 2×. The default picks by player count (Normal up
  to 4, Wide 5-8 as today, Big suggested at 9-12) but any size can be chosen
  for any count. Part of the match settings, so it is synced online and in
  replays.
- **Scaling down**: the sim keeps its pixel units and physics untouched;
  the arena is simply larger in world pixels and the renderer fits the
  bigger canvas into the window, so everything (archers, arrows, tiles,
  HUD in the arena) looks smaller. Pixel-perfect presentation keeps
  working (integer scale where the window allows, otherwise the existing
  fractional fit). Screen-space HUD (scores, P-tags, menus) stays at its
  current size so it remains readable.
- **Levels**: the original has no levels at these sizes, so they come from
  - generated variants of the existing towers: tile the 320×240 level
    layout (2×1, 2×2, mirrored or with a seam section) into one bigger
    room, with spawns, chest spots and treasure tables extended
    accordingly, using the player's own level files at run time;
  - hand-made arenas of our own (our own Ogmo-compatible format, no game
    data in git) for a few themes, later.
  Screen wrapping, miasma, moving platforms, lava and the Dark World orb
  must work at any size.
- **Modes**: Versus (all modes and the 89 variants), the new modes, Quest
  and Dark World (enemy spawn portals and wave sizes scaled with the area)
  and Trials (targets spread over the bigger room).
- **Gameplay tuning**: more archers on a bigger map changes pacing: arrow
  pickup density, chest counts and the 9-12 miasma timing (see Open
  questions) are scaled with the area; numbers set by bot playtests first.
- **Readability**: at 12 archers on a huge arena on a TV, archers must stay
  recognisable: optional thicker player outlines/indicators and a
  slightly larger arrow-count display above each archer.
- **Proof**: headless screenshots and a bot match per size and mode in
  `scripts/dev/`, frame time at 12 bots on the largest size, rollback
  determinism tests with the size in the match settings.

## Open questions

- **9-12 player Last Man Standing**: the miasma starts at 25 s as in the
  original; with many archers most rounds end in a miasma draw (68 % of
  12-player bot rounds, 17-34 % at 2-8). Keep the timing, or scale it with
  the number of archers? Kept as is until playtests say otherwise.
- 9-12 player tuning in general (arrow cap, chest chances, duplicate-archer
  tints) needs real playtests.

## Later

- Ranked Warpzone queue (1v1 and 2v2 Versus).
- Workshop towers through Warpzone mods, if that needs no extra code.
- Quest/Trials save data and unlock reveal cutscenes; co-op in the 8/12
  layouts.
