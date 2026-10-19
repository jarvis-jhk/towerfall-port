//! Birdman, tornado and skeleton details checked against
//! docs/spec/enemies.md §4.5 and §4.6.

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::enemies::{self, EnemyType, hazard};
use tfp_sim::entity::{EntityId, Kind, Tag};
use tfp_sim::events::Event;

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

fn sound(s: &Sim, name: &str) -> bool {
    s.w.events()
        .iter()
        .any(|e| matches!(*e, Event::Sound { name: n, .. } if n == name))
}

/// The flap squash relaxes back to 1 at 0.05/tick (it used to stay).
#[test]
fn birdman_flap_squash_relaxes() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Birdman", Vec2::new(200.0, 60.0), 1);
    let flap = until(&mut s, 400, |s| sound(s, "en_birdmanWingflap"));
    assert!(flap.is_some(), "flapped");
    assert!(enemy(&s, id).unwrap().scale.x > 1.15);
    s.steps(5, NONE);
    let e = enemy(&s, id).unwrap();
    assert!(
        (e.scale.x - 0.95).abs() < 0.2 && e.scale.x < 1.2,
        "{:?}",
        e.scale
    );
    s.steps(5, NONE);
    assert_eq!(enemy(&s, id).unwrap().scale, Vec2::ONE);
}

/// A tornado never enters a wall (the step is undone and the direction
/// flips), stays dangerous until it slows below 0.6 and fades from the
/// tick after.
#[test]
fn tornado_bounces_and_fades() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(250.0, FLOOR_Y)]);
    let t = hazard::spawn_tornado(&mut s.w, Vec2::new(20.0, 100.0), Vec2::new(-1.0, 0.0), None);
    let mut flipped = false;
    let mut off_at = None;
    let mut gone_at = None;
    for k in 1..=80 {
        s.step(&[NONE]);
        if !s.w.exists(t) {
            gone_at = Some(k);
            break;
        }
        let p = s.w.core(t).pos;
        assert!(
            !s.w.check_at(&s.c, t, p, Tag::Solid),
            "inside a wall at {p:?}"
        );
        if let Some(Kind::Hazard(h)) = s.w.kind(t) {
            flipped |= h.dir.x > 0.0;
        }
        if off_at.is_none() && !s.w.core(t).collidable {
            off_at = Some(k);
        }
    }
    assert!(flipped, "bounced off the wall");
    // 2.8 - 0.05 k < 0.6 first at k = 45; 12-13 fade ticks after that.
    let off = off_at.unwrap();
    assert!((44..=46).contains(&off), "non-collidable at {off}");
    let gone = gone_at.unwrap();
    assert!((off + 11..=off + 14).contains(&gone), "removed at {gone}");
}

/// A skeleton jumps toward an archer above with its jump cue.
#[test]
fn skeleton_jump_cue() {
    // Archer on the block's right end, in clear sight of the skeleton.
    let mut s = Sim::new(&ARENA, &[Vec2::new(145.0, 120.0)]);
    let id = spawn(&mut s, "Skeleton", Vec2::new(215.0, FLOOR_Y), -1);
    if let Some(Kind::Enemy(e)) = s.w.kind_mut(id) {
        e.arrows = Default::default();
    }
    let jumped = until(&mut s, 300, |s| {
        if std::env::var("DBG").is_ok() {
            let e = enemy(s, id).unwrap();
            eprintln!(
                "{} {:?} {:?} {:?}",
                e.state,
                s.w.core(id).pos,
                e.speed,
                s.pos(0)
            );
        }
        sound(s, "en_skelJump")
    });
    assert!(jumped.is_some(), "jump cue");
    assert!(enemy(&s, id).unwrap().speed.y < 0.0);
}

/// Landing plays the land cue and squashes the body, which relaxes.
#[test]
fn skeleton_land_cue_and_squash() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Skeleton", Vec2::new(250.0, 190.0), 1);
    let landed = until(&mut s, 60, |s| sound(s, "en_skelLand"));
    assert!(landed.is_some(), "land cue");
    assert_eq!(enemy(&s, id).unwrap().scale, Vec2::new(1.2, 0.8));
    s.steps(6, NONE);
    assert_eq!(enemy(&s, id).unwrap().scale, Vec2::ONE);
}

/// An arrow flying at a skeleton's back makes it dodge toward the arrow
/// (facing follows the dodge velocity) with the archers' dash cue.
#[test]
fn skeleton_dodge_faces_arrow() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Skeleton", Vec2::new(250.0, FLOOR_Y), 1);
    s.steps(2, NONE);
    let pid = s.pid(0).unwrap();
    let a = enemies::shoot_arrow(
        &mut s.w,
        pid,
        tfp_sim::quiver::ArrowType::Normal,
        Vec2::new(215.0, FLOOR_Y - 2.0),
        0.0,
    );
    if let Some(ar) = s.w.arrow_mut(a) {
        ar.owner = None;
        ar.cannot_hit = None;
        ar.cannot_hit_enemies = 0.0;
    }
    let dodged = until(&mut s, 20, |s| {
        enemy(s, id).is_some_and(|e| e.state == enemies::skeleton::DODGE)
    });
    assert!(dodged.is_some(), "dodged");
    let e = enemy(&s, id).unwrap();
    assert_eq!(e.facing, -1, "faces the dodge direction");
    assert!(sound(&s, "char_dashL"), "dash cue");
}
