//! Co-op modes: Quest (quest.md), Trials (trials.md), lives, auto revive,
//! Team Revive, determinism with rollback re-simulation, and the enemy
//! hooks into the M5 arrow types, shocks, orbs and prisms.

mod common;
use common::*;
use tfp_assets::GameInstall;
use tfp_assets::install::Installs;
use tfp_sim::enemies::{self, EnemyType};
use tfp_sim::entity::{EntityId, Kind, Tag};
use tfp_sim::player::DeathCause;
use tfp_sim::quest::{self, QuestPhase, QuestScript, QuestSettings};
use tfp_sim::trials::{self, TrialsPhase, TrialsSettings};
use tfp_sim::{Content, PlayerInput, Vec2, World};

/// The arena with two spawn portals, archer spawns and two chest spots.
const QUEST_MAP: [&str; 24] = [
    "################################",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#.....S..................S.....#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#.T..........P...P..........T..#",
    "################################",
    "################################",
];

/// The portal sits in a sealed room: its slime can never reach the archers.
const SEALED_MAP: [&str; 24] = [
    "################################",
    "#..............................#",
    "#..............................#",
    "#.#######......................#",
    "#.#.....#......................#",
    "#.#..S..#......................#",
    "#.#.....#......................#",
    "#.#######......................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#............P...P.............#",
    "################################",
    "################################",
];

/// A Trials room: archer in the middle, four dummies on the floor.
const TRIALS_MAP: [&str; 24] = [
    "################################",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..D.....D.....P......D.....D..#",
    "################################",
    "################################",
];

// ----------------------------------------------------------------- bot

/// The shared bot (`tfp_sim::bot::Bot`), one per archer (since the bot
/// move; the old scripted bot lived here).
fn shared_bots(n: u8) -> Vec<tfp_sim::bot::Bot> {
    (0..n)
        .map(|i| tfp_sim::bot::Bot::new(31 + i as i32))
        .collect()
}

fn bot_inputs(bots: &mut [tfp_sim::bot::Bot], w: &World, c: &Content) -> Vec<PlayerInput> {
    bots.iter_mut()
        .enumerate()
        .map(|(i, b)| b.input(w, c, i as u8))
        .collect()
}

// ------------------------------------------------------------ helpers

fn quest_world(c: &Content, players: usize, seed: i32) -> World {
    let mut s = QuestSettings::new(0, 0, players);
    s.seed = seed;
    quest::new_quest(c, s)
}

fn quest_phase(w: &World) -> QuestPhase {
    w.quest.as_ref().unwrap().phase
}

fn ascii_quest(waves: &[quest::script::SimpleWave]) -> Content {
    let mut c = Content::from_ascii(&QUEST_MAP);
    c.quest_scripts = vec![QuestScript::simple(waves)];
    c
}

fn kill(w: &mut World, c: &Content, index: u8) {
    let id = w.player_by_index(index).expect("alive");
    tfp_sim::hurt::with_player(w, id, |w, p| {
        tfp_sim::player::die(w, c, id, p, DeathCause::Enemy, -1);
    });
}

fn put(w: &mut World, id: EntityId, e: enemies::Enemy) {
    if let Some(Kind::Enemy(slot)) = w.kind_mut(id) {
        *slot = e;
    }
}

fn shooter(s: &Sim, index: u8) -> tfp_sim::arrow::Shooter {
    tfp_sim::arrow::Shooter {
        id: s.pid(index),
        player_index: index as i8,
        allegiance: tfp_sim::entity::Allegiance::Neutral,
        team_color: tfp_sim::entity::Allegiance::Neutral,
        hyper: false,
    }
}

fn enemy(w: &World, id: EntityId) -> Option<enemies::Enemy> {
    match w.kind(id) {
        Some(Kind::Enemy(e)) => Some(*e),
        _ => None,
    }
}

// --------------------------------------------------------------- Quest

/// Two waves of slimes and bats on a synthetic arena: the bots clear the
/// quest; each wave's chest appears; kills are counted.
#[test]
fn quest_bots_clear_ascii_quest() {
    let mut c = ascii_quest(&[
        (&["S0", "S1"], &[("Slime", 3)]),
        (&["S0", "S1"], &[("Slime", 2), ("Bat", 1)]),
    ]);
    c.quest_scripts[0].normal[0].groups[0].treasure = vec!["BombArrows".into()];
    let mut w = quest_world(&c, 2, 7);
    let mut saw_chest = false;
    let mut frame = 0;
    let mut bots = shared_bots(2);
    while frame < 60 * 120 && quest_phase(&w) != QuestPhase::Complete {
        let i = bot_inputs(&mut bots, &w, &c);
        w.step(&c, &i);
        saw_chest |= !w.tagged(Tag::TreasureChest).is_empty();
        frame += 1;
    }
    let q = w.quest.as_ref().unwrap();
    assert_eq!(
        q.phase,
        QuestPhase::Complete,
        "after {frame} ticks: wave {}",
        q.current_wave
    );
    assert!(
        q.kills.iter().sum::<u32>() >= 6,
        "kills {:?}",
        &q.kills[..2]
    );
    assert!(saw_chest, "the wave's treasure chest was created");
}

/// The real first tower (wave scripts from the player's install): two bots
/// clear at least the first wave. Skipped without an install.
#[test]
fn quest_first_tower_wave_one() {
    let installs = Installs::discover();
    let all: Vec<GameInstall> = installs.iter().cloned().collect();
    let Some(inst) = all.first() else {
        eprintln!("no TowerFall install found: skipped");
        return;
    };
    let c = tfp_sim::loader::quest_content(inst).expect("quest content");
    assert!(!c.quest_scripts.is_empty());
    let first: u32 = c.quest_scripts[0].normal[0]
        .groups
        .iter()
        .flat_map(|g| g.enemies.iter().map(|e| e.count(2, false)))
        .sum();
    let mut w = quest_world(&c, 2, 11);
    let mut frame = 0;
    let cleared = |w: &World| {
        let q = w.quest.as_ref().unwrap();
        q.current_wave >= 1 || q.phase == QuestPhase::Complete
    };
    let mut bots = shared_bots(2);
    while frame < 60 * 240 && !cleared(&w) && quest_phase(&w) != QuestPhase::GameOver {
        let i = bot_inputs(&mut bots, &w, &c);
        w.step(&c, &i);
        frame += 1;
    }
    let q = w.quest.as_ref().unwrap();
    eprintln!(
        "{}: wave {} of {}, phase {:?}, {frame} ticks, kills {:?} (wave 1: {first})",
        c.quest_scripts[0].name,
        q.current_wave,
        q.total_waves,
        q.phase,
        &q.kills[..2]
    );
    assert!(
        q.kills.iter().sum::<u32>() >= first,
        "wave 1 has {first} enemies"
    );
    assert!(
        cleared(&w),
        "{}: wave {} phase {:?} after {frame} ticks (kills {:?})",
        c.quest_scripts[0].name,
        q.current_wave,
        q.phase,
        &q.kills[..2]
    );
}

/// Quest determinism: bots for 1500 ticks, then re-simulate from snapshots
/// (rollback) with the recorded inputs; every checksum matches.
#[test]
fn quest_rollback_determinism() {
    let c = ascii_quest(&[(&["S0", "S1"], &[("Slime", 3), ("Bat", 2), ("Ghost", 1)])]);
    let mut w = quest_world(&c, 2, 3);
    let mut inputs = vec![];
    let mut sums = vec![];
    let mut snaps = vec![];
    let mut bots = shared_bots(2);
    for f in 0..1500 {
        if f % 250 == 0 {
            snaps.push((f, w.clone()));
        }
        let i = bot_inputs(&mut bots, &w, &c);
        w.step(&c, &i);
        inputs.push(i);
        sums.push(w.checksum());
    }
    for (start, snap) in snaps {
        let mut r = snap;
        for f in start..1500 {
            r.step(&c, &inputs[f]);
            assert_eq!(r.checksum(), sums[f], "diverged at {f} (from {start})");
        }
    }
    let mut fresh = quest_world(&c, 2, 3);
    for i in &inputs {
        fresh.step(&c, i);
    }
    assert_eq!(fresh.checksum(), *sums.last().unwrap());
    // The run must be a real fight (enemies killed), or it proves little.
    let kills: u32 = w.quest.as_ref().unwrap().kills.iter().sum();
    assert!(kills >= 2, "the bots kill enemies ({kills})");
}

/// Lives: a dead archer costs a life and auto revives after its corpse
/// lies 60 ticks; out of lives with everyone dead is game over.
#[test]
fn quest_lives_revive_and_game_over() {
    let mut c = Content::from_ascii(&SEALED_MAP);
    c.quest_scripts = vec![QuestScript::simple(&[(&["S0"], &[("Slime", 1)])])];
    let mut w = quest_world(&c, 1, 1);
    for _ in 0..200 {
        w.step(&c, &[NONE]);
    }
    assert_eq!(quest_phase(&w), QuestPhase::Playing);
    assert_eq!(w.quest.as_ref().unwrap().lives[0], 5);
    kill(&mut w, &c, 0);
    w.step(&c, &[NONE]);
    assert_eq!(w.quest.as_ref().unwrap().lives[0], 4);
    let mut back = None;
    for f in 0..400 {
        w.step(&c, &[NONE]);
        if w.player_by_index(0)
            .and_then(|id| w.player(id))
            .is_some_and(|p| !p.dead)
        {
            back = Some(f);
            break;
        }
    }
    let f = back.expect("auto revived");
    assert!((60..200).contains(&f), "revived after {f} ticks");
    // Spend every life.
    for _ in 0..4 {
        for _ in 0..200 {
            w.step(&c, &[NONE]);
        }
        assert_eq!(quest_phase(&w), QuestPhase::Playing);
        let alive = w
            .player_by_index(0)
            .and_then(|id| w.player(id))
            .is_some_and(|p| !p.dead);
        if alive {
            kill(&mut w, &c, 0);
        }
    }
    for _ in 0..200 {
        w.step(&c, &[NONE]);
    }
    assert_eq!(w.quest.as_ref().unwrap().lives[0], 0);
    kill(&mut w, &c, 0);
    for _ in 0..10 {
        w.step(&c, &[NONE]);
    }
    assert_eq!(quest_phase(&w), QuestPhase::GameOver);
}

// -------------------------------------------------------------- Trials

/// The bot shoots every dummy: the trial is won with a positive time and
/// the medal unlocks follow the thresholds.
#[test]
fn trials_level_completion() {
    let c = Content::from_ascii(&TRIALS_MAP);
    let mut s = TrialsSettings::new(0);
    s.arrows = 6;
    s.gold = 20.0;
    s.diamond = 0.5;
    s.dev = 0.2;
    let mut w = trials::new_trial(&c, s);
    w.step(&c, &[NONE]);
    assert_eq!(w.tagged(Tag::Dummy).len(), 4);
    let mut frame = 0;
    let mut bots = shared_bots(1);
    while frame < 60 * 60 {
        let t = w.trials.as_ref().unwrap();
        if matches!(t.phase, TrialsPhase::Won | TrialsPhase::Results) {
            break;
        }
        let i = bot_inputs(&mut bots, &w, &c);
        w.step(&c, &i);
        frame += 1;
    }
    let t = **w.trials.as_ref().unwrap();
    assert!(
        matches!(t.phase, TrialsPhase::Won | TrialsPhase::Results),
        "phase {:?}",
        t.phase
    );
    assert!(
        t.time > 0 && t.time < trials::ticks(20.0),
        "time {}",
        t.time
    );
    for _ in 0..600 {
        w.step(&c, &[NONE]);
    }
    let r = w.trials.as_ref().unwrap().result.expect("result");
    assert_eq!(r.time, t.time);
    assert!(r.new_best);
    assert_eq!(r.unlocked, [true, false, false]);
}

// --------------------------------------------------------- Team Revive

/// TDM with Team Revive: a teammate standing on the corpse for 60 ticks
/// revives it (the round goes on); the new archer is frozen, then plays.
#[test]
fn team_revive_in_tdm() {
    use tfp_sim::variants::Variant;
    use tfp_sim::versus::{self, MatchSettings, Mode, Phase};
    let c = Content::from_ascii(&QUEST_MAP);
    let mut ms = MatchSettings::new(Mode::TeamDeathmatch, 4, vec![0]);
    ms.tower_card = false;
    let mask = ms.joined_mask();
    ms.variants.set_all(Variant::TeamRevive, true, mask);
    let mut w = versus::new_match(&c, ms);
    let idle = vec![NONE; 4];
    for _ in 0..170 {
        w.step(&c, &idle);
    }
    assert_eq!(w.versus.as_ref().unwrap().phase, Phase::Playing);
    // Slots 0 and 2 are team 0.
    kill(&mut w, &c, 0);
    w.step(&c, &idle);
    let corpse = w.tagged(Tag::Corpse)[0];
    let mut revived = None;
    for f in 0..300 {
        let at = w.core(corpse).pos;
        if let Some(mate) = w.player_by_index(2) {
            w.core_mut(mate).pos = Vec2::new(at.x, w.core(mate).pos.y);
        }
        w.step(&c, &idle);
        if w.player_by_index(0)
            .and_then(|id| w.player(id))
            .is_some_and(|p| !p.dead)
        {
            revived = Some(f);
            break;
        }
        if !w.exists(corpse) {
            break;
        }
    }
    let f = revived.expect("teammate revived the corpse");
    assert!(f >= 60, "revived after {f}");
    assert_eq!(w.versus.as_ref().unwrap().phase, Phase::Playing);
}

// ------------------------------------------- enemies vs M5 mechanics

/// A player's BoltCatch shock hurts enemies in range (force 5); an
/// enemy-owned (-1) one does not.
#[test]
fn shock_hurts_enemies() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let a = enemies::spawn(
        &mut s.w,
        &s.c,
        EnemyType::Slime,
        Vec2::new(200.0, 219.0),
        1,
        &[],
    );
    let b = enemies::spawn(
        &mut s.w,
        &s.c,
        EnemyType::Slime,
        Vec2::new(280.0, 219.0),
        1,
        &[],
    );
    s.step(&[NONE]);
    tfp_sim::explosion::spawn_bolt_catch(&mut s.w, Vec2::new(280.0, 212.0), -1);
    s.step(&[NONE]);
    s.step(&[NONE]);
    assert!(
        enemy(&s.w, b).is_some_and(|e| e.alive()),
        "nobody's shock spares enemies"
    );
    tfp_sim::explosion::spawn_bolt_catch(&mut s.w, Vec2::new(200.0, 212.0), 0);
    s.step(&[NONE]);
    s.step(&[NONE]);
    assert!(
        enemy(&s.w, a).is_none_or(|e| !e.alive()),
        "player shock kills the slime"
    );
}

/// A toy arrow bounces off an enemy (speed 4 along the hit) and never kills.
#[test]
fn toy_arrow_bounces_enemy() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = enemies::spawn(
        &mut s.w,
        &s.c,
        EnemyType::Slime,
        Vec2::new(120.0, 219.0),
        -1,
        &[],
    );
    s.step(&[NONE]);
    let sh = shooter(&s, 0);
    tfp_sim::arrow::create(
        &mut s.w,
        tfp_sim::quiver::ArrowType::Toy,
        sh,
        Vec2::new(80.0, 214.0),
        0.0,
    );
    let mut pushed = false;
    for _ in 0..40 {
        s.step(&[NONE]);
        if enemy(&s.w, id).is_some_and(|e| e.speed.x > 2.0) {
            pushed = true;
        }
    }
    assert!(pushed, "bounced");
    assert!(
        enemy(&s.w, id).is_some_and(|e| e.alive()),
        "toy arrows never kill"
    );
}

/// A skeleton trapped in a prism stays inside its bars; the prism breaks
/// after its hold time (or when the skeleton dies).
#[test]
fn enemy_prism_holds_and_breaks() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = enemies::spawn(
        &mut s.w,
        &s.c,
        EnemyType::from_name("Skeleton").unwrap(),
        Vec2::new(200.0, 212.0),
        1,
        &[],
    );
    s.step(&[NONE]);
    let pr = tfp_sim::arrows::prism::start_enemy_prism(&mut s.w, id, None, 0);
    if let Some(Kind::Enemy(e)) = s.w.kind_mut(id) {
        e.prism = Some(pr);
    }
    let x0 = s.w.core(id).pos.x;
    for _ in 0..100 {
        if let Some(Kind::Enemy(e)) = s.w.kind_mut(id) {
            e.speed.x = 3.0;
        }
        s.step(&[NONE]);
        assert!(s.w.core(id).pos.x - x0 <= 16.0, "held by the bars");
    }
    assert!(s.w.exists(pr));
    for _ in 0..60 {
        s.step(&[NONE]);
    }
    assert!(!s.w.exists(pr), "shattered after 150 ticks");
    assert!(enemy(&s.w, id).is_some_and(|e| e.prism.is_none()));
}

/// A skeleton dodging catches a live bolt: the arrow goes into its list and
/// a BoltCatch shock (owner -1) hurts the archer next to it.
#[test]
fn skeleton_catches_bolt() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(180.0, FLOOR_Y)]);
    let id = enemies::spawn(
        &mut s.w,
        &s.c,
        EnemyType::from_name("Skeleton").unwrap(),
        Vec2::new(200.0, 212.0),
        -1,
        &[],
    );
    s.step(&[NONE]);
    let mut e = enemy(&s.w, id).unwrap();
    enemies::set_state(&mut s.w, &s.c, id, &mut e, enemies::skeleton::DODGE);
    put(&mut s.w, id, e);
    let at = s.w.core(id).pos;
    let sh = shooter(&s, 0);
    let a = tfp_sim::arrow::create(
        &mut s.w,
        tfp_sim::quiver::ArrowType::Bolt,
        sh,
        at - Vec2::new(30.0, 0.0),
        0.0,
    );
    let mut caught = false;
    for _ in 0..20 {
        let mut e = enemy(&s.w, id).unwrap();
        let mut ar = match s.w.kind(a) {
            Some(Kind::Arrow(x)) => *x,
            _ => break,
        };
        if enemies::skeleton::try_catch(&mut s.w, &s.c, id, &mut e, a, &mut ar) {
            put(&mut s.w, id, e);
            caught = true;
            break;
        }
        s.step(&[NONE]);
    }
    assert!(caught, "caught the bolt");
    assert_eq!(
        enemy(&s.w, id).unwrap().arrows.next(),
        Some(tfp_sim::quiver::ArrowType::Bolt)
    );
    for _ in 0..3 {
        s.step(&[NONE]);
    }
    assert!(
        s.pid(0).and_then(|p| s.w.player(p)).is_none_or(|p| p.dead),
        "the catch shock hit the archer"
    );
}
