//! King Reaper fidelity checks against docs/spec/enemies.md §4.13.

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::enemies::{self, hazard, reaper};
use tfp_sim::entity::{EntityId, Kind};
use tfp_sim::quiver::ArrowType;

fn enemy(s: &Sim, id: EntityId) -> Option<enemies::Enemy> {
    match s.w.kind(id) {
        Some(Kind::Enemy(e)) => Some(*e),
        _ => None,
    }
}

fn until(s: &mut Sim, max: usize, mut f: impl FnMut(&Sim) -> bool) -> Option<usize> {
    for k in 0..max {
        s.step(&[NONE]);
        if f(s) {
            return Some(k);
        }
    }
    None
}

fn summon(s: &mut Sim, diff: i32, final_wave: bool, from: Option<Vec2>) -> EntityId {
    let id = reaper::summon(&mut s.w, &s.c, diff, final_wave, from, false, 0);
    s.step(&[NONE]);
    id
}

fn in_attack(s: &mut Sim, id: EntityId) {
    assert!(
        until(s, 400, |s| enemy(s, id)
            .is_some_and(|e| e.state == reaper::ATTACK))
        .is_some()
    );
}

/// Hurt the boss for real (shield dropped first), writing it back.
fn hit(s: &mut Sim, id: EntityId, health: i8) {
    let mut e = enemy(s, id).unwrap();
    e.t[0] = 0.0;
    e.health = health;
    enemies::hurt(&mut s.w, &s.c, id, &mut e, Vec2::ZERO, 1, 0);
    if let Some(k) = s.w.kind_mut(id) {
        *k = Kind::Enemy(e);
    }
}

/// From the intro it enters Attack only once the transform animation ended.
#[test]
fn intro_waits_for_transform() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = summon(&mut s, 0, true, Some(Vec2::new(100.0, 100.0)));
    let k = until(&mut s, 400, |s| {
        enemy(s, id).is_some_and(|e| e.state == reaper::ATTACK)
    })
    .unwrap() as f32
        + 2.0;
    let a = enemies::anim_ticks(0.08, 56);
    assert!((k - a).abs() <= 2.0, "attack at {k}, animation {a}");
    assert!(!s.w.core(id).ignore_jump_thrus, "lands on jump-thrus");
}

/// Hurt: not collidable while hurt, back to Attack (collidable) after 60.
#[test]
fn hurt_is_not_collidable() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = summon(&mut s, 0, true, None);
    in_attack(&mut s, id);
    assert!(s.w.core(id).collidable);
    hit(&mut s, id, 3);
    assert_eq!(enemy(&s, id).unwrap().state, reaper::HURT);
    assert!(!s.w.core(id).collidable);
    let k = until(&mut s, 100, |s| {
        enemy(s, id).is_some_and(|e| e.state == reaper::ATTACK)
    })
    .unwrap();
    assert!((58..=63).contains(&k), "back after {k}");
    assert!(s.w.core(id).collidable);
    let e = enemy(&s, id).unwrap();
    assert!(reaper::shielded(&e));
}

/// A non-final death: removed when the 14-frame disappear animation ends.
#[test]
fn disappear_until_animation_ends() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = summon(&mut s, 0, false, None);
    in_attack(&mut s, id);
    hit(&mut s, id, 1);
    assert_eq!(enemy(&s, id).unwrap().state, reaper::DISAPPEAR);
    let k = until(&mut s, 200, |s| !s.w.exists(id)).unwrap() as f32 + 1.0;
    let a = enemies::anim_ticks(0.1, 14);
    assert!((k - a).abs() <= 2.0, "removed after {k}, animation {a}");
}

/// Crystals come out with a 14x14 target box; the boss fires lasers that
/// start at bounce count 7; a broken crystal lingers for its animation.
#[test]
fn crystals_fire_non_bouncing_lasers() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = summon(&mut s, 0, true, None);
    in_attack(&mut s, id);
    let crystals = |s: &Sim| -> Vec<EntityId> {
        s.w.entities()
            .filter(|&h| {
                matches!(s.w.kind(h), Some(Kind::Hazard(hz))
                    if hz.ty == hazard::HazardType::ReaperCrystal)
            })
            .collect()
    };
    until(&mut s, 400, |s| crystals(s).len() == 3).expect("three crystals");
    let cs = crystals(&s);
    assert!(s.w.core(cs[0]).target_collider.is_some());
    // Break one: harmless at once, gone after its animation.
    reaper::crystal_break(&mut s.w, cs[0]);
    assert!(!s.w.core(cs[0]).collidable);
    let k = until(&mut s, 60, |s| !s.w.exists(cs[0])).unwrap() as f32 + 1.0;
    let a = enemies::anim_ticks(0.08, 6);
    assert!((k - a).abs() <= 1.0, "break {k} vs {a}");
    let laser = until(&mut s, 200, |s| {
        s.arrows()
            .iter()
            .any(|(_, a)| a.arrow_type == ArrowType::Laser)
    });
    assert!(laser.is_some(), "lasers fired");
    let lasers: Vec<_> = s
        .arrows()
        .into_iter()
        .filter(|(_, a)| a.arrow_type == ArrowType::Laser)
        .collect();
    assert_eq!(lasers.len(), 2, "the broken crystal does not fire");
    assert!(lasers.iter().all(|(_, a)| a.extra.bounced >= 7));
}

/// A bomb at rest swells for 20 ticks (starting the tick after) and blows.
#[test]
fn bomb_swell_timing() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let owner = summon(&mut s, 0, true, None);
    let b = hazard::spawn_bomb(&mut s.w, Vec2::new(200.0, 60.0), Vec2::ZERO, owner);
    let k = until(&mut s, 60, |s| {
        !s.w.exists(b) || s.w.core(b).marked_for_removal
    })
    .unwrap();
    assert_eq!(k, 20, "explodes on its 21st update");
    assert!(!s.w.core(owner).ignore_jump_thrus);
}
