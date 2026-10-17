//! Boss 1, Dreadwood ("Lady Abigail", darkworld.md §7.2): a leaf ceiling
//! with three eyes (the only damageable parts, sharing the boss pool),
//! floor tentacles, falling bramble spores and two intermissions with
//! enemy waves. The control body is invisible, untagged and harmless.

use super::Part;
use super::boss::{self, Base, BossKind, BossState, Seq};
use crate::arrow::{self, Arrow};
use crate::checksum::{Hasher, StableHash};
use crate::collide::Hitbox;
use crate::components::Ease;
use crate::content::Content;
use crate::enemies::{EnemyType, portal};
use crate::entity::{EntityId, Tag};
use crate::math::{PI, Vec2};
use crate::player::Player;
use crate::quiver::ArrowType;
use crate::world::World;

pub const MAX_EYES: usize = 3;
pub const MAX_TENTACLES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Eye {
    pub id: Option<EntityId>,
    pub open: bool,
    pub damaged: bool,
    pub flash: f32,
    /// Hits during this opening (the second+ plays the secondary sound).
    pub hits: u8,
    pub critical: bool,
    pub lodged: [Option<ArrowType>; 4],
    pub burst: bool,
    /// Ticks into the burst (stretch 60, wait 40, flash, 20, explosion).
    pub burst_t: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Tentacle {
    pub id: Option<EntityId>,
    pub start: Vec2,
    pub max_add_x: f32,
    pub seq: Seq,
    /// Tween: from, to, ticks, length, easing out?
    pub from: f32,
    pub to: f32,
    pub t: f32,
    pub len: f32,
    pub out: bool,
}

pub const INTRO: u8 = 0;
pub const FIGHT: u8 = 1;
pub const INTERMISSION: u8 = 2;
pub const DEAD: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub phase: u8,
    pub eyes: [Eye; MAX_EYES],
    pub n_eyes: usize,
    pub tentacles: [Tentacle; MAX_TENTACLES],
    pub n_tent: usize,
    /// `nextHealthThreshold`: 2, 1, 0.
    pub threshold: i32,
    pub can_hurt: bool,
    pub intro: Seq,
    pub eye_seq: Seq,
    pub eye_order: [u8; MAX_EYES],
    pub tent_seq: Seq,
    pub tent_order: [u8; MAX_TENTACLES],
    pub spore_seq: Seq,
    pub slots: [f32; 4],
    pub slot_n: usize,
    pub inter: Seq,
    pub sets: [[Option<EnemyType>; 3]; 2],
    pub n_sets: usize,
    /// The set being dispatched: (portal, enemy) per slot, next index.
    pub pend: [Option<(EntityId, EnemyType)>; 3],
    pub pend_i: usize,
    pub spores_left: i32,
    pub death: Seq,
    pub miasma: Option<[EntityId; 2]>,
    /// Background tentacles shown (presentation).
    pub bg_in: bool,
    pub gauntlet: bool,
}

impl StableHash for State {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.phase);
        for e in &self.eyes[..self.n_eyes] {
            h.bool(e.open);
            h.bool(e.damaged);
            h.f32(e.flash);
            h.u8(e.hits);
        }
        for t in &self.tentacles[..self.n_tent] {
            t.seq.stable_hash(h);
            h.f32(t.t);
        }
        h.i32(self.threshold);
        h.bool(self.can_hurt);
        self.intro.stable_hash(h);
        self.eye_seq.stable_hash(h);
        self.tent_seq.stable_hash(h);
        self.spore_seq.stable_hash(h);
        self.inter.stable_hash(h);
        self.death.stable_hash(h);
        h.i32(self.spores_left);
        h.u32(self.pend_i as u32);
    }
}

fn dv(s: &State, b: &Base) -> f32 {
    b.difficulty as f32 + (2 - s.threshold) as f32 * 0.5
}

pub fn spawn(w: &mut World, c: &Content, mut base: Base) -> BossState {
    base.max_health = 20 + 4 * base.difficulty + 4 * base.players;
    base.health = base.max_health;
    base.midway_auto = false;
    // The control: invisible, harmless, not an Enemy.
    let body = boss::add_body(
        w,
        Vec2::new(160.0, 20.0),
        Hitbox::new(0.0, 0.0, 0.0, 0.0),
        None,
        -250,
        &[],
    );
    w.core_mut(body).collidable = false;
    w.core_mut(body).visible = false;
    w.core_mut(body).seek = false;
    let markers = c.level(w.level).markers.boss_markers.clone();
    let mut s = State {
        phase: INTRO,
        eyes: [Eye::default(); MAX_EYES],
        n_eyes: 0,
        tentacles: [Tentacle::default(); MAX_TENTACLES],
        n_tent: 0,
        threshold: 2,
        can_hurt: true,
        intro: Seq::start(0),
        eye_seq: Seq::default(),
        eye_order: [0, 1, 2],
        tent_seq: Seq::default(),
        tent_order: [0, 1, 2],
        spore_seq: Seq::default(),
        slots: [65.0, 115.0, 165.0, 215.0],
        slot_n: 0,
        inter: Seq::default(),
        sets: [[None; 3]; 2],
        n_sets: 0,
        pend: [None; 3],
        pend_i: 0,
        spores_left: 0,
        death: Seq::default(),
        miasma: None,
        bg_in: false,
        gauntlet: w.dark_world.as_ref().is_some_and(|d| d.settings.tower == 4),
    };
    for m in &markers {
        if m.pos.y < 120.0 && s.n_eyes < MAX_EYES {
            let id = super::part::add(
                w,
                m.pos,
                Hitbox::new(30.0, 8.0, -15.0, 0.0),
                -69,
                &[
                    Tag::Enemy,
                    Tag::PlayerCollider,
                    Tag::Target,
                    Tag::ExplosionCollider,
                    Tag::LightSource,
                ],
                Part::Eye(s.n_eyes as u8),
            );
            s.eyes[s.n_eyes].id = Some(id);
            s.n_eyes += 1;
        } else if m.pos.y >= 120.0 && s.n_tent < MAX_TENTACLES {
            let max_add_x = m.nodes.first().map_or(0.0, |n| (n.x - m.pos.x).max(0.0));
            let id = super::part::add(
                w,
                m.pos,
                Hitbox::new(12.0, 200.0, -6.0, 10.0),
                -60,
                &[Tag::PlayerCollider, Tag::Target],
                Part::Tentacle(s.n_tent as u8),
            );
            w.core_mut(id).collidable = false;
            s.tentacles[s.n_tent] = Tentacle {
                id: Some(id),
                start: m.pos,
                max_add_x,
                ..Default::default()
            };
            s.n_tent += 1;
        }
    }
    BossState {
        id: 1,
        body,
        base,
        kind: BossKind::Dreadwood(s),
    }
}

fn shuffle_order<const N: usize>(
    w: &mut World,
    order: &mut [u8; N],
    n: usize,
    avoid_first: Option<u8>,
) {
    let mut v: Vec<u8> = (0..n as u8).collect();
    w.rng.shuffle(&mut v);
    if let Some(a) = avoid_first
        && v.len() > 1
        && v[0] == a
    {
        v.swap(0, 1);
    }
    for (k, x) in v.into_iter().enumerate() {
        order[k] = x;
    }
}

pub fn update(w: &mut World, c: &Content, id: EntityId, s: &mut State, b: &mut Base) {
    let tm = w.time.time_mult;
    b.health = boss::pool(w).max(0);
    for e in s.eyes.iter_mut() {
        if e.flash > 0.0 {
            e.flash -= tm;
        }
    }
    for k in 0..s.n_tent {
        tentacle_tick(w, s, k, b, tm);
    }
    match s.phase {
        INTRO => intro(w, s, b, tm),
        FIGHT => {
            eye_cycle(w, s, b, tm);
            tentacle_director(w, s, b, tm);
            spores(w, s, b, tm);
            if b.difficulty > 1 && s.miasma.is_none() && b.health <= b.max_health / 3 {
                s.miasma = Some(crate::versus::miasma::spawn_mode(
                    w,
                    crate::versus::miasma::MiasmaMode::Bottom,
                ));
            }
            if b.health <= 0 {
                start_death(w, s);
            } else if b.health <= b.max_health * s.threshold / 3 {
                start_intermission(w, c, s, b);
            }
        }
        INTERMISSION => intermission(w, c, s, b, tm),
        _ => death(w, id, s, tm),
    }
}

fn intro(w: &mut World, s: &mut State, b: &mut Base, tm: f32) {
    if !s.intro.ready(tm) {
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
            s.intro.wait(120.0, 2);
        }
        2 => {
            boss::shake_level(w, 30.0);
            s.bg_in = true;
            w.sound("en_flowerbossTentacleBGShootUp", 160.0);
            s.intro.wait(60.0, 3);
        }
        _ => {
            s.intro.stop();
            start_attacks(w, s);
        }
    }
}

fn start_attacks(w: &mut World, s: &mut State) {
    s.phase = FIGHT;
    shuffle_order(w, &mut s.eye_order, s.n_eyes, None);
    s.eye_seq = Seq::start(0);
    s.tent_seq = Seq::start(0);
    s.tent_seq.wait(30.0, 0);
    s.spore_seq = Seq::start(0);
}

// --------------------------------------------------------------- eyes

fn set_open(w: &mut World, s: &mut State, i: usize, open: bool) {
    let e = &mut s.eyes[i];
    if e.open == open {
        return;
    }
    e.open = open;
    e.damaged = false;
    e.hits = 0;
    e.critical = false;
    let at = e.id.map_or(Vec2::ZERO, |id| w.core(id).pos);
    if open {
        w.sound("en_flowerbossEyeOpen", at.x);
    } else {
        w.sound("en_flowerbossEyeClose", at.x);
        throw_lodged(w, s, i, at);
    }
}

/// Lodged arrows are thrown out (closing or bursting eye).
fn throw_lodged(w: &mut World, s: &mut State, i: usize, at: Vec2) {
    {
        let lodged = std::mem::take(&mut s.eyes[i].lodged);
        for t in lodged.into_iter().flatten() {
            let shooter = arrow::Shooter {
                id: None,
                player_index: -1,
                allegiance: crate::entity::Allegiance::Neutral,
                team_color: crate::entity::Allegiance::Neutral,
                hyper: false,
            };
            let a = arrow::create(w, t, shooter, at + Vec2::new(0.0, 6.0), PI / 2.0);
            let vx = -1.6 + w.rng.next_float() * 3.2;
            let vy = w.rng.next_float() * 0.6;
            crate::arrows::with_arrow(w, a, |w, ar| {
                arrow::enter_fall_mode(w, a, ar, false, false, false);
                ar.speed = Vec2::new(vx, vy);
            });
        }
    }
}

fn eye_cycle(w: &mut World, s: &mut State, b: &Base, tm: f32) {
    if !s.eye_seq.ready(tm) || s.n_eyes == 0 {
        return;
    }
    let k = s.eye_seq.k as usize;
    match s.eye_seq.step {
        0 => {
            if k >= s.n_eyes {
                let last = s.eye_order[s.n_eyes - 1];
                shuffle_order(w, &mut s.eye_order, s.n_eyes, Some(last));
                s.eye_seq.k = 0;
                return;
            }
            let i = s.eye_order[k] as usize;
            set_open(w, s, i, true);
            s.eye_seq.next(1);
        }
        1 => {
            let i = s.eye_order[k] as usize;
            if s.eyes[i].damaged {
                let n = if b.difficulty == 0 { 40.0 } else { 30.0 };
                s.eye_seq.wait(n, 2);
            }
        }
        2 => {
            let i = s.eye_order[k] as usize;
            set_open(w, s, i, false);
            s.eye_seq.k += 1;
            s.eye_seq.wait(30.0 + 60.0 * dv(s, b), 0);
        }
        _ => {}
    }
}

/// Returns whether the eye took the hit (an unhurtable eye only plays the
/// null sound and the arrow falls off).
fn eye_hit(w: &mut World, s: &mut State, b: &mut Base, i: usize, killer: i8) -> bool {
    let at = s.eyes[i].id.map_or(Vec2::ZERO, |id| w.core(id).pos);
    if !s.can_hurt {
        w.sound("en_flowerbossEyeHitNull", at.x);
        return false;
    }
    boss::hurt(w, b, 1, killer);
    let e = &mut s.eyes[i];
    e.damaged = true;
    e.flash = 20.0;
    e.hits += 1;
    let next = b.max_health * (s.threshold - 1).max(0) / 3;
    if b.health <= next && !e.critical {
        e.critical = true;
        w.sound("en_flowerbossEyeHitCritical", at.x);
    } else if e.hits == 1 {
        w.sound("en_flowerbossEyeHit", at.x);
    } else {
        w.sound("en_flowerbossEyeHitSecondary", at.x);
    }
    true
}

// ----------------------------------------------------------- tentacles

fn tween(t: &mut Tentacle, to: f32, len: f32, out: bool, y: f32) {
    t.from = y;
    t.to = to;
    t.t = 0.0;
    t.len = len;
    t.out = out;
}

fn tentacle_tick(w: &mut World, s: &mut State, k: usize, b: &Base, tm: f32) {
    let dvv = dv(s, b);
    let t = &mut s.tentacles[k];
    let Some(id) = t.id else { return };
    if t.len > 0.0 {
        t.t += tm;
        let x = (t.t / t.len).min(1.0);
        let e = if t.out {
            Ease::CubeOut.apply(x)
        } else {
            Ease::CubeIn.apply(x)
        };
        w.core_mut(id).pos.y = t.from + (t.to - t.from) * e;
        if t.t >= t.len {
            t.len = 0.0;
        }
    }
    if !t.seq.ready(tm) {
        return;
    }
    let y = w.core(id).pos.y;
    match t.seq.step {
        // Warning: rise 24 px over 40 ticks.
        1 => {
            let steps = (t.max_add_x / 10.0) as i32 + 1;
            let kx = w.rng.next_max(steps.max(1));
            w.core_mut(id).pos = Vec2::new(t.start.x + 10.0 * kx as f32, t.start.y);
            tween(t, t.start.y - 24.0, 40.0, true, t.start.y);
            w.sound("en_flowerbossTentacleSproutBeginning", t.start.x);
            t.seq.wait(90.0 - 10.0 * dvv, 2);
        }
        // Strike.
        2 => {
            w.core_mut(id).collidable = true;
            boss::shake_level(w, 15.0);
            let top = t.start.y - 120.0 + (w.rng.next_max(30) - 20) as f32;
            tween(t, top, 60.0, true, y);
            w.sound("en_flowerbossTentacleShootUp", t.start.x);
            t.seq.wait(60.0, 3);
        }
        3 => {
            tween(t, t.start.y, 60.0, false, y);
            w.sound("en_flowerbossTentacleRecede", t.start.x);
            t.seq.wait(60.0 + 30.0, 4);
        }
        _ => {
            w.core_mut(id).collidable = false;
            t.seq.stop();
        }
    }
}

fn tentacle_director(w: &mut World, s: &mut State, b: &Base, tm: f32) {
    if !s.tent_seq.ready(tm) || s.n_tent == 0 {
        return;
    }
    let dvv = dv(s, b);
    let k = s.tent_seq.k as usize;
    if k == 0 {
        shuffle_order(w, &mut s.tent_order, s.n_tent, None);
    }
    if k >= s.n_tent {
        s.tent_seq.k = 0;
        s.tent_seq.wait(300.0 - 40.0 * dvv, 0);
        return;
    }
    let i = s.tent_order[k] as usize;
    s.tentacles[i].seq = Seq::start(1);
    s.tent_seq.k += 1;
    let r = w.rng.next_max(20) as f32;
    s.tent_seq.wait(120.0 - 20.0 * dvv + r, 0);
}

// -------------------------------------------------------------- spores

fn spawn_spore(w: &mut World, s: &mut State) {
    if s.slot_n == 0 {
        let mut v = s.slots.to_vec();
        w.rng.shuffle(&mut v);
        s.slots.copy_from_slice(&v);
        s.slot_n = 4;
    }
    s.slot_n -= 1;
    let slot = s.slots[s.slot_n];
    let x = slot - 25.0 + w.rng.next_float() * 50.0;
    let phase = w.rng.next_float() * 2.0 * PI;
    let id = super::part::add(
        w,
        Vec2::new(x, 0.0),
        Hitbox::new(10.0, 6.0, -5.0, -3.0),
        -100,
        &[
            Tag::PlayerCollider,
            Tag::Target,
            Tag::ExplosionCollider,
            Tag::ShockCollider,
        ],
        Part::Spore {
            t: 0.0,
            phase,
            free: false,
        },
    );
    w.core_mut(id).target_collider = Some(Hitbox::new(14.0, 14.0, -7.0, -7.0));
}

fn spores(w: &mut World, s: &mut State, b: &Base, tm: f32) {
    if !s.spore_seq.ready(tm) {
        return;
    }
    spawn_spore(w, s);
    s.spore_seq.wait(240.0 - 40.0 * dv(s, b), 0);
}

// -------------------------------------------------------- intermission

fn sets_for(d: i32, threshold_before: i32) -> ([[Option<EnemyType>; 3]; 2], usize) {
    use EnemyType::{EvilCrystal as C, Exploder as X};
    let first = threshold_before == 2;
    match (d, first) {
        (1, true) => ([[Some(X), Some(X), Some(C)], [None; 3]], 1),
        (1, false) => ([[Some(X), Some(C), None], [Some(X), Some(C), Some(C)]], 2),
        (2, true) => ([[Some(X), Some(C), Some(C)], [None; 3]], 1),
        (2, false) => (
            [[Some(X), Some(X), Some(C)], [Some(C), Some(C), Some(C)]],
            2,
        ),
        (_, true) => ([[Some(X), Some(X), None], [None; 3]], 1),
        (_, false) => ([[Some(X), Some(X), Some(X)], [Some(X), Some(C), None]], 2),
    }
}

fn start_intermission(w: &mut World, _c: &Content, s: &mut State, b: &Base) {
    let before = s.threshold;
    s.threshold -= 1;
    s.eye_seq.stop();
    s.tent_seq.stop();
    s.spore_seq.stop();
    for k in 0..s.n_tent {
        let t = &mut s.tentacles[k];
        if let Some(id) = t.id {
            let y = w.core(id).pos.y;
            tween(t, t.start.y, 30.0, false, y);
            w.core_mut(id).collidable = false;
        }
        t.seq.stop();
    }
    s.can_hurt = false;
    if before == 1 {
        super::signal_boss_treasure(w);
    }
    let (sets, n) = sets_for(b.difficulty, before);
    s.sets = sets;
    s.n_sets = n;
    s.phase = INTERMISSION;
    s.inter = Seq::start(0);
    s.inter.wait(15.0, 1);
}

fn portals(w: &World) -> Vec<EntityId> {
    w.dark_world
        .as_ref()
        .map(|d| d.portals.iter().map(|p| p.id).collect())
        .unwrap_or_default()
}

fn intermission(w: &mut World, c: &Content, s: &mut State, b: &Base, tm: f32) {
    let _ = c;
    if !s.inter.ready(tm) {
        return;
    }
    match s.inter.step {
        1 => {
            for i in 0..s.n_eyes {
                set_open(w, s, i, false);
            }
            s.inter.wait(45.0, 2);
        }
        2 => {
            w.sound("en_flowerbossTentacleBGRecede", 160.0);
            boss::shake_level(w, 30.0);
            s.bg_in = false;
            s.spores_left = ((1.0 + dv(s, b)) * 2.0) as i32;
            s.inter.k = 0;
            s.inter.next(3);
        }
        3 => {
            if s.spores_left > 0 {
                spawn_spore(w, s);
                s.spores_left -= 1;
                s.inter.k += 1;
                let pause = if s.inter.k % 4 == 0 { 12.0 } else { 6.0 };
                s.inter.wait(pause - 1.0, 3);
            } else {
                s.inter.k = 0;
                s.inter.wait(60.0, 4);
            }
        }
        // Enemy sets: one per portal (shuffled), 5 ticks apart.
        4 => {
            let k = s.inter.k as usize;
            if k >= s.n_sets {
                s.inter.wait(60.0, 7);
                return;
            }
            let mut ps = portals(w);
            w.rng.shuffle(&mut ps);
            let mut set: Vec<EnemyType> = s.sets[k].iter().flatten().copied().collect();
            w.rng.shuffle(&mut set);
            s.pend = [None; 3];
            for (j, ty) in set.into_iter().enumerate().take(3) {
                if let Some(&p) = ps.get(j % ps.len().max(1)) {
                    s.pend[j] = Some((p, ty));
                }
            }
            s.pend_i = 0;
            s.inter.next(20);
        }
        // set[j] on portal j, `wait(5)` after each; then `wait(60)`.
        20 => {
            while s.pend_i < 3 && s.pend[s.pend_i].is_none() {
                s.pend_i += 1;
            }
            if s.pend_i >= 3 {
                s.inter.wait(60.0, 5);
                return;
            }
            if let Some((p, ty)) = s.pend[s.pend_i] {
                portal::appear_and_spawn(w, p, ty);
            }
            s.pend_i += 1;
            s.inter.wait(5.0, 20);
        }
        5 => {
            let enemies = crate::enemies::enemy_count(w);
            if portal::queued_total(w) == 0 && enemies <= s.n_eyes {
                s.inter.k += 1;
                s.inter.next(4);
            }
        }
        7 => {
            w.sound("en_flowerbossTentacleBGShootUp", 160.0);
            boss::shake_level(w, 30.0);
            s.bg_in = true;
            s.inter.wait(60.0, 8);
        }
        _ => {
            s.can_hurt = true;
            s.inter.stop();
            start_attacks(w, s);
        }
    }
}

// --------------------------------------------------------------- death

fn start_death(w: &mut World, s: &mut State) {
    s.phase = DEAD;
    s.can_hurt = false;
    s.eye_seq.stop();
    s.tent_seq.stop();
    s.spore_seq.stop();
    s.inter.stop();
    for t in s.tentacles.iter_mut() {
        t.seq.stop();
        if let Some(id) = t.id {
            w.core_mut(id).collidable = false;
        }
    }
    if let Some(m) = s.miasma.take() {
        crate::versus::miasma::dissipate(w, m);
    }
    // Eyes sorted by x.
    let mut eyes: Vec<Eye> = s.eyes[..s.n_eyes].to_vec();
    eyes.sort_by(|a, b| {
        let ax = a.id.map_or(0.0, |i| w.core(i).pos.x);
        let bx = b.id.map_or(0.0, |i| w.core(i).pos.x);
        ax.total_cmp(&bx)
    });
    s.eyes[..s.n_eyes].copy_from_slice(&eyes);
    // Each eye entity carries its slot index: renumber after the sort.
    for (k, e) in eyes.iter().enumerate() {
        if let Some(id) = e.id {
            super::part::set(w, id, Part::Eye(k as u8));
        }
    }
    s.death = Seq::start(0);
    s.death.wait(30.0, 1);
}

fn burst(w: &mut World, c: &Content, s: &mut State, i: usize) {
    if i >= s.n_eyes {
        return;
    }
    s.eyes[i].burst = false;
    if let Some(id) = s.eyes[i].id {
        let pos = w.core(id).pos;
        throw_lodged(w, s, i, pos);
        let at = pos + Vec2::new(0.0, 10.0);
        crate::explosion::spawn(w, c, at, -1, false, &mut None);
        w.remove(id);
        s.eyes[i].id = None;
    }
}

fn death(w: &mut World, id: EntityId, s: &mut State, tm: f32) {
    if !s.death.ready(tm) {
        return;
    }
    match s.death.step {
        1 => {
            w.sound("en_flowerbossDeath", 160.0);
            s.death.wait(30.0, 2);
        }
        2 => {
            super::disable_dangers(w);
            for i in 0..s.n_eyes {
                set_open(w, s, i, true);
            }
            let spores: Vec<EntityId> = w
                .entities()
                .filter(|&e| matches!(super::part::get(w, e), Some(Part::Spore { .. })))
                .collect();
            for e in spores {
                w.remove(e);
            }
            s.death.wait(60.0, 3);
        }
        3 => {
            boss::shake_level(w, 300.0);
            s.death.wait(120.0, 4);
        }
        4 => {
            s.death.k = 0;
            s.death.wait(0.0, 5);
        }
        // Bursts: eye 0, then 60 later eye 2, then 90 later eye 1.
        5 => {
            let order = [0usize, 2, 1];
            let waits = [60.0, 90.0, 120.0];
            let k = s.death.k as usize;
            if k < 3 {
                // Burst = 60 stretch + 40 + 20, then the explosion.
                let i = order[k].min(s.n_eyes.saturating_sub(1));
                s.death.k += 1;
                s.death.wait(waits[k], 5);
                w.sound("en_flowerbossEyeHitCritical", 160.0);
                s.eyes[i].burst = true;
                if let Some(e) = s.eyes[i].id {
                    w.core_mut(e).collidable = false;
                }
            } else {
                let tail = if s.gauntlet { 120.0 } else { 600.0 };
                s.death.wait(tail, 6);
            }
        }
        _ => {
            super::boss_finished(w);
            w.remove(id);
            s.death.stop();
        }
    }
}

// --------------------------------------------------------------- parts

pub fn part_update(w: &mut World, c: &Content, id: EntityId, p: &mut Part) {
    let tm = w.time.time_mult;
    match p {
        Part::Eye(i) => {
            // A bursting eye explodes once its stretch is over (120 ticks).
            let i = *i as usize;
            boss::with_boss(w, |w, b| {
                if let BossKind::Dreadwood(s) = &mut b.kind
                    && s.eyes[i].burst
                {
                    s.eyes[i].burst_t += tm;
                    if s.eyes[i].burst_t >= 121.0 {
                        burst(w, c, s, i);
                    }
                }
            });
        }
        Part::Tentacle(_) => {}
        Part::Spore { t, phase, free } => {
            *t += tm;
            let me = w.placed(id);
            if !*free {
                // Ignores solids until it first leaves them.
                if !w.any(c, &me, Tag::Solid, id) {
                    *free = true;
                }
                w.core_mut(id).pos.y += 0.3 * tm;
                return;
            }
            let dx = 0.6 * libm::sinf(*phase + *t * 2.0 * PI / 120.0) * tm;
            if let Some(_s) = crate::actor::move_h(w, c, id, dx) {
                let pos = w.core(id).pos;
                if pos.y >= 80.0 {
                    let wall = if dx > 0.0 { 3.0 } else { -3.0 };
                    break_spore(w, id, pos + Vec2::new(wall, 0.0));
                    return;
                }
            }
            if crate::actor::move_v(w, c, id, 0.3 * tm).is_some() {
                let pos = w.core(id).pos;
                break_spore(w, id, pos + Vec2::new(0.0, 3.0));
                return;
            }
            if w.time.on_interval(6) {
                crate::entities::particles(
                    w,
                    "EnBrambleSpore",
                    w.core(id).pos,
                    1,
                    Vec2::new(3.0, 3.0),
                    0.0,
                );
            }
            if w.core(id).pos.y > 260.0 {
                w.remove(id);
            }
        }
        _ => {}
    }
}

/// A spore that lands becomes a short-lived 2-spread bramble patch.
fn break_spore(w: &mut World, id: EntityId, at: Vec2) {
    w.sound("en_flowerbossSporeHit", at.x);
    crate::arrows::brambles::start_growth(w, at, -1, None, 2, true);
    w.remove(id);
    w.core_mut(id).collidable = false;
}

pub fn part_touch(w: &mut World, c: &Content, id: EntityId, pid: EntityId, pl: &mut Player) {
    match super::part::get(w, id) {
        Some(Part::Eye(i)) => {
            let stomp = boss::is_stomp(w, id, pid, pl);
            if stomp {
                boss::bounce(w, c, id, pid, pl);
                let killer = pl.index as i8;
                boss::with_boss(w, |w, b| {
                    if let (BossKind::Dreadwood(s), base) = (&mut b.kind, &mut b.base)
                        && s.eyes[i as usize].open
                    {
                        eye_hit(w, s, base, i as usize, killer);
                    }
                });
            }
        }
        Some(Part::Tentacle(_)) => {
            if w.core(id).collidable {
                boss::hurt_player(w, c, pid, pl, w.core(id).pos);
            }
        }
        Some(Part::Spore { .. }) => {
            let top = w.core(id).top();
            let pc = w.core(pid);
            if pl.speed.y >= 0.0 && pc.bottom() - pl.speed.y <= top + 6.0 {
                crate::player::bounce_pub(w, c, pid, pl, top, false);
                w.sound("en_flowerbossSporeHitJump", w.core(id).pos.x);
            } else {
                boss::hurt_player(w, c, pid, pl, w.core(id).pos);
            }
            w.remove(id);
            w.core_mut(id).collidable = false;
        }
        _ => {}
    }
}

pub fn part_arrow(w: &mut World, _c: &Content, id: EntityId, aid: EntityId, a: &mut Arrow) -> bool {
    match super::part::get(w, id) {
        Some(Part::Eye(i)) => {
            if !a.dangerous() {
                return false;
            }
            let killer = a.player_index;
            let ty = a.arrow_type;
            let lodged = boss::with_boss(w, |w, b| {
                let (BossKind::Dreadwood(s), base) = (&mut b.kind, &mut b.base) else {
                    return false;
                };
                let i = i as usize;
                if !s.eyes[i].open {
                    w.sound("en_flowerbossEyeHitNull", w.core(id).pos.x);
                    return false;
                }
                if !eye_hit(w, s, base, i, killer) {
                    return false;
                }
                if let Some(slot) = s.eyes[i].lodged.iter_mut().find(|x| x.is_none()) {
                    *slot = Some(ty);
                    return true;
                }
                false
            })
            .unwrap_or(false);
            if lodged {
                w.remove(aid);
                w.core_mut(aid).collidable = false;
            } else {
                arrow::enter_fall_mode(w, aid, a, true, false, false);
            }
            true
        }
        Some(Part::Tentacle(_)) => {
            if a.state == arrow::ArrowState::Shooting && w.core(id).collidable {
                arrow::enter_fall_mode(w, aid, a, true, false, false);
                return true;
            }
            false
        }
        Some(Part::Spore { .. }) => {
            w.sound("en_flowerbossSporeHit", w.core(id).pos.x);
            w.remove(id);
            w.core_mut(id).collidable = false;
            arrow::enter_fall_mode(w, aid, a, true, false, false);
            true
        }
        _ => false,
    }
}
