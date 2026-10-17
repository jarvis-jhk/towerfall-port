# Architecture

## Goals

1. Play TowerFall Ascension (with Dark World) natively on Linux and Windows
   (macOS later) from the player's own copy: the original's look, feel,
   towers, arrows, variants and modes (Versus, Quest, Dark World, Trials).
2. Up to 12 archers in any mix of local players on one screen (controllers,
   keyboard) and online players. Free-for-all and teams.
3. Online play over [Warpzone](https://github.com/jarvis-jhk/warpzone) only
   (rooms, codes, NAT traversal, relay), with rollback netcode.
4. Public open source: the repo holds no game data and no decompiled code.

## The original, in short

- C# on XNA/FNA, Matt Thorson's **Monocle** engine: `Scene` → `Entity` →
  `Component`, colliders (`Hitbox`, `Circle`, `Grid`, `ColliderList`), `Actor`
  with integer positions plus a float remainder (`MoveH`/`MoveV` step one pixel
  at a time against solids), `Alarm`/`Tween`/`Coroutine`/`StateMachine`.
- Fixed logic rate 60 Hz, `Engine.TimeMult` scales per-frame quantities
  (slow-motion on final kills). Native resolution **320×240**
  (Ascension, 4 players); the 8-player build uses **420×240** towers.
- Data on disk, all plain: `Content/Atlas/*.png` + `*.xml` (packed atlases),
  `Atlas/SpriteData/*.xml` (animations), `Atlas/GameData/*.xml` (archers,
  themes, tilesets, backgrounds, tips), `Levels/**/*.oel` (Ogmo Editor XML,
  bitstring solid grids, entity lists), `Workshop/*.tower`, `SFX/*.wav`,
  `Music/Win/*.xwb/xsb/xgs` (XACT), `Misc/*.xnb` (fonts and compiled effects,
  which we reimplement as WGSL).

## Data flow

```
game install(s) (read-only)
  └─ tfp-assets ── parses atlases, sprite data, game data, levels, audio
       └─ (cache/: decoded music, packed level index — regenerable)
            └─ towerfall-port loads plain Rust data; tfp-sim gets levels/rules
```

No import step is strictly needed except for music (XACT wave bank → PCM/Ogg
cache). Both installs are found automatically (Steam library folders, the
8-player build's usual paths) or via `TF_GAME_DIR` / `TF8_GAME_DIR`.

## Simulation (`tfp-sim`)

- Fixed step 1/60 s. Pure function of `(state, inputs of every player) → state`.
  `State: Clone`; cloning is how rollback saves frames. No I/O, seeded PRNG in
  the state, no wall-clock, no `HashMap` iteration, `libm` for transcendental
  math, so Linux and Windows builds agree bit for bit.
- Monocle semantics reimplemented as plain data: entity arena with stable ids,
  depth-ordered update lists, pixel-stepping actor movement against the solid
  grid and solid entities, jump-thrus, screen wrap.
- Emits *events* (sounds, particles, screen shake, flashes) that the
  presentation layer consumes; events from rolled-back frames are deduplicated.
- Match flow (rounds, points, crowns, round end, match end, variants, teams)
  lives in the sim, identical on every peer. Menus before a match are outside
  the sim; their result is a `MatchSettings` value shared by all peers.

## Players and input

- A player slot (up to 12) is bound to a device on some machine: a controller
  (SDL2 GameController, any number) or a keyboard layout.
- Per player per frame: stick/dpad direction (quantised), jump, shoot, dodge,
  (alt-shoot for Dark World triggers), start. Small and fixed-size.

## Networking (`tfp-net`)

- GGRS P2P session; each peer is a *machine* carrying 1–N local slots. GGRS's
  socket runs on Warpzone peer channels (`warpzone-core`). Adapted from our
  rounds-port netcode.
- Desync detection with a state checksum every N frames.
- Dev: our own `warpzone-server` built from `ext/warpzone` into `scratch/net/`.

## Presentation (`tfp-render`, `towerfall-port`)

- SDL2 for window, input, audio (system `libSDL2-2.0.so.0` on Linux, shipped
  `SDL2.dll` on Windows). wgpu renders into a 320×240 / 420×240 (or wider for
  12 players) pixel target, then scales to the window with integer or
  aspect-correct scaling. Lighting layer and screen effects reimplemented in
  WGSL. Offscreen (`--headless`) for screenshots and tests.
