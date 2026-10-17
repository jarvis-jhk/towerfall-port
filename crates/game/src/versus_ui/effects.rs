//! Sprite effects the sim only signals: explosions with their lattice of
//! core / kill / push sprites (arrows.md 13.1-13.2; `EnvEvent::Particles`
//! "Explosion*"), the shock circle and the small shock (13.3), and the
//! physical debris of tower-entities.md 6.11 (`BombParticle:<type>`).
//! Cosmetic only: own state and RNG, never fed back to the sim.

use tfp_assets::look::SpriteSet;
use tfp_render::glam::Vec2;
use tfp_render::{Color, DrawList, SpriteParams};
use tfp_sim::content::LevelDef;

use crate::tower::TowerGfx;

/// One animated sprite: starts after `delay` frames, ends after `len`.
#[derive(Clone, Debug)]
struct Cell {
    sprite: &'static str,
    pos: Vec2,
    delay: u32,
    len: u32,
    fade: f32,
}

/// A debris piece (`BombParticle`).
#[derive(Clone, Debug)]
struct Debris {
    pos: Vec2,
    vel: Vec2,
    life: u32,
    t: u32,
    xb: f32,
    yb: f32,
    size: f32,
    color: Color,
    /// `stoneDebris` at half scale instead of a square.
    stone: bool,
}

/// Debris kinds of tower-entities.md 6.11.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebrisKind {
    Bomb,
    GreenBomb,
    CrackedWall,
    CrumbleBlock,
    Icicle,
}

impl DebrisKind {
    pub fn from_name(n: &str) -> Option<DebrisKind> {
        Some(match n {
            "Bomb" => DebrisKind::Bomb,
            "GreenBomb" => DebrisKind::GreenBomb,
            "CrackedWall" => DebrisKind::CrackedWall,
            "CrumbleBlock" => DebrisKind::CrumbleBlock,
            "Icicle" => DebrisKind::Icicle,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Default)]
struct Effect {
    cells: Vec<Cell>,
    debris: Vec<Debris>,
    t: u32,
}

/// Explosions waiting for the level's solid grid (resolved by the owner).
#[derive(Clone, Copy, Debug)]
pub enum Pending {
    Explosion {
        pos: Vec2,
        sup: bool,
        green: bool,
    },
    Shock {
        pos: Vec2,
        small: bool,
    },
    Debris {
        kind: DebrisKind,
        pos: Vec2,
        count: u16,
        range: Vec2,
        dir: f32,
    },
}

#[derive(Clone, Default)]
pub struct Effects {
    pending: Vec<Pending>,
    live: Vec<Effect>,
    rng: u64,
    /// The theme's cracked-block colour (CrackedWall debris).
    pub crack_color: Option<Color>,
}

const CORE: u32 = 39; // 8 frames x 0.08 s
const KILL: u32 = 54; // 9 x 0.1 s
const PUSH: u32 = 48; // 8 x 0.1 s

impl Effects {
    pub fn push(&mut self, p: Pending) {
        self.pending.push(p);
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.live.clear();
    }

    fn rand(&mut self) -> f32 {
        let x = crate::front::variants::next_rand(&mut self.rng);
        (x >> 11) as f32 / (1u64 << 53) as f32
    }

    /// Builds the pending effects against the level (cells inside solids
    /// are left out, x wraps at the level width).
    pub fn resolve(&mut self, level: &LevelDef) {
        let w = level.cells_x as f32 * 10.0;
        let solid = |p: Vec2| {
            let x = p.x.rem_euclid(w);
            level.solid((x / 10.0).floor() as i32, (p.y / 10.0).floor() as i32)
        };
        for p in std::mem::take(&mut self.pending) {
            let mut e = Effect::default();
            match p {
                Pending::Explosion { pos, sup, green } => {
                    let (kill, offsets): (f32, Vec<(i32, i32)>) = if sup {
                        let mut v = Vec::new();
                        for i in -7..=7 {
                            for j in -1..=1 {
                                v.push((i, j));
                                if !(-1..=1).contains(&i) {
                                    v.push((j, i));
                                }
                            }
                        }
                        (55.0, v)
                    } else {
                        (
                            35.0,
                            (-4..=4)
                                .flat_map(|i| (-4..=4).map(move |j| (i, j)))
                                .collect(),
                        )
                    };
                    for (i, j) in offsets {
                        let at = pos + Vec2::new(i as f32, j as f32) * 10.0;
                        if solid(at) {
                            continue;
                        }
                        let d = Vec2::new(i as f32, j as f32).length() * 10.0;
                        let (sprite, len) = match (d <= 10.0, d <= kill, green) {
                            (true, _, false) => ("ExplosionCore", CORE),
                            (true, _, true) => ("ExplosionCoreGreen", CORE),
                            (false, true, false) => ("ExplosionKill", KILL),
                            (false, true, true) => ("ExplosionKillGreen", KILL),
                            (false, false, false) => ("ExplosionPush", PUSH),
                            (false, false, true) => ("ExplosionPushGreen", PUSH),
                        };
                        e.cells.push(Cell {
                            sprite,
                            pos: Vec2::new(at.x.rem_euclid(w), at.y),
                            delay: 1 + (i.unsigned_abs() + j.unsigned_abs()),
                            len,
                            fade: 0.0,
                        });
                    }
                }
                Pending::Debris {
                    kind,
                    pos,
                    count,
                    range,
                    dir,
                } => {
                    let a0 = self.rand() * 360.0;
                    for k in 0..count {
                        let r = self.rand();
                        let (speed, xb, yb, life, size, color, stone) = match kind {
                            DebrisKind::Bomb => (
                                3.0,
                                -0.85 - 0.1 * r,
                                -0.45 - 0.1 * r,
                                120,
                                2.0,
                                0xD80000,
                                false,
                            ),
                            DebrisKind::GreenBomb => (
                                3.0,
                                -0.85 - 0.1 * r,
                                -0.45 - 0.1 * r,
                                120,
                                2.0,
                                0x7CF800,
                                false,
                            ),
                            DebrisKind::CrackedWall => (
                                3.0,
                                -0.4 - 0.4 * r,
                                -0.15 - 0.1 * r,
                                80,
                                4.0,
                                0xFFFFFF,
                                true,
                            ),
                            DebrisKind::CrumbleBlock => (
                                3.0,
                                -0.4 - 0.4 * r,
                                -0.15 - 0.1 * r,
                                80,
                                4.0,
                                0xB9DFFE,
                                true,
                            ),
                            DebrisKind::Icicle => {
                                let sz = 2.0 + (self.rand() * 4.0).floor();
                                (
                                    2.0,
                                    -0.6 - 0.1 * r,
                                    -0.3 - 0.1 * r,
                                    120,
                                    sz,
                                    0x88DFF6,
                                    false,
                                )
                            }
                        };
                        let mut color = Color::hex(color);
                        if kind == DebrisKind::CrackedWall
                            && let Some(c) = self.crack_color
                        {
                            color = c;
                        }
                        // Bomb debris: evenly around (60 deg apart from a
                        // random start); the rest around `dir`.
                        let a = if matches!(kind, DebrisKind::Bomb | DebrisKind::GreenBomb) {
                            (a0 + 360.0 / count.max(1) as f32 * k as f32).to_radians()
                        } else {
                            dir + (self.rand() - 0.5) * std::f32::consts::FRAC_PI_2
                        };
                        let at = pos
                            + Vec2::new(
                                (self.rand() * 2.0 - 1.0) * range.x,
                                (self.rand() * 2.0 - 1.0) * range.y,
                            );
                        let life = life + (self.rand() * 40.0) as u32;
                        e.debris.push(Debris {
                            pos: at,
                            vel: Vec2::new(a.cos(), a.sin()) * speed,
                            life,
                            t: 0,
                            xb,
                            yb,
                            size,
                            color,
                            stone,
                        });
                    }
                }
                Pending::Shock { pos, small } => e.cells.push(if small {
                    Cell {
                        sprite: "SmallShock",
                        pos,
                        delay: 0,
                        len: 38,
                        fade: 0.03,
                    }
                } else {
                    Cell {
                        sprite: "Shock",
                        pos,
                        delay: 0,
                        len: 24,
                        fade: 0.0,
                    }
                }),
            }
            self.live.push(e);
        }
        // Debris physics against the solid grid (bounce, gravity 0.15).
        for e in &mut self.live {
            for d in &mut e.debris {
                d.vel.y = (d.vel.y + 0.15).min(4.0);
                let nx = d.pos + Vec2::new(d.vel.x, 0.0);
                if solid(nx) {
                    d.vel.x *= d.xb;
                } else {
                    d.pos = nx;
                }
                let ny = d.pos + Vec2::new(0.0, d.vel.y);
                if solid(ny) {
                    d.vel.y = if d.vel.y > 0.0 && d.vel.y < 0.8 {
                        0.0
                    } else {
                        d.vel.y * d.yb
                    };
                } else {
                    d.pos = ny;
                }
                // Resting friction.
                if d.vel.y == 0.0 && solid(d.pos + Vec2::new(0.0, 1.0)) {
                    d.vel.x = if d.vel.x.abs() <= 1.0 {
                        0.0
                    } else {
                        d.vel.x - d.vel.x.signum()
                    };
                }
                d.pos.x = d.pos.x.rem_euclid(w);
            }
        }
    }

    /// One frame.
    pub fn update(&mut self) {
        for e in &mut self.live {
            e.t += 1;
            for d in &mut e.debris {
                d.t += 1;
            }
            e.debris.retain(|d| d.t < d.life + 120);
        }
        self.live
            .retain(|e| !e.debris.is_empty() || e.cells.iter().any(|c| e.t < c.delay + c.len));
    }

    pub fn draw(&self, dl: &mut DrawList, g: &TowerGfx) {
        for e in &self.live {
            for c in &e.cells {
                if e.t < c.delay || e.t >= c.delay + c.len {
                    continue;
                }
                let t = (e.t - c.delay) as u64;
                let alpha = (1.0 - c.fade * t as f32).max(0.0);
                if let Some((s, origin, _)) = g.sprite(SpriteSet::Main, c.sprite, "0", 0, t) {
                    dl.sprite(
                        s,
                        &SpriteParams::at(c.pos.floor())
                            .origin(origin)
                            .color(Color::WHITE * alpha),
                    );
                }
            }
            for d in &e.debris {
                let a = if d.t > d.life {
                    1.0 - (d.t - d.life) as f32 / 120.0
                } else {
                    1.0
                };
                let c = d.color * a;
                match (d.stone, g.main_sub("stoneDebris")) {
                    (true, Some(s)) => dl.sprite(
                        s,
                        &SpriteParams::at(d.pos.floor())
                            .origin(Vec2::splat(4.0))
                            .scale(Vec2::splat(0.5))
                            .color(c),
                    ),
                    _ => dl.rect(d.pos.x.floor(), d.pos.y.floor(), d.size, d.size, c),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lattice_and_lifetime() {
        let c = tfp_sim::Content::from_ascii(&[
            "##########",
            "#........#",
            "#........#",
            "#........#",
            "#........#",
            "##########",
        ]);
        let mut fx = Effects::default();
        fx.push(Pending::Explosion {
            pos: Vec2::new(50.0, 30.0),
            sup: false,
            green: false,
        });
        fx.push(Pending::Debris {
            kind: DebrisKind::Bomb,
            pos: Vec2::new(50.0, 30.0),
            count: 6,
            range: Vec2::ZERO,
            dir: 0.0,
        });
        fx.resolve(c.level(0));
        let e = &fx.live[0];
        assert!(
            !e.cells.is_empty() && e.cells.len() < 81,
            "solid cells left out"
        );
        assert_eq!(
            e.cells
                .iter()
                .filter(|c| c.sprite == "ExplosionCore")
                .count(),
            5
        );
        assert_eq!(fx.live[1].debris.len(), 6);
        for _ in 0..400 {
            fx.update();
            fx.resolve(c.level(0));
        }
        assert!(fx.live.is_empty(), "all gone after the debris fade");
    }
}
