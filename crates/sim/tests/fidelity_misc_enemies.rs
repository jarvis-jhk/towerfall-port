//! Fidelity checks for the flaming skull, evil crystals, exploder, worm and
//! techno mage against docs/spec/enemies.md §4.8–§4.12.

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::enemies::{self, EnemyType, hazard::HazardType};
use tfp_sim::entity::{Allegiance, EntityId, Kind};
use tfp_sim::events::Event;
use tfp_sim::quiver::ArrowType;

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

fn step(s: &mut Sim) {
    s.step(&[NONE, NONE]);
}

fn until(s: &mut Sim, max: usize, mut f: impl FnMut(&Sim) -> bool) -> Option<usize> {
    for k in 0..max {
        step(s);
        if f(s) {
            return Some(k);
        }
    }
    None
}

fn sounds(s: &Sim, name: &str) -> usize {
    s.w.events()
        .iter()
        .filter(|e| matches!(e, Event::Sound { name: n, .. } if *n == name))
        .count()
}

fn missiles(s: &Sim) -> Vec<EntityId> {
    s.w.entities()
        .filter(
            |&e| matches!(s.w.kind(e), Some(Kind::Hazard(h)) if h.ty == HazardType::TechnoMissile),
        )
        .collect()
}

/// §4.8: a horizontal wall hit squashes x to 0.5, which relaxes back at
/// 0.05/tick; facing turns with `vx` at the wall.
#[test]
fn skull_squash_relaxes_and_turns_at_wall() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "FlamingSkull", Vec2::new(240.0, 60.0), 1);
    let hit = until(&mut s, 300, |s| {
        enemy(s, id).is_some_and(|e| e.speed.x < 0.0)
    });
    assert!(hit.is_some(), "wall bounce");
    let e = enemy(&s, id).unwrap();
    assert_eq!(e.facing, -1);
    assert!((e.scale.x - 0.55).abs() < 1e-4, "squash {}", e.scale.x);
    until(&mut s, 9, |_| false);
    assert!((enemy(&s, id).unwrap().scale.x - 1.0).abs() < 1e-4);
    assert!((enemy(&s, id).unwrap().speed.length() - 1.0).abs() < 1e-4);
}

/// §4.8: the dead skull is removed on the 30th update after dying.
#[test]
fn skull_removed_after_30() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "FlamingSkull", Vec2::new(200.0, 60.0), 1);
    step(&mut s);
    step(&mut s);
    enemies::hurt_by(&mut s.w, &s.c, id, Vec2::ZERO, 1, 0);
    assert_eq!(enemy(&s, id).unwrap().state, enemies::skull::DEAD);
    let gone = until(&mut s, 40, |s| {
        !s.w.exists(id) || s.w.core(id).marked_for_removal
    });
    assert_eq!(gone, Some(29));
}

/// §4.9: a red crystal killed in the open fires four posthumous lasers,
/// each with the fire sound, credited to the killer.
#[test]
fn crystal_death_lasers() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "EvilCrystal", Vec2::new(200.0, 60.0), 1);
    step(&mut s);
    enemies::hurt_by(&mut s.w, &s.c, id, Vec2::new(1.0, 0.0), 1, 0);
    let mut fired = 0;
    let mut sfx = 0;
    for _ in 0..80 {
        step(&mut s);
        sfx += sounds(&s, "en_crystalFire");
        fired = s
            .arrows()
            .iter()
            .filter(|(_, a)| a.owner == Some(id) && a.arrow_type == ArrowType::Laser)
            .inspect(|(_, a)| assert_eq!(a.player_index, 0))
            .count()
            .max(fired);
    }
    assert_eq!(fired, 4);
    assert_eq!(sfx, 4);
    assert!(enemy(&s, id).is_none(), "removed");
}

/// §4.9: the blue crystal's second volley is one shot (one sound) of two
/// lasers, three lasers in all.
#[test]
fn blue_crystal_double_volley() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(200.0, FLOOR_Y)]);
    let id = spawn(&mut s, "BlueCrystal", Vec2::new(160.0, 160.0), 1);
    let mut sfx = 0;
    let mut most = 0;
    for _ in 0..120 {
        step(&mut s);
        sfx += sounds(&s, "en_crystalFire");
        let n = s
            .arrows()
            .iter()
            .filter(|(_, a)| a.owner == Some(id))
            .count();
        most = most.max(n);
        if most == 3 {
            break;
        }
    }
    assert_eq!(most, 3);
    assert_eq!(sfx, 2);
}

/// §4.10: facing follows `vx` while alive.
#[test]
fn exploder_faces_its_motion() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Exploder", Vec2::new(200.0, 100.0), 1);
    // Chasing the archer on the left.
    let left = until(&mut s, 200, |s| {
        enemy(s, id).is_some_and(|e| e.speed.x < -0.1)
    });
    assert!(left.is_some());
    assert_eq!(enemy(&s, id).unwrap().facing, -1);
}

/// §4.10: killed, it explodes after its 30-tick fuse with `blowfishDeath2`.
#[test]
fn exploder_dead_fuse() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Exploder", Vec2::new(200.0, 100.0), 1);
    step(&mut s);
    enemies::hurt_by(&mut s.w, &s.c, id, Vec2::ZERO, 1, 0);
    let mut boom = None;
    for k in 0..40 {
        step(&mut s);
        if sounds(&s, "en_blowfishDeath2") > 0 {
            boom = Some(k);
            break;
        }
    }
    // `yield return 30` from the coroutine's first run: the 32nd update.
    assert_eq!(boom, Some(31));
}

/// §4.11: falling, the worm's body turns back upright and `vy` is capped at
/// once to 1.8.
#[test]
fn worm_fall_caps_speed() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Worm", Vec2::new(250.0, 40.0), 1);
    step(&mut s);
    if let Some(Kind::Enemy(e)) = s.w.kind_mut(id) {
        e.speed = Vec2::new(0.0, 4.0);
    }
    step(&mut s);
    assert!(enemy(&s, id).unwrap().speed.y <= 1.8);
}

/// §4.12: missiles home on the mage's target and are seek-able at priority
/// 0 with neutral allegiance.
#[test]
fn techno_missile_target_and_seek() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(160.0, FLOOR_Y)]);
    spawn(&mut s, "TechnoMage", Vec2::new(140.0, 175.0), 1);
    assert!(until(&mut s, 200, |s| !missiles(s).is_empty()).is_some());
    let m = missiles(&s)[0];
    let core = s.w.core(m);
    assert_eq!(core.seek_priority, 0);
    assert_eq!(core.allegiance, Allegiance::Neutral);
    match s.w.kind(m) {
        Some(Kind::Hazard(h)) => assert_eq!(h.target, s.pid(0)),
        _ => unreachable!(),
    }
}

/// §4.12: a killed mage cannot be touched (the base death leaves it
/// non-collidable); it explodes 60 ticks later.
#[test]
fn techno_dead_body_fuse() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "TechnoMage", Vec2::new(200.0, 100.0), 1);
    step(&mut s);
    enemies::hurt_by(&mut s.w, &s.c, id, Vec2::ZERO, 1, 0);
    assert!(!s.w.core(id).collidable);
    let mut boom = None;
    for k in 0..70 {
        step(&mut s);
        if sounds(&s, "en_technoMageExplode") > 0 {
            boom = Some(k);
            break;
        }
    }
    assert_eq!(boom, Some(61));
}
