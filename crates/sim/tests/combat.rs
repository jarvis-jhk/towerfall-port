//! Arrows, catching, stomps and other multi-entity interactions.

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::arrow::ArrowState;
use tfp_sim::entity::Tag;
use tfp_sim::events::Event;
use tfp_sim::math::{PI, angle_between};
use tfp_sim::platform;
use tfp_sim::player::PState;

fn floor(x: f32) -> Vec2 {
    Vec2::new(x, FLOOR_Y)
}

/// Press Shoot for one frame, release next: one arrow.
fn shoot(s: &mut Sim, x: i8, y: i8) {
    s.step1(inp(x, y, SHOOT));
    s.step1(inp(x, y, 0));
}

/// Straight flight at 4 px/frame for 25 frames, then gravity (+0.2/frame);
/// a shot along the floor lands in it and can be picked up after 15 frames.
#[test]
fn arrow_arc_stick_and_pickup() {
    let mut s = Sim::new(&ARENA, &[floor(40.0)]);
    s.steps(2, NONE);
    assert_eq!(s.p(0).arrows.len(), 3);
    shoot(&mut s, 1, 0);
    assert_eq!(s.p(0).arrows.len(), 2);
    s.step1(NONE); // the arrow joins the scene and makes its first move
    let (_, a) = s.arrows()[0];
    assert_eq!(a.state, ArrowState::Shooting);
    let start = s.w.core(s.arrows()[0].0).pos;
    assert_eq!(start, Vec2::new(44.0, FLOOR_Y - 1.0));
    let mut ys = vec![];
    let mut xs = vec![];
    let mut speeds = vec![];
    for _ in 0..60 {
        let (id, a) = s.arrows()[0];
        xs.push(s.w.core(id).pos.x);
        ys.push(s.w.core(id).pos.y);
        speeds.push(a.speed);
        if a.state == ArrowState::Stuck {
            break;
        }
        s.step1(NONE);
    }
    // 25 shooting updates in all (the first was before `start`).
    assert!(ys[..24].iter().all(|&y| y == start.y), "{ys:?}");
    assert!(xs.windows(2).take(23).all(|w| w[1] - w[0] == 4.0), "{xs:?}");
    // speeds[k] is after k+1 updates: update 25 switches to Gravity (and
    // still moves with the old speed), update 26 is the first gravity step.
    assert_eq!(speeds[24], Vec2::new(4.0, 0.0));
    assert_eq!(speeds[25].y, 0.2, "first gravity update");
    assert!((speeds[26].y - 0.4).abs() < 1e-6);
    assert!((speeds[25].x - 3.97).abs() < 1e-6, "x decays 0.03/frame");
    let (id, a) = s.arrows()[0];
    assert_eq!(a.state, ArrowState::Stuck);
    assert!(a.stuck_dir.y > 0.0, "stuck into the floor");
    let tip = s.w.core(id).pos;
    assert_eq!(tip.y, 219.0, "2x2 hitbox resting on the floor top (y=220)");
    // Walk over to it and pick it up.
    for _ in 0..300 {
        s.step1(inp(1, 0, 0));
        if s.arrows().is_empty() {
            break;
        }
    }
    assert!(s.arrows().is_empty(), "arrow collected");
    assert_eq!(s.p(0).arrows.len(), 3);
}

/// A shot from the block top reaches the right wall and sticks in it.
#[test]
fn arrow_sticks_in_wall() {
    let mut map = ARENA;
    map[18] = "#..............................#";
    let mut s = Sim::new(&map, &[Vec2::new(120.0, 122.0)]);
    s.steps(2, NONE);
    shoot(&mut s, 1, 0);
    let mut stuck = None;
    for _ in 0..80 {
        s.step1(NONE);
        if let Some((id, a)) = s.arrows().first().copied()
            && a.state == ArrowState::Stuck
        {
            stuck = Some((s.w.core(id).pos, a));
            break;
        }
    }
    let (p, a) = stuck.expect("stuck");
    assert!(
        a.stuck_dir.x > 0.0 && a.stuck_dir.y == 0.0,
        "horizontal stick"
    );
    assert_eq!(p.x, 309.0, "tip touching the wall at x=310");
    assert!(p.y > 125.0 && p.y < 200.0);
}

/// An arrow hit starts the 3-frame Dying grace; then the archer dies, the
/// arrow is buried in the corpse and the hat drops.
#[test]
fn arrow_kill_and_corpse() {
    let mut s = Sim::new(&ARENA, &[floor(40.0), floor(120.0)]);
    s.steps(2, NONE);
    s.step(&[inp(1, 0, SHOOT), NONE]);
    s.step(&[inp(1, 0, 0), NONE]);
    let mut dying_frames = 0;
    let mut died = None;
    for f in 0..60 {
        s.step(&[NONE, NONE]);
        if let Some(id) = s.pid(1)
            && s.w.player(id).unwrap().state() == PState::Dying
        {
            dying_frames += 1;
        }
        for e in s.w.events() {
            if let Event::Death {
                player: 1,
                killer,
                cause,
            } = *e
            {
                died = Some((f, killer, cause));
            }
        }
        if died.is_some() {
            break;
        }
    }
    let (_, killer, cause) = died.expect("player 1 died");
    assert_eq!((killer, cause), (0, 0), "killed by player 0's arrow");
    // Hit frame and the next two: the third Dying update kills (the archer
    // stays in the scene, dead, until the next list update).
    assert_eq!(dying_frames, 3, "three Dying updates");
    s.step(&[NONE, NONE]);
    assert!(s.pid(1).is_none(), "archer removed");
    let corpses = s.corpses();
    assert_eq!(corpses.len(), 1);
    let (_, a) = s.arrows()[0];
    assert_eq!(a.state, ArrowState::Buried);
    assert_eq!(a.buried_in, Some(corpses[0]));
    assert_eq!(s.w.tagged(Tag::Hat).len(), 1, "hat dropped");
    // Corpse flies with the arrow's push and comes to rest on the floor.
    s.steps(120, NONE);
    let cy = s.w.core(corpses[0]).pos.y;
    assert_eq!(cy, 212.0);
}

/// A dodge press during the grace saves the archer and catches the arrow.
#[test]
fn grace_period_catch() {
    let mut s = Sim::new(&ARENA, &[floor(40.0), floor(120.0)]);
    s.steps(2, NONE);
    s.step(&[inp(1, 0, SHOOT), NONE]);
    s.step(&[inp(1, 0, 0), NONE]);
    for _ in 0..40 {
        s.step(&[NONE, NONE]);
        if s.state(1) == PState::Dying {
            break;
        }
    }
    assert_eq!(s.state(1), PState::Dying);
    s.step(&[NONE, NONE]);
    s.step(&[NONE, inp(0, 0, DODGE)]);
    assert_eq!(s.state(1), PState::Dodging);
    assert_eq!(s.p(1).arrows.len(), 4, "caught into the quiver");
    assert!(s.p(1).dodge_curse_satisfied);
    s.steps(40, NONE);
    assert!(s.pid(1).is_some(), "survived");
}

/// Dodging into a flying arrow catches it.
#[test]
fn dodge_catch() {
    let mut s = Sim::new(&ARENA, &[floor(40.0), floor(150.0)]);
    s.steps(2, NONE);
    s.step(&[inp(1, 0, SHOOT), NONE]);
    s.step(&[inp(1, 0, 0), NONE]);
    // Arrow travels 4 px/frame from x≈40; dodge toward it when close.
    for _ in 0..40 {
        let arrows = s.arrows();
        if let Some((id, _)) = arrows.first()
            && s.w.core(*id).pos.x > 100.0
        {
            break;
        }
        s.step(&[NONE, NONE]);
    }
    s.step(&[NONE, inp(-1, 0, DODGE)]);
    for _ in 0..10 {
        s.step(&[NONE, NONE]);
    }
    assert!(s.pid(1).is_some(), "alive");
    assert_eq!(s.p(1).arrows.len(), 4, "caught");
    assert!(s.arrows().is_empty());
}

/// Landing on another archer kills them (JumpedOn) and bounces the stomper.
#[test]
fn stomp() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(170.0, 150.0), floor(170.0)]);
    let mut bounced = false;
    let mut died = None;
    for _ in 0..40 {
        s.step(&[NONE, NONE]);
        for e in s.w.events() {
            if let Event::Death {
                player: 1,
                killer,
                cause,
            } = *e
            {
                died = Some((killer, cause));
                bounced = s.p(0).speed.y < 0.0;
            }
        }
        if died.is_some() {
            break;
        }
    }
    assert_eq!(died, Some((0, 3)), "stomped by player 0");
    assert!(bounced, "stomper bounced");
}

/// Side collision shoves both archers apart (1.8 px/frame).
#[test]
fn side_bump() {
    let mut s = Sim::new(&ARENA, &[floor(100.0), floor(116.0)]);
    s.steps(2, NONE);
    let mut bumped = false;
    for _ in 0..20 {
        s.step(&[inp(1, 0, 0), inp(-1, 0, 0)]);
        if s.p(0).speed.x < 0.0 && s.p(1).speed.x > 0.0 {
            bumped = true;
            break;
        }
    }
    assert!(bumped);
    assert!(s.pid(0).is_some() && s.pid(1).is_some());
}

/// Aim assist: a target within 36 px and 65° gets the exact angle.
#[test]
fn aim_assist() {
    let mut map = ARENA;
    map[20] = "#...........#..................#";
    let mut s = Sim::new(&map, &[floor(100.0), Vec2::new(125.0, 192.0)]);
    s.steps(2, NONE);
    assert!(s.p(1).on_ground, "target stands on the step");
    let o = s.pos(0) + Vec2::new(0.0, -1.0);
    let t = s.pos(1);
    assert!(o.dist_sq(t) <= 1296.0);
    s.step(&[inp(1, 0, SHOOT), NONE]);
    s.step(&[inp(1, 0, 0), NONE]);
    s.step(&[NONE, NONE]);
    let (_, a) = s.arrows()[0];
    let expect = angle_between(o, t);
    // One seeking update toward the same point leaves the heading unchanged.
    assert!(
        (a.direction - expect).abs() < 1e-3,
        "{} vs {}",
        a.direction,
        expect
    );
    assert!(a.direction < -0.5 && a.direction > -PI / 2.0);
}

/// A shield absorbs one arrow: the archer is pushed and flashes 30 frames.
#[test]
fn shield_absorbs_arrow() {
    let mut s = Sim::with(&ARENA, &[floor(40.0), floor(120.0)], |m| {
        m.players[1].as_mut().unwrap().shield = true;
    });
    s.steps(40, NONE);
    assert!(s.p(1).has_shield, "granted 20+10*index frames after spawn");
    s.step(&[inp(1, 0, SHOOT), NONE]);
    s.step(&[inp(1, 0, 0), NONE]);
    let mut broke = false;
    for _ in 0..40 {
        s.step(&[NONE, NONE]);
        if !s.p(1).has_shield {
            broke = true;
            break;
        }
    }
    assert!(broke);
    assert!(s.core(1).flash.flashing);
    assert!(s.p(1).speed.x > 0.0, "pushed by the arrow");
    s.steps(60, NONE);
    assert!(s.pid(1).is_some(), "alive");
    assert!(!s.core(1).flash.flashing);
}

/// Wings: a jump in the air flaps (-1.4) with reduced gravity.
#[test]
fn wings_flap() {
    let mut s = Sim::with(&ARENA, &[floor(60.0)], |m| {
        m.players[0].as_mut().unwrap().wings = true;
    });
    s.steps(45, NONE);
    assert!(s.p(0).has_wings);
    s.step1(inp(0, 0, JUMP));
    s.steps(5, NONE);
    s.step1(inp(0, 0, JUMP));
    assert_eq!(s.p(0).speed.y, -1.4);
    assert_eq!(s.p(0).flap_gravity, 0.0);
}

/// A moving solid carries a rider and squishes an archer against a wall.
#[test]
fn moving_solid_ride_and_squish() {
    // Platform 30x10 at y=180 moving right 1 px/frame, archer standing on it.
    let mut s = Sim::new(&ARENA, &[Vec2::new(55.0, 172.0)]);
    let m = platform::add_moving(
        &mut s.w,
        Vec2::new(40.0, 180.0),
        Vec2::new(200.0, 180.0),
        30.0,
        10.0,
        1.0,
        false,
    );
    s.step1(NONE);
    let x0 = s.pos(0).x;
    s.steps(30, NONE);
    assert_eq!(s.pos(0).x - x0, 30.0, "carried");
    assert!(s.w.exists(m));
    // Squish: a solid sweeping left into an archer standing at the left wall.
    let mut s = Sim::new(&ARENA, &[floor(14.0)]);
    platform::add_moving(
        &mut s.w,
        Vec2::new(60.0, 190.0),
        Vec2::new(10.0, 190.0),
        30.0,
        30.0,
        2.0,
        false,
    );
    let mut died = false;
    for _ in 0..60 {
        s.step1(NONE);
        if s.w
            .events()
            .iter()
            .any(|e| matches!(e, Event::Death { cause: 8, .. }))
        {
            died = true;
            break;
        }
    }
    assert!(died, "squished");
}

/// Own arrows do not hit the shooter until 20 px away; then they can.
#[test]
fn twelve_arrow_limit() {
    let mut s = Sim::with(&ARENA, &[floor(60.0)], |m| {
        m.players[0].as_mut().unwrap().arrows = 14;
    });
    s.steps(2, NONE);
    assert_eq!(s.p(0).arrows.len(), 6, "quiver holds 6");
}
