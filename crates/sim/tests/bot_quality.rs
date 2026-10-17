//! Bot quality measurements (ignored; run with `--ignored --nocapture`,
//! release build): the shared `tfp_sim::bot::Bot` against the old
//! versus-test bot in Versus (kills, rounds, draws, shots, death causes)
//! and how far two shared bots get in Dark World without assists. Plus a
//! quick deterministic check of the shared bot on the built-in arena.
use tfp_assets::install::Installs;
use tfp_sim::actor::wrap_diff_x;
use tfp_sim::rng::Rng;
use tfp_sim::variants::Variant;
use tfp_sim::versus::{self, MatchLength, MatchSettings, Mode, Phase};
use tfp_sim::{Content, Event, PlayerInput, World};

/// Aggressive bot: walks to the nearest enemy, shoots when roughly aligned,
/// jumps and dodges now and then. Presses Jump a lot, which also confirms
/// the tower card and the results screens.
struct OldBot {
    rng: Rng,
    shoot_hold: u8,
}

impl OldBot {
    fn input(&mut self, w: &World, c: &Content, slot: usize) -> PlayerInput {
        let width = c.level(w.level).width as f32;
        let Some(me) = w.player_by_index(slot as u8) else {
            // Dead or not spawned: still press Jump sometimes (confirms).
            let b = if self.rng.chance(0.1) {
                PlayerInput::JUMP
            } else {
                0
            };
            return PlayerInput::dpad(0, 0, b);
        };
        let my = w.core(me).pos;
        let my_all = w.core(me).allegiance;
        let mut best: Option<(f32, f32, f32)> = None;
        for id in w.players() {
            if id == me {
                continue;
            }
            let Some(p) = w.player(id) else { continue };
            if p.dead
                || (my_all != tfp_sim::entity::Allegiance::Neutral
                    && w.core(id).allegiance == my_all)
            {
                continue;
            }
            let o = w.core(id).pos;
            let dx = wrap_diff_x(my.x, o.x, width);
            let dy = o.y - my.y;
            let d = dx * dx + dy * dy;
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, dx, dy));
            }
        }
        let mut b = 0u8;
        let (mut x, mut y) = (0i8, 0i8);
        if let Some((_, dx, dy)) = best {
            x = if dx > 4.0 {
                1
            } else if dx < -4.0 {
                -1
            } else {
                0
            };
            if dy < -20.0 && self.rng.chance(0.15) {
                b |= PlayerInput::JUMP;
            }
            let aligned = dy.abs() < 10.0 || (dx.abs() - dy.abs()).abs() < 12.0 || dx.abs() < 6.0;
            if aligned {
                y = if dy < -10.0 {
                    -1
                } else if dy > 10.0 {
                    1
                } else {
                    0
                };
                if dx.abs() < 6.0 {
                    x = 0;
                }
                if self.shoot_hold == 0 && self.rng.chance(0.25) {
                    self.shoot_hold = 4;
                }
            }
        }
        if self.shoot_hold > 0 {
            self.shoot_hold -= 1;
            if self.shoot_hold > 0 {
                b |= PlayerInput::SHOOT;
            }
        }
        if self.rng.chance(0.03) {
            b |= PlayerInput::JUMP;
        }
        if self.rng.chance(0.01) {
            b |= PlayerInput::DODGE;
        }
        PlayerInput::dpad(x, y, b)
    }
}

enum B {
    Old(OldBot),
    New(tfp_sim::bot::Bot),
}
impl B {
    fn input(&mut self, w: &World, c: &Content, k: usize) -> PlayerInput {
        match self {
            B::Old(b) => b.input(w, c, k),
            B::New(b) => b.input(w, c, k as u8),
        }
    }
}

fn run(c: &Content, mode: Mode, n: usize, sudden: bool, new: bool, frames: u32) -> (u32, u32, u32) {
    let (mut kills, mut rounds, mut draws) = (0, 0, 0);
    let mut shots = 0;
    let mut causes = [0u32; 16];
    for seed in 0..3 {
        let mut s = MatchSettings::new(mode, n, (0..c.levels.len()).collect());
        s.length = MatchLength::Epic;
        s.seed = seed;
        s.tower_card = false;
        if sudden {
            let m = s.joined_mask();
            s.variants.set_all(Variant::SuddenDeath, true, m);
        }
        let mut w = versus::new_match(c, s);
        let mut bots: Vec<B> = (0..n)
            .map(|i| {
                if new {
                    B::New(tfp_sim::bot::Bot::new(seed * 100 + i as i32))
                } else {
                    B::Old(OldBot {
                        rng: Rng::new(seed * 100 + i as i32),
                        shoot_hold: 0,
                    })
                }
            })
            .collect();
        let mut last = Phase::Intro;
        let mut before = [0; 12];
        for _ in 0..frames {
            let i: Vec<PlayerInput> = (0..n).map(|k| bots[k].input(&w, c, k)).collect();
            w.step(c, &i);
            for e in w.events() {
                if let Event::Death {
                    player,
                    killer,
                    cause,
                } = e
                {
                    causes[*cause as usize] += 1;
                    if *killer >= 0 && *killer != *player as i8 {
                        kills += 1;
                    }
                }
                if let Event::CharSound {
                    sound: tfp_sim::events::CharSound::FireArrow,
                    ..
                } = e
                {
                    shots += 1;
                }
            }
            let m = w.versus.as_ref().unwrap();
            if m.phase == Phase::Playing && last != Phase::Playing {
                before = m.scores;
            }
            if m.phase == Phase::Results && last == Phase::Playing {
                rounds += 1;
                if m.scores == before {
                    draws += 1;
                }
            }
            last = m.phase;
            if m.phase == Phase::MatchResults || m.phase == Phase::Finished {
                break;
            }
        }
    }
    println!("   shots {shots} causes {:?}", causes);
    (kills, rounds, draws)
}

#[test]
#[ignore]
fn bot_quality() {
    let i = Installs::discover();
    let Some(inst) = i.steam.clone() else { return };
    let levels = tfp_sim::loader::versus_levels(&inst).unwrap();
    let c = tfp_sim::loader::content_from_install(&inst, &levels).unwrap();
    let f = 60 * 60 * 6;
    for (name, mode, n, sd) in [
        ("HH4", Mode::Headhunters, 4, false),
        ("LMS4", Mode::LastManStanding, 4, false),
        ("LMS4-sudden", Mode::LastManStanding, 4, true),
    ] {
        for new in [false, true] {
            let (k, r, d) = run(&c, mode, n, sd, new, f);
            println!(
                "{name} {} kills {k} rounds {r} draws {d}",
                if new { "NEW" } else { "old" }
            );
        }
    }
    if let Some(inst) = i.eight_player.clone() {
        let levels = tfp_sim::loader::versus_levels(&inst).unwrap();
        let c = tfp_sim::loader::content_from_install(&inst, &levels).unwrap();
        for new in [false, true] {
            let (k, r, d) = run(&c, Mode::LastManStanding, 12, false, new, f);
            println!(
                "LMS12 {} kills {k} rounds {r} draws {d}",
                if new { "NEW" } else { "old" }
            );
        }
    }
}

#[test]
#[ignore]
fn bot_darkworld() {
    use tfp_sim::darkworld::{self, DwPhase, DwSettings};
    let i = Installs::discover();
    let Some(inst) = i.steam.clone() else { return };
    let c = tfp_sim::loader::dark_world_content(&inst).unwrap();
    for tower in 0..4 {
        for seed in 0..2 {
            let mut s = DwSettings::new(tower, darkworld::NORMAL, 2);
            s.seed = seed;
            let mut w = darkworld::new_dark_world(&c, s);
            let mut bots: Vec<tfp_sim::bot::Bot> = (0..2)
                .map(|k| tfp_sim::bot::Bot::new(seed * 10 + k))
                .collect();
            let mut f = 0;
            let mut kills = 0;
            while f < 60 * 60 * 10 {
                let inp: Vec<PlayerInput> =
                    (0..2).map(|k| bots[k].input(&w, &c, k as u8)).collect();
                w.step(&c, &inp);
                for e in w.events() {
                    if let Event::Death { cause, killer, .. } = e {
                        println!("   death cause {cause} killer {killer} f {f}");
                    }
                }
                f += 1;
                let d = w.dark_world.as_ref().unwrap();
                if d.phase == DwPhase::GameOver && d.menu_ready {
                    break;
                }
                if d.phase == DwPhase::Complete {
                    break;
                }
                let _ = &mut kills;
            }
            let d = w.dark_world.as_ref().unwrap();
            println!(
                "tower {tower} seed {seed}: frames {f} level {} of {} phase {:?} deaths {:?}",
                d.round_index,
                d.levels(&c).len(),
                d.phase,
                d.deaths
            );
        }
    }
}

const ARENA: [&str; 24] = [
    "################################",
    "#..............................#",
    "#...P......................P...#",
    "#######....############....#####",
    "#..............................#",
    "#..............................#",
    "......P................P........",
    "#......####..........####......#",
    "##########............##########",
    "#..............................#",
    "#..............................#",
    "#.........#####.......#####....#",
    "#..............................#",
    "#..............................#",
    "#....-----..........-----......#",
    "#..............................#",
    "#..............................#",
    "#......####...........####.....#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "#..............................#",
    "################################",
    "################################",
];

/// The shared bot fights (kills happen) and is deterministic.
#[test]
fn shared_bot_fights_deterministically() {
    let c = Content::from_ascii(&ARENA);
    let run = || {
        let mut s = MatchSettings::new(Mode::Headhunters, 4, vec![0]);
        s.tower_card = false;
        s.seed = 11;
        let mut w = versus::new_match(&c, s);
        let mut bots: Vec<tfp_sim::bot::Bot> = (0..4).map(tfp_sim::bot::Bot::new).collect();
        let mut kills = 0;
        for _ in 0..6000 {
            let i: Vec<PlayerInput> = (0..4).map(|k| bots[k].input(&w, &c, k as u8)).collect();
            w.step(&c, &i);
            for e in w.events() {
                if let Event::Death { player, killer, .. } = e
                    && *killer >= 0
                    && *killer != *player as i8
                {
                    kills += 1;
                }
            }
        }
        (kills, w.checksum())
    };
    let (k1, s1) = run();
    let (k2, s2) = run();
    assert_eq!((k1, s1), (k2, s2), "deterministic");
    assert!(k1 >= 5, "the bots score kills ({k1})");
}

/// Reference: a verbatim copy of the game crate's co-op bot
/// (`crates/game/src/coop_ui/bot.rs`) to compare the shared bot against.
#[allow(dead_code, clippy::all)]
mod coopref {

    use tfp_sim::entity::{EntityId, Kind, Tag};
    use tfp_sim::{Content, PlayerInput, Vec2, World};

    fn wrap_d(a: f32, b: f32, size: f32) -> f32 {
        let mut d = (b - a) % size;
        if d > size / 2.0 {
            d -= size;
        } else if d < -size / 2.0 {
            d += size;
        }
        d
    }

    /// Living targets: enemies (sight point) and dummies (body centre).
    fn targets(w: &World) -> Vec<Vec2> {
        let mut v = vec![];
        for &e in w.tagged(Tag::Enemy) {
            if let Some(Kind::Enemy(en)) = w.kind(e)
                && en.alive()
                && w.core(e).collidable
            {
                v.push(w.core(e).pos + en.sight);
            }
        }
        // Dark World: boss bodies and eyes while they can be hit, shield orbs.
        for &e in w.tagged(Tag::Enemy) {
            if let Some(Kind::Dw(p)) = w.kind(e)
                && w.core(e).collidable
                && matches!(
                    p,
                    tfp_sim::darkworld::Part::Body | tfp_sim::darkworld::Part::Eye(_)
                )
            {
                v.push(w.core(e).pos + Vec2::new(0.0, 4.0));
            }
        }
        for e in w.entities() {
            if let Some(Kind::Dw(tfp_sim::darkworld::Part::ShieldOrb { .. })) = w.kind(e) {
                v.push(w.core(e).pos + Vec2::new(0.0, 4.0));
            }
        }
        for &d in w.tagged(Tag::Dummy) {
            if let Some(Kind::Dummy(du)) = w.kind(d)
                && !du.dead
            {
                v.push(w.core(d).pos + Vec2::new(0.0, 11.0));
            }
        }
        v
    }

    #[derive(Clone, Debug, Default)]
    pub struct CoopBot {
        stuck: u32,
        last_x: f32,
        /// Detour when blocked: ticks left and direction.
        detour: u32,
        detour_dir: i8,
        /// Exploring with nothing to do: direction and clock.
        wander: i8,
        explore_t: u32,
        /// The arrow it walks to and for how long; arrows it gave up on.
        chase: Option<(EntityId, u32)>,
        given_up: Vec<EntityId>,
        /// Jump button: ticks still held, ticks before the next press.
        hold: u32,
        cool: u32,
        /// Ticks without a shot (long waits: go to the target).
        idle: u32,
    }

    impl CoopBot {
        /// Full jumps: a press is held 14 ticks, then released 3.
        fn jumper(&mut self, want: bool) -> u8 {
            if self.hold > 0 {
                self.hold -= 1;
                return PlayerInput::JUMP;
            }
            if self.cool > 0 {
                self.cool -= 1;
                return 0;
            }
            if want {
                self.hold = 13;
                self.cool = 3;
                return PlayerInput::JUMP;
            }
            0
        }

        pub fn think(&mut self, w: &World, c: &Content, index: u8, frame: u32) -> PlayerInput {
            let none = PlayerInput::default();
            let Some(pid) = w.player_by_index(index) else {
                return none;
            };
            let Some(p) = w.player(pid) else { return none };
            if p.dead {
                return none;
            }
            let ws = w.world_size(c);
            let me = w.core(pid).pos;
            let rel = |t: Vec2| Vec2::new(wrap_d(me.x, t.x, ws.w), wrap_d(me.y, t.y, ws.h));
            let phase = (frame + index as u32 * 5) % 10;
            if (me.x - self.last_x).abs() < 0.1 {
                self.stuck += 1;
            } else {
                self.stuck = 0;
            }
            self.last_x = me.x;
            let lvl = c.level(w.level);
            // Is the straight line `me -> me + d` free of solid tiles (wrapping)?
            let clear = |d: Vec2| {
                let n = (d.length() / 4.0).ceil().max(1.0) as i32;
                (1..n).all(|k| {
                    let t = k as f32 / n as f32;
                    let x = (me.x + d.x * t).rem_euclid(ws.w);
                    let y = (me.y - 6.0 + d.y * t).rem_euclid(ws.h);
                    !lvl.solid(x as i32 / 10, y as i32 / 10)
                })
            };
            // Per target: the wrapped vector, else the direct one when only it
            // has a clear line; flag = a clear line exists.
            let pick = |t: Vec2| {
                let a = rel(t);
                let b = t - me;
                if clear(a) {
                    (a, true)
                } else if clear(b) {
                    (b, true)
                } else {
                    (a, false)
                }
            };
            let mut seen: Vec<(Vec2, bool)> = targets(w).into_iter().map(pick).collect();
            seen.sort_by(|a, b| {
                let k = |v: &(Vec2, bool)| v.0.length() + if v.1 { 0.0 } else { 60.0 };
                k(a).total_cmp(&k(b))
            });
            let line = seen.first().is_some_and(|v| v.1);
            let nearest = seen.first().map(|v| v.0);
            // Too close: get away (jump over it).
            if let Some(d) = nearest
                && d.length() < 22.0
            {
                let away = -d.x.signum() as i8;
                let j = self.jumper(true);
                return PlayerInput::dpad(away, 0, j);
            }
            if p.arrows.is_empty() {
                self.given_up.retain(|&a| w.exists(a));
                let arrow = w
                    .tagged(Tag::Arrow)
                    .iter()
                    .filter(|a| !self.given_up.contains(a))
                    .filter_map(|&a| match w.kind(a) {
                        Some(Kind::Arrow(ar)) if ar.collectible() => Some((a, rel(w.core(a).pos))),
                        _ => None,
                    })
                    .min_by(|a, b| {
                        let cost = |v: Vec2| v.x.abs() + 2.5 * v.y.abs();
                        cost(a.1).total_cmp(&cost(b.1))
                    });
                if let Some((id, d)) = arrow {
                    let t = match self.chase {
                        Some((c, t)) if c == id => t + 1,
                        _ => 0,
                    };
                    self.chase = Some((id, t));
                    if t > 300 {
                        self.given_up.push(id);
                        self.chase = None;
                    }
                    return self.walk_to(d);
                }
                if let Some(g) = self.goodies(w, &rel) {
                    return self.walk_to(g);
                }
                self.given_up.clear();
                return self.explore(frame);
            }
            let Some(d) = nearest else {
                // Nothing to shoot: open chests, collect pickups.
                return match self.goodies(w, &rel) {
                    Some(g) => self.walk_to(g),
                    None => none,
                };
            };
            let (ax, ay) = (d.x.abs(), d.y.abs());
            let aim = if ay < 9.0 {
                Some((d.x.signum() as i8, 0))
            } else if ax < 9.0 {
                Some((0, d.y.signum() as i8))
            } else if (ax - ay).abs() < 0.22 * ax.max(ay) {
                Some((d.x.signum() as i8, d.y.signum() as i8))
            } else {
                None
            };
            if let Some((x, y)) = aim.filter(|_| line) {
                let b = if phase == 0 { PlayerInput::SHOOT } else { 0 };
                if b != 0 {
                    self.idle = 0;
                }
                return PlayerInput::dpad(x, y, b);
            }
            self.idle += 1;
            if self.idle > 150 || !line {
                // No line (for a while): go there (wakes idle enemies up).
                if self.idle > 450 {
                    self.idle = 0;
                }
                return self.walk_to(d);
            }
            // Line up on a row (small height difference) or a diagonal:
            // approach when too far sideways, back off when too close.
            let toward = d.x.signum();
            let dir = if ay < 30.0 {
                if ax > 110.0 {
                    toward
                } else if ax < 40.0 {
                    -toward
                } else {
                    0.0
                }
            } else if ax > ay + 6.0 {
                toward
            } else if ax + 6.0 < ay {
                -toward
            } else {
                0.0
            };
            let j = self.jumper(d.y < -20.0 || (self.stuck > 20 && dir != 0.0));
            let down = if d.y > 40.0 && dir == 0.0 { 1 } else { 0 };
            PlayerInput::dpad(dir as i8, down, j)
        }

        /// The nearest closed chest or pickup.
        fn goodies(&self, w: &World, rel: &dyn Fn(Vec2) -> Vec2) -> Option<Vec2> {
            w.tagged(Tag::TreasureChest)
                .iter()
                .map(|&t| rel(w.core(t).pos))
                .chain(w.entities().filter_map(|e| match w.kind(e) {
                    Some(Kind::Pickup(_)) => Some(rel(w.core(e).pos)),
                    _ => None,
                }))
                .min_by(|a, b| a.length().total_cmp(&b.length()))
        }

        /// Nothing to do: run back and forth with hops.
        fn explore(&mut self, frame: u32) -> PlayerInput {
            self.explore_t += 1;
            if self.wander == 0 || self.explore_t.is_multiple_of(200) || self.stuck > 40 {
                self.wander = if self.wander > 0 { -1 } else { 1 };
                self.stuck = 0;
            }
            let j = self.jumper(self.stuck > 8 || frame.is_multiple_of(70));
            PlayerInput::dpad(self.wander, 0, j)
        }

        fn walk_to(&mut self, d: Vec2) -> PlayerInput {
            if self.detour > 0 {
                self.detour -= 1;
                let j = self.jumper(true);
                return PlayerInput::dpad(self.detour_dir, 0, j);
            }
            if self.stuck > 45 {
                // Blocked: the other way for a while (alternating).
                self.detour = 50;
                self.detour_dir = if self.detour_dir == 0 {
                    -(d.x.signum() as i8)
                } else {
                    -self.detour_dir
                };
                self.stuck = 0;
            }
            let j = self.jumper(d.y < -12.0 || self.stuck > 12);
            let down = if d.y > 20.0 && d.x.abs() < 12.0 { 1 } else { 0 };
            let dx = if d.x.abs() < 2.0 {
                0
            } else {
                d.x.signum() as i8
            };
            PlayerInput::dpad(dx, down, j)
        }
    }
}

/// The game's Dark World driver: walk into the open exit portal, else the
/// co-op bot (`coop_ui::darkworld::exit_walk` + `CoopBot`).
fn coopref_input(
    b: &mut coopref::CoopBot,
    w: &World,
    c: &Content,
    index: u8,
    frame: u32,
) -> PlayerInput {
    use tfp_sim::darkworld::Part;
    use tfp_sim::entity::Kind;
    if let Some(d) = w.dark_world.as_deref()
        && let Some(portal) = d.exit_portal.filter(|&e| w.exists(e))
        && matches!(w.kind(portal), Some(Kind::Dw(Part::ExitPortal { t })) if *t >= 60.0)
        && let Some(pid) = w.player_by_index(index)
        && w.player(pid).is_some_and(|p| !p.dead)
    {
        let ws = w.world_size(c);
        let me = w.core(pid).pos;
        let at = w.core(portal).pos;
        let mut dx = (at.x - me.x) % ws.w;
        if dx > ws.w / 2.0 {
            dx -= ws.w;
        } else if dx < -ws.w / 2.0 {
            dx += ws.w;
        }
        let dy = at.y - me.y;
        let phase = (frame + index as u32 * 5) % 20;
        let jump = dy < -10.0 && phase < 12;
        let x = if dx.abs() < 3.0 { 0 } else { dx.signum() as i8 };
        return PlayerInput::dpad(x, 0, if jump { PlayerInput::JUMP } else { 0 });
    }
    // The scene confirms the title card itself when no human plays.
    if w.player_by_index(index).is_none() && frame.is_multiple_of(13) {
        return PlayerInput::dpad(0, 0, PlayerInput::JUMP);
    }
    b.think(w, c, index, frame)
}

enum CoopDriver {
    Ref(coopref::CoopBot),
    Shared(Box<tfp_sim::bot::Bot>),
}
impl CoopDriver {
    fn input(&mut self, w: &World, c: &Content, index: u8) -> PlayerInput {
        match self {
            CoopDriver::Ref(b) => coopref_input(b, w, c, index, w.frame),
            CoopDriver::Shared(b) => b.input(w, c, index),
        }
    }
}

/// Dark World levels reached and Quest waves survived: co-op bot vs shared.
#[test]
#[ignore]
fn coop_modes_vs_coop_bot() {
    use tfp_sim::darkworld::{self, DwPhase, DwSettings};
    let i = Installs::discover();
    let Some(inst) = i.steam.clone() else { return };
    let dw = tfp_sim::loader::dark_world_content(&inst).unwrap();
    let qc = tfp_sim::loader::quest_content(&inst).unwrap();
    for shared in [false, true] {
        let name = if shared { "SHARED" } else { "coop" };
        let (mut levels, mut overs) = (0, 0);
        for tower in 0..4 {
            for seed in 0..3 {
                let mut s = DwSettings::new(tower, darkworld::NORMAL, 2);
                s.seed = seed;
                let mut w = darkworld::new_dark_world(&dw, s);
                let mut bots: Vec<CoopDriver> = (0..2)
                    .map(|k| {
                        if shared {
                            CoopDriver::Shared(Box::new(tfp_sim::bot::Bot::new(seed * 10 + k)))
                        } else {
                            CoopDriver::Ref(coopref::CoopBot::default())
                        }
                    })
                    .collect();
                for _ in 0..60 * 60 * 8 {
                    let inp: Vec<PlayerInput> =
                        (0..2).map(|k| bots[k].input(&w, &dw, k as u8)).collect();
                    w.step(&dw, &inp);
                    let d = w.dark_world.as_ref().unwrap();
                    if (d.phase == DwPhase::GameOver && d.menu_ready)
                        || d.phase == DwPhase::Complete
                    {
                        break;
                    }
                }
                let d = w.dark_world.as_ref().unwrap();
                levels += d.round_index;
                overs += (d.phase == DwPhase::GameOver) as u32;
            }
        }
        let (mut waves, mut qovers) = (0, 0);
        for tower in 0..qc.quest_scripts.len().min(8) {
            let mut s = tfp_sim::quest::QuestSettings::new(tower, tower, 2);
            s.tower_id = tower as u8;
            s.seed = 5;
            let mut w = tfp_sim::quest::new_quest(&qc, s);
            let mut bots: Vec<CoopDriver> = (0..2)
                .map(|k| {
                    if shared {
                        CoopDriver::Shared(Box::new(tfp_sim::bot::Bot::new(70 + k)))
                    } else {
                        CoopDriver::Ref(coopref::CoopBot::default())
                    }
                })
                .collect();
            for _ in 0..60 * 60 * 8 {
                let inp: Vec<PlayerInput> =
                    (0..2).map(|k| bots[k].input(&w, &qc, k as u8)).collect();
                w.step(&qc, &inp);
                let q = w.quest.as_ref().unwrap();
                if matches!(
                    q.phase,
                    tfp_sim::quest::QuestPhase::GameOver
                        | tfp_sim::quest::QuestPhase::Complete
                        | tfp_sim::quest::QuestPhase::Finished
                ) {
                    break;
                }
            }
            let q = w.quest.as_ref().unwrap();
            waves += q.current_wave.max(0);
            qovers += (q.phase == tfp_sim::quest::QuestPhase::GameOver) as i32;
        }
        let (tc, settings) = tfp_sim::loader::trials_content(&inst).unwrap();
        let mut cleared = 0;
        for t in settings.iter() {
            let mut w = tfp_sim::trials::new_trial(&tc, *t);
            let mut bot = if shared {
                CoopDriver::Shared(Box::new(tfp_sim::bot::Bot::new(3)))
            } else {
                CoopDriver::Ref(coopref::CoopBot::default())
            };
            for _ in 0..60 * 40 {
                let inp = [bot.input(&w, &tc, 0)];
                w.step(&tc, &inp);
                let tr = w.trials.as_ref().unwrap();
                if tr.result.is_some() {
                    break;
                }
            }
            cleared += w
                .trials
                .as_ref()
                .unwrap()
                .result
                .is_some_and(|r| r.time > 0) as u32;
        }
        println!(
            "{name}: DW levels reached {levels} (12 runs), game overs {overs}; Quest waves reached {waves} (8 towers), game overs {qovers}; Trials cleared {cleared} of {}",
            settings.len()
        );
    }
}
