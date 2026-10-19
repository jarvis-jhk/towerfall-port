//! Ghost family and cultist details re-checked against the reference
//! (docs/spec/enemies.md §4.3, §4.4).

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::enemies::{self, EnemyType};
use tfp_sim::entity::{EntityId, Kind};
use tfp_sim::events::Event;

fn enemy(s: &Sim, id: EntityId) -> Option<enemies::Enemy> {
    match s.w.kind(id) {
        Some(Kind::Enemy(e)) => Some(*e),
        _ => None,
    }
}

fn enemy_mut(s: &mut Sim, id: EntityId) -> &mut enemies::Enemy {
    match s.w.kind_mut(id) {
        Some(Kind::Enemy(e)) => e,
        _ => panic!("not an enemy"),
    }
}

fn spawn_with(s: &mut Sim, name: &str, pos: Vec2, facing: i8, nodes: &[Vec2]) -> EntityId {
    let ty = EnemyType::from_name(name).unwrap();
    enemies::spawn(&mut s.w, &s.c, ty, pos, facing, nodes)
}

fn spawn(s: &mut Sim, name: &str, pos: Vec2, facing: i8) -> EntityId {
    spawn_with(s, name, pos, facing, &[])
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

fn sound(s: &Sim, name: &str) -> bool {
    s.w.events()
        .iter()
        .any(|e| matches!(*e, Event::Sound { name: n, .. } if n == name))
}

/// The arena with a one-tile step on the floor at x = 200..210.
const STEP: [&str; 24] = {
    let mut m = ARENA;
    m[21] = "#...................#..........#";
    m
};

/// A cultist walking into a full-height wall turns around: its hitbox, put
/// 4 px ahead and 12 px up, still overlaps the wall (no hop).
#[test]
fn cultist_turns_at_tall_wall() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Cultist", Vec2::new(280.0, 219.0), 1);
    let mut hopped = false;
    let turned = until(&mut s, 200, |s| {
        let e = enemy(s, id).unwrap();
        hopped |= e.state == enemies::cultist::JUMP;
        e.facing == -1
    });
    assert!(turned.is_some(), "turned at the wall");
    assert!(!hopped, "no hop at a tall wall");
}

/// A cultist hops a 10 px step.
#[test]
fn cultist_hops_low_step() {
    let mut s = Sim::new(&STEP, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Cultist", Vec2::new(170.0, 219.0), 1);
    assert!(
        until(&mut s, 200, |s| enemy(s, id)
            .is_some_and(|e| e.state == enemies::cultist::JUMP))
        .is_some(),
        "hop"
    );
    assert!(
        until(&mut s, 200, |s| s.w.core(id).pos.x > 212.0).is_some(),
        "past the step: {:?}",
        s.w.core(id).pos
    );
}

/// Falling speed is capped at 1.8 at once, not eased down by 0.15.
#[test]
fn cultist_fall_cap() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Cultist", Vec2::new(250.0, 60.0), 1);
    s.step(&[NONE]);
    enemy_mut(&mut s, id).speed.y = 4.0;
    s.step(&[NONE]);
    assert_eq!(enemy(&s, id).unwrap().speed.y, 1.8);
}

/// Landing plays the cultist's landing sound and squashes the sprite.
#[test]
fn cultist_lands() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Cultist", Vec2::new(250.0, 200.0), 1);
    assert!(until(&mut s, 60, |s| sound(s, "en_cultLand")).is_some());
    assert!(enemy(&s, id).unwrap().scale.y < 1.0);
}

/// The scythe's side slash plays its sound with the attack box.
#[test]
fn scythe_side_slash_sound() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(185.0, FLOOR_Y)]);
    spawn(&mut s, "ScytheCultist", Vec2::new(155.0, 219.0), 1);
    assert!(until(&mut s, 120, |s| sound(s, "en_cultScySlashSide")).is_some());
}

/// Every ghost starts at alpha 2 (opaque, outlined); only the green ghost
/// fades as an archer comes near.
#[test]
fn green_ghost_fades_near_archer() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(150.0, FLOOR_Y)]);
    let g = spawn(&mut s, "GreenGhost", Vec2::new(150.0, 190.0), 1);
    let b = spawn(&mut s, "Ghost", Vec2::new(170.0, 190.0), 1);
    s.steps(20, NONE);
    let g = enemy(&s, g).unwrap();
    let b = enemy(&s, b).unwrap();
    assert!(g.alpha < 1.8, "green fades: {}", g.alpha);
    assert_eq!(b.alpha, 2.0);
}

/// A patrol node already within 10 px when it becomes current is passed at
/// once (no 5-tick wait aimed at it).
#[test]
fn ghost_patrol_skips_reached_node() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(20.0, FLOOR_Y)]);
    let nodes = [
        Vec2::new(200.0, 50.0),
        Vec2::new(199.0, 50.0),
        Vec2::new(280.0, 50.0),
    ];
    let id = spawn_with(&mut s, "Ghost", Vec2::new(150.0, 50.0), 1, &nodes);
    let mut saw_middle = false;
    let idle = until(&mut s, 600, |s| {
        let e = enemy(s, id).unwrap();
        saw_middle |= e.state == enemies::ghost::PATROL && e.n[1] == 1;
        e.state == enemies::ghost::IDLE
    });
    assert!(idle.is_some(), "finished the patrol");
    assert!(!saw_middle, "lingered on the reached node");
    assert!(s.w.core(id).pos.x > 265.0);
}
