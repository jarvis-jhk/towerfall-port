//! The archer (`docs/spec/player.md`). Section numbers in comments refer
//! to that spec. Formulas multiply per-frame quantities by `TimeMult` where
//! the original does, which is exact (×1.0) outside slow motion.

use crate::actor::{self, ExactCollide, SquishDir};
use crate::arrow::{self, Arrow, Shooter};
use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox, Placed};
use crate::components::{Scheduler, SpriteAnim, StateMachine};
use crate::content::{BodyAnim, Content};
use crate::corpse;
use crate::entity::{Allegiance, Core, EntityId, Kind, Tag, depth};
use crate::events::{CharSound, Event, ParticleKind};
use crate::hat;
use crate::impl_hash_enum;
use crate::input::InputState;
use crate::math::{
    PI, Vec2, abs_angle_diff, angle_between, angle_to_vector, approach, clamp, eight_way_normal,
    floor, lerp, min, pow, sign, sign_i,
};
use crate::quiver::{ArrowType, Quiver};
use crate::world::World;

// ------------------------------------------------------------ constants §1

pub const GRAVITY: f32 = 0.3;
pub const MAX_FALL: f32 = 2.8;
pub const FAST_FALL: f32 = 3.5;
pub const JUMP: f32 = -3.2;
pub const JUMP_ONPAD: f32 = -4.3;
pub const MAX_RUN: f32 = 1.5;
pub const DODGE_SPEED: f32 = 5.5;
pub const DODGE_MULT: f32 = 0.85;
pub const WINGS_JUMP: f32 = -1.4;
pub const AUTO_LOCK_DIST_SQ: f32 = 1296.0;
pub const AUTO_LOCK_MAX_ANGLE: f32 = 13.0 * PI / 36.0;
pub const HYPER_THRESHOLD: f32 = 3.6;

// ----------------------------------------------------------- hitboxes §3.2

pub const NORMAL_HITBOX: Hitbox = Hitbox::wrapping(8.0, 14.0, -4.0, -6.0);
pub const DUCKING_HITBOX: Hitbox = Hitbox::wrapping(8.0, 6.0, -4.0, 2.0);
pub const SHIELD_HITBOX: Hitbox = Hitbox::wrapping(16.0, 18.0, -8.0, -10.0);
pub const PICKUP_HITBOX: Hitbox = Hitbox::wrapping(22.0, 30.0, -11.0, -16.0);
pub const CATCH_HITBOX: Hitbox = Hitbox::wrapping(12.0, 16.0, -6.0, -8.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PState {
    Normal = 0,
    LedgeGrab = 1,
    Ducking = 2,
    Dodging = 3,
    Dying = 4,
    Frozen = 5,
}
impl_hash_enum!(PState);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HatState {
    NoHat,
    #[default]
    Normal,
    Crown,
}
impl_hash_enum!(HatState);

/// `DeathCause` (§13.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathCause {
    Arrow = 0,
    Explosion,
    Brambles,
    JumpedOn,
    Lava,
    Shock,
    SpikeBall,
    FallingObject,
    Squish,
    Curse,
    Miasma,
    Enemy,
    Chalice,
}

/// Scheduler actions of the archer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PAction {
    #[default]
    FinishAutoMove,
    EndDodgeCooldown,
    GrantShield,
    GrantWings,
    GrantInvisible,
}
impl_hash_enum!(PAction);

/// Per-archer state (§23).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub index: u8,
    pub archer: u16,
    pub allegiance: Allegiance,
    pub team_color: Allegiance,
    pub sm: StateMachine<PState>,
    pub speed: Vec2,
    pub facing: i32,
    pub aim_direction: f32,
    pub aiming: bool,
    pub start_aiming_down: bool,
    pub last_aim_direction: Option<f32>,
    pub on_ground: bool,
    pub cling: i32,
    pub dodge_sliding: bool,
    pub was_dodge_sliding: bool,
    pub ducking_hitbox: bool,
    pub hat: HatState,
    pub has_shield: bool,
    pub has_wings: bool,
    pub speed_boots: bool,
    pub invisible: bool,
    pub invis_opacity: f32,
    pub arrows: Quiver,
    pub slippery_control: f32,
    pub wall_stick_max: f32,
    pub flap_gravity: f32,
    pub auto_move: i32,
    pub auto_bounce: bool,
    pub can_var_jump: bool,
    pub can_double_jump: bool,
    pub can_hyper: bool,
    pub did_detonate: bool,
    pub did_pop: bool,
    pub grace_ledge_dir: i32,
    pub last_caught: Option<EntityId>,
    pub death_arrow: Option<EntityId>,
    pub last_platform: Option<EntityId>,
    pub dodge_speed: Vec2,
    pub dodge_curse_satisfied: bool,
    pub dodge_cooldown: bool,
    pub duck_slip_counter: f32,
    pub jump_grace: f32,
    pub jump_buffer: f32,
    pub dodge_catch: f32,
    pub dodge_end: f32,
    pub dodge_stall: f32,
    pub flap_bounce: f32,
    pub dying: f32,
    pub spam_shot: f32,
    pub arrow_regen: f32,
    /// The round's start-arrow type (RegeneratingArrows refills it).
    pub start_arrow: ArrowType,
    pub shield_regen: f32,
    pub can_detonate: f32,
    pub scheduler: Scheduler<PAction, 8>,
    pub dead: bool,
    pub gliding: bool,
    pub move_x: i32,
    pub move_y: i32,
    pub input: InputState,
    pub hyper_dir: i32,
    pub hyper_start_x: f32,
    pub can_pad_particles: bool,
    // ---- presentation-facing (not in the checksum)
    pub scale: Vec2,
    pub light_alpha: f32,
    pub body: SpriteAnim,
    pub head: SpriteAnim,
    pub bow: SpriteAnim,
    pub bow_visible: bool,
    pub head_visible: bool,
    pub wall_slide_sound: bool,
    // ---- M5 (arrows.md, pickups.md)
    /// Registered trigger arrows (`triggerArrows`).
    pub triggers: [Option<EntityId>; TRIGGER_CAP],
    /// The prism holding this archer.
    pub prism: Option<EntityId>,
    /// Quiver HUD `showCounter` (presentation).
    pub hud_counter: f32,
}

impl StableHash for Player {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.index);
        h.u32(self.archer as u32);
        self.allegiance.stable_hash(h);
        self.team_color.stable_hash(h);
        self.sm.state.stable_hash(h);
        self.sm.previous.stable_hash(h);
        h.vec2(self.speed);
        h.i32(self.facing);
        h.f32(self.aim_direction);
        let flags = [
            self.aiming,
            self.start_aiming_down,
            self.on_ground,
            self.dodge_sliding,
            self.was_dodge_sliding,
            self.ducking_hitbox,
            self.has_shield,
            self.has_wings,
            self.speed_boots,
            self.invisible,
            self.auto_bounce,
            self.can_var_jump,
            self.can_double_jump,
            self.can_hyper,
            self.did_detonate,
            self.did_pop,
            self.dodge_curse_satisfied,
            self.dodge_cooldown,
            self.dead,
            self.gliding,
            self.can_pad_particles,
        ];
        h.u32(flags.iter().fold(0u32, |m, &b| (m << 1) | b as u32));
        self.last_aim_direction.stable_hash(h);
        h.i32(self.cling);
        self.hat.stable_hash(h);
        h.f32(self.invis_opacity);
        self.arrows.stable_hash(h);
        for v in [
            self.slippery_control,
            self.wall_stick_max,
            self.flap_gravity,
            self.duck_slip_counter,
            self.jump_grace,
            self.jump_buffer,
            self.dodge_catch,
            self.dodge_end,
            self.dodge_stall,
            self.flap_bounce,
            self.dying,
            self.spam_shot,
            self.arrow_regen,
            self.start_arrow as u8 as f32,
            self.shield_regen,
            self.can_detonate,
            self.hyper_start_x,
        ] {
            h.f32(v);
        }
        h.i32(self.auto_move);
        h.i32(self.grace_ledge_dir);
        h.i32(self.hyper_dir);
        h.i32(self.move_x);
        h.i32(self.move_y);
        self.last_caught.stable_hash(h);
        self.death_arrow.stable_hash(h);
        self.last_platform.stable_hash(h);
        h.vec2(self.dodge_speed);
        self.scheduler.stable_hash(h);
        self.input.stable_hash(h);
        self.triggers.stable_hash(h);
        self.prism.stable_hash(h);
    }
}

impl Player {
    pub fn state(&self) -> PState {
        self.sm.state
    }
    pub fn state_is_frozen(&self) -> bool {
        self.sm.state == PState::Frozen
    }
    /// `IsHyper`.
    pub fn is_hyper(&self) -> bool {
        self.can_hyper
            && self.sm.state == PState::Normal
            && self.speed.length_sq() >= HYPER_THRESHOLD
    }
    /// `IsEnemy(x)`.
    pub fn is_enemy(&self, other_allegiance: Allegiance) -> bool {
        self.allegiance == Allegiance::Neutral || other_allegiance != self.allegiance
    }
    /// Trigger arrows currently armed by this archer.
    pub fn trigger_count(&self) -> usize {
        self.triggers.iter().flatten().count()
    }
    /// `RemoveTriggerArrow(arrow)`.
    pub fn remove_trigger(&mut self, a: EntityId) {
        for t in &mut self.triggers {
            if *t == Some(a) {
                *t = None;
            }
        }
    }
    fn input_ducking(&self) -> bool {
        self.on_ground && self.move_x == 0 && self.move_y == 1
    }
}

// ------------------------------------------------------------ setup §3

/// Create the archers of the match setup at their spawn points.
pub fn spawn_players(w: &mut World, c: &Content) {
    let lvl = c.level(w.level);
    let mut spawns = lvl.player_spawns.clone();
    w.rng.shuffle(&mut spawns);
    let participants: Vec<usize> = (0..w.setup.players.len())
        .filter(|&i| w.setup.players[i].is_some())
        .collect();
    let width = lvl.width as f32;
    let mut positions: Vec<Vec2> = vec![];
    for (k, _) in participants.iter().enumerate() {
        let p = if spawns.is_empty() {
            Vec2::new(width / 2.0, 0.0)
        } else {
            spawns[k % spawns.len()]
        };
        positions.push(p);
    }
    // Two players: the second takes the mirror of the first if it exists.
    if participants.len() == 2 && !spawns.is_empty() && positions[0].x != 160.0 {
        let m = Vec2::new(width - positions[0].x, positions[0].y);
        if spawns.contains(&m) {
            positions[1] = m;
        }
    }
    for (k, &slot) in participants.iter().enumerate() {
        let ps = w.setup.players[slot].unwrap();
        let pos = ps.spawn.unwrap_or(positions[k] + Vec2::new(0.0, 2.0));
        create(w, c, slot as u8, pos, &ps);
    }
}

/// `new Player(...)` (§3.1).
pub fn create(
    w: &mut World,
    c: &Content,
    index: u8,
    pos: Vec2,
    ps: &crate::world::PlayerSetup,
) -> EntityId {
    let mut core = Core::new(
        pos,
        Collider::Box(NORMAL_HITBOX),
        depth::PLAYER,
        &[
            Tag::Player,
            Tag::ExplosionCollider,
            Tag::LightSource,
            Tag::Target,
            Tag::Actor,
        ],
    );
    core.screen_wrap = true;
    core.seek = true;
    core.seek_priority = 100;
    core.allegiance = ps.allegiance;
    let frozen = w.setup.start_frozen;
    if frozen {
        core.flash.start();
    }
    let archer = (ps.archer as usize).min(c.archers.len().saturating_sub(1)) as u16;
    let mut hat = ps.hat;
    if hat == HatState::Normal
        && c.archers
            .get(archer as usize)
            .is_some_and(|a| a.start_no_hat)
    {
        hat = HatState::NoHat;
    }
    let mut p = Player {
        index,
        archer,
        allegiance: ps.allegiance,
        team_color: ps.team_color,
        sm: StateMachine::new(if frozen {
            PState::Frozen
        } else {
            PState::Normal
        }),
        speed: Vec2::ZERO,
        facing: 1,
        aim_direction: 0.0,
        aiming: false,
        start_aiming_down: false,
        last_aim_direction: None,
        on_ground: false,
        cling: 0,
        dodge_sliding: false,
        was_dodge_sliding: false,
        ducking_hitbox: false,
        hat,
        has_shield: false,
        has_wings: false,
        speed_boots: ps.speed_boots,
        invisible: false,
        invis_opacity: 1.0,
        arrows: Quiver::new(ps.arrows as usize, ArrowType::Normal),
        slippery_control: 1.0,
        wall_stick_max: 0.5,
        flap_gravity: 1.0,
        auto_move: 0,
        auto_bounce: false,
        can_var_jump: false,
        can_double_jump: false,
        can_hyper: false,
        did_detonate: false,
        did_pop: false,
        grace_ledge_dir: 0,
        last_caught: None,
        death_arrow: None,
        last_platform: None,
        dodge_speed: Vec2::ZERO,
        dodge_curse_satisfied: false,
        dodge_cooldown: false,
        duck_slip_counter: 0.0,
        jump_grace: 0.0,
        jump_buffer: 0.0,
        dodge_catch: 0.0,
        dodge_end: 0.0,
        dodge_stall: 0.0,
        flap_bounce: 0.0,
        dying: 0.0,
        spam_shot: 0.0,
        arrow_regen: 90.0,
        start_arrow: ArrowType::Normal,
        shield_regen: 240.0,
        can_detonate: 0.0,
        scheduler: Scheduler::default(),
        dead: false,
        gliding: false,
        move_x: 0,
        move_y: 0,
        input: InputState::default(),
        hyper_dir: 0,
        hyper_start_x: 0.0,
        can_pad_particles: false,
        scale: Vec2::ONE,
        light_alpha: 1.0,
        body: SpriteAnim::default(),
        head: SpriteAnim::default(),
        bow: SpriteAnim::default(),
        bow_visible: true,
        head_visible: true,
        wall_slide_sound: false,
        triggers: [None; TRIGGER_CAP],
        prism: None,
        hud_counter: if w
            .setup
            .variant_set
            .get(crate::variants::Variant::SecretArrows, index as i32)
        {
            90.0
        } else {
            0.0
        },
    };
    // §16: initial grants on the scheduler.
    if ps.shield {
        p.scheduler
            .schedule(PAction::GrantShield, 20.0 + 10.0 * index as f32, false);
    }
    if ps.wings {
        p.scheduler
            .schedule(PAction::GrantWings, 40.0 + 10.0 * index as f32, false);
    }
    if ps.invisible {
        p.scheduler.schedule(PAction::GrantInvisible, 60.0, false);
    }
    let index = p.index;
    let id = w.add(core, Kind::Player(p));
    crate::variant_fx::player_added(w, id, index);
    id
}

/// `Added()` / `SceneBegin()` (§3.3).
pub fn added(w: &mut World, c: &Content, id: EntityId) {
    let Some(Kind::Player(mut p)) = w.take_kind(id) else {
        return;
    };
    let half = w.world_size(c).w / 2.0;
    let pos = w.core(id).pos;
    p.facing = if pos.x > half { -1 } else { 1 };
    let ahead = w.check_at(
        c,
        id,
        Vec2::new(pos.x + 12.0 * p.facing as f32, pos.y),
        Tag::Solid,
    );
    let behind = w.check_at(
        c,
        id,
        Vec2::new(pos.x - 12.0 * p.facing as f32, pos.y),
        Tag::Solid,
    );
    if ahead && !behind {
        p.facing = -p.facing;
    }
    p.aim_direction = if p.facing > 0 { 0.0 } else { PI };
    // §3.3 step 8: quiver capacity (NoQuivers 1, SmallQuivers 3), for every
    // spawn (rounds, revives, Dark World levels).
    let pi = p.index as i32;
    if crate::pickups::variant(w, crate::variants::Variant::NoQuivers, pi) {
        p.arrows.set_max(1);
    } else if crate::pickups::variant(w, crate::variants::Variant::SmallQuivers, pi) {
        p.arrows.set_max(3);
    }
    if p.state() == PState::Frozen {
        p.on_ground = actor::get_below(w, c, id).is_some();
        update_animation(w, c, id, &mut p);
    }
    w.put_kind(id, Kind::Player(p));
    spawned_inside_solid_check(w, c, id);
}

fn spawned_inside_solid_check(w: &mut World, c: &Content, id: EntityId) {
    let Some(s) = w.first_at(c, id, w.core(id).pos, Tag::Solid) else {
        return;
    };
    let orig = w.core(id).pos;
    let sc = *w.core(s);
    let free = |w: &World| !w.check_at(c, id, w.core(id).pos, Tag::Solid);
    w.core_mut(id).set_bottom(sc.top());
    if free(w) {
        return;
    }
    w.core_mut(id).pos = orig;
    w.core_mut(id).set_top(sc.bottom());
    if free(w) {
        return;
    }
    w.core_mut(id).pos = orig;
    w.core_mut(id).set_left(sc.right());
    if free(w) {
        return;
    }
    w.core_mut(id).set_right(sc.left());
    if free(w) {
        return;
    }
    w.core_mut(id).pos = orig;
    w.core_mut(id).set_bottom(sc.top());
    if let Some(Kind::Player(mut p)) = w.take_kind(id) {
        die(w, c, id, &mut p, DeathCause::Squish, -1);
        w.put_kind(id, Kind::Player(p));
    }
}

/// Round start: stop flashing and unfreeze every archer.
pub fn unfreeze_all(w: &mut World, c: &Content) {
    let ids: Vec<EntityId> = w.players().collect();
    for id in ids {
        w.core_mut(id).flash.stop();
        if let Some(Kind::Player(mut p)) = w.take_kind(id) {
            if p.state() == PState::Frozen {
                set_state(w, c, id, &mut p, PState::Normal);
            }
            w.put_kind(id, Kind::Player(p));
        }
    }
}

// ------------------------------------------------------------ helpers

fn tm(w: &World) -> f32 {
    w.time.time_mult
}

fn csound(w: &mut World, p: &Player, s: CharSound, x: f32) {
    w.char_sound(s, p.index, p.archer, x);
}

fn rumble(w: &mut World, p: &Player, strength: f32, frames: u16) {
    w.emit(Event::Rumble {
        player: p.index,
        strength,
        frames,
    });
}

fn particles(w: &mut World, p: &Player, kind: ParticleKind, pos: Vec2, count: u8, dir: f32) {
    w.emit(Event::Particles {
        kind,
        player: p.index,
        pos,
        count,
        dir,
    });
}

fn pos(w: &World, id: EntityId) -> Vec2 {
    w.core(id).pos
}

fn solid_at(w: &World, c: &Content, id: EntityId, at: Vec2) -> bool {
    w.check_at(c, id, at, Tag::Solid)
}

/// `ApplyWrap` of a point.
fn wrap_point(w: &World, c: &Content, p: Vec2) -> Vec2 {
    let ws = w.world_size(c);
    let x = (if p.x < 0.0 { p.x + ws.w } else { p.x }) % ws.w;
    let y = (if p.y < 0.0 { p.y + ws.h } else { p.y }) % ws.h;
    Vec2::new(x, y)
}

fn use_normal_hitbox(w: &mut World, id: EntityId, p: &mut Player) {
    if !p.ducking_hitbox {
        return;
    }
    p.ducking_hitbox = false;
    let core = w.core_mut(id);
    core.collider = Collider::Box(NORMAL_HITBOX);
    core.seek_offset = Vec2::ZERO;
}

fn use_ducking_hitbox(w: &mut World, id: EntityId, p: &mut Player) {
    if p.ducking_hitbox {
        return;
    }
    p.ducking_hitbox = true;
    let core = w.core_mut(id);
    core.collider = Collider::Box(DUCKING_HITBOX);
    core.seek_offset = Vec2::new(0.0, 5.0);
}

/// `CanUnduck()`: the normal hitbox at the current position is free.
fn can_unduck(w: &World, c: &Content, id: EntityId) -> bool {
    let p = Placed {
        pos: pos(w, id),
        collider: Collider::Box(NORMAL_HITBOX),
    };
    !w.any(c, &p, Tag::Solid, id)
}

fn set_aiming(w: &mut World, id: EntityId, p: &mut Player, on: bool) {
    if on {
        if !p.aiming {
            p.last_aim_direction = None;
            let x = pos(w, id).x;
            csound(w, p, CharSound::Aim, x);
        }
    } else {
        p.start_aiming_down = false;
    }
    p.aiming = on;
}

fn stop_wall_slide_sound(w: &mut World, p: &mut Player, x: f32) {
    if p.wall_slide_sound {
        p.wall_slide_sound = false;
        w.emit(Event::Loop {
            name: "WALLSLIDE_LOOP",
            player: p.index,
            on: false,
            x,
        });
    }
}

/// `HasShield = true`.
pub fn gain_shield(w: &mut World, id: EntityId, p: &mut Player) {
    p.has_shield = true;
    w.core_mut(id).target_collider = Some(SHIELD_HITBOX);
    w.sound("pu_shield", pos(w, id).x);
}

/// `HasShield = false` (§16.1).
pub fn lose_shield(w: &mut World, id: EntityId, p: &mut Player) {
    p.has_shield = false;
    w.core_mut(id).target_collider = None;
    let at = pos(w, id);
    particles(w, p, ParticleKind::ShieldBreak, at, 24, 0.0);
    w.sound("pu_shieldImp", at.x);
    w.core_mut(id)
        .flash
        .start_for(30.0, crate::entity::FlashFinish::None);
    rumble(w, p, 0.5, 20);
}

// ------------------------------------------------------------ state machine

pub(crate) fn set_state(w: &mut World, c: &Content, id: EntityId, p: &mut Player, s: PState) {
    let Some((old, new)) = p.sm.set(s) else {
        return;
    };
    match old {
        PState::Ducking => leave_ducking(w, id, p),
        PState::Dodging => leave_dodge(w, c, id, p),
        PState::Dying => w.core_mut(id).collidable = true,
        _ => {}
    }
    match new {
        PState::LedgeGrab => enter_ledge_grab(w, id, p),
        PState::Ducking => enter_ducking(w, c, id, p),
        PState::Dodging => enter_dodge(w, c, id, p, old),
        PState::Dying => {
            p.dying = 3.0;
            w.core_mut(id).collidable = false;
        }
        _ => {}
    }
}

// ------------------------------------------------------------ update §4

pub fn update(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    if p.dead {
        return;
    }
    // EnableSolids / DisableSolids: the bars of this archer's prism.
    let prism = p.prism;
    if let Some(pr) = prism {
        crate::arrows::prism::set_bars(w, pr, true);
    }
    update_inner(w, c, id, p);
    if let Some(pr) = prism {
        crate::arrows::prism::set_bars(w, pr, false);
    }
}

fn update_inner(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    let t = tm(w);
    if p.hud_counter > 0.0 {
        p.hud_counter -= t;
    }
    // Trigger arrows that vanished unlink themselves (`Removed`).
    for k in 0..p.triggers.len() {
        if let Some(a) = p.triggers[k] {
            let linked = matches!(w.arrow(a), Some(ar) if ar.extra.detonator == Some(id))
                && !w.core(a).marked_for_removal;
            if !linked {
                p.triggers[k] = None;
            }
        }
    }
    let v = w.variants_for(p.index as i32);
    // 1.
    if p.spam_shot > 0.0 {
        p.spam_shot -= t;
    }
    // 2. FramesAlive.
    crate::versus::stat(w, p.index, |s| s.frames_alive += t);
    // 3. EnableSolids: no player-only solids exist yet.
    let frozen = p.state() == PState::Frozen;
    // 4. RegeneratingArrows
    if v.regenerating_arrows && !frozen {
        if !p.arrows.is_empty() {
            p.arrow_regen = 90.0;
        } else {
            p.arrow_regen -= t;
            if p.arrow_regen <= 0.0 {
                w.sound("sfx_regenArrow", pos(w, id).x);
                p.arrows.add_arrows(&[p.start_arrow]);
            }
        }
    }
    // 5. RegeneratingShields
    if v.regenerating_shields && !frozen {
        if p.has_shield {
            p.shield_regen = 240.0;
        } else {
            p.shield_regen -= t;
            if p.shield_regen <= 0.0 {
                gain_shield(w, id, p);
            }
        }
    }
    // 6. invisibility opacity
    if p.invisible {
        if p.aiming {
            p.invis_opacity = 1.0;
        } else {
            let target = invisible_target(w, c, p);
            p.invis_opacity = (p.invis_opacity - 0.02 * t).max(target);
        }
    } else {
        p.invis_opacity = approach(p.invis_opacity, 1.0, 0.05 * t);
    }
    // 7. body squash (cosmetic)
    if p.state() != PState::Dodging {
        let target = if p.speed.y.abs() > MAX_FALL {
            Vec2::new(0.8, 1.2)
        } else {
            Vec2::ONE
        };
        p.scale.x = approach(p.scale.x, target.x, 0.04 * t);
        p.scale.y = approach(p.scale.y, target.y, 0.04 * t);
    }
    // 8. light
    let la = if p.state() == PState::Ducking {
        0.0
    } else {
        1.0
    };
    p.light_alpha = approach(p.light_alpha, la, 0.05 * t);
    // 9. ground
    let below = actor::get_below(w, c, id);
    p.on_ground = below.is_some();
    if p.on_ground {
        let ice = !v.no_slipping && crate::entities::ice_under(w, c, id);
        p.slippery_control = if v.slippery_floors || ice { 0.0 } else { 1.0 };
        p.last_platform = below;
    } else {
        p.slippery_control = min(1.0, p.slippery_control + 0.1 * t);
    }
    // 10, 12. hot coals, mud (tower-entities.md §7)
    crate::entities::player_mud_tracking(w, c, id, p);
    // 11. input
    p.input = w.inputs[p.index as usize].state;
    p.move_x = p.input.move_x;
    p.move_y = p.input.move_y;
    p.aim_direction = p.input.aim_direction(p.facing, v.free_aiming);
    // 13. hyper bookkeeping
    if p.is_hyper() {
        let at = pos(w, id);
        particles(w, p, ParticleKind::Hyper, at, 2, 0.0);
    } else if p.hyper_dir != 0 {
        p.hyper_dir = 0;
    }
    // 14.
    if v.double_jumping && (p.on_ground || p.state() == PState::LedgeGrab) {
        p.can_double_jump = true;
    }
    // 15. frozen
    if frozen {
        base_update(w, c, id, p);
        if p.move_x != 0 {
            p.facing = p.move_x;
        }
        update_animation(w, c, id, p);
        return;
    }
    // 16.
    p.jump_buffer -= t;
    if p.input.jump_pressed {
        p.jump_buffer = 6.0;
    }
    // 17.
    if p.aiming {
        if let Some(last) = p.last_aim_direction
            && last != p.aim_direction
        {
            let x = pos(w, id).x;
            csound(w, p, CharSound::AimDir, x);
        }
        p.last_aim_direction = Some(p.aim_direction);
    }
    // 18.
    if p.auto_move != 0 {
        p.move_x = p.auto_move;
    }
    // 19.
    if p.can_detonate > 0.0 {
        p.can_detonate -= t;
    }
    // 20. coyote
    if p.on_ground && p.speed.y > -0.02 {
        crate::components::Counter::set_max(&mut p.jump_grace, 6.0);
        p.wall_stick_max = 0.5;
        p.flap_gravity = 1.0;
        p.grace_ledge_dir = 0;
    } else {
        p.jump_grace -= t;
    }
    // 21.
    if p.flap_bounce > 0.0 {
        if p.speed.y <= 0.0 {
            p.flap_bounce = 0.0;
        } else {
            p.flap_bounce -= t;
        }
    }
    // 22.
    p.gliding = false;
    if p.input.arrows_pressed && p.arrows.toggle_sort() {
        p.hud_counter = 40.0;
        w.sound("sfx_arrowToggle", pos(w, id).x);
    }
    // 23. base.Update (scheduler, state machine, sprites, wrap, flash)
    base_update(w, c, id, p);
    // 24.
    if p.can_hyper && !p.is_hyper() {
        p.can_hyper = false;
    }
    // 25. overlaps
    if !p.dead {
        crate::pickups::collectible_overlaps(w, c, id, p);
    }
    if !p.dead {
        player_collider_overlaps(w, c, id, p);
    }
    if !p.dead {
        collect_arrows_overlap(w, c, id, p);
        hat_shot_off_check(w, c, id, p);
    }
    // 26. fire (wings burn off)
    if !p.dead {
        crate::entities::player_fire_update(w, id, p);
    }
    // 27.
    update_animation(w, c, id, p);
}

/// Step 23: components in add order, then wrap and flash.
fn base_update(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    let t = tm(w);
    let (fired, n) = p.scheduler.update(t);
    for &a in &fired[..n] {
        match a {
            PAction::FinishAutoMove => p.auto_move = 0,
            PAction::EndDodgeCooldown => p.dodge_cooldown = false,
            PAction::GrantShield => gain_shield(w, id, p),
            PAction::GrantWings => {
                p.has_wings = true;
                w.sound("pu_wing", pos(w, id).x);
            }
            PAction::GrantInvisible => p.invisible = true,
        }
    }
    // State machine.
    let next = match p.state() {
        PState::Normal => normal_update(w, c, id, p),
        PState::LedgeGrab => ledge_grab_update(w, c, id, p),
        PState::Ducking => ducking_update(w, c, id, p),
        PState::Dodging => dodging_update(w, c, id, p),
        PState::Dying => dying_update(w, c, id, p),
        PState::Frozen => PState::Frozen,
    };
    if !p.dead {
        set_state(w, c, id, p, next);
    }
    // Sprites.
    let dt = w.time.delta_time;
    if let Some(a) = c.archers.get(p.archer as usize) {
        if let Some(s) = c.sprite(a.body) {
            p.body.update(&s.anims, dt);
        }
        if let Some(s) = c.sprite(head_sprite(c, p)) {
            p.head.update(&s.anims, dt);
        }
        if let Some(s) = c.sprite(a.bow) {
            p.bow.update(&s.anims, dt);
        }
    }
    if p.dead {
        return;
    }
    // LevelEntity: wrap (ScreenWraps), flash.
    let before = pos(w, id);
    actor::enforce_screen_wrap(w, c, id);
    if pos(w, id) != before {
        crate::versus::stat(w, p.index, |s| s.screen_wraps += 1);
    }
    w.core_mut(id).flash.update(t);
}

// ------------------------------------------------------------ Normal §6

fn normal_update(w: &mut World, c: &Content, id: EntityId, p: &mut Player) -> PState {
    let t = tm(w);
    let v = w.variants_for(p.index as i32);
    // 6.0
    let mud = crate::entities::in_mud(w, p).is_some();
    let coals = crate::entities::on_hot_coals(w, p);
    if p.on_ground && mud && p.speed.x != 0.0 && w.time.on_interval(10) {
        w.sound("env_mudPlayerMove", pos(w, id).x);
    }
    if !coals && p.input_ducking() {
        if !p.start_aiming_down {
            return PState::Ducking;
        }
    } else if p.start_aiming_down {
        p.start_aiming_down = false;
    }
    // 6.1 horizontal
    let ctl = lerp(0.35, 1.0, p.slippery_control);
    let mut max_run = MAX_RUN;
    if v.encumbrance {
        max_run -= 0.25 * (p.arrows.len() / 2) as f32;
    }
    if p.speed_boots {
        max_run *= 1.4;
    }
    if p.on_ground && mud {
        max_run *= 0.4;
    }
    if mud && p.on_ground && p.move_x != sign_i(p.speed.x) {
        p.speed.x = approach(p.speed.x, 0.0, 0.1 * t);
    } else if (p.aiming && p.on_ground)
        || (!p.aiming && p.slippery_control == 1.0 && p.move_x != sign_i(p.speed.x))
    {
        let f = if p.has_wings {
            if p.speed.x.abs() > max_run { 0.14 } else { 0.2 }
        } else if p.on_ground {
            0.2
        } else {
            0.14
        };
        p.speed.x = approach(p.speed.x, 0.0, f * ctl * t);
    }
    if !p.aiming && p.move_x != 0 {
        if p.on_ground && p.move_x != sign_i(p.speed.x) && p.speed.x != 0.0 {
            let at = pos(w, id) + Vec2::new(0.0, 8.0);
            particles(w, p, ParticleKind::Dust, at, 2, 0.0);
        }
        if p.speed.x.abs() > max_run && sign_i(p.speed.x) == p.move_x {
            p.speed.x = approach(p.speed.x, max_run * p.move_x as f32, 0.03 * t);
        } else {
            let mut a = if p.on_ground { 0.15 } else { 0.10 };
            a *= ctl;
            if p.dodge_cooldown {
                a *= 0.8;
            }
            if p.speed_boots {
                a *= 1.4;
            }
            p.speed.x = approach(p.speed.x, max_run * p.move_x as f32, a * t);
        }
    }
    // 6.2 vertical
    if p.speed.y < JUMP {
        if p.can_pad_particles {
            let at = pos(w, id);
            particles(w, p, ParticleKind::JumpPadTrail, at, 1, 0.0);
        }
    } else {
        p.can_pad_particles = false;
    }
    p.cling = 0;
    if !p.on_ground {
        let step = if p.flap_gravity < 0.5 { 0.012 } else { 0.048 };
        p.flap_gravity = approach(p.flap_gravity, 1.0, step * t);
        if p.auto_bounce && p.speed.y > 0.0 {
            p.auto_bounce = false;
        }
        let mut g = if p.speed.y <= 1.0 && (p.input.jump_check || p.auto_bounce) && p.can_var_jump {
            0.15
        } else {
            GRAVITY
        };
        g *= p.flap_gravity;
        let mut target = MAX_FALL;
        if p.move_x != 0 && can_wall_slide(w, c, id, p, p.move_x) {
            target = p.wall_stick_max;
            p.wall_stick_max = approach(p.wall_stick_max, 1.6, 0.01 * t);
            p.cling = p.move_x;
            if p.speed.y > 0.0 && !p.wall_slide_sound {
                p.wall_slide_sound = true;
                let x = pos(w, id).x;
                w.emit(Event::Loop {
                    name: "WALLSLIDE_LOOP",
                    player: p.index,
                    on: true,
                    x,
                });
            }
            if w.time.on_interval(3) {
                let at = pos(w, id) + Vec2::new(3.0 * p.cling as f32, 0.0);
                particles(w, p, ParticleKind::Dust, at, 1, 0.0);
            }
        } else if p.input.move_y == 1 && p.speed.y > 0.0 {
            target = FAST_FALL;
            let ff = tm(w);
            crate::versus::stat(w, p.index, |s| s.fast_fall_frames += ff);
        } else if p.input.jump_check && p.has_wings && p.speed.y >= -1.0 {
            target = 0.8;
            p.gliding = true;
        }
        if p.cling == 0 || p.speed.y <= 0.0 {
            let x = pos(w, id).x;
            stop_wall_slide_sound(w, p, x);
        }
        p.speed.y = approach(p.speed.y, target, g * t);
    }
    // 6.3 dodge / jump
    if !p.dodge_cooldown && p.input.dodge_pressed && !v.no_dodging {
        if p.move_x != 0 {
            p.facing = p.move_x;
        }
        return PState::Dodging;
    }
    if coals {
        hot_coals_bounce(w, id, p);
    } else if p.input.jump_pressed || p.jump_buffer > 0.0 {
        if p.jump_grace > 0.0 {
            let mut dir = p.grace_ledge_dir;
            if p.input.move_x != dir {
                dir = 0;
            }
            jump(w, c, id, p, true, true, false, dir, false);
        } else if can_wall_jump(w, c, id, p, -1) {
            wall_jump(w, id, p, 1);
        } else if can_wall_jump(w, c, id, p, 1) {
            wall_jump(w, id, p, -1);
        } else if p.can_double_jump {
            p.can_double_jump = false;
            jump(w, c, id, p, true, false, false, 0, true);
        } else if p.has_wings && p.flap_bounce <= 0.0 {
            wings_jump(w, id, p);
        }
    }
    // 6.5 shooting (§10.2)
    if p.trigger_count() > 0 {
        if p.input.alt_shoot_pressed {
            detonate_trigger_arrows(w, c, id, p);
        }
        if p.aiming {
            if !p.input.shoot_check {
                shoot_arrow(w, c, id, p);
            }
        } else if p.input.shoot_pressed {
            set_aiming(w, id, p, true);
        }
    } else if p.aiming {
        if !p.input.alt_shoot_check && !p.input.shoot_check {
            shoot_arrow(w, c, id, p);
        }
    } else if p.did_detonate {
        if !p.input.alt_shoot_check {
            p.did_detonate = false;
        }
        if p.input.shoot_pressed {
            set_aiming(w, id, p, true);
        }
    } else if p.input.alt_shoot_pressed || p.input.shoot_pressed {
        set_aiming(w, id, p, true);
    }
    // 6.6 jump-thru pop, move, ledge
    let pop = p.speed.y < -1.0
        && p.speed.y > JUMP
        && p.input.jump_check
        && w.any(c, &w.placed(id), Tag::JumpThru, id);
    if pop && !p.did_pop {
        w.sound("env_gplatPlayerJump", pos(w, id).x);
    }
    p.did_pop = pop;
    let extra = if pop { -2.0 } else { 0.0 };
    if p.move_x != 0 {
        p.facing = p.move_x;
    }
    if let Some(s) = actor::move_h(w, c, id, p.speed.x * t) {
        on_collide_h(p, s);
    }
    if let Some(s) = actor::move_v(w, c, id, p.speed.y * t + extra) {
        on_collide_v(w, c, id, p, s);
    }
    // No ledge grab while held in a prism.
    if !p.on_ground
        && !p.aiming
        && p.prism.is_none()
        && p.speed.y >= 0.0
        && p.move_x != 0
        && p.move_y <= 0
    {
        let at = pos(w, id) + Vec2::new(2.0 * p.move_x as f32, 0.0);
        if solid_at(w, c, id, at) {
            let dir = p.move_x;
            let y0 = floor(pos(w, id).y);
            for i in 0..10 {
                let ty = y0 - i as f32;
                if can_grab_ledge(w, c, id, ty, dir) {
                    return grab_ledge(w, c, id, p, ty, dir);
                }
            }
        }
    }
    PState::Normal
}

/// `CanWallJump(dir)` (§6.4).
fn can_wall_jump(w: &World, c: &Content, id: EntityId, p: &Player, dir: i32) -> bool {
    if p.has_wings {
        return false;
    }
    let at = pos(w, id);
    if solid_at(w, c, id, at + Vec2::new(0.0, 5.0)) {
        return false;
    }
    let side = wrap_point(w, c, Vec2::new(at.x + 2.0 * dir as f32, at.y));
    solid_at(w, c, id, side)
}

fn can_wall_slide(w: &World, c: &Content, id: EntityId, p: &Player, dir: i32) -> bool {
    !p.aiming && p.input.move_y != 1 && can_wall_jump(w, c, id, p, dir)
}

/// `TargetInvisibleOpacity` (player.md §4 step 6).
fn invisible_target(w: &World, c: &Content, p: &Player) -> f32 {
    if w.quest.is_some() || w.dark_world.is_some() {
        return 0.5;
    }
    let table = c.level(w.level).tower.theme.invisibility;
    match p.team_color {
        Allegiance::Neutral if p.archer > 8 => 0.4,
        Allegiance::Neutral => table[p.archer as usize],
        Allegiance::Blue => table[1],
        _ => table[8],
    }
}

/// `CanGrabLedge(targetY, dir)`.
fn can_grab_ledge(w: &World, c: &Content, id: EntityId, ty: f32, dir: i32) -> bool {
    let at = pos(w, id);
    if solid_at(w, c, id, Vec2::new(at.x, ty + 2.0)) {
        return false;
    }
    if solid_at(w, c, id, Vec2::new(at.x, at.y + 5.0)) {
        return false;
    }
    let px = at.x + 6.0 * dir as f32;
    let above = wrap_point(w, c, Vec2::new(px, ty - 1.0));
    let top = wrap_point(w, c, Vec2::new(px, ty));
    !w.point_check(c, above, Tag::Solid) && w.point_check(c, top, Tag::Solid)
}

fn grab_ledge(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    ty: f32,
    dir: i32,
) -> PState {
    p.facing = dir;
    w.core_mut(id).pos.y = ty + 2.0;
    p.speed.y = 0.0;
    for _ in 0..16 {
        let at = pos(w, id) + Vec2::new(dir as f32, 0.0);
        if solid_at(w, c, id, at) {
            break;
        }
        w.core_mut(id).pos.x += dir as f32;
    }
    p.scale = Vec2::new(1.3, 0.7);
    PState::LedgeGrab
}

fn enter_ledge_grab(w: &mut World, id: EntityId, p: &mut Player) {
    p.aiming = false;
    p.start_aiming_down = false;
    p.speed = Vec2::ZERO;
    p.flap_gravity = 1.0;
    p.last_platform = None;
    let x = pos(w, id).x;
    stop_wall_slide_sound(w, p, x);
    csound(w, p, CharSound::Grab, x);
}

/// `Jump(particles, canSuper, forceSuper, ledgeDir, doubleJump)` (§6.7).
#[allow(clippy::too_many_arguments)]
fn jump(
    w: &mut World,
    _c: &Content,
    id: EntityId,
    p: &mut Player,
    dust: bool,
    can_super: bool,
    force_super: bool,
    ledge_dir: i32,
    double_jump: bool,
) {
    p.auto_bounce = false;
    p.jump_buffer = 0.0;
    p.jump_grace = 0.0;
    prism_force_shatter(w, id, p);
    if p.is_hyper() && p.speed.x != 0.0 {
        p.hyper_dir = sign_i(p.speed.x);
        p.hyper_start_x = pos(w, id).x;
    }
    if let Some(m) = crate::entities::in_mud(w, p) {
        let x = pos(w, id).x;
        crate::entities::mud_splash_up(w, m, x);
        w.sound("env_mudPlayerJump", x);
    }
    let pad = if force_super || !can_super {
        None
    } else {
        find_jump_pad(w, _c, id, p)
    };
    p.speed.y = if force_super {
        JUMP_ONPAD
    } else if let Some(pad) = pad {
        p.can_pad_particles = true;
        crate::entities::jump_pad_launch(w, pad, pos(w, id).x);
        JUMP_ONPAD
    } else {
        JUMP
    };
    let x = pos(w, id).x;
    csound(w, p, CharSound::Jump, x);
    crate::versus::stat(w, p.index, |s| s.jumps += 1);
    p.scale = Vec2::new(0.7, 1.3);
    if ledge_dir != 0 {
        p.facing = ledge_dir;
        p.speed.x = ledge_dir as f32 * 1.5;
        p.auto_move = ledge_dir;
        p.scheduler.schedule(PAction::FinishAutoMove, 6.0, false);
    }
    if p.move_x != 0 && !p.aiming {
        p.speed.x += if double_jump { 1.0 } else { 0.5 } * p.move_x as f32;
    }
    if p.move_x != 0 && double_jump && (sign_i(p.speed.x) != p.move_x || p.speed.x.abs() < 2.0) {
        p.speed.x = 2.0 * p.move_x as f32;
    }
    p.can_var_jump = true;
    if dust {
        let at = pos(w, id) + Vec2::new(0.0, 8.0);
        particles(w, p, ParticleKind::Dust, at, 5, 0.0);
    }
}

/// The jump pad a ground jump launches from: the remembered platform if it
/// is one, else a pad touching `Position + (0, 1)` (§6.7 step 4).
fn find_jump_pad(w: &World, c: &Content, id: EntityId, p: &Player) -> Option<EntityId> {
    if let Some(lp) = p.last_platform
        && w.try_core(lp)
            .is_some_and(|core| core.has_tag(Tag::JumpPad))
    {
        return Some(lp);
    }
    w.first_at(c, id, pos(w, id) + Vec2::new(0.0, 1.0), Tag::JumpPad)
}

fn wall_jump(w: &mut World, id: EntityId, p: &mut Player, dir: i32) {
    prism_force_shatter(w, id, p);
    p.jump_buffer = 0.0;
    p.jump_grace = 0.0;
    p.auto_bounce = false;
    let x = pos(w, id).x;
    csound(w, p, CharSound::Jump, x);
    crate::versus::stat(w, p.index, |s| s.jumps += 1);
    p.speed.y = JUMP;
    p.speed.x = 2.0 * dir as f32;
    p.can_var_jump = true;
    p.scale = Vec2::new(0.7, 1.3);
    p.wall_stick_max = 0.5;
    p.flap_gravity = 1.0;
    p.facing = dir;
    p.auto_move = dir;
    p.scheduler.schedule(PAction::FinishAutoMove, 12.0, false);
    let at = pos(w, id) + Vec2::new(-4.0 * dir as f32, 0.0);
    particles(w, p, ParticleKind::Dust, at, 5, 0.0);
}

fn wings_jump(w: &mut World, id: EntityId, p: &mut Player) {
    p.auto_bounce = false;
    p.jump_buffer = 0.0;
    p.jump_grace = 0.0;
    p.speed.y = WINGS_JUMP;
    p.can_var_jump = true;
    p.flap_gravity = 0.0;
    w.sound("pu_wingFly", pos(w, id).x);
    p.scale = Vec2::new(1.2, 0.8);
}

/// `Bounce(top, forceSuper)` (stomp success).
fn bounce(w: &mut World, c: &Content, id: EntityId, p: &mut Player, top: f32, force_super: bool) {
    p.auto_bounce = true;
    let core = w.core(id);
    let target = top - (core.bottom() - core.pos.y);
    actor::move_towards_y(w, c, id, target, 4.0);
    jump(w, c, id, p, false, false, force_super, 0, false);
    if w.variants_for(p.index as i32).double_jumping {
        p.can_double_jump = true;
    }
}

fn on_collide_h(p: &mut Player, _s: EntityId) {
    p.speed.x = 0.0;
}

fn on_collide_v(w: &mut World, _c: &Content, id: EntityId, p: &mut Player, _s: EntityId) {
    if p.speed.y > 0.0 {
        let x = pos(w, id).x;
        csound(w, p, CharSound::Land, x);
        let k = 0.5 * p.speed.y / MAX_FALL;
        p.scale = Vec2::new(1.0 + k, 1.0 - k);
        p.speed.x = lerp(p.speed.x, 0.0, 0.6 * (p.speed.y / MAX_FALL));
        if p.speed.y > MAX_FALL {
            let at = pos(w, id) + Vec2::new(0.0, 8.0);
            particles(w, p, ParticleKind::Dust, at, 10, 0.0);
        }
        p.speed.y = 0.0;
    } else if p.flap_gravity < 1.0 {
        p.speed.y = 1.5;
        p.flap_bounce = 4.0;
    } else {
        p.speed.y = 0.0;
    }
}

// ------------------------------------------------------------ Ducking §7

fn enter_ducking(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    p.duck_slip_counter = 4.0;
    use_ducking_hitbox(w, id, p);
    let x = pos(w, id).x;
    if p.aiming {
        csound(w, p, CharSound::AimCancel, x);
        set_aiming(w, id, p, false);
    }
    if !p.was_dodge_sliding {
        p.scale = Vec2::new(1.2, 0.8);
        csound(w, p, CharSound::Duck, x);
    } else {
        p.was_dodge_sliding = false;
    }
    hat_pickup_check(w, c, id, p);
    if crate::pickups::level_ending(w) {
        crate::versus::stat(w, p.index, |s| s.duck_dances += 1);
    }
}

fn leave_ducking(w: &mut World, id: EntityId, p: &mut Player) {
    if !p.dodge_sliding {
        use_normal_hitbox(w, id, p);
    }
    p.scale = Vec2::new(0.8, 1.2);
    let x = pos(w, id).x;
    csound(w, p, CharSound::Duck, x);
}

fn ducking_update(w: &mut World, c: &Content, id: EntityId, p: &mut Player) -> PState {
    let t = tm(w);
    crate::versus::stat(w, p.index, |s| s.duck_frames += t);
    let v = w.variants_for(p.index as i32);
    let unduck = can_unduck(w, c, id);
    if unduck && !p.dodge_cooldown && p.input.dodge_pressed && !v.no_dodging {
        p.dodge_sliding = true;
        return PState::Dodging;
    }
    let at = pos(w, id);
    if p.input.jump_pressed
        && w.check_at(c, id, at + Vec2::new(0.0, 1.0), Tag::JumpThru)
        && !solid_at(w, c, id, at + Vec2::new(0.0, 3.0))
    {
        p.jump_buffer = 0.0;
        p.jump_grace = 0.0;
        w.core_mut(id).pos.y += 3.0;
        w.sound("env_gplatPlayerDrop", at.x);
        return PState::Normal;
    }
    if unduck {
        if crate::entities::on_hot_coals(w, p) {
            hot_coals_bounce(w, id, p);
            return PState::Normal;
        }
        if p.input.jump_pressed && p.jump_grace > 0.0 {
            jump(w, c, id, p, true, true, false, 0, false);
            return PState::Normal;
        }
        if !p.on_ground || p.move_y != 1 {
            return PState::Normal;
        }
    }
    if p.input.alt_shoot_pressed && p.trigger_count() > 0 {
        detonate_trigger_arrows(w, c, id, p);
    } else if (p.input.alt_shoot_pressed || p.input.shoot_pressed) && unduck {
        set_aiming(w, id, p, true);
        p.start_aiming_down = true;
        return PState::Normal;
    }
    let target_x = if unduck {
        0.0
    } else {
        0.8 * p.input.move_x as f32
    };
    p.speed.x = approach(p.speed.x, target_x, 0.4 * t);
    if !p.on_ground {
        p.speed.y = approach(p.speed.y, MAX_FALL, GRAVITY * t);
    }
    if let Some(s) = actor::move_h(w, c, id, p.speed.x * t) {
        on_collide_h(p, s);
    }
    if let Some(s) = actor::move_v(w, c, id, p.speed.y * t) {
        on_collide_v(w, c, id, p, s);
    }
    if sign_i(p.move_x as f32) == -p.facing {
        p.facing = -p.facing;
        p.scale = Vec2::new(1.2, 0.8);
    }
    if p.duck_slip_counter > 0.0 {
        p.duck_slip_counter -= t;
    } else if p.on_ground {
        if !actor::check_below_x(w, c, id, -3.0) {
            actor::move_h(w, c, id, -1.0);
            if !actor::check_below(w, c, id) {
                actor::move_v(w, c, id, 1.0);
            }
        } else if !actor::check_below_x(w, c, id, 3.0) {
            actor::move_h(w, c, id, 1.0);
            if !actor::check_below(w, c, id) {
                actor::move_v(w, c, id, 1.0);
            }
        }
    }
    PState::Ducking
}

// ------------------------------------------------------------ Dodging §8

fn enter_dodge(w: &mut World, c: &Content, id: EntityId, p: &mut Player, prev: PState) {
    let v = w.variants_for(p.index as i32);
    prism_force_shatter(w, id, p);
    p.last_caught = None;
    p.dodge_curse_satisfied = prev == PState::Dying;
    let mud = crate::entities::in_mud(w, p).is_some();
    if !p.dodge_sliding && p.on_ground && p.input.aim_axis.y > 0.0 && !mud {
        p.dodge_sliding = true;
        use_ducking_hitbox(w, id, p);
    }
    p.dodge_end = 20.0;
    p.dodge_stall = 5.0;
    p.scale = Vec2::new(1.4, 0.6);
    if p.on_ground {
        p.jump_grace = 12.0;
    }
    p.speed = if p.input.aim_axis.is_zero() || prev == PState::Ducking {
        Vec2::new(p.facing as f32, 0.0) * DODGE_SPEED
    } else {
        eight_way_normal(p.input.aim_axis) * DODGE_SPEED
    };
    if v.super_dodging {
        p.speed *= 1.8;
    } else if p.dodge_sliding {
        p.speed *= 1.4;
    }
    if mud {
        p.speed *= 0.4;
    }
    if v.clumsy_archers && !p.arrows.is_empty() {
        drop_arrow(w, c, id, p);
        w.sound("sfx_arrowDrop", pos(w, id).x);
    }
    let name = if p.speed.x.abs() <= 0.1 {
        if p.speed.y < 0.0 {
            "char_dashR"
        } else {
            "char_dashL"
        }
    } else if p.speed.x > 0.0 {
        "char_dashR"
    } else {
        "char_dashL"
    };
    w.sound(name, pos(w, id).x);
    p.can_var_jump = false;
    p.dodge_cooldown = true;
    crate::versus::stat(w, p.index, |s| s.dodges += 1);
    p.dodge_speed = p.speed;
    if p.aiming {
        let x = pos(w, id).x;
        csound(w, p, CharSound::AimCancel, x);
        set_aiming(w, id, p, false);
    }
}

fn leave_dodge(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    if p.dodge_sliding && can_unduck(w, c, id) {
        use_normal_hitbox(w, id, p);
    }
    p.dodge_sliding = false;
    p.dodge_catch = 0.0;
    p.dodge_end = 0.0;
    p.scheduler.schedule(PAction::EndDodgeCooldown, 25.0, false);
    if w.variants_for(p.index as i32).cursed_dodges && !p.dodge_curse_satisfied {
        p.speed = -p.dodge_speed;
        w.sound("sfx_cursedDeath", pos(w, id).x);
        die(w, c, id, p, DeathCause::Curse, -1);
    }
}

fn get_dodge_exit_state(w: &World, c: &Content, id: EntityId, p: &mut Player) -> PState {
    if p.dodge_sliding && (p.input_ducking() || !can_unduck(w, c, id)) {
        p.dodge_sliding = false;
        p.was_dodge_sliding = true;
        PState::Ducking
    } else {
        PState::Normal
    }
}

fn dodging_update(w: &mut World, c: &Content, id: EntityId, p: &mut Player) -> PState {
    let t = tm(w);
    p.invis_opacity = 1.0;
    // 2. jump-thru helping
    let here = w.placed(id);
    if p.speed.y < -0.5 && w.any(c, &here, Tag::JumpThru, id) {
        actor::move_v(w, c, id, -2.0);
    } else if p.speed.y == 0.0
        && w.any(c, &here, Tag::JumpThru, id)
        && !w.check_at(c, id, here.pos + Vec2::new(0.0, -5.0), Tag::JumpThru)
    {
        actor::move_v(w, c, id, -1.0);
    }
    // 3.
    let at = pos(w, id);
    particles(w, p, ParticleKind::Dash, at, 2, p.speed.angle());
    // 4. Shoot detonates trigger arrows (before the coals and the move).
    if p.input.shoot_pressed && p.trigger_count() > 0 {
        detonate_trigger_arrows(w, c, id, p);
    }
    // 5. hot coals
    if crate::entities::on_hot_coals(w, p) && (!p.dodge_sliding || can_unduck(w, c, id)) {
        hot_coals_bounce(w, id, p);
        return PState::Normal;
    }
    // 6.
    if p.dodge_sliding {
        hat_pickup_check(w, c, id, p);
    }
    // 7. movement
    if p.dodge_catch <= 0.0 {
        p.scale.x = approach(p.scale.x, 1.0, 0.02 * t);
        p.scale.y = approach(p.scale.y, 1.0, 0.02 * t);
        if p.speed.x != 0.0 {
            let sx = sign(p.speed.x);
            if p.speed.x.abs() > 0.02 && solid_at(w, c, id, pos(w, id) + Vec2::new(sx, 0.0)) {
                on_collide_h_dodging(w, c, id, p, None);
            }
            if let Some(s) = actor::move_h(w, c, id, p.speed.x * t) {
                on_collide_h_dodging(w, c, id, p, Some(s));
            }
        }
        if p.speed.y > 0.0 {
            let b = pos(w, id) + Vec2::new(0.0, 1.0);
            if p.speed.y > 0.02
                && (solid_at(w, c, id, b) || w.first_outside(c, id, b, Tag::JumpThru).is_some())
            {
                on_collide_v_dodging(w, c, id, p, None);
            }
            if let Some(s) = actor::move_v(w, c, id, p.speed.y * t) {
                on_collide_v_dodging(w, c, id, p, Some(s));
            }
        } else if p.speed.y < 0.0 {
            let a = pos(w, id) + Vec2::new(0.0, -1.0);
            if p.speed.y < -0.02 && solid_at(w, c, id, a) {
                on_collide_v_dodging(w, c, id, p, None);
            }
            if let Some(s) = actor::move_v(w, c, id, p.speed.y * t) {
                on_collide_v_dodging(w, c, id, p, Some(s));
            }
        }
    } else {
        p.dodge_catch -= t;
    }
    // 8.
    p.speed *= pow(DODGE_MULT, t);
    // 9. catch
    let probe = Placed {
        pos: pos(w, id),
        collider: Collider::Box(CATCH_HITBOX),
    };
    if let Some(a) = w.first(c, &probe, Tag::Arrow, id) {
        let dangerous = w.arrow(a).is_some_and(|ar| ar.dangerous());
        if !p.has_shield || !dangerous {
            catch_arrow_id(w, c, id, p, a);
        }
    }
    // 10. jump out
    if (p.input.jump_pressed || p.jump_buffer > 0.0) && (!p.dodge_sliding || can_unduck(w, c, id)) {
        let at = pos(w, id);
        if solid_at(w, c, id, wrap_point(w, c, at + Vec2::new(-2.0, 0.0))) {
            if !dodge_wall_jump(w, id, p, 1) {
                return PState::Normal;
            }
        } else if solid_at(w, c, id, wrap_point(w, c, at + Vec2::new(2.0, 0.0))) {
            if !dodge_wall_jump(w, id, p, -1) {
                return PState::Normal;
            }
        } else if p.jump_grace > 0.0 {
            if p.dodge_sliding {
                use_normal_hitbox(w, id, p);
                p.dodge_sliding = false;
                p.auto_move = sign_i(p.speed.x);
                p.scheduler.schedule(PAction::FinishAutoMove, 16.0, false);
                if p.speed.x.abs() >= 1.75 {
                    p.speed.x = sign(p.speed.x) * 2.8;
                }
            } else {
                p.speed.x = clamp(p.speed.x, -1.8, 1.8);
            }
            jump(w, c, id, p, true, true, false, 0, false);
            return PState::Normal;
        }
    }
    // 11.
    if p.dodge_sliding && p.jump_grace <= 0.0 && can_unduck(w, c, id) {
        use_normal_hitbox(w, id, p);
        p.dodge_sliding = false;
    }
    // 12.
    if p.input.dodge_pressed {
        prism_force_shatter(w, id, p);
        p.can_hyper = true;
        if p.dodge_catch > 0.0 && p.speed.length_sq() >= HYPER_THRESHOLD {
            w.sound("char_miracleGrab", pos(w, id).x);
        }
        return get_dodge_exit_state(w, c, id, p);
    }
    // 13.
    if p.dodge_end > 0.0 {
        p.dodge_end -= t;
        return PState::Dodging;
    }
    // 14.
    if p.dodge_stall > 0.0 && p.input.dodge_check {
        p.dodge_stall -= t;
        return PState::Dodging;
    }
    get_dodge_exit_state(w, c, id, p)
}

fn on_collide_h_dodging(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    platform: Option<EntityId>,
) {
    let sx = sign(p.speed.x);
    if p.on_ground && !p.dodge_sliding {
        use_ducking_hitbox(w, id, p);
        if !solid_at(w, c, id, pos(w, id) + Vec2::new(sx, 0.0)) {
            w.core_mut(id).pos.x += sx;
            p.speed *= 1.4;
            p.dodge_sliding = true;
            return;
        }
        use_normal_hitbox(w, id, p);
    }
    let at = pos(w, id);
    if !solid_at(w, c, id, Vec2::new(at.x + sx, at.y - 5.0)) {
        if platform.is_none() {
            actor::move_v(w, c, id, -2.0);
        }
    } else if !solid_at(w, c, id, Vec2::new(at.x + sx, at.y + 5.0)) {
        if platform.is_none() {
            actor::move_v(w, c, id, 2.0);
        }
    } else if p.speed.y.abs() > 0.02 {
        p.speed.y = p.speed.length() * sign(p.speed.y);
        p.speed.x = 0.0;
    }
}

fn on_collide_v_dodging(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    platform: Option<EntityId>,
) {
    if p.speed.x.abs() < 0.02 {
        if platform.is_some() {
            return;
        }
        let at = pos(w, id);
        let solid2 = |dx: f32, dy: f32| {
            solid_at(w, c, id, at + Vec2::new(dx, dy))
                && solid_at(w, c, id, at + Vec2::new(dx * 3.0 / 5.0, dy))
        };
        let jt = |dx: f32| {
            w.first_outside(c, id, at + Vec2::new(dx, 1.0), Tag::JumpThru)
                .is_some()
                && w.first_outside(c, id, at + Vec2::new(dx * 3.0 / 5.0, 1.0), Tag::JumpThru)
                    .is_some()
        };
        if p.speed.y > 0.0 {
            if !(solid2(-5.0, 1.0) || jt(-5.0)) {
                actor::move_h(w, c, id, -2.0);
            } else if !(solid2(5.0, 1.0) || jt(5.0)) {
                actor::move_h(w, c, id, 2.0);
            }
        } else if !solid2(-5.0, -1.0) {
            actor::move_h(w, c, id, -2.0);
        } else if !solid2(5.0, -1.0) {
            actor::move_h(w, c, id, 2.0);
        }
    } else {
        p.speed.x = p.speed.length() * sign(p.speed.x);
        p.speed.y = 0.0;
    }
}

/// `DodgeWallJump(dir)` (§8.6).
fn dodge_wall_jump(w: &mut World, id: EntityId, p: &mut Player, dir: i32) -> bool {
    if p.dodge_end < 12.0 {
        wall_jump(w, id, p, dir);
        return false;
    }
    p.auto_bounce = false;
    p.jump_buffer = 0.0;
    p.jump_grace = 0.0;
    let x = pos(w, id).x;
    csound(w, p, CharSound::Jump, x);
    crate::versus::stat(w, p.index, |s| s.jumps += 1);
    p.speed = angle_to_vector(-PI / 2.0 + (PI / 4.0) * dir as f32, DODGE_SPEED);
    p.dodge_end = 20.0;
    p.wall_stick_max = 0.5;
    p.flap_gravity = 1.0;
    p.facing = dir;
    p.can_var_jump = true;
    p.scale = Vec2::new(0.7, 1.3);
    true
}

// ------------------------------------------------------------ catching §8.4

fn catch_arrow_id(w: &mut World, c: &Content, id: EntityId, p: &mut Player, a: EntityId) {
    let Some(Kind::Arrow(mut ar)) = w.take_kind(a) else {
        return;
    };
    catch_arrow(w, c, id, p, a, &mut ar);
    if w.exists(a) {
        w.put_kind(a, Kind::Arrow(ar));
    }
}

/// `CatchArrow(arrow)`.
pub fn catch_arrow(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    aid: EntityId,
    a: &mut Arrow,
) {
    let ok = w.exists(aid)
        && !w.core(aid).marked_for_removal
        && a.can_catch(id)
        && !a.collectible()
        && a.cannot_hit != Some(id)
        && (!p.has_shield || !a.dangerous())
        && p.last_caught != Some(aid);
    if !ok {
        if w.exists(aid) {
            let core = w.core_mut(aid);
            core.active = true;
            core.collidable = true;
        }
        return;
    }
    p.dodge_catch = 10.0;
    crate::versus::stat(w, p.index, |s| s.arrows_caught += 1);
    if a.player_index == p.index as i8 {
        crate::versus::stat(w, p.index, |s| s.own_arrows_caught += 1);
    } else if crate::hurt::is_friendly(w, a.player_index, p.index as i8) {
        crate::versus::stat(w, p.index, |s| s.team_arrow_catches += 1);
    }
    if matches!(a.arrow_type, ArrowType::Bomb | ArrowType::SuperBomb)
        && a.extra.fuse_on
        && a.state != crate::arrow::ArrowState::Shooting
    {
        crate::versus::stat(w, p.index, |s| s.bombs_defused += 1);
    }
    let at = w.core(aid).pos;
    particles(w, p, ParticleKind::CatchShine, at, 1, 0.0);
    let x = pos(w, id).x;
    csound(w, p, CharSound::ArrowGrab, x);
    if p.dodge_end <= 0.0 && p.state() == PState::Dodging {
        w.sound("char_dodgeStallGrab", x);
    }
    rumble(w, p, 0.8, 12);
    p.dodge_curse_satisfied = true;
    p.hud_counter = 40.0;
    crate::arrows::on_player_catch(w, c, aid, a, id, p);
    if a.extra.burning {
        crate::entities::start_fire(w, p);
    }
    if crate::arrows::prism_check(a) {
        w.remove(aid);
        w.core_mut(aid).collidable = false;
        crate::arrows::prism::start_prism(w, c, id, p, a.player_index);
    } else if p.arrows.can_add_arrow(a.arrow_type) {
        arrow_collect(w, c, id, p, aid, a, true);
    } else {
        p.last_caught = Some(aid);
        arrow::drop(w, aid, a, p.facing as f32);
    }
}

/// `CollectArrows(types)` (§12.1).
pub(crate) fn collect_arrows(
    w: &mut World,
    id: EntityId,
    p: &mut Player,
    types: &[ArrowType],
) -> bool {
    if p.arrows.can_add_arrows(types) {
        if types.len() == 1 {
            let x = pos(w, id).x;
            csound(w, p, CharSound::ArrowRecover, x);
        }
        p.arrows.add_arrows(types);
        p.hud_counter = 40.0;
        crate::versus::stat(w, p.index, |s| s.arrows_collected += types.len() as u32);
        true
    } else {
        false
    }
}

/// `Arrow.OnPlayerCollect(player, force)`.
fn arrow_collect(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    aid: EntityId,
    a: &mut Arrow,
    force: bool,
) {
    if !(force || a.collectible()) {
        return;
    }
    if collect_arrows(w, id, p, &[a.arrow_type]) {
        if let Some(cp) = a.buried_in.take() {
            corpse::arrow_taken(w, c, cp, aid);
        }
        w.remove(aid);
        w.core_mut(aid).collidable = false;
    }
}

/// Step 25c: the first arrow under the pick-up box.
fn collect_arrows_overlap(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    let probe = Placed {
        pos: pos(w, id),
        collider: Collider::Box(PICKUP_HITBOX),
    };
    if let Some(a) = w.first(c, &probe, Tag::Arrow, id)
        && let Some(Kind::Arrow(mut ar)) = w.take_kind(a)
    {
        arrow_collect(w, c, id, p, a, &mut ar, false);
        w.put_kind(a, Kind::Arrow(ar));
    }
}

/// `DropArrow()` (§10.6).
fn drop_arrow(w: &mut World, _c: &Content, id: EntityId, p: &mut Player) {
    let Some(t) = p.arrows.use_arrow() else {
        return;
    };
    let angle = -3.0 * PI / 4.0 + w.rng.next_float() * (PI / 2.0);
    let a = arrow::create(w, t, shooter(id, p, w), pos(w, id), angle);
    if let Some(Kind::Arrow(mut ar)) = w.take_kind(a) {
        arrow::drop(w, a, &mut ar, -p.facing as f32);
        w.put_kind(a, Kind::Arrow(ar));
    }
}

fn shooter(id: EntityId, p: &Player, w: &World) -> Shooter {
    Shooter {
        id: Some(id),
        player_index: p.index as i8,
        allegiance: w.core(id).allegiance,
        team_color: p.team_color,
        hyper: p.is_hyper(),
    }
}

// ------------------------------------------------------------ LedgeGrab §9.1

fn ledge_grab_update(w: &mut World, c: &Content, id: EntityId, p: &mut Player) -> PState {
    let v = w.variants_for(p.index as i32);
    let t = tm(w);
    crate::versus::stat(w, p.index, |s| s.ledge_frames += t);
    if !p.dodge_cooldown && p.input.dodge_pressed && !v.no_dodging {
        return PState::Dodging;
    }
    // Shoot wins over a same-frame Alt (no detonation then).
    if p.input.shoot_pressed {
        set_aiming(w, id, p, true);
        return PState::Normal;
    }
    if p.input.alt_shoot_pressed {
        if p.trigger_count() == 0 {
            set_aiming(w, id, p, true);
            return PState::Normal;
        }
        detonate_trigger_arrows(w, c, id, p);
    }
    if p.move_y == 1 || sign_i(p.move_x as f32) != p.facing {
        p.grace_ledge_dir = -p.facing;
        p.jump_grace = 12.0;
        return PState::Normal;
    }
    if p.input.jump_pressed {
        jump(w, c, id, p, false, false, false, 0, false);
        return PState::Normal;
    }
    let ty = floor(pos(w, id).y) - 2.0;
    if !can_grab_ledge(w, c, id, ty, p.facing) {
        return PState::Normal;
    }
    PState::LedgeGrab
}

// ------------------------------------------------------------ Dying §9.3

fn dying_update(w: &mut World, c: &Content, id: EntityId, p: &mut Player) -> PState {
    let v = w.variants_for(p.index as i32);
    if p.input.shoot_pressed {
        detonate_trigger_arrows(w, c, id, p);
        if p.dead {
            return PState::Dying;
        }
    }
    if p.input.dodge_pressed && !p.dodge_cooldown && !v.no_dodging {
        crate::versus::stat(w, p.index, |s| s.grace_period_catches += 1);
        if let Some(a) = p.death_arrow.take() {
            catch_arrow_id(w, c, id, p, a);
        }
        return PState::Dodging;
    }
    if p.dying > 0.0 {
        p.dying -= tm(w);
        if p.dying <= 0.0 {
            if let Some(a) = p.death_arrow
                && let Some(Kind::Arrow(mut ar)) = w.take_kind(a)
            {
                arrow::unfreeze(w, a, &mut ar);
                die_by_arrow(w, c, id, p, a, &mut ar);
                w.put_kind(a, Kind::Arrow(ar));
            } else {
                die(w, c, id, p, DeathCause::Arrow, -1);
            }
        }
    }
    PState::Dying
}

// ------------------------------------------------------------ shooting §10

fn shoot_arrow(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    set_aiming(w, id, p, false);
    let x = pos(w, id).x;
    if let Some(t) = p.arrows.first() {
        if p.spam_shot > 0.0 {
            crate::versus::stat(w, p.index, |s| s.spam_shots += 1);
        }
        p.spam_shot = 10.0;
        csound(w, p, CharSound::FireArrow, x);
        let dir = find_auto_lock_angle(w, c, id, p);
        p.arrows.use_arrow();
        crate::versus::stat(w, p.index, |s| s.arrows_shot += 1);
        let s = shooter(id, p, w);
        let a = arrow::create(w, t, s, pos(w, id) + Vec2::new(0.0, -1.0), dir);
        p.hud_counter = 40.0;
        if t == ArrowType::Trigger {
            register_trigger_arrow(w, id, p, a);
        }
    } else {
        p.hud_counter = 40.0;
        csound(w, p, CharSound::NoFire, x);
        crate::versus::stat(w, p.index, |s| s.failed_shots += 1);
        rumble(w, p, 0.25, 5);
        if w.variants_for(p.index as i32).cursed_bows {
            p.speed = angle_to_vector(p.aim_direction, -5.5);
            w.sound("sfx_cursedDeath", x);
            die(w, c, id, p, DeathCause::Curse, p.index as i8);
        }
    }
}

/// `FindAutoLockAngle()` (§10.4).
fn find_auto_lock_angle(w: &World, c: &Content, id: EntityId, p: &Player) -> f32 {
    let o = pos(w, id) + Vec2::new(0.0, -1.0);
    let my_all = w.core(id).allegiance;
    let mut best: Option<(i32, f32)> = None;
    let mut result = p.aim_direction;
    for &e in w.tagged(Tag::Target) {
        if e == id {
            continue;
        }
        let ec = w.core(e);
        if ec.seek_priority <= 0 {
            continue;
        }
        if !(my_all == Allegiance::Neutral || ec.allegiance != my_all) {
            continue;
        }
        let d = o.dist_sq(ec.pos);
        let eligible = match best {
            None => d <= AUTO_LOCK_DIST_SQ,
            Some((bp, _)) if ec.seek_priority > bp => d <= AUTO_LOCK_DIST_SQ,
            Some((_, bd)) => d < bd,
        };
        if !eligible {
            continue;
        }
        let ang = angle_between(o, ec.pos);
        if abs_angle_diff(p.aim_direction, ang) > AUTO_LOCK_MAX_ANGLE {
            continue;
        }
        if w.line_check(c, o, ec.pos, Tag::Solid) {
            continue;
        }
        best = Some((ec.seek_priority, d));
        result = ang;
    }
    result
}

// ------------------------------------------------------------ hurting §12-13

/// `CanHurt`.
pub fn can_hurt(w: &World, id: EntityId, p: &Player) -> bool {
    !p.dead && !w.core(id).flash.flashing && (p.state() < PState::Dodging || p.has_shield)
}

/// `CanHurtIgnoreDodge`.
pub fn can_hurt_ignore_dodge(w: &World, id: EntityId, p: &Player) -> bool {
    !p.dead && !w.core(id).flash.flashing && p.state() < PState::Frozen
}

/// `Player.OnArrowHit(arrow)`: called from the arrow's update.
pub fn on_arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    let Some(Kind::Player(mut p)) = w.take_kind(id) else {
        return false;
    };
    if p.state() == PState::Dodging && !p.has_shield {
        catch_arrow(w, c, id, &mut p, aid, a);
    } else {
        hurt_delayed(w, c, id, &mut p, aid, a);
    }
    w.put_kind(id, Kind::Player(p));
    true
}

fn hurt_delayed(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    aid: EntityId,
    a: &mut Arrow,
) {
    if !can_hurt(w, id, p) {
        return;
    }
    // Hit stats for the shooter (not for self hits).
    let shooter_i = a.player_index;
    if shooter_i >= 0 && shooter_i != p.index as i8 {
        let (kind, turns, bounced, frames, drilled) = (
            a.arrow_type,
            a.extra.turns as u32,
            a.extra.bounced.max(0) as u32,
            a.travel_frames,
            a.extra.has_drilled,
        );
        crate::versus::stat(w, shooter_i as u8, |s| {
            match kind {
                ArrowType::Bolt => s.most_bolt_turns = s.most_bolt_turns.max(turns),
                ArrowType::Laser => {
                    s.most_bounces_laser_kill = s.most_bounces_laser_kill.max(bounced)
                }
                _ => s.longest_shot = s.longest_shot.max(frames),
            }
            if drilled {
                s.drill_hits += 1;
            }
        });
    }
    if p.has_shield {
        if shooter_i >= 0 {
            crate::versus::stat(w, shooter_i as u8, |s| s.shields_broken += 1);
        }
        lose_shield(w, id, p);
        p.speed = a.speed;
        arrow::enter_fall_mode(w, aid, a, true, false, true);
        if matches!(p.state(), PState::LedgeGrab | PState::Dodging) {
            set_state(w, c, id, p, PState::Normal);
        }
    } else {
        p.death_arrow = Some(aid);
        arrow::freeze(w, aid, a, id);
        set_state(w, c, id, p, PState::Dying);
    }
}

/// `Die(Arrow)` (§13.3).
fn die_by_arrow(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    aid: EntityId,
    a: &mut Arrow,
) {
    let ledge = if p.sm.previous == PState::LedgeGrab
        && Vec2::new(p.facing as f32, 0.0).dot(a.speed.safe_normalize()) > 0.8
    {
        p.facing
    } else {
        0
    };
    let laser = a.arrow_type == crate::quiver::ArrowType::Laser;
    let own = a.player_index == p.index as i8;
    let k = a.player_index;
    if k >= 0 {
        let (falling, hyper) = (a.state == crate::arrow::ArrowState::Falling, a.from_hyper);
        crate::versus::stat(w, k as u8, |s| {
            if laser && own {
                s.self_laser_kills += 1;
            }
            if falling && !own {
                s.dropped_arrow_kills += 1;
            }
            if hyper {
                if own {
                    s.hyper_self_kills += 1;
                } else {
                    s.hyper_arrow_kills += 1;
                }
            }
        });
    }
    if let Some(cp) = die_with(w, c, id, p, DeathCause::Arrow, a.player_index, laser) {
        corpse::die_by_arrow(w, c, cp, aid, a, ledge);
    }
}

/// `Die(cause, killer)` (§13.4). Returns the corpse.
pub fn die(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    cause: DeathCause,
    killer: i8,
) -> Option<EntityId> {
    die_with(w, c, id, p, cause, killer, false)
}

/// `Die` with the killing arrow's laser flag (the `DieLaser` sound).
fn die_with(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    cause: DeathCause,
    killer: i8,
    laser: bool,
) -> Option<EntityId> {
    if p.dead {
        return None;
    }
    p.dead = true;
    remove_all_trigger_arrows(w, p);
    let mut killer = killer;
    let on_ledge = p.state() == PState::LedgeGrab
        || (p.state() == PState::Dying && p.sm.previous == PState::LedgeGrab);
    let (in_cooldown, hyper) = (p.dodge_cooldown, p.is_hyper());
    if let Some(pr) = p.prism.filter(|&pr| w.exists(pr)) {
        if matches!(cause, DeathCause::Squish | DeathCause::Miasma)
            && let Some(Kind::Prism(prism)) = w.kind(pr)
        {
            killer = prism.owner;
        }
        if let Some(Kind::Prism(prism)) = w.kind(pr).copied()
            && killer >= 0
            && killer != p.index as i8
        {
            let team = w.versus.as_ref().is_some_and(|m| m.settings.team_mode())
                && crate::hurt::is_friendly(w, killer, prism.owner);
            crate::versus::stat(w, killer as u8, |s| {
                if team {
                    s.enemy_in_team_prism_kills += 1;
                } else {
                    s.enemy_in_prism_kills += 1;
                }
            });
        }
        let mut ctx: crate::hurt::Ctx = Some((id, &mut *p));
        crate::arrows::prism::captive_die(w, pr, &mut ctx);
    }
    p.prism = None;
    // 3. killer stats.
    if killer >= 0 {
        let held = w
            .player_by_index(killer as u8)
            .and_then(|k| w.player(k))
            .is_some_and(|k| !k.dead && k.prism.is_some());
        crate::versus::stat(w, killer as u8, |s| {
            if held {
                s.self_in_prism_kills += 1;
            }
            if cause == DeathCause::Squish {
                s.crush_others_kills += 1;
            }
        });
    }
    // 11. victim / killer stats.
    let me = p.index;
    crate::versus::stat(w, me, |s| {
        if in_cooldown {
            s.deaths_during_dodge_cooldown += 1;
        }
        if hyper {
            if killer == me as i8 {
                s.hyper_self_kills += 1;
            } else {
                s.hyper_deaths += 1;
            }
        }
    });
    if on_ledge && killer >= 0 && killer != me as i8 {
        crate::versus::stat(w, killer as u8, |s| s.ledge_grabbed_kills += 1);
    }
    let at = pos(w, id);
    // 4. drop the drawn arrow
    if p.aiming
        && let Some(t) = p.arrows.use_arrow()
    {
        let a = arrow::create(
            w,
            t,
            shooter(id, p, w),
            at + Vec2::new(0.0, -1.0),
            p.aim_direction,
        );
        if let Some(Kind::Arrow(mut ar)) = w.take_kind(a) {
            arrow::drop(w, a, &mut ar, -p.facing as f32);
            w.put_kind(a, Kind::Arrow(ar));
        }
        w.sound("sfx_dropDrawn", at.x);
    }
    // 5. corpse
    let cp = corpse::create(w, c, id, p);
    // 6.
    let s = match cause {
        DeathCause::Arrow if laser => CharSound::DieLaser,
        DeathCause::Arrow => CharSound::Die,
        DeathCause::Explosion => CharSound::DieBomb,
        DeathCause::JumpedOn => CharSound::DieStomp,
        _ => CharSound::DieEnv,
    };
    csound(w, p, s, at.x);
    // 7.
    rumble(w, p, 1.0, 40);
    w.screen_shake(10.0);
    particles(w, p, ParticleKind::LightFade, at, 1, 0.0);
    // 8.
    if cause != DeathCause::Squish {
        lose_hat(w, c, id, p, None, false);
    }
    // 9.
    if p.has_wings {
        p.has_wings = false;
        particles(w, p, ParticleKind::WingsBurn, at, 10, 0.0);
    }
    stop_wall_slide_sound(w, p, at.x);
    // 10.
    particles(w, p, ParticleKind::DeathSkull, at, 1, 0.0);
    // 12. Session.OnPlayerDeath -> RoundLogic (scoring, round end). Dark
    // World keeps what the archer carried (`LastKnownInventories`).
    crate::darkworld::record_death_inventory(w, p);
    crate::versus::on_player_death(w, c, p.index, killer, cause, at);
    crate::coop::on_player_death(w, c, p.index, killer, cause, at);
    crate::entities::on_player_death(w);
    crate::entities::surfaces::player_reset(w, p.index);
    w.emit(Event::Death {
        player: p.index,
        killer,
        cause: cause as u8,
    });
    // 13.
    w.remove(id);
    Some(cp)
}

/// Step 25b: `OnPlayerCollide` of every `PlayerCollider` entity overlapping
/// the archer (shield box when shielded).
fn player_collider_overlaps(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    let core = w.core(id);
    let probe = Placed {
        pos: core.pos,
        collider: core
            .target_collider
            .map(Collider::Box)
            .unwrap_or(core.collider),
    };
    let hits = w.all(c, &probe, Tag::PlayerCollider, id);
    for e in hits {
        if p.dead {
            break;
        }
        // Other `PlayerCollider` kinds (hazards, enemies) add arms here.
        match w.kind(e) {
            Some(Kind::Miasma(_)) => crate::versus::miasma::on_player_collide(w, c, e, id, p),
            Some(Kind::Bramble(_)) => crate::arrows::brambles::on_player_collide(w, c, e, id, p),
            Some(Kind::Chest(_)) => crate::treasure::on_player_collide(w, c, e, id, p),
            Some(Kind::Corpse(_)) => corpse::on_player_collide(w, c, e, id, p),
            Some(Kind::Tower(_)) => crate::entities::on_player_collide(w, c, e, id, p),
            Some(Kind::Enemy(_)) => crate::enemies::on_player_collide(w, c, e, id, p),
            Some(Kind::EnemyAttack(_)) => crate::enemies::attack::on_player_collide(w, c, e, id, p),
            Some(Kind::Hazard(_)) => crate::enemies::hazard::on_player_collide(w, c, e, id, p),
            Some(Kind::Dw(_)) => crate::darkworld::part::on_player_collide(w, c, e, id, p),
            Some(Kind::FloorMiasma(_)) => crate::quest::floor::on_player_collide(w, c, e, id, p),
            Some(Kind::Dummy(_)) => crate::trials::dummy_player_collide(w, c, e, id, p),
            _ => {}
        }
    }
}

/// `Hurt(cause, bounceFrom, attacker, sfx)` (§13.5): hazards and enemies.
#[allow(clippy::too_many_arguments)]
pub fn hurt(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    cause: DeathCause,
    bounce_from: Vec2,
    attacker: i8,
    sfx: Option<&'static str>,
) {
    let push = (pos(w, id) - bounce_from).safe_normalize_len(3.2);
    if can_hurt(w, id, p) {
        if p.has_shield {
            if attacker >= 0 {
                crate::versus::stat(w, attacker as u8, |s| s.shields_broken += 1);
            }
            lose_shield(w, id, p);
            p.speed = push;
            p.auto_bounce = true;
            if matches!(p.state(), PState::LedgeGrab | PState::Dodging) {
                set_state(w, c, id, p, PState::Normal);
            }
        } else {
            p.speed = push;
            if let Some(s) = sfx {
                w.sound(s, pos(w, id).x);
            }
            die(w, c, id, p, cause, attacker);
        }
    } else {
        p.speed = push;
        p.auto_bounce = true;
        if p.state() == PState::LedgeGrab {
            set_state(w, c, id, p, PState::Normal);
        }
    }
}

// ------------------------------------------------ tower-entity hooks

/// `Hurt(lava)` (§13.5): pushed along the lava's push direction; killed
/// with credit to the lava's owner unless protected.
pub fn hurt_lava(w: &mut World, c: &Content, id: EntityId, p: &mut Player, push: Vec2, owner: i8) {
    p.speed = push * 3.2;
    if can_hurt(w, id, p) {
        if p.has_shield {
            lose_shield(w, id, p);
            p.auto_bounce = true;
            if matches!(p.state(), PState::LedgeGrab | PState::Dodging) {
                set_state(w, c, id, p, PState::Normal);
            }
        } else {
            w.sound("sfx_lavaBurn", pos(w, id).x);
            die(w, c, id, p, DeathCause::Lava, owner);
        }
    } else {
        p.auto_bounce = true;
        if p.state() == PState::LedgeGrab {
            set_state(w, c, id, p, PState::Normal);
        }
    }
}

/// `Bounce(top)` without forced super jump (orbs, dummies, moon glass).
pub fn stomp_bounce(w: &mut World, c: &Content, id: EntityId, p: &mut Player, top: f32) {
    bounce(w, c, id, p, top, false);
}

/// `OnExplodePush(explosion, normal)` (§13.2).
pub fn on_explode_push(w: &mut World, c: &Content, id: EntityId, p: &mut Player, normal: Vec2) {
    set_state(w, c, id, p, PState::Normal);
    p.can_hyper = false;
    p.speed = normal * 5.0;
    crate::entities::start_fire(w, p);
    w.sound("sfx_bombMiss", pos(w, id).x);
    rumble(w, p, 0.25, 20);
    crate::versus::stat(w, p.index, |s| s.explosions_survived += 1);
}

/// `OnExplode` -> `Hurt(explosion, normal)` (§13.2): dodging does not
/// protect; the corpse flies away from the blast at 6.
pub fn explode_hurt(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    at: Vec2,
    killer: i8,
) {
    if !can_hurt_ignore_dodge(w, id, p) {
        return;
    }
    if p.has_shield {
        if killer >= 0 {
            crate::versus::stat(w, killer as u8, |s| s.shields_broken += 1);
        }
        lose_shield(w, id, p);
        if matches!(p.state(), PState::LedgeGrab | PState::Dodging) {
            set_state(w, c, id, p, PState::Normal);
        }
        return;
    }
    crate::versus::stat(w, p.index, |s| s.explosions_survived -= 1);
    if killer >= 0 && killer != p.index as i8 {
        match w.items.blast {
            crate::pickups::Blast::Trigger => {
                crate::versus::stat(w, killer as u8, |s| s.trigger_bomb_kills += 1)
            }
            crate::pickups::Blast::Trap => {
                crate::versus::stat(w, killer as u8, |s| s.bomb_trap_kills += 1)
            }
            crate::pickups::Blast::Normal => {}
        }
    }
    let from = pos(w, id);
    if let Some(cp) = die(w, c, id, p, DeathCause::Explosion, killer)
        && let Some(Kind::Corpse(k)) = w.kind_mut(cp)
    {
        k.speed = (from - at).safe_normalize_len(6.0);
    }
}

/// `HotCoalsBounce()` (§6.7).
pub fn hot_coals_bounce(w: &mut World, id: EntityId, p: &mut Player) {
    if p.speed.y < 0.0 {
        return;
    }
    w.sound("sfx_coalBurn", pos(w, id).x);
    p.speed.y = if p.input.jump_check { -2.88 } else { -2.24 };
    if p.move_x != 0 {
        p.speed.x += 0.3 * p.move_x as f32;
    }
    p.speed.x = clamp(p.speed.x, -3.0, 3.0);
    p.auto_bounce = true;
    p.can_var_jump = true;
    p.jump_grace = 0.0;
    crate::entities::start_fire(w, p);
    p.scale = Vec2::new(0.8, 1.2);
}

/// `DoSquish(solid, direction)` (§13.6).
pub fn do_squish(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    dir: SquishDir,
    _platform: EntityId,
) {
    if p.dead {
        return;
    }
    let mover = w.mover;
    if let Some(m) = mover {
        w.core_mut(m).collidable = true;
    }
    let set_mover = |w: &mut World, on: bool| {
        if let Some(m) = mover {
            w.core_mut(m).collidable = on;
        }
    };
    let at = pos(w, id);
    for i in 0..=2 {
        for j in 0..=2 {
            if i == 0 && j == 0 {
                continue;
            }
            for k in [-1, 1] {
                for l in [-1, 1] {
                    let off = Vec2::new((3 * i * k) as f32, (3 * j * l) as f32);
                    if !solid_at(w, c, id, at + off) {
                        w.core_mut(id).pos = at + off;
                        actor::move_exact_h(w, c, id, -off.x as i32, ExactCollide::None);
                        actor::move_exact_v(w, c, id, -off.y as i32, ExactCollide::None);
                        set_mover(w, false);
                        return;
                    }
                }
            }
        }
    }
    let dv = dir.vec();
    if dv.y != 0.0 && p.state() != PState::Ducking {
        use_ducking_hitbox(w, id, p);
        if dir == SquishDir::Up
            && let Some(m) = mover
        {
            let top = w.core(m).top();
            w.core_mut(id).set_bottom(top);
        }
        if !solid_at(w, c, id, pos(w, id)) {
            set_state(w, c, id, p, PState::Ducking);
            set_mover(w, false);
            return;
        }
    }
    let was_ducking = p.state() == PState::Ducking;
    if dir == SquishDir::Up {
        if was_ducking {
            w.core_mut(id).pos.y -= 6.0;
        } else {
            w.core_mut(id).pos.y += 8.0;
        }
    }
    let killer = mover.map(|m| w.core(m).squisher_index).unwrap_or(-1);
    if let Some(cp) = die(w, c, id, p, DeathCause::Squish, killer) {
        corpse::die_by_squish(w, cp, dir, dir == SquishDir::Up && was_ducking);
    }
    w.sound("env_deathSquish", pos(w, id).x);
    set_mover(w, false);
}

pub fn is_riding_solid(w: &World, c: &Content, id: EntityId, p: &Player, solid: EntityId) -> bool {
    if p.state() == PState::Frozen {
        return false;
    }
    if actor::base_riding_solid(w, c, id, solid) {
        return true;
    }
    let at = pos(w, id);
    let side = |dx: i32| {
        let probe = Placed {
            pos: at + Vec2::new(dx as f32, 0.0),
            collider: w.core(id).collider,
        };
        w.check(c, &probe, solid)
    };
    (p.state() == PState::Normal && p.cling != 0 && side(p.cling))
        || (p.state() == PState::LedgeGrab && side(p.facing))
}

// ------------------------------------------------------------ hats §15

fn head_sprite(c: &Content, p: &Player) -> crate::content::SpriteId {
    let Some(a) = c.archers.get(p.archer as usize) else {
        return crate::content::SpriteId::NONE;
    };
    match p.hat {
        HatState::NoHat => a.head_no_hat,
        HatState::Normal => a.head_normal,
        HatState::Crown => a.head_crown,
    }
}

/// `LoseHat(arrow, updateHead)`.
fn lose_hat(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    arrow_speed: Option<Vec2>,
    update_head: bool,
) {
    if p.hat == HatState::NoHat {
        return;
    }
    let at = pos(w, id) + Vec2::new(0.0, -7.0);
    let archer = c.archers.get(p.archer as usize);
    let spawn = match p.hat {
        HatState::Crown => true,
        // Not for an archer without a hat texture (e.g. the White base
        // archer; placeholder archers without any art keep theirs).
        HatState::Normal => archer.is_some_and(|a| {
            !a.start_no_hat && (a.hat_texture.is_some() || a.body == crate::content::SpriteId::NONE)
        }),
        HatState::NoHat => false,
    };
    if spawn {
        let material = archer
            .map(|a| a.hat_material)
            .unwrap_or(crate::content::HatMaterial::Default);
        let hid = hat::create(
            w,
            p.archer,
            p.hat,
            material,
            at,
            arrow_speed,
            p.facing < 0,
            p.index,
        );
        // Hat `Added`: crown and default hat spawned inside a solid with room
        // 5 px below drop by 5 px.
        let floaty = p.hat == HatState::Crown || material == crate::content::HatMaterial::Default;
        let hp = w.core(hid).pos;
        if floaty
            && w.check_at(c, hid, hp, Tag::Solid)
            && !w.check_at(c, hid, hp + Vec2::new(0.0, 5.0), Tag::Solid)
        {
            w.core_mut(hid).pos.y += 5.0;
        }
        if arrow_speed.is_some() {
            let name = if p.hat == HatState::Crown {
                "char_crownFall"
            } else {
                "char_hatFall"
            };
            w.sound(name, at.x);
        }
    }
    if !p.dead && update_head {
        p.hat = HatState::NoHat;
        p.head = SpriteAnim::default();
    }
}

/// `DoHatPickupCheck` (§15.2).
fn hat_pickup_check(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    let Some(h) = w.first(c, &w.placed(id), Tag::Hat, id) else {
        return;
    };
    let Some(Kind::Hat(hd)) = w.kind(h).copied() else {
        return;
    };
    if p.hat == hd.hat || !(hd.hat == HatState::Crown || hd.archer == p.archer) {
        return;
    }
    if hd.hat == HatState::Crown {
        if hd.owner != p.index {
            crate::versus::stat(w, p.index, |s| s.crowns_picked_up += 1);
        }
    } else if p.hat == HatState::NoHat {
        crate::versus::stat(w, p.index, |s| s.hats_picked_up += 1);
    }
    if p.hat != HatState::NoHat {
        lose_hat(w, c, id, p, None, false);
    }
    p.hat = hd.hat;
    p.head = SpriteAnim::default();
    w.remove(h);
    w.core_mut(h).collidable = false;
    w.sound("pu_hat", pos(w, id).x);
}

/// Shot-off test (§15.3).
fn hat_shot_off_check(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    if p.dead || p.has_shield || p.hat == HatState::NoHat {
        return;
    }
    let top = w.core(id).top() - w.core(id).pos.y;
    let probe = Placed {
        pos: pos(w, id),
        collider: Collider::Box(Hitbox::wrapping(8.0, 7.0, -4.0, top - 7.0)),
    };
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    for a in arrows {
        let Some(ar) = w.arrow(a).copied() else {
            continue;
        };
        if ar.cannot_hit == Some(id) || !ar.dangerous() || ar.speed.y >= 2.0 {
            continue;
        }
        if w.check(c, &probe, a) {
            if ar.player_index >= 0 {
                crate::versus::stat(w, ar.player_index as u8, |s| s.hats_shot_off += 1);
            }
            lose_hat(w, c, id, p, Some(ar.speed), true);
            return;
        }
    }
}

// ------------------------------------------------------------ archer vs archer §11

/// `Player.ResolvePlayerCollisions` (after all entity updates).
pub fn resolve_player_collisions(w: &mut World, c: &Content) {
    if w.collide_counter > 0.0 {
        w.collide_counter -= 1.0;
    }
    let players: Vec<EntityId> = w.players().collect();
    let mut saved: Vec<(EntityId, Collider)> = vec![];
    for &id in &players {
        if let Some(tc) = w.core(id).target_collider {
            saved.push((id, w.core(id).collider));
            w.core_mut(id).collider = Collider::Box(tc);
        }
    }
    let alive = |w: &World, id: EntityId| w.player(id).is_some_and(|p| !p.dead);
    let live = |w: &World, id: EntityId| {
        w.player(id)
            .is_some_and(|p| !p.dead && p.state() != PState::Frozen)
    };
    for i in 0..players.len() {
        let a = players[i];
        if !live(w, a) {
            continue;
        }
        for &b in &players[i + 1..] {
            if live(w, a) && live(w, b) && w.check(c, &w.placed(a), b) {
                player_on_player(w, c, a, b);
            }
        }
        if !alive(w, a) {
            continue;
        }
        let sx = w.player(a).map(|p| p.speed.x).unwrap_or(0.0);
        if sx > w.core(a).width() {
            for &b in &players[i + 1..] {
                if !alive(w, a) || !alive(w, b) {
                    continue;
                }
                let probe = Placed {
                    pos: w.core(a).pos - Vec2::new(sx / 2.0, 0.0),
                    collider: w.core(a).collider,
                };
                if w.check(c, &probe, b) {
                    player_on_player(w, c, a, b);
                }
            }
        }
    }
    for (id, col) in saved {
        if w.exists(id) {
            w.core_mut(id).collider = col;
        }
    }
}

fn player_on_player(w: &mut World, c: &Content, a: EntityId, b: EntityId) {
    let Some(Kind::Player(mut pa)) = w.take_kind(a) else {
        return;
    };
    let Some(Kind::Player(mut pb)) = w.take_kind(b) else {
        w.put_kind(a, Kind::Player(pa));
        return;
    };
    pa.invis_opacity = 1.0;
    pb.invis_opacity = 1.0;
    let mut ta = w.core(a).top() - pa.speed.y;
    let mut tb = w.core(b).top() - pb.speed.y;
    if (ta - tb).abs() > 200.0 {
        if ta > tb {
            ta -= 240.0;
        } else {
            tb -= 240.0;
        }
    }
    if (ta - tb).abs() >= 10.0 {
        // Stomp: the higher one is the stomper.
        let a_on_top = ta < tb;
        let (sid, s, vid, v) = if a_on_top {
            (a, &mut pa, b, &mut pb)
        } else {
            (b, &mut pb, a, &mut pa)
        };
        if s.speed.y >= 0.0 || v.speed.y < 0.0 {
            let v_top = w.core(vid).top();
            let friendly = !s.is_enemy(w.core(vid).allegiance);
            let force = v.state() == PState::Ducking && friendly;
            bounced_on(w, c, vid, v, sid, s);
            bounce(w, c, sid, s, v_top, force);
            if w.core(a).allegiance == w.core(b).allegiance
                && w.core(a).allegiance != Allegiance::Neutral
            {
                trade_arrow(w, sid, s, v);
            }
            rumble(w, s, 0.4, 15);
            if s.state() == PState::Dodging {
                s.dodge_curse_satisfied = true;
                crate::versus::stat(w, s.index, |st| st.dodge_stomps += 1);
            } else if s.is_hyper() {
                crate::versus::stat(w, s.index, |st| st.hyper_stomps += 1);
            }
        }
    } else {
        if w.collide_counter <= 0.0 {
            w.sound("sfx_collide", w.core(a).pos.x);
            w.collide_counter = 5.0;
        }
        side_bounce(w, c, a, &mut pa, b, &pb);
        side_bounce(w, c, b, &mut pb, a, &pa);
        let same = w.core(a).allegiance == w.core(b).allegiance
            && w.core(a).allegiance != Allegiance::Neutral;
        if same {
            trade_arrow(w, a, &mut pa, &mut pb);
        } else {
            let ww = w.world_size(c).w;
            let (xa, xb) = (w.core(a).pos.x, w.core(b).pos.x);
            if pa.arrows.is_empty()
                && !pb.arrows.is_empty()
                && pa.move_x == sign_i(actor::wrap_diff_x(xa, xb, ww))
            {
                steal_arrow(w, a, &mut pa, &mut pb);
            } else if pb.arrows.is_empty()
                && !pa.arrows.is_empty()
                && pb.move_x == sign_i(actor::wrap_diff_x(xb, xa, ww))
            {
                steal_arrow(w, b, &mut pb, &mut pa);
            }
        }
    }
    w.put_kind(b, Kind::Player(pb));
    w.put_kind(a, Kind::Player(pa));
}

/// `BouncedOn(bouncer)` (victim side).
fn bounced_on(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    bouncer_id: EntityId,
    bouncer: &Player,
) {
    p.scale = Vec2::new(1.3, 0.7);
    p.speed.y = MAX_FALL;
    if p.is_enemy(w.core(bouncer_id).allegiance) {
        hurt_bounced_on(w, c, id, p, bouncer.index as i8);
    }
}

fn hurt_bounced_on(w: &mut World, c: &Content, id: EntityId, p: &mut Player, killer: i8) {
    if !can_hurt_ignore_dodge(w, id, p) {
        return;
    }
    if p.has_shield {
        lose_shield(w, id, p);
        if killer >= 0 {
            crate::versus::stat(w, killer as u8, |s| s.shields_broken += 1);
        }
        if matches!(p.state(), PState::LedgeGrab | PState::Dodging) {
            set_state(w, c, id, p, PState::Normal);
        }
    } else if let Some(cp) = die(w, c, id, p, DeathCause::JumpedOn, killer) {
        corpse::die_by_jumped_on(w, cp);
    }
}

fn side_bounce(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    other_id: EntityId,
    other: &Player,
) {
    let ww = w.world_size(c).w;
    let mut x = w.core(id).pos.x;
    let mut ox = w.core(other_id).pos.x;
    if (x - ox).abs() >= ww - 40.0 {
        if x > ox {
            x -= ww;
        } else {
            ox -= ww;
        }
    }
    let dir = sign((x - p.speed.x) - (ox - other.speed.x));
    if !solid_at(w, c, id, pos(w, id) + Vec2::new(dir, 0.0)) {
        p.speed.x = if other.state() == PState::Dodging {
            if w.variants_for(other.index as i32).super_dodging {
                dir * 4.5
            } else if other.dodge_sliding {
                dir * 3.6
            } else {
                dir * 2.7
            }
        } else {
            dir * 1.8
        };
    }
    p.scale.x = 1.4;
    rumble(w, p, 0.25, 12);
}

fn trade_arrow(w: &mut World, id: EntityId, p: &mut Player, other: &mut Player) {
    if p.arrows.even_counts(&mut other.arrows) {
        let x = pos(w, id).x;
        csound(w, p, CharSound::ArrowSteal, x);
    }
}

fn steal_arrow(w: &mut World, id: EntityId, thief: &mut Player, victim: &mut Player) {
    if let Some(t) = victim.arrows.use_arrow() {
        crate::versus::stat(w, thief.index, |s| s.arrows_stolen += 1);
        thief.arrows.add_arrows(&[t]);
        let x = pos(w, id).x;
        csound(w, thief, CharSound::ArrowSteal, x);
    }
}

// ------------------------------------------------------------ animation §19

/// `UpdateAnimation` (cosmetic sprite selection).
fn update_animation(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    let _ = w.core(id);
    let Some(a) = c.archers.get(p.archer as usize) else {
        return;
    };
    let Some(body) = c.sprite(a.body) else { return };
    let has = |b: BodyAnim| a.body_anims[b as usize] != u8::MAX;
    let pick = |b: BodyAnim| a.body_anims[b as usize];
    let aim_or = |aim: BodyAnim, base: BodyAnim| {
        if p.aiming && has(aim) {
            pick(aim)
        } else {
            pick(base)
        }
    };
    let mut bow_visible = !body.hide_bow || p.aiming;
    let state = p.state();
    let anim = if state == PState::Dodging && p.dodge_sliding {
        bow_visible = false;
        pick(match p.hat {
            HatState::NoHat => BodyAnim::SlideNoHat,
            HatState::Normal => BodyAnim::SlideNormal,
            HatState::Crown => BodyAnim::SlideCrown,
        })
    } else if state == PState::Dodging {
        bow_visible = false;
        pick(BodyAnim::Dodge)
    } else if state == PState::Ducking {
        bow_visible = false;
        pick(BodyAnim::Duck)
    } else if state == PState::LedgeGrab || (p.cling != 0 && p.speed.y >= 0.0) {
        bow_visible = false;
        pick(BodyAnim::Ledge)
    } else if p.gliding {
        aim_or(BodyAnim::GlideAim, BodyAnim::Glide)
    } else if p.on_ground
        && p.speed.x.abs() <= 0.39
        && (p.aiming || p.move_x == 0 || state == PState::Frozen)
    {
        if has(BodyAnim::StandAim) && (p.aiming || !body.hide_bow) {
            pick(BodyAnim::StandAim)
        } else {
            pick(BodyAnim::Stand)
        }
    } else if p.on_ground {
        aim_or(BodyAnim::RunAim, BodyAnim::Run)
    } else if p.speed.y < 0.0 {
        aim_or(BodyAnim::JumpAim, BodyAnim::Jump)
    } else {
        aim_or(BodyAnim::FallAim, BodyAnim::Fall)
    };
    p.body.play(&body.anims, anim, false);
    p.bow_visible = bow_visible;
    p.head_visible = !(state == PState::Dodging && p.dodge_sliding && !body.slide_head);
    // Head (§19.3).
    let hk = match p.hat {
        HatState::Normal => 0,
        HatState::NoHat => 1,
        HatState::Crown => 2,
    };
    if let Some(head) = c.sprite(head_sprite(c, p)) {
        let s = sign_i(p.input.aim_axis.x);
        let look = if s == -p.facing {
            3
        } else if p.input.aim_axis.y <= -0.5 {
            1
        } else if p.input.aim_axis.y >= 0.5 {
            2
        } else {
            0
        };
        let idx = if state == PState::Ducking {
            12
        } else if p.on_ground || state == PState::LedgeGrab || p.speed.y.abs() < 1.0 {
            look
        } else if p.speed.y < 0.0 {
            4 + look
        } else {
            8 + look
        };
        let ai = a.head_anims[hk][idx];
        p.head.play(&head.anims, ai, false);
    }
    // Bow (§19.4).
    if let Some(bow) = c.sprite(a.bow) {
        let bi = if p.aiming && !p.arrows.is_empty() {
            1
        } else if p.aiming {
            2
        } else {
            0
        };
        p.bow.play(&bow.anims, a.bow_anims[bi], false);
    }
}

/// Facing as an `f32` sign.
pub fn facing_f(p: &Player) -> f32 {
    p.facing as f32
}

// ------------------------------------------------------------ co-op helpers

/// `Bounce(top, forceSuper)` for stomps on enemies and dummies.
pub fn bounce_pub(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    top: f32,
    force_super: bool,
) {
    bounce(w, c, id, p, top, force_super);
}

/// `HurtBouncedOn(killer)` (an enemy landed on the archer's head).
pub fn hurt_bounced_on_pub(w: &mut World, c: &Content, id: EntityId, p: &mut Player, killer: i8) {
    hurt_bounced_on(w, c, id, p, killer);
}

/// `Unfreeze()` for one archer (revive sequence).
pub fn unfreeze(w: &mut World, c: &Content, id: EntityId) {
    w.core_mut(id).flash.stop();
    if let Some(Kind::Player(mut p)) = w.take_kind(id) {
        if p.state() == PState::Frozen {
            set_state(w, c, id, &mut p, PState::Normal);
        }
        w.put_kind(id, Kind::Player(p));
    }
}

// ------------------------------------------------------------ trigger arrows §10.5

/// Registered trigger arrows per archer.
pub const TRIGGER_CAP: usize = 16;

/// `RegisterTriggerArrow(arrow)`.
fn register_trigger_arrow(w: &mut World, id: EntityId, p: &mut Player, a: EntityId) {
    if let Some(ar) = w.arrow_mut(a) {
        ar.extra.detonator = Some(id);
        ar.extra.ping = crate::arrows::TRIGGER_PING;
    }
    // The original list is unbounded; past the cap (16 linked arrows at
    // once, not seen in play) the oldest arrow is unlinked so it stays
    // collectable instead of pinging forever.
    let slot = match p.triggers.iter().position(|t| t.is_none()) {
        Some(k) => k,
        None => {
            if let Some(old) = p.triggers[0]
                && let Some(ar) = w.arrow_mut(old)
            {
                ar.extra.detonator = None;
            }
            p.triggers.copy_within(1.., 0);
            p.triggers.len() - 1
        }
    };
    p.triggers[slot] = Some(a);
    p.can_detonate = 4.0;
}

/// `DetonateTriggerArrows()`.
pub(crate) fn detonate_trigger_arrows(w: &mut World, c: &Content, id: EntityId, p: &mut Player) {
    if p.can_detonate > 0.0 || p.trigger_count() == 0 {
        return;
    }
    let list = p.triggers;
    p.triggers = [None; TRIGGER_CAP];
    for a in list.into_iter().flatten() {
        if let Some(Kind::Arrow(mut ar)) = w.take_kind(a) {
            let mut ctx: crate::hurt::Ctx = Some((id, &mut *p));
            crate::arrows::detonate(w, c, a, &mut ar, &mut ctx);
            if w.exists(a) {
                w.put_kind(a, Kind::Arrow(ar));
            }
        }
    }
    p.did_detonate = true;
}

/// `RemoveAllTriggerArrows()` (on death): the arrows stay as plain ones.
fn remove_all_trigger_arrows(w: &mut World, p: &mut Player) {
    let n = p.trigger_count() as u32;
    if n >= 2 {
        crate::versus::stat(w, p.index, |s| s.trigger_bombs_lost += n);
    }
    for a in p.triggers.into_iter().flatten() {
        if let Some(ar) = w.arrow_mut(a) {
            ar.extra.detonator = None;
        }
    }
    p.triggers = [None; TRIGGER_CAP];
}

/// `Prism.CaptiveForceShatter()` from the jump and dodge code.
fn prism_force_shatter(w: &mut World, id: EntityId, p: &mut Player) {
    if let Some(pr) = p.prism {
        let mut ctx: crate::hurt::Ctx = Some((id, &mut *p));
        crate::arrows::prism::force_shatter(w, pr, &mut ctx);
    }
}
