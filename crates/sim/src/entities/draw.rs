//! Drawables of the tower entities for [`crate::view::View`]. Art comes
//! from [`super::art::TowerArt`]; without art (ASCII sandboxes) entities
//! draw as coloured boxes.

use super::{Tower, blocks, hazards, platforms, scenery};
use crate::content::{Content, SpriteDef, TexId};
use crate::entity::EntityId;
use crate::math::{PI, Vec2, floor, sin};
use crate::view::{Drawable, Visual};
use crate::world::World;

const WHITE: [u8; 4] = [255; 4];

struct Out<'a> {
    v: &'a mut Vec<Drawable>,
    id: EntityId,
    depth: i32,
}

impl Out<'_> {
    fn push(&mut self, visual: Visual, pos: Vec2, origin: Vec2, tint: [u8; 4]) -> &mut Drawable {
        self.v.push(Drawable {
            visual,
            pos,
            origin,
            scale: Vec2::ONE,
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            tint,
            depth: self.depth,
            entity: self.id,
        });
        self.v.last_mut().unwrap()
    }
    fn rect(&mut self, pos: Vec2, w: f32, h: f32, tint: [u8; 4]) {
        if w > 0.0 && h > 0.0 {
            self.push(Visual::Rect { w, h }, pos, Vec2::ZERO, tint);
        }
    }
    /// One `fw x fh` cell (row-major `frame`) of a texture.
    fn cell(
        &mut self,
        tex: TexId,
        frame: i16,
        fw: u16,
        fh: u16,
        pos: Vec2,
        tint: [u8; 4],
    ) -> &mut Drawable {
        self.push(
            Visual::Sprite {
                tex,
                frame,
                frame_w: fw,
                frame_h: fh,
            },
            pos,
            Vec2::ZERO,
            tint,
        )
    }
    fn whole(&mut self, tex: TexId, pos: Vec2, origin: Vec2, tint: [u8; 4]) -> &mut Drawable {
        self.push(
            Visual::Sprite {
                tex,
                frame: -1,
                frame_w: 0,
                frame_h: 0,
            },
            pos,
            origin,
            tint,
        )
    }
    fn sprite(&mut self, s: &SpriteDef, frame: i16, pos: Vec2, tint: [u8; 4]) -> &mut Drawable {
        self.push(
            Visual::Sprite {
                tex: s.texture,
                frame,
                frame_w: s.frame_w,
                frame_h: s.frame_h,
            },
            pos,
            s.origin,
            tint,
        )
    }
    /// A 9-slice of 10 px tiles from a 30 x 30 sheet over `w x h`.
    fn nine(&mut self, tex: Option<TexId>, pos: Vec2, w: f32, h: f32, tint: [u8; 4], fb: [u8; 4]) {
        let Some(tex) = tex else {
            self.rect(pos, w, h, fb);
            return;
        };
        let (cols, rows) = ((w / 10.0) as i32, (h / 10.0) as i32);
        for y in 0..rows {
            for x in 0..cols {
                let tx = if cols == 1 || x == 0 {
                    0
                } else if x == cols - 1 {
                    2
                } else {
                    1
                };
                let ty = if rows == 1 || y == 0 {
                    0
                } else if y == rows - 1 {
                    2
                } else {
                    1
                };
                let p = pos + Vec2::new(x as f32 * 10.0, y as f32 * 10.0);
                self.cell(tex, (ty * 3 + tx) as i16, 10, 10, p, tint);
            }
        }
    }
    /// Left cap / middle / right cap columns of a 30-wide sheet, `fh` tall.
    #[allow(clippy::too_many_arguments)]
    fn row3(
        &mut self,
        tex: Option<TexId>,
        pos: Vec2,
        w: f32,
        fh: u16,
        row: i16,
        tint: [u8; 4],
        fb: [u8; 4],
    ) {
        let Some(tex) = tex else {
            self.rect(pos, w, fh as f32, fb);
            return;
        };
        let n = (w / 10.0) as i32;
        for i in 0..n {
            let col = if i == 0 {
                0
            } else if i == n - 1 {
                2
            } else {
                1
            };
            self.cell(
                tex,
                row * 3 + col,
                10,
                fh,
                pos + Vec2::new(i as f32 * 10.0, 0.0),
                tint,
            );
        }
    }
}

/// Frame of a sprite animation `ticks` frames after it started.
fn anim_frame(s: &SpriteDef, anim: &str, ticks: f32, looped: bool) -> i16 {
    let Some(a) = s.anims.iter().find(|a| a.id == anim) else {
        return 0;
    };
    if a.frames.is_empty() {
        return 0;
    }
    let per = (a.delay * 60.0).max(1.0);
    let mut i = floor(ticks / per) as usize;
    if looped || a.looped {
        i %= a.frames.len();
    } else {
        i = i.min(a.frames.len() - 1);
    }
    a.frames[i] as i16
}

fn alpha(t: [u8; 4], a: f32) -> [u8; 4] {
    [t[0], t[1], t[2], (a.clamp(0.0, 1.0) * 255.0) as u8]
}

/// Append the drawables of tower entity `id`.
pub fn items(v: &mut Vec<Drawable>, w: &World, c: &Content, id: EntityId, t: &Tower) {
    let core = w.core(id);
    if !core.flash.shown {
        return;
    }
    let art = &c.tower_art;
    let mut o = Out {
        v,
        id,
        depth: core.depth,
    };
    let pos = core.pos;
    let hb = core.hitbox();
    let frame_t = w.frame as f32;
    let sp = |n: &str| c.sprite(art.sprite(n));
    match t {
        Tower::MovingPlatform(m) => {
            let tex = art.tex(if m.cataclysm {
                "cataclysmMovingTiles"
            } else {
                "movingTiles"
            });
            o.nine(tex, pos, hb.w, hb.h, WHITE, [120, 110, 90, 255]);
        }
        Tower::ShiftBlock(s) => {
            let p = pos + s.jitter;
            o.nine(
                art.tex("shiftBlock"),
                p,
                hb.w,
                hb.h,
                WHITE,
                [110, 110, 120, 255],
            );
            let face = if hb.w > 20.0 && hb.h > 20.0 {
                "shiftBlockFace"
            } else if hb.h <= 20.0 {
                "shiftBlockFaceHorizontal"
            } else {
                "shiftBlockFaceVertical"
            };
            if let Some(f) = art.tex(face) {
                let angry = s.state != platforms::ShiftState::Waiting;
                let frame = s.face as i16 + if angry { 2 } else { 0 };
                let centre = p + Vec2::new(hb.w / 2.0 - 10.0, hb.h / 2.0 - 10.0);
                o.cell(f, frame, 20, 20, centre, WHITE);
            }
        }
        Tower::LoopPlatform(l) => {
            if l.master || matches!(l.dir, super::Dir4::Left | super::Dir4::Right) {
                ghost_strip(&mut o, art.tex("loopPlatformTiles"), pos, hb.w, 0, 0.8);
            }
        }
        Tower::GhostPlatform(g) => {
            ghost_strip(
                &mut o,
                art.tex("loopPlatformTiles"),
                pos,
                hb.w,
                g.frame as i16,
                g.alpha,
            );
        }
        Tower::RotatePlatform(_) => {
            ghost_strip(&mut o, art.tex("loopPlatformTiles"), pos, hb.w, 0, 0.8);
        }
        Tower::SensorBlock(s) => {
            let j = if s.shake > 0.0 {
                Vec2::new(sin(frame_t * 2.3), sin(frame_t * 3.1))
            } else {
                Vec2::ZERO
            };
            let tex = art.tex(if s.cataclysm {
                "cataclysmSensorBlock"
            } else {
                "sensorBlock"
            });
            o.row3(tex, pos + j, hb.w, 30, 0, WHITE, [90, 80, 100, 255]);
            if s.glow > 0.0 {
                o.row3(
                    art.tex("sensorLights"),
                    pos + j,
                    hb.w,
                    30,
                    0,
                    alpha(WHITE, s.glow),
                    [0; 4],
                );
            }
        }
        Tower::CrackedPlatform(b) => {
            let fb = [130, 160, 170, 255];
            o.row3(
                art.tex("crackedPlatform"),
                pos + b.jitter,
                hb.w,
                10,
                0,
                WHITE,
                fb,
            );
        }
        Tower::BreakBlock(b) => {
            if b.crumble {
                crumble_tiles(&mut o, art.tex("crumbleBlockTiles"), pos, b.w, b.h);
            } else if let Some(tex) = art.tex("crackedWall") {
                o.whole(tex, pos, Vec2::ZERO, b.crack_color);
            } else {
                o.rect(pos, 20.0, 20.0, b.crack_color);
            }
        }
        Tower::CrumbleWall(b) => {
            let p = pos + b.jitter;
            match art.tex("crumbleWall") {
                Some(t) => {
                    o.whole(t, p, Vec2::ZERO, WHITE);
                }
                None => o.rect(p, 20.0, 20.0, [150, 120, 110, 255]),
            }
            if let Some(t) = art.tex("crumbleWallLines") {
                o.whole(t, p, Vec2::ZERO, alpha(WHITE, b.fade));
            }
        }
        Tower::MoonGlass(m) => {
            let tint = alpha([255, 255, 255, 255], 0.9);
            o.nine(
                art.tex("moonGlassTiles"),
                pos,
                hb.w,
                hb.h,
                tint,
                [150, 180, 255, 140],
            );
            if m.scale != 1.0 {
                let last = o.v.len();
                let _ = last;
            }
        }
        Tower::Granite => {
            o.nine(
                art.tex("graniteTiles"),
                pos,
                hb.w,
                hb.h,
                WHITE,
                [100, 100, 100, 255],
            );
        }
        Tower::SwitchBlock(b) => {
            let name = match (b.red, core.collidable) {
                (true, true) => "redSwitchBlock",
                (true, false) => "redSwitchLines",
                (false, true) => "blueSwitchBlock",
                (false, false) => "blueSwitchLines",
            };
            let a = if b.pending { 0.3 } else { 1.0 };
            let fb = if b.red {
                [200, 40, 40, 255]
            } else {
                [40, 60, 200, 255]
            };
            let fb = if core.collidable { fb } else { alpha(fb, 0.3) };
            o.nine(art.tex(name), pos, hb.w, hb.h, alpha(WHITE, a), fb);
            let flick = b.flicker > 0.0 && (b.flicker as i32 / 5) % 2 == 0;
            if flick || b.warning > 0.0 {
                let a = if flick { 0.8 } else { b.warning };
                o.rect(pos, hb.w, hb.h, alpha([0xb8, 0xf8, 0xb8, 255], a));
            }
        }
        Tower::ProximityBlock(b) => {
            if b.scale > 0.0 {
                match sp("ProximityBlock") {
                    Some(s) => {
                        let d = o.sprite(s, b.frame as i16, pos, WHITE);
                        d.scale = Vec2::new(b.scale, b.scale);
                        d.rotation = b.rotation;
                    }
                    None => {
                        let d = o.push(
                            Visual::Rect { w: 20.0, h: 20.0 },
                            pos,
                            Vec2::new(10.0, 10.0),
                            [80, 100, 200, 255],
                        );
                        d.scale = Vec2::new(b.scale, b.scale);
                    }
                }
            }
        }
        Tower::FakeWall(f) => o.rect(pos, hb.w, hb.h, alpha([60, 60, 70, 255], f.alpha)),
        Tower::JumpPad(p) => {
            let name = if p.on { "jumpPadOn" } else { "jumpPadOff" };
            let n = p.n as usize;
            for i in 0..n {
                let col = if i == 0 {
                    0
                } else if i == n - 1 {
                    2
                } else {
                    1
                };
                let bottom = pos + Vec2::new(i as f32 * 10.0, 10.0);
                match art.tex(name) {
                    Some(t) => {
                        let d = o.cell(t, col, 10, 10, bottom, WHITE);
                        d.origin = Vec2::new(0.0, 10.0);
                        d.scale = Vec2::new(1.0, p.scales[i]);
                    }
                    None => {
                        let h = 10.0 * p.scales[i];
                        o.rect(bottom - Vec2::new(0.0, h), 10.0, h, [80, 220, 100, 255]);
                    }
                }
            }
        }
        Tower::Ice(i) => {
            let n = (i.w0 / 10.0) as i32;
            let (l, r) = (core.left(), core.right());
            for k in 0..n {
                let x = i.x0 + k as f32 * 10.0;
                let present = core.collidable && x >= l && x + 10.0 <= r;
                if !present && i.melt_alpha <= 0.0 {
                    continue;
                }
                let tile = if n == 1 {
                    6
                } else if k == 0 {
                    0
                } else if k == n - 1 {
                    5
                } else {
                    1 + (k % 3)
                };
                let a = if present { 1.0 } else { i.melt_alpha };
                match art.tex("ice") {
                    Some(t) => {
                        o.cell(t, tile as i16, 10, 10, Vec2::new(x, pos.y), alpha(WHITE, a));
                    }
                    None => o.rect(
                        Vec2::new(x, pos.y),
                        10.0,
                        5.0,
                        alpha([180, 230, 255, 255], a),
                    ),
                }
            }
        }
        Tower::Mud(m) => {
            for k in 0..m.n as usize {
                let cell = m.cells[k];
                let dy = sin(m.sine + cell.add) * cell.mult * 2.0;
                let n = m.n as usize;
                let col = if k == 0 {
                    0
                } else if k == n - 1 {
                    2
                } else {
                    1
                };
                let p = pos + Vec2::new(k as f32 * 10.0, dy);
                match art.tex("mud") {
                    Some(t) => {
                        o.cell(t, col, 10, 10, p, WHITE);
                    }
                    None => o.rect(p, 10.0, 5.0, [80, 60, 40, 255]),
                }
            }
        }
        Tower::HotCoals => {
            let dark = c.level(w.level).tower.theme.dark;
            let tex = art.tex(if dark { "hotCoalsGreen" } else { "hotCoals" });
            let n = (hb.w / 10.0) as i32;
            let row = ((frame_t / 18.0) as i32 % 6) as i16;
            for k in 0..n {
                let col = if n == 1 {
                    6
                } else if k == 0 {
                    0
                } else if k == n - 1 {
                    5
                } else {
                    1 + (k % 3)
                };
                let p = pos + Vec2::new(k as f32 * 10.0, -2.0);
                match tex {
                    Some(t) => {
                        o.cell(t, row * 7 + col as i16, 10, 10, p, WHITE);
                    }
                    None => o.rect(p, 10.0, 5.0, [240, 120, 40, 255]),
                }
            }
        }
        Tower::SnowClump(s) => match art.tex("details/snowDeposit") {
            Some(t) => {
                let d = o.cell(t, s.variant as i16, 14, 14, pos, alpha(WHITE, s.alpha));
                d.origin = Vec2::new(2.0, 9.0);
            }
            None => o.rect(
                pos + Vec2::new(0.0, -3.0),
                10.0,
                3.0,
                alpha([235, 240, 255, 255], s.alpha),
            ),
        },
        Tower::LavaControl(_) => {}
        Tower::Lava(l) => {
            let dark = c.level(w.level).tower.theme.dark;
            let col = if dark {
                [0x00, 0xe6, 0x05, 255]
            } else {
                [0xf8, 0x38, 0x00, 255]
            };
            o.depth = core.depth;
            o.rect(pos + Vec2::new(hb.x, hb.y), hb.w, hb.h, col);
            let _ = l;
        }
        Tower::Spikeball(s) => {
            let n = (floor(s.radius) as i32 / 8 - 1).max(0);
            if let Some(ch) = art.tex("spikeBallChain") {
                o.whole(ch, s.pivot, Vec2::new(4.0, 4.0), WHITE);
                for k in 1..=n {
                    let p = s.pivot + (pos - s.pivot) * (k as f32 / (n + 1) as f32);
                    o.whole(ch, p, Vec2::new(4.0, 4.0), WHITE);
                }
            }
            let j = if s.jitter > 0.0 {
                Vec2::new(sin(frame_t * 1.7) * 2.0, sin(frame_t * 2.9) * 2.0)
            } else {
                Vec2::ZERO
            };
            if s.explodes
                && let Some(sd) = sp("ExplodingSpikeball")
            {
                let f = anim_frame(
                    sd,
                    sd.anims.first().map(|a| a.id.as_str()).unwrap_or(""),
                    frame_t,
                    true,
                );
                o.sprite(sd, f, pos + j, WHITE);
            } else if let Some(t) = art.tex("spikeBall") {
                o.whole(t, pos + j, Vec2::new(11.0, 11.0), WHITE);
            } else {
                o.push(
                    Visual::Rect { w: 14.0, h: 14.0 },
                    pos + j,
                    Vec2::new(7.0, 7.0),
                    [160, 160, 170, 255],
                );
            }
        }
        Tower::Icicle(i) => match sp("icicle") {
            Some(s) => {
                o.sprite(s, 0, pos + Vec2::new(i.shift, 0.0), WHITE);
            }
            None => o.rect(
                pos + Vec2::new(-4.0 + i.shift, 0.0),
                8.0,
                12.0,
                [180, 230, 250, 255],
            ),
        },
        Tower::Orb(b) => {
            if b.explodes
                && let Some(s) = sp("ExplodingOrb")
            {
                let f = anim_frame(
                    s,
                    s.anims.first().map(|a| a.id.as_str()).unwrap_or(""),
                    frame_t,
                    true,
                );
                o.sprite(s, f, pos, WHITE);
            } else if let Some(t) = art.tex("orb") {
                o.whole(t, pos, Vec2::new(6.0, 6.0), WHITE);
            } else {
                o.push(
                    Visual::Rect { w: 12.0, h: 12.0 },
                    pos,
                    Vec2::new(6.0, 6.0),
                    [220, 120, 200, 255],
                );
            }
        }
        Tower::Lantern(l) => {
            let frame = if l.dead {
                2
            } else {
                ((frame_t / 12.0) as i16) % 2
            };
            match art.tex("lantern") {
                Some(t) => {
                    let d = o.cell(t, frame, 10, 10, pos, WHITE);
                    d.origin = if l.fallen {
                        Vec2::new(5.0, 5.0)
                    } else {
                        Vec2::new(5.0, 0.0)
                    };
                    d.rotation = l.rotation;
                }
                None => o.rect(pos + Vec2::new(-3.0, 0.0), 6.0, 10.0, [250, 220, 120, 255]),
            }
        }
        Tower::Chain(ch) => {
            let (links, _) = scenery::chain_links(ch);
            for (k, link) in links.iter().enumerate().take(ch.n as usize) {
                let var = ((ch.variants >> (2 * k)) & 3) as i16;
                match art.tex("chain") {
                    Some(t) => {
                        let d = o.cell(t, var, 10, 10, pos + *link, WHITE);
                        d.origin = Vec2::new(5.0, 0.0);
                        d.rotation = ch.theta[k];
                    }
                    None => o.rect(
                        pos + *link + Vec2::new(-1.0, 0.0),
                        2.0,
                        10.0,
                        [150, 150, 150, 255],
                    ),
                }
            }
        }
        Tower::Cobwebs(cw) => {
            if let Some(t) = art.tex("details/cobwebs") {
                let right = cw.corner % 2 == 1;
                let p = pos + Vec2::new(if right { -10.0 } else { 0.0 }, 0.0);
                let d = o.cell(t, cw.frame as i16, 20, 20, p, alpha(WHITE, cw.alpha));
                d.flip_x = right;
            }
        }
        Tower::Torch(tc) => {
            let name = match tc.lanterns {
                1 => "details/skullLantern",
                2 => "details/torchLantern",
                3 => "details/wingTorch",
                _ => "details/lantern",
            };
            if let Some(t) = art.tex(name) {
                let d = o.whole(t, pos, Vec2::new(5.0, 0.0), WHITE);
                d.visual = Visual::Sprite {
                    tex: t,
                    frame: if tc.lit { 0 } else { 1 },
                    frame_w: 10,
                    frame_h: 10,
                };
            }
        }
        Tower::Decor(d) => decor(&mut o, w, c, pos, d, frame_t),
        Tower::BigMushroom(m) => {
            if let Some(t) = art.tex("details/bigMushroomBase") {
                o.whole(t, pos, Vec2::new(5.0, 10.0), WHITE);
            }
            if let Some(t) = art.tex("details/bigMushroom") {
                let off = scenery::big_mushroom_value(m) * 4.0;
                let dr = o.whole(t, pos + Vec2::new(off, -10.0), Vec2::new(5.0, 20.0), WHITE);
                let _ = dr;
            }
        }
        Tower::Skeleton(s) => {
            if let Some(t) = art.tex("details/bones") {
                let base = pos + Vec2::new(if s.flip { 5.0 } else { -5.0 }, 0.0);
                let d = o.cell(t, s.frame as i16, 20, 20, base, WHITE);
                d.origin = Vec2::new(10.0, 20.0);
                d.flip_x = s.flip;
                d.scale = Vec2::new(1.0, s.scale_y.max(0.0));
            }
        }
        Tower::WaterDrop(d) => {
            if let Some(s) = sp("WaterDrop") {
                let f = match d.state {
                    scenery::DropState::Forming => ((18.0 - d.timer) / 3.0).clamp(0.0, 5.0) as i16,
                    scenery::DropState::Falling => 5,
                    scenery::DropState::Splash => {
                        let base = if d.on_creature { 9 } else { 6 };
                        base + ((9.0 - d.timer) / 3.0).clamp(0.0, 2.0) as i16
                    }
                    scenery::DropState::Hidden => return,
                };
                o.sprite(s, f, pos, WHITE);
            } else {
                o.rect(pos - Vec2::new(1.0, 1.0), 2.0, 2.0, [150, 200, 255, 255]);
            }
        }
        Tower::KingIntro(k) => {
            if let Some(s) = sp("KingReaperIntro") {
                let f = match k.anim {
                    1 => anim_frame(s, "pissed", k.t, false),
                    2 => anim_frame(s, "happy", k.t, false),
                    3 => anim_frame(s, "transform", k.t, false),
                    _ => anim_frame(s, "idle", 0.0, true),
                };
                o.sprite(s, f, pos, WHITE);
            }
        }
        Tower::Dummy(dm) => match sp("Dummy") {
            Some(s) => {
                let d = o.sprite(s, if dm.chain.is_some() { 1 } else { 0 }, pos, WHITE);
                d.flip_x = dm.facing_left;
            }
            None => o.rect(pos + Vec2::new(-4.0, 2.0), 8.0, 18.0, [200, 170, 120, 255]),
        },
        Tower::SwitchControl(_) => {}
    }
    let _ = (blocks::PAD_SEGMENTS, hazards::BOTTOM, PI);
}

fn ghost_strip(o: &mut Out, tex: Option<TexId>, pos: Vec2, w: f32, frame: i16, a: f32) {
    let tint = alpha(WHITE, a);
    match tex {
        Some(t) => {
            let n = (w / 10.0) as i32;
            for i in 0..n {
                let col = if i == 0 {
                    0
                } else if i == n - 1 {
                    2
                } else {
                    1
                };
                o.cell(
                    t,
                    frame.clamp(0, 2) * 3 + col,
                    10,
                    12,
                    pos + Vec2::new(i as f32 * 10.0, -4.0),
                    tint,
                );
            }
        }
        None => o.rect(pos, w, 5.0, alpha([190, 255, 240, 255], a)),
    }
}

fn crumble_tiles(o: &mut Out, tex: Option<TexId>, pos: Vec2, w: f32, h: f32) {
    let Some(t) = tex else {
        o.rect(pos, w, h, [185, 223, 254, 255]);
        return;
    };
    let (cols, rows) = ((w / 10.0) as i32, (h / 10.0) as i32);
    for y in 0..rows {
        for x in 0..cols {
            // Deterministic variant choice per cell (cosmetic).
            let alt = (x * 7 + y * 13) % 2;
            let tx = if cols == 1 {
                4
            } else if x == 0 {
                0
            } else if x == cols - 1 {
                3
            } else {
                1 + alt
            };
            let ty = if rows == 1 {
                4
            } else if y == 0 {
                0
            } else if y == rows - 1 {
                3
            } else {
                1 + alt
            };
            let (tx, ty) = if cols == 1 && rows > 1 {
                (
                    4,
                    if y == 0 {
                        0
                    } else if y == rows - 1 {
                        3
                    } else {
                        1 + alt
                    },
                )
            } else if rows == 1 && cols > 1 {
                (
                    if x == 0 {
                        0
                    } else if x == cols - 1 {
                        3
                    } else {
                        1 + alt
                    },
                    4,
                )
            } else {
                (tx, ty)
            };
            o.cell(
                t,
                (ty * 5 + tx) as i16,
                10,
                10,
                pos + Vec2::new(x as f32 * 10.0, y as f32 * 10.0),
                WHITE,
            );
        }
    }
}

fn decor(o: &mut Out, _w: &World, c: &Content, pos: Vec2, d: &scenery::Decor, ft: f32) {
    use super::PlacementKind as K;
    let art = &c.tower_art;
    let sp = |n: &str| c.sprite(art.sprite(n));
    match d.kind {
        K::BGCrystal => {
            if let Some(s) = sp("BGCrystal") {
                let f = anim_frame(
                    s,
                    s.anims.first().map(|a| a.id.as_str()).unwrap_or(""),
                    ft,
                    true,
                );
                o.sprite(s, f, pos, WHITE);
            }
        }
        K::BGMushroom => {
            if let Some(t) = art.tex("details/wallMushroom") {
                let d = o.cell(t, ((ft / 6.0) as i16) % 3, 10, 10, pos, WHITE);
                d.origin = Vec2::new(5.0, 0.0);
            }
        }
        K::GhostShipWindow => {
            if let Some(s) = sp("ShipWindow") {
                const SEQ: [i16; 8] = [0, 0, 1, 2, 3, 3, 2, 1];
                let f = SEQ[((ft / 12.0) as usize) % 8];
                o.sprite(s, f, pos, WHITE);
            }
        }
        K::PirateBanner => {
            let name = match (o.depth == -159, d.variant) {
                (true, 0) => "details/pirateBannerC",
                (true, _) => "details/pirateBannerD",
                (false, 0) => "details/pirateBannerA",
                (false, _) => "details/pirateBannerB",
            };
            if let Some(t) = art.tex(name) {
                let sway = sin(d.phase + ft * 0.13) * 2.5;
                let dr = o.whole(
                    t,
                    pos + Vec2::new(sway * 0.3, 0.0),
                    Vec2::new(10.0, 0.0),
                    WHITE,
                );
                let _ = dr;
            }
        }
        K::RainDrops => {
            if let Some(s) = sp("RainDrops") {
                let n = 1;
                for i in 0..n {
                    let f = anim_frame(
                        s,
                        s.anims.first().map(|a| a.id.as_str()).unwrap_or(""),
                        ft + i as f32 * 7.0,
                        true,
                    );
                    o.sprite(s, f, pos + Vec2::new(5.0, 10.0), WHITE);
                }
            }
        }
        K::WhiteArcherPrison => {
            if let Some(s) = sp("WhiteArcherPrison") {
                o.sprite(s, 19, pos, WHITE);
            }
        }
        K::Ogmo => {
            if let Some(s) = sp("Ogmo") {
                o.sprite(s, 0, pos, WHITE);
            }
        }
        _ => {}
    }
}
