#![allow(clippy::needless_range_loop)]
//! Versus matches (docs/spec/versus.md): scripted full matches for 2-12
//! players and teams, scoring rules, leave rule, miasma, spawns, level order,
//! and determinism of a whole match under rollback.

use tfp_sim::content::{Content, LevelDef};
use tfp_sim::entity::{Kind, Tag};
use tfp_sim::player::{DeathCause, HatState};
use tfp_sim::rng::Rng;
use tfp_sim::variants::Variant;
use tfp_sim::versus::{self, MatchLength, MatchSettings, Mode, Phase, ScoreEvent};
use tfp_sim::{PlayerInput, World};

const MAX: usize = 12;

/// A 32x24 map builder.
struct Map(Vec<Vec<char>>);

impl Map {
    fn base() -> Map {
        let mut g = vec![vec!['.'; 32]; 24];
        for x in 0..32 {
            g[0][x] = '#';
            g[22][x] = '#';
            g[23][x] = '#';
        }
        for row in g.iter_mut().take(22) {
            row[0] = '#';
            row[31] = '#';
        }
        // Wrap openings in the side walls and the ceiling.
        for row in g.iter_mut().take(14).skip(11) {
            row[0] = '.';
            row[31] = '.';
        }
        for x in 13..19 {
            g[0][x] = '.';
        }
        Map(g)
    }
    fn fill(mut self, row: usize, c0: usize, c1: usize) -> Map {
        for x in c0..=c1 {
            self.0[row][x] = '#';
            self.0[row][31 - x] = '#';
        }
        self
    }
    /// Put `ch` at (row, col) and its mirror (`P` stays `P`, `A` <-> `B`).
    fn put(mut self, row: usize, col: usize, ch: char) -> Map {
        self.0[row][col] = ch;
        let m = match ch {
            'A' => 'B',
            'B' => 'A',
            o => o,
        };
        self.0[row][31 - col] = m;
        self
    }
    fn level(&self, name: &str) -> LevelDef {
        let rows: Vec<String> = self.0.iter().map(|r| r.iter().collect()).collect();
        let refs: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
        LevelDef::from_ascii(name, &refs)
    }
}

fn tower() -> Content {
    // Level 0: 4 FFA spawns + 3v3 team spawns.
    let a = Map::base()
        .fill(16, 3, 9)
        .fill(10, 11, 13)
        .fill(5, 3, 8)
        .put(21, 5, 'P')
        .put(15, 5, 'P')
        .put(21, 3, 'A')
        .put(15, 8, 'A')
        .put(9, 12, 'A');
    // Level 1: a duel level (2 spawns) with team spawns.
    let b = Map::base()
        .fill(17, 5, 12)
        .fill(11, 2, 6)
        .put(21, 8, 'P')
        .put(21, 4, 'A')
        .put(16, 7, 'A')
        .put(10, 3, 'A');
    // Level 2: 4 FFA spawns, no team spawns.
    let d = Map::base()
        .fill(15, 6, 12)
        .fill(8, 2, 5)
        .put(21, 10, 'P')
        .put(14, 9, 'P');
    // Level 3: team-only level.
    let e = Map::base()
        .fill(16, 2, 8)
        .put(21, 6, 'A')
        .put(15, 4, 'A')
        .put(21, 12, 'A');
    Content::with_levels(vec![a.level("a"), b.level("b"), d.level("d"), e.level("e")])
}

fn settings(mode: Mode, n: usize, length: MatchLength, seed: i32) -> MatchSettings {
    let mut s = MatchSettings::new(mode, n, vec![0, 1, 2, 3]);
    s.length = length;
    s.seed = seed;
    s.tower_card = true;
    s
}

/// The shared bot (`tfp_sim::bot::Bot`) for every slot; it presses Jump
/// while dead or frozen, which confirms the tower card and the results.
fn bots(n: usize, seed: i32) -> Vec<tfp_sim::bot::Bot> {
    (0..n)
        .map(|i| tfp_sim::bot::Bot::new(seed * 100 + i as i32))
        .collect()
}

fn m(w: &World) -> &versus::Match {
    w.versus.as_ref().unwrap()
}

struct Played {
    frames: usize,
    rounds: u32,
    inputs: Vec<Vec<PlayerInput>>,
    world: World,
}

/// Run a match with bots until it is finished.
fn play(c: &Content, s: MatchSettings, seed: i32, cap: usize) -> Played {
    let n = s.player_amount();
    let mut w = versus::new_match(c, s);
    let mut b = bots(n, seed);
    let mut inputs = vec![];
    let mut frames = 0;
    while m(&w).phase != Phase::Finished && frames < cap {
        let i: Vec<PlayerInput> = (0..n).map(|k| b[k].input(&w, c, k as u8)).collect();
        w.step(c, &i);
        inputs.push(i);
        frames += 1;
        check_invariants(&w);
    }
    let rounds = m(&w).round_index + 1;
    Played {
        frames,
        rounds,
        inputs,
        world: w,
    }
}

fn check_invariants(w: &World) {
    let mm = m(w);
    assert!(mm.scores.iter().all(|&s| s >= 0));
    // Every spawned, alive slot has exactly one live archer (or none yet).
    for p in 0..MAX {
        let n = w
            .players()
            .filter(|&id| {
                w.player(id)
                    .is_some_and(|q| q.index as usize == p && !q.dead)
            })
            .count();
        assert!(n <= 1, "slot {p} has {n} archers");
    }
}

fn assert_finished(p: &Played, label: &str) {
    let mm = m(&p.world);
    assert_eq!(
        mm.phase,
        Phase::Finished,
        "{label}: not finished after {} frames",
        p.frames
    );
    let w = mm.winner;
    assert!(w >= 0, "{label}: winner");
    assert!(
        mm.scores[w as usize] >= mm.goal(),
        "{label}: winner reached the goal"
    );
    let best_other = (0..mm.settings.score_count())
        .filter(|&i| i != w as usize)
        .map(|i| mm.scores[i])
        .max()
        .unwrap();
    assert!(mm.scores[w as usize] > best_other, "{label}: unique winner");
    for slot in 0..MAX {
        if mm.settings.joined(slot) {
            assert_eq!(
                mm.stats[slot].won,
                mm.settings.score_index(slot) == w as usize
            );
            assert!(
                !mm.awards[slot].is_empty(),
                "{label}: awards for slot {slot}"
            );
        }
    }
    eprintln!(
        "{label}: {} rounds, {} frames, scores {:?}, winner {w}",
        p.rounds,
        p.frames,
        &mm.scores[..mm.settings.score_count().min(12)]
    );
}

#[test]
fn lms_two_players() {
    let c = tower();
    let p = play(
        &c,
        settings(Mode::LastManStanding, 2, MatchLength::Quick, 1),
        1,
        400_000,
    );
    assert_finished(&p, "LMS 2p");
    let mm = m(&p.world);
    assert_eq!(mm.goal(), 3);
    assert!(p.rounds >= 3);
}

#[test]
fn headhunters_four_players() {
    let c = tower();
    let p = play(
        &c,
        settings(Mode::Headhunters, 4, MatchLength::Quick, 2),
        2,
        400_000,
    );
    assert_finished(&p, "HH 4p");
    assert_eq!(m(&p.world).goal(), 5);
}

#[test]
fn lms_eight_players() {
    let c = tower();
    let p = play(
        &c,
        settings(Mode::LastManStanding, 8, MatchLength::Quick, 3),
        3,
        600_000,
    );
    assert_finished(&p, "LMS 8p");
}

#[test]
fn headhunters_twelve_players() {
    let c = tower();
    let p = play(
        &c,
        settings(Mode::Headhunters, 12, MatchLength::Quick, 4),
        4,
        600_000,
    );
    assert_finished(&p, "HH 12p");
    assert_eq!(m(&p.world).goal(), 7);
}

#[test]
fn team_deathmatch_two_teams() {
    let c = tower();
    let p = play(
        &c,
        settings(Mode::TeamDeathmatch, 6, MatchLength::Quick, 5),
        5,
        600_000,
    );
    assert_finished(&p, "TDM 3v3");
    assert_eq!(m(&p.world).settings.score_count(), 2);
}

#[test]
fn team_deathmatch_three_teams_of_four() {
    let c = tower();
    let mut s = settings(Mode::TeamDeathmatch, 12, MatchLength::Quick, 6);
    s.teams = 3;
    for (i, slot) in s.slots.iter_mut().enumerate() {
        slot.as_mut().unwrap().team = (i % 3) as u8;
    }
    let p = play(&c, s, 6, 800_000);
    assert_finished(&p, "TDM 3x4");
    assert_eq!(m(&p.world).settings.score_count(), 3);
}

// ------------------------------------------------------------ rules

/// Step a fresh match (no tower card) until the round is playing.
fn playing(c: &Content, s: MatchSettings) -> World {
    let mut s = s;
    s.tower_card = false;
    let n = s.player_amount();
    let mut w = versus::new_match(c, s);
    for _ in 0..170 {
        w.step(c, &vec![PlayerInput::default(); n]);
    }
    assert_eq!(m(&w).phase, Phase::Playing);
    w
}

fn kill(w: &mut World, c: &Content, victim: u8, killer: i8, cause: DeathCause) {
    let id = w.player_by_index(victim).expect("victim alive");
    let Some(Kind::Player(mut p)) = w.kind(id).copied() else {
        panic!()
    };
    // Take the kind out like an update would, then die.
    let k = w.kind_mut(id).unwrap();
    *k = Kind::Tiles;
    tfp_sim::player::die(w, c, id, &mut p, cause, killer);
    *w.kind_mut(id).unwrap() = Kind::Player(p);
}

fn idle(w: &mut World, c: &Content, frames: usize) {
    let n = m(w).settings.player_amount();
    for _ in 0..frames {
        w.step(c, &vec![PlayerInput::default(); n]);
    }
}

fn confirm(w: &mut World, c: &Content) {
    let n = m(w).settings.player_amount();
    let mut i = vec![PlayerInput::default(); n];
    i[0] = PlayerInput::dpad(0, 0, PlayerInput::JUMP);
    w.step(c, &i);
    idle(w, c, 1);
}

#[test]
fn headhunters_scoring_self_kills_and_point_loss() {
    let c = tower();
    let mut w = playing(&c, settings(Mode::Headhunters, 4, MatchLength::Standard, 7));
    kill(&mut w, &c, 1, 0, DeathCause::Arrow);
    assert_eq!(m(&w).scores[0], 1);
    kill(&mut w, &c, 0, 0, DeathCause::Arrow); // own arrow
    assert_eq!(m(&w).scores[0], 0, "a self kill costs a point");
    kill(&mut w, &c, 2, -1, DeathCause::Miasma);
    assert_eq!(m(&w).scores[2], 0, "never below zero");
    let ev = &m(&w).round.events;
    assert_eq!(ev, &[ScoreEvent::Gain(0), ScoreEvent::Lose(0)]);
    assert!(m(&w).round.ending, "one archer left");
    let st = &m(&w).stats;
    assert_eq!(st[0].kills.kills, 1);
    assert_eq!(st[0].kills.self_kills, 1);
    assert_eq!(st[2].deaths.environment(), 1);
    // Round end after 90 ticks -> results over a frozen level.
    idle(&mut w, &c, 91);
    assert_eq!(m(&w).phase, Phase::Results);
    assert!(w.frozen);
    let ready = m(&w).results_ready_at;
    assert_eq!(
        ready,
        40.0 + 20.0 + 20.0 + 10.0,
        "two point events, no crown change"
    );
    // Next round: overtime? no. Crown: nobody leads (all 0).
    idle(&mut w, &c, ready as usize);
    confirm(&mut w, &c);
    assert_eq!(m(&w).phase, Phase::Intro);
    assert_eq!(m(&w).round_index, 1);
    assert!(!w.frozen);
    assert_eq!(w.players().count(), 4, "everyone respawned");
}

#[test]
fn lms_survivor_point_crown_and_final_kill() {
    let c = tower();
    let mut s = settings(Mode::LastManStanding, 3, MatchLength::Quick, 8);
    s.tower_card = false;
    let mut w = playing(&c, s);
    assert_eq!(m(&w).goal(), 2);
    kill(&mut w, &c, 1, 0, DeathCause::Arrow);
    kill(&mut w, &c, 2, 0, DeathCause::JumpedOn);
    assert!(m(&w).round.ending && !m(&w).round.was_final_kill);
    idle(&mut w, &c, 91);
    assert_eq!(m(&w).scores[0], 1, "survivor point");
    assert_eq!(
        m(&w).round.events,
        vec![
            ScoreEvent::Gain(0),
            ScoreEvent::Crown({
                let mut f = [false; 12];
                f[0] = true;
                f
            })
        ]
    );
    let ready = m(&w).results_ready_at as usize;
    idle(&mut w, &c, ready);
    confirm(&mut w, &c);
    // Round 2: slot 0 is crowned.
    idle(&mut w, &c, 2);
    let id = w.player_by_index(0).unwrap();
    assert_eq!(w.player(id).unwrap().hat, HatState::Crown);
    idle(&mut w, &c, 170);
    assert_eq!(m(&w).phase, Phase::Playing);
    // The deciding kill of the match triggers the final-kill slow motion.
    kill(&mut w, &c, 1, 0, DeathCause::Arrow);
    kill(&mut w, &c, 2, 0, DeathCause::Arrow);
    assert!(m(&w).round.was_final_kill);
    idle(&mut w, &c, 1);
    assert_eq!(w.time.time_mult, 0.5);
    let mut ticks = 0;
    while m(&w).phase == Phase::Playing {
        idle(&mut w, &c, 1);
        ticks += 1;
    }
    assert!(
        (100..=125).contains(&ticks),
        "final kill takes ~110 ticks, got {ticks}"
    );
    assert!(m(&w).decided);
    assert_eq!(m(&w).winner, 0);
    assert!(m(&w).stats[0].got_win);
    let ready = m(&w).results_ready_at as usize;
    idle(&mut w, &c, ready);
    confirm(&mut w, &c);
    assert_eq!(m(&w).phase, Phase::MatchResults);
    let ready = m(&w).results_ready_at as usize;
    idle(&mut w, &c, ready);
    confirm(&mut w, &c);
    assert_eq!(m(&w).phase, Phase::Finished);
}

#[test]
fn lms_draw_gives_nobody_a_point() {
    let c = tower();
    let mut w = playing(
        &c,
        settings(Mode::LastManStanding, 2, MatchLength::Standard, 9),
    );
    kill(&mut w, &c, 1, 0, DeathCause::Arrow);
    idle(&mut w, &c, 30);
    kill(&mut w, &c, 0, 1, DeathCause::Arrow); // the survivor dies in the countdown
    idle(&mut w, &c, 70);
    assert_eq!(m(&w).phase, Phase::Results);
    assert_eq!(m(&w).scores[..2], [0, 0]);
}

#[test]
fn team_kills_do_not_score() {
    let c = tower();
    let mut w = playing(
        &c,
        settings(Mode::TeamDeathmatch, 4, MatchLength::Standard, 10),
    );
    // Slots 0, 2 are Blue (team 0); 1, 3 Red.
    kill(&mut w, &c, 2, 0, DeathCause::Arrow);
    assert_eq!(m(&w).stats[0].kills.team_kills, 1);
    assert!(!m(&w).round.ending);
    kill(&mut w, &c, 1, 3, DeathCause::Arrow);
    kill(&mut w, &c, 3, 0, DeathCause::Arrow);
    assert!(m(&w).round.ending, "only Blue is alive");
    idle(&mut w, &c, 91);
    assert_eq!(m(&w).scores[..2], [1, 0], "the surviving team scores");
}

#[test]
fn headhunters_overtime_spawns_only_the_tied_leaders() {
    let c = tower();
    let mut s = settings(Mode::Headhunters, 3, MatchLength::Instant, 11);
    s.tower_card = false;
    let mut w = playing(&c, s);
    assert_eq!(m(&w).goal(), 1);
    kill(&mut w, &c, 2, 0, DeathCause::Arrow); // 0 reaches the goal (unique) -> other could win?
    // 1 is alive with 0 points: threshold = 1 - (2 - 1) = 0 -> 1 could still tie.
    assert!(!m(&w).round.was_final_kill);
    // Slot 1's kill of 0 ties 1:1 -> no winner -> overtime next round.
    kill(&mut w, &c, 0, 1, DeathCause::Arrow);
    assert_eq!(m(&w).scores[..3], [1, 1, 0]);
    assert_eq!(m(&w).get_winner(), -1);
    idle(&mut w, &c, 91);
    let ready = m(&w).results_ready_at as usize;
    idle(&mut w, &c, ready);
    confirm(&mut w, &c);
    assert!(m(&w).round.overtime);
    idle(&mut w, &c, 2);
    let spawned: Vec<u8> = w.players().map(|id| w.player(id).unwrap().index).collect();
    assert_eq!(spawned, [0, 1], "only the tied leaders play overtime");
}

#[test]
fn leave_rule() {
    let c = tower();
    let mut w = playing(
        &c,
        settings(Mode::LastManStanding, 3, MatchLength::Standard, 12),
    );
    w.mark_disconnected(2);
    // Slot 2's archer is inert but alive; the round ends when one connected archer is left.
    kill(&mut w, &c, 1, 0, DeathCause::Arrow);
    assert!(m(&w).round.ending);
    idle(&mut w, &c, 91);
    assert_eq!(m(&w).scores[0], 1);
    assert!(!m(&w).active[2], "removed at round end");
    assert!(!m(&w).decided, "two sides remain");
    let ready = m(&w).results_ready_at as usize;
    idle(&mut w, &c, ready);
    confirm(&mut w, &c);
    idle(&mut w, &c, 2);
    let spawned: Vec<u8> = w.players().map(|id| w.player(id).unwrap().index).collect();
    assert_eq!(spawned, [0, 1], "a departed slot does not spawn again");
    // Now slot 1 leaves too: one side left -> the match ends after this round.
    idle(&mut w, &c, 170);
    w.mark_disconnected(1);
    idle(&mut w, &c, 2);
    assert!(m(&w).round.ending, "a lone connected archer ends the round");
    idle(&mut w, &c, 95);
    assert_eq!(m(&w).phase, Phase::Results);
    assert!(m(&w).decided);
    assert_eq!(m(&w).scores[..3], [2, 0, 0]);
}

#[test]
fn sudden_death_miasma_kills() {
    let c = tower();
    let mut s = settings(Mode::Headhunters, 2, MatchLength::Standard, 13);
    s.variants
        .set_all(Variant::SuddenDeath, true, s.joined_mask());
    let mut w = playing(&c, s);
    let mut died = vec![];
    for _ in 0..1500 {
        idle(&mut w, &c, 1);
        for e in w.events() {
            if let tfp_sim::Event::Death {
                player,
                cause,
                killer,
            } = *e
            {
                died.push((player, cause, killer));
            }
        }
        if m(&w).phase != Phase::Playing {
            break;
        }
    }
    assert!(!w.tagged(Tag::PlayerCollider).is_empty() || !died.is_empty());
    assert!(!died.is_empty(), "the closing wall kills idle archers");
    assert!(
        died.iter()
            .all(|&(_, cause, killer)| cause == DeathCause::Miasma as u8 && killer == -1)
    );
}

#[test]
fn derived_spawns_for_twelve() {
    let c = tower();
    let lvl = c.level(0);
    let pts = versus::spawn::derive_spawns(&c, lvl, 12, &mut Rng::new(3));
    assert_eq!(pts.len(), 12);
    let authored = tfp_sim::versus::levels::authored_pool(lvl).len();
    assert_eq!(authored, 10);
    for (i, a) in pts.iter().enumerate().skip(authored) {
        for b in &pts[..i] {
            let d = tfp_sim::actor::wrap_dist_sq(*a, *b, 320.0, 240.0);
            assert!(d >= 24.0 * 24.0 - 1e-3, "{a:?} {b:?} too close");
        }
    }
    // Tier selection: 3 players -> authored (levels with >= 3 PlayerSpawn: 0, 2).
    let s = settings(Mode::Headhunters, 3, MatchLength::Standard, 1);
    let (e, tier) = versus::levels::eligible(&c, &s, 3);
    assert_eq!((e, tier), (vec![0, 2], versus::SpawnTier::Authored));
    let s = settings(Mode::Headhunters, 8, MatchLength::Standard, 1);
    let (_, tier) = versus::levels::eligible(&c, &s, 8);
    assert_eq!(
        tier,
        versus::SpawnTier::Pool,
        "pool of level 0 has 4 + 6 points"
    );
    let s = settings(Mode::TeamDeathmatch, 6, MatchLength::Standard, 1);
    let (e, tier) = versus::levels::eligible(&c, &s, 6);
    assert_eq!((e, tier), (vec![0, 1, 3], versus::SpawnTier::Authored));
}

#[test]
fn level_order_cycles_without_repeats() {
    let c = tower();
    let mut s = settings(Mode::Headhunters, 2, MatchLength::Standard, 21);
    s.fixed_first = true;
    let mut ls = versus::LevelSystem::default();
    let mut rng = Rng::new(5);
    let order: Vec<usize> = (0..9).map(|_| ls.next(&c, &s, 2, &mut rng)).collect();
    assert_eq!(order[0], 0, "fixed first");
    for w in order.windows(2) {
        assert_ne!(w[0], w[1], "no back-to-back repeat: {order:?}");
    }
    let mut first_cycle = order[..3].to_vec();
    first_cycle.sort();
    assert_eq!(
        first_cycle,
        [0, 1, 2],
        "every eligible level once per cycle (3 is team-only)"
    );
}

#[test]
fn whole_match_rollback_determinism() {
    let c = tower();
    let s = settings(Mode::Headhunters, 4, MatchLength::Quick, 31);
    let p = play(&c, s.clone(), 31, 400_000);
    assert_finished(&p, "HH 4p (rollback)");
    // Re-run with the recorded inputs, snapshotting and resimulating.
    let mut w = versus::new_match(&c, s);
    let mut sums = vec![];
    let mut snaps = vec![];
    let mut pick = Rng::new(99);
    for (f, i) in p.inputs.iter().enumerate() {
        if pick.chance(0.003) || f == 0 {
            snaps.push((f, w.clone()));
        }
        w.step(&c, i);
        sums.push(w.checksum());
    }
    assert_eq!(
        w.checksum(),
        p.world.checksum(),
        "replaying the inputs reproduces the match"
    );
    assert!(snaps.len() > 5);
    let mut garbage = Rng::new(1);
    for (f, snap) in &snaps {
        let mut wrong = snap.clone();
        for _ in 0..10 {
            let g: Vec<PlayerInput> = (0..4)
                .map(|_| {
                    PlayerInput::dpad(garbage.range_i(-1, 3) as i8, 0, garbage.next_max(16) as u8)
                })
                .collect();
            wrong.step(&c, &g);
        }
        let mut re = snap.clone();
        for k in *f..(f + 900).min(p.inputs.len()) {
            re.step(&c, &p.inputs[k]);
            assert_eq!(re.checksum(), sums[k], "frame {k} from snapshot {f}");
        }
    }
}
