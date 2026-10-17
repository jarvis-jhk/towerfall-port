//! Scripted-input checks of the archer against the numbers in
//! docs/spec/player.md.

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::player::PState;

fn on_floor(x: f32) -> Vec2 {
    Vec2::new(x, FLOOR_Y)
}

/// Full (held) jump: rises while gravity is halved; apex ≈ Σ(3.2 − 0.15k).
#[test]
fn jump_apex_held_and_tapped() {
    let mut s = Sim::new(&ARENA, &[on_floor(60.0)]);
    s.steps(3, NONE);
    assert!(s.p(0).on_ground);
    let y0 = s.pos(0).y;
    let mut min_y = y0;
    let mut frames_up = 0;
    for f in 0..80 {
        s.step1(inp(0, 0, JUMP));
        let y = s.pos(0).y;
        if y < min_y {
            min_y = y;
            frames_up = f + 1;
        }
    }
    let held = y0 - min_y;
    // 3.2 + 3.05 + ... (22 rising frames) = 35.75 px.
    assert!((35.0..=36.0).contains(&held), "held apex {held}");
    assert!((21..=23).contains(&frames_up), "rising frames {frames_up}");
    assert!(s.p(0).on_ground, "landed again");

    // Tapped: one frame of Jump, then full gravity 0.3: 3.2 + 2.9 + ... = 18.7 px.
    let mut s = Sim::new(&ARENA, &[on_floor(60.0)]);
    s.steps(3, NONE);
    let y0 = s.pos(0).y;
    s.step1(inp(0, 0, JUMP));
    let mut min_y = s.pos(0).y;
    for _ in 0..60 {
        s.step1(NONE);
        min_y = min_y.min(s.pos(0).y);
    }
    let tapped = y0 - min_y;
    assert!((18.0..=19.0).contains(&tapped), "tapped apex {tapped}");
}

/// Run speed caps at 1.5 px/frame (accel 0.15 → 10 frames to full speed).
#[test]
fn run_speed() {
    let mut s = Sim::new(&ARENA, &[on_floor(40.0)]);
    s.steps(2, NONE);
    for k in 1..=10 {
        s.step1(inp(1, 0, 0));
        let sx = s.p(0).speed.x;
        assert!(
            (sx - (0.15 * k as f32).min(1.5)).abs() < 1e-4,
            "frame {k}: {sx}"
        );
    }
    let x0 = s.pos(0).x;
    s.steps(40, inp(1, 0, 0));
    assert_eq!(s.pos(0).x - x0, 60.0);
    assert_eq!(s.p(0).speed.x, 1.5);
    // Friction 0.2 when released.
    s.step1(NONE);
    assert!((s.p(0).speed.x - 1.3).abs() < 1e-5);
}

/// Dodge: 21 moving updates, 5.5·Σ0.85^k = 35.5 px, cooldown 25 frames.
#[test]
fn dodge_distance_and_frames() {
    let mut s = Sim::new(&ARENA, &[on_floor(40.0)]);
    s.steps(2, NONE);
    let x0 = s.pos(0).x;
    s.step1(inp(0, 0, DODGE));
    assert_eq!(s.state(0), PState::Dodging);
    assert_eq!(s.pos(0).x, x0, "no movement on the entry frame");
    let mut n = 0;
    while s.state(0) == PState::Dodging {
        s.step1(NONE);
        n += 1;
        assert!(n < 40);
    }
    assert_eq!(n, 21, "dodging update calls");
    let d = s.pos(0).x - x0;
    assert!((35.0..=36.0).contains(&d), "dodge distance {d}");
    // Cooldown: another dodge is refused until 25 frames after leaving.
    let mut refused = 0;
    loop {
        s.step1(inp(0, 0, DODGE));
        if s.state(0) == PState::Dodging {
            break;
        }
        s.step1(NONE);
        refused += 2;
        assert!(refused < 60);
    }
    assert!((24..=26).contains(&refused), "cooldown frames {refused}");
}

/// Holding the dodge button stalls up to 5 more updates.
#[test]
fn dodge_stall_while_held() {
    let mut s = Sim::new(&ARENA, &[on_floor(40.0)]);
    s.steps(2, NONE);
    s.step1(inp(0, 0, DODGE));
    let mut n = 0;
    while s.state(0) == PState::Dodging {
        s.step1(inp(0, 0, DODGE));
        n += 1;
        assert!(n < 40);
    }
    assert_eq!(n, 26);
}

/// Dodge-slide from a duck: ×1.4 speed → ≈49.6 px.
#[test]
fn duck_and_slide() {
    let mut s = Sim::new(&ARENA, &[on_floor(40.0)]);
    s.steps(2, NONE);
    s.step1(inp(0, 1, 0));
    assert_eq!(s.state(0), PState::Ducking);
    assert!(s.p(0).ducking_hitbox);
    let x0 = s.pos(0).x;
    s.step1(inp(0, 1, DODGE));
    assert_eq!(s.state(0), PState::Dodging);
    assert!(s.p(0).dodge_sliding);
    let mut n = 0;
    while s.state(0) == PState::Dodging {
        s.step1(inp(0, 1, 0));
        n += 1;
        assert!(n < 40);
    }
    assert_eq!(
        s.state(0),
        PState::Ducking,
        "slide ends in a duck while down is held"
    );
    let d = s.pos(0).x - x0;
    assert!((49.0..=50.0).contains(&d), "slide distance {d}");
    s.step1(NONE);
    assert_eq!(s.state(0), PState::Normal);
    assert!(!s.p(0).ducking_hitbox);
}

/// Coyote time: 5 airborne frames after walking off a ledge.
#[test]
fn coyote_frames() {
    for (k, expect_jump) in [(1, true), (5, true), (6, false)] {
        // Stand on the block (cols 10-14, top y=130) near its right edge.
        let mut s = Sim::new(&ARENA, &[Vec2::new(146.0, 122.0)]);
        s.steps(2, NONE);
        assert!(s.p(0).on_ground);
        // Walk right until airborne at the start of a frame.
        let mut airborne = 0;
        let mut jumped = false;
        for _ in 0..60 {
            let before_ground = s.p(0).on_ground;
            let _ = before_ground;
            let press = airborne + 1 == k;
            s.step1(inp(1, 0, if press { JUMP } else { 0 }));
            let p = s.p(0);
            if !p.on_ground {
                airborne += 1;
                if press {
                    jumped = p.speed.y < -2.0;
                    break;
                }
            }
        }
        assert_eq!(jumped, expect_jump, "jump on airborne frame {k}");
    }
}

/// Jump buffer: the press is honoured on the press frame and the following
/// 5, so the first grounded update may come up to 5 frames after the press.
#[test]
fn jump_buffer() {
    // Find the landing frame of a drop from y=150.
    let land = {
        let mut s = Sim::new(&ARENA, &[Vec2::new(60.0, 150.0)]);
        let mut f = 0;
        while !s.p(0).on_ground {
            s.step1(NONE);
            f += 1;
        }
        f
    };
    // `land` steps until grounded at the start of an update: that update is
    // step index land-1; a press at index land-1-k is honoured for k <= 5.
    for (early, expect) in [(5, true), (6, true), (7, false)] {
        let mut s = Sim::new(&ARENA, &[Vec2::new(60.0, 150.0)]);
        let press = land - early;
        let mut jumped = false;
        for f in 0..land + 3 {
            s.step1(inp(0, 0, if f == press { JUMP } else { 0 }));
            if f > press && s.p(0).speed.y < -2.0 {
                jumped = true;
            }
        }
        assert_eq!(jumped, expect, "press {early} frames before ground");
    }
}

/// Wall slide caps the fall; jumping off the wall kicks away at 2.0 for 12
/// forced frames.
#[test]
fn wall_slide_and_wall_jump() {
    // Against the left wall (x 0..10): hitbox left = x-4, so x = 14 touches.
    let mut s = Sim::new(&ARENA, &[Vec2::new(14.0, 60.0)]);
    for _ in 0..30 {
        s.step1(inp(-1, 0, 0));
    }
    let p = s.p(0);
    assert_eq!(p.cling, -1, "clinging to the wall");
    assert!(
        p.speed.y <= 1.6 && p.speed.y > 0.0,
        "wall slide speed {}",
        p.speed.y
    );
    s.step1(inp(-1, 0, JUMP));
    let p = s.p(0);
    assert_eq!(p.speed.x, 2.0);
    assert!(p.speed.y < -2.9);
    assert_eq!(p.auto_move, 1);
    // Forced away from the wall although Left is held.
    s.steps(10, inp(-1, 0, JUMP));
    assert_eq!(s.p(0).auto_move, 1);
    assert!(s.p(0).speed.x > 0.0);
    s.steps(2, inp(-1, 0, JUMP));
    assert_eq!(s.p(0).auto_move, 0, "auto move ends after 12 frames");
}

/// Falling past a ledge while holding toward it grabs it; jumping releases.
#[test]
fn ledge_grab() {
    // Block cols 10-14: left face at x=100, top y=130. Fall just left of it.
    let mut s = Sim::new(&ARENA, &[Vec2::new(95.0, 124.0)]);
    let mut grabbed = false;
    for _ in 0..40 {
        s.step1(inp(1, 0, 0));
        if s.state(0) == PState::LedgeGrab {
            grabbed = true;
            break;
        }
    }
    assert!(grabbed, "ledge grabbed");
    let c = s.core(0);
    assert_eq!(c.pos.y, 130.0 + 2.0, "hangs 2 px below the ledge top");
    assert_eq!(c.right(), 100.0, "flush against the wall");
    // Holds while pushing toward the wall.
    s.steps(10, inp(1, 0, 0));
    assert_eq!(s.state(0), PState::LedgeGrab);
    // Letting go of the direction releases with a ledge grace.
    s.step1(NONE);
    assert_eq!(s.state(0), PState::Normal);
    assert_eq!(s.p(0).grace_ledge_dir, -1);
}

/// Walking through the open screen side wraps to the other side.
#[test]
fn screen_wrap() {
    let mut s = Sim::new(&OPEN, &[on_floor(6.0)]);
    s.steps(2, NONE);
    let mut wrapped = false;
    for _ in 0..30 {
        s.step1(inp(-1, 0, 0));
        if s.pos(0).x > 300.0 {
            wrapped = true;
            break;
        }
    }
    assert!(wrapped);
    let x = s.pos(0).x;
    assert!((0.0..320.0).contains(&x));
    assert!(s.p(0).on_ground);
}

/// Down on a jump-thru + Jump drops through it.
#[test]
fn drop_through_jump_thru() {
    // Jump-thru cols 26-30 at y=180: stand on it.
    let mut s = Sim::new(&ARENA, &[Vec2::new(280.0, 170.0)]);
    s.steps(3, NONE);
    assert!(s.p(0).on_ground, "standing on the jump-thru");
    s.step1(inp(0, 1, 0));
    assert_eq!(s.state(0), PState::Ducking);
    s.step1(inp(0, 1, JUMP));
    assert_eq!(s.state(0), PState::Normal);
    s.steps(20, NONE);
    assert_eq!(s.pos(0).y, FLOOR_Y, "fell to the floor");
}

/// Jumping up through a jump-thru is not blocked.
#[test]
fn jump_up_through_jump_thru() {
    let mut s = Sim::new(&ARENA, &[on_floor(280.0)]);
    s.steps(2, NONE);
    for _ in 0..24 {
        s.step1(inp(0, 0, JUMP));
    }
    s.steps(30, NONE);
    assert_eq!(
        s.pos(0).y,
        172.0,
        "landed on top of the jump-thru (feet at y=180)"
    );
}

/// Aiming on the ground stops the archer (friction 0.2, no acceleration).
#[test]
fn aiming_stops_running() {
    let mut s = Sim::new(&ARENA, &[on_floor(40.0)]);
    s.steps(20, inp(1, 0, 0));
    s.step1(inp(1, 0, SHOOT));
    assert!(s.p(0).aiming);
    s.steps(8, inp(1, 0, SHOOT));
    assert_eq!(s.p(0).speed.x, 0.0);
    assert_eq!(s.p(0).aim_direction, 0.0);
}

/// A ground jump from a jump pad is a super jump (-4.3).
#[test]
fn jump_pad_super_jump() {
    let mut map = ARENA;
    map[21] = "#.....J........................#";
    let mut s = Sim::new(&map, &[Vec2::new(65.0, 202.0)]);
    s.steps(2, NONE);
    assert!(s.p(0).on_ground, "standing on the pad");
    s.step1(inp(0, 0, JUMP));
    assert_eq!(s.p(0).speed.y, -4.3);
    // A plain floor jump next to it is normal.
    let mut s = Sim::new(&map, &[on_floor(100.0)]);
    s.steps(2, NONE);
    s.step1(inp(0, 0, JUMP));
    assert_eq!(s.p(0).speed.y, -3.2);
}
