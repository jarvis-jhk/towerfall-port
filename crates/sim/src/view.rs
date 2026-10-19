//! What to draw this frame, as a flat list (no sim internals needed).
//!
//! Each [`Drawable`] is an atlas subtexture (optionally one cell of the
//! sprite's frame grid) or a plain rectangle, placed XNA-style: the image
//! point `origin` lands on `pos`, then it is scaled (negative = mirrored
//! around the origin) and rotated (radians, clockwise on screen). Items are
//! in draw order (back to front: layer order, then depth list order), and
//! entities that wrap the screen are repeated at their wrapped copies like
//! `LevelEntity.Render` (engine.md §9.4). The level's tiles are not part of
//! the list; renderers draw them from the level data.

use crate::content::{Content, SpriteDef, TexId};
use crate::entity::{Allegiance, EntityId, Kind};
use crate::math::{PI, Vec2};
use crate::player::{HatState, PState, Player};
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Visual {
    /// `frame` indexes the sprite's frame grid (`frame_w x frame_h` cells,
    /// row-major inside the subtexture); `frame_w == 0` = the whole texture.
    Sprite {
        tex: TexId,
        frame: i16,
        frame_w: u16,
        frame_h: u16,
    },
    /// A filled rectangle `w x h` (placeholders).
    Rect { w: f32, h: f32 },
    /// A 1-px rectangle outline `w x h` (debug hitboxes).
    Outline { w: f32, h: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawable {
    pub visual: Visual,
    pub pos: Vec2,
    pub origin: Vec2,
    pub scale: Vec2,
    pub rotation: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    /// RGBA (straight alpha).
    pub tint: [u8; 4],
    pub depth: i32,
    /// The entity it belongs to (for debugging/picking).
    pub entity: EntityId,
    /// Alpha of its 1-px black 8-neighbour outline (`DrawOutline`,
    /// rendering.md 10.4), 0 = none; [`expand_outlines`] makes the copies.
    pub outline: u8,
    /// Fraction of the frame's rows drawn from the top (`SpritePart`
    /// `DrawHeight`; the shield gain), 1 = all.
    pub clip_h: f32,
}

/// Draw options.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewOptions {
    /// Add translucent rectangles for every hitbox.
    pub hitboxes: bool,
}

/// The frame's drawables plus the world size they live in.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Drawable>,
    /// Versus match state for HUD and results screens (`None` outside a match).
    pub hud: Option<crate::versus::HudView>,
    /// Quiver above each living archer (arrows.md §14).
    pub quivers: Vec<crate::view_items::QuiverHud>,
    /// Dark orb: the lighting layer is fully dark (only lights show).
    pub dark: bool,
    /// Space orb / scroll variants: camera offset (the playfield wraps).
    pub camera_offset: Vec2,
    /// Lit cooldown stars (Show Dodge Cooldown, enemy recovery), positions.
    pub cooldowns: Vec<Vec2>,
    /// Show Treasure Spawns: chests yet to appear (position, large).
    pub treasure_previews: Vec<(Vec2, bool)>,
    /// Gunn Style freeze-frame overlay.
    pub gunn: Option<crate::variant_fx::GunnView>,
    /// Quest HUD data (`None` outside Quest).
    pub quest: Option<crate::quest::QuestView>,
    /// Trials HUD data (`None` outside Trials).
    pub trials: Option<crate::trials::Trials>,
    /// Dynamic lights of the sim's entities (position, radius); the level's
    /// static lights come from the tower look.
    pub lights: Vec<(Vec2, f32)>,
    /// Dark World HUD data (`None` outside Dark World).
    pub dark_world: Option<crate::darkworld::DwView>,
}

const WHITE: [u8; 4] = [255; 4];

fn sprite_visual(s: &SpriteDef, frame: i16) -> Visual {
    Visual::Sprite {
        tex: s.texture,
        frame,
        frame_w: s.frame_w,
        frame_h: s.frame_h,
    }
}

fn tex_visual(t: TexId) -> Visual {
    Visual::Sprite {
        tex: t,
        frame: -1,
        frame_w: 0,
        frame_h: 0,
    }
}

/// Team colours A / B (rendering.md §12.1; teams 2-5 versus.md §15.4).
fn team_colors(t: Allegiance) -> Option<([u8; 4], [u8; 4])> {
    let (a, b) = match t {
        Allegiance::Neutral => return None,
        Allegiance::Blue => (0x3CBCFC, 0xA4E4FC),
        Allegiance::Red => (0xAD4126, 0xFF6338),
        Allegiance::Green => (0x00B800, 0x45E045),
        Allegiance::Yellow => (0xF2FF00, 0xFFFF66),
        Allegiance::Purple => (0x7A42FF, 0xA77FFF),
        Allegiance::Orange => (0xEF8C21, 0xFFB15E),
    };
    let c = |v: u32| [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255];
    Some((c(a), c(b)))
}

/// Colour A or B (`light`) of whatever player slot `index` plays, as
/// `ArcherData.GetColorA/B(playerIndex, team)`: the team's colours in team
/// play, the enemy (red team) colours without a player (`index < 0`).
pub fn owner_color(w: &World, c: &Content, index: i8, team: Allegiance, light: bool) -> [u8; 4] {
    let pick = |(a, b): ([u8; 4], [u8; 4])| if light { b } else { a };
    if index < 0 {
        return pick(team_colors(Allegiance::Red).unwrap_or_default());
    }
    if let Some(tc) = team_colors(team) {
        return pick(tc);
    }
    slot_archer(w, index as usize)
        .and_then(|a| c.archers.get(a as usize))
        .map_or(WHITE, |a| if light { a.color_b } else { a.color_a })
}

/// The archer (`Content::archers` index) player slot `i` plays
/// (`TFGame.Characters`): the Versus slot, else the world setup, else the
/// slot's archer or corpse on the field.
pub fn slot_archer(w: &World, i: usize) -> Option<u16> {
    if let Some(m) = &w.versus {
        return m.settings.slots.get(i).copied().flatten().map(|s| s.archer);
    }
    if let Some(ps) = w.setup.players.get(i).copied().flatten() {
        return Some(ps.archer);
    }
    w.entities().find_map(|id| match w.kind(id) {
        Some(Kind::Player(p)) if p.index as usize == i => Some(p.archer),
        Some(Kind::Corpse(cp)) if cp.player_index as usize == i => Some(cp.archer),
        _ => None,
    })
}

/// The eight `(dx, dy)` neighbours of `DrawOutline`, in its order.
const OUTLINE: [(f32, f32); 8] = [
    (-1.0, -1.0),
    (-1.0, 0.0),
    (-1.0, 1.0),
    (0.0, -1.0),
    (0.0, 1.0),
    (1.0, -1.0),
    (1.0, 0.0),
    (1.0, 1.0),
];

/// Turns `outline` flags into black copies (rendering.md 10.4): each run
/// of one entity's drawables is preceded by the eight one-pixel-offset
/// black copies of its flagged members, so all of an entity's outlines go
/// under all of its layers (an archer's bow outline does not cross its
/// body). Run once on the final list, after the game's cosmetics.
pub fn expand_outlines(items: &mut Vec<Drawable>) {
    if items.iter().all(|d| d.outline == 0) {
        return;
    }
    let src = std::mem::take(items);
    items.reserve(src.len() * 2);
    let mut i = 0;
    while i < src.len() {
        let e = src[i].entity;
        let mut j = i + 1;
        while j < src.len() && src[j].entity == e {
            j += 1;
        }
        let run = &src[i..j];
        for (dx, dy) in OUTLINE {
            for d in run.iter().filter(|d| d.outline > 0) {
                let mut o = *d;
                o.pos += Vec2::new(dx, dy);
                o.tint = [0, 0, 0, d.outline];
                o.outline = 0;
                items.push(o);
            }
        }
        items.extend(run.iter().map(|d| Drawable { outline: 0, ..*d }));
        i = j;
    }
}

impl View {
    pub fn build(w: &World, c: &Content, opt: ViewOptions) -> View {
        let ws = w.world_size(c);
        let mut v = View {
            width: ws.w,
            height: ws.h,
            items: Vec::with_capacity(128),
            hud: w.versus.as_ref().map(|m| m.hud(w)),
            quivers: crate::view_items::quiver_huds(w),
            dark: w.items.orbs.dark,
            camera_offset: w.items.orbs.camera,
            cooldowns: crate::variant_fx::cooldown_stars(w),
            treasure_previews: crate::variant_fx::treasure_previews(w),
            gunn: crate::variant_fx::gunn_view(w),
            quest: w.quest.as_ref().map(|q| q.view()),
            trials: w.trials.as_deref().copied(),
            lights: lights(w),
            dark_world: w.dark_world.as_ref().map(|d| d.view(c)),
        };
        for id in w.entities() {
            let start = v.items.len();
            let core = w.core(id);
            if !core.visible {
                continue;
            }
            match w.kind(id) {
                Some(Kind::Player(p)) => player_items(&mut v.items, w, c, id, p),
                Some(Kind::Arrow(a)) => {
                    let tex = crate::view_items::arrow_texture(c, a).or(
                        if a.state == crate::arrow::ArrowState::Buried {
                            c.common.arrow_buried
                        } else {
                            c.common.arrow
                        },
                    );
                    if !core.flash.shown {
                        // decaying arrows blink
                    } else if let Some(t) = tex {
                        // arrows.md §14c: owner-coloured layers, all outlined.
                        use crate::quiver::ArrowType as A;
                        let b = owner_color(w, c, a.player_index, a.team_color, true);
                        let mut d = item(
                            id,
                            core.depth,
                            tex_visual(t),
                            core.pos,
                            Vec2::new(11.0, 3.0),
                            a.direction,
                        );
                        d.outline = 255;
                        if matches!(a.arrow_type, A::Feather | A::Bolt | A::Laser) {
                            d.tint = b;
                        }
                        v.items.push(d);
                        if a.arrow_type == A::Normal
                            && let Some(f) = c.common.items.arrow_feather
                        {
                            d.visual = tex_visual(f);
                            d.tint = b;
                            v.items.push(d);
                        }
                    } else {
                        let mut d = item(
                            id,
                            core.depth,
                            Visual::Rect { w: 10.0, h: 2.0 },
                            core.pos,
                            Vec2::new(9.0, 1.0),
                            a.direction,
                        );
                        d.tint = crate::view_items::arrow_tint(a.arrow_type);
                        v.items.push(d);
                    }
                }
                Some(Kind::Corpse(cp)) => {
                    let a = c.archers.get(cp.archer as usize);
                    let mut alpha = (cp.draw_opacity.clamp(0.0, 1.0) * 255.0) as u8;
                    if alpha == 0 {
                        alpha = 1;
                    }
                    if let Some(s) = a.and_then(|a| c.sprite(a.corpse)) {
                        let mut d = item(
                            id,
                            core.depth,
                            sprite_visual(s, cp.sprite.frame),
                            core.pos + s.position,
                            s.origin,
                            0.0,
                        );
                        d.scale = Vec2::new(cp.scale.x * cp.facing as f32, cp.scale.y);
                        if cp.squished.y < 0.0 {
                            d.flip_y = true;
                        }
                        d.tint[3] = alpha;
                        v.items.push(d);
                    } else {
                        let mut d = item(
                            id,
                            core.depth,
                            Visual::Rect { w: 10.0, h: 5.0 },
                            core.pos,
                            Vec2::new(5.0, -3.0),
                            0.0,
                        );
                        d.tint = a.map(|a| dim(a.color_a)).unwrap_or(WHITE);
                        v.items.push(d);
                    }
                }
                Some(Kind::Hat(h)) => {
                    let tex = if h.hat == HatState::Crown {
                        c.common.crown
                    } else {
                        c.archers.get(h.archer as usize).and_then(|a| a.hat_texture)
                    };
                    if let Some(t) = tex {
                        let origin = c
                            .texture(t)
                            .size
                            .map(|(tw, th)| Vec2::new(tw as f32 / 2.0, th as f32 / 2.0))
                            .unwrap_or(Vec2::new(4.0, 3.0));
                        let mut d =
                            item(id, core.depth, tex_visual(t), core.pos, origin, h.rotation);
                        d.flip_x = h.flipped;
                        v.items.push(d);
                    } else {
                        let mut d = item(
                            id,
                            core.depth,
                            Visual::Rect { w: 6.0, h: 4.0 },
                            core.pos,
                            Vec2::new(3.0, 3.0),
                            h.rotation,
                        );
                        d.tint = [200, 160, 40, 255];
                        v.items.push(d);
                    }
                }
                Some(Kind::Tower(t)) => crate::entities::draw::items(&mut v.items, w, c, id, t),
                Some(Kind::Enemy(e)) => {
                    let hb = core.hitbox();
                    let mut d = item(
                        id,
                        core.depth,
                        Visual::Rect { w: hb.w, h: hb.h },
                        core.pos + Vec2::new(hb.x, hb.y),
                        Vec2::ZERO,
                        0.0,
                    );
                    d.tint = crate::enemies::debug_color(e);
                    v.items.push(d);
                }
                Some(Kind::Dw(part)) => {
                    let hb = core.hitbox();
                    if hb.w > 0.0 {
                        let mut d = item(
                            id,
                            core.depth,
                            Visual::Rect { w: hb.w, h: hb.h },
                            core.pos + Vec2::new(hb.x, hb.y),
                            Vec2::ZERO,
                            0.0,
                        );
                        d.tint = crate::darkworld::part::debug_color(part);
                        v.items.push(d);
                    }
                }
                Some(Kind::Hazard(_)) | Some(Kind::Dummy(_)) => {
                    let hb = core.hitbox();
                    if hb.w > 0.0 {
                        let mut d = item(
                            id,
                            core.depth,
                            Visual::Rect { w: hb.w, h: hb.h },
                            core.pos + Vec2::new(hb.x, hb.y),
                            Vec2::ZERO,
                            0.0,
                        );
                        d.tint = if matches!(w.kind(id), Some(Kind::Dummy(_))) {
                            [200, 170, 120, 255]
                        } else {
                            [255, 120, 40, 220]
                        };
                        v.items.push(d);
                    }
                }
                Some(Kind::Portal(pt)) => {
                    if pt.scale > 0.0 {
                        let s = 20.0 * pt.scale;
                        let mut d = item(
                            id,
                            core.depth,
                            Visual::Outline { w: s, h: s },
                            core.pos - Vec2::new(s / 2.0, s / 2.0),
                            Vec2::ZERO,
                            0.0,
                        );
                        d.tint = [255, 46, 252, 255];
                        v.items.push(d);
                    }
                }
                Some(Kind::Miasma(_)) => {
                    let hb = core.hitbox();
                    if hb.w > 0.0 {
                        let mut d = item(
                            id,
                            core.depth,
                            Visual::Rect { w: hb.w, h: hb.h },
                            core.pos + Vec2::new(hb.x, hb.y),
                            Vec2::ZERO,
                            0.0,
                        );
                        d.tint = if core.collidable {
                            [150, 40, 200, 170]
                        } else {
                            [150, 40, 200, 90]
                        };
                        v.items.push(d);
                    }
                }
                Some(Kind::JumpThru) | Some(Kind::MovingSolid(_)) | Some(Kind::StaticSolid) => {
                    let hb = core.hitbox();
                    let mut d = item(
                        id,
                        core.depth,
                        Visual::Rect { w: hb.w, h: hb.h },
                        core.pos + Vec2::new(hb.x, hb.y),
                        Vec2::ZERO,
                        0.0,
                    );
                    d.tint = [150, 120, 90, 255];
                    v.items.push(d);
                }
                _ => {
                    crate::view_items::entity_items(&mut v.items, w, c, id);
                }
            }
            crate::view_items::decorate(&mut v.items, start, w, c, id);
            if opt.hitboxes {
                let core = w.core(id);
                if let crate::collide::Collider::Box(hb) = core.collider {
                    let mut d = item(
                        id,
                        core.depth,
                        Visual::Outline { w: hb.w, h: hb.h },
                        core.pos + Vec2::new(hb.x, hb.y),
                        Vec2::ZERO,
                        0.0,
                    );
                    d.tint = if core.collidable {
                        [255, 60, 60, 200]
                    } else {
                        [160, 160, 160, 200]
                    };
                    v.items.push(d);
                }
            }
            if core.screen_wrap {
                let end = v.items.len();
                let dx = if core.pos.x > ws.w / 2.0 { -ws.w } else { ws.w };
                let dy = if core.pos.y > ws.h / 2.0 { -ws.h } else { ws.h };
                for off in [Vec2::new(dx, 0.0), Vec2::new(0.0, dy), Vec2::new(dx, dy)] {
                    for k in start..end {
                        let mut d = v.items[k];
                        d.pos += off;
                        v.items.push(d);
                    }
                }
            }
        }
        v
    }
}

fn dim(c: [u8; 4]) -> [u8; 4] {
    [c[0] / 2, c[1] / 2, c[2] / 2, 255]
}

fn item(
    entity: EntityId,
    depth: i32,
    visual: Visual,
    pos: Vec2,
    origin: Vec2,
    rotation: f32,
) -> Drawable {
    Drawable {
        visual,
        pos,
        origin,
        scale: Vec2::ONE,
        rotation,
        flip_x: false,
        flip_y: false,
        tint: WHITE,
        depth,
        entity,
        outline: 0,
        clip_h: 1.0,
    }
}

/// The archer's layers (player.md §19.1): aimer, wings, body, head, bow,
/// shield.
fn player_items(out: &mut Vec<Drawable>, w: &World, c: &Content, id: EntityId, p: &Player) {
    let core = w.core(id);
    if !core.flash.shown {
        return;
    }
    let alpha = (p.invis_opacity.clamp(0.0, 1.0) * 255.0) as u8;
    let tint = [255, 255, 255, alpha.max(1)];
    let pos = core.pos;
    let f = p.facing as f32;
    let Some(a) = c.archers.get(p.archer as usize) else {
        return;
    };
    let Some(body) = c.sprite(a.body) else {
        // Placeholder: a box the size of the hitbox, a head dot, the aim.
        let hb = core.hitbox();
        let mut d = item(
            id,
            core.depth,
            Visual::Rect { w: hb.w, h: hb.h },
            pos + Vec2::new(hb.x, hb.y),
            Vec2::ZERO,
            0.0,
        );
        d.tint = a.color_a;
        d.tint[3] = alpha.max(1);
        out.push(d);
        if p.hat != HatState::NoHat {
            let mut h = item(
                id,
                core.depth,
                Visual::Rect { w: 6.0, h: 2.0 },
                pos + Vec2::new(-3.0, hb.y - 2.0),
                Vec2::ZERO,
                0.0,
            );
            h.tint = if p.hat == HatState::Crown {
                [250, 210, 40, 255]
            } else {
                a.color_b
            };
            out.push(h);
        }
        if p.aiming {
            let mut d = item(
                id,
                core.depth,
                Visual::Rect { w: 10.0, h: 1.0 },
                pos + Vec2::new(0.0, -1.0),
                Vec2::new(-2.0, 0.5),
                p.aim_direction,
            );
            d.tint = [255, 255, 255, 200];
            out.push(d);
        }
        if p.has_shield {
            let mut d = item(
                id,
                core.depth,
                Visual::Rect { w: 16.0, h: 18.0 },
                pos + Vec2::new(-8.0, -10.0),
                Vec2::ZERO,
                0.0,
            );
            d.tint = [a.color_b[0], a.color_b[1], a.color_b[2], 70];
            out.push(d);
        }
        return;
    };
    let body_frame = p.body.frame.max(0) as usize;
    let first = out.len();
    // Aimer.
    if p.aiming
        && !p.arrows.is_empty()
        && let Some(t) = a.aimer
    {
        let mut d = item(
            id,
            core.depth,
            tex_visual(t),
            pos + Vec2::new(0.0, -1.0),
            Vec2::new(0.0, 5.0),
            p.aim_direction,
        );
        d.tint = tint;
        out.push(d);
    }
    // Wings.
    if p.has_wings
        && let Some(ws) = c.sprite(c.common.wings)
    {
        let mut d = item(
            id,
            core.depth,
            sprite_visual(ws, 0),
            pos + Vec2::new(0.0, 6.0 - 6.0 * p.scale.y),
            ws.origin,
            0.0,
        );
        d.scale = Vec2::new(f, 1.0);
        d.tint = tint;
        out.push(d);
    }
    // Body.
    let mut d = item(
        id,
        core.depth,
        sprite_visual(body, p.body.frame),
        pos + body.position,
        body.origin,
        0.0,
    );
    d.scale = Vec2::new(p.scale.x * f, p.scale.y);
    d.tint = tint;
    out.push(d);
    // Head.
    let head_id = match p.hat {
        HatState::NoHat => a.head_no_hat,
        HatState::Normal => a.head_normal,
        HatState::Crown => a.head_crown,
    };
    if p.head_visible
        && let Some(head) = c.sprite(head_id)
    {
        let ox = body
            .head_x_origins
            .get(body_frame)
            .map(|&v| v as f32)
            .unwrap_or(head.origin.x);
        // Big Heads (player.md §3.3): scale 2, head origin Y table - 5.
        let big = crate::pickups::variant(w, crate::variants::Variant::BigHeads, p.index as i32);
        let oy = body
            .head_y_origins
            .get(body_frame)
            .map(|&v| v as f32)
            .unwrap_or(head.origin.y)
            - if big { 5.0 } else { 0.0 };
        let mut d = item(
            id,
            core.depth,
            sprite_visual(head, p.head.frame),
            pos + head.position,
            Vec2::new(ox, oy),
            0.0,
        );
        d.scale = Vec2::new(p.scale.x * f, p.scale.y) * if big { 2.0 } else { 1.0 };
        d.tint = tint;
        out.push(d);
    }
    // Bow.
    if p.bow_visible
        && p.state() != PState::Ducking
        && let Some(bow) = c.sprite(a.bow)
    {
        let bx =
            bow.position.x + body.bow_x_offsets.get(body_frame).copied().unwrap_or(0) as f32 * f;
        let base_y = if p.aim_direction == PI / 2.0 {
            bow.down_y.unwrap_or(bow.position.y)
        } else {
            bow.position.y
        };
        let by = base_y + body.bow_y_offsets.get(body_frame).copied().unwrap_or(0) as f32;
        let mirrored = p.aim_direction >= PI / 2.0 || p.aim_direction < -PI / 2.0;
        let rot = if mirrored {
            p.aim_direction + PI
        } else {
            p.aim_direction
        };
        let mut d = item(
            id,
            core.depth,
            sprite_visual(bow, p.bow.frame),
            pos + Vec2::new(bx, by),
            bow.origin,
            rot,
        );
        d.scale = Vec2::new(if mirrored { -1.0 } else { 1.0 }, 1.0);
        d.tint = tint;
        out.push(d);
    }
    // Outline of every layer so far (player.md §19.1), not the shield.
    if p.invis_opacity >= 0.8 {
        for d in &mut out[first..] {
            d.outline = 255;
        }
    }
    // Shield (player.md §16.1): owner colour B, team colour in teams.
    if p.has_shield
        && let Some(sh) = c.sprite(c.common.shield)
    {
        let mut d = item(id, core.depth, sprite_visual(sh, 0), pos, sh.origin, 0.0);
        d.scale = Vec2::new(0.8, 0.6);
        d.tint = team_colors(p.team_color).map_or(a.color_b, |t| t.1);
        out.push(d);
    }
}

/// Light radius of a `LightSource` entity (0 = no light now): archers 50
/// (player.md §3.1), arrows 40 / 30 (laser, bolt, prism) / 70 (trigger),
/// fire-capable arrows only while burning (arrows.md §3.1, §3.3 step 6),
/// torches their current radius, large chests 80, enemies per
/// enemies.md §4, everything else the default 40.
pub fn light_radius(w: &World, id: crate::entity::EntityId) -> f32 {
    use crate::enemies::EnemyType as E;
    use crate::quiver::ArrowType as A;
    match w.kind(id) {
        Some(Kind::Player(p)) => {
            if p.dead {
                0.0
            } else {
                50.0
            }
        }
        Some(Kind::Arrow(a)) => match a.arrow_type {
            A::Normal | A::Feather if !a.extra.burning => 0.0,
            A::Laser | A::Bolt | A::Prism => 30.0,
            A::Trigger => 70.0,
            _ => 40.0,
        },
        Some(Kind::Tower(crate::entities::Tower::Torch(t))) => t.radius,
        Some(Kind::Tower(_)) => 0.0,
        Some(Kind::Chest(ch)) => {
            if ch.ty.large() {
                80.0
            } else {
                40.0
            }
        }
        Some(Kind::Enemy(e)) => match e.ty {
            E::Elemental | E::GreenElemental => 90.0,
            E::Cultist | E::ScytheCultist | E::BossCultist | E::Skeleton(_) | E::TechnoMage => 60.0,
            E::KingReaper => 100.0,
            _ => 40.0,
        },
        _ => 40.0,
    }
}

fn lights(w: &World) -> Vec<(Vec2, f32)> {
    w.tagged(crate::entity::Tag::LightSource)
        .iter()
        .filter(|&&id| w.core(id).visible)
        .filter_map(|&id| {
            let r = light_radius(w, id);
            (r > 0.0).then(|| (w.core(id).pos, r))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(entity: u32, outline: u8, x: f32) -> Drawable {
        let mut d = item(
            EntityId {
                index: entity,
                generation: 0,
            },
            0,
            Visual::Rect { w: 1.0, h: 1.0 },
            Vec2::new(x, 0.0),
            Vec2::ZERO,
            0.0,
        );
        d.outline = outline;
        d
    }

    #[test]
    fn outlines_go_under_all_layers_of_their_entity() {
        // Entity 1: body (outlined), bow (outlined), shield (not); entity 2: plain.
        let mut v = vec![d(1, 255, 0.0), d(1, 255, 5.0), d(1, 0, 9.0), d(2, 0, 20.0)];
        expand_outlines(&mut v);
        assert_eq!(v.len(), 16 + 4);
        // Sixteen black copies first, then the three layers in order.
        assert!(
            v[..16]
                .iter()
                .all(|o| o.tint == [0, 0, 0, 255] && o.outline == 0)
        );
        assert_eq!(v[0].pos, Vec2::new(-1.0, -1.0));
        assert_eq!(v[1].pos, Vec2::new(4.0, -1.0));
        assert_eq!(
            v[16..19].iter().map(|o| o.pos.x).collect::<Vec<_>>(),
            [0.0, 5.0, 9.0]
        );
        assert!(v[16..].iter().all(|o| o.tint == WHITE && o.outline == 0));
    }

    #[test]
    fn outline_alpha_carries_over() {
        let mut v = vec![d(3, 128, 0.0)];
        expand_outlines(&mut v);
        assert_eq!(v.len(), 9);
        assert_eq!(v[0].tint, [0, 0, 0, 128]);
    }
}
