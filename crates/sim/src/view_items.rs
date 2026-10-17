//! Drawables and HUD data for the M5 entities (arrow types, explosions,
//! shocks, brambles, prisms, pick-ups, chests, lava) and the quiver HUD
//! (arrows.md §14). Art is optional: without it every item is a coloured
//! placeholder rectangle.

use crate::components::AnimDef;
use crate::content::{Content, SpriteDef, SpriteId, TexId};
use crate::entity::{EntityId, Kind};
use crate::math::{PI, Vec2, floor};
use crate::pickups::Pickups;
use crate::player::{PState, Player};
use crate::quiver::ArrowType;
use crate::view::{Drawable, Visual};
use crate::world::World;
use tfp_assets::atlas::AtlasKind;

/// Texture and sprite ids the M5 drawables use (`Content::common.items`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemArt {
    pub arrows: [Option<TexId>; 11],
    pub arrows_buried: [Option<TexId>; 11],
    pub pickups: [Option<TexId>; 21],
    pub orb_sprites: [SpriteId; 5],
    pub chest: Option<TexId>,
    pub big_chest: Option<TexId>,
    pub bottomless_chest: Option<TexId>,
    pub brambles: Option<TexId>,
    pub explosion_core: SpriteId,
    pub explosion_kill: SpriteId,
    pub explosion_push: SpriteId,
    pub shock: SpriteId,
    pub prism: SpriteId,
    pub mirror: SpriteId,
}

const ARROW_TEX: [&str; 11] = [
    "arrows/arrow",
    "arrows/bombArrow",
    "arrows/superBombArrow",
    "arrows/laserArrow",
    "arrows/brambleArrow",
    "arrows/drillArrow",
    "arrows/boltArrow",
    "arrows/squeakyArrow",
    "arrows/featherArrow",
    "arrows/triggerArrow",
    "arrows/prismArrow",
];
const ARROW_BURIED_TEX: [&str; 11] = [
    "arrows/arrowBuried",
    "arrows/bombArrowBuried",
    "arrows/superBombArrowBuried",
    "arrows/laserArrowBuried",
    "arrows/brambleArrowBuried",
    "arrows/drillArrowBuried",
    "arrows/boltArrowBuried",
    "arrows/squeakyArrowBurried",
    "arrows/featherArrowBuried",
    "arrows/triggerArrowBuried",
    "arrows/prismArrowBuried",
];
const PICKUP_TEX: [&str; 21] = [
    "pickups/arrowPickup",
    "pickups/bombArrows",
    "pickups/superBombArrows",
    "pickups/laserArrows",
    "pickups/brambleArrows",
    "pickups/drillArrows",
    "pickups/boltArrows",
    "pickups/featherArrows",
    "pickups/triggerArrows",
    "pickups/prismArrows",
    "pickups/shieldIcon",
    "pickups/wings",
    "pickups/speedBoots",
    "pickups/mirror",
    "pickups/timeOrb",
    "pickups/darkOrb",
    "pickups/fireOrb",
    "pickups/spaceOrb",
    "pickups/chaosOrb",
    "pickups/bomb",
    "pickups/yellowGem",
];

/// Register the M5 art (call once after [`Content::load_archers`]).
pub fn load_art(c: &mut Content, sprites: &tfp_assets::sprite_data::SpriteData) {
    let main = AtlasKind::Main;
    let mut art = ItemArt::default();
    for i in 0..11 {
        art.arrows[i] = Some(c.tex(main, ARROW_TEX[i]));
        art.arrows_buried[i] = Some(c.tex(main, ARROW_BURIED_TEX[i]));
    }
    for (i, n) in PICKUP_TEX.iter().enumerate() {
        art.pickups[i] = Some(c.tex(main, n));
    }
    let sprite = |c: &mut Content, id: &str| {
        sprites
            .get(id)
            .map(|d| c.add_sprite(main, d))
            .unwrap_or(SpriteId::NONE)
    };
    for (k, n) in ["TimeOrb", "DarkOrb", "FireOrb", "SpaceOrb", "ChaosOrb"]
        .iter()
        .enumerate()
    {
        art.orb_sprites[k] = sprite(c, n);
    }
    art.explosion_core = sprite(c, "ExplosionCore");
    art.explosion_kill = sprite(c, "ExplosionKill");
    art.explosion_push = sprite(c, "ExplosionPush");
    art.shock = sprite(c, "Shock");
    art.prism = sprite(c, "Prism");
    art.mirror = sprite(c, "Mirror");
    art.chest = Some(c.tex(main, "treasureChest"));
    art.big_chest = Some(c.tex(main, "bigChest"));
    art.bottomless_chest = Some(c.tex(main, "bottomlessChest"));
    art.brambles = Some(c.tex(main, "brambles"));
    c.common.items = art;
}

fn item(entity: EntityId, depth: i32, visual: Visual, pos: Vec2, origin: Vec2) -> Drawable {
    Drawable {
        visual,
        pos,
        origin,
        scale: Vec2::ONE,
        rotation: 0.0,
        flip_x: false,
        flip_y: false,
        tint: [255; 4],
        depth,
        entity,
    }
}

fn rect(entity: EntityId, depth: i32, pos: Vec2, w: f32, h: f32, tint: [u8; 4]) -> Drawable {
    let mut d = item(
        entity,
        depth,
        Visual::Rect { w, h },
        pos,
        Vec2::new(w / 2.0, h / 2.0),
    );
    d.tint = tint;
    d
}

fn tex(c: &Content, t: TexId, frame: i16, fw: u16, fh: u16) -> (Visual, Vec2) {
    let (w, h) = c
        .textures
        .get(t.0 as usize)
        .and_then(|r| r.size)
        .map(|(w, h)| (w as f32, h as f32))
        .unwrap_or((fw as f32, fh as f32));
    let (cw, ch) = if fw > 0 {
        (fw as f32, fh as f32)
    } else {
        (w, h)
    };
    (
        Visual::Sprite {
            tex: t,
            frame,
            frame_w: fw,
            frame_h: fh,
        },
        Vec2::new(cw / 2.0, ch / 2.0),
    )
}

fn sprite_frame(s: &SpriteDef, age_frames: f32) -> i16 {
    let anim: Option<&AnimDef> = s.anims.first();
    match anim {
        Some(a) if !a.frames.is_empty() => {
            let step = (a.delay * 60.0).max(1.0);
            let k = (age_frames / step) as usize;
            let k = if a.looped {
                k % a.frames.len()
            } else {
                k.min(a.frames.len() - 1)
            };
            a.frames[k] as i16
        }
        _ => 0,
    }
}

fn sprite_item(
    c: &Content,
    s: SpriteId,
    id: EntityId,
    depth: i32,
    pos: Vec2,
    age: f32,
) -> Option<Drawable> {
    let sd = c.sprite(s)?;
    Some(item(
        id,
        depth,
        Visual::Sprite {
            tex: sd.texture,
            frame: sprite_frame(sd, age),
            frame_w: sd.frame_w,
            frame_h: sd.frame_h,
        },
        pos + sd.position,
        sd.origin,
    ))
}

fn rgb(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

/// The texture an arrow of this type and state draws with.
pub fn arrow_texture(c: &Content, a: &crate::arrow::Arrow) -> Option<TexId> {
    let i = a.arrow_type as usize;
    if a.state == crate::arrow::ArrowState::Buried {
        c.common.items.arrows_buried[i]
    } else {
        c.common.items.arrows[i]
    }
}

/// Placeholder tint of an arrow type.
pub fn arrow_tint(t: ArrowType) -> [u8; 4] {
    rgb(crate::arrows::COLOR_A[t as usize])
}

/// Drawables of one M5 entity (called from `View::build`). Returns false
/// for kinds this module does not draw.
pub fn entity_items(out: &mut Vec<Drawable>, w: &World, c: &Content, id: EntityId) -> bool {
    let core = w.core(id);
    let pos = core.pos;
    let d = core.depth;
    let art = &c.common.items;
    match w.kind(id) {
        Some(Kind::Shock(s)) => match sprite_item(c, art.shock, id, d, pos, s.age) {
            Some(it) => out.push(it),
            None => out.push(rect(id, d, pos, 56.0, 56.0, [0, 255, 76, 90])),
        },
        Some(Kind::Bramble(b)) => {
            let sc = b.scale();
            let mut it = match art.brambles {
                Some(t) => {
                    let (v, o) = tex(c, t, -1, 0, 0);
                    item(id, d, v, pos, o)
                }
                None => rect(id, d, pos, 8.0, 8.0, [120, 60, 40, 255]),
            };
            it.scale = Vec2::new(sc, sc);
            it.rotation = (1.0 - sc) * 40.0 * PI / 180.0;
            if b.burning {
                it.tint = [255, 140, 60, 255];
            }
            out.push(it);
        }
        Some(Kind::Prism(pr)) => {
            let s = crate::arrows::prism::scale(pr);
            let mut at = pos;
            if pr.shaking {
                at.x += ((w.frame % 3) as f32) - 1.0;
            }
            let mut it = sprite_item(c, art.prism, id, d, at, pr.age)
                .unwrap_or_else(|| rect(id, d, at, 40.0, 40.0, [219, 74, 219, 110]));
            it.scale = s;
            out.push(it);
        }
        Some(Kind::Pickup(p)) => {
            if !core.flash.shown {
                return true;
            }
            let at = pos + p.draw_offset();
            let k = p.kind as usize;
            let orb = match p.kind {
                Pickups::TimeOrb => Some(0),
                Pickups::DarkOrb => Some(1),
                Pickups::LavaOrb => Some(2),
                Pickups::SpaceOrb => Some(3),
                Pickups::ChaosOrb => Some(4),
                _ => None,
            };
            let mut it = if let Some(o) =
                orb.and_then(|o| sprite_item(c, art.orb_sprites[o], id, d, at, p.bob * 20.0))
            {
                o
            } else if p.kind == Pickups::Mirror
                && let Some(m) = sprite_item(c, art.mirror, id, d, at, p.bob * 20.0)
            {
                m
            } else if let Some(t) = art.pickups[k] {
                let grid = match p.kind {
                    Pickups::Bomb => 12,
                    k if k.is_arrows() && k != Pickups::Arrows => 12,
                    Pickups::TimeOrb | Pickups::DarkOrb | Pickups::LavaOrb => 16,
                    _ => 0,
                };
                let (v, o) = tex(c, t, if grid > 0 { 0 } else { -1 }, grid, grid);
                item(id, d, v, at, o)
            } else {
                let col = match p.kind.arrow_type() {
                    Some(t) => arrow_tint(t),
                    None => [230, 230, 255, 255],
                };
                rect(id, d, at, 10.0, 10.0, col)
            };
            let s = p.scale();
            it.scale = Vec2::new(s, s);
            if p.kind == Pickups::Bomb {
                it.rotation = (p.swell / crate::pickups::BOMB_SWELL).min(1.0) * 2.0 * PI;
            }
            out.push(it);
        }
        Some(Kind::Chest(ch)) => {
            use crate::treasure::{ChestState, ChestType};
            if !core.flash.shown {
                return true;
            }
            let (t, cell) = match ch.ty {
                ChestType::Large => (art.big_chest, 20u16),
                ChestType::Bottomless => (art.bottomless_chest, 20),
                _ => (art.chest, 10),
            };
            let frame = match ch.state {
                ChestState::Opening => ((ch.open_t / 4.0) as i16).clamp(1, 3),
                ChestState::Opened => 3,
                _ => 0,
            };
            let mut at = pos;
            if ch.shake > 0.0 {
                at.x += ((w.frame % 3) as f32) - 1.0;
            }
            let mut it = match t {
                Some(t) => {
                    let (v, _) = tex(c, t, frame, cell, cell);
                    let h = cell as f32;
                    let mut it = item(id, d, v, at, Vec2::new(h / 2.0, h));
                    it.pos.y = core.bottom();
                    it
                }
                None => {
                    let hb = core.hitbox();
                    let col = if ch.state >= ChestState::Opening {
                        [120, 90, 40, 255]
                    } else {
                        [200, 160, 60, 255]
                    };
                    let mut it = rect(id, d, at, hb.w, hb.h, col);
                    it.pos = Vec2::new(at.x, core.bottom());
                    it.origin = Vec2::new(hb.w / 2.0, hb.h);
                    it
                }
            };
            it.scale = ch.scale;
            out.push(it);
        }
        Some(Kind::PrismBar(_)) => {}
        _ => return false,
    }
    true
}

/// Extras drawn on top of entities the base view draws (thorns on a
/// brambled corpse, the prism vanish shrink). `start` = first drawable of
/// this entity.
pub fn decorate(out: &mut Vec<Drawable>, start: usize, w: &World, c: &Content, id: EntityId) {
    if let Some(Kind::Corpse(cp)) = w.kind(id) {
        let vs = cp.vanish_scale();
        if vs != 1.0 {
            for d in &mut out[start..] {
                d.scale *= vs;
            }
        }
        let bs = cp.bramble_scale();
        if bs > 0.0 {
            let pos = w.core(id).pos;
            for (k, dx) in [-2.0f32, 2.0].into_iter().enumerate() {
                let mut it = match c.common.items.brambles {
                    Some(t) => {
                        let (v, o) = tex(c, t, -1, 0, 0);
                        item(id, w.core(id).depth - 1, v, pos + Vec2::new(dx, 4.0), o)
                    }
                    None => rect(
                        id,
                        w.core(id).depth - 1,
                        pos + Vec2::new(dx, 4.0),
                        8.0,
                        8.0,
                        [120, 60, 40, 255],
                    ),
                };
                it.scale = Vec2::new(bs, bs);
                it.rotation = k as f32 * PI / 3.0;
                out.push(it);
            }
        }
    }
}

/// The quiver above an archer's head (arrows.md §14).
#[derive(Clone, Debug, PartialEq)]
pub struct QuiverHud {
    pub player: u8,
    /// `Floor(Position)`; icons at `x = -2*count + 4*i`, `y = -22`.
    pub pos: Vec2,
    /// Arrow types in firing order (icon index = type index).
    pub arrows: Vec<ArrowType>,
    /// Armed trigger arrows (drawn first, at `y = -20`).
    pub triggers: u8,
    /// Trigger colour phase (FF2E16 / EBFF5B, flips every 6 frames).
    pub trigger_flash: bool,
    /// Draw the quiver at all this frame.
    pub shown: bool,
    /// Empty quiver: blink the "no arrows" icon (`NONE_COLORS[phase]`).
    pub empty_blink: Option<u8>,
}

/// Quiver HUD of every living archer.
pub fn quiver_huds(w: &World) -> Vec<QuiverHud> {
    let mut out = vec![];
    for id in w.players() {
        let Some(p) = w.player(id) else { continue };
        if p.dead {
            continue;
        }
        out.push(quiver_hud(w, id, p));
    }
    out
}

fn quiver_hud(w: &World, id: EntityId, p: &Player) -> QuiverHud {
    let core = w.core(id);
    let secret = w
        .setup
        .variant_set
        .get(crate::variants::Variant::SecretArrows, p.index as i32);
    let hidden = secret
        || p.state() == PState::Ducking
        || (p.state() == PState::Dodging && p.dodge_sliding)
        || p.invisible;
    let counter = p.hud_counter > 0.0;
    let shown = counter || p.aiming || !hidden;
    let blink_phase = ((w.frame / 6) % 2) as u8;
    QuiverHud {
        player: p.index,
        pos: Vec2::new(floor(core.pos.x), floor(core.pos.y)),
        arrows: p.arrows.as_slice().to_vec(),
        triggers: p.trigger_count() as u8,
        trigger_flash: blink_phase == 1,
        shown,
        empty_blink: (p.arrows.is_empty() && counter).then_some(blink_phase),
    }
}
