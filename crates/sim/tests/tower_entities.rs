//! Tower-entity behaviour from docs/spec/tower-entities.md on ASCII arenas.

mod common;
use common::*;
use tfp_sim::entities::{Placement, PlacementKind as K, Tower};
use tfp_sim::entity::{EntityId, Kind, Tag};
use tfp_sim::events::Event;
use tfp_sim::{Content, MatchSetup, PlayerInput, PlayerSetup, Vec2, World};

struct T {
    c: Content,
    w: World,
    /// Events of the first step (the archers enter the scene).
    ev0: Vec<Event>,
}

fn place(kind: K, x: f32, y: f32, w: f32, h: f32) -> Placement {
    Placement::new(kind, Vec2::new(x, y), Vec2::new(w, h))
}

impl T {
    fn new(map: &[&str], ps: Vec<Placement>, spawns: &[Vec2]) -> T {
        let mut c = Content::from_ascii(map);
        c.levels[0].tower.placements = ps;
        let mut setup = MatchSetup::default();
        for (i, &s) in spawns.iter().enumerate() {
            setup.players[i] = Some(PlayerSetup {
                archer: i as u16,
                spawn: Some(s),
                ..Default::default()
            });
        }
        let w = World::new(&c, &setup);
        let mut t = T { c, w, ev0: vec![] };
        t.ev0 = t.step(NONE);
        t
    }
    fn step(&mut self, i: PlayerInput) -> Vec<Event> {
        self.w.step(&self.c, &[i]);
        self.w.events().to_vec()
    }
    fn steps(&mut self, n: usize, i: PlayerInput) -> Vec<Event> {
        let mut ev = std::mem::take(&mut self.ev0);
        for _ in 0..n {
            ev.extend(self.step(i));
        }
        ev
    }
    fn towers(&self, f: impl Fn(&Tower) -> bool) -> Vec<EntityId> {
        self.w
            .entities()
            .filter(|&e| matches!(self.w.kind(e), Some(Kind::Tower(t)) if f(t)))
            .collect()
    }
    fn one(&self, f: impl Fn(&Tower) -> bool) -> EntityId {
        let v = self.towers(f);
        assert_eq!(v.len(), 1, "expected exactly one entity");
        v[0]
    }
    fn pos(&self, id: EntityId) -> Vec2 {
        self.w.core(id).pos
    }
    fn player(&self) -> Option<EntityId> {
        self.w.player_by_index(0)
    }
}

fn deaths(ev: &[Event]) -> Vec<(u8, i8, u8)> {
    ev.iter()
        .filter_map(|e| match e {
            Event::Death {
                player,
                killer,
                cause,
            } => Some((*player, *killer, *cause)),
            _ => None,
        })
        .collect()
}

/// Rows 1 and 22 fully solid (side lava only), floor top at y = 220.
const SEALED: [&str; 24] = [
    "################################",
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
    "################################",
    "################################",
];

// ------------------------------------------------------------ §5.1

#[test]
fn moving_platform_rests_120_then_moves_at_0_4_and_carries_its_rider() {
    let mut p = place(K::MovingPlatform, 100.0, 100.0, 40.0, 10.0);
    p.node = Some(Vec2::new(180.0, 100.0));
    let mut t = T::new(&ARENA, vec![p], &[Vec2::new(120.0, 92.0)]);
    let id = t.one(|t| matches!(t, Tower::MovingPlatform(_)));
    // Rest 120 frames from load (one already ran).
    t.steps(118, NONE);
    assert_eq!(t.pos(id), Vec2::new(100.0, 100.0));
    let ev = t.steps(2, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_movingBlockStart",
            ..
        }
    )));
    // 50 frames of motion = 20 px at 0.4 px/frame.
    t.steps(50, NONE);
    let dx = t.pos(id).x - 100.0;
    assert!((19.0..=21.0).contains(&dx), "moved {dx}");
    let pl = t.player().unwrap();
    assert!(
        (t.w.core(pl).pos.x - (120.0 + dx)).abs() <= 1.0,
        "rider carried"
    );
    // Arrives (80 px leg ~ 200 frames), rests, comes back.
    let ev = t.steps(170, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_movingBlockEnd",
            ..
        }
    )));
    assert_eq!(t.pos(id), Vec2::new(180.0, 100.0));
}

// ------------------------------------------------------------ §6.1

#[test]
fn cracked_platform_shifts_shakes_80_breaks_and_respawns_after_120() {
    let p = place(K::CrackedPlatform, 100.0, 100.0, 40.0, 0.0);
    let mut t = T::new(&ARENA, vec![p], &[Vec2::new(120.0, 92.0)]);
    let id = t.one(|t| matches!(t, Tower::CrackedPlatform(_)));
    let ev = t.steps(10, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_shakyFloor",
            ..
        }
    )));
    assert_eq!(t.pos(id).y, 103.0, "shifted 3 px down");
    let pl = t.player().unwrap();
    assert_eq!(t.w.core(pl).bottom(), 103.0, "the rider sank with it");
    assert!(t.w.core(id).collidable);
    let ev = t.steps(80, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_shakyFloorBreak",
            ..
        }
    )));
    assert!(!t.w.core(id).collidable && !t.w.core(id).visible);
    // The archer drops to the floor; the block respawns 120 frames later.
    let ev = t.steps(121, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_shakyFloorRevive",
            ..
        }
    )));
    t.steps(2, NONE);
    assert!(t.w.core(id).collidable);
    assert_eq!(t.pos(id), Vec2::new(100.0, 100.0));
}

// ------------------------------------------------------------ §6.2, §6.3, §4.3

#[test]
fn explosions_break_cracked_walls_and_crumble_blocks_after_5_frames() {
    let ps = vec![
        place(K::CrackedWall, 100.0, 100.0, 0.0, 0.0),
        place(K::CrumbleBlock, 200.0, 100.0, 20.0, 20.0),
    ];
    let mut t = T::new(&ARENA, ps, &[Vec2::new(40.0, 212.0)]);
    let wall = t.one(|t| matches!(t, Tower::BreakBlock(b) if !b.crumble));
    let block = t.one(|t| matches!(t, Tower::BreakBlock(b) if b.crumble));
    // Inside a solid: nothing happens.
    assert!(!tfp_sim::entities::explode(
        &mut t.w,
        &t.c,
        Vec2::new(105.0, 105.0),
        0,
        false,
        false,
        None
    ));
    assert!(tfp_sim::entities::explode(
        &mut t.w,
        &t.c,
        Vec2::new(90.0, 110.0),
        0,
        false,
        false,
        None
    ));
    assert!(tfp_sim::entities::explode(
        &mut t.w,
        &t.c,
        Vec2::new(190.0, 110.0),
        0,
        false,
        false,
        None
    ));
    t.steps(4, NONE);
    assert!(
        t.w.exists(wall) && t.w.exists(block),
        "still solid for 5 frames"
    );
    t.steps(2, NONE);
    assert!(!t.w.exists(wall) && !t.w.exists(block));
}

#[test]
fn crumble_block_without_open_left_face_is_widened_two_pixels() {
    // Block flush against the left wall: no open left face.
    let ps = vec![place(K::CrumbleBlock, 10.0, 100.0, 20.0, 20.0)];
    let t = T::new(&ARENA, ps, &[Vec2::new(160.0, 212.0)]);
    let b = t.one(|t| matches!(t, Tower::BreakBlock(_)));
    let hb = t.w.core(b).hitbox();
    assert_eq!((hb.x, hb.w), (-2.0, 22.0));
}

// ------------------------------------------------------------ §8.1

#[test]
fn side_lava_rises_to_30_px_and_kills_with_owner_credit() {
    let mut t = T::new(
        &SEALED,
        vec![],
        &[Vec2::new(16.0, 212.0), Vec2::new(160.0, 212.0)],
    );
    tfp_sim::entities::start_lava(&mut t.w, &t.c, 1, false);
    let mut ev = vec![];
    for _ in 0..300 {
        t.w.step(&t.c, &[NONE, NONE]);
        ev.extend(t.w.events().to_vec());
    }
    let d = deaths(&ev);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0], (0, 1, tfp_sim::player::DeathCause::Lava as u8));
    assert!(
        t.w.player_by_index(1).is_some(),
        "the centre archer is out of reach"
    );
    let lavas = t.towers(|t| matches!(t, Tower::Lava(_)));
    assert_eq!(lavas.len(), 2);
    let left = lavas
        .iter()
        .map(|&l| t.w.core(l).pos.x)
        .fold(f32::MAX, f32::min);
    assert!((28.0..=32.0).contains(&left), "left lava edge {left}");
}

// ------------------------------------------------------------ §7

#[test]
fn ice_removes_ground_friction() {
    let run = |ice: bool| {
        let ps = if ice {
            vec![place(K::Ice, 10.0, 220.0, 300.0, 0.0)]
        } else {
            vec![]
        };
        let mut t = T::new(&ARENA, ps, &[Vec2::new(60.0, 212.0)]);
        t.steps(40, inp(1, 0, 0));
        t.steps(20, NONE);
        t.w.player(t.player().unwrap()).unwrap().speed.x
    };
    assert_eq!(run(false), 0.0);
    assert!(run(true) > 0.5, "slides on ice");
}

#[test]
fn mud_caps_run_speed_at_40_percent() {
    let ps = vec![place(K::Mud, 10.0, 215.0, 300.0, 0.0)];
    let mut t = T::new(&ARENA, ps, &[Vec2::new(60.0, 212.0)]);
    let ev = t.steps(60, inp(1, 0, 0));
    let p = *t.w.player(t.player().unwrap()).unwrap();
    assert!(p.speed.x <= 0.61 && p.speed.x > 0.5, "speed {}", p.speed.x);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_mudPlayerMove",
            ..
        }
    )));
}

#[test]
fn hot_coals_auto_bounce_and_ignite() {
    let ps = vec![place(K::HotCoals, 10.0, 220.0, 300.0, 0.0)];
    let mut t = T::new(&ARENA, ps, &[Vec2::new(100.0, 212.0)]);
    let ev = t.steps(3, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "sfx_coalBurn",
            ..
        }
    )));
    let p = *t.w.player(t.player().unwrap()).unwrap();
    assert!(p.speed.y < -1.5, "bounced: {}", p.speed.y);
    assert!(t.w.tower.players[0].fire > 0.0);
}

// ------------------------------------------------------------ §6.10

#[test]
fn jump_pad_launches_at_4_3_and_squashes() {
    let ps = vec![place(K::JumpPad, 100.0, 210.0, 30.0, 0.0)];
    let mut t = T::new(&ARENA, ps, &[Vec2::new(115.0, 202.0)]);
    t.steps(2, NONE);
    t.step(inp(0, 0, JUMP));
    let p = *t.w.player(t.player().unwrap()).unwrap();
    assert_eq!(p.speed.y, -4.3);
    let pad = t.one(|t| matches!(t, Tower::JumpPad(_)));
    let Some(Kind::Tower(Tower::JumpPad(j))) = t.w.kind(pad) else {
        unreachable!()
    };
    assert!(j.scales[1] > 1.7, "{:?}", j.scales);
}

// ------------------------------------------------------------ §6.7

#[test]
fn switch_blocks_swap_every_300_frames() {
    let ps = vec![
        place(K::RedSwitchBlock, 100.0, 100.0, 20.0, 20.0),
        place(K::BlueSwitchBlock, 200.0, 100.0, 20.0, 20.0),
    ];
    let mut t = T::new(&ARENA, ps, &[Vec2::new(40.0, 212.0)]);
    let red = t.one(|t| matches!(t, Tower::SwitchBlock(b) if b.red));
    let blue = t.one(|t| matches!(t, Tower::SwitchBlock(b) if !b.red));
    assert!(t.w.core(red).collidable && !t.w.core(blue).collidable);
    let ev = t.steps(300, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_blockSwitch",
            ..
        }
    )));
    assert!(!t.w.core(red).collidable && t.w.core(blue).collidable);
}

// ------------------------------------------------------------ §8.5

#[test]
fn spike_ball_kills_without_credit() {
    // Pivot (160, 170), radius 40: the ball sweeps the floor at the bottom.
    let mut p = place(K::SpikeBall, 160.0, 130.0, 0.0, 0.0);
    p.node = Some(Vec2::new(160.0, 170.0));
    let mut t = T::new(&ARENA, vec![p], &[Vec2::new(160.0, 212.0)]);
    let ev = t.steps(200, NONE);
    let d = deaths(&ev);
    assert_eq!(
        d,
        vec![(0, -1, tfp_sim::player::DeathCause::SpikeBall as u8)]
    );
}

// ------------------------------------------------------------ §6.8, §9.1

#[test]
fn proximity_block_vanishes_near_archers() {
    let ps = vec![
        place(K::ProximityBlock, 100.0, 190.0, 0.0, 0.0),
        place(K::ProximityBlock, 250.0, 60.0, 0.0, 0.0),
    ];
    let mut t = T::new(&ARENA, ps, &[Vec2::new(100.0, 212.0)]);
    t.steps(2, NONE);
    let ids = t.towers(|t| matches!(t, Tower::ProximityBlock(_)));
    let near = *ids.iter().find(|&&i| t.pos(i).x == 100.0).unwrap();
    let far = *ids.iter().find(|&&i| t.pos(i).x == 250.0).unwrap();
    assert!(!t.w.core(near).collidable && t.w.core(far).collidable);
    // Walk away beyond 52 px: it comes back.
    t.steps(60, inp(-1, 0, 0));
    t.steps(25, NONE);
    assert!(t.w.core(near).collidable);
}

#[test]
fn lantern_falls_when_touched_and_breaks_on_landing() {
    let ps = vec![place(K::Lantern, 100.0, 200.0, 0.0, 0.0)];
    let mut t = T::new(&ARENA, ps, &[Vec2::new(100.0, 212.0)]);
    let ev = t.steps(40, NONE);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_lampFall",
            ..
        }
    )));
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Sound {
            name: "env_lampCrash",
            ..
        }
    )));
    let l = t.one(|t| matches!(t, Tower::Lantern(_)));
    assert!(!t.w.core(l).has_tag(Tag::Target));
}
