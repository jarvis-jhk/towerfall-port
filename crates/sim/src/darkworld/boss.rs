//! `DarkWorldBoss` (darkworld.md §7): the shared base of the four bosses -
//! health and the boss pool, stomp damage with the 5-tick double-stomp
//! rule, rapid-hit pitch, the automatic midway treasure, the standard
//! intro, level shakes - plus the dispatch to each boss module. The boss
//! state lives in `DarkWorld::boss`; the boss body is a `Kind::Dw` entity
//! with `Part::Body`.

use super::{Part, amaranth, cataclysm, cyclops, dreadwood};
use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox};
use crate::content::Content;
use crate::entity::{Core, EntityId, Kind, Tag};
use crate::math::Vec2;
use crate::player::{self, Player};
use crate::world::World;

/// A coroutine flattened into a step number and a wait (`wait(N)` resumes
/// after N + 1 ticks, darkworld.md conventions).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Seq {
    pub on: bool,
    pub step: u16,
    pub wait: f32,
    /// Generic loop counter of the coroutine.
    pub k: i32,
}

impl Seq {
    pub fn start(step: u16) -> Seq {
        Seq {
            on: true,
            step,
            wait: 0.0,
            k: 0,
        }
    }
    /// Tick the wait; true when the current step should run now.
    pub fn ready(&mut self, tm: f32) -> bool {
        if !self.on {
            return false;
        }
        if self.wait > 0.0 {
            self.wait -= tm;
            if self.wait > 0.0 {
                return false;
            }
        }
        true
    }
    /// `wait(n)` then continue at `next`.
    pub fn wait(&mut self, n: f32, next: u16) {
        self.wait = n + 1.0;
        self.step = next;
    }
    /// Continue at `next` on the next tick.
    pub fn next(&mut self, next: u16) {
        self.wait = 1.0;
        self.step = next;
    }
    pub fn stop(&mut self) {
        self.on = false;
    }
}

impl StableHash for Seq {
    fn stable_hash(&self, h: &mut Hasher) {
        h.bool(self.on);
        h.u32(self.step as u32);
        h.f32(self.wait);
        h.i32(self.k);
    }
}

/// The base-class state of a boss.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Base {
    pub health: i32,
    pub max_health: i32,
    pub stomp_damage: i32,
    /// Ticks since the last stomp (a second stomp within 5 deals 0).
    pub stomp_t: f32,
    /// Rapid-hit sound pitch index (0..5) and its 40-tick window.
    pub pitch: u8,
    pub pitch_t: f32,
    /// Automatic midway treasure at 2/3 health.
    pub midway_auto: bool,
    pub midway_done: bool,
    pub flash: f32,
    pub difficulty: i32,
    pub players: i32,
}

impl StableHash for Base {
    fn stable_hash(&self, h: &mut Hasher) {
        h.i32(self.health);
        h.i32(self.max_health);
        h.f32(self.stomp_t);
        h.u8(self.pitch);
        h.f32(self.pitch_t);
        h.bool(self.midway_done);
        h.f32(self.flash);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BossKind {
    Amaranth(amaranth::State),
    Dreadwood(dreadwood::State),
    Cyclops(cyclops::State),
    Cataclysm(cataclysm::State),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BossState {
    pub id: u8,
    pub body: EntityId,
    pub base: Base,
    pub kind: BossKind,
}

impl StableHash for BossState {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.id);
        self.body.stable_hash(h);
        self.base.stable_hash(h);
        match &self.kind {
            BossKind::Amaranth(s) => s.stable_hash(h),
            BossKind::Dreadwood(s) => s.stable_hash(h),
            BossKind::Cyclops(s) => s.stable_hash(h),
            BossKind::Cataclysm(s) => s.stable_hash(h),
        }
    }
}

/// Add the boss body entity (an Enemy-tagged boss, bounty 0).
pub fn add_body(
    w: &mut World,
    pos: Vec2,
    hb: Hitbox,
    target: Option<Hitbox>,
    depth: i32,
    tags: &[Tag],
) -> EntityId {
    let mut core = Core::new(pos, Collider::Box(hb), depth, tags);
    core.screen_wrap = false;
    core.ignore_jump_thrus = true;
    core.target_collider = target;
    core.seek = true;
    core.seek_priority = 50;
    core.allegiance = crate::entity::Allegiance::Red;
    w.add(core, Kind::Dw(Part::Body))
}

/// Construct boss `id` for the current level (darkworld.md §5.1).
pub fn spawn(w: &mut World, c: &Content, id: u8, difficulty: u8) {
    let (_, players) = super::params(w);
    let base = Base {
        difficulty: difficulty as i32,
        players,
        stomp_damage: 2,
        midway_auto: true,
        ..Default::default()
    };
    let state = match id {
        0 => amaranth::spawn(w, c, base),
        1 => dreadwood::spawn(w, c, base),
        2 => cyclops::spawn(w, c, base),
        _ => cataclysm::spawn(w, c, base),
    };
    if let Some(d) = w.dark_world.as_mut() {
        d.boss = Some(Box::new(state));
    }
}

/// Run `f` on the boss state (taken out of the world while it runs).
pub fn with_boss<R>(w: &mut World, f: impl FnOnce(&mut World, &mut BossState) -> R) -> Option<R> {
    let mut b = w.dark_world.as_mut()?.boss.take()?;
    let r = f(w, &mut b);
    if let Some(d) = w.dark_world.as_mut()
        && d.boss.is_none()
    {
        d.boss = Some(b);
    }
    Some(r)
}

/// Kept for the session hook (bosses run from their body's update).
pub fn update_state(_w: &mut World, _c: &Content) {}

/// The boss body's entity update.
pub fn update_body(w: &mut World, c: &Content, id: EntityId) {
    with_boss(w, |w, b| {
        if b.body != id {
            return;
        }
        let tm = w.time.time_mult;
        b.base.stomp_t += tm;
        if b.base.pitch_t > 0.0 {
            b.base.pitch_t -= tm;
            if b.base.pitch_t <= 0.0 {
                b.base.pitch = 0;
            }
        }
        if b.base.flash > 0.0 {
            b.base.flash -= tm;
        }
        let base = &mut b.base;
        match &mut b.kind {
            BossKind::Amaranth(s) => amaranth::update(w, c, id, s, base),
            BossKind::Dreadwood(s) => dreadwood::update(w, c, id, s, base),
            BossKind::Cyclops(s) => cyclops::update(w, c, id, s, base),
            BossKind::Cataclysm(s) => cataclysm::update(w, c, id, s, base),
        }
    });
}

/// `DarkWorldBoss.Hurt`: apply `damage` (health actually lost goes off the
/// pool; a boss hit for a real killer), the hit pitch, the midway signal.
/// Returns the health lost.
pub fn hurt(w: &mut World, base: &mut Base, damage: i32, killer: i8) -> i32 {
    let before = base.health;
    base.health = (base.health - damage).max(0);
    let lost = before - base.health;
    // Every hit by a player counts in `BossHits`, even one that took no
    // health (the 0-damage stomp).
    super::boss_damaged(w, lost, killer);
    if lost > 0 {
        base.pitch = (base.pitch + 1).min(5);
        base.pitch_t = 40.0;
    }
    if base.midway_auto && !base.midway_done && base.health <= base.max_health * 2 / 3 {
        base.midway_done = true;
        super::signal_boss_treasure(w);
    }
    lost
}

/// Stomp damage with the double-stomp rule.
pub fn stomp_damage(base: &mut Base) -> i32 {
    let d = if base.stomp_t < 5.0 {
        0
    } else {
        base.stomp_damage
    };
    base.stomp_t = 0.0;
    d
}

/// The player's feet were above the body's top last tick (stomp test,
/// 6 px, as for enemies).
pub fn is_stomp(w: &World, body: EntityId, pid: EntityId, p: &Player) -> bool {
    let top = w.core(body).top();
    let pc = w.core(pid);
    p.speed.y >= 0.0 && pc.bottom() - p.speed.y <= top + 6.0
}

/// Bounce a stomping player off the body.
pub fn bounce(w: &mut World, c: &Content, body: EntityId, pid: EntityId, p: &mut Player) {
    let top = w.core(body).top();
    player::bounce_pub(w, c, pid, p, top, false);
}

/// `ShakeLevel(t)`: screen shake and arrows stuck high up fall.
pub fn shake_level(w: &mut World, t: f32) {
    w.screen_shake(t);
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    for a in arrows {
        let high = w.core(a).pos.y < 120.0;
        if let Some(ar) = w.arrow(a).copied()
            && high
            && ar.state == crate::arrow::ArrowState::Stuck
        {
            crate::arrows::with_arrow(w, a, |w, ar| {
                crate::arrow::enter_fall_mode(w, a, ar, false, false, false)
            });
        }
    }
}

/// Living players (id, position).
pub fn living(w: &World) -> Vec<(EntityId, Vec2)> {
    w.players()
        .filter_map(|id| {
            w.player(id)
                .filter(|p| !p.dead)
                .map(|_| (id, w.core(id).pos))
        })
        .collect()
}

/// A random living player.
pub fn random_target(w: &mut World) -> Option<(EntityId, Vec2)> {
    let l = living(w);
    if l.is_empty() {
        return None;
    }
    let k = w.rng.choose_index(l.len());
    Some(l[k])
}

/// Hurt a player (boss contact / projectile; cause Enemy).
pub fn hurt_player(w: &mut World, c: &Content, pid: EntityId, p: &mut Player, from: Vec2) {
    player::hurt(
        w,
        c,
        pid,
        p,
        crate::player::DeathCause::Enemy,
        from,
        -1,
        None,
    );
}

/// Hurt a player by id (outside its own update).
pub fn hurt_player_id(w: &mut World, c: &Content, pid: EntityId, from: Vec2) {
    crate::hurt::with_player(w, pid, |w, p| hurt_player(w, c, pid, p, from));
}

/// The standard intro's boss-mode start: the pool is every boss's health.
pub fn enter_boss_mode(w: &mut World, max: i32) {
    super::enter_boss_mode(w, max);
}

/// Health of the shared pool (Dreadwood).
pub fn pool(w: &World) -> i32 {
    w.dark_world.as_ref().map_or(0, |d| d.boss_health)
}
