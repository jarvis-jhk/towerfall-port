//! Miasma art (tower-entities.md 8.2): the side walls of the Versus time
//! limit and the Dark World boss miasma (the Amaranth on Legendary,
//! Dreadwood's stand-in, Cataclysm's finale) as sim `Drawable`s: a
//! translucent body (38004C at 60 %), `miasma/tentacles` with a vertical
//! sine distortion along the inner edge and the `MiasmaSurface` strips
//! bobbing on it; the opacity follows the wall's `Percent` and a slow
//! 240-tick pulse. Also Quest's floor miasma strips. Replaces the sim's
//! debug boxes.

use tfp_sim::entity::{EntityId, Kind};
use tfp_sim::{Content, Drawable, Vec2, View, Visual, World};

use super::art::{Art, frame_at};

const BODY: [u8; 3] = [0x38, 0x00, 0x4C];

fn item(visual: Visual, pos: Vec2, origin: Vec2, id: EntityId, depth: i32) -> Drawable {
    Drawable {
        visual,
        pos,
        origin,
        scale: Vec2::new(1.0, 1.0),
        rotation: 0.0,
        flip_x: false,
        flip_y: false,
        tint: [255; 4],
        depth,
        entity: id,
        outline: 0,
        clip_h: 1.0,
    }
}

/// Swaps the miasma's debug boxes for its art.
pub fn apply(view: &mut View, w: &World, c: &Content, art: &Art) {
    let mut add = Vec::new();
    let mut replaced = Vec::new();
    let surface = art.sprite("MiasmaSurface");
    let tentacles = art.tex("miasma/tentacles");
    let frame = w.frame as f32;
    for id in w.entities() {
        let Some(Kind::Miasma(m)) = w.kind(id) else {
            continue;
        };
        replaced.push(id);
        let core = w.core(id);
        let hb = core.hitbox();
        let depth = core.depth;
        let a = ((m.percent + 0.1) / 0.3).clamp(0.0, 1.0)
            * (0.5 + 0.2 * (frame / 240.0 * std::f32::consts::TAU).sin())
            / 0.7;
        if a <= 0.0 {
            continue;
        }
        let left = m.left;
        let x0 = (core.pos.x + hb.x).floor();
        let edge = if left { x0 + hb.w } else { x0 };
        // Body.
        if hb.w > 0.0 {
            let mut d = item(
                Visual::Rect { w: hb.w, h: 240.0 },
                Vec2::new(x0, 0.0),
                Vec2::ZERO,
                id,
                depth,
            );
            d.tint = [BODY[0], BODY[1], BODY[2], (153.0 * a.min(1.0)) as u8];
            add.push(d);
        }
        // Tentacles hanging over the inner edge (vertical sine).
        if let Some(t) = tentacles
            && let Some((tw, th)) = c.texture(t).size
        {
            let slice = 4u16;
            for i in 0..th.div_ceil(slice) {
                let dx =
                    ((frame * 0.05 + std::f32::consts::PI / 16.0 * i as f32).sin() * 2.0).round();
                let mut d = item(
                    Visual::Sprite {
                        tex: t,
                        frame: i as i16,
                        frame_w: tw,
                        frame_h: slice,
                    },
                    Vec2::new(edge + if left { dx } else { -dx }, 0.0),
                    Vec2::new(tw as f32 - 8.0, -((i * slice) as f32)),
                    id,
                    depth,
                );
                if !left {
                    d.scale.x = -1.0;
                }
                d.tint = [255, 255, 255, (255.0 * a.min(1.0)) as u8];
                add.push(d);
            }
        }
        // Surface strips along the edge, bobbing.
        if let Some(s) = c.sprite(surface) {
            let f = frame_at(s, "0", frame);
            let c100 = frame / 100.0 * std::f32::consts::TAU;
            for i in 0..6 {
                let bob = 3.0 * (c100 + i as f32 * std::f32::consts::FRAC_PI_4).sin();
                let y = 20.0 + 40.0 * i as f32;
                let mut d = item(
                    Visual::Sprite {
                        tex: s.texture,
                        frame: f,
                        frame_w: s.frame_w,
                        frame_h: s.frame_h,
                    },
                    Vec2::new(edge + if left { bob } else { -bob }, y),
                    Vec2::new(s.frame_w as f32 / 2.0, s.origin.y),
                    id,
                    depth,
                );
                d.rotation = if left {
                    std::f32::consts::FRAC_PI_2
                } else {
                    -std::f32::consts::FRAC_PI_2
                };
                d.tint = [255, 255, 255, (255.0 * a.min(1.0)) as u8];
                add.push(d);
            }
        }
    }
    // Quest floor miasma (tower-entities.md 8.4): one 10-px `FloorMiasma`
    // sprite per 10 px at (10 i, 5), scaled in Y by the strip's scale;
    // animation 0 while starting up / arming, 1 idle, 2 dying out.
    let fsprite = art.sprite("FloorMiasma");
    for id in w.entities() {
        let Some(Kind::FloorMiasma(f)) = w.kind(id) else {
            continue;
        };
        replaced.push(id);
        let core = w.core(id);
        let Some(s) = c.sprite(fsprite) else { continue };
        if !core.visible {
            continue;
        }
        use tfp_sim::quest::floor;
        let (anim, t) = match f.state {
            floor::START_UP | floor::ACTIVATING => ("0", f.t),
            floor::DIE_OUT => ("2", f.t),
            _ => ("1", frame),
        };
        let cell = frame_at(s, anim, t);
        let n = (core.hitbox().w / 10.0).floor() as i32;
        for i in 0..n {
            let mut d = item(
                Visual::Sprite {
                    tex: s.texture,
                    frame: cell,
                    frame_w: s.frame_w,
                    frame_h: s.frame_h,
                },
                Vec2::new(
                    core.pos.x.floor() + 10.0 * i as f32,
                    core.pos.y.floor() + 5.0,
                ),
                s.origin,
                id,
                core.depth,
            );
            d.scale.y = f.scale;
            add.push(d);
        }
    }
    if replaced.is_empty() {
        return;
    }
    view.items.retain(|d| !replaced.contains(&d.entity));
    view.items.extend(add);
}
