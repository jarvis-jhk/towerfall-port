//! Enemy base class and spawn portal details (docs/spec/enemies.md §1, §2)
//! re-checked against the reference.

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::enemies::{self, EnemyType};
use tfp_sim::entity::{EntityId, Kind, Tag};

fn enemy(s: &Sim, id: EntityId) -> Option<enemies::Enemy> {
    match s.w.kind(id) {
        Some(Kind::Enemy(e)) => Some(*e),
        _ => None,
    }
}

fn spawn(s: &mut Sim, name: &str, pos: Vec2, facing: i8) -> EntityId {
    let ty = EnemyType::from_name(name).unwrap();
    enemies::spawn(&mut s.w, &s.c, ty, pos, facing, &[])
}

fn until(s: &mut Sim, max: usize, mut f: impl FnMut(&Sim) -> bool) -> Option<usize> {
    for k in 0..max {
        s.step(&[NONE, NONE]);
        if f(s) {
            return Some(k);
        }
    }
    None
}

/// A Quest wave opens a portal and queues its enemy 5 ticks later: the
/// spawning animation starts at the queueing, not after the appear tween
/// (§2), so the enemy is out ~47 ticks later, not ~82.
#[test]
fn portal_spawns_during_appear_tween() {
    let mut map = ARENA;
    map[10] = "#..............S...............#";
    let mut s = Sim::new(&map, &[Vec2::new(40.0, FLOOR_Y)]);
    let portals = enemies::portal::add_level_portals(&mut s.w, &s.c);
    s.step(&[NONE]);
    assert!(enemies::portal::appear(&mut s.w, portals[0]));
    s.steps(5, NONE);
    enemies::portal::spawn_enemy(&mut s.w, portals[0], EnemyType::Slime);
    let k = until(&mut s, 200, |s| !s.w.tagged(Tag::Enemy).is_empty()).expect("spawned");
    assert!((44..=50).contains(&k), "spawn after {k} ticks");
}

/// The "+1 queued" after a spawn lasts one tick less than the old port did:
/// it is gone by the second tick after the spawn.
#[test]
fn portal_add_counter_is_short() {
    let mut map = ARENA;
    map[10] = "#..............S...............#";
    let mut s = Sim::new(&map, &[Vec2::new(40.0, FLOOR_Y)]);
    let portals = enemies::portal::add_level_portals(&mut s.w, &s.c);
    s.step(&[NONE]);
    enemies::portal::appear_and_spawn(&mut s.w, portals[0], EnemyType::Slime);
    // Step until the queue is popped (the counter covers the gap).
    until(
        &mut s,
        200,
        |s| matches!(s.w.kind(portals[0]), Some(Kind::Portal(p)) if p.add_counter > 0.0),
    )
    .expect("spawned");
    assert_eq!(enemies::portal::queued_total(&s.w), 1);
    s.step(&[NONE]);
    assert_eq!(enemies::portal::queued_total(&s.w), 0);
}

/// Flashing only lets arrows through (§1.5): a shield skeleton that just
/// lost its shield still dies to a direct hurt (stomp, explosion, ...).
#[test]
fn flashing_enemy_still_hurt() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "SkeletonS", Vec2::new(200.0, 214.0), -1);
    s.step(&[NONE]);
    assert!(enemies::hurt_by(&mut s.w, &s.c, id, Vec2::ZERO, 1, 0));
    assert!(s.w.core(id).flash.flashing, "shield broke and flashes");
    assert!(enemy(&s, id).is_some_and(|e| e.alive()));
    enemies::hurt_by(&mut s.w, &s.c, id, Vec2::ZERO, 1, 0);
    assert!(
        enemy(&s, id).is_none_or(|e| !e.alive()),
        "killed while flashing"
    );
}

/// Only bats, ghosts and the flaming skull ignore jump-thrus (§1.1).
#[test]
fn jump_thru_flags() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    for (name, ignores) in [
        ("Bat", true),
        ("Ghost", true),
        ("FlamingSkull", true),
        ("Exploder", false),
        ("EvilCrystal", false),
        ("TechnoMage", false),
        ("Slime", false),
    ] {
        let id = spawn(&mut s, name, Vec2::new(200.0, 100.0), 1);
        assert_eq!(s.w.core(id).ignore_jump_thrus, ignores, "{name}");
    }
}

/// `HorizontalPlayer` tests for the player before stopping at a wall
/// (§1.8): an archer standing on a low step is found from the floor.
#[test]
fn horizontal_player_sees_onto_a_step() {
    let mut map = ARENA;
    map[21] = "#...................##.........#";
    let mut s = Sim::new(&map, &[Vec2::new(205.0, 190.0)]);
    s.steps(20, NONE);
    let p = s.pid(0).unwrap();
    assert!(s.pos(0).y < 210.0, "on the step: {:?}", s.pos(0));
    let found = enemies::horizontal_player(&s.w, &s.c, Vec2::new(102.0, 215.0), 1);
    assert_eq!(found, Some(p));
}
