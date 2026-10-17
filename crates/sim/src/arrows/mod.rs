//! The arrow types (arrows.md §1, §9): per-type constants, the per-type
//! state every arrow carries ([`ArrowExtra`]) and the hooks the base arrow
//! (`crate::arrow`) calls where the original's subclasses override it.
//! Drilling, the bolt's turning, brambles and prisms live in submodules.

pub mod bolt;
pub mod brambles;
pub mod drill;
pub mod prism;

use crate::arrow::{self, Arrow, ArrowState};
use crate::content::Content;
use crate::entity::{EntityId, FlashFinish, Kind};
use crate::explosion;
use crate::hurt::{self, Ctx};
use crate::impl_hash;
use crate::math::{PI, Vec2, angle_to_vector, floor};
use crate::player::Player;
use crate::quiver::ArrowType;
use crate::world::World;

/// Names shown on pick-up (arrows.md §1).
pub const NAMES: [&str; 11] = [
    "+2",
    "BOMB",
    "SUPER BOMB",
    "LASER",
    "BRAMBLE",
    "DRILL",
    "BOLT",
    "TOY",
    "FEATHER",
    "TRIGGER",
    "PRISM",
];

/// HUD tint per type (colour A).
pub const COLOR_A: [[u8; 3]; 11] = [
    [0xF7, 0xEA, 0xC3],
    [0xF8, 0xB8, 0x00],
    [0xF8, 0xB8, 0x00],
    [0xB8, 0xF8, 0x18],
    [0xF8, 0x78, 0x58],
    [0x8E, 0xE8, 0xFF],
    [0x00, 0xFF, 0x4C],
    [0xFF, 0x6D, 0xFA],
    [0xBC, 0x70, 0xFF],
    [0x1B, 0xB7, 0xEE],
    [0xDB, 0x4A, 0xDB],
];

/// Secondary tint per type (colour B).
pub const COLOR_B: [[u8; 3]; 11] = [
    [0xFF, 0xFF, 0xFF],
    [0xF7, 0xD8, 0x83],
    [0xF7, 0xD8, 0x83],
    [0xD0, 0xF7, 0x6C],
    [0xF7, 0xB0, 0x9E],
    [0xD8, 0xF7, 0xFF],
    [0x00, 0xD3, 0x3B],
    [0xFF, 0xB5, 0xFC],
    [0xD5, 0xA5, 0xFF],
    [0x56, 0xD4, 0xFF],
    [0xFF, 0x52, 0xFF],
];

/// "Empty quiver" blink colours of the HUD.
pub const NONE_COLORS: [[u8; 3]; 2] = [[0xF8, 0x38, 0x00], [0xF8, 0x78, 0x58]];

pub const BOMB_FUSE: f32 = 50.0;
pub const LASER_MAX_BOUNCES: i32 = 8;
pub const TRIGGER_PING: f32 = 120.0;

/// `StartSpeed`.
pub fn start_speed(t: ArrowType) -> f32 {
    match t {
        ArrowType::Laser => 5.0,
        ArrowType::Feather => 6.0,
        _ => 4.0,
    }
}

/// `SeekTurnRate` (radians per frame at `tm` = 1).
pub fn seek_turn_rate(t: ArrowType) -> f32 {
    match t {
        ArrowType::Laser | ArrowType::Bolt => PI / 180.0,
        ArrowType::Feather => PI / 60.0,
        _ => PI / 90.0,
    }
}

/// `CanCatchFire`.
pub fn can_catch_fire(t: ArrowType) -> bool {
    matches!(t, ArrowType::Normal | ArrowType::Feather)
}

/// Light radius (presentation).
pub fn light_radius(t: ArrowType) -> f32 {
    match t {
        ArrowType::Laser | ArrowType::Bolt | ArrowType::Prism => 30.0,
        ArrowType::Trigger => 70.0,
        _ => 40.0,
    }
}

/// State of the type-specific parts of an arrow (one flat struct so every
/// arrow stays `Copy` and the same size).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ArrowExtra {
    /// Bomb / super bomb fuse frames left (runs while `fuse_on`).
    pub fuse: f32,
    pub fuse_on: bool,
    pub exploded: bool,
    /// Laser wall contacts so far.
    pub bounced: i32,
    /// Bramble: power spent (growth started or a corpse took it).
    pub power_used: bool,
    /// Bramble: the growth routine finished (`FinishBrambles`).
    pub brambles_done: bool,
    /// Drill.
    pub has_drilled: bool,
    pub inert: bool,
    /// Bolt: frames before it may turn again; turns made.
    pub can_turn: f32,
    pub turns: u16,
    /// Feather: sine counter and the unit perpendicular of its heading.
    pub sine: f32,
    pub perpendicular: Vec2,
    /// Trigger: the player that can detonate it, and the ping loop counter.
    pub detonator: Option<EntityId>,
    pub ping: f32,
    /// On fire (only `CanCatchFire` types; lit by torches).
    pub burning: bool,
    /// Fire frames left (60 when lit).
    pub fire_t: f32,
}

impl_hash!(ArrowExtra {
    fuse,
    fuse_on,
    exploded,
    bounced,
    power_used,
    brambles_done,
    has_drilled,
    inert,
    can_turn,
    turns,
    sine,
    perpendicular,
    detonator,
    ping,
    burning,
    fire_t
});

/// Subclass `Init` hooks (arrows.md §9).
pub fn init(a: &mut Arrow) {
    a.extra = ArrowExtra::default();
    match a.arrow_type {
        ArrowType::Bomb | ArrowType::SuperBomb => {
            a.extra.fuse = BOMB_FUSE;
            a.extra.fuse_on = true;
        }
        ArrowType::Bolt => a.extra.can_turn = 6.0,
        ArrowType::Feather => {
            let s = a.speed;
            a.extra.perpendicular = Vec2::new(-s.y, s.x).safe_normalize();
        }
        _ => {}
    }
}

/// Run `f` on the arrow `id`'s state (taken out of its slot).
pub fn with_arrow<R>(
    w: &mut World,
    id: EntityId,
    f: impl FnOnce(&mut World, &mut Arrow) -> R,
) -> Option<R> {
    let Some(Kind::Arrow(mut a)) = w.take_kind(id) else {
        return None;
    };
    let r = f(w, &mut a);
    if w.exists(id) {
        w.put_kind(id, Kind::Arrow(a));
    }
    Some(r)
}

/// Work the subclasses do after the base `Arrow.Update` (fuses, decays,
/// the trigger ping).
pub fn after_update(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    let tm = w.time.time_mult;
    match a.arrow_type {
        ArrowType::Bomb | ArrowType::SuperBomb => {
            if a.extra.fuse_on {
                a.extra.fuse -= tm;
                if a.extra.fuse <= 0.0 {
                    explode(w, c, id, a, &mut None);
                }
            }
        }
        ArrowType::Laser => {
            if !w.core(id).flash.flashing && a.state >= ArrowState::Stuck {
                w.core_mut(id)
                    .flash
                    .start_for(60.0, FlashFinish::RemoveSelf);
            }
        }
        ArrowType::Bramble => {
            if a.extra.brambles_done && !w.core(id).flash.flashing && a.state >= ArrowState::Stuck {
                w.core_mut(id)
                    .flash
                    .start_for(60.0, FlashFinish::RemoveSelf);
            }
        }
        ArrowType::Trigger if a.extra.detonator.is_some() => {
            a.extra.ping -= tm;
            if a.extra.ping <= 0.0 {
                a.extra.ping = TRIGGER_PING;
                a.wiggle.start();
                w.sound("pu_triggerArrowPrimed", w.core(id).pos.x);
            }
        }
        _ => {}
    }
}

/// `plusOneKill`: buried in the corpse of a player the owner may score on.
fn plus_one_kill(w: &World, a: &Arrow) -> bool {
    match a.buried_in.and_then(|cp| w.kind(cp)) {
        Some(Kind::Corpse(cp)) => !hurt::is_friendly(w, a.player_index, cp.player_index),
        _ => false,
    }
}

/// The blast point: 10 px behind the tip, floored.
fn blast_point(w: &World, id: EntityId, a: &Arrow) -> Vec2 {
    let p = w.core(id).pos - angle_to_vector(a.direction, 10.0);
    Vec2::new(floor(p.x), floor(p.y))
}

/// `Explode()` of bomb, super bomb and trigger arrows (§9.2, §9.3, §9.9).
/// `ctx` is the player whose update is running, if any (a trigger
/// detonated by its frozen victim).
pub fn explode(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow, ctx: &mut Ctx) {
    if a.extra.exploded || !w.exists(id) {
        return;
    }
    a.extra.exploded = true;
    a.extra.fuse_on = false;
    let at = blast_point(w, id, a);
    let pos = w.core(id).pos;
    let plus = plus_one_kill(w, a);
    let pi = a.player_index;
    match a.arrow_type {
        ArrowType::SuperBomb => {
            if !explosion::spawn_super(w, c, at, pi, plus, ctx) {
                explosion::spawn_super(w, c, pos, pi, plus, ctx);
            }
            w.sound("pu_superBombExplode", pos.x);
        }
        ArrowType::Trigger => {
            remove_detonator(w, id, a, ctx);
            let centre = if explosion::spawn_get(w, c, at, pi, plus, ctx) {
                Some(at)
            } else if explosion::spawn_get(w, c, pos, pi, plus, ctx) {
                Some(pos)
            } else {
                None
            };
            if let (Some(centre), true, Some(victim)) = (centre, a.frozen, a.frozen_player) {
                explosion::hurt_frozen_victim(w, c, centre, pi, victim, ctx);
            }
            w.sound("pu_bombArrowExplode", pos.x);
        }
        _ => {
            if !explosion::spawn(w, c, at, pi, plus, ctx) {
                explosion::spawn(w, c, pos, pi, plus, ctx);
            }
            w.sound("pu_bombArrowExplode", pos.x);
        }
    }
    w.remove(id);
}

/// `TriggerArrow.Detonate()`.
pub fn detonate(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow, ctx: &mut Ctx) {
    a.extra.detonator = None;
    if w.exists(id) && !w.core(id).marked_for_removal {
        explode(w, c, id, a, ctx);
    }
}

/// `RemoveDetonator()`: unlink from the detonating player's list.
pub fn remove_detonator(w: &mut World, id: EntityId, a: &mut Arrow, ctx: &mut Ctx) {
    if let Some(d) = a.extra.detonator.take() {
        hurt::with_player_ctx(w, d, ctx, |_, p| p.remove_trigger(id));
    }
}

/// `HitLava()` (§3.10).
pub fn hit_lava(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    if w.core(id).marked_for_removal {
        return;
    }
    match a.arrow_type {
        ArrowType::Bomb | ArrowType::SuperBomb | ArrowType::Trigger => {
            explode(w, c, id, a, &mut None)
        }
        ArrowType::Laser | ArrowType::Bolt => {}
        _ => {
            w.sound("sfx_burn", w.core(id).pos.x);
            w.remove(id);
        }
    }
}

/// `OnPlayerCatch(player)` (§4.2): bolt shock, trigger unlink.
pub fn on_player_catch(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    a: &mut Arrow,
    pid: EntityId,
    p: &mut Player,
) {
    match a.arrow_type {
        ArrowType::Bolt if a.dangerous() => {
            let at = w.core(pid).pos;
            explosion::spawn_bolt_catch(w, at, p.index as i8);
            w.sound("sfx_boltArrowExplode", at.x);
        }
        ArrowType::Trigger => {
            let mut ctx: Ctx = Some((pid, p));
            remove_detonator(w, id, a, &mut ctx);
        }
        _ => {}
    }
}

/// `Fire.Start()` on an arrow (torches): 60 frames, only `CanCatchFire` types.
pub fn ignite(a: &mut Arrow) -> bool {
    if !can_catch_fire(a.arrow_type) {
        return false;
    }
    a.extra.burning = true;
    a.extra.fire_t = 60.0;
    true
}

/// `PrismCheck()`.
pub fn prism_check(a: &Arrow) -> bool {
    a.arrow_type == ArrowType::Prism && a.dangerous()
}

/// Shared by the base update (§3.3 step 6): a burning arrow in flight
/// ignites the bramble tiles it touches.
pub fn fire_step(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) {
    if a.extra.burning {
        a.extra.fire_t -= w.time.time_mult;
        if a.extra.fire_t <= 0.0 {
            a.extra.burning = false;
            a.extra.fire_t = 0.0;
        }
    }
    if can_catch_fire(a.arrow_type) && a.extra.burning && a.state < ArrowState::Stuck {
        let p = w.placed(id);
        for b in w.all(c, &p, crate::entity::Tag::Brambles, id) {
            brambles::ignite(w, b);
        }
    }
}

/// `EnterFallMode` override of the bolt: a small shock and gone.
pub fn bolt_vanish(w: &mut World, id: EntityId) {
    let at = w.core(id).pos;
    w.emit(crate::events::Event::Particles {
        kind: crate::events::ParticleKind::SmallShock,
        player: u8::MAX,
        pos: at,
        count: 1,
        dir: 0.0,
    });
    w.remove(id);
}

/// Laser wall contact while shooting (§9.4): reflect unless this is the
/// eighth contact. Returns true when the contact was consumed.
pub fn laser_bounce(w: &mut World, id: EntityId, a: &mut Arrow, horizontal: bool) -> bool {
    if a.state != ArrowState::Shooting {
        return false;
    }
    a.extra.bounced += 1;
    if a.extra.bounced >= LASER_MAX_BOUNCES
        && !crate::pickups::variant(
            w,
            crate::variants::Variant::InfiniteLasers,
            a.player_index as i32,
        )
    {
        w.sound("char_fireLaserDeflectEnd", w.core(id).pos.x);
        return false;
    }
    if horizontal {
        a.speed.x = -a.speed.x;
    } else {
        a.speed.y = -a.speed.y;
    }
    a.direction = a.speed.angle();
    let k = (a.extra.bounced - 1).clamp(0, 6) as u8;
    w.sound_indexed("char_fireLaserDeflect", k, w.core(id).pos.x);
    true
}

/// Toy `HitWall`: bounce back at 2 px/frame, never stick (§9.8).
pub fn toy_hit_wall(w: &mut World, id: EntityId, a: &mut Arrow) {
    if a.collided_h {
        a.speed.y = -a.speed.y;
    } else {
        a.speed.x = -a.speed.x;
    }
    a.speed = a.speed.safe_normalize() * -2.0;
    a.state = ArrowState::Falling;
    a.cannot_hit = None;
    w.sound("env_arrowToyWall", w.core(id).pos.x);
}

/// Bramble `HitWall`: the first contact grows a patch (§9.5).
pub fn bramble_hit_wall(w: &mut World, id: EntityId, a: &mut Arrow) {
    if a.extra.power_used || a.squished {
        return;
    }
    a.extra.power_used = true;
    let at = w.core(id).pos;
    brambles::start_growth(w, at, a.player_index, Some(id), 3, false);
}

/// Toy arrows replace target collisions with a squeaky bounce (§9.8).
pub fn toy_target_collisions(w: &mut World, c: &Content, id: EntityId, a: &mut Arrow) -> bool {
    use crate::collide::{Collider, Placed};
    if !w.core(id).collidable {
        return false;
    }
    let me = w.placed(id);
    let lvl = c.level(w.level);
    let ws = w.world_size(c);
    let targets: Vec<EntityId> = w.tagged(crate::entity::Tag::Target).to_vec();
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
        if !crate::collide::collide(&recv, &me, lvl, ws) {
            continue;
        }
        let v = (tc.pos - (me.pos - a.speed)).safe_normalize();
        if let Some(Kind::Player(_)) = w.kind(t) {
            hurt::with_player(w, t, |w, p| hurt::on_squeaky_bounce(w, c, t, p, v));
        } else if let Some(Kind::Tower(_)) = w.kind(t) {
            crate::entities::on_squeaky_bounce(w, t, v, a.player_index);
        } else {
            crate::enemies::on_squeaky_bounce(w, t, v);
        }
        a.state = ArrowState::Falling;
        a.cannot_hit = None;
        a.speed = v * -2.0;
        a.wiggle.start();
        w.sound("env_arrowToyChar", me.pos.x);
        return true;
    }
    false
}

/// Start values for an arrow dropped out of a quiver: bombs lose their
/// fuse (§3.8).
pub fn on_drop(a: &mut Arrow) {
    if matches!(a.arrow_type, ArrowType::Bomb | ArrowType::SuperBomb) {
        a.extra.fuse_on = false;
    }
}

/// Re-export for callers that only need the base module.
pub use arrow::enter_fall_mode;
