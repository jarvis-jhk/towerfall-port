//! Every Versus level of both installs as a one-level Headhunters match
//! with all treasure on (Max Treasure + Ignore Tower Item Set: every
//! pickup and orb can drop): 3000 bot frames each (1500 in debug builds)
//! without a panic, and a rollback re-simulation on a sample of levels.
//! Skipped without an install. The full run is meant for release builds
//! (`cargo test --release -p tfp-sim --test versus_treasure_levels`).

use tfp_assets::install::{GameInstall, Installs};
use tfp_sim::rng::Rng;
use tfp_sim::variants::Variant;
use tfp_sim::versus::{self, MatchSettings, Mode};
use tfp_sim::{Content, PlayerInput, World};

const FRAMES: usize = if cfg!(debug_assertions) { 1500 } else { 3000 };
/// Snapshot frame of the rollback sample.
const SNAP: usize = FRAMES * 2 / 5;

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
            if self.rng.chance(0.05) {
                b |= PlayerInput::ALT_SHOOT;
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
    let v: Vec<GameInstall> = Installs::discover().iter().cloned().collect();
    if v.is_empty() {
        eprintln!("no TowerFall install found: skipped");
    }
    v
}

fn settings(level: usize, players: usize, seed: i32) -> MatchSettings {
    let mut s = MatchSettings::new(Mode::Headhunters, players, vec![level]);
    s.tower_card = false;
    s.seed = seed;
    let mask = s.joined_mask();
    for v in [Variant::MaxTreasure, Variant::IgnoreTowerItemSet] {
        s.variants.set_all(v, true, mask);
    }
    s
}

#[test]
fn every_versus_level_with_all_treasure() {
    for inst in installs() {
        let levels = tfp_sim::loader::versus_levels(&inst).expect("tower index");
        let c: Content = tfp_sim::loader::content_from_install(&inst, &levels).expect("content");
        let players = if inst.level_width > 320 { 8 } else { 4 };
        let mut chests = 0usize;
        let mut pickups = 0usize;
        for li in 0..c.levels.len() {
            let seed = li as i32 + 1;
            let mut bots: Vec<tfp_sim::bot::Bot> = (0..players)
                .map(|i| tfp_sim::bot::Bot::new(seed * 13 + i as i32))
                .collect();
            let mut input: Vec<Vec<PlayerInput>> = Vec::with_capacity(FRAMES);
            let mut w = versus::new_match(&c, settings(li, players, seed));
            let sample = li % 7 == 0;
            let mut sums = vec![];
            let mut snap: Option<World> = None;
            for f in 0..FRAMES {
                if sample && f == SNAP {
                    snap = Some(w.clone());
                }
                let i: Vec<PlayerInput> = bots
                    .iter_mut()
                    .enumerate()
                    .map(|(k, b)| b.input(&w, &c, k as u8))
                    .collect();
                w.step(&c, &i);
                input.push(i);
                if sample && f >= SNAP {
                    sums.push(w.checksum());
                }
                if f % 100 == 0 {
                    chests += w.tagged(tfp_sim::entity::Tag::TreasureChest).len();
                    pickups += w
                        .entities()
                        .filter(|&e| matches!(w.kind(e), Some(tfp_sim::entity::Kind::Pickup(_))))
                        .count();
                }
            }
            let _ = tfp_sim::View::build(&w, &c, Default::default());
            // Rollback: 20 mispredicted frames from the snapshot, restore,
            // re-simulate the real inputs; every checksum must match.
            if let Some(s) = snap {
                let mut b = s.clone();
                let mut wrong = Bot::new(99);
                for _ in 0..20 {
                    let g: Vec<PlayerInput> = (0..players).map(|_| wrong.next()).collect();
                    b.step(&c, &g);
                }
                let mut b = s;
                for (k, f) in (SNAP..FRAMES).enumerate() {
                    b.step(&c, &input[f]);
                    assert_eq!(b.checksum(), sums[k], "{}: desync at {f}", c.level(li).name);
                }
            }
        }
        println!(
            "{:?}: {} levels x {FRAMES} frames, {players} archers; chest samples {chests}, pickup samples {pickups}",
            inst.kind,
            c.levels.len()
        );
        assert!(chests > 0 && pickups > 0, "treasure appeared");
    }
}
