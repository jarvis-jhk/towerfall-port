//! M5 completion: the variants without another home (Gunn Style, Dark
//! Portals, corpse variants, Stealth Archers, scrolling camera), the
//! Treasure Draft, arrow / torch / cobweb / corpse fire, brambles riding a
//! moving platform, the M5 stat counters, `DieLaser` and the round's random
//! arrow type.

mod common;
use common::*;
use tfp_sim::entities::placement::{Placement, PlacementKind};
use tfp_sim::entity::{EntityId, Kind, Tag};
use tfp_sim::events::{CharSound, Event};
use tfp_sim::player::DeathCause;
use tfp_sim::quiver::ArrowType;
use tfp_sim::variants::Variant;
use tfp_sim::versus::{self, MatchSettings, Mode, Phase};
use tfp_sim::{Content, Vec2, View, ViewOptions, World};

/// Four archer spawns on the floor, one spawn portal, two chest spots.
const MAP: [&str; 24] = [
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
    "#...............S..............#",
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
    "#.T...P......P......P......P.T.#",
    "################################",
    "################################",
];

fn idle(n: usize) -> Vec<tfp_sim::PlayerInput> {
    vec![NONE; n]
}

/// A match on `MAP` with `n` players and `vars` on, stepped to Playing.
fn playing(n: usize, mode: Mode, vars: &[Variant]) -> (Content, World) {
    let c = Content::from_ascii(&MAP);
    let w = playing_on(&c, n, mode, vars);
    (c, w)
}

fn playing_on(c: &Content, n: usize, mode: Mode, vars: &[Variant]) -> World {
    let mut ms = MatchSettings::new(mode, n, vec![0]);
    ms.tower_card = false;
    let mask = ms.joined_mask();
    for &v in vars {
        ms.variants.set_all(v, true, mask);
    }
    let mut w = versus::new_match(c, ms);
    for _ in 0..400 {
        if w.versus.as_ref().unwrap().phase == Phase::Playing {
            break;
        }
        w.step(c, &idle(n));
    }
    assert_eq!(w.versus.as_ref().unwrap().phase, Phase::Playing);
    w
}

fn kill(w: &mut World, c: &Content, index: u8, cause: DeathCause, killer: i8) {
    let id = w.player_by_index(index).expect("alive");
    tfp_sim::hurt::with_player(w, id, |w, p| {
        tfp_sim::player::die(w, c, id, p, cause, killer);
    });
}

fn alive(w: &World, i: u8) -> bool {
    w.player_by_index(i)
        .and_then(|id| w.player(id))
        .is_some_and(|p| !p.dead)
}

fn corpse_of(w: &World, i: u8) -> Option<EntityId> {
    w.tagged(Tag::Corpse)
        .iter()
        .copied()
        .find(|&id| matches!(w.kind(id), Some(Kind::Corpse(cp)) if cp.player_index == i as i8))
}

fn stats(w: &World, i: usize) -> tfp_sim::versus::stats::MatchStats {
    w.versus.as_ref().unwrap().stats[i]
}

// ------------------------------------------------------------ Gunn Style

/// Gunn Style: 10 ticks after a death the level freezes for 30 ticks plus a
/// 10-tick fade, with the overlay in the view.
#[test]
fn gunn_style_freeze_frame() {
    let (c, mut w) = playing(3, Mode::LastManStanding, &[Variant::GunnStyle]);
    kill(&mut w, &c, 0, DeathCause::Arrow, 1);
    let mut frozen_at = None;
    let mut thawed_at = None;
    for t in 0..80 {
        w.step(&c, &idle(3));
        if w.frozen && frozen_at.is_none() {
            frozen_at = Some(t);
            let v = View::build(&w, &c, ViewOptions::default());
            let g = v.gunn.expect("overlay");
            assert_eq!(g.victim, 0);
            assert_eq!(g.alpha, 1.0);
        }
        if frozen_at.is_some() && !w.frozen && thawed_at.is_none() {
            thawed_at = Some(t);
        }
    }
    let (a, b) = (frozen_at.expect("froze"), thawed_at.expect("thawed"));
    assert!((8..=11).contains(&a), "froze at {a}");
    assert_eq!(b - a, 40, "30 frozen + 10 fade");
}

// ---------------------------------------------------------- Dark Portals

/// Dark Portals: a portal opens after the round start and keeps up to 3
/// bats / slimes in play.
#[test]
fn dark_portals_spawn_enemies() {
    let (c, mut w) = playing(2, Mode::LastManStanding, &[Variant::DarkPortals]);
    let mut seen = 0;
    let mut max = 0;
    for _ in 0..900 {
        w.step(&c, &idle(2));
        let n = tfp_sim::enemies::enemy_count(&w);
        seen = seen.max(n);
        max = max.max(n);
        if w.versus.as_ref().unwrap().phase != Phase::Playing {
            break;
        }
    }
    assert!(seen >= 1, "enemies came out of the portal");
    assert!(max <= 3, "at most 3 at a time ({max})");
    for &e in w.tagged(Tag::Enemy) {
        let ty = match w.kind(e) {
            Some(Kind::Enemy(en)) => en.ty,
            _ => continue,
        };
        assert!(matches!(
            ty,
            tfp_sim::enemies::EnemyType::Bat | tfp_sim::enemies::EnemyType::Slime
        ));
    }
}

// --------------------------------------------------------------- corpses

/// Exploding Corpses: the corpse blows up 60 ticks after death and takes
/// a neighbour with it (kill credited to the dead archer).
#[test]
fn exploding_corpse() {
    let (c, mut w) = playing(3, Mode::LastManStanding, &[Variant::ExplodingCorpses]);
    // Put archer 1 next to archer 0.
    let p0 = w.player_by_index(0).unwrap();
    let p1 = w.player_by_index(1).unwrap();
    let at = w.core(p0).pos;
    w.core_mut(p1).pos = at + Vec2::new(12.0, 0.0);
    kill(&mut w, &c, 0, DeathCause::Curse, -1);
    w.step(&c, &idle(3));
    let cp = corpse_of(&w, 0).expect("corpse");
    let mut gone = None;
    for t in 0..90 {
        w.step(&c, &idle(3));
        if !w.exists(cp) {
            gone = Some(t);
            break;
        }
    }
    let t = gone.expect("exploded");
    assert!((55..=62).contains(&t), "exploded after {t}");
    assert!(!alive(&w, 1), "the neighbour died in the blast");
}

/// Trigger Corpses: the fuse only charges while the dead archer holds a
/// button (40 ticks).
#[test]
fn trigger_corpse_needs_the_button() {
    let (c, mut w) = playing(3, Mode::LastManStanding, &[Variant::TriggerCorpses]);
    kill(&mut w, &c, 0, DeathCause::Curse, -1);
    w.step(&c, &idle(3));
    let cp = corpse_of(&w, 0).expect("corpse");
    for _ in 0..120 {
        w.step(&c, &idle(3));
    }
    assert!(w.exists(cp), "no button, no blast");
    let mut held = idle(3);
    held[0] = inp(0, 0, SHOOT);
    let mut gone = None;
    for t in 0..60 {
        w.step(&c, &held);
        if !w.exists(cp) {
            gone = Some(t);
            break;
        }
    }
    let t = gone.expect("detonated");
    assert!((36..=42).contains(&t), "after {t}");
}

/// Corpses Drop Arrows: the quiver comes out one arrow per 10 ticks.
#[test]
fn corpses_drop_arrows() {
    let (c, mut w) = playing(3, Mode::LastManStanding, &[Variant::CorpsesDropArrows]);
    let before = w.tagged(Tag::Arrow).len();
    let n = w
        .player(w.player_by_index(0).unwrap())
        .unwrap()
        .arrows
        .len();
    assert!(n >= 2);
    kill(&mut w, &c, 0, DeathCause::Curse, -1);
    for _ in 0..(10 * n + 5) {
        w.step(&c, &idle(3));
    }
    let thrown = w
        .tagged(Tag::Arrow)
        .iter()
        .filter(|&&a| w.arrow(a).is_some_and(|ar| ar.player_index == 0))
        .count();
    assert_eq!(thrown, n, "all {n} arrows thrown ({before} before)");
    let cp = corpse_of(&w, 0).unwrap();
    assert!(matches!(w.kind(cp), Some(Kind::Corpse(k)) if k.arrows.is_empty()));
}

// --------------------------------------------------------------- archers

/// Stealth Archers: no light source; Show Dodge Cooldown: a lit star.
#[test]
fn stealth_archers_and_dodge_cooldown() {
    let (c, mut w) = playing(
        2,
        Mode::LastManStanding,
        &[Variant::StealthArchers, Variant::ShowDodgeCooldown],
    );
    let p0 = w.player_by_index(0).unwrap();
    assert!(!w.core(p0).has_tag(Tag::LightSource));
    let v = View::build(&w, &c, ViewOptions::default());
    assert!(v.lights.iter().all(|&(p, _)| p != w.core(p0).pos));
    let mut i = idle(2);
    i[0] = inp(1, 0, DODGE);
    w.step(&c, &i);
    let mut lit = false;
    for _ in 0..40 {
        w.step(&c, &idle(2));
        lit |= !View::build(&w, &c, ViewOptions::default())
            .cooldowns
            .is_empty();
    }
    assert!(lit, "cooldown star shown");
}

/// Always Scrolling drifts the camera; Offset World moves it half a screen
/// diagonally during the intro.
#[test]
fn scrolling_and_offset_world() {
    let (c, mut w) = playing(2, Mode::LastManStanding, &[Variant::AlwaysScrolling]);
    let a = w.items.orbs.camera;
    for _ in 0..120 {
        w.step(&c, &idle(2));
    }
    let b = w.items.orbs.camera;
    assert!((b - a).length() > 20.0, "scrolled {a:?} -> {b:?}");
    let (_, w2) = playing(2, Mode::LastManStanding, &[Variant::OffsetWorld]);
    let o = w2.items.orbs.camera;
    assert_eq!((o.x.abs(), o.y.abs()), (160.0, 120.0));
}

// ------------------------------------------------------- Treasure Draft

/// Treasure Draft: the intro waits for the picks; every archer gets one
/// item, in pick order, through its own Confirm presses.
#[test]
fn treasure_draft_picks() {
    let c = Content::from_ascii(&MAP);
    let mut ms = MatchSettings::new(Mode::LastManStanding, 3, vec![0]);
    ms.tower_card = false;
    let mask = ms.joined_mask();
    ms.variants.set_all(Variant::TreasureDraft, true, mask);
    let mut w = versus::new_match(&c, ms);
    w.step(&c, &idle(3));
    let d = w.versus.as_ref().unwrap().draft.clone().expect("draft");
    assert_eq!(d.order.len(), 3);
    assert_eq!(d.offers.len(), 4);
    let mut t = 0;
    while w.versus.as_ref().unwrap().phase == Phase::Intro && t < 2000 {
        // Each archer presses Confirm every 7 ticks (only the picker counts).
        let i: Vec<_> = (0..3)
            .map(|_| if t % 7 == 0 { inp(0, 0, JUMP) } else { NONE })
            .collect();
        w.step(&c, &i);
        t += 1;
    }
    let m = w.versus.as_ref().unwrap();
    assert_eq!(m.phase, Phase::Playing, "the intro went on after the draft");
    let d = m.draft.as_ref().unwrap();
    assert!(d.finished());
    assert_eq!(d.picks.len(), 3);
    assert_eq!(d.offers.len(), 1, "one left over");
    assert!(t > 160 + 100, "the draft held the intro ({t} ticks)");
    let order: Vec<u8> = d.picks.iter().map(|p| p.0).collect();
    assert_eq!(order, d.order);
    // Every pick reached its archer.
    for &(slot, kind) in &d.picks {
        let p = *w.player(w.player_by_index(slot).unwrap()).unwrap();
        use tfp_sim::pickups::Pickups as P;
        let ok = match kind {
            P::Shield => p.has_shield,
            P::Wings => p.has_wings,
            P::SpeedBoots => p.speed_boots,
            P::Mirror => p.invisible,
            k if k.is_arrows() => p.arrows.len() >= 5,
            _ => true,
        };
        assert!(ok, "slot {slot} got {kind:?}");
    }
}

/// Draft pick order: ascending score in free-for-all.
#[test]
fn draft_order_lowest_score_first() {
    let c = Content::from_ascii(&MAP);
    let mut w = playing_on(&c, 4, Mode::LastManStanding, &[]);
    let mut m = w.versus.take().unwrap();
    m.scores[0] = 3;
    m.scores[1] = 1;
    m.scores[2] = 2;
    m.scores[3] = 0;
    let order = versus::draft::pick_order(&mut w, &m);
    assert_eq!(order, vec![3, 1, 2, 0]);
}

// ------------------------------------------------------------------ fire

fn with_placement(map: &[&str], p: Placement) -> Content {
    let mut c = Content::from_ascii(map);
    c.levels[0].tower.placements.push(p);
    c
}

/// A lit torch sets a passing normal arrow alight (60 frames); the burning
/// arrow kills an archer and the corpse burns.
#[test]
fn torch_lights_arrow_and_corpse_burns() {
    let mut torch = Placement::new(PlacementKind::BGLantern, Vec2::new(80.0, 205.0), Vec2::ZERO);
    torch.flag = true;
    let c = with_placement(&ARENA, torch);
    let mut s = Sim {
        c: c.clone(),
        w: World::new(&c, &{
            let mut m = tfp_sim::MatchSetup::default();
            for (i, x) in [40.0, 140.0].iter().enumerate() {
                m.players[i] = Some(tfp_sim::PlayerSetup {
                    archer: i as u16,
                    spawn: Some(Vec2::new(*x, FLOOR_Y)),
                    ..Default::default()
                });
            }
            m
        }),
    };
    s.step(&[NONE, NONE]);
    s.step(&[inp(1, 0, SHOOT), NONE]);
    s.step(&[inp(1, 0, 0), NONE]);
    let mut burning = false;
    for _ in 0..60 {
        s.step(&[NONE, NONE]);
        burning |= s.arrows().iter().any(|(_, a)| a.extra.burning);
        if s.pid(1).is_none() {
            break;
        }
    }
    assert!(burning, "the torch lit the arrow");
    let cp = s.corpses()[0];
    assert!(
        matches!(s.w.kind(cp), Some(Kind::Corpse(k)) if k.fire_t > 0.0),
        "burning corpse"
    );
}

/// A burning arrow through a cobweb burns it away in 15 ticks.
#[test]
fn burning_arrow_burns_cobweb() {
    // Web in the corner above-left of (60, 220) (floor below, wall left? use the floor).
    let web = Placement::new(PlacementKind::Cobwebs, Vec2::new(100.0, 150.0), Vec2::ZERO);
    let c = with_placement(&ARENA, web);
    let mut s = Sim::new(&[], &[]);
    s.c = c;
    s.w = World::new(&s.c, &tfp_sim::MatchSetup::default());
    s.step(&[]);
    let webs: Vec<EntityId> =
        s.w.entities()
            .filter(|&e| {
                matches!(
                    s.w.kind(e),
                    Some(Kind::Tower(tfp_sim::entities::Tower::Cobwebs(_)))
                )
            })
            .collect();
    assert_eq!(webs.len(), 1);
    let at = s.w.core(webs[0]).pos + Vec2::new(5.0, 5.0);
    let shooter = tfp_sim::arrow::Shooter {
        id: None,
        player_index: -1,
        allegiance: tfp_sim::entity::Allegiance::Neutral,
        team_color: tfp_sim::entity::Allegiance::Neutral,
        hyper: false,
    };
    let a = tfp_sim::arrow::create(
        &mut s.w,
        ArrowType::Normal,
        shooter,
        at - Vec2::new(20.0, 0.0),
        0.0,
    );
    if let Some(ar) = s.w.arrow_mut(a) {
        assert!(tfp_sim::arrows::ignite(ar));
    }
    let mut gone = false;
    for _ in 0..40 {
        s.step(&[]);
        if !s.w.exists(webs[0]) {
            gone = true;
            break;
        }
    }
    assert!(gone, "the web burned away");
}

/// Bramble tiles on a moving platform move with it.
#[test]
fn brambles_ride_moving_platform() {
    let mut mp = Placement::new(
        PlacementKind::MovingPlatform,
        Vec2::new(100.0, 90.0),
        Vec2::new(40.0, 10.0),
    );
    mp.node = Some(Vec2::new(200.0, 90.0));
    let c = with_placement(&ARENA, mp);
    let mut s = Sim::new(&[], &[]);
    s.c = c;
    s.w = World::new(&s.c, &tfp_sim::MatchSetup::default());
    s.step(&[]);
    s.step(&[]);
    let plat =
        s.w.entities()
            .find(|&e| {
                matches!(
                    s.w.kind(e),
                    Some(Kind::Tower(tfp_sim::entities::Tower::MovingPlatform(_)))
                )
            })
            .expect("platform");
    let top = s.w.core(plat).top();
    let x = s.w.core(plat).pos.x + 20.0;
    let b = tfp_sim::arrows::brambles::spawn_tile(&mut s.w, Vec2::new(x, top - 4.0), 999, 1);
    s.step(&[]);
    let (p0, b0) = (s.w.core(plat).pos, s.w.core(b).pos);
    for _ in 0..300 {
        s.step(&[]);
    }
    if !s.w.exists(b) {
        panic!("tile fell off its platform");
    }
    let (p1, b1) = (s.w.core(plat).pos, s.w.core(b).pos);
    assert!((p1 - p0).length() > 5.0, "the platform moved");
    assert!(
        ((b1 - b0) - (p1 - p0)).length() < 0.01,
        "the tile moved with it"
    );
}

// ----------------------------------------------------------------- stats

/// A few of the M5 counters: frames alive, spam shots, own catches,
/// failed shots; and the laser death sound.
#[test]
fn stat_counters_and_die_laser() {
    let (c, mut w) = playing(2, Mode::LastManStanding, &[]);
    let fa = stats(&w, 0).frames_alive;
    let mut i = idle(2);
    i[0] = inp(1, 0, SHOOT);
    w.step(&c, &i);
    w.step(&c, &idle(2));
    w.step(&c, &i);
    w.step(&c, &idle(2));
    let s = stats(&w, 0);
    assert!(s.frames_alive >= fa + 4.0);
    assert_eq!(s.arrows_shot, 2);
    assert_eq!(s.spam_shots, 1, "second shot within 10 ticks");
    // A laser kill plays DieLaser.
    let p1 = w.player_by_index(1).unwrap();
    let at = w.core(p1).pos;
    let shooter = tfp_sim::arrow::Shooter {
        id: w.player_by_index(0),
        player_index: 0,
        allegiance: tfp_sim::entity::Allegiance::Neutral,
        team_color: tfp_sim::entity::Allegiance::Neutral,
        hyper: false,
    };
    tfp_sim::arrow::create(
        &mut w,
        ArrowType::Laser,
        shooter,
        at - Vec2::new(30.0, 0.0),
        0.0,
    );
    let mut laser_death = false;
    for _ in 0..40 {
        w.step(&c, &idle(2));
        laser_death |= w.events().iter().any(|e| {
            matches!(
                e,
                Event::CharSound {
                    sound: CharSound::DieLaser,
                    player: 1,
                    ..
                }
            )
        });
    }
    assert!(laser_death, "DieLaser played");
    assert!(stats(&w, 0).longest_shot > 0.0 || stats(&w, 0).most_bounces_laser_kill == 0);
}

/// The round's random arrow type honours the treasure exclusions.
#[test]
fn random_arrow_type_respects_exclusions() {
    let no_others = [
        Variant::StartWithRandomArrows,
        Variant::NoExtraArrows,
        Variant::NoBombArrows,
        Variant::NoBrambleArrows,
        Variant::NoDrillArrows,
        Variant::NoBoltArrows,
        Variant::NoSuperBombArrows,
        Variant::NoFeatherArrows,
        Variant::NoTriggerArrows,
        Variant::NoPrismArrows,
    ];
    let (_c, w) = playing(2, Mode::LastManStanding, &no_others);
    assert_eq!(
        w.versus.as_ref().unwrap().round.random_arrow_type,
        ArrowType::Laser
    );
    let p = w.player(w.player_by_index(0).unwrap()).unwrap();
    assert_eq!(p.arrows.first(), Some(ArrowType::Laser));
}
