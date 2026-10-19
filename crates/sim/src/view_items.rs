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
    /// The normal arrow's fletching layer (tinted owner colour B).
    pub arrow_feather: Option<TexId>,
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
    /// pickups.md §2.2b: the glow under every item, the orb border, the
    /// animated trigger / prism arrow items.
    pub pickup_glow: Option<TexId>,
    pub orb_border: Option<TexId>,
    pub trigger_pickup: SpriteId,
    pub prism_pickup: SpriteId,
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
    art.arrow_feather = Some(c.tex(main, "arrows/arrowFeather"));
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
    art.trigger_pickup = sprite(c, "TriggerArrowsPickup");
    art.prism_pickup = sprite(c, "PrismArrowsPickup");
    art.pickup_glow = Some(c.tex(main, "pickups/glow"));
    art.orb_border = Some(c.tex(main, "pickups/orbBorder"));
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
        outline: 0,
        clip_h: 1.0,
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
            it.outline = if art.brambles.is_some() { 255 } else { 0 };
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
            pickup_items(out, w, c, id, p);
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
                    // pickups.md §6.3: outlined until opened.
                    it.outline = if ch.state != ChestState::Opened {
                        255
                    } else {
                        0
                    };
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

/// One item (pickups.md §2.2b): glow, outline, graphic(s).
fn pickup_items(
    out: &mut Vec<Drawable>,
    w: &World,
    c: &Content,
    id: EntityId,
    p: &crate::pickups::Pickup,
) {
    use crate::components::Ease;
    let core = w.core(id);
    let (pos, d) = (core.pos, core.depth);
    let art = &c.common.items;
    let at = pos + p.draw_offset();
    // The unpack tween's eased value (glow alpha, item scale).
    let g = Ease::BackOut.apply((p.t / crate::pickups::UNPACK).min(1.0));
    // Frames since the item appeared (the bob counter wraps every 480).
    let age = p.bob / (crate::math::TAU / 120.0);
    let (sn, sn2) = (crate::math::sin(p.bob), crate::math::sin(p.bob / 2.0));
    let breathe = Vec2::new(1.0 + 0.05 * sn2, 1.0 + 0.05 * sn);
    // 1. Glow.
    if let Some(t) = art.pickup_glow {
        let bomb = p.kind == Pickups::Bomb;
        let (v, o) = tex(c, t, -1, 0, 0);
        let mut it = item(id, d, v, if bomb { pos } else { at }, o);
        let a = if bomb { 0.5 } else { 0.3 } * g.clamp(0.0, 1.0);
        it.tint = if bomb {
            [255, 0, 0, (a * 255.0) as u8]
        } else {
            [0xFF, 0xEB, 0x2E, (a * 255.0) as u8]
        };
        out.push(it);
    }
    let strip = |k: usize, frame: i16| {
        art.pickups[k].map(|t| {
            let (v, o) = tex(c, t, frame, 12, 12);
            item(id, d, v, at, o)
        })
    };
    let whole = |k: usize| {
        art.pickups[k].map(|t| {
            let (v, o) = tex(c, t, -1, 0, 0);
            item(id, d, v, at, o)
        })
    };
    let orb = match p.kind {
        Pickups::DarkOrb => Some(0usize),
        Pickups::TimeOrb => Some(1),
        Pickups::LavaOrb => Some(2),
        Pickups::SpaceOrb => Some(3),
        Pickups::ChaosOrb => Some(4),
        _ => None,
    };
    let k = p.kind as usize;
    let s = p.scale();
    match p.kind {
        Pickups::Shield => {
            // The archer's bubble in the joined players' colours, the icon
            // on top (only the icon is outlined).
            if let Some(sd) = c.sprite(c.common.shield) {
                let mut b = sprite_item(c, c.common.shield, id, d, at, age).expect("shield sprite");
                b.pos = at;
                b.origin = Vec2::new(sd.frame_w as f32 / 2.0, sd.frame_h as f32 / 2.0);
                b.scale = if core.collidable {
                    Vec2::new(0.6 + 0.1 * sn2, 0.6 + 0.1 * sn)
                } else {
                    Vec2::new(0.6 * g, 0.6 * g)
                };
                b.tint = shield_pickup_color(w, c, age);
                out.push(b);
            }
            if let Some(mut it) = whole(k) {
                it.scale = if core.collidable {
                    breathe
                } else {
                    Vec2::new(g, g)
                };
                it.outline = 255;
                out.push(it);
            }
            return;
        }
        _ if orb.is_some() => {
            let o = orb.unwrap_or(0);
            const BORDER: [u32; 4] = [0xF8B8F8, 0xFFFFFF, 0xF8B800, 0xFFFFFF];
            let sc = if core.collidable {
                breathe
            } else {
                Vec2::new(g, g)
            };
            let shown = (w.frame / 5).is_multiple_of(2);
            if shown && let Some(t) = art.orb_border {
                let (v, org) = tex(c, t, -1, 0, 0);
                let mut it = item(id, d, v, at, org);
                let ci = if o == 4 {
                    (w.frame / 6) as usize % 4
                } else {
                    o
                };
                let col = BORDER[ci];
                it.tint = [(col >> 16) as u8, (col >> 8) as u8, col as u8, 255];
                it.scale = sc;
                it.outline = 255;
                out.push(it);
            }
            let sid = art.orb_sprites[[1, 0, 2, 3, 4][o]];
            if let Some(mut it) = sprite_item(c, sid, id, d, at, age) {
                it.scale = sc;
                it.outline = if shown { 0 } else { 255 };
                out.push(it);
            }
            return;
        }
        _ => {}
    }
    let mut it = match p.kind {
        Pickups::BombArrows | Pickups::SuperBombArrows | Pickups::LaserArrows => {
            strip(k, ((age / 18.0) as i16) % 2)
        }
        Pickups::BoltArrows => strip(k, ((age / 3.0) as i16) % 3),
        Pickups::TriggerArrows => sprite_item(c, art.trigger_pickup, id, d, at, age),
        Pickups::PrismArrows => sprite_item(c, art.prism_pickup, id, d, at, age),
        Pickups::Mirror => sprite_item(c, art.mirror, id, d, at, age),
        Pickups::Bomb => {
            // Frames 0,0,2,1,1,2 every 3 ticks; the swell speeds it x1 -> x4
            // (cubic in over 40 frames: +30 s^4 ticks of animation).
            let sw = (p.swell / crate::pickups::BOMB_SWELL).min(1.0);
            let t = age + 30.0 * sw * sw * sw * sw;
            strip(k, [0, 0, 2, 1, 1, 2][(t / 3.0) as usize % 6])
        }
        _ => whole(k),
    }
    .unwrap_or_else(|| {
        let col = match p.kind.arrow_type() {
            Some(t) => arrow_tint(t),
            None => [230, 230, 255, 255],
        };
        rect(id, d, at, 10.0, 10.0, col)
    });
    it.scale = Vec2::new(s, s);
    // Breathing replaces the tween scale once collidable (10 frames in).
    if core.collidable && matches!(p.kind, Pickups::Wings | Pickups::SpeedBoots) {
        it.scale = breathe;
    }
    if p.kind == Pickups::Bomb {
        it.rotation = (p.swell / crate::pickups::BOMB_SWELL).min(1.0) * 2.0 * PI;
    }
    it.outline = if matches!(it.visual, Visual::Sprite { .. }) {
        255
    } else {
        0
    };
    out.push(it);
}

/// The shield item's bubble colour: every 12 frames the next joined slot
/// among the first four (blue / red team colour B in team play), only with
/// more than one player (pickups.md §2.2b).
fn shield_pickup_color(w: &World, c: &Content, age: f32) -> [u8; 4] {
    let team = w.versus.as_ref().is_some_and(|m| m.settings.team_mode());
    let joined: Vec<usize> = (0..4)
        .filter(|&i| crate::view::slot_archer(w, i).is_some())
        .collect();
    let all = (0..crate::world::MAX_PLAYERS)
        .filter(|&i| crate::view::slot_archer(w, i).is_some())
        .count();
    let step = if all > 1 { (age / 12.0) as usize } else { 0 };
    if team {
        let t = if step % 2 == 0 {
            crate::entity::Allegiance::Blue
        } else {
            crate::entity::Allegiance::Red
        };
        return crate::view::owner_color(w, c, 0, t, true);
    }
    match joined.get(step % joined.len().max(1)) {
        Some(&i) => {
            crate::view::owner_color(w, c, i as i8, crate::entity::Allegiance::Neutral, true)
        }
        None => [255; 4],
    }
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
                it.outline = if c.common.items.brambles.is_some() {
                    255
                } else {
                    0
                };
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
