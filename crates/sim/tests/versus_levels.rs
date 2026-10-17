//! Every Versus level of both installs (Steam 320 px, 8-player 420 px):
//! all `.oel` tags are supported, the level runs 600 frames of bot input,
//! and rollback re-simulation reproduces every checksum. Skipped (with a
//! note) when no install is found.

use std::collections::BTreeMap;
use tfp_assets::install::{GameInstall, Installs};
use tfp_sim::entity::Kind;
use tfp_sim::rng::Rng;
use tfp_sim::{Content, MatchSetup, PlayerInput, PlayerSetup, World};

/// Misprediction inputs (random; the real inputs come from the shared
/// bot `tfp_sim::bot::Bot`).
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
            self.left = self.rng.range_i(3, 25);
            let x = self.rng.range_i(-1, 3) as i8;
            let y = if self.rng.chance(0.2) {
                self.rng.range_i(-1, 3) as i8
            } else {
                0
            };
            let mut b = 0;
            if self.rng.chance(0.4) {
                b |= PlayerInput::JUMP;
            }
            if self.rng.chance(0.3) {
                b |= PlayerInput::SHOOT;
            }
            if self.rng.chance(0.15) {
                b |= PlayerInput::DODGE;
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
        i
    }
}

fn installs() -> Vec<GameInstall> {
    let all = Installs::discover();
    let v: Vec<GameInstall> = all.iter().cloned().collect();
    if v.is_empty() {
        eprintln!("no TowerFall install found: skipped");
    }
    v
}

fn load(inst: &GameInstall) -> Content {
    let levels = tfp_sim::loader::versus_levels(inst).expect("tower index");
    tfp_sim::loader::content_from_install(inst, &levels).expect("content")
}

fn setup(c: &Content, level: usize, players: usize) -> MatchSetup {
    let mut s = MatchSetup {
        level,
        seed: c.level(level).load_seed,
        ..Default::default()
    };
    for i in 0..players {
        s.players[i] = Some(PlayerSetup {
            archer: (i % c.archers.len().max(1)) as u16,
            ..Default::default()
        });
    }
    // Keep the arena busy: dead archers come back.
    s.respawn_frames = Some(60);
    s
}

fn tower_of(name: &str) -> &str {
    name.split('/').next().unwrap_or(name)
}

#[test]
fn every_versus_tag_is_supported() {
    for inst in installs() {
        let c = load(&inst);
        let mut per_tower: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for l in &c.levels {
            let e = per_tower.entry(tower_of(&l.name).to_owned()).or_default();
            for u in &l.tower.unsupported {
                if !e.contains(u) {
                    e.push(u.clone());
                }
            }
        }
        for (t, u) in &per_tower {
            println!("{:?} {t}: unsupported {u:?}", inst.kind);
        }
        assert!(
            per_tower.values().all(|u| u.is_empty()),
            "unsupported entity types: {per_tower:?}"
        );
        // And every level actually places tower entities where expected.
        let placed: usize = c.levels.iter().map(|l| l.tower.placements.len()).sum();
        assert!(placed > 1000, "{placed} placements");
    }
}

#[test]
fn every_versus_level_runs_600_frames_of_bots() {
    for inst in installs() {
        let c = load(&inst);
        let players = if inst.level_width > 320 { 8 } else { 4 };
        for li in 0..c.levels.len() {
            let mut w = World::new(&c, &setup(&c, li, players));
            let mut bots: Vec<tfp_sim::bot::Bot> = (0..players)
                .map(|i| tfp_sim::bot::Bot::new(li as i32 * 31 + i as i32))
                .collect();
            for _ in 0..600 {
                let inputs: Vec<PlayerInput> = bots
                    .iter_mut()
                    .enumerate()
                    .map(|(k, b)| b.input(&w, &c, k as u8))
                    .collect();
                w.step(&c, &inputs);
            }
            let towers = w
                .entities()
                .filter(|&e| matches!(w.kind(e), Some(Kind::Tower(_))))
                .count();
            let _ = tfp_sim::View::build(&w, &c, Default::default());
            assert!(
                towers > 0 || c.level(li).tower.placements.is_empty(),
                "{}: no tower entities",
                c.level(li).name
            );
        }
    }
}

/// Rollback: from several frames, simulate 20 mispredicted frames, restore
/// the saved clone and re-simulate the real inputs; every checksum must
/// match an uninterrupted run. One level per tower (the busiest).
#[test]
fn rollback_resimulation_matches_on_every_tower() {
    for inst in installs() {
        let c = load(&inst);
        let mut best: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for (i, l) in c.levels.iter().enumerate() {
            let n = l.tower.placements.len();
            let e = best.entry(tower_of(&l.name)).or_insert((i, n));
            if n > e.1 {
                *e = (i, n);
            }
        }
        for (&tower, &(li, _)) in &best {
            let s = setup(&c, li, 4);
            let frames = 500;
            // The real inputs: shared bots on the uninterrupted run.
            let mut bots: Vec<tfp_sim::bot::Bot> =
                (0..4).map(|i| tfp_sim::bot::Bot::new(1000 + i)).collect();
            let mut inputs: Vec<Vec<PlayerInput>> = vec![];
            let mut a = World::new(&c, &s);
            let mut sums = vec![];
            for _ in 0..frames {
                let inp: Vec<PlayerInput> = bots
                    .iter_mut()
                    .enumerate()
                    .map(|(k, b)| b.input(&a, &c, k as u8))
                    .collect();
                a.step(&c, &inp);
                sums.push(a.checksum());
                inputs.push(inp);
            }
            let mut b = World::new(&c, &s);
            let mut wrong = Bot::new(77);
            let mut f = 0;
            while f < frames {
                if f % 50 == 25 {
                    let saved = b.clone();
                    for _ in 0..20 {
                        let guess: Vec<PlayerInput> = (0..4).map(|_| wrong.next()).collect();
                        b.step(&c, &guess);
                    }
                    b = saved;
                }
                b.step(&c, &inputs[f]);
                assert_eq!(b.checksum(), sums[f], "{tower}: desync at frame {f}");
                f += 1;
            }
        }
    }
}
