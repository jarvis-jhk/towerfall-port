//! `TeamReviver` (player.md §18.2, quest.md §5.3, versus.md §7.3,
//! darkworld.md §4.2): a corpse that can come back to life.
//!
//! Quest: auto-revive only (spent lives / free revive at wave start).
//! Dark World and Team Deathmatch's Team Revive: a living teammate standing
//! on the corpse for 60 ticks. Revivers are updated by the level before the
//! entities (`coop::session_update`), like the original's `ReviveUpdate`.

use crate::collide::{Collider, Hitbox, Placed};
use crate::components::StateMachine;
use crate::content::Content;
use crate::entity::{Allegiance, Core, EntityId, Kind, Tag};
use crate::math::Vec2;
use crate::player::{self, HatState, PState};
use crate::quiver::ArrowType;
use crate::world::{PlayerSetup, World};
use crate::{impl_hash, impl_hash_enum};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviveMode {
    Quest,
    DarkWorld,
    TeamDeathmatch,
}
impl_hash_enum!(ReviveMode);

const SMALL: Hitbox = Hitbox::new(24.0, 25.0, -12.0, -20.0);
const BIG: Hitbox = Hitbox::new(40.0, 46.0, -20.0, -30.0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeamReviver {
    pub corpse: EntityId,
    pub mode: ReviveMode,
    pub player_index: u8,
    pub allegiance: Allegiance,
    pub age: f32,
    pub reviving: bool,
    pub counter: f32,
    pub reviver: Option<EntityId>,
    /// Flagged by the round logic (spent life / wave start).
    pub auto: bool,
    /// Counter reached 0: the corpse counts as alive.
    pub revived: bool,
    /// Ticks into the finishing sequence (`None` = not started).
    pub seq: Option<f32>,
    pub new_player: Option<EntityId>,
    /// Kept for the view; the corpse itself now rises (always 0).
    pub levitate: f32,
    /// Levitation: target height (corpse y - 6 at revive start) and the
    /// 90-tick bob.
    pub lift_y: f32,
    pub sine: crate::components::SineWave,
}
impl_hash!(TeamReviver {
    corpse,
    mode,
    player_index,
    age,
    reviving,
    counter,
    reviver,
    auto,
    revived,
    seq,
    new_player,
    lift_y
});

/// Create the reviver for a new corpse when the mode wants one.
pub fn on_corpse_added(
    w: &mut World,
    c: &Content,
    corpse: EntityId,
    player_index: i8,
    allegiance: Allegiance,
    pos: Vec2,
) {
    if player_index < 0 {
        return;
    }
    let mode = if w.quest.is_some() {
        ReviveMode::Quest
    } else if w.dark_world.is_some() {
        ReviveMode::DarkWorld
    } else if let Some(m) = &w.versus {
        if m.settings.team_mode()
            && m.settings
                .variants
                .any(crate::variants::Variant::TeamRevive)
        {
            ReviveMode::TeamDeathmatch
        } else {
            return;
        }
    } else {
        return;
    };
    let _ = c;
    let core = Core::new(
        pos + Vec2::new(0.0, 8.0),
        Collider::Box(SMALL),
        0,
        &[Tag::TeamReviver],
    );
    w.add(
        core,
        Kind::Reviver(TeamReviver {
            corpse,
            mode,
            player_index: player_index as u8,
            allegiance,
            age: 0.0,
            reviving: false,
            counter: 60.0,
            reviver: None,
            auto: false,
            revived: false,
            seq: None,
            new_player: None,
            levitate: 0.0,
            lift_y: 0.0,
            sine: crate::components::SineWave::frames(90.0),
        }),
    );
}

/// `sfx_revive{Blue,Red}team{Start,Cancel,Finish}` (`k` = 0, 1, 2).
fn team_cue(t: &TeamReviver, k: usize) -> &'static str {
    let red = t.allegiance == Allegiance::Red;
    match (red, k) {
        (false, 0) => "sfx_reviveBlueteamStart",
        (false, 1) => "sfx_reviveBlueteamCancel",
        (false, _) => "sfx_reviveBlueteamFinish",
        (true, 0) => "sfx_reviveRedteamStart",
        (true, 1) => "sfx_reviveRedteamCancel",
        (true, _) => "sfx_reviveRedteamFinish",
    }
}

/// All revivers in the scene.
pub fn revivers(w: &World) -> Vec<EntityId> {
    w.tagged(Tag::TeamReviver).to_vec()
}

/// The reviver of player slot `p`, if any.
pub fn of_player(w: &World, p: u8) -> Option<EntityId> {
    revivers(w)
        .into_iter()
        .find(|&r| matches!(w.kind(r), Some(Kind::Reviver(t)) if t.player_index == p))
}

/// Flag a player's corpse for auto-revive.
pub fn set_auto(w: &mut World, p: u8) -> bool {
    if let Some(r) = of_player(w, p)
        && let Some(Kind::Reviver(t)) = w.kind_mut(r)
        && !t.auto
    {
        t.auto = true;
        return true;
    }
    false
}

/// `PlayerCorpse.Reviving`: a reviver is lifting this corpse.
pub fn corpse_reviving(w: &World, corpse: EntityId) -> bool {
    revivers(w).iter().any(|&r| {
        matches!(w.kind(r), Some(Kind::Reviver(t)) if t.corpse == corpse && (t.reviving || t.seq.is_some()))
    })
}

/// Drop every auto-revive flag (Dark World revive timeout).
pub fn clear_auto_all(w: &mut World) {
    for r in revivers(w) {
        if let Some(Kind::Reviver(t)) = w.kind_mut(r) {
            t.auto = false;
        }
    }
}

/// A reviver mid-revive, or auto-flagged and unfinished.
pub fn any_pending(w: &World) -> bool {
    revivers(w).iter().any(
        |&r| matches!(w.kind(r), Some(Kind::Reviver(t)) if t.reviving || t.seq.is_some() || t.auto),
    )
}

/// Free bottom-centre spot for an 8x14 archer at `base + (0, 0..=8 step 2)`.
fn free_spot(w: &World, c: &Content, base: Vec2, me: EntityId) -> Option<Vec2> {
    for dy in [0.0, 2.0, 4.0, 6.0, 8.0] {
        let bottom = base + Vec2::new(0.0, dy);
        let p = Placed {
            pos: bottom - Vec2::new(0.0, 8.0),
            collider: Collider::Box(player::NORMAL_HITBOX),
        };
        if !w.any(c, &p, Tag::Solid, me) {
            return Some(bottom);
        }
    }
    None
}

/// `ReviveUpdate` for every reviver (before the entity updates).
pub fn update_all(w: &mut World, c: &Content) {
    for r in revivers(w) {
        let Some(Kind::Reviver(mut t)) = w.kind(r).copied() else {
            continue;
        };
        update(w, c, r, &mut t);
        if let Some(Kind::Reviver(slot)) = w.kind_mut(r) {
            *slot = t;
        }
    }
}

fn update(w: &mut World, c: &Content, id: EntityId, t: &mut TeamReviver) {
    let tm = w.time.time_mult;
    if let Some(s) = t.seq {
        finish_sequence(w, c, id, t, s + tm);
        return;
    }
    let Some(cp) = w.try_core(t.corpse).copied() else {
        w.remove(id);
        return;
    };
    let squished = matches!(w.kind(t.corpse), Some(Kind::Corpse(k)) if k.is_squished());
    let bottom = cp.pos + Vec2::new(0.0, 8.0);
    w.core_mut(id).pos = bottom;
    t.age += tm;
    if t.age < 60.0 {
        return;
    }
    let prism_hit = matches!(w.kind(t.corpse), Some(Kind::Corpse(k)) if k.prism_t >= 0.0);
    let blocked = squished || prism_hit || free_spot(w, c, bottom, id).is_none();
    let mut who = None;
    if t.auto {
        who = Some(None);
    } else if t.mode != ReviveMode::Quest {
        let probe = Placed {
            pos: bottom,
            collider: Collider::Box(if t.reviving { BIG } else { SMALL }),
        };
        let mates: Vec<EntityId> = w
            .players()
            .filter(|&p| {
                w.player(p)
                    .is_some_and(|q| !q.dead && q.allegiance == t.allegiance)
                    && w.check(c, &probe, p)
            })
            .collect();
        let keep = t.reviver.filter(|r| mates.contains(r));
        if let Some(k) = keep.or(mates.first().copied()) {
            who = Some(Some(k));
        }
    }
    match (who, blocked) {
        (Some(rev), false) => {
            if !t.reviving {
                t.reviving = true;
                w.core_mut(id).collider = Collider::Box(BIG);
                w.sound(team_cue(t, 0), cp.pos.x);
                t.lift_y = cp.pos.y - 6.0;
                t.sine = crate::components::SineWave::frames(90.0);
                if let Some(Kind::Corpse(k)) = w.kind_mut(t.corpse) {
                    k.pinned = false;
                    k.ledge = 0;
                }
            }
            t.reviver = rev;
            t.counter -= tm;
            levitate(w, c, t, cp.pos, rev, squished, tm);
            if t.counter <= 0.0 {
                t.revived = true;
                crate::coop::on_revived(w, t.player_index);
                crate::enemies::player_ghost::despawn(w, c, t.player_index, cp.pos);
                t.seq = Some(0.0);
            }
        }
        _ => {
            if t.reviving {
                w.sound(team_cue(t, 1), cp.pos.x);
                t.reviving = false;
                t.counter = 60.0;
                t.levitate = 0.0;
                w.core_mut(id).collider = Collider::Box(SMALL);
            }
        }
    }
}

/// The corpse rises toward `lift_y` (with a 2 px bob, at most 0.6/tick)
/// and, unless auto-reviving, follows the reviving teammate (0.2 / 0.6).
#[allow(clippy::too_many_arguments)]
fn levitate(
    w: &mut World,
    c: &Content,
    t: &mut TeamReviver,
    at: Vec2,
    rev: Option<EntityId>,
    squished: bool,
    tm: f32,
) {
    t.sine.update(tm);
    let mut v = Vec2::new(0.0, (t.lift_y + t.sine.value * 2.0 - at.y).clamp(-0.6, 0.6));
    if !t.auto
        && let Some(r) = rev.filter(|&r| w.exists(r))
    {
        let rp = w.core(r).pos;
        let me = w.core(t.corpse).pos;
        let ws = w.world_size(c);
        let dx = crate::actor::wrap_diff_x(me.x, rp.x, ws.w);
        if dx.abs() > 10.0 {
            v.x = dx.signum() * 0.2;
        }
        let dy = rp.y - me.y;
        if dy.abs() > 14.0 {
            v.y = dy.signum() * 0.6;
        }
    }
    if !squished && let Some(Kind::Corpse(k)) = w.kind_mut(t.corpse) {
        k.speed = crate::math::approach_vec(k.speed, v, 4.0 * tm);
    }
}

/// `Revive`: wait(5); create the archer; wait(1) remove corpse; wait(15)
/// voice; wait(15) unfreeze; done.
fn finish_sequence(w: &mut World, c: &Content, id: EntityId, t: &mut TeamReviver, s: f32) {
    t.seq = Some(s);
    if s >= 6.0 && t.new_player.is_none() {
        let Some(cp) = w.try_core(t.corpse).copied() else {
            w.remove(id);
            return;
        };
        let bottom = cp.pos + Vec2::new(0.0, 8.0);
        let Some(spot) = free_spot(w, c, bottom, id) else {
            // Cancelled: the corpse stays.
            t.seq = None;
            t.reviving = false;
            t.revived = false;
            t.auto = false;
            t.counter = 60.0;
            crate::coop::on_revive_failed(w, t.player_index);
            return;
        };
        let (archer, mut arrows) = match w.kind(t.corpse) {
            Some(Kind::Corpse(k)) => (k.archer, k.arrows),
            _ => (0, crate::quiver::Quiver::default()),
        };
        if let Some(Kind::Corpse(k)) = w.kind(t.corpse).copied()
            && let Some(a) = k.cushion.iter().next()
            && let Some(at) = w.arrow(a).map(|x| x.arrow_type)
            && arrows.can_add_arrow(at)
        {
            arrows.add_arrows(&[at]);
            w.remove(a);
        }
        if t.mode == ReviveMode::Quest && arrows.is_empty() {
            arrows.add_arrows(&[ArrowType::Normal]);
        }
        let ps = PlayerSetup {
            archer,
            allegiance: t.allegiance,
            team_color: if t.mode == ReviveMode::Quest {
                Allegiance::Neutral
            } else {
                t.allegiance
            },
            hat: HatState::NoHat,
            arrows: 0,
            ..Default::default()
        };
        let pid = player::create(w, c, t.player_index, spot - Vec2::new(0.0, 8.0), &ps);
        if let Some(p) = w.player_mut(pid) {
            p.arrows = arrows;
            p.sm = StateMachine::new(PState::Frozen);
        }
        let flash = if t.mode == ReviveMode::TeamDeathmatch {
            45.0
        } else {
            135.0
        };
        w.core_mut(pid)
            .flash
            .start_for(flash, crate::entity::FlashFinish::None);
        if t.mode == ReviveMode::DarkWorld
            && let Some(r) = t.reviver.filter(|&r| w.exists(r))
        {
            w.core_mut(r)
                .flash
                .start_for(60.0, crate::entity::FlashFinish::None);
        }
        if let Some(r) = t.reviver
            && let Some(rp) = w.player(r).map(|p| p.index)
        {
            crate::versus::stat(w, rp, |s| s.revives += 1);
            crate::darkworld::on_revived(w, Some(rp));
        }
        t.new_player = Some(pid);
        let at = w.core(pid).pos;
        crate::explosion::spawn_team_revive(w, at, t.player_index as i8);
        w.sound(team_cue(t, 2), spot.x);
    }
    if s >= 7.0 && w.exists(t.corpse) {
        w.remove(t.corpse);
    }
    if s >= 37.0 {
        if let Some(pid) = t.new_player.filter(|&p| w.exists(p)) {
            player::unfreeze(w, c, pid);
        }
        w.remove(id);
    }
}
