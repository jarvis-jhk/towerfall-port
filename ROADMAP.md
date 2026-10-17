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
- macOS.
