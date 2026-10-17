//! `sim-rollback-fuzz`: rollback fuzzing of every mode.
//!
//! Inputs: the shared bot (`tfp_sim::bot::Bot`) on even slots, random held
//! buttons on odd slots.
//!
//! Each scenario runs twice in lock step from the same start: a *straight*
//! world that only ever steps with the real inputs, and a *rollback* world
//! that keeps the last 11 states and, at random frames, does what a rollback
//! session does: restore the state of `d` frames ago (1..=10), run those
//! frames again with made-up inputs (the misprediction), restore again and
//! re-run them with the real inputs. After every restore the cosmetic RNG
//! is reseeded at random (it is not part of the checksum, so gameplay must
//! not read it). The two checksums must agree on every frame; the first
//! difference is reported with the parts of the state that differ.
//!
//! Matches restart (next seed / level / tower) when they finish and at
//! least every 20 000 frames, so a long run covers many rounds and levels.
//!
//! Usage: `sim-rollback-fuzz [frames per scenario] [seed] [scenario filter]`
//! (defaults 100000, 1, all). Exit code 1 on any divergence.

use tfp_assets::install::{GameInstall, Installs};
use tfp_sim::content::Content;
use tfp_sim::rng::Rng;
use tfp_sim::variants::Variant;
use tfp_sim::versus::{self, MatchSettings, Mode, Phase};
use tfp_sim::{MatchSetup, PlayerInput, PlayerSetup, World};

const MAX_DEPTH: usize = 10;
const RESTART_EVERY: usize = 20_000;

/// Random-but-plausible input source (holds directions and buttons).
struct Bot {
    rng: Rng,
    left: i32,
    cur: PlayerInput,
}

impl Bot {
    fn new(seed: i32) -> Bot {
        Bot {
            rng: Rng::new(seed),
            left: 0,
            cur: PlayerInput::default(),
        }
    }
    fn next(&mut self) -> PlayerInput {
        if self.left <= 0 {
            self.left = self.rng.range_i(2, 25);
            let x = self.rng.range_i(-1, 3) as i8;
            let y = if self.rng.chance(0.25) {
                self.rng.range_i(-1, 3) as i8
            } else {
                0
            };
            let mut b = 0;
            if self.rng.chance(0.4) {
                b |= PlayerInput::JUMP;
            }
            if self.rng.chance(0.35) {
                b |= PlayerInput::SHOOT;
            }
            if self.rng.chance(0.15) {
                b |= PlayerInput::DODGE;
            }
            if self.rng.chance(0.08) {
                b |= PlayerInput::ALT_SHOOT;
            }
            if self.rng.chance(0.02) {
                b |= PlayerInput::START;
            }
            self.cur = PlayerInput::dpad(x, y, b);
        }
        self.left -= 1;
        let mut i = self.cur;
        if self.rng.chance(0.06) {
            i.buttons ^= PlayerInput::DODGE;
        }
        if self.rng.chance(0.08) {
            i.buttons ^= PlayerInput::JUMP;
        }
        if self.rng.chance(0.05) {
            i.buttons ^= PlayerInput::SHOOT;
        }
        i
    }
}

struct Scenario<'a> {
    name: String,
    content: &'a Content,
    players: usize,
    make: Box<dyn Fn(u32) -> World + 'a>,
}

/// The match / run has nothing more to simulate.
fn finished(w: &World) -> bool {
    if let Some(m) = &w.versus {
        return m.phase == Phase::Finished;
    }
    if let Some(q) = &w.quest {
        return q.phase == tfp_sim::quest::QuestPhase::Finished;
    }
    if let Some(t) = &w.trials {
        return t.phase == tfp_sim::trials::TrialsPhase::Finished;
    }
    if let Some(d) = &w.dark_world {
        use tfp_sim::darkworld::DwPhase;
        return (d.phase == DwPhase::GameOver && d.menu_ready) || d.phase == DwPhase::Complete;
    }
    false
}

fn report(name: &str, f: usize, a: &World, b: &World) {
    println!("DIVERGENCE {name} at frame {f}");
    let pa = a.checksum_parts();
    let pb = b.checksum_parts();
    let mut shown = 0;
    for (x, y) in pa.iter().zip(pb.iter()) {
        if x != y && shown < 12 {
            println!(
                "  straight {} {:016x} | rollback {} {:016x}",
                x.0, x.1, y.0, y.1
            );
            shown += 1;
        }
    }
    if pa.len() != pb.len() {
        println!("  entity counts differ: {} vs {}", pa.len(), pb.len());
    }
}

/// Run one scenario; returns whether it stayed in sync.
fn fuzz(s: &Scenario, frames: usize, seed: u32) -> bool {
    let c = s.content;
    let mut run = 0u32;
    let mut straight = (s.make)(seed);
    let mut rb = straight.clone();
    // Real inputs: the shared bot on even slots (real fights), random
    // held buttons on odd slots (odd inputs).
    let mut bots: Vec<Bot> = (0..s.players)
        .map(|i| Bot::new(seed as i32 * 7919 + i as i32 * 97))
        .collect();
    let mut shared: Vec<tfp_sim::bot::Bot> = (0..s.players)
        .map(|i| tfp_sim::bot::Bot::new(seed as i32 * 31 + i as i32))
        .collect();
    let mut chaos = Rng::new(seed as i32 ^ 0x2b2b);
    let mut wrong = Bot::new(seed as i32 ^ 0x7777);
    // Ring of (state before frame k, real inputs of frame k).
    let mut history: Vec<(World, Vec<PlayerInput>)> = Vec::with_capacity(MAX_DEPTH + 1);
    let (mut rollbacks, mut resimmed) = (0u64, 0u64);
    let mut since = 0usize;
    for f in 0..frames {
        if since >= RESTART_EVERY || finished(&straight) {
            run += 1;
            straight = (s.make)(seed.wrapping_mul(31).wrapping_add(run));
            rb = straight.clone();
            history.clear();
            since = 0;
        }
        since += 1;
        let inputs: Vec<PlayerInput> = (0..s.players)
            .map(|k| {
                if k % 2 == 0 {
                    shared[k].input(&straight, c, k as u8)
                } else {
                    bots[k].next()
                }
            })
            .collect();
        straight.step(c, &inputs);
        history.push((rb.clone(), inputs.clone()));
        if history.len() > MAX_DEPTH + 1 {
            history.remove(0);
        }
        rb.step(c, &inputs);
        if chaos.chance(0.3) {
            let d = chaos.range_i(1, MAX_DEPTH as i32) as usize;
            let d = d.min(history.len());
            let base = history.len() - d;
            // Misprediction: the same frames with other inputs.
            rb = history[base].0.clone();
            for _ in 0..d {
                let guess: Vec<PlayerInput> = (0..s.players).map(|_| wrong.next()).collect();
                rb.step(c, &guess);
            }
            // The real inputs arrive: restore and re-simulate.
            rb = history[base].0.clone();
            // (`NO_RESEED=1` turns this off to tell a cosmetic-RNG leak from
            // a rollback bug.)
            if std::env::var_os("NO_RESEED").is_none() {
                rb.cosmetic_rng = Rng::new(chaos.next_i32());
            }
            for (state, inp) in history.iter_mut().skip(base) {
                *state = rb.clone();
                rb.step(c, inp);
            }
            rollbacks += 1;
            resimmed += d as u64;
        }
        if straight.checksum128() != rb.checksum128() {
            report(&s.name, f, &straight, &rb);
            return false;
        }
    }
    println!(
        "ok {:<22} {frames} frames, {} runs, {rollbacks} rollbacks ({resimmed} frames re-simulated)",
        s.name,
        run + 1
    );
    true
}

const ARENA: [&str; 24] = [
    "################################",
    "#..............................#",
    "#...P......................P...#",
    "#######....############....#####",
    "#..............................#",
    "#..............................#",
    "......P................P........",
    "#......####..........####......#",
    "##########............##########",
    "#..............................#",
    "#..............................#",
    "#.........#####.......#####....#",
    "#..............................#",
    "#..............................#",
    "#....-----..........-----......#",
    "#..............................#",
    "#..............................#",
    "#......####...........####.....#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "################################",
    "################################",
];

/// Variant sets cycled through by the "versus-variants" scenario.
fn variant_mix(k: u32) -> &'static [Variant] {
    use Variant::*;
    const MIXES: [&[Variant]; 6] = [
        &[
            ReturnAsGhosts,
            GunnStyle,
            ExplodingCorpses,
            MaxTreasure,
            IgnoreTowerItemSet,
        ],
        &[
            TriggerCorpses,
            CorpsesDropArrows,
            DarkPortals,
            SlipperyFloors,
            StartWithTriggerArrows,
        ],
        &[
            StartWithPrismArrows,
            RegeneratingArrows,
            RegeneratingShields,
            AlwaysLava,
            BigHeads,
        ],
        &[
            TreasureDraft,
            BottomlessTreasure,
            BombChests,
            StealthArchers,
            DoubleJumping,
        ],
        &[
            StartWithBrambleArrows,
            SuperSeekingArrows,
            AlwaysScrolling,
            OffsetWorld,
            AlwaysDark,
        ],
        &[
            StartWithDrillArrows,
            StartWithBoltArrows,
            SlowTime,
            SuddenDeath,
            CursedBows,
        ],
    ];
    MIXES[k as usize % MIXES.len()]
}

#[allow(clippy::too_many_arguments)]
fn versus_settings(
    c: &Content,
    mode: Mode,
    players: usize,
    seed: u32,
    treasure: bool,
    mix: Option<u32>,
) -> MatchSettings {
    let n = c.levels.len();
    let first = (seed as usize * 7) % n;
    let mut s = MatchSettings::new(mode, players, vec![first, (first + 1) % n, (first + 2) % n]);
    s.tower_card = seed.is_multiple_of(3);
    s.seed = seed as i32;
    let mask = s.joined_mask();
    if treasure {
        for v in [Variant::MaxTreasure, Variant::IgnoreTowerItemSet] {
            s.variants.set_all(v, true, mask);
        }
    }
    if let Some(k) = mix {
        for &v in variant_mix(k) {
            s.variants.set_all(v, true, mask);
        }
    }
    if mode == Mode::TeamDeathmatch {
        s.teams = if players > 4 { 3 } else { 2 };
        let t = s.teams as usize;
        for (i, slot) in s.slots.iter_mut().enumerate() {
            if let Some(sl) = slot.as_mut() {
                sl.team = (i % t) as u8;
            }
        }
        s.variants.set_all(Variant::TeamRevive, true, mask);
    }
    s
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let frames: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let seed: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let filter = args.get(3).cloned().unwrap_or_default();

    let ascii = Content::from_ascii(&ARENA);
    let installs = Installs::discover();
    let load = |inst: Option<&GameInstall>| {
        inst.and_then(|i| {
            let levels = tfp_sim::loader::versus_levels(i).ok()?;
            tfp_sim::loader::content_from_install(i, &levels).ok()
        })
    };
    let steam = load(installs.steam.as_ref());
    let eight = load(installs.eight_player.as_ref());
    let quest = installs
        .steam
        .as_ref()
        .and_then(|i| tfp_sim::loader::quest_content(i).ok())
        .filter(|c| !c.quest_scripts.is_empty());
    let trials = installs
        .steam
        .as_ref()
        .and_then(|i| tfp_sim::loader::trials_content(i).ok())
        .filter(|(_, s)| !s.is_empty());
    let dw = installs
        .steam
        .as_ref()
        .and_then(|i| tfp_sim::loader::dark_world_content(i).ok())
        .filter(|c| c.dark_world.len() >= 4);

    let mut sc: Vec<Scenario> = vec![];
    sc.push(Scenario {
        name: "ascii-lms".into(),
        content: &ascii,
        players: 4,
        make: Box::new(|s| {
            let mut m = MatchSettings::new(Mode::LastManStanding, 4, vec![0]);
            m.tower_card = false;
            m.seed = s as i32;
            versus::new_match(&ascii, m)
        }),
    });
    sc.push(Scenario {
        name: "ascii-sandbox".into(),
        content: &ascii,
        players: 4,
        make: Box::new(|s| {
            let mut m = MatchSetup {
                seed: s as i32,
                respawn_frames: Some(30),
                ..Default::default()
            };
            for i in 0..4 {
                m.players[i] = Some(PlayerSetup {
                    archer: i as u16,
                    arrows: 3,
                    ..Default::default()
                });
            }
            World::new(&ascii, &m)
        }),
    });
    if let Some(c) = steam.as_ref() {
        for (name, mode, n, treasure) in [
            ("versus-lms", Mode::LastManStanding, 4, false),
            ("versus-hh-treasure", Mode::Headhunters, 4, true),
            ("versus-tdm", Mode::TeamDeathmatch, 4, true),
        ] {
            sc.push(Scenario {
                name: name.into(),
                content: c,
                players: n,
                make: Box::new(move |s| {
                    versus::new_match(c, versus_settings(c, mode, n, s, treasure, None))
                }),
            });
        }
        sc.push(Scenario {
            name: "versus-variants".into(),
            content: c,
            players: 4,
            make: Box::new(move |s| {
                let mode = [
                    Mode::LastManStanding,
                    Mode::Headhunters,
                    Mode::TeamDeathmatch,
                ][s as usize % 3];
                versus::new_match(c, versus_settings(c, mode, 4, s, false, Some(s)))
            }),
        });
    }
    if let Some(c) = eight.as_ref() {
        sc.push(Scenario {
            name: "tdm12".into(),
            content: c,
            players: 12,
            make: Box::new(move |s| {
                versus::new_match(
                    c,
                    versus_settings(c, Mode::TeamDeathmatch, 12, s, true, None),
                )
            }),
        });
        sc.push(Scenario {
            name: "lms8-variants".into(),
            content: c,
            players: 8,
            make: Box::new(move |s| {
                versus::new_match(
                    c,
                    versus_settings(c, Mode::LastManStanding, 8, s, true, Some(s + 1)),
                )
            }),
        });
    }
    if let Some(c) = quest.as_ref() {
        sc.push(Scenario {
            name: "quest".into(),
            content: c,
            players: 2,
            make: Box::new(move |s| {
                let n = c.quest_scripts.len();
                let k = s as usize % n;
                let mut q = tfp_sim::quest::QuestSettings::new(k, k, 2);
                q.hardcore = s % 2 == 1;
                q.seed = s as i32;
                tfp_sim::quest::new_quest(c, q)
            }),
        });
    }
    if let Some((c, settings)) = trials.as_ref() {
        sc.push(Scenario {
            name: "trials".into(),
            content: c,
            players: 1,
            make: Box::new(move |s| {
                let mut t = settings[s as usize % settings.len()];
                t.start_screen = s % 2 == 0;
                tfp_sim::trials::new_trial(c, t)
            }),
        });
    }
    if let Some(c) = dw.as_ref() {
        use tfp_sim::darkworld::{self, DwSettings};
        sc.push(Scenario {
            name: "darkworld".into(),
            content: c,
            players: 2,
            make: Box::new(move |s| {
                let tower = s as usize % c.dark_world.len();
                let mut d = DwSettings::new(tower, (s % 3) as u8, 2);
                d.seed = s as i32;
                let mut w = darkworld::new_dark_world(c, d);
                // Start somewhere along the tower (every level, bosses included).
                let n = w.dark_world.as_ref().unwrap().levels(c).len();
                darkworld::jump_to_level(&mut w, c, (s as usize / 3) % n);
                w
            }),
        });
        sc.push(Scenario {
            name: "darkworld-bosses".into(),
            content: c,
            players: 2,
            make: Box::new(move |s| {
                let tower = s as usize % c.dark_world.len();
                let mut d = DwSettings::new(tower, darkworld::LEGENDARY.min((s % 3) as u8), 2);
                d.seed = s as i32 + 100;
                let mut w = darkworld::new_dark_world(c, d);
                let last = w.dark_world.as_ref().unwrap().levels(c).len() - 1;
                darkworld::jump_to_level(&mut w, c, last);
                w
            }),
        });
    }

    let mut ok = true;
    for s in sc.iter().filter(|s| s.name.contains(&filter)) {
        ok &= fuzz(s, frames, seed);
    }
    if !ok {
        std::process::exit(1);
    }
}
