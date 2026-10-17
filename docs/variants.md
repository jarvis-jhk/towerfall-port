# Versus variants in the sim (audit)

All 89 variants of `versus.md` §13.3, with where the sim applies them. **Sim** = the
full gameplay effect is simulated; **View** = the effect is presentation only and the
sim exposes what the game crate needs to draw it (field of `tfp_sim::View`); the game
crate still has to draw it. Menus, unlock gating and the variant icons are game-crate
work. Last audit: M5 completion (with M9 merged).

## Rules

| Variant | Kind | Where |
|---|---|---|
| No Autobalance | Sim | `versus/spawn.rs` (no crown arrow penalty, no catch-up shield) |
| Weak Autobalance | Sim | `versus/spawn.rs::spawn_shield` (catch-up shield off) |
| No Friendly Fire | Sim | `hurt.rs::can_hurt_player` (arrows, brambles, bramble corpses, player ghosts) |
| No Time Limit | Sim | `versus/mod.rs` (the miasma never appears) |
| Sudden Death | Sim | `versus/mod.rs` (miasma 60 ticks after round start) |
| Team Revive | Sim | `revive.rs` (TDM revivers, TeamRevive shock circle) |
| Gunn Style | Sim + View | `variant_fx.rs` (10-tick delay, 30 frozen + 10 fade per death); overlay `View::gunn` |

## Treasure (global)

| Variant | Kind | Where |
|---|---|---|
| No Treasure, Max Treasure, Always Big Treasure, Bottomless Treasure, Bomb Chests, Symmetrical Treasure | Sim | `treasure.rs::get_chest_spawns_for_level` |
| Show Treasure Spawns | View | `View::treasure_previews` (chests waiting to appear) |
| Treasure Draft | Sim + View | `versus/draft.rs` (order, offers, picks from the pickers' inputs, gains, holds the intro); `HudView::draft` |

## Arrows (per player)

| Variant | Kind | Where |
|---|---|---|
| No Arrows, Single Arrow, Max Arrows | Sim | `versus/spawn.rs::inventory` (Max also excludes the Arrows pickup) |
| Regenerating Arrows | Sim | `player.rs` update step 4 (90 ticks, start type) |
| Secret Arrows | View | `view_items::quiver_huds` (hidden except aiming / after the P-tag) |
| Start With Bomb / Laser / Bramble / Drill / Bolt / Super Bomb / Feather / Trigger / Prism / Toy Arrows | Sim | `versus/settings.rs::start_arrow_type`, `spawn.rs` |
| Start With Random Arrows | Sim | `versus::begin_round` draws `treasure::get_random_arrow_type(true)` (exclusions and unlocks honoured) |
| Infinite Lasers / Drills / Brambles | Sim | `arrows/mod.rs`, `arrows/drill.rs`, `arrows/brambles.rs` |
| No Seeking Arrows, Super Seeking Arrows | Sim | `arrow.rs::seek`, `arrows/bolt.rs`, `arrows/drill.rs` |
| Anti Gravity Arrows | Sim | `arrow.rs` (never falls) |

## Archers (per player)

| Variant | Kind | Where |
|---|---|---|
| Start With Shields / Wings / Speed Boots, Start Invisible | Sim | `versus/spawn.rs` (+ treasure exclusions) |
| Regenerating Shields | Sim | `player.rs` update step 5 (240 ticks) |
| Stealth Archers | Sim | `variant_fx::player_added` (no LightSource; absent from `View::lights`) |
| Double Jumping | Sim | `player.rs` |
| Exploding Corpses | Sim | `variant_fx::corpse_update` (60 ticks, explosion credited to the dead archer) |
| Trigger Corpses | Sim | `variant_fx::corpse_update` (40-tick charge while Shoot/Dodge/Jump is held) |
| Return As Ghosts | Sim | `corpse.rs::ghost_update`, `enemies/player_ghost.rs` |
| Corpses Drop Arrows | Sim | `variant_fx::corpse_update` (one arrow per 10 ticks, alternating sides) |
| Encumbrance, Small Quivers, No Quivers | Sim | `player.rs`, `versus/spawn.rs` |
| No Dodging, Super Dodging, Clumsy Archers | Sim | `player.rs` |
| Show Dodge Cooldown | View | `View::cooldowns` (also the enemies' recovery stars) |
| Free Aiming | Sim | `player.rs` aim |
| No Slipping, Slippery Floors | Sim | `player.rs`, `entities/surfaces.rs`, corpses |

## Curses (per player)

| Variant | Kind | Where |
|---|---|---|
| Cursed Bows, Cursed Dodges | Sim | `player.rs` (cause Curse) |
| Big Heads | View | `view.rs` head drawable (scale 2, origin Y - 5) |

## Arena (global)

| Variant | Kind | Where |
|---|---|---|
| No Hazards | Sim | `entities` loader skips the moving hazards |
| Always Dark | Sim + View | `pickups/orbs.rs` (permanent darkness; `View::dark`) |
| Slow Time | Sim | `time::OrbLogic` (permanent slow motion) |
| Always Lava | Sim | `entities::start_lava` (variant control) |
| Always Scrolling | View | `pickups/orbs.rs::start_scroll` at round start (`View::camera_offset`) |
| Offset World | View | `pickups/orbs.rs::do_offset_world` in the round intro (`View::camera_offset`) |
| Dark Portals | Sim | `variant_fx.rs` (portal at a random Spawner, Bat/Slime while fewer than 3 enemies, every 61 ticks) |

## Powerups (global)

| Variant | Kind | Where |
|---|---|---|
| Ignore Tower Item Set, Arrow Shuffle | Sim | `treasure.rs::compute_rates` |
| No Shield / Wings / Speed Boots / Looking Glass / Extra Arrows / Bomb / Laser / Bramble / Drill / Bolt / Super Bomb / Feather / Trigger / Prism Arrows, No Bomb, No Dark / Time / Lava / Space Orb | Sim | `treasure.rs::item_exclusions` (orb exclusions also remove the chaos orb) |

Port decisions: Gunn Style is always available (the original hides it behind a cheat);
the Treasure Draft pick order keeps the original's even-team quirk (Red counts as the
leader while the match is undecided), appends further trailing members for teams larger
than two, and orders 3+ teams by ascending score.
