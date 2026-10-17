//! Colliders and collision tests (engine.md §6, §9.2).
//!
//! The *receiver* (the querying entity's collider) decides the semantics:
//! a plain `Hitbox` tests one box, a wrapping hitbox also tests its copies
//! shifted by the world size; the level grid tests the cells a rectangle
//! covers. Integer truncation points follow the original exactly.

use crate::checksum::{Hasher, StableHash};
use crate::content::LevelDef;
use crate::impl_hash;
use crate::math::{Vec2, closest_point_on_line};

/// `Hitbox(width, height, x, y)`; `wrap` = `WrapHitbox`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hitbox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub wrap: bool,
}
impl_hash!(Hitbox { x, y, w, h, wrap });

impl Hitbox {
    pub const fn new(w: f32, h: f32, x: f32, y: f32) -> Hitbox {
        Hitbox {
            x,
            y,
            w,
            h,
            wrap: false,
        }
    }
    pub const fn wrapping(w: f32, h: f32, x: f32, y: f32) -> Hitbox {
        Hitbox {
            x,
            y,
            w,
            h,
            wrap: true,
        }
    }
    #[inline]
    pub fn abs(&self, pos: Vec2) -> Abs {
        let l = pos.x + self.x;
        let t = pos.y + self.y;
        Abs {
            l,
            t,
            r: l + self.w,
            b: t + self.h,
        }
    }
}

/// An entity's collider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collider {
    None,
    Box(Hitbox),
    /// The level's solid grid (`LevelTiles`), 10x10 cells at the origin;
    /// `w x h` is its size in pixels.
    Grid {
        w: f32,
        h: f32,
    },
}

impl StableHash for Collider {
    fn stable_hash(&self, h: &mut Hasher) {
        match self {
            Collider::None => h.u8(0),
            Collider::Box(b) => {
                h.u8(1);
                b.stable_hash(h)
            }
            Collider::Grid { .. } => h.u8(2),
        }
    }
}

/// Absolute float edges of a box: `[l, r) x [t, b)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abs {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
}

impl Abs {
    /// `Collider.Bounds`: each of left, top, width, height truncated.
    #[inline]
    pub fn bounds(&self) -> IRect {
        IRect {
            x: self.l as i32,
            y: self.t as i32,
            w: (self.r - self.l) as i32,
            h: (self.b - self.t) as i32,
        }
    }
    #[inline]
    pub fn overlaps(&self, o: &Abs) -> bool {
        self.l < o.r && self.r > o.l && self.t < o.b && self.b > o.t
    }
    /// Hitbox vs Rectangle (float edges vs int rect).
    #[inline]
    pub fn overlaps_rect(&self, r: &IRect) -> bool {
        self.r > r.x as f32
            && self.b > r.y as f32
            && self.l < r.right() as f32
            && self.t < r.bottom() as f32
    }
    #[inline]
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.l && p.x < self.r && p.y >= self.t && p.y < self.b
    }
}

/// XNA `Rectangle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    #[inline]
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    #[inline]
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    #[inline]
    pub fn intersects(&self, o: &IRect) -> bool {
        o.x < self.right() && self.x < o.right() && o.y < self.bottom() && self.y < o.bottom()
    }
    #[inline]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        self.x <= px && px < self.right() && self.y <= py && py < self.bottom()
    }
    #[inline]
    pub fn shifted(&self, dx: i32, dy: i32) -> IRect {
        IRect {
            x: self.x + dx,
            y: self.y + dy,
            ..*self
        }
    }
}

/// World size for wrap copies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldSize {
    pub w: f32,
    pub h: f32,
}

/// `WrapHitbox.BuildHitList`: the box plus its wrapped copies (≤ 7).
pub fn wrap_rects(a: &Abs, ws: WorldSize) -> ([IRect; 7], usize) {
    let b = a.bounds();
    let mut out = [IRect::default(); 7];
    out[0] = b;
    let mut n = 1;
    let (wi, hi) = (ws.w as i32, ws.h as i32);
    let mut sx = false;
    let mut sy = false;
    if a.l < 0.0 {
        out[n] = b.shifted(wi, 0);
        n += 1;
        sx = true;
    } else if a.r > ws.w {
        out[n] = b.shifted(-wi, 0);
        n += 1;
        sx = true;
    }
    if a.t < 0.0 {
        out[n] = b.shifted(0, hi);
        n += 1;
        sy = true;
    } else if a.b > ws.h {
        out[n] = b.shifted(0, -hi);
        n += 1;
        sy = true;
    }
    if sx && sy {
        for i in [-1, 1] {
            for j in [-1, 1] {
                if n < 7 {
                    out[n] = b.shifted(i * wi, j * hi);
                    n += 1;
                }
            }
        }
    }
    (out, n)
}

// ---------------------------------------------------------------- the grid

/// Grid vs rectangle (engine.md §6.4).
pub fn grid_rect(level: &LevelDef, r: &IRect) -> bool {
    if r.w <= 0 || r.h <= 0 {
        return false;
    }
    let gw = level.cells_x as i32 * 10;
    let gh = level.cells_y as i32 * 10;
    let gb = IRect {
        x: 0,
        y: 0,
        w: gw,
        h: gh,
    };
    if !r.intersects(&gb) {
        return false;
    }
    let mut x0 = ((r.x as f32) / 10.0) as i32;
    let mut y0 = ((r.y as f32) / 10.0) as i32;
    let mut w = ((r.right() as f32 - 1.0) / 10.0) as i32 - x0 + 1;
    let mut h = ((r.bottom() as f32 - 1.0) / 10.0) as i32 - y0 + 1;
    if x0 < 0 {
        w += x0;
        x0 = 0;
    }
    if y0 < 0 {
        h += y0;
        y0 = 0;
    }
    if x0 + w > level.cells_x as i32 {
        w = level.cells_x as i32 - x0;
    }
    if y0 + h > level.cells_y as i32 {
        h = level.cells_y as i32 - y0;
    }
    for x in x0..x0 + w {
        for y in y0..y0 + h {
            if level.solids[y as usize * level.cells_x + x as usize] {
                return true;
            }
        }
    }
    false
}

/// Grid vs point.
pub fn grid_point(level: &LevelDef, p: Vec2) -> bool {
    let gw = level.cells_x as f32 * 10.0;
    let gh = level.cells_y as f32 * 10.0;
    if !(p.x >= 0.0 && p.x < gw && p.y >= 0.0 && p.y < gh) {
        return false;
    }
    level.solid((p.x / 10.0) as i32, (p.y / 10.0) as i32)
}

/// Grid vs segment (cell-space DDA).
pub fn grid_segment(level: &LevelDef, from: Vec2, to: Vec2) -> bool {
    let (mut x1, mut y1) = (from.x / 10.0, from.y / 10.0);
    let (mut x2, mut y2) = (to.x / 10.0, to.y / 10.0);
    let steep = (y2 - y1).abs() > (x2 - x1).abs();
    if steep {
        std::mem::swap(&mut x1, &mut y1);
        std::mem::swap(&mut x2, &mut y2);
    }
    if x1 > x2 {
        std::mem::swap(&mut x1, &mut x2);
        std::mem::swap(&mut y1, &mut y2);
    }
    let slope = (y2 - y1).abs() / (x2 - x1);
    let step = if y1 < y2 { 1 } else { -1 };
    let mut row = y1 as i32;
    let mut acc = 0.0f32;
    let c0 = x1 as i32;
    let c1 = x2 as i32;
    for col in c0..=c1 {
        let hit = if steep {
            level.solid(row, col)
        } else {
            level.solid(col, row)
        };
        if hit {
            return true;
        }
        acc += slope;
        if acc >= 0.5 {
            row += step;
            acc -= 1.0;
        }
    }
    false
}

// --------------------------------------------------------------- segments

const S_TOP: u8 = 1;
const S_BOTTOM: u8 = 2;
const S_RIGHT: u8 = 4;
const S_LEFT: u8 = 8;

fn sector(a: &Abs, p: Vec2) -> u8 {
    let mut s = 0;
    if p.x < a.l {
        s |= S_LEFT;
    } else if p.x >= a.r {
        s |= S_RIGHT;
    }
    if p.y < a.t {
        s |= S_TOP;
    } else if p.y >= a.b {
        s |= S_BOTTOM;
    }
    s
}

/// `Collide.LineCheck(a1, a2, b1, b2)`: segment vs segment, inclusive.
pub fn segments_intersect(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> bool {
    let d = a2 - a1;
    let e = b2 - b1;
    let den = d.x * e.y - d.y * e.x;
    if den == 0.0 {
        return false;
    }
    let w = b1 - a1;
    let t = (w.x * e.y - w.y * e.x) / den;
    if !(0.0..=1.0).contains(&t) {
        return false;
    }
    let u = (w.x * d.y - w.y * d.x) / den;
    (0.0..=1.0).contains(&u)
}

/// Hitbox vs segment (sector test, engine.md §6.4).
pub fn box_segment(a: &Abs, from: Vec2, to: Vec2) -> bool {
    let sa = sector(a, from);
    let sb = sector(a, to);
    if sa == 0 || sb == 0 {
        return true;
    }
    if sa & sb != 0 {
        return false;
    }
    let u = sa | sb;
    let tl = Vec2::new(a.l, a.t);
    let tr = Vec2::new(a.r, a.t);
    let bl = Vec2::new(a.l, a.b);
    let br = Vec2::new(a.r, a.b);
    (u & S_TOP != 0 && segments_intersect(tl, tr, from, to))
        || (u & S_BOTTOM != 0 && segments_intersect(bl, br, from, to))
        || (u & S_LEFT != 0 && segments_intersect(tl, bl, from, to))
        || (u & S_RIGHT != 0 && segments_intersect(tr, br, from, to))
}

fn rect_abs(r: &IRect) -> Abs {
    Abs {
        l: r.x as f32,
        t: r.y as f32,
        r: r.right() as f32,
        b: r.bottom() as f32,
    }
}

/// Circle vs segment.
pub fn circle_segment(c: Vec2, r: f32, a: Vec2, b: Vec2) -> bool {
    c.dist_sq(closest_point_on_line(a, b, c)) < r * r
}

// --------------------------------------------------------------- dispatch

/// A positioned collider for a test.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub pos: Vec2,
    pub collider: Collider,
}

/// `receiver.Collide(argument)` following the dispatch matrix of
/// engine.md §6.3.
pub fn collide(recv: &Placed, arg: &Placed, level: &LevelDef, ws: WorldSize) -> bool {
    match (recv.collider, arg.collider) {
        (Collider::None, _) | (_, Collider::None) => false,
        (Collider::Box(a), Collider::Box(b)) => {
            let aa = a.abs(recv.pos);
            let ba = b.abs(arg.pos);
            if !a.wrap {
                aa.overlaps(&ba)
            } else {
                let (ra, na) = wrap_rects(&aa, ws);
                if !b.wrap {
                    ra[..na].iter().any(|r| ba.overlaps_rect(r))
                } else {
                    let (rb, nb) = wrap_rects(&ba, ws);
                    ra[..na]
                        .iter()
                        .any(|r| rb[..nb].iter().any(|o| r.intersects(o)))
                }
            }
        }
        (Collider::Box(a), Collider::Grid { .. }) => {
            let aa = a.abs(recv.pos);
            if !a.wrap {
                grid_rect(level, &aa.bounds())
            } else {
                let (ra, na) = wrap_rects(&aa, ws);
                ra[..na].iter().any(|r| grid_rect(level, r))
            }
        }
        (Collider::Grid { .. }, Collider::Box(b)) => grid_rect(level, &b.abs(arg.pos).bounds()),
        (Collider::Grid { .. }, Collider::Grid { .. }) => false,
    }
}

/// `collider.Collide(point)`.
pub fn collide_point(recv: &Placed, p: Vec2, level: &LevelDef, ws: WorldSize) -> bool {
    match recv.collider {
        Collider::None => false,
        Collider::Grid { .. } => grid_point(level, p),
        Collider::Box(a) => {
            let aa = a.abs(recv.pos);
            if !a.wrap {
                aa.contains(p)
            } else {
                let (ra, na) = wrap_rects(&aa, ws);
                ra[..na].iter().any(|r| r.contains(p.x as i32, p.y as i32))
            }
        }
    }
}

/// `collider.Collide(from, to)`.
pub fn collide_segment(
    recv: &Placed,
    from: Vec2,
    to: Vec2,
    level: &LevelDef,
    ws: WorldSize,
) -> bool {
    match recv.collider {
        Collider::None => false,
        Collider::Grid { .. } => grid_segment(level, from, to),
        Collider::Box(a) => {
            let aa = a.abs(recv.pos);
            if !a.wrap {
                box_segment(&aa, from, to)
            } else {
                let (ra, na) = wrap_rects(&aa, ws);
                ra[..na].iter().any(|r| box_segment(&rect_abs(r), from, to))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level() -> LevelDef {
        LevelDef::from_ascii(
            "t",
            &[
                "#..#", //
                "....", //
                "....", //
                "#...", //
            ],
        )
    }
    const WS: WorldSize = WorldSize { w: 40.0, h: 40.0 };

    #[test]
    fn edge_adjacent_boxes_do_not_collide() {
        let a = Placed {
            pos: Vec2::new(0.0, 0.0),
            collider: Collider::Box(Hitbox::new(10.0, 10.0, 0.0, 0.0)),
        };
        let b = Placed {
            pos: Vec2::new(10.0, 0.0),
            collider: Collider::Box(Hitbox::new(10.0, 10.0, 0.0, 0.0)),
        };
        assert!(!collide(&a, &b, &level(), WS));
        let b = Placed {
            pos: Vec2::new(9.5, 0.0),
            ..b
        };
        assert!(collide(&a, &b, &level(), WS));
    }

    #[test]
    fn grid_rect_negative_and_edges() {
        let l = level();
        // Cell (0,0) solid: a rect at x=-3..5 touches it.
        assert!(grid_rect(
            &l,
            &IRect {
                x: -3,
                y: 2,
                w: 8,
                h: 3
            }
        ));
        // Entirely left of the grid: not solid (the grid does not wrap).
        assert!(!grid_rect(
            &l,
            &IRect {
                x: -13,
                y: 2,
                w: 8,
                h: 3
            }
        ));
        // Right edge cell (3,0) solid; [30,40) x [0,10)
        assert!(grid_rect(
            &l,
            &IRect {
                x: 30,
                y: 0,
                w: 1,
                h: 1
            }
        ));
        assert!(!grid_rect(
            &l,
            &IRect {
                x: 20,
                y: 0,
                w: 10,
                h: 10
            }
        ));
        // Zero size never matches.
        assert!(!grid_rect(
            &l,
            &IRect {
                x: 0,
                y: 0,
                w: 0,
                h: 5
            }
        ));
    }

    #[test]
    fn wrap_hitbox_sees_tiles_across_the_edge() {
        let l = level();
        // A box straddling the right edge at the top: its x-1 copy overlaps cell (0,0).
        let a = Placed {
            pos: Vec2::new(38.0, 12.0),
            collider: Collider::Box(Hitbox::wrapping(4.0, 4.0, 0.0, 0.0)),
        };
        let g = Placed {
            pos: Vec2::ZERO,
            collider: Collider::Grid { w: 40.0, h: 40.0 },
        };
        assert!(!collide(&a, &g, &l, WS)); // row 1 is empty everywhere
        let a = Placed {
            pos: Vec2::new(38.0, 32.0),
            ..a
        };
        // copy at x=-2..2 overlaps cell (0,3) which is solid
        assert!(collide(&a, &g, &l, WS));
        let plain = Placed {
            collider: Collider::Box(Hitbox::new(4.0, 4.0, 0.0, 0.0)),
            ..a
        };
        assert!(!collide(&plain, &g, &l, WS));
    }

    #[test]
    fn wrap_diagonal_copies() {
        let ab = Abs {
            l: -2.0,
            t: -2.0,
            r: 2.0,
            b: 2.0,
        };
        let (r, n) = wrap_rects(&ab, WS);
        assert_eq!(n, 7);
        assert!(r[..n].iter().any(|r| r.x == 38 && r.y == 38));
    }

    #[test]
    fn segments() {
        let l = level();
        assert!(grid_segment(&l, Vec2::new(15.0, 5.0), Vec2::new(35.0, 5.0)));
        assert!(!grid_segment(
            &l,
            Vec2::new(5.0, 15.0),
            Vec2::new(35.0, 25.0)
        ));
        let a = Abs {
            l: 0.0,
            t: 0.0,
            r: 10.0,
            b: 10.0,
        };
        assert!(box_segment(&a, Vec2::new(-5.0, 5.0), Vec2::new(15.0, 5.0)));
        assert!(!box_segment(
            &a,
            Vec2::new(-5.0, -5.0),
            Vec2::new(15.0, -5.0)
        ));
        assert!(box_segment(&a, Vec2::new(-5.0, 4.0), Vec2::new(4.0, -3.0)));
        assert!(!box_segment(&a, Vec2::new(-5.0, 4.0), Vec2::new(4.0, -5.0)));
    }
}
