//! Dark World (darkworld.md) on the player's install (skipped without the
//! expansion): the tower data, bots clearing the first level and taking the
//! exit portal, every boss fought to its end (damage applied by the test,
//! archers kept invulnerable, intermission enemies cleared), lives / game
//! over / continue, and rollback re-simulation of a level and of a boss.

mod common;
use common::*;
use tfp_assets::install::Installs;
use tfp_sim::darkworld::{self, DwPhase, DwSettings};
use tfp_sim::entity::Kind;
use tfp_sim::player::DeathCause;
use tfp_sim::{Content, PlayerInput, Vec2, World};

fn content() -> Option<Content> {
    let all: Vec<_> = Installs::discover().iter().cloned().collect();
    let inst = all.iter().find(|i| i.dark_world.is_some())?;
    let c = tfp_sim::loader::dark_world_content(inst).expect("dark world content");
    (!c.dark_world.is_empty()).then_some(c)
}

macro_rules! need_content {
    () => {
        match content() {
            Some(c) => c,
            None => {
                eprintln!("no Dark World install found: skipped");
                return;
            }
        }
    };
}

fn dw(w: &World) -> &darkworld::DarkWorld {
    w.dark_world.as_ref().unwrap()
}

fn kill(w: &mut World, c: &Content, index: u8) {
    if let Some(id) = w.player_by_index(index) {
        tfp_sim::hurt::with_player(w, id, |w, p| {
            tfp_sim::player::die(w, c, id, p, DeathCause::Enemy, -1);
        });
    }
}

// ----------------------------------------------------------------- bot

/// The shared bot (`tfp_sim::bot::Bot`), one per archer (since the bot
/// move; it walks into the open exit portal by itself).
fn shared_bots(n: u8) -> Vec<tfp_sim::bot::Bot> {
    (0..n)
        .map(|i| tfp_sim::bot::Bot::new(41 + i as i32))
        .collect()
}

fn bots(bots: &mut [tfp_sim::bot::Bot], w: &World, c: &Content) -> Vec<PlayerInput> {
    bots.iter_mut()
        .enumerate()
        .map(|(i, b)| b.input(w, c, i as u8))
        .collect()
}

// --------------------------------------------------------------- tests

#[test]
fn tower_data() {
    let c = need_content!();
    assert_eq!(c.dark_world.len(), 5);
    for t in &c.dark_world {
        for (d, list) in t.levels.iter().enumerate() {
            assert!(!list.is_empty(), "{} difficulty {d}", t.name);
            let last = list.last().unwrap();
            assert!(last.boss.is_some(), "{}: the last level is a boss", t.name);
            for (k, l) in list.iter().enumerate() {
                let lv = c.level(l.arena);
                assert!(lv.player_spawns.len() >= 4, "{}: arena {k} spawns", t.name);
                if k + 1 < list.len() {
                    assert!(
                        lv.markers.endless_portal.is_some(),
                        "{}: arena {k} has no exit",
                        t.name
                    );
                }
                if l.boss.is_none() {
                    assert!(
                        !lv.markers.spawners.is_empty(),
                        "{}: arena {k} has no spawner",
                        t.name
                    );
                    assert!(
                        !t.sets[l.enemy_set].1.is_empty(),
                        "{}: empty enemy set",
                        t.name
                    );
                }
            }
        }
    }
    // Normal: 5 stages + boss on towers 0-3; 4 bosses on the Gauntlet.
    assert_eq!(c.dark_world[0].levels[0].len(), 6);
    assert_eq!(c.dark_world[4].levels[0].len(), 4);
}

/// Level 1 of the Amaranth with two shared bots, unassisted (the old test
/// bot needed invulnerability, free arrows, cleared leftovers and a carry
/// to the exit portal above a ceiling; the shared bot's reachability
/// graph finds the way): the waves are paced and spawned, the clear opens
/// the exit portal, the archers go through and level 2 loads with them.
#[test]
fn bots_clear_level_one() {
    let c = need_content!();
    let mut s = DwSettings::new(0, darkworld::NORMAL, 2);
    s.seed = 2;
    let mut w = darkworld::new_dark_world(&c, s);
    let mut frame = 0;
    let mut b = shared_bots(2);
    while frame < 60 * 300 && dw(&w).round_index == 0 && dw(&w).phase != DwPhase::GameOver {
        let i = bots(&mut b, &w, &c);
        w.step(&c, &i);
        frame += 1;
    }
    let d = dw(&w);
    eprintln!(
        "level 1: phase {:?}, round {}, {frame} ticks, kills {:?}",
        d.phase,
        d.round_index,
        &d.kills[..2]
    );
    assert_eq!(d.round_index, 1, "reached level 2 (phase {:?})", d.phase);
    assert!(d.kills.iter().sum::<u32>() > 0);
    // Both archers are in the new level.
    for _ in 0..5 {
        w.step(&c, &[NONE, NONE]);
    }
    assert!(w.player_by_index(0).is_some() && w.player_by_index(1).is_some());
}

/// Fight boss `tower` to its end: archers flash (invulnerable), the test
/// deals damage whenever the boss can take it, clears intermission enemies
/// and shield orbs. Returns the boss states seen.
fn fight(c: &Content, tower: usize, players: usize, remove_archers: bool) -> Vec<(u8, u8)> {
    fight_at(c, tower, players, remove_archers, darkworld::NORMAL)
}

fn fight_at(
    c: &Content,
    tower: usize,
    players: usize,
    remove_archers: bool,
    diff: u8,
) -> Vec<(u8, u8)> {
    let mut s = DwSettings::new(tower, diff, players);
    s.seed = 2;
    let mut w = darkworld::new_dark_world(c, s);
    let boss_level = dw(&w).levels(c).len() - 1;
    darkworld::jump_to_level(&mut w, c, boss_level);
    let idle = vec![NONE; players];
    if remove_archers {
        // Darkfang's fists crush through invulnerability: fight without
        // archers (removed, not killed).
        w.step(c, &idle);
        let ids: Vec<_> = w.players().collect();
        for id in ids {
            w.remove(id);
        }
    }
    let mut seen: Vec<(u8, u8)> = vec![];
    for t in 0..60 * 400 {
        // Crushes kill through invulnerability: solo fights get spare lives.
        if players == 1 {
            w.dark_world.as_mut().unwrap().extra_lives = 50;
        }
        for i in 0..players as u8 {
            if let Some(id) = w.player_by_index(i) {
                w.core_mut(id)
                    .flash
                    .start_for(5.0, tfp_sim::entity::FlashFinish::None);
            }
        }
        if let Some(bp) = darkworld::boss_phase(&w)
            && seen.last() != Some(&bp)
        {
            seen.push(bp);
        }
        if t % 30 == 0 {
            // Clear intermission enemies and shield orbs.
            let none = w.entities().next().unwrap();
            tfp_sim::enemies::kill_all(&mut w, c, none);
            let orbs: Vec<_> = w
                .entities()
                .filter(|&e| matches!(w.kind(e), Some(Kind::Dw(darkworld::Part::ShieldOrb { .. }))))
                .collect();
            for o in orbs {
                w.remove(o);
            }
        }
        if t % 20 == 0
            && dw(&w).boss_mode
            && let Some((id, st)) = darkworld::boss_phase(&w)
        {
            let ok = match id {
                0 => st == 1,
                1 => st == 1,
                2 => st == 1,
                _ => st == 2 || st == 4,
            };
            if ok {
                darkworld::debug_damage(&mut w, 1, 0);
            }
        }
        // The Cataclysm heart phase is not boss mode.
        if t % 20 == 0 && darkworld::boss_phase(&w) == Some((3, 4)) {
            darkworld::debug_damage(&mut w, 1, 0);
        }
        w.step(c, &idle);
        if dw(&w).boss_finished {
            break;
        }
    }
    assert!(
        dw(&w).boss_finished,
        "boss {tower} finished; states {seen:?}"
    );
    // The boss level of a tower is its last: the session completes.
    for _ in 0..10 {
        w.step(c, &idle);
    }
    assert_eq!(dw(&w).phase, DwPhase::Complete, "boss {tower}");
    let r = dw(&w).result.expect("result");
    assert!(r.completed);
    seen
}

#[test]
fn amaranth_fight() {
    let c = need_content!();
    let seen = fight(&c, 0, 1, false);
    assert!(seen.contains(&(0, 1)) && seen.contains(&(0, 2)), "{seen:?}");
}

#[test]
fn dreadwood_fight() {
    let c = need_content!();
    let seen = fight(&c, 1, 2, false);
    let inter = seen.iter().filter(|&&(_, s)| s == 2).count();
    assert_eq!(inter, 2, "two intermissions: {seen:?}");
}

#[test]
fn darkfang_fight() {
    let c = need_content!();
    let seen = fight(&c, 2, 1, true);
    let mad = seen.iter().filter(|&&(_, s)| s == 2).count();
    assert_eq!(mad, 2, "two Mad phases: {seen:?}");
}

/// Legendary Cataclysm: the finale with the miasma and the rising lava.
#[test]
fn cataclysm_legendary_fight() {
    let c = need_content!();
    let seen = fight_at(&c, 3, 1, true, darkworld::LEGENDARY);
    assert!(seen.contains(&(3, 3)) && seen.contains(&(3, 4)), "{seen:?}");
}

#[test]
fn cataclysm_fight() {
    let c = need_content!();
    let seen = fight(&c, 3, 1, false);
    for st in [1u8, 2, 3, 4] {
        assert!(seen.contains(&(3, st)), "state {st}: {seen:?}");
    }
}

/// Solo: 3 extra lives, each spent by an auto revive 60 ticks after death;
/// without lives game over; continue only after level 1.
#[test]
fn solo_lives_game_over_and_continue() {
    let c = need_content!();
    let mut w = darkworld::new_dark_world(&c, DwSettings::new(0, darkworld::HARDCORE, 1));
    for _ in 0..200 {
        w.step(&c, &[NONE]);
    }
    assert_eq!(dw(&w).extra_lives, 3);
    for life in (0..3).rev() {
        kill(&mut w, &c, 0);
        let mut back = false;
        for _ in 0..500 {
            w.step(&c, &[NONE]);
            if w.player_by_index(0)
                .and_then(|id| w.player(id))
                .is_some_and(|p| !p.dead)
            {
                back = true;
                break;
            }
        }
        assert!(back, "revived");
        assert_eq!(dw(&w).extra_lives, life);
        for _ in 0..60 {
            w.step(&c, &[NONE]);
        }
    }
    kill(&mut w, &c, 0);
    for _ in 0..5 {
        w.step(&c, &[NONE]);
    }
    assert_eq!(dw(&w).phase, DwPhase::GameOver);
    assert!(
        !darkworld::continue_level(&mut w, &c),
        "no continue on level 1"
    );
    // On level 2 a continue reloads the level (Hardcore: 8 continues).
    darkworld::jump_to_level(&mut w, &c, 1);
    for _ in 0..200 {
        w.step(&c, &[NONE]);
    }
    w.dark_world.as_mut().unwrap().extra_lives = 0;
    kill(&mut w, &c, 0);
    for _ in 0..5 {
        w.step(&c, &[NONE]);
    }
    assert_eq!(dw(&w).phase, DwPhase::GameOver);
    assert_eq!(dw(&w).continues_remaining(), 8);
    assert!(darkworld::continue_level(&mut w, &c));
    assert_eq!(dw(&w).continues, 1);
    assert_eq!(dw(&w).continues_remaining(), 7);
    assert_eq!(dw(&w).round_index, 1);
    assert_eq!(dw(&w).extra_lives, 3);
}

fn rollback(c: &Content, mut w: World, players: usize, frames: usize) {
    let mut inputs = vec![];
    let mut sums = vec![];
    let mut snaps = vec![];
    let mut b = shared_bots(players as u8);
    for f in 0..frames {
        if f % 300 == 0 {
            snaps.push((f, w.clone()));
        }
        let i = bots(&mut b, &w, c);
        w.step(c, &i);
        inputs.push(i);
        sums.push(w.checksum());
    }
    for (start, snap) in snaps {
        let mut r = snap;
        for f in start..frames {
            r.step(c, &inputs[f]);
            assert_eq!(r.checksum(), sums[f], "diverged at {f} (from {start})");
        }
    }
}

#[test]
fn rollback_level_and_boss() {
    let c = need_content!();
    let w = darkworld::new_dark_world(&c, DwSettings::new(1, darkworld::LEGENDARY, 2));
    rollback(&c, w, 2, 1800);
    for tower in 0..4 {
        let mut w = darkworld::new_dark_world(&c, DwSettings::new(tower, darkworld::HARDCORE, 2));
        let last = dw(&w).levels(&c).len() - 1;
        darkworld::jump_to_level(&mut w, &c, last);
        rollback(&c, w, 2, 1500);
    }
}

#[test]
fn pacing_penalty_and_variant_rules() {
    assert_eq!(darkworld::dead_penalty(4, 1), 10);
    assert_eq!(darkworld::dead_penalty(4, 3), 3);
    assert_eq!(darkworld::dead_penalty(2, 1), 6);
    assert_eq!(darkworld::dead_penalty(2, 2), 0);
    assert_eq!(darkworld::dead_penalty(8, 4), 6);
    use tfp_sim::variants::{Variant, VariantSet};
    let mut v = VariantSet::default();
    for p in darkworld::POWER_UPS {
        v.set_all(p, true, 1);
    }
    v.set_all(Variant::ExplodingCorpses, true, 1);
    darkworld::enforce_variants(&mut v, darkworld::HARDCORE, 1, 1);
    let on = darkworld::POWER_UPS.iter().filter(|&&p| v.any(p)).count();
    assert_eq!(on, 1, "Hardcore allows one power-up");
    assert!(!v.any(Variant::ExplodingCorpses), "not solo");
}

/// Bottom miasma (Dreadwood): rises from the floor, hurts an archer in it.
#[test]
fn bottom_miasma_rises_and_hurts() {
    use tfp_sim::versus::miasma::{self, MiasmaMode};
    let mut s = Sim::new(&ARENA, &[Vec2::new(160.0, FLOOR_Y)]);
    let ids = miasma::spawn_mode(&mut s.w, MiasmaMode::Bottom);
    assert_eq!(ids[0], ids[1]);
    // At tick 300 of the cycle: percent 0.28 * CubeInOut(120/240) = 0.14.
    for _ in 0..300 {
        s.step(&[NONE]);
    }
    let core = s.w.core(ids[0]);
    let h = core.hitbox().h;
    assert!((h - (0.14 * 120.0 - 5.0)).abs() < 1.5, "height {h}");
    assert!(
        (core.pos.y + h - 240.0).abs() < 0.01,
        "anchored at the floor"
    );
    // At the crest (0.28 -> height 28.6) the floor archer (feet at 220) is inside.
    for _ in 0..140 {
        s.step(&[NONE]);
    }
    assert!(
        s.pid(0)
            .is_none_or(|id| s.w.player(id).is_none_or(|p| p.dead)),
        "the miasma killed"
    );
}

/// The Amaranth / Cataclysm miasma timelines.
#[test]
fn boss_miasma_timelines() {
    use tfp_sim::versus::miasma::{MiasmaMode, boss_timeline};
    let (p, side) = boss_timeline(MiasmaMode::Amaranth, 180.0, 1.0);
    assert!((p - 0.25).abs() < 1e-5 && side == 0.0);
    let (_, side) = boss_timeline(MiasmaMode::Amaranth, 180.0 + 240.0, -1.0);
    assert!((side + 0.25).abs() < 1e-5);
    let (p, _) = boss_timeline(MiasmaMode::Cataclysm, 500.0, 1.0);
    assert!((p - 0.2).abs() < 1e-5);
}

/// Boss lava: one bottom slab rising to target 25; gone after BossFinish.
#[test]
fn boss_lava_rises_and_leaves() {
    let mut s = Sim::new(&ARENA, &[]);
    let ctl = tfp_sim::entities::hazards::start_boss_lava(&mut s.w, &s.c);
    for _ in 0..400 {
        s.step(&[]);
    }
    let lava =
        s.w.entities()
            .find(|&e| {
                matches!(
                    s.w.kind(e),
                    Some(Kind::Tower(tfp_sim::entities::Tower::Lava(_)))
                )
            })
            .expect("lava");
    let y = s.w.core(lava).pos.y;
    assert!((y - (243.0 - 25.0)).abs() <= 1.5, "rests at {y}");
    tfp_sim::entities::hazards::boss_lava_finish(&mut s.w, ctl);
    for _ in 0..400 {
        s.step(&[]);
    }
    assert!(!s.w.exists(ctl) && !s.w.exists(lava), "removed");
}
