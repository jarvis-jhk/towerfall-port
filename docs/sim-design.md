# Simulation design (`tfp-sim`)

The sim is the whole game logic inside a match: archers, arrows, treasure,
tower entities, enemies, match flow. Everything else (rendering, audio,
menus, networking) reads it or feeds it inputs. Behaviour comes from
`docs/spec/*.md`; this file is about *shape*, not rules.

## Shape

```rust
pub struct Content { /* Arc-shared, immutable: towers, levels, sprite data, archer data, tuning */ }
pub struct World   { /* Clone; everything that changes */ }
#[repr(C)] #[derive(Pod)]
pub struct PlayerInput { pub aim_x: i8, pub aim_y: i8, pub buttons: u8, pub _pad: u8 }

impl World {
    pub fn new(content: &Content, setup: &MatchSetup) -> World;
    pub fn step(&mut self, content: &Content, inputs: &[PlayerInput]);
    pub fn events(&self) -> &[Event];   // produced by the last step
    pub fn checksum(&self) -> u64;      // for desync detection
}
```

- `MatchSetup`: player slots (≤ 12: archer, alt skin, colour, team, which
  machine/device is irrelevant to the sim), mode (Last Man Standing,
  Headhunters, Team Deathmatch; later Quest/Dark World/Trials), match length,
  variants, treasure mask, the tower (a match plays one tower; its levels cycle in a seeded shuffle), seed. Identical on every peer (the
  lobby distributes it).
- `Content` is built from `tfp-assets` once: levels as solid/BG bit grids plus
  typed entity placements, tilesets for collision-relevant data only,
  sprite animation tables (gameplay reads animation state in places, so frame
  counts and delays must be in the sim), archer gameplay data. Not cloned.
- `World` holds only what changes. It must be cheap to clone (target
  < 50 µs for 12 archers and a busy level): rollback clones it every frame.

## Time

- The original runs Monocle at 1/60 s with `Engine.TimeMult` scaling
  per-frame quantities (1.0 normally; lower during the final-kill slow motion
  and some variants). We step at exactly 60 Hz and keep `time_mult: f32` as
  world state, applied wherever the original applies it, so slow motion is
  part of the deterministic sim.
- All timers are frame counters or `f32` counters decremented by
  `time_mult`, matching the original per timer (the spec says which).

## Determinism

- f32 math like the original (it is f32 XNA `Vector2` math). Transcendentals
  only through `libm`; no `f32::sin` etc., no fast-math, no SIMD reductions.
- One seeded PRNG in `World` with separate streams where the original uses
  separate generators or where presentation-only randomness must not perturb
  gameplay (cosmetic randomness that never feeds back stays in the
  presentation layer, seeded from frame + entity id).
- Entities live in insertion-ordered `Vec`s with generational ids allocated
  deterministically; no `HashMap`/`HashSet` iteration; no wall clock, no
  threads, no I/O in `step`.
- `checksum()` hashes gameplay-relevant state in a defined order.

## Entities: Monocle semantics as data

- Monocle updates entities in list order and draws by depth; additions and
  removals are applied between frames (`UpdateLists`). We keep the same rule:
  `spawn()`/`remove()` queue, applied at the start of the next step, so update
  order matches the original.
- One arena of `Entity { id, active, collidable, pos: Vec2, remainder: Vec2,
  depth, tags, collider, kind: EntityKind }`, where `EntityKind` is an enum
  with one variant per entity type (`Player(PlayerState)`, `Arrow(ArrowState)`,
  `Chest(..)`, `MovingPlatform(..)`, …). Per-type `update` functions are plain
  `fn(&mut World, &Content, EntityId)`; cross-entity access goes through the
  world by id (no references held across calls).
- Actor movement (`move_h`/`move_v` stepping one pixel against the solid grid,
  solid entities and jump-thrus, with `on_collide` callbacks as enum-returned
  results) and screen wrap are shared helpers in `actor.rs`, implemented from
  `docs/spec/engine.md`.
- Collision queries are linear scans over tag lists (a level has at most a few
  hundred entities); no spatial hash until profiling says so.
- Sprite animation state that gameplay reads (current animation id, frame,
  finished) lives in the entity state and advances in `step`. Purely visual
  things (particles, hair physics, screen shake) are produced as events and
  simulated by the presentation layer.

## Input

- Per player per frame: aim/move direction as two i8 axes (dpad and stick
  both quantised; the spec's deadzone and 8-way rules are applied in the sim so
  local and remote behave alike) and held buttons: jump, shoot, dodge, alt
  shoot, start. "Pressed this frame" is derived in the sim from the previous
  frame's buttons (kept in `World`), so an input is a pure "held" snapshot and
  GGRS prediction (repeat last input) behaves.
- Pause and menus are not sim inputs in online play (pausing is local-only or
  a lobby vote); `start` is used for in-match prompts only.

## Events (sim → presentation)

Sound cues (name, position, volume/pitch hints), particle bursts (type,
position, direction), screen shake/flash, kill feed, score changes, phase
changes. Because rollback re-runs frames, the presentation keys events by
`(frame, sequence)` and fires each once; looped sounds are cancelled when the
frame that started them is rolled back and not re-confirmed.

## Match flow inside the sim

Round phases (all timers in frames): `Spawning → Playing → RoundEnding
(slow-mo) → Scoreboard → NextLevel …` until `MatchOver`. The next level comes from the tower's level pool, shuffled with the seeded
PRNG (the original uses unseeded randomness here; see levels.md §9.1).
The end-of-round replay is presentation-side: the client keeps a ring buffer
of the last N rendered snapshots (cheap, because `World` clones are cheap) and
plays them back after the round ends.

## Players beyond 4

- Slots are a fixed `[Option<PlayerSlot>; 12]`; spawn assignment, colours,
  teams and HUD follow `docs/spec/versus.md` § "Scaling to 12 players".
- Wide (420×240) towers from the 8-player build are used when present and
  more than 4 players are in the match; level width is data, never a constant.
