//! King Reaper, the Quest boss (enemies.md §4.13), with its crystals,
//! bombs and beams (`hazard.rs` entities driven from here).

use super::hazard::{Hazard, HazardType};
use super::*;
use crate::components::Ease;
use crate::math::{angle_to_vector, approach, lerp, min};

pub const APPEAR: u8 = 0;
pub const ATTACK: u8 = 1;
pub const HURT: u8 = 2;
pub const DISAPPEAR: u8 = 3;
pub const DEAD: u8 = 4;

const SHIELD: usize = 0;
const PHASE: usize = 1;
const MOVE_MULT: usize = 2;
const MOVE_TARGET: usize = 3;
const TIMER: usize = 4;
const DIFF: usize = 0;
const THROWS: usize = 1;
const CYCLE: usize = 2;
pub const F_FINAL: u16 = F_TYPE;
pub const F_INTRO: u16 = F_TYPE << 1;
/// Tower 11 variant: boss skeletons have wings, two extra portals.
pub const F_WING: u16 = F_TYPE << 2;

pub fn init(core: &mut Core, e: &mut Enemy) {
    e.health = 3;
    e.bounty = 0;
    e.flags |= F_EXPLOSIONS_PUSH;
    core.depth = -300;
    core.collidable = false;
    core.seek = false;
    e.t[MOVE_MULT] = 1.0;
    e.t[MOVE_TARGET] = 1.0;
}

/// Summon the reaper (`<reaper>` group). `from` = the intro king's spot.
pub fn summon(
    w: &mut World,
    c: &Content,
    difficulty: i32,
    final_wave: bool,
    from: Option<Vec2>,
    wing: bool,
    extra_hp: i8,
) -> EntityId {
    let ws = w.world_size(c);
    let centre = Vec2::new(ws.w / 2.0, ws.h / 2.0);
    let id = spawn(w, c, EnemyType::KingReaper, from.unwrap_or(centre), 1, &[]);
    if let Some(Kind::Enemy(e)) = w.kind_mut(id) {
        e.n[DIFF] = difficulty.clamp(0, 4);
        e.set(F_FINAL, final_wave);
        e.set(F_INTRO, from.is_some());
        e.set(F_WING, wing);
        e.health += extra_hp;
        e.target_pos = from.unwrap_or(centre);
    }
    id
}

pub fn shielded(e: &Enemy) -> bool {
    e.t[SHIELD] > 0.0
}

fn activate_shield(e: &mut Enemy) {
    e.t[SHIELD] = 330.0 + 60.0 * e.n[DIFF] as f32 - 30.0 * e.health as f32;
}

fn d(e: &Enemy) -> f32 {
    90.0 - 20.0 * e.n[DIFF] as f32
}

pub fn enter(w: &mut World, _c: &Content, id: EntityId, e: &mut Enemy, s: u8) {
    let x = w.core(id).pos.x;
    match s {
        ATTACK => {
            let core = w.core_mut(id);
            core.collidable = true;
            core.seek = true;
            e.n[CYCLE] = 0;
        }
        HURT => {
            w.sound("boss_hit", x);
            e.scale = Vec2::new(0.8, 0.8);
            w.screen_shake(20.0);
        }
        DISAPPEAR => {
            w.sound("boss_disappear", x);
            w.screen_shake(20.0);
            w.core_mut(id).collidable = false;
        }
        // The final death: dark ends, the floors die out at once.
        DEAD => {
            w.sound("boss_death", x);
            w.core_mut(id).collidable = false;
            e.t[TIMER] = -1.0;
            e.t[PHASE] = 0.0;
            crate::pickups::orbs::end_dark(w);
            crate::quest::floor::die_out_all(w);
        }
        _ => {}
    }
}

fn living_players(w: &World) -> usize {
    w.players()
        .filter(|&p| w.player(p).is_some_and(|q| !q.dead))
        .count()
}

pub fn update(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) -> u8 {
    let tm = w.time.time_mult;
    let ws = w.world_size(c);
    let centre = Vec2::new(ws.w / 2.0, ws.h / 2.0);
    match e.state {
        APPEAR => {
            if e.has(F_INTRO) {
                e.t[TIMER] += tm;
                let k = Ease::CubeInOut.apply((e.t[TIMER] / 180.0).min(1.0));
                let p = e.target_pos + (centre - e.target_pos) * k;
                w.core_mut(id).pos = p;
            }
            e.speed = Vec2::ZERO;
            APPEAR
        }
        ATTACK | HURT => {
            if e.state == ATTACK && e.t[SHIELD] > 0.0 && living_players(w) > 0 {
                e.t[SHIELD] -= tm;
            }
            if e.state == HURT {
                approach_speed(e, Vec2::ZERO, 0.3 * tm);
                return HURT;
            }
            let period = 240.0 - 20.0 * e.n[DIFF] as f32;
            e.t[PHASE] += PI * 2.0 / period * tm;
            e.t[MOVE_MULT] = approach(e.t[MOVE_MULT], e.t[MOVE_TARGET], 0.04 * tm);
            let a = e.t[PHASE];
            let desired = centre
                + Vec2::new(16.0 * crate::math::sin(a), 12.0 * crate::math::sin(2.0 * a))
                    * e.t[MOVE_MULT];
            let mut v = shortest(w, c, w.core(id).pos, desired);
            if v.length() > 2.0 {
                v = v.safe_normalize() * 2.0;
            }
            approach_speed(e, v, 0.8 * tm);
            ATTACK
        }
        DISAPPEAR => {
            approach_speed(e, Vec2::ZERO, 0.1 * tm);
            DISAPPEAR
        }
        _ => {
            // Drift to the centre; count the death animation's ticks.
            let mut v = shortest(w, c, w.core(id).pos, centre);
            if v.length_sq() > 4.0 {
                v = v.safe_normalize() * 2.0;
            }
            approach_speed(e, v, 0.8 * tm);
            if e.t[TIMER] >= 0.0 {
                e.t[TIMER] += tm;
            }
            DEAD
        }
    }
}

/// Portals the reaper summons from (names in the level).
fn portal_pool(w: &World, c: &Content, e: &Enemy) -> Vec<EntityId> {
    let mut names = vec!["BotLeft", "BotRight", "MidLeft", "MidRight"];
    if e.has(F_WING) {
        names.extend(["BotMidLeft", "BotMidRight"]);
    }
    portal::portals(w, c)
        .into_iter()
        .filter(|(_, n)| names.contains(&n.as_str()))
        .map(|(id, _)| id)
        .collect()
}

fn spawn_minions(w: &mut World, _c: &Content, e: &Enemy) -> bool {
    let total = enemy_count(w) as i32 - 1 + portal::queued_total(w) as i32;
    let skel = EnemyType::Skeleton(SkelKind {
        arrow: ArrowType::Normal,
        shield: false,
        wings: e.has(F_WING),
        mimic: false,
        jester: false,
        boss: true,
    });
    let (ty, n) = match e.n[DIFF] {
        0 => return false,
        1 if total < 3 => (EnemyType::BossCultist, 1),
        2 if total < 4 => (EnemyType::BossCultist, if total >= 3 { 1 } else { 2 }),
        3 if total < 5 => (EnemyType::BossCultist, if total >= 4 { 1 } else { 2 }),
        4 if total < 2 => (skel, 1),
        _ => return false,
    };
    let pool = portal_pool(w, _c, e);
    if pool.is_empty() {
        return false;
    }
    for _ in 0..n {
        let p = pool[w.rng.choose_index(pool.len())];
        portal::appear_and_spawn(w, p, ty);
    }
    true
}

fn throw_bomb(w: &mut World, c: &Content, id: EntityId, two: Option<f32>) {
    let me = w.core(id).pos;
    let start = me + Vec2::new(0.0, 16.0);
    let ws = w.world_size(c);
    let target = w
        .players()
        .filter(|&p| w.player(p).is_some_and(|q| !q.dead))
        .map(|p| w.core(p).pos)
        .min_by(|a, b| {
            wrap_dist_sq(start, *a, ws.w, ws.h).total_cmp(&wrap_dist_sq(start, *b, ws.w, ws.h))
        });
    let (tp, near_wall) = match target {
        Some(t) => (t, t.x < 30.0 || t.x > ws.w - 30.0),
        None => (start + angle_to_vector(w.rng.next_angle(), 60.0), false),
    };
    let mut a = shortest(w, c, start, tp).angle();
    let mut speed = 3.0;
    if near_wall {
        a += if tp.x < ws.w / 2.0 { 5.0 } else { -5.0 } * PI / 180.0;
        speed *= 1.2;
    } else {
        a += (w.rng.next_float() * 20.0 - 10.0) * PI / 180.0;
    }
    let angles: Vec<f32> = match two {
        Some(off) => vec![a - off, a + off],
        None => vec![a],
    };
    for ang in angles {
        hazard::spawn_bomb(w, start, angle_to_vector(ang, speed), id);
    }
    w.sound("boss_bombThrow", me.x);
}

fn spawn_beams(w: &mut World, id: EntityId, e: &Enemy) {
    let up = -PI / 2.0;
    let angles: Vec<f32> = match e.n[DIFF] {
        0 => vec![up],
        1 => vec![0.0],
        2 => {
            if w.rng.chance(0.5) {
                vec![up, 0.0]
            } else {
                vec![-PI / 4.0, PI / 4.0]
            }
        }
        3 => {
            let extra = if w.rng.next_max(2) == 0 {
                -PI / 4.0
            } else {
                -3.0 * PI / 4.0
            };
            vec![up, 0.0, extra]
        }
        _ => vec![up, 0.0, -PI / 4.0, -3.0 * PI / 4.0],
    };
    for a in angles {
        hazard::spawn_beam(w, id, a);
    }
}

fn crystals(w: &World, id: EntityId) -> Vec<EntityId> {
    w.entities()
        .filter(|&h| matches!(w.kind(h), Some(Kind::Hazard(hz)) if hz.owner == Some(id) && hz.ty == HazardType::ReaperCrystal))
        .collect()
}

fn hazards_of(w: &World, id: EntityId, ty: HazardType) -> Vec<EntityId> {
    w.entities()
        .filter(
            |&h| matches!(w.kind(h), Some(Kind::Hazard(hz)) if hz.owner == Some(id) && hz.ty == ty),
        )
        .collect()
}

pub fn co(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy, step: u16) {
    let x = w.core(id).pos.x;
    match (e.state, step) {
        (APPEAR, 0) => {
            if e.has(F_INTRO) {
                e.t[TIMER] = 0.0;
                e.co.yield_frames(120.0, 1);
            } else {
                w.sound("boss_reappear", x);
                e.co.yield_frames(20.0, 10);
            }
        }
        (APPEAR, 1) => e.co.yield_frames(40.0, 2),
        (APPEAR, 2) => {
            w.sound("boss_headTransform", x);
            w.screen_shake(20.0);
            e.co.yield_frames(20.0, 3);
        }
        (APPEAR, 3) => e.co.yield_frames(60.0, 4),
        (APPEAR, 4) => {
            activate_shield(e);
            e.co.yield_frames(20.0, 99);
        }
        (APPEAR, 10) => {
            w.screen_shake(20.0);
            e.co.yield_frames(20.0, 11);
        }
        (APPEAR, 11) => {
            activate_shield(e);
            e.co.yield_frames(20.0, 99);
        }
        (APPEAR, _) => {
            set_state(w, c, id, e, ATTACK);
            e.co.finish();
        }
        // Attack cycle: SpawnEnemies, LaserCrystals, ThrowBomb, Beam, each followed by wait(D).
        (ATTACK, 0) => {
            if spawn_minions(w, c, e) {
                e.co.yield_frames(20.0, 1);
            } else {
                co(w, c, id, e, 1);
            }
        }
        (ATTACK, 1) => e.co.yield_frames(d(e), 2),
        (ATTACK, 2) => e.co.yield_frames(10.0, 3),
        (ATTACK, 3) => {
            let bolt = e.n[DIFF] >= 4;
            let n = if bolt { 3 } else { 3 + e.n[DIFF] };
            let start = w.rng.next_angle();
            let sgn = if w.rng.next_max(2) == 0 { -1.0 } else { 1.0 };
            let extra = [PI / 4.0, PI / 2.0, 3.0 * PI / 4.0][w.rng.choose_index(3)];
            let sgn2 = if w.rng.next_max(2) == 0 { -1.0 } else { 1.0 };
            let orbit = sgn * 4.0 * PI + sgn2 * extra;
            for i in 0..n {
                let a = start + i as f32 * PI * 2.0 / n as f32;
                let h = hazard::spawn_crystal(w, id, a, bolt);
                if let Some(Kind::Hazard(hz)) = w.kind_mut(h) {
                    hz.t[2] = orbit;
                }
            }
            w.sound("boss_crystalSpawn", x);
            e.co.yield_frames(30.0, 4);
        }
        (ATTACK, 4) => e.co.yield_frames(60.0, 5),
        (ATTACK, 5) => e.co.yield_frames(d(e), 6),
        (ATTACK, 6) => {
            e.n[THROWS] = 0;
            co(w, c, id, e, 7);
        }
        (ATTACK, 7) => e.co.yield_frames(15.0, 8),
        (ATTACK, 8) => {
            e.co.yield_frames(15.0, 9);
        }
        (ATTACK, 9) => {
            let k = e.n[THROWS];
            let (two, total, gap) = match e.n[DIFF] {
                0 => (None, 1, 0.0),
                1 => (None, 2, 40.0),
                2 => (Some(25.0 * PI / 180.0), 1, 0.0),
                3 => (
                    if k == 1 {
                        Some(30.0 * PI / 180.0)
                    } else {
                        None
                    },
                    2,
                    30.0,
                ),
                _ => (None, 3, 10.0),
            };
            throw_bomb(w, c, id, two);
            e.n[THROWS] += 1;
            if e.n[THROWS] < total {
                e.co.yield_frames(gap, 7);
            } else {
                e.co.yield_frames(30.0, 10);
            }
        }
        (ATTACK, 10) => e.co.yield_frames(d(e), 11),
        (ATTACK, 11) => {
            w.sound("boss_beam", x);
            e.t[MOVE_TARGET] = 0.2;
            e.co.yield_frames(30.0, 12);
        }
        (ATTACK, 12) => {
            spawn_beams(w, id, e);
            e.co.yield_frames(60.0, 13);
        }
        (ATTACK, 13) => {
            e.scale = Vec2::new(1.3, 1.3);
            w.screen_shake(20.0);
            e.co.yield_frames(60.0, 14);
        }
        (ATTACK, 14) => {
            e.t[MOVE_TARGET] = 1.0;
            e.scale = Vec2::ONE;
            e.co.yield_frames(d(e), 0);
        }
        (HURT, 0) => e.co.yield_frames(60.0, 1),
        (HURT, _) => {
            activate_shield(e);
            set_state(w, c, id, e, ATTACK);
            e.co.finish();
        }
        (DISAPPEAR, 0) => e.co.yield_frames(30.0, 1),
        (DISAPPEAR, 1) => {
            e.co.yield_frames(30.0, 2);
        }
        (DISAPPEAR, _) => {
            w.remove(id);
            e.co.finish();
        }
        (DEAD, 0) => e.co.yield_frames(60.0, 1),
        // The death animation starts; while it shows frame <= 62 a bomb
        // goes off every 20 ticks around the boss.
        (DEAD, 1) => {
            e.t[TIMER] = 0.0;
            death_check(e);
        }
        (DEAD, 2) => {
            let at = w.core(id).pos;
            e.t[PHASE] += w.rng.range_f(PI / 4.0, PI * 3.0 / 4.0);
            let r = w.rng.range_i(5, 25) as f32;
            let p = at + angle_to_vector(e.t[PHASE], r);
            w.sound("pu_bombArrowExplode", p.x);
            crate::explosion::spawn(w, c, p, -1, false, &mut None);
            death_check(e);
        }
        // Frame 73: the crown drops; the boss goes when the animation ends.
        (DEAD, 3) => {
            if death_frame(e.t[TIMER]) < 73 {
                e.co.yield_frames(0.0, 3);
                return;
            }
            let at = w.core(id).pos + Vec2::new(0.0, -12.0);
            crate::hat::create(
                w,
                0,
                crate::player::HatState::Crown,
                crate::content::HatMaterial::Default,
                at,
                None,
                false,
                u8::MAX,
            );
            e.co.yield_frames(0.0, 4);
        }
        (DEAD, _) => {
            if e.t[TIMER] < DEATH_FRAMES.len() as f32 * DEATH_DELAY {
                e.co.yield_frames(0.0, 4);
                return;
            }
            w.remove(id);
            e.co.finish();
        }
        _ => e.co.finish(),
    }
}

/// The `death` animation (bossSpriteData `KingReaper`): frame numbers at
/// 0.15 s (9 ticks) each.
const DEATH_FRAMES: [u8; 33] = [
    60, 61, 62, 61, 62, 61, 62, 61, 62, 61, 62, 61, 62, 61, 62, 61, 62, 63, 64, 65, 66, 67, 68, 69,
    70, 71, 72, 73, 74, 75, 76, 77, 78,
];
const DEATH_DELAY: f32 = 9.0;

/// The frame number shown `t` ticks into the death animation.
fn death_frame(t: f32) -> u8 {
    let i = ((t / DEATH_DELAY) as usize).min(DEATH_FRAMES.len() - 1);
    DEATH_FRAMES[i]
}

/// Loop head of the death bombs: another bomb in 20 while frame <= 62.
fn death_check(e: &mut Enemy) {
    if death_frame(e.t[TIMER]) <= 62 {
        e.co.yield_frames(20.0, 2);
    } else {
        e.co.yield_frames(0.0, 3);
    }
}

/// Damage: only in Attack without a shield.
pub fn hurt(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    force: Vec2,
    damage: i8,
    killer: i8,
) {
    if e.state != ATTACK || shielded(e) || !e.alive() {
        return;
    }
    e.speed = force;
    e.killer = killer;
    for h in crystals(w, id) {
        w.remove(h);
    }
    // `QuickFade`: every beam stops being deadly and fades out in 10.
    for h in hazards_of(w, id, HazardType::ReaperBeam) {
        if let Some(Kind::Hazard(hz)) = w.kind_mut(h)
            && hz.t[2] <= 0.0
        {
            hz.t[2] = 10.0;
        }
    }
    e.health -= damage;
    if e.health <= 0 {
        super::die(w, c, id, e, killer);
        return;
    }
    set_state(w, c, id, e, HURT);
}

/// `Die`: every other enemy dies, portals close; final -> Dead else Disappear.
pub fn die(w: &mut World, c: &Content, id: EntityId, e: &mut Enemy) {
    kill_all(w, c, id);
    portal::force_cancel_all(w, c);
    let s = if e.has(F_FINAL) { DEAD } else { DISAPPEAR };
    set_state(w, c, id, e, s);
}

pub fn arrow_hit(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    aid: EntityId,
    a: &mut Arrow,
) -> bool {
    if !e.alive() {
        return false;
    }
    if shielded(e) || e.state != ATTACK {
        arrow::enter_fall_mode(w, aid, a, true, false, true);
        return true;
    }
    hurt(w, c, id, e, a.speed, 1, a.player_index);
    arrow::enter_fall_mode(w, aid, a, true, false, false);
    true
}

pub fn touch(
    w: &mut World,
    c: &Content,
    id: EntityId,
    e: &mut Enemy,
    pid: EntityId,
    p: &mut Player,
) {
    if shielded(e) {
        hurt_player(w, c, id, pid, p);
    } else {
        plain_touch(w, c, id, e, pid, p);
    }
}

/// Crystal lifecycle: out to radius 32 (25 ticks), orbit at 31..86, fire at 92.
pub fn crystal_update(w: &mut World, c: &Content, id: EntityId, h: &mut Hazard, _tm: f32) {
    let Some(owner) = h.owner.filter(|&o| w.exists(o)) else {
        w.remove(id);
        return;
    };
    let age = h.t[0];
    let r = 32.0 * Ease::CubeOut.apply(min(age / 25.0, 1.0));
    let orbit = h.t[2] * Ease::CubeInOut.apply(((age - 31.0) / 55.0).clamp(0.0, 1.0));
    let a = h.angle + orbit;
    let centre = w.core(owner).pos;
    w.core_mut(id).pos = centre + angle_to_vector(a, r);
    if age >= 92.0 {
        let t = if h.bolt {
            ArrowType::Bolt
        } else {
            ArrowType::Laser
        };
        let pos = w.core(id).pos;
        shoot_arrow(w, owner, t, pos, a);
        w.remove(id);
        w.core_mut(id).collidable = false;
    }
    let _ = c;
}

/// Beam: 60 ticks of telegraph, then 30 deadly ticks fading out; it
/// follows the boss. A quick fade (`t[2]`, the boss was hurt) is harmless
/// and removes it after 10 ticks.
pub fn beam_update(w: &mut World, c: &Content, id: EntityId, h: &mut Hazard, tm: f32) {
    if let Some(o) = h.owner.filter(|&o| w.exists(o)) {
        w.core_mut(id).pos = w.core(o).pos;
    }
    if h.t[2] > 0.0 {
        h.t[2] -= tm;
        h.alpha = (h.t[2] / 10.0).max(0.0);
        if h.t[2] <= 0.0 {
            w.remove(id);
        }
        return;
    }
    h.t[1] += tm;
    if h.t[1] >= 90.0 {
        w.remove(id);
        return;
    }
    if h.t[1] < 60.0 {
        return;
    }
    let origin = w.core(id).pos;
    let dir = hazard::unit(h.angle);
    let perp = Vec2::new(-dir.y, dir.x);
    let lvl = c.level(w.level);
    let ws = w.world_size(c);
    let victims: Vec<EntityId> = w
        .players()
        .filter(|&p| {
            w.player(p).is_some_and(|q| !q.dead) && {
                let core = w.core(p);
                let placed = Placed {
                    pos: core.pos,
                    collider: core.collider,
                };
                [-4.0, 0.0, 4.0].iter().any(|&o| {
                    let a = origin + perp * o - dir * 400.0;
                    let b = origin + perp * o + dir * 400.0;
                    crate::collide::collide_segment(&placed, a, b, lvl, ws)
                })
            }
        })
        .collect();
    // Touching it sets the archer alight, and hurts it if it can be hurt.
    for pid in victims {
        if let Some(Kind::Player(mut p)) = w.take_kind(pid) {
            crate::entities::start_fire(w, &p);
            if player::can_hurt(w, pid, &p) {
                player::hurt(w, c, pid, &mut p, DeathCause::Enemy, origin, -1, None);
            }
            w.put_kind(pid, Kind::Player(p));
        }
    }
    let _ = lerp(0.0, 0.0, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn death_animation_frames() {
        assert_eq!(death_frame(0.0), 60);
        assert_eq!(death_frame(16.0 * 9.0), 62);
        assert_eq!(death_frame(17.0 * 9.0), 63);
        assert_eq!(death_frame(27.0 * 9.0), 73);
        assert_eq!(death_frame(10_000.0), 78);
        // The bomb loop: checked at 0, 21, 42, ...: frame <= 62 until 153.
        let bombs = (0..)
            .take_while(|k| death_frame(*k as f32 * 21.0) <= 62)
            .count();
        assert_eq!(bombs, 8);
    }
}
