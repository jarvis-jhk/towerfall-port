//! Enemy behaviour checks against docs/spec/enemies.md (one scenario per
//! enemy family) plus the spawn portal, the player ghost and Team Revive.

mod common;
use common::*;
use tfp_sim::Vec2;
use tfp_sim::enemies::{self, EnemyType, hazard::HazardType};
use tfp_sim::entity::{EntityId, Kind, Tag};
use tfp_sim::events::Event;
use tfp_sim::player::DeathCause;

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

/// Step with idle input until `f` holds (returns the frame) or `max` frames.
fn until(s: &mut Sim, max: usize, mut f: impl FnMut(&Sim) -> bool) -> Option<usize> {
    for k in 0..max {
        s.step(&[NONE, NONE]);
        if f(s) {
            return Some(k);
        }
    }
    None
}

fn deaths(s: &Sim) -> Vec<(u8, u8)> {
    s.w.events()
        .iter()
        .filter_map(|e| match *e {
            Event::Death { player, cause, .. } => Some((player, cause)),
            _ => None,
        })
        .collect()
}

fn player_dead(s: &Sim, i: u8) -> bool {
    s.pid(i)
        .is_none_or(|id| s.w.player(id).is_none_or(|p| p.dead))
}

#[test]
fn names_parse() {
    for n in [
        "Slime",
        "RedSlime",
        "BlueSlime",
        "Bat",
        "BombBat",
        "SuperBombBat",
        "Crow",
        "Ghost",
        "GreenGhost",
        "Elemental",
        "GreenElemental",
        "Cultist",
        "ScytheCultist",
        "BossCultist",
        "Birdman",
        "DarkBirdman",
        "Skeleton",
        "SkeletonS",
        "BombSkeletonS",
        "LaserSkeleton",
        "WingSkeletonS",
        "MimicSkeleton",
        "Jester",
        "BossSkeleton",
        "BossWingSkeleton",
        "Mole",
        "Worm",
        "FlamingSkull",
        "Exploder",
        "EvilCrystal",
        "BlueCrystal",
        "BoltCrystal",
        "PrismCrystal",
        "TechnoMage",
    ] {
        assert!(EnemyType::from_name(n).is_some(), "{n}");
    }
    assert!(EnemyType::from_name("Dragon").is_none());
    assert_eq!(EnemyType::from_name("Skeleton").unwrap().bounty(), 1000);
    assert_eq!(EnemyType::from_name("BossSkeleton").unwrap().bounty(), 0);
}

/// Slime walks at 0.4 toward its facing, jumps (1.6, -3.5) at a player 25..68
/// px ahead, and kills on touch from the side.
#[test]
fn slime_walk_jump_touch() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(200.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Slime", Vec2::new(100.0, 219.0), 1);
    until(&mut s, 30, |_| false);
    let e = enemy(&s, id).unwrap();
    assert!((e.speed.x - 0.4).abs() < 1e-4, "walk speed {}", e.speed.x);
    let jumped = until(&mut s, 400, |s| {
        enemy(s, id).is_some_and(|e| e.speed.y <= -3.0)
    });
    assert!(jumped.is_some(), "jumped at the player");
    let e = enemy(&s, id).unwrap();
    assert!((e.speed.x - 1.6).abs() < 0.05);
    let dead = until(&mut s, 400, |s| player_dead(s, 0));
    assert!(dead.is_some(), "the slime kills on touch");
}

/// Stomping a slime kills it and bounces the archer.
#[test]
fn stomp_kills_slime() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(205.0, 150.0)]);
    let id = spawn(&mut s, "Slime", Vec2::new(205.0, 219.0), -1);
    let mut bounced = false;
    for _ in 0..80 {
        s.step(&[NONE]);
        if let Some(p) = s.pid(0).and_then(|i| s.w.player(i).copied())
            && p.speed.y < -2.0
        {
            bounced = true;
        }
    }
    assert!(bounced, "bounced off the slime");
    assert!(!player_dead(&s, 0));
    assert!(enemy(&s, id).is_none_or(|e| !e.alive()), "slime dead");
}

/// Any arrow kills a slime.
#[test]
fn arrow_kills_slime() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Slime", Vec2::new(140.0, 219.0), 1);
    s.step(&[inp(1, 0, SHOOT)]);
    s.step(&[inp(1, 0, 0)]);
    let dead = until(&mut s, 80, |s| enemy(s, id).is_none_or(|e| !e.alive()));
    assert!(dead.is_some(), "slime shot");
}

/// A bat swoops at a visible archer below it and kills with its attack box.
#[test]
fn bat_swoops_and_kills() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(170.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Bat", Vec2::new(150.0, 160.0), 1);
    let swooped = until(&mut s, 200, |s| {
        enemy(s, id).is_some_and(|e| e.state == enemies::bat::SWOOP)
    });
    assert!(swooped.is_some(), "swoop");
    assert!(
        until(&mut s, 300, |s| player_dead(s, 0)).is_some(),
        "the swoop kills"
    );
}

/// A bomb bat's swoop box kills the archer (cause Enemy, §1.7) and then the
/// bat explodes and is removed.
#[test]
fn bomb_bat_explodes() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(170.0, FLOOR_Y)]);
    let id = spawn(&mut s, "BombBat", Vec2::new(150.0, 170.0), 1);
    assert!(
        until(&mut s, 400, |s| player_dead(s, 0)).is_some(),
        "archer killed"
    );
    assert!(
        until(&mut s, 30, |s| !s.w.exists(id)).is_some(),
        "bat exploded and removed"
    );
}

/// An arrow lodges in a bat it kills; the bat lingers while it holds arrows.
#[test]
fn arrow_lodges_in_bat() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Bat", Vec2::new(120.0, 211.0), -1);
    s.step(&[inp(1, 0, SHOOT)]);
    s.step(&[inp(1, 0, 0)]);
    assert!(until(&mut s, 60, |s| enemy(s, id).is_some_and(|e| !e.alive())).is_some());
    let e = enemy(&s, id).unwrap();
    assert_eq!(enemies::lodged_count(&e), 1);
}

/// Ghosts chase a visible archer and kill on touch.
#[test]
fn ghost_chases_and_kills() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(200.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Ghost", Vec2::new(110.0, 180.0), 1);
    assert!(
        until(&mut s, 60, |s| enemy(s, id)
            .is_some_and(|e| e.state == enemies::ghost::CHASE))
        .is_some()
    );
    assert!(
        until(&mut s, 900, |s| player_dead(s, 0)).is_some(),
        "ghost kills"
    );
}

/// A ghost dodges an arrow homing on it at 20..40 px.
#[test]
fn ghost_dodges_homing_arrow() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Ghost", Vec2::new(130.0, 200.0), -1);
    s.step(&[inp(1, 0, SHOOT)]);
    s.step(&[inp(1, 0, 0)]);
    let dodged = until(&mut s, 60, |s| {
        enemy(s, id).is_some_and(|e| e.state == enemies::ghost::DODGE)
    });
    assert!(dodged.is_some(), "dodge");
}

/// The cultist lunges at an archer within 40 px; its attack box kills.
#[test]
fn cultist_lunge() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(185.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Cultist", Vec2::new(155.0, 219.0), 1);
    assert!(
        until(&mut s, 120, |s| enemy(s, id)
            .is_some_and(|e| e.state == enemies::cultist::ATTACK))
        .is_some()
    );
    assert!(until(&mut s, 120, |s| player_dead(s, 0)).is_some());
}

/// Birdmen throw tornadoes at archers within 100 px.
#[test]
fn birdman_tornado() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(160.0, FLOOR_Y)]);
    spawn(&mut s, "Birdman", Vec2::new(140.0, 160.0), 1);
    let tornado = until(&mut s, 300, |s| {
        s.w.entities()
            .any(|e| matches!(s.w.kind(e), Some(Kind::Hazard(h)) if h.ty == HazardType::Tornado))
    });
    assert!(tornado.is_some());
}

/// A skeleton spots an archer on its facing side and shoots its arrows.
#[test]
fn skeleton_shoots() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(160.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Skeleton", Vec2::new(70.0, 212.0), 1);
    let shot = until(&mut s, 400, |s| {
        s.arrows().iter().any(|(_, a)| a.owner == Some(id))
    });
    assert!(shot.is_some(), "skeleton fired");
}

/// A shielded skeleton loses the shield to the first hit and dies to the second.
#[test]
fn skeleton_shield() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "SkeletonS", Vec2::new(250.0, 212.0), -1);
    s.step(&[NONE]);
    let mut e = enemy(&s, id).unwrap();
    let mut w = s.w.clone();
    enemies::hurt(&mut w, &s.c, id, &mut e, Vec2::new(1.0, 0.0), 1, 0);
    assert!(e.alive(), "shield absorbed");
    assert!(w.core(id).flash.flashing);
    for _ in 0..40 {
        w.core_mut(id).flash.update(1.0);
    }
    enemies::hurt(&mut w, &s.c, id, &mut e, Vec2::new(1.0, 0.0), 1, 0);
    assert!(!e.alive(), "second hit kills");
}

/// Moles roll at an archer in their vision cone; rolling kills.
#[test]
fn mole_rolls() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(150.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Mole", Vec2::new(100.0, 205.0), 1);
    assert!(
        until(&mut s, 200, |s| enemy(s, id)
            .is_some_and(|e| e.state == enemies::mole::ROLL))
        .is_some()
    );
    assert!(until(&mut s, 200, |s| player_dead(s, 0)).is_some());
}

/// The flaming skull flies diagonally at speed 1 and bounces off walls.
#[test]
fn flaming_skull_bounces() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "FlamingSkull", Vec2::new(250.0, 60.0), 1);
    until(&mut s, 5, |_| false);
    let e = enemy(&s, id).unwrap();
    assert!((e.speed.length() - 1.0).abs() < 1e-4);
    assert!(
        until(&mut s, 200, |s| enemy(s, id)
            .is_some_and(|e| e.speed.x < 0.0))
        .is_some(),
        "wall bounce"
    );
}

/// Red crystals fire lasers at archers within 100 px.
#[test]
fn crystal_fires_laser() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(200.0, FLOOR_Y)]);
    let id = spawn(&mut s, "EvilCrystal", Vec2::new(160.0, 160.0), 1);
    let shot = until(&mut s, 200, |s| {
        s.arrows()
            .iter()
            .any(|(_, a)| a.owner == Some(id) && a.arrow_type == tfp_sim::quiver::ArrowType::Laser)
    });
    assert!(shot.is_some());
}

/// The exploder chases, arms near an archer and explodes.
#[test]
fn exploder_explodes() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(200.0, FLOOR_Y)]);
    spawn(&mut s, "Exploder", Vec2::new(150.0, 190.0), 1);
    let mut cause = None;
    for _ in 0..800 {
        s.step(&[NONE]);
        if let Some(&(_, c)) = deaths(&s).first() {
            cause = Some(c);
            break;
        }
    }
    assert_eq!(cause, Some(DeathCause::Explosion as u8));
}

/// A worm walking off the block's edge wraps around the corner.
#[test]
fn worm_turns_corners() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = spawn(&mut s, "Worm", Vec2::new(130.0, 122.0), 1);
    let start = enemy(&s, id).unwrap().facing;
    let turned = until(&mut s, 600, |s| {
        enemy(s, id).is_some_and(|e| e.state == enemies::worm::WALK && e.target_pos.y.abs() < 0.5)
    });
    assert!(turned.is_some(), "clinging to a side wall (facing {start})");
}

/// The techno mage fires homing missiles at visible archers.
#[test]
fn techno_mage_missile() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(160.0, FLOOR_Y)]);
    spawn(&mut s, "TechnoMage", Vec2::new(140.0, 175.0), 1);
    let fired = until(&mut s, 200, |s| {
        s.w.entities().any(
            |e| matches!(s.w.kind(e), Some(Kind::Hazard(h)) if h.ty == HazardType::TechnoMissile),
        )
    });
    assert!(fired.is_some());
}

/// The King Reaper shields itself; arrows bounce off; unshielded it takes damage.
#[test]
fn king_reaper_shield_and_damage() {
    let mut s = Sim::new(&ARENA, &[Vec2::new(40.0, FLOOR_Y)]);
    let id = enemies::reaper::summon(&mut s.w, &s.c, 0, true, None, false, 0);
    assert!(
        until(&mut s, 200, |s| enemy(s, id)
            .is_some_and(|e| e.state == enemies::reaper::ATTACK))
        .is_some()
    );
    let mut e = enemy(&s, id).unwrap();
    assert!(enemies::reaper::shielded(&e));
    assert_eq!(e.health, 3);
    let mut w = s.w.clone();
    enemies::hurt(&mut w, &s.c, id, &mut e, Vec2::ZERO, 1, 0);
    assert_eq!(e.health, 3, "shield blocks damage");
    e.t[0] = 0.0;
    enemies::hurt(&mut w, &s.c, id, &mut e, Vec2::ZERO, 1, 0);
    assert_eq!(e.health, 2);
    assert_eq!(e.state, enemies::reaper::HURT);
}

/// A portal appears over 40 ticks and its enemy comes out ~46 ticks later.
#[test]
fn portal_spawn_timing() {
    let mut map = ARENA;
    map[10] = "#..............S...............#";
    let mut s = Sim::new(&map, &[Vec2::new(40.0, FLOOR_Y)]);
    let portals = enemies::portal::add_level_portals(&mut s.w, &s.c);
    s.step(&[NONE]);
    enemies::portal::appear_and_spawn(&mut s.w, portals[0], EnemyType::Slime);
    let k = until(&mut s, 200, |s| !s.w.tagged(Tag::Enemy).is_empty()).expect("spawned");
    assert!((84..=90).contains(&k), "spawn after {k} ticks");
    assert!(
        until(&mut s, 60, |s| !enemies::portal::is_open(&s.w, portals[0])).is_some(),
        "auto close"
    );
}
