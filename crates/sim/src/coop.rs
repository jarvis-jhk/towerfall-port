//! Co-op mode dispatch (Quest, Trials; Dark World later): the hooks the
//! world and the archer call, routed to whichever mode is running. Keeps
//! the shared files down to one call per hook.

use crate::content::Content;
use crate::entity::{EntityId, Kind};
use crate::math::Vec2;
use crate::player::DeathCause;
use crate::world::World;

/// `RegisterEnemyKill(position, killer, bounty)`.
pub fn register_enemy_kill(w: &mut World, pos: Vec2, killer: i8, bounty: u16) {
    crate::quest::register_kill(w, pos, killer, bounty);
    crate::darkworld::register_kill(w, pos, killer, bounty);
}

/// Session update (unfrozen ticks, before the entities).
pub fn session_update(w: &mut World, c: &Content) {
    crate::revive::update_all(w, c);
    crate::quest::session_update(w, c);
    crate::trials::session_update(w, c);
    crate::darkworld::session_update(w, c);
}

/// Between ticks (frozen or not).
pub fn end_of_tick(w: &mut World, c: &Content) {
    crate::quest::end_of_tick(w, c);
    crate::trials::end_of_tick(w, c);
    crate::darkworld::end_of_tick(w, c);
}

/// A player died (after the corpse exists).
pub fn on_player_death(
    w: &mut World,
    c: &Content,
    victim: u8,
    killer: i8,
    cause: DeathCause,
    at: Vec2,
) {
    crate::variant_fx::on_player_death(w, victim, at);
    crate::quest::on_player_death(w, c, victim, killer, cause, at);
    crate::darkworld::on_player_death(w, c, victim, killer, cause, at);
}

/// A revive completed its countdown (the corpse counts as alive).
pub fn on_revived(w: &mut World, p: u8) {
    if let Some(m) = w.versus.as_mut() {
        m.round.alive[p as usize] = true;
    }
}

/// A revive could not place the archer.
pub fn on_revive_failed(w: &mut World, p: u8) {
    crate::versus::revive_failed(w, p);
    crate::quest::check_game_over(w);
    crate::darkworld::check_game_over(w);
}

/// An entity left the scene (`Removed`).
pub fn on_removed(w: &mut World, id: EntityId) {
    if let Some(Kind::Enemy(e)) = w.kind(id)
        && e.alive()
        && e.ty != crate::enemies::EnemyType::PlayerGhost
    {
        let pos = w.core(id).pos;
        register_enemy_kill(w, pos, -1, 0);
    }
}

/// `MatchSettings.SoloMode`: Quest and Trials (Dark World later).
pub fn solo_mode(w: &World) -> bool {
    w.quest.is_some() || w.trials.is_some() || w.dark_world.is_some()
}

/// `ShockCircle.CanDamagePlayer` for a TeamRevive shock of `revived`:
/// never in solo modes; else only archers of another allegiance (a
/// neutral reviver hurts everybody).
pub fn team_revive_can_hurt(w: &World, revived: i8, victim: i8) -> bool {
    if solo_mode(w) || revived < 0 {
        return false;
    }
    let team = |i: i8| {
        w.setup
            .players
            .get(i as usize)
            .and_then(|p| p.as_ref())
            .map(|p| p.allegiance)
            .unwrap_or(crate::entity::Allegiance::Neutral)
    };
    let a = team(revived);
    a == crate::entity::Allegiance::Neutral || a != team(victim)
}
