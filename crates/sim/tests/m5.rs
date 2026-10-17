//! M5: arrow types, explosions and kill credit, triggers, prisms, orbs,
//! lava, pick-ups, chests and the Versus treasure spawner, the quiver HUD,
//! and rollback determinism in a busy scene. Numbers come from
//! docs/spec/arrows.md and docs/spec/pickups.md.

mod common;
use common::*;
use tfp_sim::arrow::{self, ArrowState, Shooter};
use tfp_sim::entity::{Allegiance, EntityId, Kind, Tag};
use tfp_sim::events::Event;
use tfp_sim::math::PI;
use tfp_sim::pickups::{self, ItemSetup, Pickups, orbs};
use tfp_sim::quiver::{ArrowType, Quiver};
use tfp_sim::rng::Rng;
use tfp_sim::treasure::{self, Appear, ChestSpawn, ChestState, ChestType};
use tfp_sim::variants::Variant;
use tfp_sim::{Content, PlayerInput, Vec2, View, ViewOptions, World};

fn floor(x: f32) -> Vec2 {
    Vec2::new(x, FLOOR_Y)
}

fn shooter(id: Option<EntityId>, index: i8) -> Shooter {
    Shooter {
        id,
        player_index: index,
        allegiance: Allegiance::Neutral,
        team_color: Allegiance::Neutral,
        hyper: false,
    }
}

/// An arrow of type `t` fired by nobody (player index `index`).
fn fire(s: &mut Sim, t: ArrowType, pos: Vec2, dir: f32, index: i8) -> EntityId {
    arrow::create(&mut s.w, t, shooter(None, index), pos, dir)
}

fn give(s: &mut Sim, index: u8, t: ArrowType, n: usize) {
    let id = s.pid(index).unwrap();
    s.w.player_mut(id).unwrap().arrows = Quiver::new(n, t);
}

fn deaths(s: &Sim) -> Vec<(u8, i8, u8)> {
    s.w.events()
        .iter()
        .filter_map(|e| match *e {
            Event::Death {
                player,
                killer,
                cause,
            } => Some((player, killer, cause)),
            _ => None,
        })
        .collect()
}

fn sounded(s: &Sim, name: &str) -> bool {
    s.w.events()
        .iter()
        .any(|e| matches!(e, Event::Sound { name: n, .. } if *n == name))
}

fn count_kind(s: &Sim, f: impl Fn(&Kind) -> bool) -> usize {
    s.w.entities()
        .filter(|&e| s.w.kind(e).is_some_and(&f))
        .count()
}

// ------------------------------------------------------------- arrows

/// A bomb arrow explodes 50 updates after it is shot (sticking does not
/// stop the fuse); the shooter next to it dies by its own blast (killer =
/// itself), an archer 41 px away is only pushed (45 px push, 35 px kill).
#[test]
fn bomb_fuse_radius_and_self_kill() {
    let mut s = Sim::new(&ARENA, &[floor(290.0), floor(255.0)]);
    s.steps(2, NONE);
    give(&mut s, 0, ArrowType::Bomb, 2);
    s.step(&[inp(1, 0, SHOOT), NONE]);
    s.step(&[inp(1, 0, 0), NONE]);
    let mut boom = None;
    let mut died = vec![];
    for k in 1..=80 {
        s.step(&[NONE, NONE]);
        died.extend(deaths(&s));
        if sounded(&s, "pu_bombArrowExplode") {
            boom = Some(k);
            break;
        }
        if k == 20 {
            let (_, a) = s.arrows()[0];
            assert_eq!(a.state, ArrowState::Stuck, "stuck in the wall");
            assert!(!a.collectible(), "a lit bomb cannot be walked over");
        }
    }
    assert_eq!(boom, Some(50), "50-frame fuse");
    assert_eq!(died, vec![(0, 0, 1)], "self-kill by explosion");
    let c = s.p(1);
    assert!(c.speed.x < -1.0, "pushed away: {:?}", c.speed);
    s.step(&[NONE, NONE]);
    assert!(s.arrows().is_empty(), "the bomb arrow is gone");
}

/// A dropped bomb arrow loses its fuse and becomes a plain collectible.
#[test]
fn dropped_bomb_is_defused() {
    let mut s = Sim::new(&ARENA, &[]);
    let id = fire(&mut s, ArrowType::Bomb, Vec2::new(100.0, 150.0), 0.0, -1);
    s.step(&[]);
    tfp_sim::arrows::with_arrow(&mut s.w, id, |w, a| arrow::drop(w, id, a, 1.0));
    s.steps(120, NONE);
    let a = s.w.arrow(id).copied().expect("still there");
    assert!(!a.extra.fuse_on);
    assert!(a.collectible());
}

/// Lasers fly at 5 px/frame without gravity, reflect off 7 walls and stick
/// on the 8th contact, then vanish 60 frames after coming to rest.
#[test]
fn laser_bounces_seven_times_then_decays() {
    let mut s = Sim::new(&ARENA, &[]);
    let id = fire(&mut s, ArrowType::Laser, Vec2::new(100.0, 100.0), 0.0, -1);
    s.step(&[]);
    let mut flips = 0;
    let mut last = 1.0f32;
    let mut rest_at = None;
    for f in 0..1000 {
        s.step(&[]);
        let Some(a) = s.w.arrow(id).copied() else {
            let r = rest_at.expect("came to rest before vanishing");
            assert!(
                (59..=62).contains(&(f - r)),
                "vanished {} frames after rest",
                f - r
            );
            assert_eq!(flips, 7);
            return;
        };
        if a.state == ArrowState::Shooting {
            assert_eq!(a.speed.length(), 5.0);
            assert_eq!(s.w.core(id).pos.y, 100.0, "no gravity");
            if a.speed.x.signum() != last {
                flips += 1;
                last = a.speed.x.signum();
            }
        } else if rest_at.is_none() {
            assert_eq!(a.state, ArrowState::Stuck);
            assert_eq!(a.extra.bounced, 8);
            rest_at = Some(f);
        }
    }
    panic!("laser never vanished");
}

/// A drill arrow bores through one wall (slowed to 30 %, speeding up to 3)
/// and comes out shooting at 4; it never drills twice.
#[test]
fn drill_goes_through_one_wall() {
    let mut map = ARENA;
    map[10] = "#...............##.............#";
    let mut s = Sim::new(&map, &[]);
    let id = fire(&mut s, ArrowType::Drill, Vec2::new(100.0, 105.0), 0.0, -1);
    let mut drilled = false;
    let mut out = None;
    for _ in 0..200 {
        s.step(&[]);
        let Some(a) = s.w.arrow(id).copied() else {
            break;
        };
        let x = s.w.core(id).pos.x;
        if a.state == ArrowState::Drilling {
            drilled = true;
            assert!(a.speed.length() <= 3.0 + 1e-4);
        } else if drilled && out.is_none() {
            assert_eq!(a.state, ArrowState::Shooting);
            assert!((a.speed.length() - 4.0).abs() < 1e-4);
            assert!(x >= 180.0, "came out on the far side at {x}");
            assert!(a.extra.has_drilled);
            out = Some(x);
        }
    }
    assert!(drilled && out.is_some());
    let (_, a) = s.arrows()[0];
    assert!(a.state >= ArrowState::Stuck);
    assert!(
        s.w.core(s.arrows()[0].0).pos.x > 180.0,
        "did not stick in the drilled wall"
    );
}

/// Bolt and feather arrows never enter Gravity; a normal arrow does after
/// 25 frames. Feathers fly at 6 and wobble sideways.
#[test]
fn bolt_and_feather_never_fall() {
    let mut s = Sim::new(&OPEN, &[]);
    let bolt = fire(&mut s, ArrowType::Bolt, Vec2::new(40.0, 60.0), 0.0, -1);
    let feather = fire(&mut s, ArrowType::Feather, Vec2::new(40.0, 120.0), 0.0, -1);
    let normal = fire(&mut s, ArrowType::Normal, Vec2::new(40.0, 30.0), 0.0, -1);
    let mut wobble = 0.0f32;
    for f in 0..150 {
        s.step(&[]);
        let b = s.w.arrow(bolt).copied().unwrap();
        assert_eq!(b.state, ArrowState::Shooting);
        assert!((b.speed.length() - 4.0).abs() < 1e-4);
        assert_eq!(s.w.core(bolt).pos.y, 60.0);
        let fa = s.w.arrow(feather).copied().unwrap();
        assert_eq!(fa.state, ArrowState::Shooting);
        assert!((fa.speed.length() - 6.0).abs() < 1e-4);
        let dy = s.w.core(feather).pos.y - 120.0;
        // 0.4 px/frame sideways over half a 30-frame sine: about 3.8 px.
        assert!(dy.abs() <= 5.0, "feather wobble {dy}");
        wobble = wobble.max(dy.abs());
        if f == 30 {
            assert_eq!(s.w.arrow(normal).unwrap().state, ArrowState::Gravity);
        }
    }
    assert!(wobble >= 1.0, "the feather wobbles ({wobble})");
}

/// A bramble arrow grows thorns where it lands; they kill an archer
/// standing next to it (killer = the arrow's owner, cause Brambles).
#[test]
fn brambles_grow_and_kill() {
    let mut s = Sim::new(&ARENA, &[floor(162.0)]);
    s.steps(2, NONE);
    let id = fire(
        &mut s,
        ArrowType::Bramble,
        Vec2::new(150.0, 200.0),
        PI / 2.0,
        -1,
    );
    let mut died = vec![];
    let mut tiles = 0;
    for _ in 0..150 {
        s.step1(NONE);
        died.extend(deaths(&s));
        tiles = tiles.max(s.w.tagged(Tag::Brambles).len());
    }
    assert!(tiles >= 3, "a patch grew ({tiles} tiles)");
    assert_eq!(died, vec![(0, -1, 2)]);
    if let Some(a) = s.w.arrow(id) {
        assert!(a.extra.power_used && !a.collectible());
    }
}

/// Trigger arrows explode when their owner presses alt-shoot; the owner
/// gets the kill.
#[test]
fn trigger_arrow_detonation() {
    let mut s = Sim::new(&ARENA, &[floor(200.0), floor(50.0)]);
    s.steps(2, NONE);
    let a_id = s.pid(0).unwrap();
    let id = arrow::create(
        &mut s.w,
        ArrowType::Trigger,
        shooter(Some(a_id), 0),
        Vec2::new(30.0, 200.0),
        PI / 2.0,
    );
    s.steps(30, NONE);
    s.w.arrow_mut(id).unwrap().extra.detonator = Some(a_id);
    s.w.player_mut(a_id).unwrap().triggers[0] = Some(id);
    s.step(&[NONE, NONE]);
    assert!(!s.w.arrow(id).unwrap().collectible(), "armed: no walk-over");
    assert_eq!(s.p(0).trigger_count(), 1);
    s.step(&[inp(0, 0, PlayerInput::ALT_SHOOT), NONE]);
    let d = deaths(&s);
    assert_eq!(d, vec![(1, 0, 1)], "blast kills the archer next to it");
    assert!(s.w.arrow(id).is_none() || s.w.core(id).marked_for_removal);
    assert_eq!(s.p(0).trigger_count(), 0);
}

/// Catching a live prism arrow traps the catcher; the prism holds it in
/// place, then shatters after 120 + 30 frames.
#[test]
fn prism_traps_the_catcher() {
    let mut s = Sim::new(&ARENA, &[floor(100.0)]);
    s.steps(2, NONE);
    fire(
        &mut s,
        ArrowType::Prism,
        Vec2::new(60.0, FLOOR_Y - 1.0),
        0.0,
        1,
    );
    s.step1(inp(-1, 0, DODGE));
    let mut trapped = None;
    for f in 0..20 {
        s.step1(NONE);
        if s.p(0).prism.is_some() {
            trapped = Some(f);
            break;
        }
    }
    assert!(trapped.is_some(), "caught the prism arrow");
    s.step1(NONE);
    assert!(s.arrows().is_empty(), "the arrow is consumed");
    assert_eq!(count_kind(&s, |k| matches!(k, Kind::Prism(_))), 1);
    s.steps(25, NONE);
    let x0 = s.pos(0).x;
    for _ in 0..60 {
        s.step1(inp(1, 0, 0));
    }
    assert!((s.pos(0).x - x0).abs() <= 16.0, "held by the bars");
    s.steps(120, NONE);
    assert!(s.p(0).prism.is_none(), "shattered");
    assert_eq!(count_kind(&s, |k| matches!(k, Kind::Prism(_))), 0);
}

/// Toy arrows never kill: an archer hit is bounced and the arrow falls.
#[test]
fn toy_arrows_bounce_archers() {
    let mut s = Sim::new(&ARENA, &[floor(100.0)]);
    s.steps(2, NONE);
    let id = fire(
        &mut s,
        ArrowType::Toy,
        Vec2::new(60.0, FLOOR_Y - 1.0),
        0.0,
        1,
    );
    for _ in 0..30 {
        s.step1(NONE);
        assert!(deaths(&s).is_empty());
    }
    assert!(s.pid(0).is_some());
    let a = s.w.arrow(id).copied().unwrap();
    assert!(a.state >= ArrowState::Falling);
}

/// Two dangerous arrows meeting head-on both bounce; a bolt vanishes.
#[test]
fn arrow_clash_and_bolt_vanish() {
    let mut s = Sim::new(&ARENA, &[]);
    let a = fire(&mut s, ArrowType::Normal, Vec2::new(100.0, 100.0), 0.0, -1);
    let b = fire(&mut s, ArrowType::Bolt, Vec2::new(140.0, 100.0), PI, -1);
    for _ in 0..10 {
        s.step(&[]);
    }
    assert_eq!(s.w.arrow(a).unwrap().state, ArrowState::Falling);
    assert!(s.w.arrow(b).is_none(), "the bolt vanished");
}

// ------------------------------------------------------------- orbs, lava

/// Dark orb: darkness 30 frames after pick-up for 600..899 frames.
#[test]
fn dark_orb_duration() {
    let mut s = Sim::new(&ARENA, &[]);
    orbs::do_dark_orb(&mut s.w);
    let mut on = None;
    for f in 1..=2000 {
        s.step(&[]);
        let dark = s.w.items.orbs.dark;
        if on.is_none() && dark {
            on = Some(f);
        }
        if let Some(t) = on
            && !dark
        {
            assert_eq!(t, 30);
            let len = f - t;
            assert!((600..=900).contains(&len), "dark for {len} frames");
            return;
        }
    }
    panic!("darkness never ended");
}

/// Time orb: slow motion 10 frames later, for 300..359 slowed frames (the
/// counters run on TimeMult, so it lasts longer in real frames).
#[test]
fn time_orb_duration() {
    let mut s = Sim::new(&ARENA, &[]);
    orbs::do_time_orb(&mut s.w, false);
    for _ in 0..10 {
        s.step(&[]);
    }
    assert_eq!(s.w.orb.game_rate_target, 0.5);
    let end = s.w.orb.slow_time_end;
    assert!((299.0..360.0).contains(&end), "end counter {end}");
    let mut frames = 0;
    let mut min_rate = 1.0f32;
    while s.w.orb.game_rate_target < 1.0 && frames < 2000 {
        s.step(&[]);
        frames += 1;
        min_rate = min_rate.min(s.w.time.time_rate);
    }
    assert!(min_rate <= 0.5 + 1e-4);
    assert!(frames > 300, "slowed counters take longer ({frames})");
}

/// Space orb: the camera scrolls one full screen over 360 frames.
#[test]
fn space_orb_scrolls_one_screen() {
    let mut s = Sim::new(&ARENA, &[]);
    orbs::do_space_orb(&mut s.w, &s.c);
    s.steps(361, NONE);
    let cam = s.w.items.orbs.camera;
    assert!(
        cam == Vec2::new(320.0, 0.0)
            || cam == Vec2::new(-320.0, 0.0)
            || cam == Vec2::new(0.0, 240.0)
            || cam == Vec2::new(0.0, -240.0),
        "{cam:?}"
    );
}

/// The lava orb's lava kills with credit to whoever took the orb.
#[test]
fn lava_orb_kill_credit() {
    let mut map = ARENA;
    map[1] = "################################";
    let mut s = Sim::new(&map, &[floor(20.0)]);
    s.steps(2, NONE);
    orbs::do_lava_orb(&mut s.w, &s.c, 1);
    let mut died = vec![];
    for _ in 0..400 {
        s.step1(NONE);
        died.extend(deaths(&s));
    }
    assert_eq!(died, vec![(0, 1, 4)]);
}

// ------------------------------------------------------------- pick-ups

/// Shield, wings, boots and the mirror are granted on touch; an item the
/// archer cannot use stays.
#[test]
fn pickups_grant_their_items() {
    let mut s = Sim::new(&ARENA, &[floor(100.0)]);
    s.steps(2, NONE);
    let at = s.pos(0);
    for k in [
        Pickups::Shield,
        Pickups::Wings,
        Pickups::SpeedBoots,
        Pickups::Mirror,
        Pickups::LaserArrows,
    ] {
        pickups::create(&mut s.w, k, at, at, -1, false);
    }
    s.steps(20, NONE);
    let p = s.p(0);
    assert!(p.has_shield && p.has_wings && p.speed_boots && p.invisible);
    assert_eq!(&p.arrows.as_slice()[..2], &[ArrowType::Laser; 2]);
    assert!(s.w.tagged(Tag::PlayerCollectible).is_empty());
    // A full quiver refuses plain arrows.
    give(&mut s, 0, ArrowType::Normal, 6);
    let at = s.pos(0);
    pickups::create(&mut s.w, Pickups::Arrows, at, at, -1, false);
    s.steps(20, NONE);
    assert_eq!(s.w.tagged(Tag::PlayerCollectible).len(), 1, "left in place");
}

/// The trapped chest's bomb goes off 70 frames after opening and can kill
/// the opener (a self-kill).
#[test]
fn bomb_pickup_kills_opener() {
    let mut s = Sim::new(&ARENA, &[floor(100.0)]);
    s.steps(2, NONE);
    let at = s.pos(0);
    pickups::create(&mut s.w, Pickups::Bomb, at, at, 0, false);
    let mut boom = None;
    for f in 1..=100 {
        s.step1(NONE);
        if let Some(d) = deaths(&s).first() {
            boom = Some((f, *d));
            break;
        }
    }
    let (f, d) = boom.expect("exploded");
    assert!((70..=72).contains(&f), "after {f} frames");
    assert_eq!(d, (0, 0, 1));
}

// ------------------------------------------------------------- chests

fn chest(s: &mut Sim, pos: Vec2, ty: ChestType, items: [Option<Pickups>; 3]) -> EntityId {
    treasure::create(
        &mut s.w,
        &ChestSpawn {
            pos,
            ty,
            timer: 5.0,
            contents: items,
        },
        Appear::Time,
    )
}

fn chest_state(s: &Sim, id: EntityId) -> ChestState {
    match s.w.kind(id) {
        Some(Kind::Chest(ch)) => ch.state,
        _ => panic!("no chest"),
    }
}

/// A Time chest appears after its timer, flashes 30 frames, then opens on
/// touch and its item flies out.
#[test]
fn chest_appears_opens_and_gives() {
    let mut s = Sim::new(&ARENA, &[floor(130.0)]);
    s.steps(2, NONE);
    let id = chest(
        &mut s,
        Vec2::new(160.0, 210.0),
        ChestType::Normal,
        [Some(Pickups::Shield), None, None],
    );
    s.steps(3, NONE);
    assert_eq!(chest_state(&s, id), ChestState::WaitingToAppear);
    assert!(!s.w.core(id).visible);
    s.steps(5, NONE);
    assert_eq!(chest_state(&s, id), ChestState::Appearing);
    s.steps(31, NONE);
    assert_eq!(chest_state(&s, id), ChestState::Closed);
    for _ in 0..60 {
        s.step1(inp(1, 0, 0));
        if chest_state(&s, id) >= ChestState::Opening {
            break;
        }
    }
    assert!(chest_state(&s, id) >= ChestState::Opening);
    s.step1(NONE);
    assert_eq!(
        count_kind(
            &s,
            |k| matches!(k, Kind::Pickup(p) if p.kind == Pickups::Shield)
        ),
        1
    );
    s.steps(50, NONE);
    assert_eq!(chest_state(&s, id), ChestState::Opened);
}

/// Arrows open small chests (opener = the arrow's player) but only shake
/// large ones; the arrow bounces either way.
#[test]
fn arrows_open_small_chests_only() {
    let mut s = Sim::new(&ARENA, &[]);
    let small = chest(
        &mut s,
        Vec2::new(200.0, 210.0),
        ChestType::Normal,
        [Some(Pickups::Wings), None, None],
    );
    let big = chest(
        &mut s,
        Vec2::new(100.0, 210.0),
        ChestType::Large,
        [Some(Pickups::Wings), None, None],
    );
    s.steps(100, NONE);
    let a = fire(&mut s, ArrowType::Normal, Vec2::new(170.0, 214.0), 0.0, 2);
    let b = fire(&mut s, ArrowType::Normal, Vec2::new(70.0, 214.0), 0.0, 2);
    s.steps(12, NONE);
    assert!(chest_state(&s, small) >= ChestState::Opening);
    match s.w.kind(small) {
        Some(Kind::Chest(ch)) => assert_eq!(ch.opener, 2),
        _ => unreachable!(),
    }
    assert_eq!(chest_state(&s, big), ChestState::Closed);
    for id in [a, b] {
        assert_eq!(s.w.arrow(id).unwrap().state, ArrowState::Falling);
    }
}

// ------------------------------------------------------------- spawner

fn twilight_spire() -> ItemSetup {
    let names: Vec<String> = [
        "Arrows",
        "Shield",
        "Wings",
        "SpeedBoots",
        "Mirror",
        "DarkOrb",
        "TimeOrb",
        "LavaOrb",
        "SpaceOrb",
        "ChaosOrb",
        "Bomb",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    ItemSetup::default().with_tower(Some(&names), 0.3, false)
}

/// pickups.md §13: Twilight Spire's rates, and 20 000 seeded draws follow
/// them.
#[test]
fn treasure_rates_and_draws() {
    let mut s = Sim::with(&ARENA, &[], |m| {
        m.seed = 77;
        m.items = twilight_spire();
    });
    let r = s.w.items.rates;
    let total: f32 = r.iter().sum();
    let pr = |p: Pickups| r[p as usize] / total;
    assert!((pr(Pickups::Arrows) - 0.30).abs() < 1e-4);
    assert!((pr(Pickups::Shield) - 0.1428).abs() < 1e-3);
    assert!((pr(Pickups::Mirror) - 0.0714).abs() < 1e-3);
    assert!((pr(Pickups::TimeOrb) - 0.0428).abs() < 1e-3);
    assert!((pr(Pickups::Bomb) - 0.0286).abs() < 1e-3);
    assert_eq!(
        r[Pickups::BombArrows as usize],
        0.0,
        "not in the tower mask"
    );
    let mut counts = [0u32; 21];
    for _ in 0..20_000 {
        counts[treasure::get_treasure_spawn(&mut s.w, 0) as usize] += 1;
    }
    for p in [
        Pickups::Arrows,
        Pickups::Shield,
        Pickups::Wings,
        Pickups::DarkOrb,
    ] {
        let f = counts[p as usize] as f32 / 20_000.0;
        assert!((f - pr(p)).abs() < 0.012, "{p:?}: {f} vs {}", pr(p));
    }
    assert_eq!(counts[Pickups::BombArrows as usize], 0);
    // Same seed, same draws.
    let mut t = Sim::with(&ARENA, &[], |m| {
        m.seed = 77;
        m.items = twilight_spire();
    });
    let mut u = Sim::with(&ARENA, &[], |m| {
        m.seed = 77;
        m.items = twilight_spire();
    });
    let a: Vec<Pickups> = (0..50)
        .map(|_| treasure::get_treasure_spawn(&mut t.w, 0))
        .collect();
    let b: Vec<Pickups> = (0..50)
        .map(|_| treasure::get_treasure_spawn(&mut u.w, 0))
        .collect();
    assert_eq!(a, b);
}

/// Variants exclude items (and the chaos orb with any orb).
#[test]
fn treasure_exclusions() {
    let s = Sim::with(&ARENA, &[], |m| {
        m.variant_set.set_all(Variant::NoShield, true, 1);
        m.variant_set.set_all(Variant::AlwaysDark, true, 1);
    });
    let r = s.w.items.rates;
    assert_eq!(r[Pickups::Shield as usize], 0.0);
    assert_eq!(r[Pickups::DarkOrb as usize], 0.0);
    assert_eq!(r[Pickups::ChaosOrb as usize], 0.0);
    assert!(r[Pickups::Wings as usize] > 0.0);
    // Arrows get 60 % of the total by default.
    let total: f32 = r.iter().sum();
    let arrows: f32 = r[..=9].iter().sum();
    assert!((arrows / total - 0.6).abs() < 1e-4);
}

const CHESTS: [&str; 24] = [
    "################################",
    "#..............................#",
    "#..T......T...T..T...T......T..#",
    "#..............................#",
    "#..T......T...T..T...T......T..#",
    "#..............................#",
    "#..T......T...T..T...T......T..#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#...............G..............#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..P........................P..#",
    "#..............................#",
    "#..............................#",
    "################################",
    "################################",
];

fn spawns_for(s: &mut Sim, seed: i32, n: usize) -> Vec<ChestSpawn> {
    s.w.rng = Rng::new(seed);
    let lvl = s.c.level(0).clone();
    treasure::get_chest_spawns_for_level(&mut s.w, &s.c, &lvl.treasure, &lvl.big_treasure, n)
}

/// Small chest counts follow `ChestChances` (≤ 4 for 2 players … ≤ 8 for
/// 12, the port's extrapolation); timers are 10 + [0, 60n).
#[test]
fn chest_counts_and_timers() {
    let mut s = Sim::new(&CHESTS, &[]);
    let mut means = vec![];
    for (n, max) in [(2, 4), (4, 6), (8, 7), (12, 8)] {
        let mut sum = 0;
        for seed in 0..400 {
            let sp = spawns_for(&mut s, seed, n);
            let small: Vec<_> = sp.iter().filter(|c| c.ty == ChestType::Normal).collect();
            if small.is_empty() {
                // A 3 % big chest replaces the small ones.
                assert!(sp.iter().all(|c| c.ty == ChestType::Large) && sp.len() <= 1);
                continue;
            }
            assert!(small.len() <= max, "{n} players: {} chests", small.len());
            for c in &small {
                assert!(c.timer >= 10.0 && c.timer < 10.0 + 60.0 * n as f32);
            }
            sum += small.len();
        }
        means.push(sum as f32 / 400.0);
    }
    assert!(means.windows(2).all(|m| m[1] >= m[0] - 0.05), "{means:?}");
    assert_eq!(spawns_for(&mut s, 5, 4), spawns_for(&mut s, 5, 4), "seeded");
}

/// Symmetrical treasure mirrors chests (same timer and item); always-big
/// gives one large chest with up to 3 items (non-arrow items unique); bomb
/// chests use every position with growing timers.
#[test]
fn chest_variants() {
    let mut s = Sim::with(&CHESTS, &[], |m| {
        m.variant_set.set_all(Variant::SymmetricalTreasure, true, 1);
    });
    for seed in 0..50 {
        let sp = spawns_for(&mut s, seed, 4);
        for c in sp.iter().filter(|c| c.ty == ChestType::Normal) {
            let m = Vec2::new(320.0 - c.pos.x, c.pos.y);
            let twin = sp.iter().find(|o| o.pos == m).expect("mirrored");
            assert_eq!((twin.timer, twin.contents), (c.timer, c.contents));
        }
    }
    let mut s = Sim::with(&CHESTS, &[], |m| {
        m.variant_set.set_all(Variant::AlwaysBigTreasure, true, 1);
    });
    for seed in 0..50 {
        let sp = spawns_for(&mut s, seed, 3);
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].ty, ChestType::Large);
        let items: Vec<Pickups> = sp[0].contents.iter().flatten().copied().collect();
        assert!(!items.is_empty() && items.len() <= 3);
        for (i, a) in items.iter().enumerate() {
            assert_ne!(*a, Pickups::Bomb);
            if !a.is_arrows() {
                assert!(!items[i + 1..].contains(a), "{items:?}");
            }
        }
        assert!((30.0..120.0).contains(&sp[0].timer));
    }
    let mut s = Sim::with(&CHESTS, &[], |m| {
        m.variant_set.set_all(Variant::BombChests, true, 1);
    });
    let sp = spawns_for(&mut s, 3, 2);
    assert_eq!(sp.len(), 18, "every small position");
    assert!(sp.windows(2).all(|w| w[1].timer > w[0].timer));
    assert!(
        sp.iter()
            .all(|c| c.ty == ChestType::AutoOpen && c.contents[0] == Some(Pickups::Bomb))
    );
}

/// A Versus round spawns chests when it starts.
#[test]
fn versus_round_spawns_chests() {
    use tfp_sim::versus::settings::{MatchSettings, Mode};
    let c = Content::from_ascii(&CHESTS);
    let mut found = 0;
    for seed in 0..6 {
        let mut set = MatchSettings::new(Mode::LastManStanding, 2, vec![0]);
        set.seed = seed;
        set.tower_card = false;
        let mut w = tfp_sim::versus::new_match(&c, set);
        for _ in 0..400 {
            w.step(&c, &[]);
        }
        found += w.tagged(Tag::TreasureChest).len();
    }
    assert!(found > 0, "chests in 6 rounds");
}

// ------------------------------------------------------------- HUD

/// The quiver HUD lists the arrows in firing order and shows for 40 frames
/// after a shot.
#[test]
fn quiver_hud() {
    let mut s = Sim::new(&ARENA, &[floor(100.0)]);
    s.steps(2, NONE);
    let id = s.pid(0).unwrap();
    s.w.player_mut(id)
        .unwrap()
        .arrows
        .add_arrows(&[ArrowType::Bomb]);
    let v = View::build(&s.w, &s.c, ViewOptions::default());
    let q = &v.quivers[0];
    assert_eq!(q.arrows, s.p(0).arrows.as_slice().to_vec());
    assert_eq!(q.arrows[0], ArrowType::Bomb);
    assert!(q.shown, "not hidden when standing");
    s.step1(inp(0, 1, 0));
    s.step1(inp(0, 1, 0));
    let v = View::build(&s.w, &s.c, ViewOptions::default());
    assert!(!v.quivers[0].shown, "ducking hides it");
}

// ------------------------------------------------------------- determinism

/// Four archers with every special arrow type, chests, orbs and lava;
/// restoring snapshots and re-simulating reproduces every checksum.
#[test]
fn busy_scene_rollback_is_exact() {
    const FRAMES: usize = 3000;
    const TYPES: [ArrowType; 10] = [
        ArrowType::Bomb,
        ArrowType::SuperBomb,
        ArrowType::Laser,
        ArrowType::Bramble,
        ArrowType::Drill,
        ArrowType::Bolt,
        ArrowType::Toy,
        ArrowType::Feather,
        ArrowType::Trigger,
        ArrowType::Prism,
    ];
    let mut map = CHESTS;
    map[9] = "#...P..........P.........P..P..#";
    map[15] = "#####......##########......#####";
    let c = Content::from_ascii(&map);
    let mut setup = tfp_sim::MatchSetup {
        seed: 4321,
        respawn_frames: Some(45),
        ..tfp_sim::MatchSetup::default()
    };
    for i in 0..4 {
        setup.players[i] = Some(tfp_sim::PlayerSetup {
            archer: i as u16,
            arrows: 3,
            ..Default::default()
        });
    }
    let mut w = World::new(&c, &setup);
    w.step(&c, &[]);
    // Respawned archers come back with plain arrows: hand out specials
    // again every 120 frames (the same in the run and every re-run).
    let refill = |w: &mut World, f: usize| {
        if !f.is_multiple_of(120) {
            return;
        }
        for (k, id) in w.players().collect::<Vec<_>>().into_iter().enumerate() {
            let r = f / 120 + k;
            let p = w.player_mut(id).unwrap();
            p.arrows = Quiver::new(2, TYPES[r * 2 % 10]);
            p.arrows.add_arrows(&[TYPES[(r * 2 + 1) % 10]; 2]);
        }
    };
    treasure::round_start(&mut w, &c, 4, false, true);
    for (k, kind) in [
        Pickups::ChaosOrb,
        Pickups::TimeOrb,
        Pickups::LavaOrb,
        Pickups::DarkOrb,
    ]
    .into_iter()
    .enumerate()
    {
        let at = Vec2::new(60.0 + 70.0 * k as f32, 80.0);
        pickups::create(&mut w, kind, at, at, -1, false);
    }
    // The shared bots on the live world (inputs recorded for the
    // re-simulations).
    let mut bots: Vec<tfp_sim::bot::Bot> = (0..4)
        .map(|i| tfp_sim::bot::Bot::new(11 + i * 7919))
        .collect();
    let mut inputs: Vec<Vec<PlayerInput>> = Vec::with_capacity(FRAMES);
    let mut sums = Vec::with_capacity(FRAMES);
    let mut snaps: Vec<(usize, World)> = vec![];
    let mut pick = Rng::new(9);
    let (mut booms, mut kills) = (0, 0);
    for f in 0..FRAMES {
        if f % 25 == 0 && pick.chance(0.5) {
            snaps.push((f, w.clone()));
        }
        refill(&mut w, f);
        let i: Vec<PlayerInput> = bots
            .iter_mut()
            .enumerate()
            .map(|(k, b)| b.input(&w, &c, k as u8))
            .collect();
        w.step(&c, &i);
        inputs.push(i);
        sums.push(w.checksum());
        for e in w.events() {
            match e {
                Event::Death { .. } => kills += 1,
                Event::Sound { name, .. } if name.contains("Explode") => booms += 1,
                _ => {}
            }
        }
    }
    eprintln!(
        "{FRAMES} frames: {kills} deaths, {booms} explosions, {} snapshots",
        snaps.len()
    );
    assert!(kills >= 3, "the bots fight");
    assert!(booms >= 5, "special arrows go off");
    assert!(snaps.len() >= 20);
    for (f, snap) in &snaps {
        let mut re = snap.clone();
        for k in *f..FRAMES.min(f + 300) {
            refill(&mut re, k);
            re.step(&c, &inputs[k]);
            assert_eq!(re.checksum(), sums[k], "frame {k} from snapshot {f}");
        }
    }
}
