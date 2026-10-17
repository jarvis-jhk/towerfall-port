//! Small in-level marks drawn from the sim's view, inside the darkened
//! gameplay layer like the original's entity components: the quiver above
//! each archer (arrows.md 14), Show Treasure Spawns outlines (versus.md
//! 13.2) and the cooldown stars (Show Dodge Cooldown; enemies.md 1.10).

use tfp_assets::look::SpriteSet;
use tfp_render::glam::Vec2;
use tfp_render::{Color, DrawList, SpriteParams};
use tfp_sim::View;

use crate::tower::TowerGfx;

/// Quiver icons by arrow type (index = type).
const QUIVER_ICONS: [&str; 11] = [
    "player/arrowHUD/arrow",
    "player/arrowHUD/bombArrow",
    "player/arrowHUD/superBombArrow",
    "player/arrowHUD/laserArrow",
    "player/arrowHUD/brambleArrow",
    "player/arrowHUD/drillArrow",
    "player/arrowHUD/boltArrow",
    "player/arrowHUD/squeakyArrow",
    "player/arrowHUD/featherArrow",
    "player/arrowHUD/triggerArrow",
    "player/arrowHUD/prismArrow",
];

/// Icon tint per arrow type.
const QUIVER_COLORS: [u32; 11] = [
    0xF7EAC3, 0xF8B800, 0xF8B800, 0xB8F818, 0xF87858, 0x8EE8FF, 0x00FF4C, 0xFF6DFA, 0xBC70FF,
    0x1BB7EE, 0xDB4ADB,
];

const NONE_COLORS: [u32; 2] = [0xF83800, 0xF87858];

fn v(p: tfp_sim::Vec2) -> Vec2 {
    Vec2::new(p.x, p.y)
}

pub fn draw_marks(dl: &mut DrawList, view: &View, g: &TowerGfx, t: u64) {
    // Treasure previews: pulsing contour at 60 % white, scale +-20 %.
    for &(pos, big) in &view.treasure_previews {
        let pos = v(pos);
        let name = if big {
            "bigChestContour"
        } else {
            "treasureChestContour"
        };
        if let Some(s) = g.main_sub(name) {
            let k = 1.0 + 0.2 * (t as f32 * 0.1).sin();
            let size = Vec2::new(s.rect.w as f32, s.rect.h as f32);
            dl.sprite(
                s,
                &SpriteParams::at(pos)
                    .origin(size / 2.0)
                    .scale(Vec2::splat(k))
                    .color(Color::WHITE * 0.6),
            );
        }
    }
    // Quivers.
    for q in view.quivers.iter().filter(|q| q.shown) {
        let qp = v(q.pos);
        let n = q.arrows.len() + q.triggers as usize;
        let x0 = qp.x - 2.0 * n as f32;
        if let Some(trig) = g.main_sub("player/arrowHUD/trigger") {
            let c = Color::hex(if q.trigger_flash { 0xEBFF5B } else { 0xFF2E16 });
            for i in 0..q.triggers {
                let p = Vec2::new(x0 + 4.0 * i as f32, qp.y - 20.0).floor();
                dl.sprite(trig, &SpriteParams::at(p).color(c));
            }
        }
        for (i, &a) in q.arrows.iter().enumerate() {
            let k = (a as usize).min(10);
            if let Some(s) = g.main_sub(QUIVER_ICONS[k]) {
                let p = Vec2::new(x0 + 4.0 * (i + q.triggers as usize) as f32, qp.y - 22.0).floor();
                dl.sprite(s, &SpriteParams::at(p).color(Color::hex(QUIVER_COLORS[k])));
            }
        }
        if let Some(phase) = q.empty_blink
            && let Some(s) = g.main_sub("player/noArrowHUD")
        {
            let size = Vec2::new(s.rect.w as f32, s.rect.h as f32);
            dl.sprite(
                s,
                &SpriteParams::at(qp + Vec2::new(0.0, -18.0))
                    .origin(size / 2.0)
                    .color(Color::hex(NONE_COLORS[phase as usize % 2])),
            );
        }
    }
    // Cooldown stars (the sim gives the star positions).
    for &p in &view.cooldowns {
        if let Some((s, origin, _)) = g.sprite(SpriteSet::Main, "StunStars", "0", 0, t) {
            dl.sprite(s, &SpriteParams::at(v(p)).origin(origin));
        }
    }
}
