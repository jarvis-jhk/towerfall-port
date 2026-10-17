# Front end: implementation notes

`crates/game/src/front/` implements `docs/spec/ui.md` and `input.md` for
Versus. This file records what the code does where the specs leave room,
deviations, and open points. Screens can be rendered headless:

```sh
target/debug/towerfall-port --headless --frames 60 --screen rollcall --players 12 \
    --native-screenshot scratch/ui/rollcall.png      # or --screenshot (window size)
scratch/ui/shots.sh                                  # every screen, all layouts
```

`--screen NAME` starts on (and stays on) a screen: `intro, title, main,
rollcall, versus, variants, teams, map, options, controls, keyboard, pause,
match-end, ready`. `--players N` pre-joins N archers with stand-in devices
(slot 1 keyboard, others alternating Xbox / DualShock). `--capacity 4|8|12`
forces the layout. Without `--screen` the binary starts at the intro and
the menus are fully playable with keyboard and pads.

## Layouts

| Capacity | Virtual width | Rollcall | Source |
|---|---|---|---|
| 4 | 320 | one row, 60x120 portraits, name plates, device names | Steam build (ui.md 6.1) |
| 8 | 420 | 2 x 4, 60x60 portraits, P1..P8 | 8-player build |
| 12 | 420 | 2 x 6 at x = 35 + 70 i | ui.md 18 (our design) |

The capacity follows the "ARCHERS" option (ours): AUTO picks 4 / 8 / 12 by
the number of local devices (pads + keyboard layouts), never below the
highest joined slot. 60x60 portraits come from the 8-player install's
menu atlas when present, else the middle of the Steam 60x120 art.

## Device binding

* 4 slots: the original's `AssignInputs` — unjoined slots follow device
  order (pads in connection order, then keyboard layouts), re-run every
  tick, so plugging in a pad moves the keyboard to the next slot. Back from
  an unjoined element leaves the screen (clears all joins).
* 8 / 12 slots: input.md 10.2 — no slot is bound in advance. The first
  Confirm/Start of an unbound device takes the lowest free slot (bound but
  not joined: carousel, alt costume, device name shown); the next
  Confirm/Start joins. Back while bound-unjoined frees the slot; Back from an
  unbound device leaves the screen. The press that binds never also joins.
  Not done: preferring the slot a device used before.
* Unplugging a bound pad un-joins and frees its slot.
* Remote (online) owners: `settings::Owner::Remote { peer, name }` is the
  hook for `tfp-net`; such slots draw the name and take no local input.

## Archer uniqueness (12 players)

Original rule (archer index unique) while any *usable* archer is free;
then the (archer, costume) pair; once no usable unique look is left,
duplicates. "Usable" excludes Red without the Dark World content (Red is
still shown in the carousel and gives the lock message, like the original;
automatic carousel steps skip it). Duplicate looks are not yet tinted
differently in game (rendering decides; versus.md 15.1).

## Screens: notes and deviations

* **Main menu:** CO-OP and TRIALS lead to the co-op screens (below),
  ARCHIVES to the records (`archives.rs`); WORKSHOP and CREDITS are drawn
  but play `ui_invalid` (not part of the port yet). ONLINE (ours) is a
  blade: on 320 alone in the top-left corner (up from VERSUS; the blade
  stack's top would sit under the VERSUS label), on 420 at the top of the
  blade stack. Returning to the main menu selects what was chosen last.
* **Versus settings:** FFA limit 12 (versus.md 15.2: no forced mode change
  above 6). Goal scores per versus.md 3.2 / 15.5 (HH 14 for 9-12).
* **Variants:** all 89 Steam variants in menu order, including the 19
  *Unlisted* "No X" exclusions (they only stay out of the round-intro icon
  strip; ui.md 8 was corrected after checking the reference). Without Dark
  World content its variants are greyed in a final "DARK WORLD EXPANSION"
  section. Up/Down move to the nearest item of the adjacent row. Presets are
  kept for the session only (no save file yet). No "NEW" tags.
* **Team select:** 2 teams as in the original; 3-6 teams (versus.md 15.4):
  with Team Deathmatch selected, Alt on the mode row cycles TEAMS 2..min(6,
  archers) (shown under the mode name); the team select then shows one column
  per team (Blue/Red banners, tinted banners for Green, Yellow, Purple,
  Orange), Left/Right move along the columns (neutral tags flash at the
  centre), Alt = BALANCE (everybody into even teams). Valid when everybody is
  assigned and every team has a member. 12-player rows at y = 38 + 16 i.
* **Map:** Versus strip (RANDOM + towers), Dark World towers locked without
  the expansion, no-random flags (Alt2), random pick with hop animation,
  seed entry on procedural towers (Down), zoom + SineIn fade. Not drawn:
  the animated map decorations (Twilight Spire, boat, Sunken City, gates),
  unlock sequences, workshop button.
* **Main menu, 420 wide (5-12 archers):** FIGHT! in the middle, CO-OP
  (big button; Quest stays 1-4 archers) on the right under the logo, TRIALS
  and ARCHIVES (small) below it, the blades on the left. Up/Down/Left/Right
  links follow that layout.
* **Treasure Draft (ours: layout; order and offers per pickups.md 11):** a
  panel in the intro: the pick order as P-tags (the picker flashes, those
  done are dimmed with their pick's icon), the remaining offers (first
  frame of each pick-up image, <= 16 px), the picker's tag over the cursor
  and a blinking box while choosing, "Pn TOOK X". Bots pick after a short
  look around (90 frames).
* **Pause / match end:** `pause::PauseMenu` is the reusable entity for the
  level scene; the front end shows it over the chosen level (frozen).
* **Options:** FULLSCREEN, MUSIC, SOUNDS, CONTROL OPTIONS, VIDEO SCALE
  (ours: fit / integer / stretch), ARCHERS (ours), versus lessons, the three
  effect toggles, VERTICAL SYNC (restart dialog). Left out (no backing
  feature yet): replay options, dev console, clear all data.
* **Control options / keyboard:** hold to pause, discovery, rumble,
  KEYBOARD LAYOUT single / shared (ours: WASD + arrows pair), configure
  keyboard with the original's 11 prompts, icon-less and duplicate keys
  rejected. Bindings are listed as text + key caps instead of leader lines
  on the pad picture.
* **Configure controller (ours, `remap.rs`; the original has no pad
  rebinding):** last row of the control options. The controller that
  presses a button is the one shown (its SDL name, "(DEFAULT)" until
  customised); Confirm captures one input per gameplay action (JUMP, SHOOT,
  DODGE, ALT SHOOT, ARROW TOGGLE, START: any face/shoulder/stick button or
  a trigger, not the d-pad, Guide or Back), duplicates rejected, the pad's
  Back/View button or the keyboard's Back cancels; Alt resets the model to
  the default layout. Maps are per controller model (`Options::pads`),
  used by the match (`play::pad_input_with`); menus keep the fixed
  buttons.
* **Persistence (`options_store.rs`):** all options, keyboard layouts and
  controller maps are written to `options.cfg` in the data directory
  (`$TFP_DATA_DIR`, else `$XDG_DATA_HOME/towerfall-port` /
  `%APPDATA%\towerfall-port`) as soon as they change; plain `key = value`
  text, unknown keys ignored. Fullscreen and vsync apply at start-up (the
  CLI flags still win), scale every frame. On the first run (no
  `options.cfg`) the original's `tf_saveData` is imported read only when
  found (volumes, fullscreen, effects, pause / discovery / rumble, vsync,
  lessons, keyboard configs with XNA key names); `--import-save` /
  `--save-file PATH` repeat it. Headless runs use the defaults and never
  write.
* **Match ready (temporary):** after the map, shows (and prints) the
  resulting setup over the level; Start opens the pause menu, Confirm the
  end-of-match menu. Replaced by the game scene.
* Not implemented: credits, unlock panels, the saver, unlock save data.
* **Team select (layout ours for 3-6 teams and 12 slots):** 2 teams keep the
  full banners at every size; 3-6 teams put half-size banners and names in
  a band under the title with the rows below (12 slots: scale 1, 12 px
  apart); on 320 the FIGHT! banner sits at y 211 (clear of the button
  guide), the 4-slot rows at 135 + 18 i.
* **420 main menu:** CO-OP at (372, 150), under DARK WORLD and with its
  skull orbit inside the screen.
* **Map screens:** no button guides (only the main menu has them).

## Result type

`settings::FrontEndSetup { slots, settings: MatchSettings, tower_name }`;
`FrontEndSetup::to_sim(level, seed)` builds `tfp_sim::world::MatchSetup`
(archers, allegiances, start inventory from variants, the variant flags the
sim knows).

## Co-op screens (Quest, Trials; `crates/game/src/coop_ui/`)

```sh
target/debug/towerfall-port --headless --frames 60 --screen coop --native-screenshot scratch/ui/coop.png
target/debug/towerfall-port --headless --frames 60 --screen quest-map --players 2 ...
target/debug/towerfall-port --headless --frames 60 --screen trials-map --players 1 ...
scripts/dev/coop-proof.sh     # bots play; scratch/quest-sheet.png, scratch/trials-sheet.png
```

* **Flow:** main → CO-OP (QUEST / DARK WORLD; without the expansion DARK
  WORLD shakes and plays `ui_invalid`; see the Dark World section) → rollcall (mode Quest) → quest map → difficulty
  → zoom → game. TRIALS → rollcall (mode Trials) → trials grid → zoom →
  game. The map leaves a `CoopLaunch` in `Session::coop`; `Game` takes it
  (`FrontEnd::take_coop`) and runs `coop_ui::scene::CoopPlay`; MAP / QUIT
  come back through `FrontEnd::return_to` (the mode's map or the main
  menu, menu music restarts).
* **Rollcall modes** (`RollcallMode`, ui.md 6): Quest 1-4 archers (ours:
  up to 4 with quest.md 7 scaling, the original caps at 2), Trials exactly 1
  (starts by itself 20 ticks after the join, no ready banner); co-op
  rollcalls start with nobody joined; Back from a quest rollcall returns
  to the co-op menu. Titles `quest` / `trials`.
* **Quest map** (ours where ui.md is silent): the versus map scene's look
  (water, land, cursor, camera tween, zoom) with one block per quest tower
  (all 14 revealed: everything in the install is unlocked), special /
  super-special blocks for towers 8-11, skull badges (white normal, red
  hardcore, gold no-deaths) from the session records, best time under the
  title. Confirm opens a NORMAL / HARDCORE panel (Up/Down, Confirm, Back).
* **Trials grid:** one column per tower (16 with Dark World), tier I at
  the strip, II and III stacked above as small blocks with the numerals;
  Left/Right keep the tier, Up/Down change it; medal badge (gold, diamond,
  dev) per level; GOLD / DIAMOND / BEST times under the title. Levels whose
  file is missing (no Dark World) are locked.
* **In game:** Start pauses (Quest: RESUME, RESTART, MAP, QUIT; Trials:
  RESTART, RESUME, MAP, QUIT); the end menus open when the sim's result
  screen is ready (Quest game over: RESTART, MAP, QUIT; complete: MAP,
  RESTART, QUIT; Trials: NEXT TRIAL (tiers I/II), RESTART, MAP, QUIT, or
  RESTART first after a failure). RESTART skips the title card. Without a
  human device everything is played by bots and MAP is chosen by itself.
* **Records:** session only (no save file yet): quest skulls, best times
  (1 / 2+ archers), attempts; trials best time, medals, attempts; co-op
  crowns (quest.md 5.5) for the next session start.
* **Layout choices (ours):** game over title at y 66, status 96, tip banner
  at 145 with white outlined text, menu panel centred at y 212; complete:
  banner 36, portraits 84, stats 118-138, lives tally 154 with the skull
  after it, the bottom banner (mode, then the time) drops to y 172. Lives
  rows for players 3-4 sit 12 px below rows 1-2. The revive in progress
  shows a pulsing `teamRevive` icon over the corpse (ours).

* **Debris** (`coop_ui/debris.rs`, enemies.md 7): enemy drops follow the
  original's families. Light (slime eyes, halos): gravity 0.15, halved near
  the apex. Heavy (knives, scythe parts, skulls, staffs, mole shells,
  fallen crystals): gravity 0.3 to 3, floor bounces at -0.5 with a random
  kick and the drop sound, walls -0.5, settles upright, 180 ticks then a
  120-tick fade. Floaty (bat wings): sways down at 0.08 to 1.2. A mimic
  that copied an archer leaves that archer's corpse and a shadow burst. The
  last King Reaper leaves the king's head, which rolls and stays.
* **Controls panel** (`coop_ui/intro_controls.rs`): before the first
  Quest tower's title card in Normal mode, one panel per distinct joined
  device (a keyboard panel when only bots play); the sim waits until it
  is confirmed (bots after 150 ticks).
* **King's Court:** from `Quest::king_transformed` the level dims to a
  spotlight on the King (20-tick fade in) through the sim's 60-tick pause
  and its `transform` frames; when the Reaper comes out (transform frame
  16) the King NPC and its light go away and the spotlight moves to the
  Reaper until 120 ticks after it appeared (20-tick fade out).

## Dark World (M10; `crates/game/src/coop_ui/darkworld/`)

```sh
target/debug/towerfall-port --headless --frames 60 --screen darkworld-map --players 2 --native-screenshot scratch/ui/dw-map.png
target/debug/towerfall-port --headless --play --mode darkworld --tower 0 --players 2 [--difficulty 0-2]
    [--variants "CURSED BOWS,START WITH SHIELDS"] [--dw-level N] [--dw-assist N]
    [--quest-end complete@N|gameover@N] --shots DIR
scripts/dev/darkworld-proof.sh   # scratch/darkworld-sheet.png (level, 4 bosses, game over, clear)
```

* **Flow:** CO-OP → DARK WORLD (initially selected when it was the last
  co-op mode) → rollcall (mode DarkWorld, 1-4 archers, title
  `darkWorld`; Back returns to CO-OP) → Dark World map → difficulty
  overlay → variants overlay → BEGIN! (or Start) → zoom → game. MAP from the
  game returns to the Dark World map. Everything in the install is
  unlocked (towers 3 and 4 are not gated on clears).
* **Map:** ui.md 9.2 (Dark World): five blocks, skull / eye badges and the
  stats overlay from the session records (the totals are sums of the
  session's Legendary best times / most curses for the current player
  count). The variants overlay shows "n/cap" under the power-up box (ours)
  and the curse list for the player count (Exploding Corpses for 2+, No
  Treasure solo); the launch cleans the set for the joined slots and runs
  `darkworld::enforce_variants`. Choices persist for the session
  (`CoopSession::dw_*`).
* **In game** (`coop_ui::scene::CoopPlay`, Dark World mode): title card
  (waits for Confirm), LEVEL n / FIGHT!! and boss cards with the variant
  icons, boss title, gems (solo), boss bar, GO!, fades between levels,
  game over, complete with cards and crowns (darkworld.md 6, 6.1, 9).
  Pause: RESUME, [RESTART LEVEL (n)] (a continue exists and level > 1),
  RESTART, MAP, QUIT; game over (after the sim's menu delay): [CONTINUE
  (n)], RESTART, MAP, QUIT (no number when continues are unlimited);
  complete: MAP, RESTART, QUIT after the sequence and a Confirm. CONTINUE /
  RESTART LEVEL call `darkworld::continue_level`, RESTART a new session.
  Bots walk into the open exit portal and target boss bodies, open eyes
  and shield orbs.
* **Trigger-arrow slide:** Darkfang's first level (from the map), once per
  run, only with SHOW VERSUS LESSONS on (the original gates it on the same
  `ShowTips` option; persisted in `options.cfg` as `lessons`, imported from
  `tf_saveData`). The sim runs it as its `DwPhase::Tips` phase
  (`DwSettings::trigger_tips`, closed by Jump / Start / Dodge); the HUD
  follows the phase. Without a human device the presenter closes it after
  120 ticks. Without a device icon the line reads PRESS ALT SHOOT.
* **8 / 12-player layout:** CO-OP on the right under the logo leads to the
  same co-op menu; the Dark World rollcall still takes 1-4 archers and the
  map is laid out 420 wide (the arenas stay 320 wide).
* **Music** (audio.md 6.2): the tower theme from 30 ticks into a normal
  level (also under the level-1 title card); boss levels play nothing until
  the boss card is over, then `DarkBoss` until the boss dies (Cataclysm:
  until its heart appears); silence from the transition out of the
  second-to-last level; `GameOver` only when no continue is left; on a
  clear silence, then 50 ticks in the victory jingle (`VictoryTeam` for 2+,
  else the archer's own).
* **Records:** the save store (`save.rs`) is the source of truth. A
  session's `DwResult` goes to `save::record_dark_world` once, when the
  session ends (the clear, or RESTART / MAP / QUIT from its game over;
  CONTINUE keeps the session going; a quit from the pause records
  nothing). The maps' badges and stats panel read the store through
  `save::coop_records()` (`DarkWorldTower` -> `darkworld::DwRecord`), and
  `game.rs` re-seeds the front end from it after every co-op game. In the
  scene, `DwRecord::complete` keeps the session copy current.
* **Boss art:** darkworld.md 6.1. Animation clocks, wiggles, jitter, fades,
  the look direction, horns, dust, the burst eye sockets and the break
  animations of removed parts are presentation state (`bosses::BossFx`),
  restarted from sim state changes (boss state, coroutine step, eye /
  open flags). Turn to dust is per pixel like the original's
  fall-and-sweep: each pixel starts 4 ticks per row from the bottom plus a
  random 0-9, falls 0.2 px a tick, fades from 60 ticks in, and drifts
  sideways (still for 60 ticks, then easing up to 0.6 px a tick by 480).
  The Dreadwood's flowers fall one at a time (shuffled, 30-40 ticks apart)
  with the original's sway; its green leaf ceiling fades out (300 ticks,
  CubeOut) over the withered one; the background tentacles' wiggle and
  sway rates follow the death sequence (1, 0, 4, 0). The Amaranth's
  shield grows and spins in over 240 ticks from the intro's music cue; its
  eye eases to the sim's look angle at 0.2 a tick. Hit flashes use the sim's boss flash where it has one (the
  Amaranth, Darkfang) and a 20-tick flash on any health drop otherwise.
* **Deviations (ours):** the eye awards on the complete screen sit right
  of the CONTINUES / DEATHS lines (the original centres them over the
  lines); the game-over screen adds LEVEL n, the continues left and the
  difficulty under the title; player cards for more than 4 archers are
  spread evenly.
* **Exit-portal zoom:** the composited level is drawn into a canvas
  (`TowerView::screen_target`) and scaled 1 -> 3 about the portal (40 ticks
  CubeInOut from 31 ticks into the transition); the HUD and the fade stay
  unzoomed, as in the original.
* **Proof aids:** `--dw-assist N` (the sim test's scaffolding: invulnerable
  bots with arrows, never out of lives solo, leftovers cleared, archers
  carried into the open portal after 4 s, one point of boss damage every N
  ticks while hurtable, intermission enemies and old shield orbs removed;
  Darkfang's fists crush through invulnerability, so the archers are
  removed for that fight). `--quest-end complete@N` jumps to the last level
  and removes its boss.
* **Miasma** (`coop_ui/miasma.rs`, all modes through `art::apply`): body,
  tentacles and surface strips per tower-entities.md 8.2.
* **Quest wave visuals** (`coop_ui/screen_fx.rs`, `coop_ui/miasma.rs`):
  floor miasma strips (one `FloorMiasma` sprite per 10 px at (10 i, 5),
  Y-scaled by the strip, animation 0 while starting / arming, 1 idle, 2
  dying out); dark waves / the dark orb through `View::dark` (only the
  lights shine, as in Versus); slow waves / the time orb as the screen wave
  (frequency 6, amplitude `0.05 * (1 - TimeRate)`, gated by SCREEN WAVE
  EFFECTS); scroll waves / the space orb as `View::camera_offset` with the
  level image wrapping (the shipped quest data has no scroll waves; the
  space orb uses it). The level is composited into a canvas
  (`TowerView::screen_target`) whenever a camera transform is active.
* **Trials:** chained dummies use frame 1, rotated with the chain's last
  link; the `DummyHead` image is drawn on top; both are outlined. Hidden
  gems: `TrialsSettings::gem_found` comes from the records (the chest
  starts empty); `Trials::gem_collected` is kept at once in the records and
  the save store (`save::with`, `TrialsLevel::gem`, also imported from
  `tf_saveData`'s `YellowGemsFound`); the find shows "n / 7" where the gem
  was, in the Yellow archer's two colours (swapping every 10 ticks, fading
  over ~140 ticks); the trials grid shows `trials/yellowGem` at the block's
  top right.
