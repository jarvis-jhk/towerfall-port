# Community mods, natively

Jan (2026-10-19): take the most popular existing TowerFall mods and build
their behaviour into the port, every one of them optional. This file is the
survey that picks them, so the choice can be reviewed, and the record of
whose idea each feature was. The plan and the rules are in
[ROADMAP.md](../ROADMAP.md#community-mods).

## How the list was made

Survey of 2026-10-19:

- **GameBanana** (the TowerFall section, game 18654) is where FortRise
  points players for mods; its 60 TowerFall mods were ranked by downloads
  (counts as of the survey date).
- **GitHub**: the mod repositories by stars (Bartizan, Bartizan+, TF.EX,
  TF-8-Player, Towerfall-Redemption, ebe1kenobi's FortRise mods).
- **towerfall.wiki.gg/wiki/Modding** for what the community lists.

The TowerFall modding scene is small: the most downloaded mod has 375
downloads. "Popular" is therefore a soft ranking; the order below is the
download order, with ties broken by views.

Skipped and not counted:

- **Frameworks and libraries**: FortRise, Bartizan's patcher (its bundled
  *mods* are counted, item 4), Bartizan+'s patcher, Ascencore, LoaderAI,
  MonoMod/Harmony. Counted instead is what they enable.
- **Already in the port**: Wider Set (Terria, RedDude) and
  TowerFall – 8 Players Mod for FortRise (Juan_JSAR, from Jonesey13's
  TF-8-Player): we have 5-12 players on the wide towers. TF.EX (DShad):
  we have rollback netplay over Warpzone. AIJimmy / AINova (ebe1.kenobi):
  we have bots.
- **Art only, nothing to reimplement**: Mech KingReaper Skin (Terria), and
  the archer skins, which are handled by item 1 instead of being drawn
  again.
- **Outside the game**: Discord Presence (Terria; talks to the Discord
  client), TowerFall Localization (Juan_JSAR; translation is its own topic,
  not a mod feature).

## Licences

Every GameBanana mod here uses GameBanana's default permissions: download
and install yes; redistribute or modify "ask the author". The GitHub
repositories have no licence file (Bartizan, Bartizan+,
Towerfall-Redemption, ebe1kenobi's mods: all rights reserved) except
TF-8-Player (MIT) and TF.EX (GPL-3.0), neither of which we use. So:
**nothing is copied** (no code, no art, no sounds, no levels). Every
feature is rebuilt clean-room from the mod's public description and its
observable behaviour, drawn with the player's own TowerFall data or with
our own art, and credited here and in the in-game mod list.

## The twenty

| # | Mod (author) | Downloads | What it does | Kind | Native plan |
|---|---|---|---|---|---|
| 1 | **Archer packs**: Custom Archer Loader (RedDude), counted for Madeline & Badeline (Terria, 253), King of TowerFall!, Among Us: Red & Blue, Crimson/Spooky Puppet, Shadow Archer, Hitbox Archer, Osiria Archers, King Louis, Perplexing/Rainbow Puppet (DantinouTheRealOne), Yuu! (liliovali) | 253 (top archer pack) | New playable archers from sprite packs | Archers | Load archer packs the player installed themselves (ArcherLoader folder layout: sprites, portraits, colours) from our data dir at run time, as we read the game's own files. We ship none of them. Online, peers without the pack see the base archer it is built on. |
| 2 | **Baron Mode** (Terria) | 119 | Last Man Standing where Quest gems are lives: with a gem you come back, without one you stay dead | Mode | Versus mode on the LMS rules; gems from chests/kills, spent on respawn. |
| 3 | **Additional Variants** (Terria, DShad) | 118 | 24 variants: Bottomless Quiver, Atomic Arrow, Annoying Mage, Shock Death, Chest Death, Jester's Hat, Darker World, Lava Overload, Chaotic Roll, No Hypers, No Dodge Cancel, Fading Arrow, Neon Arena, Dash Stamina, King's Wrath, No Arrow Tinks, Drilling Arrow, Unfair Autobalance, Auto Open Chest, Exploding Shield, Clumsy Swap, No Explosion Damage, Fragile Prism | Variants | Each one a mod variant in its own column of the variants menu. |
| 4 | **Bartizan** mods (Kha, TehMillhouse; FortRise port by Terria) | 94 | Respawn mode, Crawl mode (ghosts score by killing the living, a kill brings you back), Win Counter; variants No Head Bounce, No Ledge Grab, Awfully Slow Arrows, Awfully Fast Arrows, Infinite Arrows, No Dodge Cooldown | Modes, variants, UI | Two modes, six variants, a win counter on the results that lasts until player select. |
| 5 | **TowerBall** (Suyooo; FortRise port by CoolModder, Terria) | 84 | Basketball: dunk balls into a basket, players respawn; variants Single Trigger Arrow, Exploding Corpses, Big Heads (dunk), Bottomless Treasure, Anti-Gravity Arrows, Toy Arrows | Mode | Team mode with ball and basket entities; baskets placed by a per-tower table, ball physics shared with the planned Orb Ball mode. |
| 6 | **More Replay** (Terria) | 80 | Instant replay after Quest/Dark World game over; save/export a replay | UI | Reuse the Versus replay buffer in co-op; export as GIF/PNG sequence to the data dir. |
| 7 | **Training Dummy** (Terria et al.) | 80 | Play as a Trials training dummy | Archer | Archer built at run time from the player's own Trials dummy sprites (art risk: the dummy has no run/jump frames, so it may need procedural squash/stretch). |
| 8 | **Skeleton Archer** (Terria) | 77 | Play as the skeleton enemy | Archer | Archer built at run time from the player's own skeleton enemy sprites. |
| 9 | **Colored Drill Arrows** (Terria) | 53 | Drill arrows tinted in the owner's colour | Cosmetic | Palette tint in the renderer. |
| 10 | **Infinity Gauntlet** (Terria, co-author) | 52 | Endless co-op Dark World climb, see how high you get; Normal / Hardcore (enemy archers only, every arrow, regenerating arrows) / Legendary (every non-archer enemy) | Mode | Endless co-op run that strings the player's own Quest/Dark World levels together with rising waves; no levels shipped. |
| 11 | **Miasma Colors** (Terria) | 51 | Each Quest level gets its own miasma colour | Cosmetic | A colour per level theme in the renderer. |
| 12 | **Autolock Modifier** (Terria, DantinouTheRealOne) | 50 | Configure the aim assist (autolock) | Option | Per-player strength/off setting; it changes the sim, so it is part of the match config. |
| 13 | **Profiles** (Terria), with ebe1.kenobi's **Archer** (18) | 45 | Named profiles with favourite archer and own keyboard/pad config | Option | Profiles in our options store, picked on archer select. |
| 14 | **MenuVariants** (CoolModder) | 44 | Custom title screens and side bezels; Ascension or Dark World logo | Cosmetic | Logo choice and our own bezel styles. |
| 15 | **Speedrun Timer** (Ralidon) | 31 | Run timer in Quest and Dark World | UI | HUD timer from the sim's frame count, with splits per level. |
| 16 | **Score Counter** (Terria) | 28 | The old (pre-Dark World) Quest score counter | UI | HUD option. |
| 17 | **Music Restart** (Terria) | 24 | Restarting a level from pause restarts its music | Option | Audio option. |
| 18 | **Fortfeit** (Terria) | 23 | Forfeit a running match after the replay and go straight to results | Option | Pause/replay option; online the host decides. |
| 19 | **Boss Practice Towers** (Terria) | 20 | Go straight to a Dark World boss without the levels before it | Mode | Boss select in the Dark World map using the existing boss levels. |
| 20 | **Power** (ebe1.kenobi) | 15 | Every archer gets two of 25 powers on the triggers (beam, disc, flight, phasing, invisibility, shield, arrow magnet, tile wall, tunnel, grappling hook, gravity well, shockwave, swap places, shadow clones, curse, ...) | Mode/variant | Powers as sim entities with cooldowns, built in small groups. |

## Next in line

If one of the twenty turns out not to be doable, the next by downloads:
Soccer (ebe1.kenobi, 15), No Seek Quest (CoolModder, 13), Accelerate (13),
MoveOrDie (12), Arcade announcer (11), Lava pickup (11), Tournament (10),
Handicap (10), Gravity pickup, PlayTag (overlaps the planned Hot Potato),
Missile arrows, Black Hole, Corpse, Rotate, Respawn with lives, WinCounters,
Scroll (all ebe1.kenobi); CoolModder's Oops, All Arrows (11 arrow types:
ice, slime, bait, prism trap, land mine, rocket, freaky, tornado, mech,
boomerang, nyan) and Warlord mode; Bartizan+'s variants (zpchavez: Crown
Summons Chalice Ghost, Ghost Revives, Ghost Items, Ghost Joust, Variety
Pack, CalvinFall, Meaner Monsters, ...).

## Links

- GameBanana: https://gamebanana.com/games/18654 (each mod at
  `https://gamebanana.com/mods/<id>`: Baron 590115, Additional Variants
  589901, Bartizan 590131, TowerBall 461512, More Replay 463749, Training
  Dummy 619424, Skeleton Archer 638281, Colored Drill Arrows 589965,
  Infinity Gauntlet 634554, Miasma Colors 466427, Autolock Modifier 607280,
  Profiles 693658, MenuVariants 461510, Speedrun Timer 628505, Score
  Counter 619407, Music Restart 460729, Fortfeit 688948, Boss Practice
  Towers 693672, Power 714876, Madeline & Badeline 619431)
- Bartizan: https://github.com/Kha/Bartizan; Bartizan+:
  https://github.com/zpchavez/Bartizan
- TowerBall: https://suyo.be/towerball/
- Towerfall-Redemption: https://github.com/CoolModder/Towerfall-Redemption
- Custom Archer Loader: https://github.com/RedDude/ArcherLoader
- ebe1.kenobi's mods: https://github.com/ebe1kenobi
- FortRise: https://github.com/FortRise/FortRise
