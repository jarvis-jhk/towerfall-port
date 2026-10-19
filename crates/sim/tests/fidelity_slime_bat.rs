//! Fidelity checks for the slime and bat family (enemies.md §4.1, §4.2).

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::enemies::{self, EnemyType};
use tfp_sim::entity::{EntityId, Kind};

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

/// The jump wind-up: wait(5), squash steps of 0.1 tested before each step
/// (1.0 -> 0.6 in four), wait(5), then the jump: 17 ticks in Jump, and the
/// scale does not relax back up meanwhile.
#[test]
fn slime_jump_windup_timing() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(200.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Slime", Vec2::new(40.0, 220.0), 1);
    // Walk until it winds up (the scale has relaxed to 1 by then).
    until(&mut s, 600, |s| {
        enemy(s, id).is_some_and(|e| e.state == enemies::slime::JUMP)
    })
    .expect("winds up");
    assert_eq!(enemy(&s, id).unwrap().scale.y, 1.0);
    let mut min_y = 1.0f32;
    let n = until(&mut s, 60, |s| {
        let e = enemy(s, id).unwrap();
        if e.state == enemies::slime::JUMP {
            assert!(e.scale.y <= min_y, "no relax during the wind-up");
        }
        min_y = min_y.min(e.scale.y);
        e.state != enemies::slime::JUMP
    })
    .expect("jumps");
    assert_eq!(n + 1, 17, "ticks in Jump");
    let e = enemy(&s, id).unwrap();
    assert_eq!(e.state, enemies::slime::AIR);
    assert!((e.speed.y - -3.5).abs() < 0.2, "{:?}", e.speed);
}

/// Landing on a ledge edge with nothing behind: a raw shift off the edge,
/// still falling (vy kept), down to the floor.
#[test]
fn slime_slides_off_ledge_still_falling() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(30.0, FLOOR_Y)]);
    // Hitbox 149..165 overlaps the block (100..150) by one pixel; facing
    // left, the test "behind" is to the right, off the block.
    let id = spawn(&mut s, "Slime", Vec2::new(157.0, 115.0), -1);
    until(&mut s, 60, |s| s.w.core(id).pos.x >= 159.0).expect("slid off the edge");
    let e = enemy(&s, id).unwrap();
    assert!(e.speed.y > 0.0, "still falling: {:?}", e.speed);
    assert_ne!(e.state, enemies::slime::LAND);
    until(&mut s, 120, |s| s.w.core(id).pos.y >= 220.0).expect("reached the floor");
}

/// A super bomb bat touched by an archer does not explode (only the normal
/// bomb bat does); the touch shoves it away.
#[test]
fn only_bomb_bat_explodes_on_touch() {
    for (name, survives) in [("SuperBombBat", true), ("BombBat", false)] {
        let mut s = Sim::new(&ARENA, &[Vec2::new(100.0, FLOOR_Y)]);
        let id = spawn(&mut s, name, Vec2::new(100.0, 206.0), 1);
        for _ in 0..2 {
            s.step(&[NONE]);
        }
        assert_eq!(s.w.exists(id), survives, "{name}");
        if survives {
            assert!(enemy(&s, id).unwrap().alive());
        }
    }
}

/// A dead bat stops ignoring jump-thrus: its body lands on the platform.
#[test]
fn dead_bat_lands_on_jump_thru() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Bat", Vec2::new(285.0, 160.0), 1);
    s.step(&[NONE]);
    assert!(enemies::hurt_by(&mut s.w, &s.c, id, Vec2::ZERO, 1, -1));
    until(&mut s, 60, |_| false);
    let e = enemy(&s, id).expect("body lingers");
    assert_eq!(e.state, enemies::bat::DEAD);
    let y = s.w.core(id).pos.y;
    assert!((175.0..180.0).contains(&y), "rests on the jump-thru: {y}");
    assert!(s.w.core(id).collidable);
}

/// A bat faces its target once per pass (wind-up), never during the swoop.
#[test]
fn bat_facing_fixed_during_swoop() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(30.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Bat", Vec2::new(60.0, 170.0), 1);
    until(&mut s, 200, |s| {
        enemy(s, id).is_some_and(|e| e.state == enemies::bat::SWOOP)
    })
    .expect("swoops");
    s.step(&[NONE]);
    let facing = enemy(&s, id).unwrap().facing;
    assert_eq!(facing, -1, "faces the archer");
    until(&mut s, 40, |s| {
        let Some(e) = enemy(s, id) else { return true };
        if e.state != enemies::bat::SWOOP {
            return true;
        }
        assert_eq!(e.facing, facing, "facing fixed during the pass");
        false
    })
    .expect("swoop ends");
}
