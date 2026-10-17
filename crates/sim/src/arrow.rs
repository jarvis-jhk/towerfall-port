//! The base arrow (arrows.md §2–7). The per-type behaviour (§9) hooks in
//! through `crate::arrows` where the original's subclasses override it.

use crate::actor::{self, wrap_dist_sq};
use crate::collide::{Collider, Hitbox, Placed};
use crate::components::Wiggler;
use crate::content::Content;
use crate::entity::{Allegiance, Core, EntityId, FlashFinish, Kind, Tag, depth};
use crate::math::{
    PI, Vec2, abs_angle_diff, angle_diff, angle_to_vector, approach, clamp, max, min, pow,
    shorter_angle, sign,
};
use crate::quiver::ArrowType;
use crate::world::World;
use crate::{impl_hash, impl_hash_enum};

pub const HITBOX: Hitbox = Hitbox::wrapping(2.0, 2.0, -1.0, -1.0);
pub const OTHER_ARROW_HITBOX: Hitbox = Hitbox::wrapping(4.0, 4.0, -2.0, -2.0);
pub const START_SPEED: f32 = 4.0;
pub const SEEK_RADIUS_SQ: f32 = 4900.0;
pub const SEEK_MAX_ANGLE: f32 = 2.0 * PI / 9.0;
pub const SEEK_TURN_RATE: f32 = PI / 90.0;
pub const ARROW_LIMIT: usize = 12;

/// The level's arrow cap: 12 (original) for up to 4 archers, ×1.5 for 5–8,
/// ×2 for 9–12 (project decision, docs/STATUS.md).
pub fn arrow_limit(w: &World) -> usize {
    let n = if let Some(m) = &w.versus {
        m.settings.player_amount()
    } else if let Some(q) = &w.quest {
        q.settings.slots.iter().filter(|s| s.is_some()).count()
    } else if let Some(d) = &w.dark_world {
        d.n_players()
    } else {
        w.setup.players.iter().filter(|p| p.is_some()).count()
    };
    match n {
        0..=4 => ARROW_LIMIT,
        5..=8 => ARROW_LIMIT * 3 / 2,
        _ => ARROW_LIMIT * 2,
    }
}
pub const MAX_FALL: f32 = 5.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArrowState {
    Shooting = 0,
    Drilling = 1,
    Gravity = 2,
    Falling = 3,
    Stuck = 4,
    LayingOnGround = 5,
    Buried = 6,
}
impl_hash_enum!(ArrowState);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrow {
    pub arrow_type: ArrowType,
    pub state: ArrowState,
    pub speed: Vec2,
    /// Heading (graphics rotate with it).
    pub direction: f32,
    pub owner: Option<EntityId>,
    pub cannot_hit: Option<EntityId>,
    pub player_index: i8,
    pub team_color: Allegiance,
    pub from_hyper: bool,
    pub shooting: f32,
    pub cannot_pickup: f32,
    pub cannot_catch: f32,
    pub cannot_hit_enemies: f32,
    pub travel_frames: f32,
    pub stuck_to: Option<EntityId>,
    pub stuck_dir: Vec2,
    pub collided_h: bool,
    pub squished: bool,
    pub frozen: bool,
    pub frozen_player: Option<EntityId>,
    /// The corpse this arrow is buried in (`ArrowCushion`).
    pub buried_in: Option<EntityId>,
    pub popped: bool,
    /// Cosmetic stick wiggle.
    pub wiggle: Wiggler,
    /// Type-specific state (fuse, bounces, drill, bolt, feather, trigger).
    pub extra: crate::arrows::ArrowExtra,
}

impl_hash!(Arrow {
    arrow_type,
    state,
    speed,
    direction,
    owner,
    cannot_hit,
    player_index,
    team_color,
    from_hyper,
    shooting,
    cannot_pickup,
    cannot_catch,
    cannot_hit_enemies,
    travel_frames,
    stuck_to,
    stuck_dir,
    collided_h,
    squished,
    frozen,
    frozen_player,
    buried_in,
    popped,
    extra
});

impl Arrow {
    /// `Dangerous`.
    pub fn dangerous(&self) -> bool {
        match self.state {
            ArrowState::Shooting | ArrowState::Gravity => true,
            ArrowState::Falling => {
                self.speed.y >= 5.5 && abs_angle_diff(self.direction, PI / 2.0) <= PI / 45.0
            }
            _ => false,
        }
    }
    /// `IsCollectible`.
    pub fn collectible(&self) -> bool {
        let base = self.cannot_pickup <= 0.0 && self.state >= ArrowState::Stuck;
        base && match self.arrow_type {
            ArrowType::Bomb | ArrowType::SuperBomb => !self.extra.fuse_on,
            ArrowType::Bramble => !self.extra.power_used,
            ArrowType::Trigger => self.extra.detonator.is_none(),
            _ => true,
        }
    }
    /// `CanCatch(catcher)` (the removal test is done by the caller).
    pub fn can_catch(&self, catcher: EntityId) -> bool {
        !(self.cannot_catch > 0.0 && self.owner == Some(catcher))
            && !(self.arrow_type == ArrowType::Bramble && self.extra.power_used)
    }
}

/// Who fires an arrow.
#[derive(Clone, Copy, Debug)]
pub struct Shooter {
    pub id: Option<EntityId>,
    pub player_index: i8,
    pub allegiance: Allegiance,
    pub team_color: Allegiance,
    pub hyper: bool,
}

/// `Arrow.Create` + `Init` (arrows.md §3.2); the arrow joins the scene next tick.
pub fn create(
    w: &mut World,
    t: ArrowType,
    shooter: Shooter,
    pos: Vec2,
    direction: f32,
) -> EntityId {
    let mut core = Core::new(
        pos,
        Collider::Box(HITBOX),
        depth::ARROW,
        &[Tag::Arrow, Tag::LightSource, Tag::Actor],
    );
    core.screen_wrap = true;
    core.allegiance = shooter.allegiance;
    let mut speed = angle_to_vector(direction, crate::arrows::start_speed(t));
    if speed.y.abs() <= 0.01 {
        speed.y = 0.0;
    }
    let mut a = Arrow {
        arrow_type: t,
        state: ArrowState::Shooting,
        speed,
        direction,
        owner: shooter.id,
        cannot_hit: shooter.id,
        player_index: shooter.player_index,
        team_color: shooter.team_color,
        from_hyper: shooter.hyper,
        shooting: 25.0,
        cannot_pickup: 0.0,
        cannot_catch: 0.0,
        cannot_hit_enemies: 0.0,
        travel_frames: 0.0,
        stuck_to: None,
        stuck_dir: Vec2::ZERO,
        collided_h: false,
        squished: false,
        frozen: false,
        frozen_player: None,
        buried_in: None,
        popped: false,
        wiggle: Wiggler::new(90.0, 6.0, None),
        extra: crate::arrows::ArrowExtra::default(),
    };
    crate::arrows::init(&mut a);
    w.add(core, Kind::Arrow(a))
}

/// `Arrow.Update` (arrows.md §3.3) followed by the type's own work.
/// Moon glass is passable only during the base update (§13.1): fuses and
/// decays run with it solid again.
pub fn update(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    crate::entities::moonglass_for_arrows(w, true);
    base_update(w, c, id, a);
    crate::entities::moonglass_for_arrows(w, false);
    if w.exists(id) && !w.core(id).marked_for_removal {
        crate::arrows::after_update(w, c, id, a);
    }
}

fn base_update(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    let tm = w.time.time_mult;
    // 1. un-squish
    if a.squished && !w.check_at(c, id, w.core(id).pos, Tag::Solid) {
        a.squished = false;
        w.core_mut(id).collidable = true;
    }
    // 2.
    if a.buried_in.is_some() && a.state != ArrowState::Buried {
        a.state = ArrowState::Buried;
    }
    // 3. jump-thru pop (cosmetic sound once per passage)
    if a.speed.y < 0.0 && w.any(c, &w.placed(id), Tag::JumpThru, id) {
        if !a.popped {
            w.sound("env_gplatArrowPierce", w.core(id).pos.x);
        }
        a.popped = true;
    } else {
        a.popped = false;
    }
    // 5.
    if a.cannot_catch > 0.0 {
        a.cannot_catch -= tm;
    }
    // 6. fire
    crate::arrows::fire_step(w, c, id, a);
    // 8. components (wiggle, the feather's sine, the bramble growth
    // coroutine), wrap, flash
    a.wiggle.update(tm);
    if a.arrow_type == ArrowType::Feather {
        a.extra.sine = (a.extra.sine + 2.0 * PI / 30.0 * tm) % (PI * 8.0);
    }
    if a.arrow_type == ArrowType::Bramble
        && a.extra.power_used
        && !a.extra.brambles_done
        && crate::arrows::brambles::tick_growth(w, c, id)
    {
        a.extra.brambles_done = true;
    }
    let before = w.core(id).pos;
    actor::enforce_screen_wrap(w, c, id);
    if w.core(id).pos != before && a.player_index >= 0 {
        crate::versus::stat(w, a.player_index as u8, |s| s.arrow_screen_wraps += 1);
    }
    if let Some(FlashFinish::RemoveSelf) = w.core_mut(id).flash.update(tm) {
        w.remove(id);
    }
    // 9.
    arrow_update(w, c, id, a);
    // 10. (lasers and brambles run their own unconditional decay)
    if w.core(id).flash.flashing
        && !matches!(a.arrow_type, ArrowType::Laser | ArrowType::Bramble)
        && w.tagged(Tag::Arrow).len() <= arrow_limit(w)
    {
        w.core_mut(id).flash.stop();
    }
    // 11.
    if a.cannot_pickup > 0.0 {
        a.cannot_pickup -= tm;
    }
    if a.cannot_hit_enemies > 0.0 {
        a.cannot_hit_enemies -= tm;
    }
    // 12.
    if a.dangerous() {
        let hit = if a.arrow_type == ArrowType::Toy {
            crate::arrows::toy_target_collisions(w, c, id, a)
        } else {
            check_for_target_collisions(w, c, id, a)
        };
        if hit || !w.exists(id) || w.core(id).marked_for_removal {
            return;
        }
        if let Some(ch) = a.cannot_hit {
            let ws = w.world_size(c);
            let far = match w.try_core(ch) {
                Some(cc) => wrap_dist_sq(w.core(id).pos, cc.pos, ws.w, ws.h) >= 400.0,
                None => true,
            };
            if far {
                a.cannot_hit = None;
            }
        }
        arrow_clash(w, c, id, a);
    }
}

fn arrow_update(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    let tm = w.time.time_mult;
    if a.state < ArrowState::Stuck {
        a.travel_frames += tm;
    }
    match a.state {
        ArrowState::Shooting => {
            shoot_update(w, c, id, a);
            if let Some(p) = actor::move_h(w, c, id, a.speed.x * tm) {
                on_collide_h(w, c, id, a, p);
            }
            if a.state != ArrowState::Stuck
                && let Some(p) = actor::move_v(w, c, id, a.speed.y * tm)
            {
                on_collide_v(w, c, id, a, p);
            }
        }
        ArrowState::Gravity => {
            a.speed.x = approach(a.speed.x, 0.0, 0.03 * tm);
            a.speed.y = min(a.speed.y + 0.2 * tm, MAX_FALL);
            if !a.speed.is_zero() {
                a.direction = a.speed.angle();
            }
            if let Some(p) = actor::move_h(w, c, id, a.speed.x * tm) {
                on_collide_h(w, c, id, a, p);
            }
            if let Some(p) = actor::move_v(w, c, id, a.speed.y * tm) {
                on_collide_v(w, c, id, a, p);
            }
        }
        ArrowState::Falling => {
            let acc = if a.speed.y.abs() <= 1.0 { 0.06 } else { 0.2 };
            a.speed.y = min(a.speed.y + acc * tm, MAX_FALL);
            a.speed.x *= pow(0.98, tm);
            if let Some(p) = actor::move_h(w, c, id, a.speed.x * tm) {
                on_collide_h(w, c, id, a, p);
            }
            if let Some(p) = actor::move_v(w, c, id, a.speed.y * tm) {
                on_collide_v(w, c, id, a, p);
            }
            if a.state == ArrowState::Falling {
                if a.speed.y < 0.0 {
                    let s = if a.speed.x == 0.0 {
                        1.0
                    } else {
                        sign(a.speed.x)
                    };
                    a.direction += max(1.0, (1.0 - a.speed.y).abs()) * (8.0 * PI / 180.0) * s * tm;
                } else if a.speed.y > 0.0 {
                    let target = a.speed.angle();
                    a.direction = crate::math::angle_approach(a.direction, target, PI / 45.0 * tm);
                }
            }
        }
        ArrowState::Stuck => {
            // The platform or its `StuckPair` (the other half of a loop).
            let still = match a.stuck_to {
                Some(s) if w.exists(s) => {
                    let p = Placed {
                        pos: w.core(id).pos + a.stuck_dir,
                        collider: Collider::Box(HITBOX),
                    };
                    w.check(c, &p, s)
                        || crate::entities::platforms::stuck_pair(w, s)
                            .is_some_and(|q| w.exists(q) && w.check(c, &p, q))
                }
                _ => false,
            };
            if !still {
                enter_fall_mode(w, id, a, false, false, true);
            }
        }
        ArrowState::LayingOnGround => {
            if actor::check_below(w, c, id) {
                let target = shorter_angle(a.direction, 0.0, PI);
                a.direction = crate::math::angle_approach(a.direction, target, PI / 30.0 * tm);
            } else {
                enter_fall_mode(w, id, a, false, false, true);
            }
        }
        ArrowState::Drilling => crate::arrows::drill::drilling_update(w, c, id, a),
        ArrowState::Buried => {}
    }
}

fn shoot_update(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    match a.arrow_type {
        ArrowType::Bolt => return crate::arrows::bolt::shoot_update(w, c, id, a),
        ArrowType::Laser => {
            seek(w, c, id, a);
            return;
        }
        ArrowType::Feather => {
            if seek(w, c, id, a) {
                let s = a.speed;
                a.extra.perpendicular = Vec2::new(-s.y, s.x).safe_normalize();
            }
            let tm = w.time.time_mult;
            let wob = a.extra.perpendicular * crate::math::sin(a.extra.sine) * 0.4 * tm;
            actor::move_h(w, c, id, wob.x);
            actor::move_v(w, c, id, wob.y);
            return;
        }
        _ => {}
    }
    let tm = w.time.time_mult;
    if !w.variants_for(a.player_index as i32).anti_gravity_arrows {
        a.shooting -= tm;
        if a.shooting <= 0.0 {
            a.state = ArrowState::Gravity;
        }
    }
    seek(w, c, id, a);
}

/// `UpdateSeeking()`: turn toward the best target (§3.5). Returns whether
/// it turned.
fn seek(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) -> bool {
    let Some(hit) = find_seek_target(w, c, id, a, &SeekQuery::default_for(a)) else {
        return false;
    };
    crate::enemies::on_arrow_seeking(w, c, hit.target, id, a);
    let v = w.variants_for(a.player_index as i32);
    if v.no_seeking_arrows {
        return false;
    }
    let mut r = crate::arrows::seek_turn_rate(a.arrow_type) * w.time.time_mult;
    if v.super_seeking_arrows {
        r *= 2.0;
    }
    a.direction += clamp(angle_diff(a.direction, hit.angle), -r, r);
    a.speed = angle_to_vector(a.direction, crate::arrows::start_speed(a.arrow_type));
    true
}

/// Which walls block a seek line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sight {
    /// Solids (and jump-thrus when the target is below).
    Normal,
    /// Only granite (a drill before / while drilling).
    GraniteOnly,
}

/// `FindSeekTarget` parameters (radius, window, minimum priority, sight).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeekQuery {
    pub radius_sq: f32,
    pub min_dist_sq: f32,
    pub max_angle: f32,
    pub min_priority: i32,
    pub sight: Sight,
    /// The bolt's turning test: a 6x6 box swept along the line.
    pub swept: bool,
}

impl SeekQuery {
    /// The default window for this arrow (70 px, 40 degrees).
    pub fn default_for(a: &Arrow) -> SeekQuery {
        SeekQuery {
            radius_sq: SEEK_RADIUS_SQ,
            min_dist_sq: 0.0,
            max_angle: SEEK_MAX_ANGLE,
            min_priority: i32::MIN,
            sight: if a.arrow_type == ArrowType::Drill {
                crate::arrows::drill::sight(a)
            } else {
                Sight::Normal
            },
            swept: false,
        }
    }
}

/// A seek target: the entity, the wrapped angle and its seek point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeekHit {
    pub target: EntityId,
    pub angle: f32,
    pub point: Vec2,
}

/// `FindSeekTarget` (arrows.md §3.5).
pub fn find_seek_target(
    w: &World,
    c: &Content,
    id: EntityId,
    a: &Arrow,
    q: &SeekQuery,
) -> Option<SeekHit> {
    let me = w.core(id);
    let pos = me.pos;
    let ws = w.world_size(c);
    let mut best: Option<(i32, f32, SeekHit)> = None;
    for &t in w.tagged(Tag::Target) {
        let tc = w.core(t);
        if !tc.seek || Some(t) == a.owner || tc.seek_priority < q.min_priority {
            continue;
        }
        if !(tc.allegiance != me.allegiance || me.allegiance == Allegiance::Neutral) {
            continue;
        }
        let tp = tc.pos + tc.seek_offset;
        let d = wrap_dist_sq(pos, tp, ws.w, ws.h);
        if d >= q.radius_sq || d < q.min_dist_sq {
            continue;
        }
        let blocked = match q.sight {
            Sight::GraniteOnly => wrap_line_hit(w, c, pos, tp, Tag::Granite),
            Sight::Normal => {
                let below = wrap_diff_y(pos.y, tp.y, ws.h) > 0.0;
                wrap_line_hit(w, c, pos, tp, Tag::Solid)
                    || (below && wrap_line_hit(w, c, pos, tp, Tag::JumpThru))
            }
        };
        if blocked {
            continue;
        }
        let ang = wrap_angle(pos, tp, ws.w, ws.h);
        let diff = abs_angle_diff(a.direction, ang);
        if diff > q.max_angle {
            continue;
        }
        if q.swept && swept_box_hits(w, c, pos, tp, d) {
            continue;
        }
        let better = match best {
            None => true,
            Some((bp, bd, _)) => tc.seek_priority > bp || (tc.seek_priority == bp && diff < bd),
        };
        if better {
            best = Some((
                tc.seek_priority,
                diff,
                SeekHit {
                    target: t,
                    angle: ang,
                    point: tp,
                },
            ));
        }
    }
    best.map(|(_, _, h)| h)
}

/// The bolt's swept 6x6 box test along the (wrapped) line to `to`.
fn swept_box_hits(w: &World, c: &Content, from: Vec2, to: Vec2, dist_sq: f32) -> bool {
    let ws = w.world_size(c);
    let v = Vec2::new(
        actor::wrap_diff_x(from.x, to.x, ws.w),
        actor::wrap_diff_x(from.y, to.y, ws.h),
    );
    let dir = v.safe_normalize();
    let steps = crate::math::floor(dist_sq.sqrt() / 3.0) as i32 - 1;
    let lvl = c.level(w.level);
    for i in 2..steps {
        let p = Placed {
            pos: from + dir * (3.0 * i as f32),
            collider: Collider::Box(Hitbox::wrapping(6.0, 6.0, -3.0, -3.0)),
        };
        let hit = w.tagged(Tag::Solid).iter().any(|&e| {
            let t = w.core(e);
            t.collidable
                && crate::collide::collide(
                    &Placed {
                        pos: t.pos,
                        collider: t.collider,
                    },
                    &p,
                    lvl,
                    ws,
                )
        });
        if hit {
            return true;
        }
    }
    false
}

fn wrap_diff_y(from: f32, to: f32, h: f32) -> f32 {
    actor::wrap_diff_x(from, to, h)
}

/// `WrapMath.WrapLineHit` over one tag.
pub fn wrap_line_hit(w: &World, c: &Content, a: Vec2, b: Vec2, tag: Tag) -> bool {
    let ws = w.world_size(c);
    let (ww, hh) = (ws.w, ws.h);
    let horiz_sq = horiz_dist_sq(a, b, ww);
    let vert_sq = horiz_dist_sq(Vec2::new(a.y, a.x), Vec2::new(b.y, b.x), hh);
    if horiz_sq < vert_sq {
        if a.x + (ww - b.x) < (b.x - a.x).abs() {
            w.line_check(c, a, b - Vec2::new(ww, 0.0), tag)
                || w.line_check(c, a + Vec2::new(ww, 0.0), b, tag)
        } else if b.x + (ww - a.x) < (b.x - a.x).abs() {
            w.line_check(c, a - Vec2::new(ww, 0.0), b, tag)
                || w.line_check(c, a, b + Vec2::new(ww, 0.0), tag)
        } else {
            w.line_check(c, a, b, tag)
        }
    } else if a.y + (hh - b.y) < (b.y - a.y).abs() {
        w.line_check(c, a, b - Vec2::new(0.0, hh), tag)
            || w.line_check(c, a + Vec2::new(0.0, hh), b, tag)
    } else if b.y + (hh - a.y) < (b.y - a.y).abs() {
        w.line_check(c, a - Vec2::new(0.0, hh), b, tag)
            || w.line_check(c, a, b + Vec2::new(0.0, hh), tag)
    } else {
        w.line_check(c, a, b, tag)
    }
}

/// `WrapHorizDistanceSquared` (x axis of the given points).
fn horiz_dist_sq(a: Vec2, b: Vec2, ww: f32) -> f32 {
    if a.x + (ww - b.x) < (b.x - a.x).abs() {
        a.dist_sq(b - Vec2::new(ww, 0.0))
    } else if b.x + (ww - a.x) < (b.x - a.x).abs() {
        (a - Vec2::new(ww, 0.0)).dist_sq(b)
    } else {
        a.dist_sq(b)
    }
}

/// `WrapMath.WrapAngle(a, b)`.
pub fn wrap_angle(a: Vec2, b: Vec2, ww: f32, hh: f32) -> f32 {
    let horiz = horiz_dist_sq(a, b, ww);
    let vert = horiz_dist_sq(Vec2::new(a.y, a.x), Vec2::new(b.y, b.x), hh);
    let ang = crate::math::angle_between;
    if horiz < vert {
        if a.x + (ww - b.x) < (b.x - a.x).abs() {
            ang(a, b - Vec2::new(ww, 0.0))
        } else if b.x + (ww - a.x) < (b.x - a.x).abs() {
            ang(a - Vec2::new(ww, 0.0), b)
        } else {
            ang(a, b)
        }
    } else if a.y + (hh - b.y) < (b.y - a.y).abs() {
        ang(a, b - Vec2::new(0.0, hh))
    } else if b.y + (hh - a.y) < (b.y - a.y).abs() {
        ang(a - Vec2::new(0.0, hh), b)
    } else {
        ang(a, b)
    }
}

fn on_collide_h(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow, platform: EntityId) {
    if type_collide(w, id, a, platform, true) {
        return;
    }
    match a.state {
        ArrowState::Shooting | ArrowState::Gravity => {
            if a.speed.y < 0.0 && (a.speed.x / a.speed.y).abs() < 0.1 {
                a.speed.x = 0.0;
                a.direction = a.speed.angle();
            } else {
                a.stuck_dir = if a.speed.x.abs() < 1.0 {
                    Vec2::new(sign(a.speed.x), 0.0)
                } else {
                    Vec2::new(a.speed.x, 0.0)
                };
                a.collided_h = true;
                hit_wall(w, c, id, a, platform);
            }
        }
        ArrowState::Falling => a.speed.x = -a.speed.x,
        _ => {}
    }
}

/// Laser reflections and drill starts replace the base collide rules.
fn type_collide(
    w: &mut World,
    id: EntityId,
    a: &mut Arrow,
    platform: EntityId,
    horizontal: bool,
) -> bool {
    match a.arrow_type {
        ArrowType::Laser => crate::arrows::laser_bounce(w, id, a, horizontal),
        ArrowType::Drill => crate::arrows::drill::on_collide(w, id, a, platform),
        _ => false,
    }
}

fn on_collide_v(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow, platform: EntityId) {
    if type_collide(w, id, a, platform, false) {
        return;
    }
    if a.state == ArrowState::Stuck {
        return;
    }
    if a.dangerous() {
        if (a.speed.y < 0.0 || a.state == ArrowState::Shooting)
            && a.speed.x != 0.0
            && (a.speed.y / a.speed.x).abs() < 0.1
        {
            a.speed.y = 0.0;
            a.direction = a.speed.angle();
        } else {
            a.stuck_dir = if a.speed.y.abs() < 1.0 {
                Vec2::new(0.0, sign(a.speed.y))
            } else {
                Vec2::new(0.0, a.speed.y)
            };
            a.collided_h = false;
            hit_wall(w, c, id, a, platform);
        }
    } else if a.state == ArrowState::Falling && a.speed.y < 0.0 {
        a.speed.y = 0.0;
    } else {
        a.state = ArrowState::LayingOnGround;
        a.speed = Vec2::ZERO;
    }
}

/// `HitWall(platform)`.
fn hit_wall(w: &mut World, _c: &Content, id: EntityId, a: &mut Arrow, platform: EntityId) {
    match a.arrow_type {
        ArrowType::Toy => return crate::arrows::toy_hit_wall(w, id, a),
        ArrowType::Bramble => crate::arrows::bramble_hit_wall(w, id, a),
        _ => {}
    }
    if a.squished {
        return;
    }
    a.state = ArrowState::Stuck;
    a.stuck_to = Some(platform);
    a.cannot_hit = None;
    a.speed = Vec2::ZERO;
    a.cannot_pickup = 15.0;
    w.sound("env_arrowStick", w.core(id).pos.x);
    a.wiggle.start();
}

/// `EnterFallMode(bounce, zeroX, sound)`.
pub fn enter_fall_mode(
    w: &mut World,
    id: EntityId,
    a: &mut Arrow,
    bounce: bool,
    zero_x: bool,
    sound: bool,
) {
    if a.arrow_type == ArrowType::Bolt {
        crate::arrows::bolt_vanish(w, id);
        return;
    }
    if a.squished || a.state == ArrowState::Buried {
        return;
    }
    a.state = ArrowState::Falling;
    a.cannot_hit = None;
    if bounce {
        if sound {
            let name = if a.arrow_type == ArrowType::Toy {
                "env_arrowToyWall"
            } else {
                "char_arrowCollide"
            };
            w.sound(name, w.core(id).pos.x);
        }
        a.speed.y = -(2.0 + w.rng.next_float());
        a.speed.x = if zero_x {
            0.0
        } else {
            sign(-a.speed.x) * (0.5 + 0.3 * w.rng.next_float())
        };
    }
}

/// `Drop(fallSide)`.
pub fn drop(w: &mut World, id: EntityId, a: &mut Arrow, side: f32) {
    unfreeze(w, id, a);
    a.state = ArrowState::Falling;
    a.cannot_hit = None;
    a.cannot_catch = 25.0;
    a.speed.y = -(2.0 + w.rng.next_float());
    a.speed.x = side * (0.5 + 0.3 * w.rng.next_float());
    crate::arrows::on_drop(a);
}

/// `Freeze(player)`.
pub fn freeze(w: &mut World, id: EntityId, a: &mut Arrow, player: EntityId) {
    let core = w.core_mut(id);
    core.active = false;
    core.collidable = false;
    a.frozen = true;
    a.frozen_player = Some(player);
}

/// `Unfreeze()`.
pub fn unfreeze(w: &mut World, id: EntityId, a: &mut Arrow) {
    if w.exists(id) {
        let core = w.core_mut(id);
        core.active = true;
        core.collidable = true;
    }
    a.frozen = false;
    a.frozen_player = None;
}

/// `Bury(cushion)`: the arrow sticks in a corpse.
pub fn bury(_w: &mut World, _id: EntityId, a: &mut Arrow, corpse: EntityId) {
    a.buried_in = Some(corpse);
    a.cannot_hit = None;
    a.state = ArrowState::Buried;
    a.cannot_pickup = 15.0;
    a.speed = Vec2::ZERO;
}

pub fn on_squish(w: &mut World, id: EntityId, a: &mut Arrow) {
    a.squished = true;
    w.core_mut(id).collidable = false;
    a.speed = Vec2::ZERO;
    if a.state == ArrowState::Shooting {
        a.state = ArrowState::LayingOnGround;
    }
}

pub fn is_riding_solid(w: &World, c: &Content, id: EntityId, a: &Arrow, solid: EntityId) -> bool {
    match a.state {
        ArrowState::Stuck => a.stuck_to == Some(solid),
        ArrowState::LayingOnGround => {
            w.core(id).collidable && actor::base_riding_solid(w, c, id, solid)
        }
        _ => false,
    }
}

pub fn is_riding_jump_thru(w: &World, c: &Content, id: EntityId, a: &Arrow, jt: EntityId) -> bool {
    match a.state {
        ArrowState::Stuck => a.stuck_to == Some(jt),
        ArrowState::LayingOnGround => {
            w.core(id).collidable && actor::base_riding_jump_thru(w, c, id, jt)
        }
        _ => false,
    }
}

/// `CheckForTargetCollisions` (arrows.md §5.1).
fn check_for_target_collisions(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) -> bool {
    let targets: Vec<EntityId> = w.tagged(Tag::Target).to_vec();
    let arrow_p = w.placed(id);
    if !w.core(id).collidable {
        return false;
    }
    for t in targets {
        if !w.exists(t) || Some(t) == a.cannot_hit {
            continue;
        }
        let tc = w.core(t);
        if !tc.collidable {
            continue;
        }
        let recv = Placed {
            pos: tc.pos,
            collider: tc.target_collider.map(Collider::Box).unwrap_or(tc.collider),
        };
        let lvl = c.level(w.level);
        if !crate::collide::collide(&recv, &arrow_p, lvl, w.world_size(c)) {
            continue;
        }
        let hit = match w.kind(t) {
            Some(Kind::Player(_)) => {
                if !can_hurt_player(w, a, t) {
                    continue;
                }
                crate::player::on_arrow_hit(w, c, t, id, a)
            }
            Some(Kind::Chest(_)) => crate::treasure::on_arrow_hit(w, c, t, id, a),
            Some(Kind::Tower(_)) => crate::entities::on_arrow_hit(w, c, t, id, a),
            Some(Kind::Enemy(_)) => {
                if a.cannot_hit_enemies > 0.0 && w.core(t).has_tag(Tag::Enemy) {
                    continue;
                }
                crate::enemies::on_arrow_hit(w, c, t, id, a)
            }
            Some(Kind::EnemyAttack(_)) => crate::enemies::attack::on_arrow_hit(w, t, id, a),
            Some(Kind::Hazard(_)) => crate::enemies::hazard::on_arrow_hit(w, c, t, id, a),
            Some(Kind::Dw(_)) => crate::darkworld::part::on_arrow_hit(w, c, t, id, a),
            Some(Kind::Dummy(_)) => {
                let y = w.core(id).pos.y;
                crate::trials::dummy_arrow_push(w, t, y, a.speed.x)
            }
            _ => false,
        };
        if hit {
            return true;
        }
    }
    false
}

/// `Session.CanHurtPlayer` for an arrow (NoFriendlyFire, arrows.md §5.1):
/// with the variant on, team arrows skip teammates and free-for-all arrows
/// skip their owner.
fn can_hurt_player(w: &World, a: &Arrow, target: EntityId) -> bool {
    if !w.variants_for(-1).no_friendly_fire || a.player_index < 0 {
        return true;
    }
    let Some(p) = w.player(target) else {
        return true;
    };
    if p.allegiance != Allegiance::Neutral && a.team_color != Allegiance::Neutral {
        p.allegiance != a.team_color
    } else {
        a.player_index != p.index as i8
    }
}

/// Arrow-vs-arrow clash (arrows.md §6).
fn arrow_clash(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    let probe = Placed {
        pos: w.core(id).pos,
        collider: Collider::Box(OTHER_ARROW_HITBOX),
    };
    let Some(other) = w.first(c, &probe, Tag::Arrow, id) else {
        return;
    };
    let Some(Kind::Arrow(o)) = w.kind(other).copied() else {
        return;
    };
    if o.dangerous() && a.speed.dot(o.speed) <= -0.4 {
        enter_fall_mode(w, id, a, true, false, true);
        let mut o = o;
        enter_fall_mode(w, other, &mut o, true, false, true);
        if let Some(Kind::Arrow(slot)) = w.kind_mut(other) {
            *slot = o;
        }
    }
}

/// `Arrow.EnforceLimit` (arrows.md §7.1).
pub fn enforce_limit(w: &mut World, _c: &Content) {
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    if arrows.len() <= arrow_limit(w) {
        return;
    }
    for id in arrows {
        let Some(a) = w.arrow(id).copied() else {
            continue;
        };
        if w.core(id).flash.flashing {
            return;
        }
        if a.state >= ArrowState::Stuck {
            w.core_mut(id)
                .flash
                .start_for(60.0, FlashFinish::RemoveSelf);
            return;
        }
    }
}
