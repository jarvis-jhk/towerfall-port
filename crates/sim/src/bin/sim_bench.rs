//! `sim-bench`: cost of `World::clone` (rollback saves one per frame) and of
//! `World::step` with 12 archers fighting (bot inputs). Uses a real Versus
//! level when an install is found, else a built-in arena.
//!
//! Usage: `sim-bench [frames]` (release build recommended).

use std::hint::black_box;
use std::time::Instant;
use tfp_assets::install::Installs;
use tfp_sim::bot::Bot;
use tfp_sim::content::Content;
use tfp_sim::{MatchSetup, PlayerInput, PlayerSetup, World};

const ARENA: [&str; 24] = [
    "#######......##########......###",
    "#..............................#",
    "#..P........................P..#",
    "#######..................#######",
    "#..............................#",
    "#..........-------.............#",
    "#..P........................P..#",
    "#....#####............#####....#",
    "#....#...................#.....#",
    "......P................P........",
    "................................",
    "#######........##........#######",
    "#..............##..............#",
    "#...P......................P...#",
    "#.....------........------.....#",
    "#..............................#",
    "#...P......................P...#",
    "#######....############....#####",
    "#..............................#",
    "#..............................#",
    "......P................P........",
    "#......####..........####......#",
    "##########............##########",
    "#######......##########......###",
];

fn main() {
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);
    let installs = Installs::discover();
    let content = installs
        .steam
        .as_ref()
        .or(installs.eight_player.as_ref())
        .and_then(|inst| {
            let levels = tfp_sim::loader::versus_levels(inst).ok()?;
            let lvl = levels
                .iter()
                .find(|p| p.to_string_lossy().contains("Sacred Ground"))?
                .clone();
            tfp_sim::loader::content_from_install(inst, &[lvl]).ok()
        });
    let (c, name) = match content {
        Some(c) => {
            let n = c.level(0).name.clone();
            (c, n)
        }
        None => (Content::from_ascii(&ARENA), "built-in arena".to_owned()),
    };
    let mut setup = MatchSetup {
        seed: 42,
        respawn_frames: Some(30),
        ..Default::default()
    };
    for i in 0..12 {
        setup.players[i] = Some(PlayerSetup {
            archer: i as u16,
            arrows: 3,
            ..Default::default()
        });
    }
    let mut w = World::new(&c, &setup);
    // The shared bots (they think outside the timed region).
    let mut bots: Vec<Bot> = (0..12).map(|i| Bot::new(1000 + i)).collect();
    let think = |bots: &mut Vec<Bot>, w: &World| -> Vec<PlayerInput> {
        bots.iter_mut()
            .enumerate()
            .map(|(k, b)| b.input(w, &c, k as u8))
            .collect()
    };

    // Warm up.
    for _ in 0..600 {
        let i = think(&mut bots, &w);
        w.step(&c, &i);
    }
    let mut step_ns = 0u128;
    let mut clone_ns = 0u128;
    let mut sum_ns = 0u128;
    let mut max_step = 0u128;
    let mut max_clone = 0u128;
    let mut entities = 0usize;
    let mut max_entities = 0usize;
    let mut deaths = 0usize;
    for _ in 0..frames {
        let i = think(&mut bots, &w);
        let t0 = Instant::now();
        let snap = black_box(w.clone());
        let t1 = Instant::now();
        w.step(&c, &i);
        let t2 = Instant::now();
        let sum = black_box(w.checksum());
        let t3 = Instant::now();
        drop(snap);
        let (cl, st, cs) = (
            (t1 - t0).as_nanos(),
            (t2 - t1).as_nanos(),
            (t3 - t2).as_nanos(),
        );
        clone_ns += cl;
        step_ns += st;
        sum_ns += cs;
        max_clone = max_clone.max(cl);
        max_step = max_step.max(st);
        let n = w.entity_count();
        entities += n;
        max_entities = max_entities.max(n);
        deaths += w
            .events()
            .iter()
            .filter(|e| matches!(e, tfp_sim::Event::Death { .. }))
            .count();
        black_box(sum);
    }
    let f = frames as f64;
    println!("level: {name}; 12 archers, {frames} frames, {deaths} deaths");
    println!(
        "entities: avg {:.1}, max {max_entities}",
        entities as f64 / f
    );
    println!(
        "clone:    avg {:.2} µs, max {:.2} µs",
        clone_ns as f64 / f / 1000.0,
        max_clone as f64 / 1000.0
    );
    println!(
        "step:     avg {:.2} µs, max {:.2} µs",
        step_ns as f64 / f / 1000.0,
        max_step as f64 / 1000.0
    );
    println!("checksum: avg {:.2} µs", sum_ns as f64 / f / 1000.0);
    println!(
        "arena: {} bytes ({} B per entity slot, Player state {} B)",
        w.arena_bytes(),
        std::mem::size_of::<tfp_sim::entity::Kind>() + std::mem::size_of::<tfp_sim::entity::Core>(),
        std::mem::size_of::<tfp_sim::player::Player>()
    );
    versus_bench(frames);
}

/// A 12-archer Headhunters match on the whole Sacred Ground tower (real data
/// only): level order, round flow, clone and step cost with match state.
fn versus_bench(frames: usize) {
    use tfp_sim::versus::{self, MatchLength, MatchSettings, Mode, Phase};
    let installs = Installs::discover();
    let Some(inst) = installs.steam.as_ref().or(installs.eight_player.as_ref()) else {
        return;
    };
    let Ok(levels) = tfp_sim::loader::versus_levels(inst) else {
        return;
    };
    let tower: Vec<_> = levels
        .iter()
        .filter(|p| p.to_string_lossy().contains("Sacred Ground"))
        .cloned()
        .collect();
    let Ok(c) = tfp_sim::loader::content_from_install(inst, &tower) else {
        return;
    };
    let mut s = MatchSettings::new(Mode::Headhunters, 12, (0..tower.len()).collect());
    s.length = MatchLength::Quick;
    s.fixed_first = true;
    s.seed = 7;
    let mut w = versus::new_match(&c, s);
    let mut bots: Vec<Bot> = (0..12).map(|i| Bot::new(5000 + i)).collect();
    let (mut clone_ns, mut step_ns, mut n) = (0u128, 0u128, 0u128);
    let mut last_round = u32::MAX;
    for _ in 0..frames {
        let i: Vec<PlayerInput> = bots
            .iter_mut()
            .enumerate()
            .map(|(k, b)| b.input(&w, &c, k as u8))
            .collect();
        let t0 = Instant::now();
        let snap = black_box(w.clone());
        let t1 = Instant::now();
        w.step(&c, &i);
        let t2 = Instant::now();
        drop(snap);
        clone_ns += (t1 - t0).as_nanos();
        step_ns += (t2 - t1).as_nanos();
        n += 1;
        let m = w.versus.as_ref().unwrap();
        if m.round_index != last_round {
            last_round = m.round_index;
            println!(
                "versus round {}: level {} ({}), {} archers, tier {:?}",
                m.round_index,
                w.level,
                c.level(w.level).name,
                w.players().count(),
                m.levels.tier
            );
        }
        if m.phase == Phase::Finished {
            break;
        }
    }
    let m = w.versus.as_ref().unwrap();
    println!(
        "versus 12p HH on Sacred Ground: {} frames, phase {:?}, scores {:?}",
        n, m.phase, m.scores
    );
    println!(
        "versus clone avg {:.2} µs, step avg {:.2} µs",
        clone_ns as f64 / n as f64 / 1000.0,
        step_ns as f64 / n as f64 / 1000.0
    );
}
