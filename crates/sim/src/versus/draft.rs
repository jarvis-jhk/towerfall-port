//! Treasure Draft (versus.md §13.2, pickups.md §11): inside every round's
//! intro the archers pick one offered pickup each, one after the other.
//! The sim owns the pick order, the offers, the cursor (driven by the
//! picker's input), the gains and the "finished" signal that lets the intro
//! continue; panels, chests and pointers are drawn by the game from
//! [`DraftView`].
//!
//! Timings follow the original's sequence (ticks): 10 title, 10 chests
//! drop, 10 pointers enter (2 apart), 5, chests open, 20; per picker 20
//! (pointer moves in), then input (a move blocks 5 ticks), the pick is
//! applied 10 ticks after it; 30 ticks after the last gain the draft is
//! finished.

use super::{Match, Mode};
use crate::checksum::{Hasher, StableHash};
use crate::content::Content;
use crate::pickups::Pickups;
use crate::world::{MAX_PLAYERS, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Panel, chests and pointers come in.
    Opening,
    /// The active pointer moves in (no input yet).
    PointerIn,
    /// The active picker chooses.
    Choosing,
    /// The pick is taken; it is applied when the wait runs out.
    Applying,
    /// After the last gain.
    Closing,
    Finished,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    /// Player slots in pick order.
    pub order: Vec<u8>,
    /// Remaining offers, left to right.
    pub offers: Vec<Pickups>,
    /// Picks made so far (slot, pickup), in order.
    pub picks: Vec<(u8, Pickups)>,
    /// Index into `order` of the active picker.
    pub current: usize,
    /// Cursor over `offers`.
    pub cursor: usize,
    pub stage: Stage,
    /// Ticks left in the current stage (or the move lock while choosing).
    pub wait: f32,
    /// Ticks since the draft started (UI clock).
    pub t: f32,
}

impl StableHash for Draft {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u32(self.order.len() as u32);
        for &o in &self.order {
            h.u8(o);
        }
        h.u32(self.offers.len() as u32);
        for &o in &self.offers {
            h.u8(o as u8);
        }
        for &(p, k) in &self.picks {
            h.u8(p);
            h.u8(k as u8);
        }
        h.u32(self.current as u32);
        h.u32(self.cursor as u32);
        h.u8(self.stage as u8);
        h.f32(self.wait);
        h.f32(self.t);
    }
}

/// What the HUD needs to draw the draft.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftView {
    pub order: Vec<u8>,
    pub offers: Vec<Pickups>,
    pub picks: Vec<(u8, Pickups)>,
    /// The active picker (slot), while picking.
    pub picker: Option<u8>,
    pub cursor: usize,
    pub stage: Stage,
    pub t: f32,
}

impl Draft {
    pub fn finished(&self) -> bool {
        self.stage == Stage::Finished
    }

    pub fn view(&self) -> DraftView {
        DraftView {
            order: self.order.clone(),
            offers: self.offers.clone(),
            picks: self.picks.clone(),
            picker: matches!(
                self.stage,
                Stage::PointerIn | Stage::Choosing | Stage::Applying
            )
            .then(|| self.order.get(self.current).copied())
            .flatten(),
            cursor: self.cursor,
            stage: self.stage,
            t: self.t,
        }
    }
}

// --------------------------------------------------------------- order

/// Slots that take part: in the match and spawned this round.
fn participants(m: &Match) -> Vec<usize> {
    (0..MAX_PLAYERS)
        .filter(|&p| m.active[p] && m.round.spawned[p])
        .collect()
}

/// The pick order (pickups.md §11 "Participants and pick order").
pub fn pick_order(w: &mut World, m: &Match) -> Vec<u8> {
    let parts = participants(m);
    let mut out: Vec<usize> = vec![];
    if m.settings.team_mode() {
        let team = |t: u8| -> Vec<usize> {
            parts
                .iter()
                .copied()
                .filter(|&p| m.settings.slots[p].is_some_and(|s| s.team == t))
                .collect()
        };
        if m.settings.teams == 2 {
            let (blue, red) = (team(0), team(1));
            if blue.is_empty() || red.is_empty() {
                out = parts.clone();
            } else if blue.len() == red.len() {
                // The original's quirk: Red counts as the leader unless Blue
                // has already won.
                let (mut lead, mut trail) = if m.get_winner() == 0 {
                    (blue, red)
                } else {
                    (red, blue)
                };
                w.rng.shuffle(&mut lead);
                w.rng.shuffle(&mut trail);
                if m.score_lead(m.settings.score_index(lead[0])) >= 3 {
                    out.extend(&trail);
                    out.extend(&lead);
                } else {
                    // `trailing[0]`, the leaders, `trailing[1]`; the port
                    // appends any further trailing members (teams > 2 a side).
                    out.push(trail[0]);
                    out.extend(&lead);
                    out.extend(&trail[1..]);
                }
            } else {
                let (small, mut large) = if blue.len() < red.len() {
                    (blue, red)
                } else {
                    (red, blue)
                };
                w.rng.shuffle(&mut large);
                let lead = m.score_lead(m.settings.score_index(small[0]));
                if lead <= 0 {
                    out.extend(&small);
                    out.extend(&large);
                } else {
                    let k = (lead as usize).min(large.len());
                    out.extend(&large[..k]);
                    out.extend(&small);
                    out.extend(&large[k..]);
                }
            }
        } else {
            // Port extension for 3+ teams: teams by ascending score (ties in
            // random order), members shuffled.
            let mut teams: Vec<u8> = (0..m.settings.teams).collect();
            w.rng.shuffle(&mut teams);
            teams.sort_by_key(|&t| m.scores[t as usize]);
            for t in teams {
                let mut l = team(t);
                w.rng.shuffle(&mut l);
                out.extend(l);
            }
        }
    } else {
        // Ascending score, equal scores shuffled.
        let mut rest = parts.clone();
        while !rest.is_empty() {
            let low = rest.iter().map(|&p| m.scores[p]).min().unwrap();
            let mut tier: Vec<usize> = rest
                .iter()
                .copied()
                .filter(|&p| m.scores[p] == low)
                .collect();
            rest.retain(|&p| m.scores[p] != low);
            w.rng.shuffle(&mut tier);
            out.extend(tier);
        }
        if m.is_in_overtime() {
            let hi = m.highest();
            out.retain(|&p| m.scores[p] >= hi);
        }
    }
    out.into_iter().map(|p| p as u8).collect()
}

// ---------------------------------------------------------------- flow

/// Start the draft for the round (from `begin_round`), or `None` when the
/// variant is off or nothing can be offered.
pub fn start(w: &mut World, m: &Match) -> Option<Draft> {
    if !m
        .settings
        .variants
        .any(crate::variants::Variant::TreasureDraft)
    {
        return None;
    }
    let order = pick_order(w, m);
    if order.is_empty() {
        return None;
    }
    let idx: Vec<usize> = (0..m.settings.score_count()).collect();
    let all_tied = idx.iter().map(|&i| m.scores[i]).max() == idx.iter().map(|&i| m.scores[i]).min();
    let spawn_shield = participants(m)
        .into_iter()
        .any(|p| super::spawn::spawn_shield(m, p));
    let even = m.settings.mode == Mode::TeamDeathmatch && {
        let t: Vec<usize> = (0..m.settings.teams)
            .map(|t| m.settings.team_size(t))
            .collect();
        t.iter().all(|&n| n == t[0])
    };
    let offers = crate::treasure::draft_pickups(w, order.len(), all_tied, spawn_shield, even);
    if offers.is_empty() {
        return None;
    }
    let n = order.len() as f32;
    Some(Draft {
        order,
        offers,
        picks: vec![],
        current: 0,
        cursor: 0,
        stage: Stage::Opening,
        wait: 10.0 + 10.0 + 10.0 + 2.0 * n + 5.0 + 20.0,
        t: 0.0,
    })
}

/// One intro tick of the draft.
pub fn update(w: &mut World, c: &Content, d: &mut Draft) {
    d.t += 1.0;
    if d.wait > 0.0 {
        d.wait -= 1.0;
    }
    match d.stage {
        Stage::Opening if d.wait <= 0.0 => {
            d.stage = Stage::PointerIn;
            d.wait = 20.0;
        }
        Stage::PointerIn if d.wait <= 0.0 => {
            d.stage = Stage::Choosing;
            d.cursor = 0;
        }
        Stage::Choosing if d.wait <= 0.0 => choose(w, d),
        Stage::Applying if d.wait <= 0.0 => {
            if let Some(&(slot, kind)) = d.picks.last() {
                apply(w, c, slot, kind);
            }
            d.current += 1;
            if d.current < d.order.len() {
                d.stage = Stage::PointerIn;
                d.wait = 20.0;
            } else {
                d.stage = Stage::Closing;
                d.wait = 30.0;
            }
        }
        Stage::Closing if d.wait <= 0.0 => d.stage = Stage::Finished,
        _ => {}
    }
}

fn choose(w: &mut World, d: &mut Draft) {
    let slot = d.order[d.current];
    let auto = d.offers.iter().all(|&o| o == d.offers[0]);
    let s = w.inputs[slot as usize].state;
    let confirm = s.jump_pressed || s.start_pressed;
    let gone = !w.connected[slot as usize];
    if auto || confirm || gone {
        let kind = d.offers.remove(d.cursor.min(d.offers.len() - 1));
        d.picks.push((slot, kind));
        d.cursor = 0;
        d.stage = Stage::Applying;
        d.wait = 10.0;
        w.sound("ui_click", 160.0);
        return;
    }
    if s.move_x < 0 && d.cursor > 0 {
        d.cursor -= 1;
        d.wait = 5.0;
        w.sound("ui_move2", 160.0);
    } else if s.move_x > 0 && d.cursor + 1 < d.offers.len() {
        d.cursor += 1;
        d.wait = 5.0;
        w.sound("ui_move2", 160.0);
    }
}

/// `DoPlayerCollect`: the gain without the guards.
fn apply(w: &mut World, c: &Content, slot: u8, kind: Pickups) {
    if let Some(id) = w.player_by_index(slot) {
        crate::hurt::with_player(w, id, |w, p| {
            crate::pickups::gain(w, c, id, p, kind, false);
        });
    }
}
