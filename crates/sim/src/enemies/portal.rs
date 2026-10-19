//! `QuestSpawnPortal` (enemies.md §2): appears, queues enemy names, spawns
//! them one by one (~46 ticks apart), disappears.

use super::*;

const QUEUE: usize = 24;
/// Sprite clock of the 'spawning' animation: frame 25 after 13 advances of 0.06 s.
fn spawn_at() -> f32 {
    anim_ticks(0.06, 13)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portal {
    /// Index into the level's spawner markers (name, nodes).
    pub marker: u16,
    pub appeared: bool,
    pub auto_disappear: bool,
    /// The running appear tween restarts the spawning animation when it
    /// completes (the queue was non-empty when `Appear` was called).
    pub complete_spawn: bool,
    queue: [Option<EnemyType>; QUEUE],
    qlen: u8,
    /// Ticks into the appear/disappear tween (40 ticks).
    pub tween: f32,
    pub tween_dir: i8,
    /// Ticks into the spawning animation (`None` = idle loop).
    pub spawn_t: Option<f32>,
    /// `QueuedSpawns` counts +1 for 2 ticks after a spawn.
    pub add_counter: f32,
    /// Alternating facing for portals at the exact centre.
    pub flip: bool,
    pub scale: f32,
}

impl StableHash for Portal {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u32(self.marker as u32);
        h.bool(self.appeared);
        h.bool(self.auto_disappear);
        h.bool(self.complete_spawn);
        h.u8(self.qlen);
        h.f32(self.tween);
        h.i32(self.tween_dir as i32);
        self.spawn_t.stable_hash(h);
        h.f32(self.add_counter);
        h.bool(self.flip);
    }
}

impl Portal {
    pub fn queued(&self) -> usize {
        self.qlen as usize + if self.add_counter > 0.0 { 1 } else { 0 }
    }
    fn push(&mut self, t: EnemyType) {
        if (self.qlen as usize) < QUEUE {
            self.queue[self.qlen as usize] = Some(t);
            self.qlen += 1;
        }
    }
    fn pop(&mut self) -> Option<EnemyType> {
        if self.qlen == 0 {
            return None;
        }
        let t = self.queue[0];
        self.queue.copy_within(1..self.qlen as usize, 0);
        self.qlen -= 1;
        t
    }
}

/// Create one portal per `Spawner` marker of the level (scene build).
pub fn add_level_portals(w: &mut World, c: &Content) -> Vec<EntityId> {
    let n = c.level(w.level).markers.spawners.len();
    (0..n).map(|i| add_portal(w, c, i)).collect()
}

/// One portal at spawner marker `i` (closed).
pub fn add_portal(w: &mut World, c: &Content, i: usize) -> EntityId {
    let pos = c.level(w.level).markers.spawners[i].pos;
    let mut core = Core::new(pos, Collider::None, 0, &[Tag::LightSource]);
    core.active = true;
    w.add(
        core,
        Kind::Portal(Portal {
            marker: i as u16,
            appeared: false,
            auto_disappear: false,
            complete_spawn: false,
            queue: [None; QUEUE],
            qlen: 0,
            tween: 0.0,
            tween_dir: 0,
            spawn_t: None,
            add_counter: 0.0,
            flip: false,
            scale: 0.0,
        }),
    )
}

/// All portals with their marker names.
pub fn portals(w: &World, c: &Content) -> Vec<(EntityId, String)> {
    let lvl = c.level(w.level);
    let names = |m: u16| {
        lvl.markers
            .spawners
            .get(m as usize)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };
    w.entities()
        .filter_map(|id| match w.kind(id) {
            Some(Kind::Portal(p)) => Some((id, names(p.marker))),
            _ => None,
        })
        .collect()
}

pub fn queued_total(w: &World) -> usize {
    w.entities()
        .filter_map(|id| match w.kind(id) {
            Some(Kind::Portal(p)) => Some(p.queued()),
            _ => None,
        })
        .sum()
}

fn with(w: &mut World, id: EntityId, f: impl FnOnce(&mut Portal)) {
    if let Some(Kind::Portal(p)) = w.kind_mut(id) {
        f(p);
    }
}

/// `Appear`: false if already appeared. The tween restarts the spawning
/// animation on completion only if something is queued now (§2).
pub fn appear(w: &mut World, id: EntityId) -> bool {
    let mut changed = false;
    with(w, id, |p| {
        if !p.appeared {
            p.appeared = true;
            p.tween = 0.0;
            p.tween_dir = 1;
            p.complete_spawn = p.qlen > 0;
            changed = true;
        }
    });
    if changed {
        let x = w.core(id).pos.x;
        w.sound("sfx_portalAppear", x);
    }
    changed
}

/// `Disappear`.
pub fn disappear(w: &mut World, id: EntityId) -> bool {
    let mut changed = false;
    with(w, id, |p| {
        if p.appeared {
            p.appeared = false;
            p.tween = 0.0;
            p.tween_dir = -1;
            p.complete_spawn = false;
            changed = true;
        }
    });
    if changed {
        let x = w.core(id).pos.x;
        w.sound("sfx_portalDisappear", x);
    }
    changed
}

/// `ForceDisappear`: not while auto-disappearing.
pub fn force_disappear(w: &mut World, id: EntityId) -> bool {
    let auto = matches!(w.kind(id), Some(Kind::Portal(p)) if p.auto_disappear);
    if auto { false } else { disappear(w, id) }
}

pub fn is_open(w: &World, id: EntityId) -> bool {
    matches!(w.kind(id), Some(Kind::Portal(p)) if p.appeared)
}

/// `SpawnEnemy(name)`: an empty queue restarts the spawning animation now,
/// even during the appear tween.
pub fn spawn_enemy(w: &mut World, id: EntityId, t: EnemyType) {
    with(w, id, |p| {
        if p.qlen == 0 {
            p.spawn_t = Some(0.0);
        }
        p.push(t);
        p.auto_disappear = false;
    });
}

/// `AppearAndSpawn(name)`.
pub fn appear_and_spawn(w: &mut World, id: EntityId, t: EnemyType) {
    with(w, id, |p| {
        p.push(t);
        p.auto_disappear = true;
    });
    if !appear(w, id) {
        // Port: never leave the queue stalled on an open, idle portal (§2).
        with(w, id, |p| {
            let pending = p.tween_dir == 1 && p.complete_spawn;
            if p.spawn_t.is_none() && !pending {
                p.spawn_t = Some(0.0);
            }
        });
    }
}

/// `ForceCancel` on every portal.
pub fn force_cancel_all(w: &mut World, c: &Content) {
    let ids: Vec<EntityId> = portals(w, c).into_iter().map(|p| p.0).collect();
    for id in ids {
        with(w, id, |p| {
            p.qlen = 0;
            p.spawn_t = None;
        });
        disappear(w, id);
    }
}

/// Sprite (spawn event), then the tween, then the add counter (§2).
pub fn update(w: &mut World, c: &Content, id: EntityId, p: &mut Portal) {
    let tm = w.time.time_mult;
    if let Some(t) = p.spawn_t {
        let t = t + tm;
        p.spawn_t = Some(t);
        if t >= spawn_at() {
            spawn_one(w, c, id, p);
        }
    }
    if p.tween_dir != 0 {
        p.tween += tm;
        let k = (p.tween / 40.0).min(1.0);
        p.scale = if p.tween_dir > 0 { k } else { 1.0 - k };
        if p.tween >= 40.0 {
            p.tween_dir = 0;
            if p.complete_spawn {
                p.complete_spawn = false;
                w.sound("sfx_portalSpawn", w.core(id).pos.x);
                p.spawn_t = Some(0.0);
            }
        }
    }
    if p.add_counter > 0.0 {
        p.add_counter -= tm;
    }
}

fn spawn_one(w: &mut World, c: &Content, _id: EntityId, p: &mut Portal) {
    let Some(ty) = p.pop() else {
        p.spawn_t = None;
        return;
    };
    let lvl = c.level(w.level);
    let marker = &lvl.markers.spawners[p.marker as usize];
    let pos = marker.pos;
    let half = lvl.width as f32 / 2.0;
    let facing = if pos.x == half {
        p.flip = !p.flip;
        if p.flip { 1 } else { -1 }
    } else if pos.x < half {
        1
    } else {
        -1
    };
    let nodes = marker.nodes.clone();
    super::spawn(w, c, ty, pos + ty.spawn_offset(), facing, &nodes);
    p.add_counter = 2.0;
    if p.qlen > 0 {
        p.spawn_t = Some(0.0);
    } else {
        p.spawn_t = None;
        if p.auto_disappear && p.appeared {
            p.appeared = false;
            p.tween = 0.0;
            p.tween_dir = -1;
            p.complete_spawn = false;
            w.sound("sfx_portalDisappear", pos.x);
        }
    }
    w.sound("sfx_portalSpawn", pos.x);
}
