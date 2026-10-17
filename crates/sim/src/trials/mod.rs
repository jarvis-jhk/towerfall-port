//! Trials (docs/spec/trials.md): one archer, destroy every dummy as fast as
//! possible. The clock is integer ticks; medal times are data.

use crate::checksum::{Hasher, StableHash};
use crate::collide::{Collider, Hitbox};
use crate::content::Content;
use crate::entities::Tower;
use crate::entity::{Allegiance, Core, EntityId, Kind, Tag};
use crate::impl_hash;
use crate::math::Vec2;
use crate::player::{self, Player};
use crate::world::{MatchSetup, PlayerSetup, World};

/// Medal thresholds and level data (`trials.xml`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrialsSettings {
    /// Index into `Content::levels`.
    pub level: usize,
    pub archer: u16,
    /// Arrows in the starting quiver (default 3).
    pub arrows: u8,
    /// Ticks until switch blocks flip once (default 200).
    pub switch_timer: u16,
    /// Seconds: gold, diamond, dev.
    pub gold: f32,
    pub diamond: f32,
    pub dev: f32,
    /// Stored best time in ticks (0 = none) and unlocked medals.
    pub best: u32,
    pub unlocked: [bool; 3],
    pub start_screen: bool,
    pub seed: i32,
    /// This level's hidden gem was already found: its chest spawns empty.
    pub gem_found: bool,
}

impl TrialsSettings {
    pub fn new(level: usize) -> TrialsSettings {
        TrialsSettings {
            level,
            archer: 0,
            arrows: 3,
            switch_timer: 200,
            gold: 7.5,
            diamond: 3.0,
            dev: 1.7,
            best: 0,
            unlocked: [false; 3],
            start_screen: false,
            seed: 0,
            gem_found: false,
        }
    }
}

/// Seconds -> ticks (`round(s * 60)`; every shipped threshold is exact).
pub fn ticks(seconds: f32) -> u32 {
    crate::math::round_he(seconds * 60.0) as u32
}

/// `GetTimeString`: `S.mmm` under a minute, else `M:SS.mmm`.
pub fn time_string(t: u32) -> String {
    let ms = t as u64 * 1000 / 60;
    let (m, s, f) = (ms / 60_000, (ms / 1000) % 60, ms % 1000);
    if m > 0 {
        format!("{m}:{s:02}.{f:03}")
    } else {
        format!("{s}.{f:03}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrialsPhase {
    Intro,
    Playing,
    Won,
    Failed,
    Results,
    Finished,
}

/// Result of a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrialsResult {
    /// Ticks (0 = failure).
    pub time: u32,
    pub new_best: bool,
    /// Newly unlocked gold / diamond / dev time.
    pub unlocked: [bool; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trials {
    pub settings: TrialsSettings,
    pub phase: TrialsPhase,
    pub phase_t: f32,
    pub start_pos: Vec2,
    pub started: bool,
    pub time: u32,
    pub dummies_total: u32,
    /// Living dummies the HUD still shows.
    pub drawing: u32,
    pub result: Option<TrialsResult>,
    pub ready: bool,
    /// The hidden gem was collected this attempt (the app records it).
    pub gem_collected: bool,
    intro_wait: f32,
    confirm_at: f32,
    confirmed: bool,
}
impl StableHash for Trials {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u8(self.phase as u8);
        h.f32(self.phase_t);
        h.vec2(self.start_pos);
        h.bool(self.started);
        h.u32(self.time);
        h.u32(self.drawing);
        h.bool(self.result.is_some());
        h.bool(self.gem_collected);
    }
}

/// A Trials `Dummy` (trials.md §8): a static target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dummy {
    pub facing: i8,
    pub dead: bool,
    /// The chain it hangs from (it follows the chain; arrows push it).
    pub chain: Option<EntityId>,
}
impl_hash!(Dummy {
    facing,
    dead,
    chain
});

pub const DUMMY_HITBOX: Hitbox = Hitbox::wrapping(8.0, 18.0, -4.0, 2.0);

/// Create the world for a Trials attempt.
pub fn new_trial(c: &Content, settings: TrialsSettings) -> World {
    let setup = MatchSetup {
        level: settings.level,
        seed: settings.seed,
        start_frozen: true,
        ..MatchSetup::default()
    };
    let mut w = World::new(c, &setup);
    let lvl = c.level(settings.level);
    let spawn = lvl
        .player_spawns
        .first()
        .copied()
        .unwrap_or(Vec2::new(lvl.width as f32 / 2.0, 0.0))
        + Vec2::new(0.0, 2.0);
    let ps = PlayerSetup {
        archer: settings.archer,
        allegiance: Allegiance::Neutral,
        arrows: settings.arrows,
        ..Default::default()
    };
    player::create(&mut w, c, 0, spawn, &ps);
    // A `.oel` `Dummy` is both a tower placement (spawned with its chain
    // link by the loader) and a Trials marker: Trials keeps one dummy per
    // spot, the Trials kind, taking over the placement's chain.
    let placed: Vec<EntityId> = w
        .tagged(Tag::Dummy)
        .iter()
        .copied()
        .filter(|&id| matches!(w.kind(id), Some(Kind::Tower(Tower::Dummy(_)))))
        .collect();
    let mut n = 0;
    if placed.is_empty() {
        for d in lvl.markers.dummies.clone() {
            add_dummy(&mut w, d.pos, d.facing, None);
            n += 1;
        }
    } else {
        for id in placed {
            let Some(Kind::Tower(Tower::Dummy(td))) = w.kind(id).copied() else {
                continue;
            };
            let pos = w.core(id).pos;
            w.remove(id);
            let facing = if td.facing_left { -1 } else { 1 };
            let nd = add_dummy(&mut w, pos, facing, td.chain);
            if let Some(ch) = td.chain
                && let Some(Kind::Tower(Tower::Chain(chain))) = w.kind_mut(ch)
            {
                chain.holding = Some(nd);
            }
            n += 1;
        }
    }
    add_chests(&mut w, c, &settings);
    configure_switch_control(&mut w, settings.switch_timer);
    // Start screen: wait(20), emblem slam (10), wait(20), the goal board's
    // own sequence, then Confirm; afterwards a 30-tick slide-out. Restart:
    // wait(10).
    let s = &settings;
    let board =
        11.0 + 11.0 + if s.unlocked[2] { 11.0 } else { 0.0 } + if s.best > 0 { 31.0 } else { 0.0 };
    w.trials = Some(Box::new(Trials {
        settings,
        phase: TrialsPhase::Intro,
        phase_t: 0.0,
        start_pos: spawn,
        started: false,
        time: 0,
        dummies_total: n,
        drawing: n,
        result: None,
        ready: false,
        gem_collected: false,
        intro_wait: if settings.start_screen { 31.0 } else { 11.0 },
        confirm_at: 21.0 + 11.0 + 21.0 + board,
        confirmed: !settings.start_screen,
    }));
    crate::entities::apply_solo_seek(&mut w);
    w
}

/// `TrialsRoundLogic`: every `TreasureChest` element becomes a chest with
/// its `Type`, appear `Mode` and single `Treasure` (no timer). A gem chest
/// whose gem was already found starts empty (opened).
fn add_chests(w: &mut World, c: &Content, s: &TrialsSettings) {
    use crate::pickups::Pickups;
    use crate::treasure::{self, Appear, ChestSpawn, ChestType};
    let lvl = c.level(s.level);
    for d in lvl.markers.chests.iter().filter(|d| !d.big) {
        let ty = ChestType::from_name(&d.kind).unwrap_or(ChestType::Normal);
        let appear = Appear::from_name(&d.mode).unwrap_or(Appear::Normal);
        let item = Pickups::from_name(&d.treasure);
        let item = item.filter(|&p| !(p == Pickups::Gem && s.gem_found));
        treasure::create(
            w,
            &ChestSpawn {
                pos: d.pos,
                ty,
                timer: 0.0,
                contents: [item, None, None],
            },
            appear,
        );
    }
}

/// Trials: the switch-block control waits for the clock, flips once after
/// `switch_timer` ticks and removes itself.
fn configure_switch_control(w: &mut World, timer: u16) {
    let ids: Vec<EntityId> = w.entities().collect();
    for id in ids {
        if let Some(Kind::Tower(Tower::SwitchControl(sc))) = w.kind_mut(id) {
            sc.timer = timer as f32;
            sc.once = true;
        }
    }
}

pub fn add_dummy(w: &mut World, pos: Vec2, facing: i8, chain: Option<EntityId>) -> EntityId {
    let mut core = Core::new(
        pos,
        Collider::Box(DUMMY_HITBOX),
        -99,
        &[
            Tag::Dummy,
            Tag::PlayerCollider,
            Tag::ExplosionCollider,
            Tag::LightSource,
            Tag::Target,
        ],
    );
    core.seek = true;
    core.seek_priority = 50;
    core.seek_offset = Vec2::new(0.0, 13.0);
    core.allegiance = Allegiance::Red;
    core.active = false;
    w.add(
        core,
        Kind::Dummy(Dummy {
            facing,
            dead: false,
            chain,
        }),
    )
}

/// Dummies still standing: Trials markers and tower placements alike.
fn living_dummies(w: &World) -> u32 {
    w.tagged(Tag::Dummy)
        .iter()
        .filter(|&&id| {
            !w.core(id).marked_for_removal
                && match w.kind(id) {
                    Some(Kind::Dummy(d)) => !d.dead,
                    Some(Kind::Tower(Tower::Dummy(_))) => true,
                    _ => false,
                }
        })
        .count() as u32
}

fn kill_dummy(w: &mut World, id: EntityId) {
    if let Some(Kind::Dummy(d)) = w.kind_mut(id)
        && !d.dead
    {
        d.dead = true;
        let chain = d.chain.take();
        if let Some(ch) = chain
            && let Some(Kind::Tower(Tower::Chain(c))) = w.kind_mut(ch)
            && c.holding == Some(id)
        {
            c.holding = None;
        }
        let pos = w.core(id).pos;
        w.core_mut(id).collidable = false;
        w.remove(id);
        w.sound("sfx_dummyImp", pos.x);
        w.emit(crate::Event::Particles {
            kind: crate::events::ParticleKind::Dust,
            player: 255,
            pos: pos + Vec2::new(0.0, 12.0),
            count: 20,
            dir: 0.0,
        });
    }
}

/// Any arrow kills a dummy (the arrow keeps flying); a chained one
/// pushes its chain by the arrow's x speed.
pub fn dummy_arrow_hit(w: &mut World, id: EntityId) -> bool {
    kill_dummy(w, id);
    true
}

/// [`dummy_arrow_hit`] with the arrow's push on the chain.
pub fn dummy_arrow_push(w: &mut World, id: EntityId, arrow_y: f32, speed_x: f32) -> bool {
    if let Some(Kind::Dummy(d)) = w.kind(id)
        && let Some(ch) = d.chain
    {
        crate::entities::scenery::chain_push_pub(w, ch, arrow_y, speed_x);
    }
    dummy_arrow_hit(w, id)
}

pub fn dummy_explode(w: &mut World, _c: &Content, id: EntityId, _normal: Vec2) {
    kill_dummy(w, id);
}

/// Stomp: `player.bottom - speedY <= dummy.top + 10` while falling.
pub fn dummy_player_collide(
    w: &mut World,
    c: &Content,
    id: EntityId,
    pid: EntityId,
    p: &mut Player,
) {
    let top = w.core(id).top();
    let pb = w.core(pid).bottom();
    if p.speed.y >= 0.0 && pb - p.speed.y <= top + 10.0 {
        player::bounce_pub(w, c, pid, p, top, false);
        kill_dummy(w, id);
    }
}

pub fn session_update(w: &mut World, c: &Content) {
    let Some(mut t) = w.trials.take() else { return };
    let _ = c;
    if t.phase == TrialsPhase::Playing {
        let pid = w.player_by_index(0);
        let moved = match pid {
            None => true,
            Some(id) => w.core(id).pos != t.start_pos || w.player(id).is_none_or(|p| p.dead),
        };
        if !t.started && (moved || !w.tagged(Tag::Arrow).is_empty()) {
            t.started = true;
        }
        if t.started && t.result.is_none() {
            t.time += 1;
        }
        let living = living_dummies(w);
        while t.drawing > living {
            t.drawing -= 1;
        }
        if t.drawing == 0 && t.result.is_none() {
            win(w, &mut t);
        } else if t.started
            && t.result.is_none()
            && w.player_by_index(0)
                .is_none_or(|id| w.player(id).is_none_or(|p| p.dead))
        {
            t.result = Some(TrialsResult::default());
            t.phase = TrialsPhase::Failed;
            t.phase_t = 0.0;
        }
    }
    w.trials = Some(t);
}

fn win(w: &mut World, t: &mut Trials) {
    let time = t.time;
    let s = &t.settings;
    let new_best = s.best == 0 || time < s.best;
    let mut unlocked = [false; 3];
    if new_best {
        for (i, th) in [s.gold, s.diamond, s.dev].into_iter().enumerate() {
            unlocked[i] = !s.unlocked[i] && time <= ticks(th);
        }
    }
    t.result = Some(TrialsResult {
        time,
        new_best,
        unlocked,
    });
    t.phase = TrialsPhase::Won;
    t.phase_t = 0.0;
    let fanfare = time <= ticks(s.dev);
    w.sound(
        if fanfare {
            "sfx_devTimeFinalDummy"
        } else {
            "sfx_trainingSuccess"
        },
        160.0,
    );
}

pub fn end_of_tick(w: &mut World, c: &Content) {
    let Some(mut t) = w.trials.take() else { return };
    t.phase_t += 1.0;
    match t.phase {
        TrialsPhase::Intro => {
            if !t.confirmed && t.phase_t >= t.confirm_at {
                let s = w.inputs[0].state;
                if s.jump_pressed || s.start_pressed {
                    t.confirmed = true;
                    t.phase_t = 0.0;
                }
            } else if t.confirmed && t.phase_t >= t.intro_wait {
                player::unfreeze_all(w, c);
                crate::entities::round_started(w, c);
                t.phase = TrialsPhase::Playing;
                t.phase_t = 0.0;
            }
        }
        TrialsPhase::Won => {
            // wait(40) + icon pops (6 apart) + wait(30) + slide 20 + wait(30).
            let done = 41.0 + 7.0 * t.dummies_total as f32 + 31.0 + 20.0 + 31.0;
            if t.phase_t >= done {
                to_results(w, &mut t);
            }
        }
        TrialsPhase::Failed => {
            if t.phase_t >= 121.0 + 20.0 + 31.0 {
                to_results(w, &mut t);
            }
        }
        TrialsPhase::Results => {
            let ready_at = if t.result.is_some_and(|r| r.time > 0) {
                21.0 + 41.0 + 10.0 + 120.0 + 41.0
            } else {
                21.0 + 60.0 + 41.0
            };
            t.ready = t.phase_t >= ready_at;
            let s = w.inputs[0].state;
            if t.ready && (s.jump_pressed || s.start_pressed) {
                t.phase = TrialsPhase::Finished;
            }
        }
        _ => {}
    }
    w.trials = Some(t);
}

fn to_results(w: &mut World, t: &mut Trials) {
    w.set_frozen(true);
    w.orb.cancel_slow_mo(&mut w.time);
    t.phase = TrialsPhase::Results;
    t.phase_t = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_strings() {
        assert_eq!(time_string(450), "7.500");
        assert_eq!(time_string(60 * 61 + 3), "1:01.050");
        assert_eq!(ticks(7.5), 450);
        assert_eq!(ticks(1.7), 102);
    }
}
