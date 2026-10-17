//! Enemy explosions (bomb bats, exploders, techno missiles, King Reaper
//! bombs) go through the shared explosion (`entities::explosion`,
//! arrows.md §13.1), which also pushes/kills enemies and dummies.

use super::*;

/// `Explosion.Spawn(at, owner)`: false inside a solid.
pub fn explode(w: &mut World, c: &Content, at: Vec2, owner: i8, super_: bool) -> bool {
    let ws = w.world_size(c);
    crate::entities::explosion::spawn(w, c, wrap(at, ws.w, ws.h), owner, false, super_, None)
}
