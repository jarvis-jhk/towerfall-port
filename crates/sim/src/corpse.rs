//! The archer's corpse (`PlayerCorpse`, player.md §14): physics, ledge and
//! wall pinning, being squished, and the `ArrowCushion` holding the
//! arrows buried in it. Variant behaviours (exploding/trigger corpses,
//! ghosts, revival, brambles, prism vanish) come with their features.

use crate::actor::{self, SquishDir};
use crate::arrow::{self, Arrow};
use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox};
use crate::components::SpriteAnim;
use crate::content::Content;
use crate::entity::{Allegiance, Core, EntityId, Kind, Tag, depth};
use crate::impl_hash;
use crate::math::{PI, Vec2, approach, min, sign};
use crate::player::{HatState, Player};
use crate::world::World;

pub const HITBOX: Hitbox = Hitbox::wrapping(8.0, 8.0, -4.0, 0.0);
const CUSHION_CAP: usize = 8;

/// Arrows buried in a body (`ArrowCushion` with locked offset).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cushion {
    arrows: [Option<EntityId>; CUSHION_CAP],
    /// Random extra rotation per arrow (cosmetic).
    rot: [f32; CUSHION_CAP],
}

impl StableHash for Cushion {
    fn stable_hash(&self, h: &mut Hasher) {
        self.arrows.stable_hash(h);
    }
}

impl Cushion {
    pub fn iter(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.arrows.iter().flatten().copied()
    }
    pub fn is_empty(&self) -> bool {
        self.arrows.iter().all(|a| a.is_none())
    }
    fn add(&mut self, a: EntityId, rot: f32) {
        if let Some(i) = self.arrows.iter().position(|x| x.is_none()) {
            self.arrows[i] = Some(a);
            self.rot[i] = rot;
        }
    }
    fn remove(&mut self, a: EntityId) {
        for s in &mut self.arrows {
            if *s == Some(a) {
                *s = None;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CorpseAnim {
    Ground = 0,
    Fall,
    Pinned,
    Slouched,
    Flying,
    Ledge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corpse {
    pub player_index: i8,
    pub archer: u16,
    pub killer: i8,
    pub allegiance: Allegiance,
    pub team_color: Allegiance,
    pub hat: HatState,
    pub facing: i32,
    pub speed: Vec2,
    pub cushion: Cushion,
    pub pinned: bool,
    pub against_wall: bool,
    pub ledge: i32,
    pub squished: Vec2,
    pub squished_counter: f32,
    pub fall_sprite_counter: f32,
    pub death_angle: f32,
    pub draw_opacity: f32,
    pub arrows: crate::quiver::Quiver,
    /// Frames since a bramble arrow killed it (-1: no thorns).
    pub bramble_t: f32,
    /// Frames since a prism arrow killed it (-1: no vanish).
    pub prism_t: f32,
    pub prism_fall: bool,
    // presentation
    pub anim: CorpseAnim,
    pub sprite: SpriteAnim,
    pub scale: Vec2,
    /// Return As Ghosts: ticks until the ghost appears (`< 0` = never).
    pub ghost_t: f32,
    /// Exploding / trigger corpses: the charge 0..1 (player.md §14.5).
    pub explode_t: f32,
    /// Corpses Drop Arrows: ticks to the next arrow (`< 0` = off) and side.
    pub drop_t: f32,
    pub drop_dir: i8,
    /// The charge loop sound is on (presentation).
    pub charging: bool,
    /// Burning frames left (hot coals, lava, explosions, burning arrows;
    /// cosmetic apart from the light).
    pub fire_t: f32,
    /// `dodgeTooLateCounter` (10 frames after death; stat DodgesTooLate).
    pub dodge_late: f32,
}

impl_hash!(Corpse {
    player_index,
    archer,
    killer,
    allegiance,
    team_color,
    hat,
    facing,
    speed,
    cushion,
    pinned,
    against_wall,
    ledge,
    squished,
    squished_counter,
    fall_sprite_counter,
    death_angle,
    draw_opacity,
    arrows,
    bramble_t,
    prism_t,
    prism_fall,
    explode_t,
    drop_t,
    drop_dir,
    fire_t,
    dodge_late
});

impl Corpse {
    pub fn is_squished(&self) -> bool {
        !self.squished.is_zero()
    }
    /// The thorns hurt (`BrambleCollidable`).
    pub fn brambles_harmful(&self) -> bool {
        self.bramble_t >= BRAMBLE_GROW_AT + 20.0 && self.bramble_t < BRAMBLE_SHRINK_AT
    }
    /// Thorn scale for drawing (0 when none).
    pub fn bramble_scale(&self) -> f32 {
        use crate::components::Ease;
        let t = self.bramble_t;
        if t < BRAMBLE_GROW_AT {
            0.0
        } else if t < BRAMBLE_GROW_AT + 20.0 {
            Ease::BackOut.apply((t - BRAMBLE_GROW_AT) / 20.0)
        } else if t < BRAMBLE_SHRINK_AT {
            1.0
        } else {
            1.0 - Ease::BackIn.apply(((t - BRAMBLE_SHRINK_AT) / 20.0).min(1.0))
        }
    }
    /// Hair opacity of the corpse: the prism vanish fades the hair with the
    /// body (`1 - BackIn` over the last 15 frames), else 1.
    pub fn hair_alpha(&self) -> f32 {
        if self.prism_t < 15.0 {
            1.0
        } else {
            1.0 - crate::components::Ease::BackIn.apply(((self.prism_t - 15.0) / 15.0).min(1.0))
        }
    }

    /// Prism vanish scale multiplier (1 normally).
    pub fn vanish_scale(&self) -> f32 {
        if self.prism_t < 15.0 {
            1.0
        } else {
            1.2 * (1.0
                - crate::components::Ease::BackIn.apply(((self.prism_t - 15.0) / 15.0).min(1.0)))
        }
    }
}

pub const BRAMBLE_GROW_AT: f32 = 10.0;
pub const BRAMBLE_SHRINK_AT: f32 = 610.0;

/// `new PlayerCorpse(player, killer)` (§14.1), added to the level.
pub fn create(w: &mut World, c: &Content, pid: EntityId, p: &Player) -> EntityId {
    let pos = w.core(pid).pos;
    let mut core = Core::new(
        pos,
        Collider::Box(HITBOX),
        depth::CORPSE,
        &[Tag::Corpse, Tag::ExplosionCollider, Tag::Actor],
    );
    core.screen_wrap = true;
    core.allegiance = p.allegiance;
    let mut speed = p.speed;
    // On Added: a small hop if resting on a solid.
    if speed.y > -1.0 && w.check_at(c, pid, pos + Vec2::new(0.0, 1.0), Tag::Solid) {
        speed.y = -1.0;
    }
    let cp = Corpse {
        player_index: p.index as i8,
        archer: p.archer,
        killer: -1,
        allegiance: p.allegiance,
        team_color: p.team_color,
        hat: p.hat,
        facing: p.facing,
        speed,
        cushion: Cushion::default(),
        pinned: false,
        against_wall: false,
        ledge: 0,
        squished: Vec2::ZERO,
        squished_counter: 0.0,
        fall_sprite_counter: 0.0,
        death_angle: 0.0,
        draw_opacity: p.invis_opacity,
        arrows: p.arrows,
        bramble_t: -1.0,
        prism_t: -1.0,
        prism_fall: false,
        anim: CorpseAnim::Ground,
        sprite: SpriteAnim::default(),
        scale: Vec2::ONE,
        ghost_t: ghost_delay(w, p.index),
        explode_t: 0.0,
        drop_t: -1.0,
        drop_dir: 1,
        charging: false,
        fire_t: 0.0,
        dodge_late: 10.0,
    };
    let mut cp = cp;
    crate::variant_fx::corpse_added(w, &mut cp);
    let (pi, all) = (cp.player_index, cp.allegiance);
    let id = w.add(core, Kind::Corpse(cp));
    crate::revive::on_corpse_added(w, c, id, pi, all, pos);
    id
}

/// Return As Ghosts: 60 ticks (55 with exploding corpses), else never.
fn ghost_delay(w: &World, p: u8) -> f32 {
    use crate::variants::Variant;
    let set = w
        .versus
        .as_ref()
        .map(|m| m.settings.variants)
        .unwrap_or(w.setup.variant_set);
    if !set.get(Variant::ReturnAsGhosts, p as i32) {
        -1.0
    } else if set.get(Variant::ExplodingCorpses, p as i32) {
        55.0
    } else {
        60.0
    }
}

/// Spawn the ghost once its delay ran out (waits while squished/revived).
fn ghost_update(w: &mut World, c: &Content, id: EntityId, cp: &mut Corpse) {
    if cp.ghost_t < 0.0 || cp.player_index < 0 {
        return;
    }
    cp.ghost_t -= w.time.time_mult;
    if cp.ghost_t > 0.0 {
        return;
    }
    let revived = crate::revive::of_player(w, cp.player_index as u8)
        .is_some_and(|r| matches!(w.kind(r), Some(Kind::Reviver(t)) if t.revived || t.reviving));
    if cp.is_squished() || revived {
        cp.ghost_t = 1.0;
        return;
    }
    cp.ghost_t = -1.0;
    let pos = w.core(id).pos;
    crate::enemies::player_ghost::spawn_for(
        w,
        c,
        cp.player_index as u8,
        pos,
        cp.speed,
        cp.allegiance,
    );
}

fn with<R>(w: &mut World, id: EntityId, f: impl FnOnce(&mut World, &mut Corpse) -> R) -> Option<R> {
    let Some(Kind::Corpse(mut cp)) = w.take_kind(id) else {
        return None;
    };
    let r = f(w, &mut cp);
    w.put_kind(id, Kind::Corpse(cp));
    Some(r)
}

/// `DieByArrow(arrow, ledge)` (§14.2): the arrow is buried in the corpse.
pub fn die_by_arrow(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    aid: EntityId,
    a: &mut Arrow,
    ledge: i32,
) {
    // `ArrowCushion.AddArrow`: the buried angle is gameplay state (a
    // released arrow keeps it), drawn from the sim generator.
    let rot = (w.rng.range_i(-22, 44)) as f32 * PI / 180.0;
    let kind = a.arrow_type;
    let burning = a.extra.burning;
    // A prism kill is an ordinary death in the solo modes.
    let prism = kind == crate::quiver::ArrowType::Prism && !crate::coop::solo_mode(w);
    if kind == crate::quiver::ArrowType::Bramble {
        // The growth routine never finishes for a kill, so the arrow
        // stays buried (no decay).
        a.extra.power_used = true;
        w.tag(id, Tag::PlayerCollider, true);
        let d = w.core(id).depth;
        w.set_depth(id, d - 5);
    }
    with(w, id, |w, cp| {
        cp.death_angle = a.speed.angle();
        cp.killer = a.player_index;
        if kind == crate::quiver::ArrowType::Bramble {
            cp.bramble_t = 0.0;
        } else if prism {
            cp.prism_t = 0.0;
        } else if ledge != 0 {
            cp.ledge = ledge;
            cp.speed = Vec2::ZERO;
        } else {
            if a.speed.x != 0.0 {
                cp.facing = -sign(a.speed.x) as i32;
            }
            cp.speed = Vec2::new(a.speed.x * 0.8, a.speed.y * 1.2);
        }
        cp.cushion.add(aid, rot);
        if burning {
            cp.fire_t = FIRE_TIME;
        }
        arrow::bury(w, aid, a, id);
    });
}

/// Corpse fire length (frames).
pub const FIRE_TIME: f32 = 60.0;

/// `fire.Start()` on a corpse.
pub fn start_fire(w: &mut World, id: EntityId) {
    if let Some(Kind::Corpse(cp)) = w.kind_mut(id) {
        cp.fire_t = FIRE_TIME;
    }
}

/// Fire tick: hot coals under the corpse keep it alight; flame particles.
fn fire_update(w: &mut World, c: &Content, id: EntityId, cp: &mut Corpse) {
    let pos = w.core(id).pos;
    if w.check_at(c, id, pos + Vec2::new(0.0, 1.0), Tag::HotCoals) {
        cp.fire_t = FIRE_TIME;
    }
    if cp.fire_t > 0.0 {
        cp.fire_t -= w.time.time_mult;
        if w.time.on_interval(2) {
            crate::entities::particles(w, "Fire", pos, 1, Vec2::new(3.0, 4.0), -PI / 2.0);
        }
    }
}

/// `LavaCollide`: the corpse burns; thorns go.
pub fn lava_collide(w: &mut World, id: EntityId) {
    start_fire(w, id);
    let thorny = matches!(w.kind(id), Some(Kind::Corpse(cp)) if cp.bramble_t >= 0.0);
    if thorny {
        w.tag(id, Tag::PlayerCollider, false);
        if let Some(Kind::Corpse(cp)) = w.kind_mut(id) {
            cp.bramble_t = -1.0;
        }
    }
}

/// Thorns hurt an archer touching a brambled corpse.
pub fn on_player_collide(w: &mut World, c: &Content, e: EntityId, pid: EntityId, p: &mut Player) {
    let Some(Kind::Corpse(cp)) = w.kind(e).copied() else {
        return;
    };
    if cp.brambles_harmful() && crate::hurt::can_hurt_player(w, cp.killer, p.index as i8) {
        let from = w.core(e).pos;
        crate::hurt::hurt(
            w,
            c,
            pid,
            p,
            crate::player::DeathCause::Brambles,
            from,
            cp.killer,
            Some("env_deathSpike"),
        );
    }
}

/// `ReleaseArrows(speed)`: every buried arrow falls out.
pub(crate) fn release_arrows(w: &mut World, cp: &mut Corpse, speed: Vec2) {
    for k in 0..CUSHION_CAP {
        let Some(a) = cp.cushion.arrows[k].take() else {
            continue;
        };
        if let Some(ar) = w.arrow_mut(a) {
            ar.buried_in = None;
            ar.state = arrow::ArrowState::Falling;
            ar.speed = speed;
        }
    }
    cp.pinned = false;
}

/// Thorns and prism vanish timelines (player.md §14.2, §14.6).
fn update_special(w: &mut World, id: EntityId, cp: &mut Corpse) -> bool {
    let t = w.time.time_mult;
    if cp.bramble_t >= 0.0 {
        let before = cp.bramble_t;
        cp.bramble_t += t;
        let x = w.core(id).pos.x;
        if before < BRAMBLE_GROW_AT && cp.bramble_t >= BRAMBLE_GROW_AT {
            w.sound("pu_brambleGrow", x);
        }
        if before < BRAMBLE_SHRINK_AT && cp.bramble_t >= BRAMBLE_SHRINK_AT {
            w.sound("pu_brambleDisappear", x);
        }
        if cp.bramble_t >= BRAMBLE_SHRINK_AT + 20.0 {
            cp.bramble_t = -1.0;
            w.tag(id, Tag::PlayerCollider, false);
        }
    }
    if cp.prism_t >= 0.0 {
        let before = cp.prism_t;
        cp.prism_t += t;
        let now = cp.prism_t;
        let crossed = |k: f32| before < k && now >= k;
        if crossed(5.0) {
            cp.prism_fall = true;
            cp.speed.x *= 0.3;
        }
        if crossed(15.0) {
            let sp = cp.speed;
            release_arrows(w, cp, sp);
            w.sound("sfx_corpseVanish", w.core(id).pos.x);
        }
        if crossed(20.0) {
            w.emit(crate::events::Event::Particles {
                kind: crate::events::ParticleKind::PrismVanish,
                player: cp.player_index as u8,
                pos: w.core(id).pos,
                count: 20,
                dir: 0.0,
            });
        }
        if cp.prism_t >= 30.0 {
            w.remove(id);
            return true;
        }
    }
    false
}

/// `DieBySquish(direction, ducking)`.
pub fn die_by_squish(w: &mut World, id: EntityId, dir: SquishDir, ducking: bool) {
    let core = w.core_mut(id);
    core.collidable = false;
    core.pushable = false;
    if ducking {
        core.pos.y += 8.0;
    }
    with(w, id, |_, cp| {
        cp.squished_counter = 30.0;
        cp.squished = dir.vec();
        if cp.squished.x != 0.0 {
            cp.facing = cp.squished.x as i32;
        }
    });
}

/// `DieByJumpedOn()`.
pub fn die_by_jumped_on(w: &mut World, id: EntityId) {
    with(w, id, |_, cp| cp.speed = Vec2::new(0.0, 2.8));
}

/// `ArrowTaken()` (a player pulled an arrow out).
pub fn arrow_taken(w: &mut World, _c: &Content, id: EntityId, aid: EntityId) {
    if let Some(Kind::Corpse(cp)) = w.kind_mut(id) {
        cp.cushion.remove(aid);
        cp.pinned = false;
    }
}

pub fn on_squish(w: &mut World, id: EntityId, cp: &mut Corpse, dir: SquishDir) {
    let core = w.core_mut(id);
    core.collidable = false;
    core.pushable = false;
    cp.squished_counter = 30.0;
    cp.squished = dir.vec();
}

pub fn is_riding_solid(w: &World, c: &Content, id: EntityId, cp: &Corpse, solid: EntityId) -> bool {
    if cp.is_squished() {
        return false;
    }
    if actor::base_riding_solid(w, c, id, solid) {
        return true;
    }
    let pos = w.core(id).pos;
    let side = |dx: f32| {
        let p = crate::collide::Placed {
            pos: pos + Vec2::new(dx, 0.0),
            collider: w.core(id).collider,
        };
        w.check(c, &p, solid)
    };
    (cp.pinned && side(-cp.facing as f32)) || (cp.ledge != 0 && side(cp.ledge as f32))
}

/// Per-frame update (§14.3).
pub fn update(w: &mut World, c: &Content, id: EntityId, cp: &mut Corpse) {
    ghost_update(w, c, id, cp);
    fire_update(w, c, id, cp);
    if crate::variant_fx::corpse_update(w, c, id, cp) {
        return;
    }
    if update_special(w, id, cp) {
        return;
    }
    let t = w.time.time_mult;
    cp.draw_opacity = approach(cp.draw_opacity, 1.0, 0.02 * t);
    if cp.pinned && cp.cushion.is_empty() {
        cp.pinned = false;
    }
    let solid_at = |w: &World, at: Vec2| w.check_at(c, id, at, Tag::Solid);
    let pos = w.core(id).pos;
    if cp.is_squished() {
        cp.speed = Vec2::ZERO;
        if solid_at(w, pos) {
            cp.squished_counter = 30.0;
            cp.scale = if cp.squished.x != 0.0 {
                Vec2::new(0.6, 1.2)
            } else {
                Vec2::new(1.6, 0.6)
            };
        } else {
            cp.scale.x = approach(cp.scale.x, 1.0, 0.02 * t);
            cp.scale.y = approach(cp.scale.y, 1.0, 0.02 * t);
            cp.squished_counter -= t;
            if cp.squished_counter <= 0.0 || !solid_at(w, pos + cp.squished) {
                if cp.squished.x != 0.0 {
                    cp.speed = -cp.squished * 0.6;
                    cp.scale = Vec2::new(1.4, 0.6);
                } else {
                    cp.scale = Vec2::new(0.6, 1.4);
                }
                let core = w.core_mut(id);
                core.collidable = true;
                core.pushable = true;
                cp.squished = Vec2::ZERO;
                w.sound("char_squishPop", pos.x);
            }
        }
        cp.anim = if cp.squished.y != 0.0 {
            CorpseAnim::Ground
        } else {
            CorpseAnim::Pinned
        };
        actor::enforce_screen_wrap(w, c, id);
    } else if cp.ledge != 0 {
        cp.anim = CorpseAnim::Ledge;
        if !solid_at(w, pos + Vec2::new(cp.ledge as f32, 0.0)) {
            cp.ledge = 0;
        }
        actor::enforce_screen_wrap(w, c, id);
    } else {
        let below = actor::check_below(w, c, id);
        if cp.against_wall && !solid_at(w, pos + Vec2::new(-cp.facing as f32, 0.0)) {
            cp.pinned = false;
            cp.against_wall = false;
        }
        let mut extra = false;
        let reviving = crate::revive::corpse_reviving(w, id);
        if below {
            cp.fall_sprite_counter = 2.0;
            // Archer corpses lying on each other slide apart (0.5).
            let mut target = 0.0;
            if cp.player_index >= 0 {
                let me = w.placed(id);
                let others: Vec<EntityId> = w.tagged(Tag::Corpse).to_vec();
                for o in others {
                    if o == id
                        || !matches!(w.kind(o), Some(Kind::Corpse(k)) if k.player_index >= 0)
                        || !w.check(c, &me, o)
                    {
                        continue;
                    }
                    let ox = w.core(o).pos.x;
                    let mut dir = if ox == pos.x {
                        if o.index > id.index { 1 } else { -1 }
                    } else if ox < pos.x {
                        1
                    } else {
                        -1
                    };
                    if !can_slip(w, c, id, dir) {
                        dir = 0;
                    }
                    target = dir as f32 * 0.5;
                    break;
                }
            }
            if !cp.prism_fall {
                let f = crate::entities::corpse_ground_friction(w, c, id, cp.player_index);
                cp.speed.x = approach(cp.speed.x, target, f * t);
            }
            if cp.speed.x.abs() <= 0.5 {
                if !actor::check_below_x(w, c, id, 4.0) || !actor::check_below_x(w, c, id, 2.0) {
                    cp.speed.x = 0.5;
                    extra = true;
                } else if !actor::check_below_x(w, c, id, -4.0)
                    || !actor::check_below_x(w, c, id, -2.0)
                {
                    cp.speed.x = -0.5;
                    extra = true;
                }
            }
        } else if !cp.pinned {
            if cp.fall_sprite_counter > 0.0 {
                cp.fall_sprite_counter -= t;
            }
            if !cp.prism_fall {
                cp.speed.x = approach(cp.speed.x, 0.0, 0.02 * t);
            }
            // No gravity while being revived; a prism fall is slow.
            if !reviving {
                let mut m = if cp.against_wall { 0.6 } else { 1.0 };
                if cp.prism_fall {
                    m *= 0.3;
                }
                let k = if cp.speed.y.abs() < 0.5 { 0.5 } else { 1.0 };
                cp.speed.y = min(cp.speed.y + 0.3 * k * m * t, 2.8 * m);
            }
        }
        if let Some(s) = actor::move_h(w, c, id, cp.speed.x * t) {
            collide_h(w, c, id, cp, s);
        }
        if let Some(_s) = actor::move_v(w, c, id, cp.speed.y * t) {
            if cp.speed.y > 0.0 && !reviving {
                w.sound("char_bodyFall", w.core(id).pos.x);
                cp.scale = Vec2::new(1.4, 0.6);
            }
            cp.speed.y = 0.0;
        }
        if extra {
            actor::move_v(w, c, id, 1.0);
        }
        actor::enforce_screen_wrap(w, c, id);
        if cp.speed.x.abs() <= 1.0 {
            cp.scale.x = approach(cp.scale.x, 1.0, 0.01 * t);
            cp.scale.y = approach(cp.scale.y, 1.0, 0.01 * t);
        }
        let on_ground = actor::check_below(w, c, id);
        cp.anim = if cp.speed.x.abs() >= 2.5 {
            CorpseAnim::Flying
        } else if on_ground && !reviving {
            if cp.against_wall {
                CorpseAnim::Slouched
            } else {
                CorpseAnim::Ground
            }
        } else if cp.against_wall || reviving {
            CorpseAnim::Pinned
        } else if cp.fall_sprite_counter > 0.0 {
            CorpseAnim::Ground
        } else {
            CorpseAnim::Fall
        };
    }
    w.core_mut(id).flash.update(t);
    // Sprite.
    if let Some(a) = c.archers.get(cp.archer as usize)
        && let Some(s) = c.sprite(a.corpse)
    {
        cp.sprite
            .play(&s.anims, a.corpse_anims[cp.anim as usize], false);
        cp.sprite.update(&s.anims, w.time.delta_time);
    }
    pose_cushion(w, id, cp);
}

/// `CanSlip(dir)`: no solid and no new corpse one pixel that way.
fn can_slip(w: &World, c: &Content, id: EntityId, dir: i32) -> bool {
    let at = w.core(id).pos + Vec2::new(dir as f32, 0.0);
    !w.check_at(c, id, at, Tag::Solid) && w.first_outside(c, id, at, Tag::Corpse).is_none()
}

fn collide_h(w: &mut World, c: &Content, id: EntityId, cp: &mut Corpse, _s: EntityId) {
    if !cp.cushion.is_empty() && cp.speed.x.abs() >= 2.5 {
        let core = w.core(id);
        let p = Vec2::new(core.pos.x + 6.0 * sign(cp.speed.x), core.top() + 2.0);
        if w.point_check(c, p, Tag::Solid) {
            cp.pinned = true;
            cp.speed.y = 0.0;
            w.sound("char_bodyPinned", p.x);
        }
    }
    cp.against_wall = true;
    cp.speed.x = 0.0;
}

/// Anchor and rotation of buried arrows per animation (§14.3 step 7).
fn cushion_pose(anim: CorpseAnim) -> (Vec2, f32) {
    match anim {
        CorpseAnim::Flying | CorpseAnim::Pinned => (Vec2::new(4.0, 13.0), PI),
        CorpseAnim::Ground => (Vec2::new(7.0, 17.0), PI / 2.0),
        CorpseAnim::Slouched => (Vec2::new(6.0, 16.0), 3.0 * PI / 4.0),
        CorpseAnim::Fall => (Vec2::new(6.0, 14.0), 3.0 * PI / 4.0),
        CorpseAnim::Ledge => (Vec2::new(9.0, 12.0), PI / 4.0),
    }
}

fn pose_cushion(w: &mut World, id: EntityId, cp: &mut Corpse) {
    let (anchor, rot) = cushion_pose(cp.anim);
    let mut off = anchor - Vec2::new(6.0, 12.0);
    let mut rot = rot;
    if cp.facing < 0 {
        off.x = -off.x;
        rot = PI - rot;
    }
    let base = w.core(id).pos + off;
    for i in 0..CUSHION_CAP {
        let Some(a) = cp.cushion.arrows[i] else {
            continue;
        };
        let keep = matches!(w.arrow(a), Some(ar) if ar.state == arrow::ArrowState::Buried && ar.buried_in == Some(id));
        if !keep || w.core(a).marked_for_removal {
            cp.cushion.arrows[i] = None;
            continue;
        }
        w.core_mut(a).pos = base;
        let r = rot + cp.cushion.rot[i];
        if let Some(ar) = w.arrow_mut(a) {
            ar.direction = r;
        }
    }
}
