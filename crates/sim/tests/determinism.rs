//! Determinism: identical inputs give identical checksums; restoring a
//! cloned world and re-simulating reproduces every later frame (what
//! rollback relies on).

mod common;
use common::*;
use tfp_sim::bot::Bot;
use tfp_sim::events::Event;
use tfp_sim::rng::Rng;
use tfp_sim::{Content, MatchSetup, PlayerInput, PlayerSetup, World};

const MAP: [&str; 24] = [
    "#######......##########......###",
    "#..............................#",
    "#..P........................P..#",
    "#######..................#######",
    "#..............................#",
    "#..........-------.............#",
    "#..............................#",
    "#....#####............#####....#",
    "#....#...................#.....#",
    "......P................P........",
    "................................",
    "#######........##........#######",
    "#..............##..............#",
    "#..............................#",
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

/// Misprediction inputs: random buttons held for random stretches (the
/// real inputs come from the shared bot, `tfp_sim::bot::Bot`).
struct Garbage {
    rng: Rng,
    left: i32,
    cur: PlayerInput,
}

impl Garbage {
    fn new(seed: i32) -> Garbage {
        Garbage {
            rng: Rng::new(seed),
            left: 0,
            cur: PlayerInput::default(),
        }
    }
    fn next(&mut self) -> PlayerInput {
        if self.left <= 0 {
            self.left = self.rng.range_i(3, 25);
            let x = self.rng.range_i(-1, 3) as i8;
            let mut b = 0;
            if self.rng.chance(0.35) {
                b |= PlayerInput::JUMP;
            }
            if self.rng.chance(0.3) {
                b |= PlayerInput::SHOOT;
            }
            if self.rng.chance(0.1) {
                b |= PlayerInput::DODGE;
            }
            self.cur = PlayerInput::dpad(x, 0, b);
        }
        self.left -= 1;
        self.cur
    }
}

fn setup(n: usize) -> MatchSetup {
    let mut s = MatchSetup {
        seed: 1234,
        respawn_frames: Some(45),
        ..MatchSetup::default()
    };
    for i in 0..n {
        s.players[i] = Some(PlayerSetup {
            archer: i as u16,
            arrows: 3,
            shield: i % 3 == 1,
            ..Default::default()
        });
    }
    s
}

/// Shared bots, one per archer.
fn bots(n: usize, seed: i32) -> Vec<Bot> {
    (0..n).map(|i| Bot::new(seed + i as i32 * 7919)).collect()
}

/// This tick's inputs from the bots looking at `w`.
fn think(bots: &mut [Bot], w: &World, c: &Content) -> Vec<PlayerInput> {
    bots.iter_mut()
        .enumerate()
        .map(|(i, b)| b.input(w, c, i as u8))
        .collect()
}

#[test]
fn rollback_resimulation_is_exact() {
    const FRAMES: usize = 10_000;
    let c = Content::from_ascii(&MAP);
    // Four archers: the shared bots dodge and catch well, two alone fight
    // too little for this test.
    let mut bots = bots(4, 99);
    let mut inp: Vec<Vec<PlayerInput>> = Vec::with_capacity(FRAMES);
    let mut w = World::new(&c, &setup(4));
    let mut sums = Vec::with_capacity(FRAMES);
    let mut snaps: Vec<(usize, World)> = vec![];
    let mut pick = Rng::new(7);
    let mut deaths = 0;
    let mut shots = 0;
    let mut max_entities = 0;
    for f in 0..FRAMES {
        if pick.chance(0.004) {
            snaps.push((f, w.clone()));
        }
        // The real inputs: the shared bots on the live world (recorded for
        // the re-simulations below).
        let frame_inputs = think(&mut bots, &w, &c);
        w.step(&c, &frame_inputs);
        inp.push(frame_inputs);
        sums.push(w.checksum());
        for e in w.events() {
            match e {
                Event::Death { .. } => deaths += 1,
                Event::CharSound {
                    sound: tfp_sim::events::CharSound::FireArrow,
                    ..
                } => shots += 1,
                _ => {}
            }
        }
        max_entities = max_entities.max(w.entity_count());
    }
    eprintln!(
        "{FRAMES} frames: {deaths} deaths, {shots} shots, max {max_entities} entities, {} snapshots",
        snaps.len()
    );
    // Shared bots since the bot move (the random-input bot needed >= 5 / 3
    // deaths; these fight properly): a real fight in 10 000 frames.
    assert!(deaths >= 5 && shots >= 50, "the bots must actually fight");
    assert!(snaps.len() >= 10);
    let mut distinct = sums.clone();
    distinct.dedup();
    assert!(distinct.len() > FRAMES / 2, "checksums change over time");

    let mut garbage = Garbage::new(4242);
    for (f, snap) in &snaps {
        // Misprediction: run ahead with wrong inputs, then roll back.
        let mut wrong = snap.clone();
        for _ in 0..8 {
            let g: Vec<PlayerInput> = (0..4).map(|_| garbage.next()).collect();
            wrong.step(&c, &g);
        }
        let mut re = snap.clone();
        assert_eq!(
            re.checksum(),
            if *f == 0 { re.checksum() } else { sums[f - 1] }
        );
        for k in *f..FRAMES.min(f + 600) {
            re.step(&c, &inp[k]);
            assert_eq!(
                re.checksum(),
                sums[k],
                "re-simulated frame {k} from snapshot {f}"
            );
        }
    }
}

#[test]
fn independent_runs_agree() {
    let c = Content::from_ascii(&MAP);
    let run = || {
        let mut w = World::new(&c, &setup(4));
        let mut b = bots(4, 5);
        let mut out = vec![];
        for _ in 0..3000 {
            let i = think(&mut b, &w, &c);
            w.step(&c, &i);
            out.push(w.checksum128());
        }
        out
    };
    assert_eq!(run(), run());
}

#[test]
fn twelve_archers_run() {
    let c = Content::from_ascii(&MAP);
    let mut w = World::new(&c, &setup(12));
    let mut b = bots(12, 77);
    for _ in 0..2000 {
        let i = think(&mut b, &w, &c);
        w.step(&c, &i);
    }
    assert!(w.players().count() > 0);
    let _ = NONE;
}
