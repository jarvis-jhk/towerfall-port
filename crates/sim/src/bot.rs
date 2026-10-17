//! A shared input bot for tests, soaks and empty slots, in every mode.
//!
//! It reads the world like a player looks at the screen and only produces
//! [`PlayerInput`]; it never touches the simulation, so it is safe under
//! rollback (online bots read the predicted world). Deterministic: the
//! world and its own seeded generator are its only inputs.
//!
//! Behaviour, in priority order:
//! 1. dead / frozen / between rounds: press Jump now and then (confirms the
//!    tower card, results and menus that wait for Confirm);
//! 2. danger: a dangerous arrow on a collision course is dodged (a dodge
//!    also catches it); a lit bomb close by is fled with a jump; an enemy,
//!    enemy attack, hazard or boss shot about to touch is dodged through;
//!    in Quest / Dark World it backs off from enemies closer than 36 px;
//! 3. the Dark World exit portal once open: walk in;
//! 4. the miasma: once it exists, stay out of the outer quarters;
//! 5. no arrows: walk to the nearest collectible arrow, else a chest or a
//!    pickup, else stomp the nearest target;
//! 6. a target (enemy archer, enemy, Trials dummy, Dark World boss body /
//!    eye / shield orb; in Dark World targets below are preferred) on one
//!    of the eight aim lines with a clear shot, aimed where it will be
//!    when the arrow arrives: shoot;
//! 7. otherwise line up with the target (row or diagonal at a useful
//!    distance), or walk to it when there is no line for a while.
//!
//! Getting somewhere uses a reachability graph of standing cells per level
//! (`path.rs`: walk, fall, drop-through, jump and wrap edges, rebuilt when
//! the level changes) for the exit portal, a stomp target and a spot with
//! a clear line to a target out of sight for 200 ticks; arrows and
//! pickups too outside Dark World (measured: walking straight to them is
//! better there). A route that makes no progress for 90 ticks falls back
//! to walking straight for 120.
//!
//! # Using it (game crate, tests)
//!
//! ```ignore
//! // One bot per bot-driven slot, when the match / run starts:
//! let mut bot = tfp_sim::bot::Bot::new(seed); // any i32, e.g. slot index
//! // Every tick, before `World::step`, for that slot:
//! let input: PlayerInput = bot.input(&world, &content, slot as u8);
//! inputs[slot] = input;
//! world.step(&content, &inputs);
//! ```
//!
//! * `input` reads the world as it is before this tick's step (online: the
//!   machine's current, possibly predicted world); only the returned input
//!   travels. The bot is not game state: keep it outside `World`, never
//!   roll it back, one per local bot slot.
//! * It never presses Start. It confirms waiting screens itself (Jump while
//!   dead / frozen), and walks into the Dark World exit portal itself (no
//!   separate exit helper needed).
//! * `Bot` is `Clone` and about 300 bytes; it keeps a little memory (stuck
//!   detection, the arrow it chases, the shot in progress), so call
//!   `input` every tick for its slot and recreate it for a new match.

use crate::content::Content;
use crate::entity::{Allegiance, EntityId, Kind, Tag};
use crate::input::PlayerInput;
use crate::math::Vec2;
use crate::quiver::ArrowType;
use crate::rng::Rng;
use crate::world::World;

mod path;
use path::{Move, Nav};

/// What `route` heads for.
#[derive(Clone, Copy, Debug)]
enum Goal {
    /// Reach this point (relative to the bot).
    Point(Vec2),
    /// Find a standing spot with a clear aim line to this point (relative).
    ShootAt(Vec2),
}

/// Arrow speed used for the lead (normal arrows fly at 4 px/tick).
const ARROW_SPEED: f32 = 4.0;
/// The eight aim directions (dpad x, y).
const DIRS: [(i8, i8); 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

fn wrap_d(a: f32, b: f32, size: f32) -> f32 {
    let mut d = (b - a).rem_euclid(size);
    if d > size / 2.0 {
        d -= size;
    }
    d
}

/// A target: relative position (wrapped) and velocity.
#[derive(Clone, Copy, Debug)]
struct Target {
    d: Vec2,
    v: Vec2,
}

#[derive(Clone, Debug)]
pub struct Bot {
    rng: Rng,
    stuck: u32,
    last_x: f32,
    detour: u32,
    detour_dir: i8,
    wander: i8,
    explore_t: u32,
    chase: Option<(EntityId, u32)>,
    given_up: Vec<EntityId>,
    /// Jump button: ticks still held, ticks before the next press.
    hold: u32,
    cool: u32,
    /// Ticks without a shot.
    idle: u32,
    /// Ticks until the next shot is allowed.
    reload: u32,
    /// The aim of the shot in progress (held while Shoot is down).
    aim: Option<(i8, i8)>,
    aim_t: u32,
    /// The level's reachability graph (rebuilt when the level changes).
    nav: Option<Box<Nav>>,
    /// Ticks spent ducking for a drop-through.
    duck_t: u32,
    /// Routing progress: the cell routed from and for how many ticks;
    /// routing is paused (ticks left) when it makes no progress.
    route_from: Option<usize>,
    route_t: u32,
    route_off: u32,
    /// Ticks with an empty quiver.
    empty_t: u32,
}

impl Bot {
    pub fn new(seed: i32) -> Bot {
        Bot {
            rng: Rng::new(seed),
            stuck: 0,
            last_x: 0.0,
            detour: 0,
            detour_dir: 0,
            wander: 0,
            explore_t: 0,
            chase: None,
            given_up: vec![],
            hold: 0,
            cool: 0,
            idle: 0,
            reload: 0,
            aim: None,
            aim_t: 0,
            nav: None,
            duck_t: 0,
            route_from: None,
            route_t: 0,
            route_off: 0,
            empty_t: 0,
        }
    }

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

    /// The input of archer `index` for this tick.
    pub fn input(&mut self, w: &World, c: &Content, index: u8) -> PlayerInput {
        // Shots, cooldowns and timers advance every tick.
        self.reload = self.reload.saturating_sub(1);
        let confirm = |b: &mut Bot| {
            let j = if b.rng.chance(0.08) {
                PlayerInput::JUMP
            } else {
                0
            };
            PlayerInput::dpad(0, 0, j)
        };
        let Some(pid) = w.player_by_index(index) else {
            return confirm(self);
        };
        let Some(p) = w.player(pid) else {
            return confirm(self);
        };
        if p.dead || w.frozen {
            return confirm(self);
        }
        if p.state() == crate::player::PState::Frozen {
            return confirm(self);
        }
        // A shot in progress: keep aiming, release on the third tick.
        if let Some((x, y)) = self.aim {
            self.aim_t += 1;
            if self.aim_t < 3 {
                return PlayerInput::dpad(x, y, PlayerInput::SHOOT);
            }
            self.aim = None;
            self.reload = 3 + self.rng.next_max(6) as u32;
            return PlayerInput::dpad(x, y, 0);
        }
        if !p.arrows.is_empty() {
            self.empty_t = 0;
        }
        let ws = w.world_size(c);
        let me = w.core(pid).pos;
        let my_all = w.core(pid).allegiance;
        let rel = |t: Vec2| Vec2::new(wrap_d(me.x, t.x, ws.w), wrap_d(me.y, t.y, ws.h));
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
        // 2. Danger.
        for &a in w.tagged(Tag::Arrow) {
            let Some(ar) = w.arrow(a) else { continue };
            let r = rel(w.core(a).pos);
            if ar.player_index == index as i8 && ar.state != crate::arrow::ArrowState::Shooting {
                continue;
            }
            if matches!(ar.arrow_type, ArrowType::Bomb | ArrowType::SuperBomb)
                && ar.extra.fuse_on
                && r.length() < 50.0
            {
                let away = if r.x > 0.0 { -1 } else { 1 };
                let j = self.jumper(true);
                return PlayerInput::dpad(away, 0, j);
            }
            if !ar.dangerous() || ar.player_index == index as i8 {
                continue;
            }
            let v = ar.speed;
            let vv = v.dot(v);
            if vv < 0.01 || r.length() > 60.0 {
                continue;
            }
            let t = -r.dot(v) / vv;
            if (0.0..=10.0).contains(&t) && (r + v * t).length() < 10.0 && !p.dodge_cooldown {
                // Dodge across the arrow's path.
                let perp = if v.x.abs() > v.y.abs() {
                    (0, if r.y > 0.0 { -1 } else { 1 })
                } else {
                    (if r.x > 0.0 { -1 } else { 1 }, 0)
                };
                return PlayerInput::dpad(perp.0, perp.1, PlayerInput::DODGE);
            }
        }

        // Contact threats (enemies, their attacks, hazards, boss shots)
        // about to touch: dodge through them (a dodge cannot be hurt).
        if !p.dodge_cooldown {
            let my_v = p.speed;
            for &e in w.tagged(Tag::PlayerCollider) {
                let (threat, v) = match w.kind(e) {
                    Some(Kind::Enemy(en))
                        if en.alive() && en.ty != crate::enemies::EnemyType::PlayerGhost =>
                    {
                        (true, en.speed)
                    }
                    Some(Kind::EnemyAttack(_)) | Some(Kind::Hazard(_)) => (true, Vec2::ZERO),
                    // Falling orbs and icicles (shot down by the bot itself,
                    // often) and spike balls.
                    Some(Kind::Tower(crate::entities::Tower::Orb(o))) if o.falling => {
                        (true, Vec2::new(0.0, o.v.max(1.0)))
                    }
                    Some(Kind::Tower(crate::entities::Tower::Icicle(i)))
                        if i.state == crate::entities::hazards::IcicleState::Falling =>
                    {
                        (true, Vec2::new(0.0, i.v.max(1.0)))
                    }
                    Some(Kind::Tower(crate::entities::Tower::Spikeball(_))) => (true, Vec2::ZERO),
                    Some(Kind::Dw(part)) => (
                        matches!(
                            part,
                            crate::darkworld::Part::Bullet { .. }
                                | crate::darkworld::Part::Missile { .. }
                                | crate::darkworld::Part::Blade { .. }
                                | crate::darkworld::Part::AmaShot { .. }
                                | crate::darkworld::Part::CycShot { .. }
                                | crate::darkworld::Part::Spore { .. }
                        ),
                        Vec2::ZERO,
                    ),
                    _ => (false, Vec2::ZERO),
                };
                if !threat || !w.core(e).collidable {
                    continue;
                }
                let r = rel(w.core(e).pos) - Vec2::new(0.0, -6.0);
                let dv = v - my_v;
                // Closest approach over the next 10 ticks.
                let vv = dv.dot(dv);
                let t = if vv > 0.001 {
                    (-r.dot(dv) / vv).clamp(0.0, 10.0)
                } else {
                    0.0
                };
                if (r + dv * t).length() < 20.0 {
                    let away = if r.x > 0.0 { -1 } else { 1 };
                    return PlayerInput::dpad(away, 0, PlayerInput::DODGE);
                }
            }
        }
        // Keep clear of enemies (back off with a hop).
        if w.quest.is_some() || w.dark_world.is_some() {
            let near = w
                .tagged(Tag::Enemy)
                .iter()
                .filter(|&&e| {
                    matches!(w.kind(e), Some(Kind::Enemy(en)) if en.alive() && w.core(e).collidable)
                })
                .map(|&e| rel(w.core(e).pos))
                .filter(|r| r.length() < 36.0)
                .min_by(|a, b| a.length().total_cmp(&b.length()));
            if let Some(r) = near {
                let away = if r.x > 0.0 { -1 } else { 1 };
                let j = self.jumper(r.y > -8.0);
                return PlayerInput::dpad(away, 0, j);
            }
        }

        // The Dark World exit portal is open: go through.
        if let Some(d) = w.dark_world.as_ref()
            && let Some(portal) = d.exit_portal
            && w.exists(portal)
            && w.core(portal).collidable
        {
            let d = rel(w.core(portal).pos);
            return self
                .route(w, c, pid, Goal::Point(d))
                .unwrap_or_else(|| self.walk_to(d));
        }

        // 3. The miasma: keep to the middle half.
        if w.versus.as_ref().is_some_and(|m| m.round.miasma.is_some()) {
            let mid = ws.w / 2.0;
            let off = me.x - mid;
            if off.abs() > ws.w / 4.0 {
                let j = self.jumper(self.stuck > 8);
                return PlayerInput::dpad(if off > 0.0 { -1 } else { 1 }, 0, j);
            }
        }

        let dark = w.dark_world.is_some();
        // Targets.
        let mut targets: Vec<Target> = vec![];
        for id in w.players() {
            if id == pid {
                continue;
            }
            let Some(o) = w.player(id) else { continue };
            let oa = w.core(id).allegiance;
            let enemy =
                my_all == Allegiance::Neutral && oa == Allegiance::Neutral || p.is_enemy(oa);
            if o.dead || !enemy || w.quest.is_some() || w.dark_world.is_some() {
                continue;
            }
            targets.push(Target {
                d: rel(w.core(id).pos + Vec2::new(0.0, -4.0)),
                v: o.speed,
            });
        }
        for &e in w.tagged(Tag::Enemy) {
            match w.kind(e) {
                Some(Kind::Enemy(en))
                    if en.alive()
                        && w.core(e).collidable
                        && en.ty != crate::enemies::EnemyType::PlayerGhost =>
                {
                    targets.push(Target {
                        d: rel(w.core(e).pos + en.sight),
                        v: en.speed,
                    });
                }
                Some(Kind::Dw(crate::darkworld::Part::Body | crate::darkworld::Part::Eye(_)))
                    if w.core(e).collidable =>
                {
                    targets.push(Target {
                        d: rel(w.core(e).pos + Vec2::new(0.0, 4.0)),
                        v: Vec2::ZERO,
                    });
                }
                _ => {}
            }
        }
        for e in w.entities() {
            match w.kind(e) {
                Some(Kind::Dw(crate::darkworld::Part::ShieldOrb { .. })) => targets.push(Target {
                    d: rel(w.core(e).pos + Vec2::new(0.0, 4.0)),
                    v: Vec2::ZERO,
                }),
                Some(Kind::Dummy(du)) if !du.dead => targets.push(Target {
                    d: rel(w.core(e).pos + Vec2::new(0.0, 11.0)),
                    v: Vec2::ZERO,
                }),
                _ => {}
            }
        }
        // Prefer reachable lines, then distance.
        // Per target: the wrapped vector if it has a clear line, else the
        // direct (unwrapped) one if that has; with no line either way the
        // direct one when the level's side walls are closed at this height
        // (no going round through the wrap).
        let row = (me.y / 10.0).floor() as i32;
        let edge_closed = lvl.solid(0, row) || lvl.solid(lvl.cells_x as i32 - 1, row);
        let mut seen: Vec<(Target, bool)> = targets
            .into_iter()
            .map(|t| {
                let mut direct = t.d;
                let tx = me.x + t.d.x;
                if tx < 0.0 {
                    direct.x += ws.w;
                } else if tx >= ws.w {
                    direct.x -= ws.w;
                }
                if clear(t.d) {
                    (t, true)
                } else if direct != t.d && clear(direct) {
                    (Target { d: direct, ..t }, true)
                } else if edge_closed {
                    (Target { d: direct, ..t }, false)
                } else {
                    (t, false)
                }
            })
            .collect();
        seen.sort_by(|a, b| {
            // Dark World: targets below are safer to shoot, prefer them
            // (measured: more levels cleared; no gain in Quest).
            let below = |v: &(Target, bool)| {
                if dark && v.0.d.y > 8.0 { 30.0 } else { 0.0 }
            };
            let k = |v: &(Target, bool)| v.0.d.length() + if v.1 { 0.0 } else { 80.0 } - below(v);
            k(a).total_cmp(&k(b))
        });

        // 4. No arrows: collect.
        if p.arrows.is_empty() {
            self.given_up.retain(|&a| w.exists(a));
            // Without arrows for 300 ticks: stop chasing arrows for 300
            // (pickups, then stomping the target instead).
            self.empty_t += 1;
            if self.empty_t > 600 {
                self.empty_t = 0;
            }
            let chase_arrows = self.empty_t <= 300;
            let reach = self.reach_set(w, c, pid);
            let arrow = w
                .tagged(Tag::Arrow)
                .iter()
                .filter(|_| chase_arrows)
                .filter(|a| !self.given_up.contains(a))
                .filter(|&&a| match (reach.as_ref(), self.nav.as_deref()) {
                    (Some(r), Some(nav)) => r.reaches(nav, w.core(a).pos),
                    _ => true,
                })
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
                // (Dark World: walking straight to arrows measured better.)
                if !dark && let Some(i) = self.route(w, c, pid, Goal::Point(d)) {
                    return i;
                }
                return self.walk_to(d);
            }
            if let Some(g) = goodies(w, &rel) {
                if !dark && let Some(i) = self.route(w, c, pid, Goal::Point(g)) {
                    return i;
                }
                return self.walk_to(g);
            }
            // Nothing to pick up: stomp the nearest target.
            if let Some(&(t, _)) = seen.first() {
                let g = t.d + Vec2::new(0.0, -12.0);
                return self
                    .route(w, c, pid, Goal::Point(g))
                    .unwrap_or_else(|| self.walk_to(g));
            }
            self.given_up.clear();
            return self.explore(w.frame);
        }

        let Some(&(target, line)) = seen.first() else {
            return match goodies(w, &rel) {
                Some(g) => self
                    .route(w, c, pid, Goal::Point(g))
                    .unwrap_or_else(|| self.walk_to(g)),
                None => self.explore(w.frame),
            };
        };
        let d = target.d;

        // Too close: jump over / away.
        if d.length() < 18.0 {
            let away = -d.x.signum() as i8;
            let j = self.jumper(true);
            return PlayerInput::dpad(away, 0, j);
        }

        // 5. Shoot along the aim line closest to the predicted position.
        if self.reload == 0 {
            let t = (d.length() / ARROW_SPEED).min(25.0);
            let lead = d + target.v * t;
            let mut best: Option<((i8, i8), f32)> = None;
            for &(x, y) in &DIRS {
                let u = Vec2::new(x as f32, y as f32).safe_normalize();
                let along = lead.dot(u);
                if along <= 0.0 {
                    continue;
                }
                let miss = (lead - u * along).length();
                if best.is_none_or(|b| miss < b.1) {
                    best = Some(((x, y), miss));
                }
            }
            if let Some(((x, y), miss)) = best
                && miss < 9.0 + lead.length() * 0.05
                && clear(lead)
            {
                self.idle = 0;
                self.aim = Some((x, y));
                self.aim_t = 0;
                return PlayerInput::dpad(x, y, PlayerInput::SHOOT);
            }
        }

        // 6. Line up, or go there.
        self.idle += 1;
        if self.idle > 60 || !line {
            if self.idle > 300 {
                self.idle = 0;
            }
            // Long without a shot: look for a reachable spot with a line.
            if self.idle > 200
                && let Some(i) = self.route(w, c, pid, Goal::ShootAt(d))
            {
                return i;
            }
            return self.walk_to(d);
        }
        let (ax, ay) = (d.x.abs(), d.y.abs());
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
            // Steep: back off for a diagonal when the target is above; when
            // it is below, go toward it (straight above it, or off the
            // ledge down to its level), backing off there is often blocked.
            if d.y > 0.0 { toward } else { -toward }
        } else {
            0.0
        };
        let j = self.jumper(d.y < -20.0 || (self.stuck > 20 && dir != 0.0));
        let down = if d.y > 40.0 && dir == 0.0 { 1 } else { 0 };
        PlayerInput::dpad(dir as i8, down, j)
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

    /// What the bot can reach from where it stands (`None`: no graph cell
    /// under it, assume everything).
    fn reach_set(&mut self, w: &World, c: &Content, pid: EntityId) -> Option<Reach> {
        if self.nav.as_ref().is_none_or(|n| n.level != w.level) {
            self.nav = Some(Box::new(Nav::build(w.level, c.level(w.level))));
        }
        let nav = self.nav.as_deref()?;
        let ws = w.world_size(c);
        let core = w.core(pid);
        let start = nav.stand_at(core.pos.x.rem_euclid(ws.w), core.bottom())?;
        let (dist, _) = nav.bfs(start);
        Some(Reach { dist, w: ws.w })
    }

    /// Head for `goal` along the level's standing-cell graph: the first
    /// edge of the shortest path to the best reachable cell, as inputs.
    /// `None` when there is nothing better than walking straight (already
    /// on the best cell, no graph cell under the bot, nothing reachable).
    fn route(&mut self, w: &World, c: &Content, pid: EntityId, goal: Goal) -> Option<PlayerInput> {
        if self.nav.as_ref().is_none_or(|n| n.level != w.level) {
            self.nav = Some(Box::new(Nav::build(w.level, c.level(w.level))));
        }
        if self.route_off > 0 {
            self.route_off -= 1;
            return None;
        }
        let nav = self.nav.as_deref()?;
        let ws = w.world_size(c);
        let core = w.core(pid);
        let me = core.pos;
        let start = nav.stand_at(me.x.rem_euclid(ws.w), core.bottom())?;
        // No progress for 90 ticks on the same cell: walk straight for a
        // while instead (the graph is only an approximation).
        if self.route_from == Some(start) {
            self.route_t += 1;
            if self.route_t > 90 {
                self.route_t = 0;
                self.route_off = 120;
                self.duck_t = 0;
                return None;
            }
        } else {
            self.route_from = Some(start);
            self.route_t = 0;
        }
        let (dist, parent) = nav.bfs(start);
        let lvl = c.level(w.level);
        // Clear straight line between two points (wrapping), by tiles.
        let line = |a: Vec2, d: Vec2| {
            let n = (d.length() / 4.0).ceil().max(1.0) as i32;
            (1..n).all(|k| {
                let t = k as f32 / n as f32;
                let x = (a.x + d.x * t).rem_euclid(ws.w);
                let y = (a.y + d.y * t).rem_euclid(ws.h);
                !lvl.solid(x as i32 / 10, y as i32 / 10)
            })
        };
        let rel =
            |p: (f32, f32)| Vec2::new(wrap_d(me.x, p.0, ws.w), wrap_d(core.bottom(), p.1, ws.h));
        let mut best: Option<(usize, f32)> = None;
        for (i, pos) in nav.stands() {
            let dd = dist[i];
            if dd == u16::MAX {
                continue;
            }
            // `r`: the cell's feet position relative to the bot's feet.
            let r = rel(pos);
            let score = match goal {
                Goal::Point(g) => {
                    // `g` is relative to the bot's centre (feet - 8).
                    let off = g + Vec2::new(0.0, 8.0) - r;
                    if off.length() > 40.0 {
                        continue;
                    }
                    off.length() * 4.0 + dd as f32
                }
                Goal::ShootAt(t) => {
                    let eye = r + Vec2::new(0.0, -14.0);
                    let to = t + Vec2::new(0.0, 8.0) - eye;
                    let (ax, ay) = (to.x.abs(), to.y.abs());
                    let aligned = ay < 8.0 || ax < 8.0 || (ax - ay).abs() < 0.2 * ax.max(ay);
                    if !aligned || to.length() > 160.0 || to.length() < 24.0 {
                        continue;
                    }
                    let abs = Vec2::new(me.x + eye.x, core.bottom() + eye.y);
                    if !line(abs, to) {
                        continue;
                    }
                    dd as f32 * 10.0 + to.length() * 0.1
                }
            };
            if best.is_none_or(|b| score < b.1) {
                best = Some((i, score));
            }
        }
        let (goal_cell, _) = best?;
        let (next, mv) = nav.first_step(start, goal_cell, &parent)?;
        let (nx, _) = nav.cell(next);
        let dx = wrap_d(me.x, nx as f32 * 10.0 + 5.0, ws.w);
        let x = if dx.abs() < 2.0 { 0 } else { dx.signum() as i8 };
        Some(match mv {
            Move::Walk | Move::Fall => {
                self.duck_t = 0;
                PlayerInput::dpad(x, 0, 0)
            }
            Move::Jump => {
                self.duck_t = 0;
                let on_ground = w.player(pid).is_some_and(|p| p.on_ground);
                let j = self.jumper(on_ground);
                PlayerInput::dpad(x, 0, j)
            }
            Move::Drop => {
                self.duck_t += 1;
                let j = if self.duck_t >= 3 {
                    self.duck_t = 0;
                    PlayerInput::JUMP
                } else {
                    0
                };
                PlayerInput::dpad(0, 1, j)
            }
        })
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

/// Reachable standing cells from the bot's cell.
struct Reach {
    dist: Vec<u16>,
    w: f32,
}

impl Reach {
    /// Something lying at `p` (an arrow on the ground or in a wall):
    /// a reachable standing cell within one cell of it.
    fn reaches(&self, nav: &Nav, p: Vec2) -> bool {
        let x = p.x.rem_euclid(self.w);
        [-10.0, 0.0, 10.0].iter().any(|&dx| {
            [-10.0, 0.0, 10.0].iter().any(|&dy| {
                nav.stand_at((x + dx).rem_euclid(self.w), p.y + dy + 1.0)
                    .is_some_and(|i| {
                        let (_, cy) = nav.cell(i);
                        // The landing cell must be close below, not far down.
                        (cy as f32 * 10.0 - (p.y + dy)).abs() < 25.0 && self.dist[i] != u16::MAX
                    })
            })
        })
    }
}

/// The nearest closed chest or pickup (relative).
fn goodies(w: &World, rel: &dyn Fn(Vec2) -> Vec2) -> Option<Vec2> {
    w.tagged(Tag::TreasureChest)
        .iter()
        .filter(|&&t| {
            matches!(w.kind(t), Some(Kind::Chest(ch)) if ch.state == crate::treasure::ChestState::Closed)
        })
        .map(|&t| rel(w.core(t).pos))
        .chain(w.entities().filter_map(|e| match w.kind(e) {
            Some(Kind::Pickup(_)) => Some(rel(w.core(e).pos)),
            _ => None,
        }))
        .min_by(|a, b| a.length().total_cmp(&b.length()))
}
