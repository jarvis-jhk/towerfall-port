//! Boss 0, The Amaranth ("Wretched Seer", darkworld.md §7.1): an eye with
//! a rotating 172-degree shield, slow homing-free shots, beams once hurt,
//! rotating platforms that reverse and (Legendary) the miasma.

use super::Part;
use super::boss::{self, Base, BossKind, BossState, Seq};
use crate::arrow::{self, Arrow};
use crate::checksum::{Hasher, StableHash};
use crate::collide::Hitbox;
use crate::components::Ease;
use crate::content::Content;
use crate::entity::{EntityId, Kind, Tag};
use crate::math::{PI, Vec2, angle_diff, angle_to_vector, approach_vec};
use crate::player::Player;
use crate::world::World;

pub const HALF_ARC: f32 = 1.500_983_2;
pub const SHIELD_RANGE_SQ: f32 = 1296.0;
const BASE_SPEED: f32 = 0.006_98;
const TARGETS: [Vec2; 5] = [
    Vec2::new(160.0, 120.0),
    Vec2::new(130.0, 120.0),
    Vec2::new(190.0, 120.0),
    Vec2::new(160.0, 90.0),
    Vec2::new(160.0, 140.0),
];

pub const INTRO: u8 = 0;
pub const NORMAL: u8 = 1;
pub const DEAD: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub state: u8,
    pub speed: Vec2,
    /// Shield direction (rad), signed speed, target magnitude sign, the
    /// multiplier (beams reverse it) and the lock counter.
    pub shield: f32,
    pub shield_speed: f32,
    pub shield_dir: f32,
    pub shield_mult: f32,
    pub shield_lock: f32,
    /// Shield grow 0..1 (intro).
    pub shield_scale: f32,
    pub move_to: usize,
    pub idle: f32,
    pub look: f32,
    /// Next shot leans to this side.
    pub lean: f32,
    pub intro: Seq,
    pub shoot: Seq,
    pub beam: Seq,
    pub beams: [Option<EntityId>; 3],
    pub platforms: Seq,
    pub plat_from: f32,
    pub plat_to: f32,
    pub plat_t: f32,
    pub miasma: Option<[EntityId; 2]>,
    pub death: Seq,
    pub shake: f32,
}

impl StableHash for State {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.state);
        h.vec2(self.speed);
        h.f32(self.shield);
        h.f32(self.shield_speed);
        h.f32(self.shield_dir);
        h.f32(self.shield_mult);
        h.f32(self.shield_lock);
        h.u32(self.move_to as u32);
        h.f32(self.idle);
        h.f32(self.lean);
        self.intro.stable_hash(h);
        self.shoot.stable_hash(h);
        self.beam.stable_hash(h);
        self.beams.stable_hash(h);
        self.platforms.stable_hash(h);
        h.f32(self.plat_t);
        self.death.stable_hash(h);
    }
}

pub fn spawn(w: &mut World, _c: &Content, mut base: Base) -> BossState {
    base.max_health = 12 + 4 * base.difficulty + 4 * base.players;
    base.health = base.max_health;
    base.stomp_damage = 2;
    let body = boss::add_body(
        w,
        Vec2::new(160.0, 120.0),
        Hitbox::new(40.0, 28.0, -20.0, -12.0),
        Some(Hitbox::new(32.0, 32.0, -16.0, -16.0)),
        -250,
        &[
            Tag::Enemy,
            Tag::PlayerCollider,
            Tag::Target,
            Tag::ExplosionCollider,
            Tag::LightSource,
        ],
    );
    w.core_mut(body).collidable = false;
    let dir = if w.rng.next_max(2) == 0 { 1.0 } else { -1.0 };
    BossState {
        id: 0,
        body,
        base,
        kind: BossKind::Amaranth(State {
            state: INTRO,
            speed: Vec2::ZERO,
            shield: 29.8,
            shield_speed: 0.0,
            shield_dir: dir,
            shield_mult: 1.0,
            shield_lock: 0.0,
            shield_scale: 0.0,
            move_to: 0,
            idle: 0.0,
            look: 0.0,
            lean: dir,
            intro: Seq::start(0),
            shoot: Seq::default(),
            beam: Seq::default(),
            beams: [None; 3],
            platforms: Seq::default(),
            plat_from: 1.0,
            plat_to: 1.0,
            plat_t: -1.0,
            miasma: None,
            death: Seq::default(),
            shake: 0.0,
        }),
    }
}

fn in_arc(s: &State, center: Vec2, p: Vec2) -> bool {
    let d = p - center;
    d.length_sq() <= SHIELD_RANGE_SQ && angle_diff(s.shield, d.angle()).abs() <= HALF_ARC
}

fn bearing_in_arc(s: &State, center: Vec2, from: Vec2) -> bool {
    angle_diff(s.shield, (from - center).angle()).abs() <= HALF_ARC
}

pub fn update(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &mut Base) {
    let tm = w.time.time_mult;
    if s.shake > 0.0 {
        s.shake -= tm;
    }
    match s.state {
        INTRO => intro(w, id, s, b, tm),
        NORMAL => {
            shield_update(w, c, id, s, tm);
            movement(w, id, s, tm);
            shooting(w, id, s, b, tm);
            beams(w, id, s, b, tm);
            platforms(w, c, s, b, tm);
            if b.difficulty >= 2 && s.miasma.is_none() && b.health <= b.max_health / 3 {
                s.miasma = Some(crate::versus::miasma::spawn_mode(
                    w,
                    crate::versus::miasma::MiasmaMode::Amaranth,
                ));
            }
            if b.health <= 0 {
                die(w, id, s);
            }
        }
        _ => death(w, id, s, tm),
    }
    // Beam angles follow the shield (opposite side).
    let n = s.beams.iter().flatten().count() as i32;
    for (k, bm) in s.beams.iter().enumerate() {
        if let Some(e) = *bm
            && let Some(Part::AmaBeam { t, .. }) = super::part::get(w, e)
        {
            let spread = match (b.difficulty, n) {
                (_, 1) => 0.0,
                (1, _) => [-0.436, 0.436, 0.0][k],
                _ => [-2.0 * PI / 9.0, 0.0, 2.0 * PI / 9.0][k],
            };
            super::part::set(
                w,
                e,
                Part::AmaBeam {
                    t,
                    angle: s.shield + PI + spread,
                },
            );
            let at = w.core(id).pos;
            w.core_mut(e).pos = at;
        }
    }
}

fn intro(w: &mut World, id: EntityId, s: &mut State, b: &mut Base, tm: f32) {
    if !s.intro.ready(tm) {
        if s.intro.step == 2 {
            // Shield grows and rotates in over 240 ticks.
            s.intro.k += 1;
            let t = Ease::CubeOut.apply((s.intro.k as f32 / 240.0).min(1.0));
            s.shield_scale = t;
            s.shield = 29.8 + (-PI / 2.0 - 29.8) * t;
        }
        return;
    }
    match s.intro.step {
        0 => {
            if super::round_started(w) {
                s.intro.wait(180.0, 1);
            }
        }
        1 => {
            boss::enter_boss_mode(w, b.max_health);
            w.sound("en_tentacleBossAppear", 160.0);
            s.intro.k = 0;
            s.intro.wait(200.0, 2);
        }
        2 => {
            boss::shake_level(w, 30.0);
            s.intro.wait(40.0, 3);
        }
        _ => {
            s.shield = -PI / 2.0;
            s.shield_scale = 1.0;
            s.state = NORMAL;
            w.core_mut(id).collidable = true;
            s.shoot = Seq::start(0);
            s.shoot.wait(120.0, 1);
            s.beam = Seq::start(0);
            s.beam.wait(240.0, 1);
            s.platforms = Seq::start(0);
            s.move_to = 0;
            s.idle = 0.0;
        }
    }
}

fn shield_update(w: &mut World, c: &Content, id: EntityId, s: &mut State, tm: f32) {
    if s.shield_lock > 0.0 {
        s.shield_lock -= tm;
    } else {
        let target = BASE_SPEED * s.shield_dir * s.shield_mult;
        s.shield_speed = crate::math::approach(s.shield_speed, target, 0.001_745 * tm);
    }
    s.shield += s.shield_speed * tm;
    let center = w.core(id).pos;
    // Players inside the arc burn.
    for (pid, at) in boss::living(w) {
        if in_arc(s, center, at) {
            boss::hurt_player_id(w, c, pid, center);
            s.shield_speed = 0.0;
            s.shield_lock = 10.0;
        }
    }
    // Live arrows inside the arc are deflected.
    let arrows: Vec<EntityId> = w.tagged(Tag::Arrow).to_vec();
    for a in arrows {
        let Some(ar) = w.arrow(a).copied() else {
            continue;
        };
        if !ar.dangerous()
            || ar.state != arrow::ArrowState::Shooting
            || !in_arc(s, center, w.core(a).pos)
        {
            continue;
        }
        deflect(w, id, s, a);
    }
}

fn deflect(w: &mut World, id: EntityId, s: &mut State, a: EntityId) {
    let center = w.core(id).pos;
    let at = w.core(a).pos;
    let push = w.arrow(a).map_or(Vec2::ZERO, |ar| ar.speed * 0.5);
    s.speed += push;
    crate::arrows::with_arrow(w, a, |w, ar| {
        arrow::enter_fall_mode(w, a, ar, true, false, true);
        ar.speed = (at - center).safe_normalize();
    });
    s.shield_speed = 0.0;
    s.shield_lock = 10.0;
    w.sound("en_tentacleBossShieldDeflect", center.x);
}

fn movement(w: &mut World, id: EntityId, s: &mut State, tm: f32) {
    let pos = w.core(id).pos;
    let target = TARGETS[s.move_to];
    let d = target - pos;
    if s.speed.length() >= 0.5 {
        s.speed = approach_vec(s.speed, Vec2::ZERO, 0.2 * tm);
    } else if d.length_sq() > 100.0 {
        let want = if d.length() > 0.5 {
            d.safe_normalize() * 0.5
        } else {
            d
        };
        s.speed = approach_vec(s.speed, want, 0.03 * tm);
    } else {
        s.speed = approach_vec(s.speed, Vec2::ZERO, 0.03 * tm);
        s.idle += tm;
        if s.idle >= 60.0 && s.idle - tm < 60.0 || s.idle >= 120.0 && s.idle - tm < 120.0 {
            let turn = (40.0 + w.rng.next_float() * 140.0) * PI / 180.0;
            s.look += if w.rng.next_max(2) == 0 { turn } else { -turn };
        }
        if s.idle >= 180.0 {
            s.idle = 0.0;
            let mut n = w.rng.choose_index(TARGETS.len() - 1);
            if n >= s.move_to {
                n += 1;
            }
            s.move_to = n;
        }
    }
    w.core_mut(id).pos = pos + s.speed * tm;
}

fn shooting(w: &mut World, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    if !s.shoot.ready(tm) {
        return;
    }
    let add = if b.health <= b.max_health / 3 {
        2
    } else if b.health <= b.max_health * 2 / 3 {
        1
    } else {
        0
    };
    match s.shoot.step {
        1 => {
            s.shoot.k = 2 + b.difficulty + add;
            s.shoot.next(2);
        }
        2 => {
            if s.shoot.k <= 0 {
                s.shoot
                    .wait((120 - 10 * (2 * b.difficulty + add)).max(0) as f32, 1);
                return;
            }
            s.shoot.k -= 1;
            if let Some((_, at)) = boss::random_target(w) {
                let center = w.core(id).pos;
                let aim = (at - center).angle();
                let off = -w.rng.next_float() * 45.0 * s.lean * PI / 180.0;
                s.lean = -s.lean;
                let a = aim + off;
                super::part::add(
                    w,
                    center + angle_to_vector(a, 24.0),
                    Hitbox::new(12.0, 6.0, -6.0, -2.0),
                    -240,
                    &[Tag::PlayerCollider, Tag::Target, Tag::ExplosionCollider],
                    Part::AmaShot {
                        t: 0.0,
                        heading: a,
                        grace: 30.0,
                    },
                );
                w.sound("en_tentacleBossProjectileSpawn", center.x);
            }
            s.shoot.wait(40.0, 2);
        }
        _ => s.shoot.next(1),
    }
}

fn beams(w: &mut World, id: EntityId, s: &mut State, b: &Base, tm: f32) {
    if !s.beam.ready(tm) {
        return;
    }
    match s.beam.step {
        1 => {
            let gate = if b.difficulty == 0 {
                b.max_health / 3
            } else {
                b.max_health * 2 / 3
            };
            if b.health <= gate {
                s.beam.wait(30.0, 2);
            }
        }
        2 => {
            let center = w.core(id).pos;
            w.sound("en_tentacleBossBeam", center.x);
            let n = (b.difficulty.clamp(0, 2) + 1) as usize;
            for k in 0..3 {
                s.beams[k] = (k < n).then(|| {
                    super::part::add(
                        w,
                        center,
                        Hitbox::new(0.0, 0.0, 0.0, 0.0),
                        -260,
                        &[],
                        Part::AmaBeam {
                            t: 0.0,
                            angle: s.shield + PI,
                        },
                    )
                });
            }
            s.beam.wait(60.0, 3);
        }
        3 => {
            // Lunge along the shield, the shield turns the other way.
            s.speed = angle_to_vector(s.shield, 2.5);
            boss::shake_level(w, 30.0);
            s.shield_mult = [-1.0, -1.5, -2.0][b.difficulty.clamp(0, 2) as usize];
            s.beam.next(4);
        }
        4 => {
            let alive = s.beams[0].is_some_and(|e| w.exists(e));
            if !alive {
                s.beams = [None; 3];
                s.shield_mult = 1.0;
                s.beam.wait(30.0, 5);
            }
        }
        // wait(360), then the loop starts over with the 30-tick reddening.
        5 => s.beam.wait(360.0, 6),
        _ => s.beam.wait(30.0, 2),
    }
}

fn platforms(w: &mut World, c: &Content, s: &mut State, b: &Base, tm: f32) {
    if b.difficulty < 1 {
        return;
    }
    if s.plat_t >= 0.0 {
        s.plat_t += tm;
        let e = Ease::CubeInOut.apply((s.plat_t / 120.0).min(1.0));
        let m = s.plat_from + (s.plat_to - s.plat_from) * e;
        set_platform_mult(w, m);
        if s.plat_t >= 120.0 {
            s.plat_t = -1.0;
        }
    }
    let _ = c;
    if !s.platforms.ready(tm) {
        return;
    }
    match s.platforms.step {
        0 => {
            if b.health <= b.max_health * 2 / 3 {
                s.platforms.wait(600.0, 1);
            }
        }
        1 => {
            s.plat_from = 1.0;
            s.plat_to = -1.0;
            s.plat_t = 0.0;
            s.platforms.wait(600.0 + 120.0, 2);
        }
        _ => {
            s.plat_from = -1.0;
            s.plat_to = 1.0;
            s.plat_t = 0.0;
            s.platforms.wait(600.0 + 120.0, 1);
        }
    }
}

fn set_platform_mult(w: &mut World, m: f32) {
    let ids: Vec<EntityId> = w.entities().collect();
    for e in ids {
        if let Some(Kind::Tower(crate::entities::Tower::RotatePlatform(r))) = w.kind_mut(e) {
            r.speed_mult = m;
        }
    }
}

fn die(w: &mut World, id: EntityId, s: &mut State) {
    s.state = DEAD;
    w.core_mut(id).collidable = false;
    w.tag(id, Tag::Enemy, false);
    if let Some(m) = s.miasma.take() {
        crate::versus::miasma::dissipate(w, m);
    }
    // Shots break, beams fade.
    let parts: Vec<EntityId> = w
        .entities()
        .filter(|&e| {
            matches!(
                super::part::get(w, e),
                Some(Part::AmaShot { .. } | Part::AmaBeam { .. })
            )
        })
        .collect();
    for e in parts {
        w.remove(e);
    }
    s.beams = [None; 3];
    boss::shake_level(w, 60.0);
    s.plat_from = 1.0;
    s.plat_to = 0.0;
    s.plat_t = 0.0;
    s.death = Seq::start(0);
    s.death.wait(30.0, 1);
}

fn death(w: &mut World, id: EntityId, s: &mut State, tm: f32) {
    if s.plat_t >= 0.0 {
        s.plat_t += tm;
        set_platform_mult(w, 1.0 - (s.plat_t / 120.0).min(1.0));
        if s.plat_t >= 120.0 {
            s.plat_t = -1.0;
        }
    }
    if !s.death.ready(tm) {
        return;
    }
    match s.death.step {
        1 => {
            w.sound("en_tentacleBossDeath", 160.0);
            s.death.wait(30.0 + 60.0 + 60.0, 2);
        }
        2 => s.death.wait(360.0, 3),
        _ => {
            super::boss_finished(w);
            w.remove(id);
            s.death.stop();
        }
    }
}

// ------------------------------------------------------------ hits

/// A stomp's horizontal shove (the unreachable `+8` branch omitted).
pub fn shove(player_x: f32, boss_x: f32) -> f32 {
    if player_x < boss_x - 8.0 {
        -3.0
    } else if player_x < boss_x - 3.0 {
        -2.0
    } else if player_x > boss_x + 3.0 {
        2.0
    } else {
        0.0
    }
}

fn damage(
    w: &mut World,
    id: EntityId,
    s: &mut State,
    b: &mut Base,
    dmg: i32,
    killer: i8,
    from: Vec2,
) {
    let center = w.core(id).pos;
    boss::hurt(w, b, dmg, killer);
    w.sound("en_tentacleBossDamage", center.x);
    // The shield spins away from the hit at 3 degrees/tick for 10 ticks.
    let side = angle_diff((from - center).angle(), s.shield).signum();
    s.shield_speed = PI / 60.0 * if side == 0.0 { 1.0 } else { side };
    s.shield_lock = 10.0;
    s.shake = 30.0;
    b.flash = 20.0;
    w.screen_shake(20.0);
}

pub fn body_arrow(w: &mut World, _c: &Content, id: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    if !a.dangerous() {
        return false;
    }
    let from = w.core(aid).pos - a.speed;
    let r = boss::with_boss(w, |w, bs| {
        let (BossKind::Amaranth(s), base) = (&mut bs.kind, &mut bs.base) else {
            return false;
        };
        if s.state != NORMAL {
            return false;
        }
        let center = w.core(id).pos;
        if bearing_in_arc(s, center, from) {
            s.speed += a.speed * 0.5;
            arrow::enter_fall_mode(w, aid, a, true, false, true);
            a.speed = (from - center).safe_normalize();
            s.shield_speed = 0.0;
            s.shield_lock = 10.0;
            w.sound("en_tentacleBossShieldDeflect", center.x);
        } else {
            damage(w, id, s, base, 1, a.player_index, from);
            arrow::enter_fall_mode(w, aid, a, true, false, false);
        }
        true
    });
    r.unwrap_or(false)
}

pub fn body_touch(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    let stomp = boss::is_stomp(w, id, pid, p);
    let at = w.core(pid).pos;
    boss::with_boss(w, |w, bs| {
        let (BossKind::Amaranth(s), base) = (&mut bs.kind, &mut bs.base) else {
            return;
        };
        if s.state != NORMAL {
            return;
        }
        let center = w.core(id).pos;
        let inside = bearing_in_arc(s, center, at);
        if stomp {
            boss::bounce(w, c, id, pid, p);
            p.speed.x = shove(at.x, center.x);
            if inside {
                boss::hurt_player(w, c, pid, p, center);
                s.shield_speed = 0.0;
                s.shield_lock = 10.0;
            } else {
                let dmg = boss::stomp_damage(base);
                damage(w, id, s, base, dmg, p.index as i8, at);
            }
        } else if inside {
            boss::hurt_player(w, c, pid, p, center);
        } else {
            s.speed = (center - at).safe_normalize() * 2.0;
        }
    });
}

/// A blast in its kill radius: 1 damage (the shield does not cover it).
pub fn body_explode(w: &mut World, id: EntityId, owner: i8) {
    boss::with_boss(w, |w, bs| {
        if let (BossKind::Amaranth(s), base) = (&mut bs.kind, &mut bs.base)
            && s.state == NORMAL
        {
            let at = w.core(id).pos + Vec2::new(0.0, -20.0);
            damage(w, id, s, base, 1, owner, at);
        }
    });
}

// ------------------------------------------------------------ parts

pub fn part_update(w: &mut World, c: &Content, id: EntityId, p: &mut Part) {
    let tm = w.time.time_mult;
    match p {
        Part::AmaShot { t, heading, grace } => {
            *t += tm;
            if *grace > 0.0 {
                *grace -= tm;
                // A player inside keeps the grace running.
                let me = w.placed(id);
                if w.any(c, &me, Tag::Player, id) {
                    *grace = 30.0;
                }
            }
            let e = Ease::CubeInOut.apply((*t / 60.0).min(1.0));
            let speed = 1.0 + (0.4 - 1.0) * e;
            let pos = w.core(id).pos + angle_to_vector(*heading, speed * tm);
            w.core_mut(id).pos = pos;
            if !(-40.0..=360.0).contains(&pos.x) || !(-40.0..=280.0).contains(&pos.y) {
                w.remove(id);
            }
        }
        Part::AmaBeam { t, angle } => {
            *t += tm;
            if *t >= 160.0 {
                w.remove(id);
                return;
            }
            if *t < 60.0 {
                return;
            }
            let origin = w.core(id).pos + angle_to_vector(*angle, 16.0);
            let end = origin + angle_to_vector(*angle, 400.0);
            let lvl = c.level(w.level);
            let ws = w.world_size(c);
            let victims: Vec<EntityId> = boss::living(w)
                .into_iter()
                .filter(|&(pid, _)| {
                    let placed = w.placed(pid);
                    crate::collide::collide_segment(&placed, origin, end, lvl, ws)
                })
                .map(|(pid, _)| pid)
                .collect();
            for pid in victims {
                boss::hurt_player_id(w, c, pid, origin);
                crate::hurt::with_player(w, pid, |w, pl| crate::entities::start_fire(w, pl));
            }
        }
        _ => {}
    }
}

pub fn shot_touch(w: &mut World, c: &Content, id: EntityId, pid: EntityId, p: &mut Player) {
    let Some(Part::AmaShot { grace, .. }) = super::part::get(w, id) else {
        return;
    };
    if grace > 0.0 {
        return;
    }
    let me = w.core(id);
    let top = me.top();
    let pc = w.core(pid);
    if p.speed.y >= 0.0 && pc.bottom() - p.speed.y <= top + 6.0 {
        crate::player::bounce_pub(w, c, pid, p, top, false);
    } else {
        boss::hurt_player(w, c, pid, p, w.core(id).pos);
    }
    w.sound("en_tentacleBossProjectileKill", w.core(id).pos.x);
    w.remove(id);
    w.core_mut(id).collidable = false;
}

pub fn shot_arrow(w: &mut World, _c: &Content, id: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    let Some(Part::AmaShot { grace, .. }) = super::part::get(w, id) else {
        return false;
    };
    if grace > 0.0 {
        return false;
    }
    w.remove(id);
    w.core_mut(id).collidable = false;
    arrow::enter_fall_mode(w, aid, a, true, false, false);
    true
}
