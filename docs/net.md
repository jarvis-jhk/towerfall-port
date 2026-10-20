# Online play (`tfp-net`)

towerfall-port plays online with GGRS rollback sessions whose packets travel
over [Warpzone](https://github.com/jarvis-jhk/warpzone) peer channels, and
**only** over Warpzone (rooms, codes, room browser, NAT traversal, relay).
`warpzone-core` is used directly from Rust (git dependency pinned to
`c967f5c1`, the commit checked out in `ext/warpzone`); the game is a tier-2
Warpzone game (`packaging/warpzone/towerfall-port.toml`). The code is adapted
from our rounds-port netcode (`rounds-net`).

* Each process is a **machine** with 0..12 local player **slots**; a match
  has at most 12 slots in any mix (12 × 1, 1 × 12, 4 × 3, 2 + 4 + 6, ...).
  Machines without slots can **spectate**.
* Fixed 60 Hz step (TowerFall's 1/60 s); GGRS is told 60 fps.
* The room host is authoritative for the lobby: it assigns machine ids,
  global slots, archers and teams, picks the seed and settings, starts and
  ends matches, decides when a machine has dropped out, and feeds the
  spectators. The host always plays (at least one slot).

## At a glance

| | |
|---|---|
| Net protocol | **2** (`tfp_net::wire::PROTOCOL`; protocol 2 added `MachineView::info`). Machines only play together with the same `towerfall-port <version> net<protocol>` (`online::net_version`, Warpzone `game_version`, checked in `Hello`). |
| Played online | Versus (Last Man Standing, Headhunters, Team Deathmatch with 2-6 teams; any tower incl. Cataclysm; variants for everyone or single archers; standard or the 8-player build's wide towers), Quest (1-4 archers), Dark World (1-4), Trials (1 archer, the others watch). 2-12 archers on any mix of machines; spectators. |
| Entry points | main menu ONLINE, `--online`, `--warpzone-host`, `--warpzone-join ROOM|CODE|INVITE` (the descriptor's `[launch]`; values of at most 8 characters are join codes) |
| The host decides | settings (game, mode, length, teams, tower, variants, difficulty), the slot table (archers unique while possible, teams balanced), the seed and the procedural layout seed per match, wide or standard towers, start (every machine ready, or Start twice), kick, the Dark World game-over menu |
| Every machine checks | the same game and protocol version; the same content (FNV over the level / script files, the archer data and the generated geometry) for the chosen tower or co-op mode, else it refuses the match |
| Leaving | a machine that leaves or goes silent: its archers stand idle for the rest of the round and are removed at the round end (Versus), freeze/resume for everyone else; the host leaving ends the room |

**Lobby rules in detail.** *Wide towers*: only when every playing machine's
8-player-build hash equals the host's, more than 4 archers are in the slot
table and the tower exists in that build (the lobby says which towers and
why). *Variants*: titles on for everyone, per-player variants by (machine,
local player); Dark World variants only when every machine has Dark World,
the Dark World variant screen lists only its power-ups and curses.
*Co-op*: Quest 1-4 archers, Dark World 1-4, Trials exactly one; the Dark
World game-over menu is the host's (CONTINUE / RESTART travel in its input,
MAP ends the match for all, QUIT leaves), guests see it read-only; a
completed co-op run ends the match after its results sequence. *Ready*: the
host's Start needs every machine ready; a second Start within 3 s starts
anyway. *Pause*: local overlay only, the match goes on.

## The game side: `NetGame`

tfp-net does not depend on tfp-sim. The sim implements

```rust
pub trait NetGame: Clone + 'static {
    type Input: NetInput;                                   // any bytemuck::Pod
    fn advance(&mut self, inputs: &[(Self::Input, InputStatus)]); // one 1/60 s frame
    fn checksum(&self) -> u128;                             // desync detection
}
```

`Clone` is how rollback saves a frame. `inputs[slot]` is the input of global
slot `slot` (`MatchStart::players`). The sim must be deterministic bit for
bit (see CLAUDE.md: `libm`, no `HashMap` iteration, no clock, seeded PRNG in
the state). `NetSession::step(&mut game)` runs a frame; `advance()` returns
the raw `Request`s (`Save`, `Load`, `Advance { frame, inputs, resim }`) for a
game that wants to interleave its own bookkeeping. `resim = true` marks a
frame simulated again after a rollback: the presentation layer should not
replay its sounds/particles for it.

### Input: `TfInput` (4 bytes per player per frame)

| Byte | Field | Meaning |
|---|---|---|
| 0 | `aim_x: i8` | direction, multiples of 8 in −120..=120, +x = right |
| 1 | `aim_y: i8` | same, +y = **down** |
| 2 | `buttons: u8` | held: `BTN_JUMP` 1, `BTN_SHOOT` 2, `BTN_DODGE` 4, `BTN_ALT` 8 (alt shoot / Dark World trigger), `BTN_START` 16 |
| 3 | `reserved` | 0 |

Held states, not presses (the sim compares with the previous frame), so
GGRS's "repeat the last input" prediction is right most of the time.
`TfInput::from_stick(x, y, deadzone, buttons)` quantises an analog stick
(radial dead zone, 15 levels per side) so that stick wobble does not cause
mispredictions; `from_dpad` gives the 8 keyboard/d-pad directions at full
deflection. docs/sim-design.md applies the spec's dead zone and 8-way rules
inside the sim (so local and remote players behave alike): then pass
`deadzone = 0.0` here and only quantise. On the wire an input is its raw
bytes (`input::Wire<I>`), zero bytes = no input. The layout is the one
docs/sim-design.md gives `tfp_sim::PlayerInput` (`aim_x, aim_y, buttons,
_pad`), so the two convert with `bytemuck::cast` (or the sim's own type is
used as `NetGame::Input` directly).

## Online play in the game (`crates/game/src/online`, M8)

| Module | What |
|---|---|
| `adapter.rs` | `NetWorld { world: tfp_sim::World, content: Arc<Content> }`: `NetGame` with `Input = tfp_sim::PlayerInput` (byte-identical to `TfInput`), `checksum` = `World::checksum128`; `OnlineSettings` (the opaque `MatchSettings::game` bytes); tower loading and the sim's Versus `MatchSettings` from a `MatchStart` |
| `gate.rs` | `EventGate`: which sim events the presentation may play under rollback |
| `runner.rs` | `OnlineMatch`: one match without graphics: carries out requests, feeds the gate, detects the end on the confirmed timeline |
| `present.rs` | sounds (SFX, archer voices, loops), screen shake, kill/match feed |
| `scene.rs` | `OnlineScene`: menus, lobby, the match on screen (with `versus_ui` fed confirmed frames) |
| `bot.rs`, `harness.rs` | test bots; `--net-harness` / `--net-peer` multi-process tests |

**The adapter.** `advance` turns `InputStatus::Disconnected` into
`World::mark_disconnected(slot)` before the step (idempotent, part of the
cloned state, so it survives rollback and happens at the same frame
everywhere) and steps the world with the 12 inputs. A save is a `World`
clone plus an `Arc` bump. Local inputs are quantised to multiples of 8 per
axis before they enter GGRS (the sim applies the spec's dead zone itself).

**Match settings.** The host's lobby settings carry `OnlineSettings`
(bincode): mode (Last Man Standing / Headhunters / Team Deathmatch), length,
the versus tower's directory name, the number of its level files and an
FNV hash of their bytes, the variants (titles on for everyone, and
per-player variants for single archers as (machine, local player) pairs, so
the flags stay with the player when the slot table is renumbered; they go
into the sim's per-slot `VariantSet`), the number of teams in Team
Deathmatch (2-6), wide towers on/off, the game kind (Versus / Quest /
Trials) with the quest tower or trial and hardcore, tower card on/off. Roster, archers (base archer +
alt costume → the sim's archer index), teams and the seed come from
`MatchStart`. Every machine loads the tower when the lobby names it (guests
while still in the lobby, so `MatchStarted` does not stall the GGRS
synchronisation) and refuses a match whose tower files differ from the
host's.

**Wide towers** (420×240, the official 8-player build). Each machine puts a
`MachineInfo` into the lobby (`NetSession::set_machine_info`, protocol 2):
FNV hashes over all versus level files plus `archerData.xml` of its Steam
install and of its 8-player build (if installed), and whether Dark World is
there. The host plays wide towers only when every playing machine's
8-player hash equals its own, more than 4 archers are in the slot table,
and the chosen tower exists in the 8-player build; otherwise the Steam
towers (320×240). It re-decides on every roster change and shows the
decision and its reason in the lobby ("WIDE TOWERS (8-PLAYER BUILD, ALL
MACHINES)", "STANDARD TOWERS (NOT EVERY MACHINE HAS THE 8-PLAYER BUILD)",
"... (4 OR FEWER ARCHERS)", "... (TOWER NOT IN THE 8-PLAYER BUILD)"). The
per-tower data hash in the settings then guards the actual level files as
before. Dark World variants are offered only when every machine has Dark
World. Procedural towers are not offered online. The sim's Versus flow does
the rest, including the leave rule (`World::connected`, removal at round
end, match decided when fewer than two sides remain).

**Settled frames.** `NetSession::confirmed_frame()` is the newest frame
whose inputs are known; inputs that arrive while a machine waits at the
prediction limit are confirmed before the rollback they cause has been
simulated. Everything that must never see a frame change again — the
confirmed-timeline presentation, the event gate's confirmed events, the
end-of-match report — uses `NetSession::settled_frame()` instead: the frame
that was confirmed when the last `advance` produced its requests.

**Match end.** `OnlineMatch` records the checksum of every state whose
Versus phase is `Finished` (results acknowledged); once the first such
state's frame is confirmed it calls `finish_match(frame, checksum)`, so
every machine reports the same frame, and `MatchEnded { agreed }` compares
them.

**Events and audio under rollback** (`EventGate`):

* *Immediate* events — sounds, archer voices, particles, rumble, screen
  shake — play when **first seen**: each `(frame, identity)` once, where the
  identity is the event without positions (a corrected frame moves things a
  little). A re-simulation that produces the same event stays silent; one
  that produces a different event plays it.
* *Confirmed* events — match flow (round start/end, points, crowns, final
  kill, match end) and deaths for the kill feed — are released only when
  their frame is confirmed, from the final timeline: no announcement for a
  kill a rollback undoes.
* *Looped* sounds (wall slide, ...) follow a per-frame log of on/off
  transitions; a `Load` drops the log of the re-simulated frames, and after
  every batch the wanted set of loops is compared with what plays: loops
  started only in rolled-back frames are **cancelled**, loops stopped only
  in rolled-back frames resume. Confirmed frames are folded into a base set.

**Menus** (`--online`, or `--warpzone-host` / `--warpzone-join ROOM|CODE`
from the Warpzone launcher; a value of at most 8 characters is a join code,
longer ones a room id or invite link):

* *Online menu*: host a room, join by code, browse rooms, quit. The player's
  Warpzone name comes from `TFP_PLAYER_NAME` (else `$USER`), registered on
  first use.
* *New room / match settings*: public (listed in the browser) or private
  (unlisted, join code only); GAME = Versus (mode, length, teams 2-6 in
  Team Deathmatch, tower, VARIANTS) or Quest (quest tower, hardcore) or
  Trials (the trial). The variant screen toggles each variant for
  everyone (ALL) or, for per-player variants, for single archers (P1, P2,
  ... of the slot table); turning one on clears its linked partners.
* *Join by code*: type it (keyboard) or pick letters with a pad (up/down
  letter, right next, start join).
* *Browser*: rooms of this game with their `players` / `mode` / `map`
  metadata, refreshed every 5 s; confirm joins.
* *Lobby*: room code, settings, wide/standard towers and why, then every
  archer of the next match as a rollcall portrait card — local and remote
  alike: the archer's portrait (alt costume), its colour or (Team
  Deathmatch) its team's frame, P-number, the owning machine's name and a
  ready mark — and a strip of the machines with their ping. Any local
  device joins with confirm (several per machine: mixed local + online up to
  12 archers), then left/right = archer, up = alt costume, down = team, back
  = leave the roster. Guests: start = ready / not ready. Host: start = start
  the match once every machine is ready (a second start within 3 s starts
  anyway), F2 or menu-alt = settings, F3 or menu-alt-2 = MACHINES (each
  machine's archers, ping, ready, 8-player build; confirm twice = kick).
* *Match*: the Versus presentation of `crates/game/src/versus_ui` (tower
  card, round intro, P-tags, skulls, particles, final-kill spotlight,
  instant replay, round and match results, awards) plus the network state
  ("waiting for the other machines" / "an archer left, resyncing" /
  "spectating"), tower music and victory jingles (requested by `versus_ui`
  on the confirmed timeline). At the end everybody returns to the lobby.
* *Pause online*: the sim cannot stop for one machine, so Start (or
  Escape) opens the Versus pause menu (`match_pause`) as a **local overlay**
  — RESUME, or QUIT = leave the match and the room — while the match goes on
  for everybody; this machine's archers get no input while it is open (they
  stand idle, like an absent player), the music drops to half volume, and
  the Start that resumes is kept from the sim until released. A local
  player's controller going missing shows the "CONTROLLER DISCONNECTED"
  panel the same way (once per loss); the archer idles until the pad is
  back. The disconnect *leave rule* (inert this round, removed at round end)
  is only for machines that leave the room.

**The Versus presentation under rollback.** `VersusUi` keeps local state per
step (particles, skulls, results clocks, the replay recording), so it is fed
the **confirmed timeline only**: `OnlineMatch` keeps a copy of the world
after every step (`keep_snapshots`; re-simulation overwrites it, a `Load`
drops the later ones) and `drain_confirmed()` hands each confirmed frame
out once, in order, to `VersusUi::after_step`. The HUD (tower card, scores,
round/match results) is drawn from the newest confirmed `HudView`; the level
and the archers are drawn from the live (predicted) world, so play stays as
responsive as offline, except while the instant replay runs (`render_world`
returns the replay frame). The replay is local: recorded from confirmed
frames on each machine, shown while the sim is in the round-results phase.
Local Confirm presses pass `VersusUi::gate_inputs` before they enter GGRS,
so a player can only continue once *their* replay/results animation allows
it; the sim moves on for everyone at the first Confirm from any machine. A
machine whose players are all bots confirms by itself (`auto`). Sounds and
archer voices stay on the `EventGate` (first-seen) so they are not delayed;
the presentation's own cues (results, replay) come from the confirmed
timeline. The confirmed timeline lags the live one by the confirmation
delay (input delay + about half the round trip: typically 2–8 frames).

**Testing the game online** (`scripts/net/game-matrix.sh`, a full Versus
match per case between headless `towerfall-port --net-peer` processes with
bots over the dev server; `--net-harness` takes `--locals 4x3`,
`--spectators N`, `--mode lms|hh|tdm`, `--length instant|quick|standard|epic`,
`--tower NAME`, `--private`, `--delay/--jitter/--loss`, `--relay`,
`--leave M@F`, `--crash M@F`):

```sh
scripts/net/dev-server.sh &
export WARPZONE_SERVER=http://127.0.0.1:3012
towerfall-port --net-harness --locals 4x3 --mode lms --length quick --delay 30 --loss 3
scripts/net/game-matrix.sh
```

Results (2026-10-17, one laptop, Sacred Ground, bots; each a complete
Versus match from the tower card to acknowledged match results; checksum =
the first finished state, compared on every machine):

| Case | Machines × archers | Mode / length | Link | Result | Rounds | Rollbacks | Max depth |
|---|---|---|---|---|---|---|---|
| 2x1-lms-quick | 2 × 1 | LMS quick | none | PASS (frame 8230) | 4 | 2 | 2 |
| 3x1-hh-rtt80-loss3-code | 3 × 1, private room, joined by code | HH quick | 40 ms ± 10, 3 % loss | PASS (frame 11360) | 7 | 11549 | 8 |
| 4x1-tdm-rtt100-loss5 | 4 × 1, private room | TDM quick | 50 ms ± 15, 5 % loss | PASS (frame 9642) | 5 | 16947 | 8 |
| 4x3-lms-rtt60-loss3-spec | 4 × 3 (12 archers) + 1 spectator | LMS quick | 30 ms ± 10, 3 % loss | PASS, spectator agrees (frame 8624) | 6 | 17898 | 8 |
| 12x1-lms-rtt60-loss2 | 12 × 1 | LMS quick | 30 ms ± 10, 2 % loss | PASS (frame 4150) | 3 | 31472 | 8 |
| 4x1-lms-leave-crash | 4 × 1; m2 leaves @1500, m3 crashes @1000 | LMS quick | 30 ms ± 10, 2 % loss | PASS (2 left, 2 resumes each) | 3 | 2195 | 5 |

No desync in any case. `scripts/net/scene-pair.sh` runs the real online
scene (menus, lobby, rendering, audio path) in two headless processes: host
(2 bot archers, public room) and guest (1 bot, joined by room id), a full
Headhunters match, both `agreed=true`; `--mid N` stops the guest mid-match
(the host finishes after the drop, screenshots in `scratch/net/scene/`).
Unit tests (`cargo test -p towerfall-port online`): the adapter (exact
re-simulation from a clone, the leave mark), the event gate (first-seen
across re-simulation, confirmed-only match flow, loops cancelled/resumed on
rollback), and three machines plus a spectator playing the real sim over an
in-memory room with 30 ms / 3 % loss.

**Co-op online**: `OnlineSettings::kind` selects Quest or Trials, `coop_index` the
quest tower or trial, `hardcore`. `CoopData` loads the co-op content
(`loader::quest_content` / `trials_content`) with an FNV hash over the
quest levels and wave scripts (or the trial levels) and the archer data;
`new_coop_world` builds `quest::new_quest` / `trials::new_trial` from the
`MatchStart` (archers by slot, the match seed, extended co-op above 2
archers). The content is prepared exactly like the local Quest / Trials
scene (`coop_ui::scene`): the arenas' theme flags and the enemy art. Quest
takes 1-4 archers, Trials exactly one (the other machines spectate). The
sim's Quest runs online with rollback exactly like Versus. On screen
(`online/scene/coop_view.rs`) it uses the local co-op presentation's parts —
enemy art, debris, the Quest and Trials HUDs, the Versus effects and
particles — fed the confirmed timeline like `versus_ui` in Versus (HUD,
debris, particles never show a rewritten frame; the arena and actors come
from the live world), with the theme / boss / game-over music. The local
scene's pause menu actions (RESTART, MAP) do not exist online; Start opens
the local pause overlay as in Versus. The harness plays them with bots: `--kind quest --coop-index N
--until-wave W` (the run ends for the report once wave W has started; on
the Gauntlet towers 12 and 13, which have no waves, once the gauntlet's
enemies are all down — their progress is `QuestView::gauntlet`),
`--kind trials`, `--max-frames N` (a frame cap; the state at that frame is
compared, `capped=true`).

**Procedural towers** (Cataclysm). The host picks a fresh 9-digit layout
seed for every match (`OnlineSettings::procedural_seed`, also part of the
sim's `MatchSettings` and so of the checksum); every machine builds the
tower with `loader::content_for_match(seed, team mode)` and the level look
with the same key (`TowerGfx::look_procedural`). The data hash covers the
generated geometry (a `Debug` hash of every `LevelDef`), so a machine whose
generator produced something else is refused like one with other files.

**Dark World online.** GAME = DARK WORLD with the tower, the difficulty
(Normal / Hardcore / Legendary) and its power-ups and curses (the variant
screen shows only those); 1-4 archers. The content is built like the local
Dark World scene (`dark_world_content`, theme flags, enemy and boss art)
with the same data hash; the presentation (`online/scene/coop_view.rs`) uses
the local scene's boss art and effects (`BossFx`), the Dark World HUD and
its title / level / boss / game-over / complete screens, fed confirmed frames.
*Menus, all machines:* the game-over menu (CONTINUE (n) from level 2 on,
RESTART, MAP, QUIT) belongs to the host; guests see it read-only ("THE HOST
DECIDES"). CONTINUE and RESTART travel in the host's slot-0 input — the
reserved byte of `PlayerInput` (`ACTION_CONTINUE`, `ACTION_RESTART`) — and
`NetWorld::advance` applies them before the step (`darkworld::continue_level`
/ `restart`, the deterministic restart on the same settings) only in the
screen they belong to, so they happen at the same frame everywhere and a
predicted repeat is harmless. MAP ends the match for everyone (back to the
lobby), QUIT leaves the room. A completed run ends the match once its
complete sequence has played (a sim-state test), and everybody returns to the
lobby. Tests: `--kind darkworld --coop-index N --difficulty D`,
`--dw-assist N` (the local proof's assist, applied inside the step on every
machine), `--dw-level L` (start at level L), `--until-level L`, `--dw-menu
continue|restart` (the host answers the game-over menu).

**Soak** (`scripts/net/soak.sh [count] [seed] [parallel]`, i.e.
`towerfall-port --net-soak`): random full matches — random tower, mode
(Team Deathmatch with 2-4 teams), 2-12 archers on 2-4 machines, 0-4 random
variants (global or for single archers), a treasure variant half the time,
0-60 ms ± 0-20 ms, 0-5 % loss, public or private room — each one a
`--net-harness` run with a frame cap of 18000 (5 min of play), logs in
`scratch/net/soak/`, and a statistics summary.

**Drawing** goes through the shared match drawing path (`match_draw.rs`)
like the local scenes; online feeds it the live (predicted) world for the
level and actors and the confirmed timeline for everything with local
state (HUDs, results, particles, replay).

## Topology

Warpzone rooms are a **star**: a guest is connected only to the host
(directly after NAT punching, or through the Warpzone relay). GGRS is a full
mesh, so every GGRS datagram carries `dst`/`src` machine ids and the host
forwards guest-to-guest datagrams unchanged (it checks that `src` is the
sending guest). Consequences: guest-to-guest latency is two hops; the host's
upload carries `O(machines²)` streams (measured: 4 machines at 60 Hz, about
760 forwarded datagrams/s; fine for 12 on a home connection, but a weak host
uplink hurts everyone).

## Channels and framing

All encodings are bincode 1 with varint integers and a 64 KiB limit.

| Channel | Mode | Content |
|---|---|---|
| 1 `CH_GGRS` | unreliable | `kind:u8 dst:u8 src:u8 tag:u16le` + one bincode `ggrs::Message`; kind 1 = whole, kind 2 = fragment (`msg_id:u16le idx:u8 count:u8` + part; reassembled, incomplete ones expire after 2 s). `tag` = `match_id << 4 \| epoch`: datagrams of another match or epoch are dropped. |
| 2 `CH_LOBBY` | reliable, ordered | `LobbyMsg`, split into chunks of `more:u8` + ≤ 1099 bytes |

## Lobby and match protocol

| Message | Direction | Meaning |
|---|---|---|
| `Hello {protocol, game_version, slots, spectate, info}` | guest → host | first message; mismatching versions get `Reject` and a kick |
| `SetSlots`, `SetReady`, `SetSpectate`, `SetInfo` | guest → host | local players (`name`, `archer`, `skin`, team request), ready flag, watch matches, the game's machine info (`MachineView::info`; protocol 2) |
| `Welcome {machine}` | host → guest | our machine id (host = 0, then 1, 2, ... in join order, never reused in a room) |
| `State(LobbyState)` | host → all | phase, roster (machines, their slot requests, ready/waiting/spectate, `rejoined_from`), settings, and `preview`: the slot table the next match would get |
| `Start(MatchStart)` | host → players, spectators | `match_id`, `seed`, settings, the slot table, GGRS parameters, spectators |
| `Done {match_id, frame, checksum}` | guest → host | the sim ended at confirmed `frame` |
| `End {match_id, reports, desyncs}` | host → all | every playing machine reported (or dropped): drop the session; everyone compares the checksums |
| `Freeze` / `Frozen` / `Resume` / `Dropped` | both | machines dropping out mid-match, see below |
| `Desync {match_id, report}` | guest → host | GGRS found a checksum mismatch with another machine |
| `Abort {match_id, reason}` | host → all | match aborted (host action, desync policy) |
| `Spec {match_id, first, frames}` | host → spectators | final inputs of frames `first..`, see Spectators |
| `App {from, to, data}` | any → any via host | game data outside the sim (chat, menu cursors) |

**Settings** (`MatchSettings`, host-chosen, part of `MatchStart`):
`team_mode` (`Ffa` or `Teams(n)`), `archers` (how many distinct archers the
game offers), `mode` and `map` (labels for the room browser), and `game`:
opaque bytes the game defines (mode, tower, variants, rounds to win, ...).
Room metadata for the browser: `players` (archers in the next match),
`mode`, `map` — the descriptor's `display` keys. `list_rooms(has_space)`
returns them in `Event::RoomList`.

**Slot assignment** (`lobby::assign`, unit-tested): machines by id, each
machine's slots in its order, until 12 (the rest sit out;
`accepted_slots` says how many fit).
*Archers*: TowerFall gives each archer to one player. A request is honoured
if that archer is free, else the slot gets the next free archer after it
(wrapping), like the original's archer select skipping taken archers. With
more players than archers (12 players, fewer archers), once all are taken
requests are honoured as duplicates numbered `dup` = 1, 2, ... for the
renderer to tint. The skin (normal / alt / secret) is the player's choice.
*Teams*: free-for-all: team = slot. `Teams(n)`: a valid request is honoured,
otherwise the smallest team (lowest index on ties); the host can override
with `set_team`.

Everybody builds the same GGRS P2P session from `MatchStart`: players =
slots, own slots `Local`, others `Remote(machine)`. Machines that join while
a match runs are marked `waiting` and play from the next `start_match`.

**Ending a match**: when the sim decides the match is over at a *confirmed*
frame, the game calls `finish_match(frame, checksum)` and keeps calling
`advance` (others may still need our inputs) until `MatchEnded` arrives.
Leaving the GGRS session earlier would make the others see us drop.

## Leaving, dropping out, rejoining

**The rule.** A machine that leaves (or crashes, or goes silent for 2 s)
during a match: from an agreed frame `F` on, its slots get the zero input
with `InputStatus::Disconnected` on every machine. The sim (tfp-sim) then
does this, deterministically:

1. For the rest of the current round its archers are **inert**: no input,
   so they stand (or fall) where they are; they can be shot, stomped and
   killed as usual and those kills count. They do not block the round end:
   a round whose remaining *connected* archers are all on one side ends as
   it normally would.
2. At the **round end** they are **removed**: they do not spawn in later
   rounds, their points and crowns stay on the scoreboard (greyed), they
   cannot win the match.
3. If fewer than two sides with connected archers remain, the match ends
   after that round; results show the standings at that point.

Why: removing an archer mid-round would change a frame everybody has already
simulated and makes a sudden gap in a fight; an inert body for a few seconds
is what the original shows when a controller is unplugged, and round
boundaries are where TowerFall changes the cast anyway. `toy` implements
the same rule and the tests check it.

**How the net layer gets to `F`** (freeze/resume). GGRS 0.13 cannot drop a
peer reliably with more than two machines: every machine disconnects it at
the last frame *it* received, the views converge on the minimum by gossip,
and the resulting rollback can reach behind inputs GGRS already discarded or
behind its save window — it panics (reproduced in rounds-net with 4 machines
and 2 % loss). tfp-net therefore never lets GGRS disconnect anyone (GGRS
timeout 600 s) and does it itself:

1. The host notices a machine is gone: Warpzone `PeerLeft`, or no GGRS
   datagram from it for `disconnect_timeout_ms` (2 s; 10 s before the
   session is synchronised).
2. It sends `Freeze {epoch, gone}` to the others (and `Dropped` to the gone
   machine if it can still be reached). Every machine stops advancing and
   answers `Frozen {frame}`: the newest frame `r` whose inputs were all
   confirmed when it last simulated and whose successor state `r + 1` is
   saved (GGRS keeps `rollback_window + 1 = 25` states; we predict at most
   `max_prediction = 8` frames, so the frame is always still there).
3. When all answered (3 s timeout: the silent ones are dropped too, new
   epoch), the host sends `Resume {frame: F = min(r) + 1, gone}`.
4. Every machine loads its saved state of `F` and continues in a new GGRS
   session (new `tag`) in which the gone machines' slots are dummy local
   players of the host that always send zero. Frame numbers keep counting.
   The first `input_delay` frames of a new session have zero inputs for
   everybody (GGRS fills its input delay with defaults) — the same on every
   machine.

The game sees `MatchFrozen {gone}` (show "archer left, resyncing…"), then
`PlayersDropped {machines, slots, from_frame}`, and handles the `Load` that
comes with the next `advance`. A freeze costs about one round trip plus a
GGRS re-synchronisation (a few hundred ms).

**Rejoin.** A machine that left can join the room again (same code / room
id). It gets a new machine id (ids are never reused); the host recognises
the Warpzone account (`MachineView::rejoined_from`,
`MachineJoined { rejoined_from }`) so the lobby can show "Ann is back" and
the game can restore her archer choice. A running match is not re-entered
(there is no mid-match state transfer): the machine waits and plays from the
next match. **The host leaving ends the room** (Warpzone has no host
migration): guests get `RoomLeft` / `MatchAborted`.

## Spectators

A machine with no slots that called `set_spectate(true)` watches every match
that starts while it is in the room (`MatchStart::spectators`,
`NetSession::is_spectator`). It does not join the GGRS session. The host
records the inputs of every frame it simulates and streams frames that are
**final** to the spectators on the reliable channel (`Spec`, batches of ≥ 4
frames, ≤ 256 per message): frames at least `max_prediction + input_delay +
4` (14) frames behind the host's confirmed frame, so that a freeze — which
restarts at most that far before the host's confirmed frame — never rewrites
a frame a spectator already has. The spectator simulates them without
rollback (`advance` returns only `Advance` requests, inputs `Confirmed` or
`Disconnected`), catching up 2–4 frames per step when far behind. At the end
the host flushes everything confirmed before `End`, and the spectator emits
`MatchEnded` once it simulated the last frame. Cost: 12 × 4 bytes + 3 per
frame, ~3 KB/s per spectator; spectators are ~250 ms behind plus their
link. Spectators leaving do not affect the match.

## Desync detection and reporting

* GGRS desync detection is on (`desync_interval` = 10 frames): every save
  carries the game's checksum, peers exchange the checksums of confirmed
  frames, a mismatch gives `NetEvent::Desync {frame, local, remote, machine}`
  locally.
* Each machine reports the first desync per peer per match to the host
  (`LobbyMsg::Desync`); the host emits `DesyncReported(report)` and collects
  them (≤ 64 per match) into `End` → `MatchEnded { desyncs }` on everyone.
* `finish_match` checksums are compared by everyone (`MatchEnded { agreed }`).
* Policy (`NetConfig::desync_policy`): `Report` (default: keep playing,
  show a warning, save a replay/log) or `Abort` (the host aborts the match
  for everybody on the first report: `MatchAborted { reason: "desync ..." }`).

## Statistics

`stats()` → `NetStats`: current/confirmed frame, frames ahead, rollbacks
(Load requests), re-simulated frames, deepest rollback, prediction stalls,
desyncs, resumes, spectator buffer, transport counters (datagrams, bytes,
fragments, forwarded, dropped by the link simulator) and per machine: GGRS
ping / kbps / frames behind, Warpzone RTT of our link to it, `relayed`,
`via_host`. `last_match_stats()` keeps them after the match.

## API for the game crate

```rust
use tfp_net::{LaunchAction, MatchSettings, NetConfig, NetEvent, NetSession, SlotRequest, TfInput};
use tfp_net::warpzone_core::client::{Event, HostOptions};

let mut cfg = NetConfig::new(env!("CARGO_PKG_VERSION"));
cfg.auto_register = None;                     // or Some(name) after asking the player
let mut net: NetSession<SimState> = NetSession::new(cfg)?;   // SimState: NetGame<Input = TfInput>
match net.handle_launch(&std::env::args().collect::<Vec<_>>()) {
    LaunchAction::Joining { .. } => show_connecting(),
    LaunchAction::HostScreen => open_host_screen(), // then net.host(HostOptions {..})
    LaunchAction::None => {}
}
net.set_local_slots(vec![SlotRequest { name: "Ann".into(), archer: 2, skin: 0, team: None }]);
// host: net.set_settings(MatchSettings { team_mode, archers, mode, map, game: bytes })?; net.start_match(None)
// browse: net.list_rooms(true) -> NetEvent::Warpzone(Event::RoomList(..)); net.join(id, None, None)
// code:   net.join_code("K7QM3X")

// every frame
net.poll();
while let Some(e) = net.next_event() {
    match e {
        NetEvent::MatchStarted(ms) => sim = SimState::new(&ms),   // seed, settings, slot table
        NetEvent::LobbyChanged => redraw_lobby(net.lobby()),     // roster + preview
        NetEvent::Warpzone(e) => match *e { Event::RoomHosted { code, invite, .. } => .., _ => {} },
        NetEvent::MatchEnded { .. } | NetEvent::MatchAborted { .. } => back_to_lobby(),
        _ => {}
    }
}
// every 1/60 s step (run ~10 % slower while net.frames_ahead() > 0)
for &slot in net.local_slots() { net.add_local_input(slot, read_pad(slot))?; }
let stepped = net.step(&mut sim);            // or handle net.advance() requests yourself
if sim.over() && sim.frame() <= net.confirmed_frame() { net.finish_match(sim.frame(), sim.checksum())?; }
```

Other calls: `register`, `host`, `join`, `join_code`, `list_rooms`,
`answer_knock`, `kick`, `set_room_closed`, `leave`, `set_ready`,
`set_spectate`, `set_team`, `end_match`, `abort_match`, `send_app`,
`lobby()`, `me()`, `is_host()`, `is_spectator()`, `current_match()`,
`is_running()`, `current_frame()`, `confirmed_frame()`, `stats()`,
`last_match_stats()`, `with_warpzone(|c| ..)` (the raw
`warpzone_core::client::Client`). Everything is single-threaded: call it
from the game thread (the Warpzone client runs its own HTTP threads).

`NetSession::memory(MemHub::join(name), cfg)` gives the same API over an
in-process room (no server) — used by the tests and by `netsim memory`, and
usable for debugging several sessions in one process.

## Link simulation

Outgoing GGRS datagrams can be delayed, jittered and dropped by the machine
that originates them (the host's forwarding adds nothing, so a
guest-to-guest path gets one delay): `NetConfig::link_sim` or
`TFP_NET_DELAY_MS` (one way), `TFP_NET_JITTER_MS` (uniform ±),
`TFP_NET_LOSS` (percent). `TFP_NET_FORCE_RELAY=1` routes every Warpzone
connection through the server relay. The lobby channel is not affected.

## Testing

Everything below runs on one machine against our own server; `source env.sh`
first (the scripts do it themselves, also from a git worktree:
`TFP_SDL2_LIB_DIR` and the tools come from the main checkout).

| What | Command | Proves |
|---|---|---|
| tfp-net unit + in-process matches | `cargo test -p tfp-net` | framing, lobby rules, toy matches over memory rooms: mixed locals, 12 × 1, loss, leave, crashes, spectators, desync detection, abort policy, rejoin, machine info |
| game online unit tests | `cargo test -p towerfall-port online` | the adapter (exact re-simulation, the leave mark, per-player variants, wide-tower rule, procedural layouts), the event gate, the runner (3 machines + spectator on the real sim, confirmed timeline), co-op worlds, Dark World host actions, soak case generator |
| dev server | `scripts/net/dev-server.sh` [`--build` / `--reset` / `--check`] | local `warpzone-server` on http 3012 / udp 3490 (`TFP_WZ_PORT`, `TFP_WZ_UDP_PORT`); `--check` = `check-games packaging/warpzone` |
| netsim (toy game) | `cargo run -p tfp-net --example netsim -- run --locals 4x3 [--delay MS --jitter MS --loss PCT --relay --spectators N --leave M@F --crash M@F --desync M@F --abort-on-desync]`, `-- memory ...` (one process, no server) | tfp-net end to end, one process per machine |
| netsim matrix | `scripts/net/netsim-matrix.sh [frames] [filter]` | 12 toy cases (clean, latency/loss, relay, leave/crash, desync) |
| the real game, headless | `towerfall-port --net-harness --locals 4x3 [--spectators N --mode lms\|hh\|tdm --length instant\|quick\|standard\|epic --teams N --tower NAME --variants SPEC --private --no-wide --procedural-seed N --kind quest\|trials\|darkworld --coop-index N --difficulty D --hardcore --until-wave W --until-level L --dw-assist N --dw-level L --dw-menu continue\|restart --max-frames N --delay/--jitter/--loss --relay --leave M@F --crash M@F]` | full matches with bots, one `--net-peer` process per machine; `SUMMARY PASS` = every machine reported the same checksum |
| game matrix | `scripts/net/game-matrix.sh [filter]` | 2 × 1, 3 × 1 by code, TDM at 100 ms / 5 %, 4 × 3 + spectator, 12 × 1, Cataclysm, Dark World level 1, Dark World continue, leave + crash |
| the online scene | `scripts/net/scene-pair.sh [--mid N]` | menus, lobby, the match with its presentation in two headless game processes; screenshots in `scratch/net/scene/` |
| soak | `scripts/net/soak.sh [count] [seed] [parallel]` | random matches (towers incl. Cataclysm, modes, 2-12 archers on 2-4 machines, variants, treasure, latency/loss, private rooms, about one in five Quest or Dark World); statistics and failures |

Wrap the long ones in the machine's performance-mode helper when it has one.

`dev-server.sh` builds `warpzone-server` and `wz` from `ext/warpzone` into
`scratch/net/wz-target/` (ext/ is only read), writes its config and database
to `scratch/net/wz-dev/` (registration and login limits raised: tests create
a dozen identities from one IP) and serves `packaging/warpzone/` as its
games directory (validated with `check-games` on every start). Default
ports 3012/3490 avoid Warpzone's defaults (2998/3478) and rounds-port's,
which other projects use.

`netsim` plays `tfp_net::toy`: a 320×240 arena with up to 12 dots that run,
jump, dodge-dash and shoot bouncing arrows, with screen wrap and rounds every
600 frames; integer physics, a PRNG in the state, bots that hold an input
for 4–40 frames. `run` uses one OS process per machine, each with its own
`WARPZONE_HOME` under `scratch/net/homes/`. Machine 0 hosts a public room;
odd machines join by code, even ones through `--warpzone-join <room>` (the
launcher path); the last `--spectators N` machines watch. At the target
frame every player reports its checksum through `finish_match`; the
orchestrator prints `SUMMARY PASS` when all remaining machines (spectators
included) report the same one and no desync was seen. `--leave M@F` leaves
gracefully, `--crash M@F` exits without a word, `--desync M@F` corrupts
machine M's state at frame F (PASS = detected and reported, or with
`--abort-on-desync` aborted everywhere).

### Results

2026-10-17, `scripts/net/netsim-matrix.sh 3600`: 3600 frames (60 s) per case,
one OS process per machine against the local dev server, one laptop. Delays
are one way per originating machine, so a guest-to-guest round trip is about
twice the value plus the host hop. Rollbacks are summed over machines.

| Case | Machines × slots | Link | Result | Rollbacks | Max depth |
|---|---|---|---|---|---|
| 4x3-clean | 4 × 3 | none | PASS | 19 | 6 |
| 12x1-clean | 12 × 1 | none | PASS | 147 | 7 |
| mixed-2-4-6 | 2 + 4 + 6 | none | PASS | 0 | 0 |
| 4x3-rtt50-loss5 | 4 × 3 | 25 ms ± 10, 5 % loss | PASS | 1283 | 7 |
| 4x3-rtt150-loss5 | 4 × 3 | 75 ms ± 25, 5 % loss | PASS | 4650 | 8 |
| 12x1-rtt100-loss5 | 12 × 1 | 50 ms ± 20, 5 % loss | PASS | 14072 | 7 |
| 4x3-relay-spec | 4 × 3 + 1 spectator | forced relay (all links relayed) | PASS (spectator agrees) | 3 | 1 |
| 6x2-relay-rtt100-loss5 | 6 × 2 | forced relay + 50 ms ± 15, 5 % loss | PASS | 6761 | 7 |
| 4x1-leave-crash | 4 × 1 | 30 ms ± 10, 2 % loss; m2 leaves @900, m3 crashes @1500 | PASS (2 left, 2 resumes) | 192 | 6 |
| 6x2-rtt100-leave-crash-spec | 6 × 2 + 1 spectator | 50 ms ± 15, 5 % loss; leave @800, crash @1600 | PASS (4 + spectator left, 2 resumes) | 3607 | 8 |
| 3x1-desync | 3 × 1 | 20 ms ± 5, 2 % loss; m1 corrupts its state @600 | PASS: detected (1179 GGRS events, 4 pair reports to the host), final checksums disagree | 97 | 6 |
| 3x1-desync-abort | 3 × 1 | none; m2 corrupts @600, abort policy | PASS: detected, match aborted on all 3 | 0 | 0 |

No desync in any case without injection; the deepest rollback is the
prediction limit (8). `cargo test -p tfp-net` additionally runs in-process
matches: 2+1+3, 4 × 3 with 40 ms/5 % loss, 12 × 1, 1 × 12, a graceful leave,
two crashes plus a leave in a 5-machine match, two spectators, a spectator
through a drop, desync detection + reporting, the abort policy, and rejoin
recognition.

**Soak** (`scripts/net/soak.sh`, 2026-10-17, three runs of random full
matches with the real game, 2 matches in parallel on one laptop):

| Run | Matches | PASS | Archers | Wide | Co-op | Rollbacks | Deepest | Desyncs |
|---|---|---|---|---|---|---|---|---|
| seed 1 | 32 | 31 (+1 too long; PASS when capped) | 239 | 23 | – | 121 576 | 8 | 0 |
| seed 2 | 32 | 32 (4 capped) | 249 | 26 | – | 231 514 | 8 | 0 |
| seed 3 | 40 | 39 (4 capped) | 242 | 20 | 3 Quest, 6 Dark World | 127 152 | 8 | 0 |

The one failure of seed 3 (a 4-archer Quest on 3 machines) was a machine
dropped by the host for 2 s of silence while the next soak match started its
processes; the match itself was in step (identical state on every machine
up to then), and the same case passed twice when re-run alone. 

## Known limits

* **Rooms are a star** (Warpzone): GGRS traffic between guests is forwarded
  by the host (two hops, the host's upload carries `O(machines²)` streams).
  A mesh option in `warpzone-core` would halve guest-to-guest latency.
* **No host migration**: the host leaving closes the room and ends the
  match. The host always plays (it coordinates drops, plays dropped slots'
  dummies, feeds spectators); it cannot spectate.
* **GGRS 0.13 drop handling** is worked around by freeze/resume (a few
  hundred ms of pause for everyone when a machine drops).
* **No mid-match join or rejoin**: `NetGame` needs only `Clone`, there is no
  state transfer; latecomers and rejoiners play from the next match.
* **Spectators** are host-fed (~3 KB/s each, ~250 ms behind) and must be in
  the room when the match starts.
* **No pause online** (local overlay); the local co-op scenes' RESTART /
  MAP pause items do not exist online.
* **Content must match exactly**: every machine needs the same game data for
  the chosen tower / mode (the hash refuses the match otherwise); the wide
  towers need the 8-player build on every machine.
* **Co-op limits**: Quest and Dark World online take up to 4 archers,
  Trials one; Quest has no host menu yet (a finished run returns to the
  lobby); co-op records (Dark World times, Trials medals) are not written
  for online runs.
* **Bots** (tests only): Headhunters with two archers left and Last Man
  Standing with 12 can run very long (self-kills, draws); the soak caps
  matches at 18000 frames and compares the state there.
* **Release**: `scripts/package.sh` produces the asset names the descriptor
  expects (`towerfall-port-*-linux-x86_64.tar.gz`, `...-windows-x86_64.zip`,
  `SHA256SUMS`, the binary in a `towerfall-port-<version>-<platform>/`
  folder); nothing else is needed for Warpzone (warpzone-core is compiled
  in). The launcher's `WARPZONE_DATA_TOWERFALL` is read by
  `tfp_assets::Installs::discover` (either build).
