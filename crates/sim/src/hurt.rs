//! Damage entry points of the archer that the M5 hazards use (player.md
//! §13.1–13.5): hazard `Hurt`, `Hurt(lava)`, explosion push and kill, the
//! toy-arrow bounce, fire; plus the session's friendly-fire predicates.
//!
//! Many callers run inside some entity's update while the archer's state
//! may be out of its slot (taken by its own update). [`Ctx`] carries the
//! archer whose update is running so such calls reach the right struct.

use crate::content::Content;
use crate::entity::{Allegiance, EntityId, Kind};
use crate::math::Vec2;
use crate::player::{self, DeathCause, PState, Player};
use crate::world::World;

/// The archer whose update is running (its state is borrowed), if any.
pub type Ctx<'a> = Option<(EntityId, &'a mut Player)>;

/// Run `f` on player `id` (taking its state out of the slot).
pub fn with_player<R>(
    w: &mut World,
    id: EntityId,
    f: impl FnOnce(&mut World, &mut Player) -> R,
) -> Option<R> {
    // Only take a player's state (other kinds must stay in their slot).
    if !matches!(w.kind(id), Some(Kind::Player(_))) {
        return None;
    }
    let Some(Kind::Player(mut p)) = w.take_kind(id) else {
        return None;
    };
    let r = f(w, &mut p);
    w.put_kind(id, Kind::Player(p));
    Some(r)
}

/// Like [`with_player`], but uses the borrowed state when `id` is the
/// archer of `ctx`.
pub fn with_player_ctx<R>(
    w: &mut World,
    id: EntityId,
    ctx: &mut Ctx,
    f: impl FnOnce(&mut World, &mut Player) -> R,
) -> Option<R> {
    if let Some((cid, p)) = ctx
        && *cid == id
    {
        return Some(f(w, p));
    }
    with_player(w, id, f)
}

fn allegiance_of(w: &World, index: i8) -> Allegiance {
    if index < 0 {
        return Allegiance::Neutral;
    }
    w.setup
        .players
        .get(index as usize)
        .and_then(|p| p.as_ref())
        .map(|p| p.allegiance)
        .unwrap_or(Allegiance::Neutral)
}

/// `Session.IsFriendly(attacker, victim)`: same team in team modes, the
/// same player in free-for-all; index -1 is never friendly.
pub fn is_friendly(w: &World, attacker: i8, victim: i8) -> bool {
    if attacker < 0 || victim < 0 {
        return false;
    }
    let (a, v) = (allegiance_of(w, attacker), allegiance_of(w, victim));
    if a != Allegiance::Neutral && v != Allegiance::Neutral {
        a == v
    } else {
        attacker == victim
    }
}

/// `Session.CanHurtPlayer(attacker, victim)` (NoFriendlyFire).
pub fn can_hurt_player(w: &World, attacker: i8, victim: i8) -> bool {
    if !w.variants_for(-1).no_friendly_fire || attacker < 0 || victim < 0 {
        return true;
    }
    !is_friendly(w, attacker, victim)
}

/// `Hurt(cause, bounceFrom, attacker, sfx)` (§13.5) via
/// [`player::hurt`]. Returns true if the archer died.
#[allow(clippy::too_many_arguments)]
pub fn hurt(
    w: &mut World,
    c: &Content,
    id: EntityId,
    p: &mut Player,
    cause: DeathCause,
    from: Vec2,
    attacker: i8,
    sfx: Option<&'static str>,
) -> bool {
    if p.dead {
        return false;
    }
    player::hurt(w, c, id, p, cause, from, attacker, sfx);
    p.dead
}

/// `OnSqueakyBounce(arrow, normal)`: back to Normal, launched at 5.
pub fn on_squeaky_bounce(w: &mut World, c: &Content, id: EntityId, p: &mut Player, v: Vec2) {
    if p.dead {
        return;
    }
    player::set_state(w, c, id, p, PState::Normal);
    p.speed = v * 5.0;
}
