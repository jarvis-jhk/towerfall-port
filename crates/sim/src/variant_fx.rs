//! Variant behaviours that belong to no other system (versus.md §13.2 and
//! §13.3, player.md §3.3 / §14): the Gunn Style freeze-frame, the Dark
//! Portals sequence, exploding / trigger corpses, corpses that throw out
//! their arrows, and Stealth Archers. Also the view data of Show Dodge
//! Cooldown and Show Treasure Spawns.

use crate::checksum::{Hasher, StableHash};
use crate::content::Content;
use crate::corpse::Corpse;
use crate::enemies::{self, EnemyType, portal};
use crate::entity::{EntityId, Kind, Tag};
use crate::events::Event;
use crate::math::{PI, Vec2};
use crate::pickups::{variant, variant_any};
use crate::variants::Variant;
use crate::world::World;

/// Gunn Style: ticks from the death to the freeze, the frozen span and the fade.
pub const GUNN_DELAY: f32 = 10.0;
pub const GUNN_HOLD: f32 = 30.0;
pub const GUNN_FADE: f32 = 10.0;

/// One Gunn Style freeze-frame (one per death).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gunn {
    pub t: f32,
    pub victim: u8,
    /// Where the archer died (the corpse silhouette is drawn there).
    pub at: Vec2,
}

/// The Dark Portals sequence of the current round.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct DarkPortals {
    pub t: f32,
    pub portal: Option<EntityId>,
    /// Ticks until the next enemy check.
    pub next: f32,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct VariantFx {
    pub gunn: Vec<Gunn>,
    pub dark: Option<DarkPortals>,
    /// The Dark Portals sequence was considered for this scene.
    pub dark_checked: bool,
}

impl StableHash for VariantFx {
    fn stable_hash(&self, h: &mut Hasher) {
        h.u32(self.gunn.len() as u32);
        for g in &self.gunn {
            h.f32(g.t);
            h.u8(g.victim);
            h.vec2(g.at);
        }
        h.bool(self.dark.is_some());
        if let Some(d) = &self.dark {
            h.f32(d.t);
            d.portal.stable_hash(h);
            h.f32(d.next);
        }
        h.bool(self.dark_checked);
    }
}

// ------------------------------------------------------------ per tick

/// After the entity updates: Gunn Style runs even while the level is
/// frozen (a HUD entity in the original); Dark Portals only when not.
pub fn update(w: &mut World, c: &Content) {
    gunn_update(w);
    if !w.frozen {
        dark_portals_update(w, c);
    }
}

// ---------------------------------------------------------- Gunn Style

/// `RoundLogic.OnPlayerDeath`: a freeze-frame per death with Gunn Style on.
pub fn on_player_death(w: &mut World, victim: u8, at: Vec2) {
    if variant_any(w, Variant::GunnStyle) {
        w.vfx.gunn.push(Gunn { t: 0.0, victim, at });
    }
}

/// A freeze-frame was holding the level before this tick.
fn had_freeze(before: &[Gunn]) -> bool {
    before.iter().any(|g| g.t >= GUNN_DELAY)
}

fn gunn_update(w: &mut World) {
    if w.vfx.gunn.is_empty() {
        return;
    }
    let mut list = std::mem::take(&mut w.vfx.gunn);
    let was_frozen = had_freeze(&list);
    for g in &mut list {
        let before = g.t;
        g.t += 1.0;
        if before < GUNN_DELAY && g.t >= GUNN_DELAY {
            w.set_frozen(true);
            w.sound("sfx_gunnFreeze", 160.0);
        }
    }
    list.retain(|g| g.t < GUNN_DELAY + GUNN_HOLD + GUNN_FADE);
    // The last running freeze-frame lifts the freeze, unless the round
    // ended meanwhile (the results keep the level frozen).
    let any_frozen = list.iter().any(|g| g.t >= GUNN_DELAY);
    let playing = w
        .versus
        .as_ref()
        .is_none_or(|m| m.phase == crate::versus::Phase::Playing && !m.round_end_pending());
    if !any_frozen && w.frozen && playing && was_frozen {
        w.set_frozen(false);
    }
    w.vfx.gunn = list;
}

/// The Gunn Style overlay for the view: the victim's spot and the overlay
/// alpha (1 while frozen, fading over the last 10 ticks).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GunnView {
    pub victim: u8,
    pub at: Vec2,
    pub alpha: f32,
}

pub fn gunn_view(w: &World) -> Option<GunnView> {
    w.vfx.gunn.iter().find(|g| g.t >= GUNN_DELAY).map(|g| {
        let fade = g.t - GUNN_DELAY - GUNN_HOLD;
        let alpha = if fade <= 0.0 {
            1.0
        } else {
            1.0 - crate::components::Ease::CubeIn.apply((fade / GUNN_FADE).min(1.0))
        };
        GunnView {
            victim: g.victim,
            at: g.at,
            alpha,
        }
    })
}

// --------------------------------------------------------- Dark Portals

const DARK_FIRST: f32 = 21.0;
const DARK_SECOND: f32 = 21.0;
const DARK_EVERY: f32 = 61.0;

/// `DarkPortalsVariantSequence` (tower-entities.md §11.5, enemies.md §5):
/// once the round started, a portal at a random `Spawner` marker that
/// spawns a Bat or a Slime whenever fewer than 3 enemies exist.
fn dark_portals_update(w: &mut World, c: &Content) {
    if !w.vfx.dark_checked {
        w.vfx.dark_checked = true;
        if w.versus.is_some() && variant_any(w, Variant::DarkPortals) {
            w.vfx.dark = Some(DarkPortals::default());
        }
    }
    let Some(mut d) = w.vfx.dark else { return };
    if !crate::pickups::round_started(w) {
        return;
    }
    d.t += w.time.time_mult;
    match d.portal {
        None if d.t >= DARK_FIRST => {
            let n = c.level(w.level).markers.spawners.len();
            if n == 0 {
                w.vfx.dark = None;
                return;
            }
            let k = w.rng.choose_index(n);
            let id = portal::add_portal(w, c, k);
            portal::appear(w, id);
            d.portal = Some(id);
            d.next = DARK_SECOND;
        }
        Some(id) => {
            d.next -= w.time.time_mult;
            if d.next <= 0.0 {
                d.next += DARK_EVERY;
                if enemies::enemy_count(w) + portal::queued_total(w) < 3 {
                    let ty = if w.rng.next_max(2) == 0 {
                        EnemyType::Bat
                    } else {
                        EnemyType::Slime
                    };
                    portal::spawn_enemy(w, id, ty);
                }
            }
        }
        None => {}
    }
    w.vfx.dark = Some(d);
}

// ------------------------------------------------------------- corpses

/// Ticks between two arrows thrown by a Corpses Drop Arrows corpse.
pub const DROP_EVERY: f32 = 10.0;

/// Corpse variant state at creation (player.md §14.1): the drop alarm.
pub fn corpse_added(w: &World, cp: &mut Corpse) {
    if cp.player_index >= 0
        && variant(w, Variant::CorpsesDropArrows, cp.player_index as i32)
        && !cp.arrows.is_empty()
    {
        cp.drop_t = DROP_EVERY;
        cp.drop_dir = 1;
    }
}

fn can_explode(w: &World, cp: &Corpse) -> bool {
    cp.player_index >= 0
        && cp.prism_t < 0.0
        && (variant(w, Variant::ExplodingCorpses, cp.player_index as i32)
            || variant(w, Variant::TriggerCorpses, cp.player_index as i32))
}

/// Per tick, inside the corpse update. Returns true when the corpse
/// exploded (it is removed; the caller stops).
pub fn corpse_update(w: &mut World, c: &Content, id: EntityId, cp: &mut Corpse) -> bool {
    let tm = w.time.time_mult;
    // DodgesTooLate: the owner pressed Dodge within 10 frames of death.
    if cp.dodge_late > 0.0 && cp.player_index >= 0 {
        cp.dodge_late -= tm;
        let pi = cp.player_index as u8;
        if w.inputs[pi as usize].state.dodge_pressed {
            cp.dodge_late = 0.0;
            crate::versus::stat(w, pi, |s| s.dodges_too_late += 1);
        }
    }
    // Corpses Drop Arrows (looping 10-tick alarm).
    if cp.drop_t > 0.0 {
        cp.drop_t -= tm;
        if cp.drop_t <= 0.0 {
            cp.drop_t += DROP_EVERY;
            drop_next_arrow(w, id, cp);
            if cp.arrows.is_empty() {
                cp.drop_t = -1.0;
            }
        }
    }
    if !can_explode(w, cp) {
        return false;
    }
    let pi = cp.player_index;
    let trigger = variant(w, Variant::TriggerCorpses, pi as i32);
    let (rate, charge) = if trigger {
        let s = w.inputs[pi as usize].state;
        let held = s.shoot_check || s.dodge_check || s.jump_check;
        if held && cp.explode_t < 0.75 {
            w.emit(Event::Rumble {
                player: pi as u8,
                strength: 0.2,
                frames: 2,
            });
        }
        (40.0, held)
    } else {
        (60.0, true)
    };
    let x = w.core(id).pos.x;
    if !charge || cp.is_squished() {
        if cp.charging {
            cp.charging = false;
            w.emit(Event::Loop {
                name: "sfx_bombChestLoop",
                player: u8::MAX,
                on: false,
                x,
            });
        }
        cp.explode_t = (cp.explode_t - tm / rate).max(0.0);
        return false;
    }
    cp.explode_t = (cp.explode_t + tm / rate).min(1.0);
    if cp.explode_t >= 1.0 {
        explode(w, c, id, cp);
        return true;
    }
    if !cp.charging {
        cp.charging = true;
        w.emit(Event::Loop {
            name: "sfx_bombChestLoop",
            player: u8::MAX,
            on: true,
            x,
        });
    }
    false
}

/// `PlayerCorpse.Explode` (player.md §14.5).
fn explode(w: &mut World, c: &Content, id: EntityId, cp: &mut Corpse) {
    let at = w.core(id).pos;
    if cp.charging {
        cp.charging = false;
        w.emit(Event::Loop {
            name: "sfx_bombChestLoop",
            player: u8::MAX,
            on: false,
            x: at.x,
        });
    }
    w.sound("pu_bombArrowExplode", at.x);
    w.core_mut(id).collidable = false;
    // A trigger-bomb blast (feeds TriggerBombKills).
    w.items.blast = crate::pickups::Blast::Trigger;
    crate::explosion::spawn(w, c, at, cp.player_index, false, &mut None);
    w.items.blast = crate::pickups::Blast::Normal;
    let speed = cp.speed;
    crate::corpse::release_arrows(w, cp, speed);
    w.emit(Event::Rumble {
        player: cp.player_index as u8,
        strength: 1.0,
        frames: 30,
    });
    w.remove(id);
}

/// `DropNextArrow`: one arrow from the corpse's quiver, thrown up and to
/// alternating sides.
fn drop_next_arrow(w: &mut World, id: EntityId, cp: &mut Corpse) {
    if cp.is_squished() || cp.arrows.is_empty() {
        return;
    }
    let Some(t) = cp.arrows.use_arrow() else {
        return;
    };
    let at = w.core(id).pos;
    let dir = -0.75 * PI + w.rng.next_float() * (PI / 2.0);
    let shooter = crate::arrow::Shooter {
        id: Some(id),
        player_index: cp.player_index,
        allegiance: cp.allegiance,
        team_color: cp.team_color,
        hyper: false,
    };
    let a = crate::arrow::create(w, t, shooter, at, dir);
    let side = (-cp.facing * cp.drop_dir as i32) as f32;
    cp.drop_dir = -cp.drop_dir;
    crate::arrows::with_arrow(w, a, |w, ar| crate::arrow::drop(w, a, ar, side));
    w.sound("sfx_arrowDrop", at.x);
}

// ------------------------------------------------------------- archers

/// `Player.Added` variant steps (player.md §3.3): Stealth Archers removes
/// the light source.
pub fn player_added(w: &mut World, id: EntityId, index: u8) {
    if variant(w, Variant::StealthArchers, index as i32) {
        w.tag(id, Tag::LightSource, false);
    }
}

/// Show Dodge Cooldown (archers) and the enemy cooldown displays
/// (enemies.md §1.10): positions of lit cooldown stars.
pub fn cooldown_stars(w: &World) -> Vec<Vec2> {
    let mut out = vec![];
    for id in w.players() {
        let Some(p) = w.player(id) else { continue };
        if !p.dead && p.dodge_cooldown && variant(w, Variant::ShowDodgeCooldown, p.index as i32) {
            out.push(w.core(id).pos + Vec2::new(0.0, -10.0));
        }
    }
    for &id in w.tagged(Tag::Enemy) {
        if let Some(Kind::Enemy(e)) = w.kind(id)
            && let Some(off) = enemies::cooldown_star(e)
        {
            out.push(w.core(id).pos + off);
        }
    }
    out
}

/// Show Treasure Spawns: chests still waiting to appear (spot, large).
pub fn treasure_previews(w: &World) -> Vec<(Vec2, bool)> {
    if !variant_any(w, Variant::ShowTreasureSpawns) {
        return vec![];
    }
    w.tagged(Tag::TreasureChest)
        .iter()
        .filter_map(|&id| match w.kind(id) {
            Some(Kind::Chest(ch)) if ch.state == crate::treasure::ChestState::WaitingToAppear => {
                Some((w.core(id).pos, ch.ty.large()))
            }
            _ => None,
        })
        .collect()
}
