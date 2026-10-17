//! Procedural level variation for towers flagged `<procedural/>` (Versus
//! Cataclysm): new geometry inside the level's `RandomBlock` areas, extra
//! hazards/platforms, randomised treasure spots, a generated BG wall layer and
//! BG detail tiles, all from one seed (`MatchSettings.RandomLevelSeed` in the
//! original, shared by every peer). Behaviour: `docs/spec/levels.md` §13.
//!
//! Everything is mirror-symmetric about the centre column; the order of PRNG
//! draws is part of the behaviour, so each stage below keeps the original's
//! scan, shuffle and draw order. The 8-player build runs the same code with
//! W = 42 but keeps a few 32-wide constants, reproduced via [`Dims`].

use crate::dotnet_random::DotNetRandom;
use crate::level::{Entity, Level};
use crate::xml::{BitGrid, TileGrid};
use std::collections::BTreeMap;

const H: i32 = 24;

/// Grid width and the constants that depend on it (and the ones the
/// 8-player build forgot to widen).
#[derive(Clone, Copy, Debug)]
pub struct Dims {
    pub w: i32,
    pub half: i32,
    /// The "thin" test of bottom-wrap blocks (16 in both builds).
    legacy_half: i32,
    /// Column skipped by the lantern and vent-block scans (15 in both).
    legacy_skip: i32,
    /// Right bound of the item stage's solid-collision box test (32 in both).
    legacy_w: i32,
}

impl Dims {
    pub fn for_width_cells(w: i32) -> Dims {
        Dims {
            w,
            half: w / 2,
            legacy_half: 16,
            legacy_skip: 15,
            legacy_w: 32,
        }
    }
    #[inline]
    fn m(&self, x: i32) -> i32 {
        self.w - 1 - x
    }
}

/// Cell rectangle (x, y, w, h) with XNA-style edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct R {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl R {
    fn new(x: i32, y: i32, w: i32, h: i32) -> R {
        R { x, y, w, h }
    }
    fn right(&self) -> i32 {
        self.x + self.w
    }
    fn bottom(&self) -> i32 {
        self.y + self.h
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        self.x <= x && x < self.right() && self.y <= y && y < self.bottom()
    }
    fn intersects(&self, o: &R) -> bool {
        o.x < self.right() && self.x < o.right() && o.y < self.bottom() && self.y < o.bottom()
    }
}

/// Bool grid with safe access: reads outside are `false`, writes ignored.
#[derive(Clone, PartialEq, Eq)]
struct G {
    w: i32,
    c: Vec<bool>,
}

impl G {
    fn new(w: i32) -> G {
        G {
            w,
            c: vec![false; (w * H) as usize],
        }
    }
    fn from_bits(b: &BitGrid) -> G {
        G {
            w: b.width as i32,
            c: b.cells.clone(),
        }
    }
    fn to_bits(&self) -> BitGrid {
        BitGrid {
            width: self.w as usize,
            height: H as usize,
            cells: self.c.clone(),
        }
    }
    #[inline]
    fn get(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < H && self.c[(y * self.w + x) as usize]
    }
    #[inline]
    fn put(&mut self, x: i32, y: i32, v: bool) {
        if x >= 0 && y >= 0 && x < self.w && y < H {
            self.c[(y * self.w + x) as usize] = v;
        }
    }
    fn set(&mut self, x: i32, y: i32, w: i32, h: i32) {
        for i in x..x + w {
            for j in y..y + h {
                self.put(i, j, true);
            }
        }
    }
    /// All cells of the rectangle set (empty rectangle: true).
    fn all(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        (x..x + w).all(|i| (y..y + h).all(|j| self.get(i, j)))
    }
    /// Every in-grid cell equals `v` (cells outside are skipped).
    fn check(&self, x: i32, y: i32, w: i32, h: i32, v: bool) -> bool {
        for i in x.max(0)..(x + w).min(self.w) {
            for j in y.max(0)..(y + h).min(H) {
                if self.get(i, j) != v {
                    return false;
                }
            }
        }
        true
    }
}

/// Tile-override grid (`-1` = none), safe access.
struct T {
    w: i32,
    c: Vec<i32>,
}

impl T {
    fn new(w: i32) -> T {
        T {
            w,
            c: vec![-1; (w * H) as usize],
        }
    }
    fn get(&self, x: i32, y: i32) -> i32 {
        if x >= 0 && y >= 0 && x < self.w && y < H {
            self.c[(y * self.w + x) as usize]
        } else {
            -1
        }
    }
    fn put(&mut self, x: i32, y: i32, v: i32) {
        if x >= 0 && y >= 0 && x < self.w && y < H {
            self.c[(y * self.w + x) as usize] = v;
        }
    }
    /// All `-1` over `[x,x+w) ∩ [0,W-1)` × `[y,y+h) ∩ [0,23)` — the last
    /// column and row are not examined (as in the original).
    fn empty(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        for i in x.max(0)..(x + w).min(self.w - 1) {
            for j in y.max(0)..(y + h).min(H - 1) {
                if self.get(i, j) != -1 {
                    return false;
                }
            }
        }
        true
    }
}

/// `k = 0; while Chance(a·b^k) k++` (double overload; `a`, `b` are f32
/// constants widened).
fn count_geo(rng: &mut DotNetRandom, a: f32, b: f32) -> i32 {
    let mut k = 0;
    while rng.chance_f64(a as f64 * (b as f64).powf(k as f64)) {
        k += 1;
    }
    k
}

/// `k = 1; while Chance(b^k) k++`.
fn count_geo1(rng: &mut DotNetRandom, b: f32) -> i32 {
    let mut k = 1;
    while rng.chance_f64((b as f64).powf(k as f64)) {
        k += 1;
    }
    k
}

// ------------------------------------------------------------- geometry

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Style {
    HWrap,
    BottomVWrap,
    TopVWrap,
    Single,
    LeftPocket,
    RightPocket,
    LeftWall,
    RightWall,
    Ceiling,
    Floor,
    Floating,
}

fn generate_geometry(d: Dims, rng: &mut DotNetRandom, rects: &[R], base: &G) -> G {
    let mut out = G::new(d.w);
    let mut by: BTreeMap<Style, Vec<R>> = BTreeMap::new();
    for r in rects {
        let style = if r.x == 0 {
            Style::HWrap
        } else if r.y == 0 {
            Style::TopVWrap
        } else if r.bottom() == H {
            Style::BottomVWrap
        } else if r.w == 1 && r.h == 1 {
            Style::Single
        } else {
            let l = base.all(r.x - 1, r.y, 1, r.h);
            let rr = base.all(r.right(), r.y, 1, r.h);
            let t = base.all(r.x, r.y - 1, r.w, 1);
            let b = base.all(r.x, r.bottom(), r.w, 1);
            if l && t && b {
                Style::LeftPocket
            } else if rr && t && b {
                Style::RightPocket
            } else if b {
                Style::Floor
            } else if l {
                Style::LeftWall
            } else if rr {
                Style::RightWall
            } else if t {
                Style::Ceiling
            } else {
                Style::Floating
            }
        };
        by.entry(style).or_default().push(*r);
    }
    let list = |s: Style| by.get(&s).cloned().unwrap_or_default();

    for r in list(Style::HWrap) {
        let f = rng.chance(0.2);
        let mut n = 1 + rng.next_max(r.w);
        let mut m = rng.next_max(r.h + 1);
        if m == r.h - 1 && !f {
            m += rng.choose(&[-1, 1]);
        }
        let y0 = if m == r.h - 1 {
            if n > 0 {
                n = rng.choose(&[1, 1, 1, r.w.min(2)]);
            }
            r.y
        } else {
            rng.choose(&[r.y, r.bottom() - m])
        };
        out.set(0, y0, n, m);
    }
    for r in list(Style::BottomVWrap) {
        let thin = r.w == 1
            || (r.x > 0 && !base.get(r.x - 1, 23))
            || (r.right() < d.legacy_half && !base.get(r.right(), 23));
        let mut n = rng.next_max(r.w + 1);
        if !thin && n == r.w - 1 {
            n += rng.choose(&[1, -1]);
        }
        let m = 1 + rng.next_max(r.h);
        let x0 = rng.choose(&[r.x, r.right() - n]);
        out.set(x0, H - m, n, m);
        out.set(x0, 0, n, 1);
    }
    for r in list(Style::TopVWrap) {
        let Some(x0) = (r.x..r.right()).find(|&i| out.get(i, 23)) else {
            continue;
        };
        let mut n = 1;
        while out.get(x0 + n, 23) && n < r.w - (x0 - r.x) {
            n += 1;
        }
        let m = 1 + rng.next_max(r.h);
        out.set(x0, 0, n, m);
        out.set(x0, 23, n, 1);
    }
    for r in list(Style::Single) {
        if rng.choose(&[true, false]) {
            out.set(r.x, r.y, 1, 1);
        }
    }
    for (right, rs) in [
        (false, list(Style::LeftPocket)),
        (true, list(Style::RightPocket)),
    ] {
        for r in rs {
            let n = rng.next_max(r.w + 1);
            let x = if right { r.right() - n } else { r.x };
            if n <= 0 {
                continue;
            }
            let mut t = rng.next_max(r.h - 2);
            if t > 0 {
                t += 2;
            }
            let mut s = rng.next_max(r.h - t);
            let dd = if t > 0 {
                -n + 1 + rng.next_max(r.w)
            } else {
                let v = -1 + rng.next_max(3.min(r.w - n + 1));
                if s == r.h - 1 {
                    s -= 1;
                }
                v
            };
            for i in 0..r.h {
                if i < s {
                    out.set(x, r.y + i, n, 1);
                } else if i >= s + t {
                    let x1 = if right { x - dd } else { x };
                    out.set(x1, r.y + i, n + dd, 1);
                }
            }
        }
    }
    for (right, rs) in [
        (false, list(Style::LeftWall)),
        (true, list(Style::RightWall)),
    ] {
        for r in rs {
            let mut c = rng.next_max(r.w + 1);
            let mut run = 0;
            for i in 0..r.h {
                let x = if right { r.right() - c } else { r.x };
                out.set(x, r.y + i, c, 1);
                run += 1;
                let test = !right || run >= 2;
                if test && !rng.chance_f64(0.9f64.powf(run as f64)) {
                    let c2 = rng.next_max(r.w + 1);
                    if c2 != c {
                        c = c2;
                        run = 0;
                    }
                }
            }
        }
    }
    for (floor, rs) in [(false, list(Style::Ceiling)), (true, list(Style::Floor))] {
        let base_p: f64 = if floor { 0.85 } else { 0.9 };
        for r in rs {
            let mut c = rng.next_max(r.h + 1);
            let mut run = 0;
            for i in 0..r.w {
                let y = if floor { r.bottom() - c } else { r.y };
                out.set(r.x + i, y, 1, c);
                run += 1;
                if !rng.chance_f64(base_p.powf(run as f64)) {
                    let c2 = rng.next_max(r.h + 1);
                    if c2 != c {
                        c = c2;
                        run = 0;
                    }
                }
            }
        }
    }
    let mut floating = list(Style::Floating);
    rng.shuffle(&mut floating);
    while floating.len() >= 5 {
        floating.remove(0);
    }
    for r in floating {
        let k = (r.w * r.h / 4).max(1) + rng.choose(&[0, 1]);
        for _ in 0..k {
            let n = rng.next_max(r.w + 1);
            let m = rng.next_max(r.h + 1);
            let x0 = r.x + rng.next_max(r.w - n);
            let y0 = r.y + rng.next_max(r.h - m);
            out.set(x0, y0, n, m);
        }
    }

    // Mirror the left half, add the base, then four hole-filling passes.
    for x in 0..d.half {
        for y in 0..H {
            if out.get(x, y) {
                out.put(d.m(x), y, true);
            }
        }
    }
    for x in 0..d.w {
        for y in 0..H {
            if base.get(x, y) {
                out.put(x, y, true);
            }
        }
    }
    for y in 1..H - 1 {
        for x in 1..d.w - 1 {
            if !out.get(x, y) && out.get(x - 1, y) && out.get(x, y - 1) && out.get(x, y + 1) {
                out.put(x, y, true);
            }
        }
    }
    for y in 1..H - 1 {
        for x in (1..d.w - 1).rev() {
            if !out.get(x, y) && out.get(x + 1, y) && out.get(x, y - 1) && out.get(x, y + 1) {
                out.put(x, y, true);
            }
        }
    }
    for x in 1..d.w - 1 {
        for y in 1..H - 1 {
            if !out.get(x, y) && out.get(x, y - 1) && out.get(x - 1, y) && out.get(x + 1, y) {
                out.put(x, y, true);
            }
        }
    }
    for x in 1..d.w - 1 {
        for y in (1..H - 1).rev() {
            if !out.get(x, y) && out.get(x, y + 1) && out.get(x - 1, y) && out.get(x + 1, y) {
                out.put(x, y, true);
            }
        }
    }
    out
}

// ------------------------------------------------------------- items

fn ent(name: &str, attrs: &[(&str, String)], nodes: Vec<(f32, f32)>) -> Entity {
    let get = |k: &str| {
        attrs
            .iter()
            .find(|(a, _)| *a == k)
            .and_then(|(_, v)| v.parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    Entity {
        name: name.to_owned(),
        id: None,
        x: get("x"),
        y: get("y"),
        attrs: attrs
            .iter()
            .filter(|(k, _)| *k != "x" && *k != "y")
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
        nodes,
    }
}

fn xy(x: i32, y: i32) -> [(&'static str, String); 2] {
    [("x", x.to_string()), ("y", y.to_string())]
}

fn with_w(x: i32, y: i32, w: i32) -> [(&'static str, String); 3] {
    [
        ("x", x.to_string()),
        ("y", y.to_string()),
        ("width", w.to_string()),
    ]
}

/// Items stage: returns `(BaseSolids, MovingPlatforms)`; carves `solids`
/// and appends entities.
fn add_items(
    d: Dims,
    rng: &mut DotNetRandom,
    solids: &mut G,
    out: &mut Vec<Entity>,
) -> (G, Vec<R>) {
    let base = solids.clone();
    let (w, half) = (d.w, d.half);
    let collide_grid = |g: &G, r: R| -> bool {
        for i in r.x..r.right() {
            for j in r.y..r.bottom() {
                if i >= 0 && j >= 0 && i < d.legacy_w && j < H && g.get(i, j) {
                    return true;
                }
            }
        }
        false
    };
    let mut orb_blockers: Vec<R> = Vec::new();

    // (1) Exploding orbs.
    let mut k = count_geo(rng, 0.2, 0.9);
    if k > 0 {
        let mut cand = Vec::new();
        for j in 2..=half {
            for i in 2..=23 {
                if solids.get(j, i)
                    || solids.get(j - 1, i)
                    || solids.get(j - 1, i - 1)
                    || solids.get(j, i - 1)
                {
                    continue;
                }
                if (1..8).all(|l| !solids.get(j, (i + l) % H) && !solids.get(j - 1, (i + l) % H)) {
                    cand.push((j, i));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            orb_blockers.push(R::new(p.0 - 1, p.1 - 1, 2, 2));
            cand.retain(|c| (c.0 - p.0).abs() > 1);
            out.push(ent("ExplodingOrb", &xy(p.0 * 10, p.1 * 10), vec![]));
            if p.0 != half {
                out.push(ent("ExplodingOrb", &xy((w - p.0) * 10, p.1 * 10), vec![]));
            }
        }
    }

    // (2) Mud.
    let k = count_geo(rng, 0.2, 0.5);
    if k > 0 {
        let mut runs: Vec<R> = Vec::new();
        for i in 2..23 {
            let mut cur: Option<R> = None;
            for j in 1..half {
                match cur {
                    None => {
                        if !solids.get(j, i) && solids.get(j - 1, i) && solids.get(j, i + 1) {
                            cur = Some(R::new(j, i, 1, 1));
                        }
                    }
                    Some(mut c) => {
                        if solids.get(j, i) && solids.get(j, i + 1) {
                            runs.push(c);
                            cur = None;
                        } else if solids.get(j, i + 1) {
                            c.w += 1;
                            cur = Some(c);
                        } else {
                            cur = None;
                        }
                    }
                }
            }
            if let Some(c) = cur {
                runs.push(c);
            }
        }
        runs.retain(|r| if r.right() == half { r.w > 1 } else { r.w > 2 });
        rng.shuffle(&mut runs);
        for r in runs.iter().take(k as usize) {
            if r.right() == half {
                out.push(ent(
                    "Mud",
                    &with_w(r.x * 10, r.y * 10 + 5, r.w * 20),
                    vec![],
                ));
            } else {
                out.push(ent(
                    "Mud",
                    &with_w(r.x * 10, r.y * 10 + 5, r.w * 10),
                    vec![],
                ));
                out.push(ent(
                    "Mud",
                    &with_w((w - r.x - r.w) * 10, r.y * 10 + 5, r.w * 10),
                    vec![],
                ));
            }
        }
    }

    // (3) Jump pads.
    let k = count_geo(rng, 0.5, 0.85);
    if k > 0 {
        let mut runs: Vec<R> = Vec::new();
        for i in 4..H {
            let mut cur: Option<R> = None;
            for j in 1..half {
                if solids.get(j, i) && !solids.get(j, i - 1) {
                    match &mut cur {
                        Some(c) => c.w += 1,
                        None => cur = Some(R::new(j, i, 1, 1)),
                    }
                } else if let Some(c) = cur.take()
                    && (c.w > 1 || c.x == half - 1)
                {
                    runs.push(c);
                }
            }
            if let Some(c) = cur.take()
                && (c.w > 1 || c.x == half - 1)
            {
                runs.push(c);
            }
        }
        let mut segs: Vec<R> = Vec::new();
        for r in &runs {
            let free: Vec<bool> = (0..r.w)
                .map(|j| (1..7).all(|k| !solids.get(r.x + j, (r.y - k + H) % H)))
                .collect();
            let mut open: Option<R> = None;
            for j in 0..=r.w {
                if j < r.w && free[j as usize] {
                    match &mut open {
                        Some(o) => o.w += 1,
                        None => open = Some(R::new(r.x + j, r.y, 1, 1)),
                    }
                } else if let Some(o) = open.take()
                    && (o.w > 1 || o.right() == half)
                {
                    segs.push(o);
                }
            }
        }
        rng.shuffle(&mut segs);
        for mut r in segs.into_iter().take(k as usize) {
            if r.right() == half {
                if r.w > 1 {
                    let ow = r.w;
                    r.w = rng.next_max(ow) + 1;
                    r.x = half - r.w;
                }
            } else if r.w > 2 {
                let ow = r.w;
                r.w = rng.next_max(ow - 1) + 2;
                r.x += rng.next_max(ow - r.w + 1);
            }
            for j in r.x..r.right() {
                solids.put(j, r.y, false);
                solids.put(d.m(j), r.y, false);
            }
            if r.right() == half {
                out.push(ent(
                    "JumpPad",
                    &with_w(r.x * 10, r.y * 10, r.w * 20),
                    vec![],
                ));
            } else {
                out.push(ent(
                    "JumpPad",
                    &with_w(r.x * 10, r.y * 10, r.w * 10),
                    vec![],
                ));
                out.push(ent(
                    "JumpPad",
                    &with_w((w - r.x - r.w) * 10, r.y * 10, r.w * 10),
                    vec![],
                ));
            }
        }
    }

    // (4) Crumble walls.
    let mut k = count_geo(rng, 0.8, 0.5);
    if k > 0 {
        let mut cand = Vec::new();
        for j in 2..=half {
            for i in 1..22 {
                let a = (j + w - 1) % w;
                let b = (j + w - 2) % w;
                let c = (j + w - 3) % w;
                if solids.get(j, i)
                    && solids.get(a, i)
                    && solids.get(j, i + 1)
                    && solids.get(a, i + 1)
                    && solids.get(j, i - 1)
                    && solids.get(a, i - 1)
                {
                    let left = !base.get(b, i)
                        && !base.get(b, i + 1)
                        && !base.get(c, i)
                        && !base.get(c, i + 1);
                    let right = !base.get(j + 1, i)
                        && !base.get(j + 1, i + 1)
                        && !base.get(j + 2, i)
                        && !base.get(j + 2, i + 1);
                    let below = !base.get(j, i + 2) && !base.get(a, i + 2);
                    if [left, right, below].iter().filter(|v| **v).count() >= 2 {
                        cand.push((j, i));
                    }
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            cand.retain(|c| !((c.0 - p.0).abs() <= 1 && (c.1 - p.1).abs() <= 1));
            // The centre test compares with 16 in both builds; in the 8-player
            // build a candidate at 16 therefore clears the true centre and
            // puts one wall at x = 150, while one at 21 takes the paired path.
            if p.0 == d.legacy_half {
                for (x, y) in [
                    (half, p.1),
                    (half, p.1 + 1),
                    (half - 1, p.1),
                    (half - 1, p.1 + 1),
                ] {
                    solids.put(x, y, false);
                }
                out.push(ent("CrumbleWall", &xy(p.0 * 10 - 10, p.1 * 10), vec![]));
            } else {
                let q = w - p.0;
                for x in [p.0, p.0 - 1, q, q - 1] {
                    solids.put(x, p.1, false);
                    solids.put(x, p.1 + 1, false);
                }
                out.push(ent("CrumbleWall", &xy(p.0 * 10 - 10, p.1 * 10), vec![]));
                out.push(ent("CrumbleWall", &xy(q * 10 - 10, p.1 * 10), vec![]));
            }
        }
    }

    // (5) Spike ball.
    let mut spike = false;
    let mut rows = Vec::new();
    for k in 6..18 {
        if !solids.get(half, k) || !solids.get(half, k - 1) {
            continue;
        }
        let bx = R::new(half - 5, k - 5, 10, 10);
        if orb_blockers.iter().any(|b| b.intersects(&bx)) {
            continue;
        }
        let mut ok = true;
        'outer: for n in 0..=5 {
            for l in -5..=4 {
                let mut must = n >= 2 || !(-2..=1).contains(&l);
                if must && n == 5 && (l == 4 || l == -5) {
                    must = false;
                }
                if must && base.get(half + n, k + l) {
                    ok = false;
                    break 'outer;
                }
            }
        }
        if ok {
            rows.push(k);
        }
    }
    if !rows.is_empty() && rng.chance(0.3) {
        spike = true;
        rng.shuffle(&mut rows);
        let i0 = rows[0];
        let mut ext = 4;
        while ext < 6 {
            let mut ok = (0..=1 + ext)
                .all(|n| !base.get(half + n, i0 - 2 - ext) && !base.get(half + n, i0 + 1 + ext));
            if ok {
                ok = (-1 - ext..=ext).all(|l| !base.get(half + 1 + ext, i0 + l));
            }
            if !ok {
                break;
            }
            ext += 1;
        }
        let explodes = if rng.choose(&[true, false]) {
            "True"
        } else {
            "False"
        };
        let cx = half * 10;
        out.push(ent(
            "SpikeBall",
            &[
                ("x", cx.to_string()),
                ("y", ((i0 - ext) * 10).to_string()),
                ("Explodes", explodes.to_owned()),
            ],
            vec![(cx as f32, (i0 * 10) as f32)],
        ));
    }

    // (6) Moving platform.
    let mut moving = Vec::new();
    if spike {
        return (base, moving);
    }
    let mut hor = Vec::new();
    let mut r = R::new(7, 8, half - 7, 6);
    while r.bottom() < 22 {
        let ex = R::new(r.x - 2, r.y - 3, r.w + 4, r.h + 3);
        if !collide_grid(&base, ex) && !orb_blockers.iter().any(|b| b.intersects(&ex)) {
            hor.push(r);
        }
        r.y += 1;
    }
    let mut ver = Vec::new();
    let mut r = R::new(half - 4, 1, 4, 10);
    while r.bottom() < 22 {
        let ex = R::new(r.x - 2, r.y, r.w + 4, r.h);
        if !collide_grid(&base, ex) && !orb_blockers.iter().any(|b| b.intersects(&ex)) {
            ver.push(r);
        }
        r.y += 1;
    }
    if (hor.is_empty() && ver.is_empty()) || !rng.chance(0.35) {
        return (base, moving);
    }
    let mut horizontal = rng.choose(&[!hor.is_empty(), ver.is_empty()]);
    if !ver.is_empty() {
        horizontal = false;
    }
    if horizontal {
        rng.shuffle(&mut hor);
        let src = hor[0];
        let pw = 3 + rng.next_max(3);
        let ph = 2 + rng.next_max(2);
        let px = src.x + rng.next_max(3);
        let py = src.y + rng.next_max(7 - ph);
        let nx = w - px - pw;
        out.push(ent(
            "MovingPlatform",
            &[
                ("x", (px * 10).to_string()),
                ("y", (py * 10).to_string()),
                ("width", (pw * 10).to_string()),
                ("height", (ph * 10).to_string()),
            ],
            vec![((nx * 10) as f32, (py * 10) as f32)],
        ));
        moving.push(R::new(px, py, nx + pw, ph));
    } else {
        rng.shuffle(&mut ver);
        let src = ver[0];
        let pw = (1 + rng.next_max(4)) * 2;
        let ph = 2 + rng.next_max(2);
        let px = half - pw / 2;
        let mut py = src.y + rng.next_max(2);
        let mut ny = src.bottom() - ph - rng.next_max(2);
        let hit = |y: i32| collide_grid(&base, R::new(px, y, pw, ph));
        if hit(py - 2) && !hit(py - 1) {
            py -= 1;
        }
        if hit(ny + 2) && !hit(ny + 1) {
            ny += 1;
        }
        if rng.choose(&[true, false]) {
            std::mem::swap(&mut py, &mut ny);
        }
        out.push(ent(
            "MovingPlatform",
            &[
                ("x", (px * 10).to_string()),
                ("y", (py * 10).to_string()),
                ("width", (pw * 10).to_string()),
                ("height", (ph * 10).to_string()),
            ],
            vec![((px * 10) as f32, (ny * 10) as f32)],
        ));
        moving.push(R::new(px, py, pw, ny + ph));
    }
    (base, moving)
}

// ------------------------------------------------------------- treasure

fn add_treasure(
    d: Dims,
    rng: &mut DotNetRandom,
    base: &G,
    blockers: &[R],
    teams: bool,
    ents: &mut Vec<Entity>,
) {
    ents.retain(|e| e.name != "TreasureChest" && e.name != "BigTreasureChest");
    let blocked = |x: i32, y: i32| blockers.iter().any(|b| b.contains(x, y));
    let mut cand: Vec<(f32, f32)> = Vec::new();
    for j in 0..d.half {
        for k in 0..H {
            if j <= 0
                || j == d.half - 2
                || !(4..23).contains(&k)
                || !base.get(j, k + 1)
                || !base.get(j + 1, k + 1)
            {
                continue;
            }
            if (0..4).all(|i| {
                !base.get(j, k - i)
                    && !base.get(j + 1, k - i)
                    && !blocked(j, k - i)
                    && !blocked(j + 1, k - i)
            }) {
                cand.push((((j + 1) * 10) as f32, ((k + 1) * 10) as f32));
            }
        }
    }
    rng.shuffle(&mut cand);
    let mut excl: Vec<(f32, f32)> = Vec::new();
    let names: &[&str] = if teams {
        &["TeamSpawnA", "TeamSpawnB"]
    } else {
        &["PlayerSpawn"]
    };
    for n in names {
        excl.extend(ents.iter().filter(|e| e.name == *n).map(|e| (e.x, e.y)));
    }
    let mut picks: Vec<(f32, f32)> = Vec::new();
    let mut i = 0;
    while picks.len() < 6 && i < cand.len() {
        let c = cand[i];
        if !excl
            .iter()
            .any(|e| (c.0 - e.0).abs() <= 30.0 && (c.1 - e.1).abs() <= 10.0)
        {
            excl.push(c);
            picks.push(c);
        }
        i += 1;
    }
    let shift =
        |rng: &mut DotNetRandom, p: (f32, f32)| (p.0, p.1 + (-10 + rng.choose(&[-10, -20])) as f32);
    for p in picks.iter_mut() {
        *p = shift(rng, *p);
    }
    let centre = (d.w * 5) as f32;
    let mirrors: Vec<_> = picks
        .iter()
        .filter(|p| p.0 != centre)
        .map(|p| ((d.w * 10) as f32 - p.0, p.1))
        .collect();
    picks.extend(mirrors);
    let fmt = |v: f32| format!("{v}");
    for p in &picks {
        ents.push(ent(
            "TreasureChest",
            &[("x", fmt(p.0)), ("y", fmt(p.1))],
            vec![],
        ));
    }
    cand.retain(|c| c.0 == centre);
    if cand.is_empty() {
        // The centre-column test keeps the 32-wide columns 15/16 for the
        // blocker lookups in both builds.
        let free = (0..H).all(|i| {
            !base.get(d.half - 1, i)
                && !base.get(d.half, i)
                && !blocked(d.legacy_skip, i)
                && !blocked(d.legacy_half, i)
        });
        if free {
            cand.push((centre, (100 + rng.next_max(10) * 10) as f32));
        }
    }
    if !cand.is_empty() {
        let value = |y: f32| {
            if y <= 110.0 {
                y as i32
            } else {
                -(y as i32) + 10000
            }
        };
        // List.Sort is unstable; with this key ties only occur between equal
        // positions, so a stable sort gives the same picture.
        cand.sort_by_key(|c| std::cmp::Reverse(value(c.1)));
        let p = shift(rng, cand[0]);
        ents.push(ent(
            "BigTreasureChest",
            &[("x", fmt(p.0)), ("y", fmt(p.1))],
            vec![],
        ));
    }
}

// ------------------------------------------------------------- BG bits

fn generate_bg_bits(d: Dims, rng: &mut DotNetRandom, solids: &G) -> G {
    let w = d.w;
    let mut bg = G::new(w);
    let set = |bg: &mut G, x: i32, y: i32| {
        bg.put(x, y, true);
        if x > 0 && solids.get(x - 1, y) {
            bg.put(x - 1, y, true);
        }
        if x < w - 1 && solids.get(x + 1, y) {
            bg.put(x + 1, y, true);
        }
        if y > 0 && solids.get(x, y - 1) {
            bg.put(x, y - 1, true);
        }
        if y < H - 1 && solids.get(x, y + 1) {
            bg.put(x, y + 1, true);
        }
    };
    const DEPTHS: [i32; 10] = [1, 2, 2, 2, 2, 2, 2, 3, 3, 3];
    for i in 0..w {
        if !solids.get(i, 0) {
            let f = i > 0 && i < w - 1 && solids.get(i - 1, 0) && solids.get(i + 1, 0);
            let n = rng.choose(&DEPTHS);
            for j in 0..n {
                set(&mut bg, i, j);
                if f {
                    set(&mut bg, i - 1, j);
                    set(&mut bg, i + 1, j);
                }
            }
        }
        if solids.get(i, 23) {
            continue;
        }
        let f = i > 0 && i < w - 1 && solids.get(i - 1, 23) && solids.get(i + 1, 23);
        let n = rng.choose(&DEPTHS);
        for j in 0..n {
            set(&mut bg, i, 23 - j);
            if f {
                set(&mut bg, i - 1, 23 - j);
                set(&mut bg, i + 1, 23 - j);
            }
        }
    }
    for k in 0..H {
        if !solids.get(0, k) {
            set(&mut bg, 0, k);
            set(&mut bg, 1, k);
        }
    }
    let step = |rng: &mut DotNetRandom, v: i32, lim: i32, run: &mut i32| -> i32 {
        if *run >= 4 || rng.chance(0.6) {
            *run = 0;
            if v <= 2 {
                v + 1
            } else if v < lim {
                v + rng.choose(&[-2, -1, -1, 1, 1, 2])
            } else {
                v - 1
            }
        } else {
            *run += 1;
            v
        }
    };
    if rng.choose(&[0, 1]) == 0 {
        let lim = rng.choose(&[8, 9, 9, 10]);
        for bottom in [false, true] {
            let mut h = rng.choose(&[4, 5, 5, 6]);
            let mut run = 0;
            for i in 0..d.half {
                h = step(rng, h, lim, &mut run);
                for k in 0..h {
                    bg.put(i, if bottom { 23 - k } else { k }, true);
                }
            }
        }
    } else {
        let lim = rng.choose(&[8, 9, 10, 10, 11]);
        // Both loops write the left side (the second was meant for the right).
        for _ in 0..2 {
            let mut wd = rng.choose(&[5, 6, 6, 7]);
            let mut run = 0;
            for k in 0..H {
                wd = step(rng, wd, lim, &mut run);
                for i in 0..wd {
                    bg.put(i, k, true);
                }
            }
        }
    }
    let check = |bg: &G, x: i32, y: i32| bg.get(x, y) || solids.get(x, y);
    for _ in 0..2 {
        for i in 1..w - 1 {
            for k in 1..H - 1 {
                if bg.get(i, k) && !solids.get(i, k) {
                    if !bg.get(i - 1, k) && !bg.get(i + 1, k) {
                        if !check(&bg, i - 1, k + 1) && !check(&bg, i - 1, k - 1) {
                            set(&mut bg, i + 1, k);
                            continue;
                        }
                        if !check(&bg, i + 1, k + 1) {
                            set(&mut bg, i - 1, k);
                            continue;
                        }
                        let dx = rng.choose(&[1, -1]);
                        set(&mut bg, i + dx, k);
                    } else if !bg.get(i, k - 1) && !bg.get(i, k + 1) {
                        if !check(&bg, i + 1, k - 1) && !check(&bg, i - 1, k - 1) {
                            set(&mut bg, i, k + 1);
                            continue;
                        }
                        if !check(&bg, i + 1, k + 1) && !check(&bg, i - 1, k + 1) {
                            set(&mut bg, i, k - 1);
                            continue;
                        }
                        let dy = rng.choose(&[1, -1]);
                        set(&mut bg, i, k + dy);
                    }
                } else if !bg.get(i, k)
                    && !solids.get(i, k)
                    && check(&bg, i - 1, k)
                    && check(&bg, i + 1, k)
                    && check(&bg, i, k - 1)
                    && check(&bg, i, k + 1)
                {
                    set(&mut bg, i, k);
                }
            }
        }
    }
    for i in 0..d.half {
        for k in 0..H {
            let v = bg.get(i, k);
            bg.put(d.m(i), k, v);
        }
    }
    bg
}

// ------------------------------------------------------------- BG details

fn generate_details(
    d: Dims,
    rng: &mut DotNetRandom,
    base: &G,
    bg: &G,
    fin: &G,
    ents: &mut Vec<Entity>,
) -> T {
    let (w, half) = (d.w, d.half);
    let mut ov = T::new(w);

    // 1. 2×2 ornaments.
    let mut k = count_geo(rng, 0.9, 0.9);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 0..half {
            for m in 0..23 {
                if base.check(l, m, 2, 2, false) && bg.check(l - 1, m - 1, 4, 4, true) {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            let ids = if rng.chance(0.5) {
                [8, 9, 28, 29]
            } else {
                [10, 11, 30, 31]
            };
            let mut put4 = |x: i32| {
                ov.put(x, p.1, ids[0]);
                ov.put(x + 1, p.1, ids[1]);
                ov.put(x, p.1 + 1, ids[2]);
                ov.put(x + 1, p.1 + 1, ids[3]);
            };
            put4(p.0);
            if p.0 < half - 1 {
                put4(w - p.0 - 2);
            }
            cand.retain(|c| !((c.0 - p.0).abs() <= 1 && (c.1 - p.1).abs() <= 1));
        }
    }
    // 2. Floor shelves 2×1.
    let mut k = count_geo(rng, 0.9, 0.8);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 0..half {
            for m in 0..23 {
                if base.check(l, m, 2, 1, false)
                    && base.check(l, m + 1, 2, 1, true)
                    && bg.check(l - 1, m - 1, 4, 3, true)
                    && ov.empty(l, m, 2, 1)
                {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            ov.put(p.0, p.1, 49);
            ov.put(p.0 + 1, p.1, 50);
            if p.0 < half - 1 {
                let x = w - p.0 - 2;
                ov.put(x, p.1, 49);
                ov.put(x + 1, p.1, 50);
            }
            cand.retain(|c| !((c.0 - p.0).abs() <= 1 && c.1 == p.1));
        }
    }
    // 3. Wall vents 1×2.
    let mut k = count_geo(rng, 0.9, 0.8);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 0..half {
            for m in 0..22 {
                if base.check(l, m, 1, 2, false)
                    && base.get(l, m + 2)
                    && bg.check(l - 1, m - 1, 3, 4, true)
                    && ov.empty(l, m, 1, 2)
                {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            for x in [p.0, d.m(p.0)] {
                ov.put(x, p.1, 13);
                ov.put(x, p.1 + 1, 33);
            }
        }
    }
    // 4. Floor nicks.
    let mut k = count_geo(rng, 0.95, 0.9);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 0..half {
            for m in 0..23 {
                if !base.get(l, m)
                    && base.get(l, m + 1)
                    && ov.get(l, m) == -1
                    && bg.check(l - 1, m - 1, 3, 3, true)
                {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            let a = rng.choose(&[69, 70]);
            let b = if a == 69 { 70 } else { 69 };
            ov.put(p.0, p.1, a);
            ov.put(d.m(p.0), p.1, b);
        }
    }
    let eligible = |ov: &T, l: i32, m: i32| {
        !base.get(l, m) && bg.check(l - 1, m - 1, 3, 3, true) && ov.get(l, m) == -1
    };
    // 5. Vertical strips.
    let mut k = count_geo(rng, 0.8, 0.5);
    if k > 0 {
        let mut runs = Vec::new();
        for l in 1..half {
            let mut cur: Option<R> = None;
            for m in 0..H {
                let f = eligible(&ov, l, m);
                match &mut cur {
                    Some(c) if f => c.h += 1,
                    Some(c) => {
                        if c.h >= 4 {
                            runs.push(*c);
                        }
                        cur = None;
                    }
                    None if f => cur = Some(R::new(l, m, 1, 1)),
                    None => {}
                }
            }
            if let Some(c) = cur
                && c.h >= 4
            {
                runs.push(c);
            }
        }
        rng.shuffle(&mut runs);
        while k > 0 && !runs.is_empty() {
            let mut r = runs.remove(0);
            k -= 1;
            if r.h > 3 {
                let orig = r;
                r.y += rng.next_max(r.h - 3);
                r.h = 3;
                if orig.bottom() - r.bottom() > 0 {
                    r.h += rng.next_max(orig.bottom() - r.bottom());
                }
            }
            for x in [r.x, d.m(r.x)] {
                ov.put(x, r.y, 12);
                ov.put(x, r.bottom() - 1, 52);
                for i in 1..r.h - 1 {
                    ov.put(x, r.y + i, 32);
                }
            }
        }
    }
    // 6. Horizontal beams 71/72/73.
    let mut k = count_geo(rng, 0.8, 0.5);
    if k > 0 {
        let mut runs = Vec::new();
        for m in 0..H {
            let mut cur: Option<R> = None;
            for l in 1..half {
                let f = eligible(&ov, l, m);
                match &mut cur {
                    Some(c) if f => c.w += 1,
                    Some(c) => {
                        if c.w >= 4 {
                            runs.push(*c);
                        }
                        cur = None;
                    }
                    None if f => cur = Some(R::new(l, m, 1, 1)),
                    None => {}
                }
            }
            if let Some(c) = cur
                && c.w >= 4
            {
                runs.push(c);
            }
        }
        rng.shuffle(&mut runs);
        while k > 0 && !runs.is_empty() {
            let mut r = runs.remove(0);
            k -= 1;
            if r.w > 3 {
                let orig = r;
                r.x += rng.next_max(r.w - 3);
                r.w = 3;
                if orig.right() - r.right() > 0 {
                    r.w += rng.next_max(orig.right() - r.right());
                }
            }
            let beam = |ov: &mut T, x: i32, wd: i32| {
                ov.put(x, r.y, 71);
                ov.put(x + wd - 1, r.y, 73);
                for i in 1..wd - 1 {
                    ov.put(x + i, r.y, 72);
                }
            };
            if r.right() == half {
                beam(&mut ov, r.x, r.w * 2);
            } else {
                beam(&mut ov, r.x, r.w);
                beam(&mut ov, w - r.x - r.w, r.w);
            }
        }
    }
    // 7. Single decorations.
    let mut k = count_geo1(rng, 0.95);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 0..half {
            for m in 0..H {
                if !base.get(l, m) && ov.get(l, m) == -1 && bg.check(l - 1, m - 1, 3, 3, true) {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            let a = rng.choose(&[48, 51, 53, 68]);
            ov.put(p.0, p.1, a);
            ov.put(d.m(p.0), p.1, a);
        }
    }
    // 8. Wall lanterns (entities).
    let mut k = count_geo1(rng, 0.9);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 2..=half {
            if l == d.legacy_skip {
                continue;
            }
            for m in 2..=22 {
                if base.check(l - 1, m - 1, 2, 3, false)
                    && bg.check(l - 2, m - 1, 4, 3, true)
                    && ov.empty(l - 1, m, 2, 1)
                {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            ents.push(ent("BGLantern", &xy(p.0 * 10, p.1 * 10), vec![]));
            if p.0 != half {
                ents.push(ent("BGLantern", &xy((w - p.0) * 10, p.1 * 10), vec![]));
            }
            cand.retain(|c| (c.0 - p.0).abs() > 2);
        }
    }
    // 9. Pillars with studs.
    let mut k = count_geo(rng, 0.8, 0.7);
    if k > 0 {
        let mut runs = Vec::new();
        for l in 1..half - 1 {
            let mut cur: Option<R> = None;
            for m in 1..H {
                if let Some(mut c) = cur {
                    if fin.get(l, m) {
                        cur = None;
                        continue;
                    }
                    c.h += 1;
                    cur = Some(c);
                    if (m < 23 && fin.get(l, m + 1)) || bg.check(l - 1, m, 3, 1, true) {
                        if (4..=10).contains(&c.h) {
                            runs.push(c);
                        }
                        cur = None;
                    } else if !bg.check(l - 1, m, 3, 1, false) || !fin.check(l - 1, m, 3, 1, false)
                    {
                        cur = None;
                    }
                } else if (bg.check(l - 1, m, 3, 1, true) && !fin.get(l, m))
                    || (fin.get(l, m - 1)
                        && bg.check(l - 1, m, 3, 1, false)
                        && fin.check(l - 1, m, 3, 1, false))
                {
                    cur = Some(R::new(l, m, 1, 1));
                }
            }
        }
        rng.shuffle(&mut runs);
        while k > 0 && !runs.is_empty() {
            let r = runs.remove(0);
            k -= 1;
            let x2 = d.m(r.x);
            for x in [r.x, x2] {
                ov.put(x, r.y, 14);
                ov.put(x, r.bottom() - 1, 74);
                for i in 1..r.h - 1 {
                    ov.put(x, r.y + i, 15);
                }
            }
            let stud = |ov: &mut T, a: i32| {
                ov.put(r.x, r.y + a, 35);
                ov.put(x2, r.y + a, 35);
            };
            if r.h >= 6 {
                if rng.chance(0.7) {
                    let a = 1 + rng.next_max((r.h - 2) / 2);
                    stud(&mut ov, a);
                    let b = r.h - r.h / 2 + rng.next_max((r.h - 2) / 2);
                    stud(&mut ov, b);
                } else if rng.chance(0.9) {
                    let a = 2 + rng.next_max(r.h - 4);
                    stud(&mut ov, a);
                }
            } else if rng.chance(0.9) {
                let a = 1 + rng.next_max(r.h - 2);
                stud(&mut ov, a);
            }
            for x in [r.x, x2] {
                if rng.chance(0.3) {
                    let a = 1 + rng.next_max(r.h - 3);
                    let b = a + 1 + rng.next_max(r.h - 2 - a);
                    ov.put(x, r.y + a, 34);
                    ov.put(x, r.y + b, 54);
                    for i in a + 1..b {
                        ov.put(x, r.y + i, -1);
                    }
                }
            }
            runs.retain(|c| !((c.x - r.x).abs() <= 1 && c.y < r.bottom() && r.y < c.bottom()));
        }
    }
    // 10. Beams with studs and gaps.
    let mut k = count_geo(rng, 0.5, 0.7);
    if k > 0 {
        let mut runs = Vec::new();
        for m in 1..23 {
            let mut cur: Option<R> = None;
            for l in 1..half {
                if let Some(mut c) = cur {
                    if fin.get(l, m) || ov.get(l, m) != -1 {
                        cur = None;
                        continue;
                    }
                    c.w += 1;
                    cur = Some(c);
                    if (l < half - 1 && fin.get(l + 1, m)) || bg.check(l, m - 1, 1, 3, true) {
                        if (4..=8).contains(&c.w) {
                            runs.push(c);
                        }
                        cur = None;
                    } else if !bg.check(l, m - 1, 1, 3, false) || !fin.check(l, m - 1, 1, 3, false)
                    {
                        cur = None;
                    }
                } else if ov.get(l, m) == -1
                    && ((bg.check(l, m - 1, 1, 3, true) && !fin.get(l, m))
                        || (fin.get(l - 1, m)
                            && bg.check(l, m - 1, 1, 3, false)
                            && fin.check(l, m - 1, 1, 3, false)))
                {
                    cur = Some(R::new(l, m, 1, 1));
                }
            }
            if let Some(c) = cur
                && (2..=4).contains(&c.w)
            {
                let dbl = R::new(c.x, c.y, c.w * 2, 1);
                if (0..dbl.w).all(|i| ov.get(dbl.x + i, dbl.y) == -1) {
                    runs.push(dbl);
                }
            }
        }
        rng.shuffle(&mut runs);
        while k > 0 && !runs.is_empty() {
            let r = runs.remove(0);
            k -= 1;
            let beam = |ov: &mut T, x: i32| {
                ov.put(x, r.y, 16);
                ov.put(x + r.w - 1, r.y, 19);
                for i in 1..r.w - 1 {
                    ov.put(x + i, r.y, 55);
                }
            };
            let gap = |ov: &mut T, rng: &mut DotNetRandom, x: i32| {
                let a = 1 + rng.next_max(r.w - 3);
                let b = a + 1 + rng.next_max(r.w - 2 - a);
                ov.put(x + a, r.y, 17);
                ov.put(x + b, r.y, 18);
                for i in a + 1..b {
                    ov.put(x + i, r.y, -1);
                }
            };
            // `(Height - 2) / 2` with Height 1 truncates to 0: Next(0).
            let zero_draw = (r.h - 2) / 2;
            if r.right() > half {
                beam(&mut ov, r.x);
                if rng.chance(0.9) {
                    let a = 1 + rng.next_max(zero_draw);
                    ov.put(r.x + a, r.y, 75);
                    ov.put(r.x + r.w - 1 - a, r.y, 75);
                }
                if rng.chance(0.3) {
                    gap(&mut ov, rng, r.x);
                }
            } else {
                let x2 = w - r.x - r.w;
                beam(&mut ov, r.x);
                beam(&mut ov, x2);
                let stud = |ov: &mut T, a: i32| {
                    ov.put(r.x + a, r.y, 75);
                    ov.put(x2 + r.w - 1 - a, r.y, 75);
                };
                if r.w >= 6 {
                    if rng.chance(0.7) {
                        let a = 1 + rng.next_max(zero_draw);
                        stud(&mut ov, a);
                        let b = r.w - r.w / 2 + rng.next_max((r.w - 2) / 2);
                        stud(&mut ov, b);
                    } else if rng.chance(0.9) {
                        let a = 2 + rng.next_max(r.w - 4);
                        stud(&mut ov, a);
                    }
                } else if rng.chance(0.9) {
                    let a = 1 + rng.next_max(r.w - 2);
                    stud(&mut ov, a);
                }
                if rng.chance(0.3) {
                    gap(&mut ov, rng, r.x);
                }
                if rng.chance(0.3) {
                    gap(&mut ov, rng, x2);
                }
            }
            runs.retain(|c| !((c.y - r.y).abs() <= 1 && c.x < r.right() && r.x < c.right()));
        }
    }
    // 11. Vent blocks 2×2.
    let mut k = count_geo(rng, 0.8, 0.4);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 1..=half {
            if l == d.legacy_skip {
                continue;
            }
            for m in 2..23 {
                if base.check(l - 1, m + 1, 2, 1, true)
                    && base.check(l - 1, m - 1, 2, 2, false)
                    && bg.check(l - 1, m - 1, 2, 2, false)
                    && ov.empty(l - 1, m - 1, 2, 2)
                {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            let mut put = |x: i32| {
                ov.put(x - 1, p.1 - 1, 58);
                ov.put(x, p.1 - 1, 59);
                ov.put(x - 1, p.1, 78);
                ov.put(x, p.1, 79);
            };
            put(p.0);
            if p.0 != half {
                put(w - p.0);
            }
            cand.retain(|c| !((c.0 - p.0).abs() <= 1 && (c.1 - p.1).abs() <= 1));
        }
    }
    // 12. Hanging vines.
    let mut k = count_geo(rng, 0.8, 0.6);
    if k > 0 {
        let mut runs = Vec::new();
        for l in 1..half - 1 {
            for m in (6..=22).rev() {
                if bg.check(l, m - 2, 1, 3, false)
                    && base.check(l, m - 2, 1, 3, false)
                    && base.get(l, m + 1)
                    && ov.empty(l, m - 2, 1, 3)
                {
                    let mut v = R::new(l, m - 1, 1, 2);
                    for _ in 0..2 {
                        if bg.get(l, v.y - 2) || base.get(l, v.y - 2) || ov.get(l, v.y - 2) != -1 {
                            break;
                        }
                        v.h += 1;
                        v.y -= 1;
                    }
                    runs.push(v);
                }
            }
        }
        rng.shuffle(&mut runs);
        while k > 0 && !runs.is_empty() {
            let mut a = runs.remove(0);
            k -= 1;
            let mut b = a;
            b.x = d.m(a.x);
            if a.h > 2 {
                let n1 = 2 + rng.next_max(a.h - 1);
                let n2 = 2 + rng.next_max(a.h - 1);
                a.y = a.bottom() - n1;
                a.h = n1;
                b.y = b.bottom() - n2;
                b.h = n2;
            }
            for v in [a, b] {
                ov.put(v.x, v.y, 37);
                ov.put(v.x, v.bottom() - 1, 77);
                for i in 1..v.h - 1 {
                    ov.put(v.x, v.y + i, 57);
                }
            }
        }
    }
    // 13. Floor pebbles.
    let mut k = count_geo1(rng, 0.8);
    if k > 0 {
        let mut cand = Vec::new();
        for l in 0..half - 1 {
            for m in 1..23 {
                if base.get(l, m + 1) && !base.get(l, m) && !bg.get(l, m) && ov.get(l, m) == -1 {
                    cand.push((l, m));
                }
            }
        }
        rng.shuffle(&mut cand);
        while k > 0 && !cand.is_empty() {
            let p = cand.remove(0);
            k -= 1;
            if rng.chance(0.6) {
                if rng.chance(0.5) {
                    ov.put(p.0, p.1, 38);
                } else {
                    ov.put(d.m(p.0), p.1, 38);
                }
            } else {
                ov.put(p.0, p.1, 39);
                ov.put(d.m(p.0), p.1, 39);
                k -= 1;
            }
        }
    }
    ov
}

// ------------------------------------------------------------- pipeline

/// The generated replacement for a procedural level.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub solids: BitGrid,
    pub bg: BitGrid,
    /// Replaces the file's `BGTiles` overrides.
    pub bg_tiles: TileGrid,
    /// The level's entities with treasure re-placed and generated items
    /// appended (`RandomBlock` areas stay in the list; they are never
    /// instantiated).
    pub entities: Vec<Entity>,
}

/// Runs the whole generator on `level` with `seed` (the match's random-level
/// seed); `teams` = Team Deathmatch.
pub fn generate(level: &Level, seed: i32, teams: bool) -> Generated {
    let (cw, ch) = level.cells();
    assert_eq!(ch, 24, "procedural levels are 24 cells tall");
    let d = Dims::for_width_cells(cw as i32);
    let base = G::from_bits(&level.solids);
    let rects: Vec<R> = level
        .entities_named("RandomBlock")
        .map(|e| {
            let (w, h) = e.size().unwrap_or((10, 10));
            R::new(e.x as i32 / 10, e.y as i32 / 10, w / 10, h / 10)
        })
        .collect();
    let mut rng = DotNetRandom::new(seed);
    let mut solids = generate_geometry(d, &mut rng, &rects, &base);
    let mut entities = level.entities.clone();
    let mut added = Vec::new();
    let (base_solids, moving) = add_items(d, &mut rng, &mut solids, &mut added);
    entities.extend(added);
    add_treasure(d, &mut rng, &base_solids, &moving, teams, &mut entities);
    let bg = generate_bg_bits(d, &mut rng, &solids);
    let ov = generate_details(d, &mut rng, &base_solids, &bg, &solids, &mut entities);
    Generated {
        solids: solids.to_bits(),
        bg: bg.to_bits(),
        bg_tiles: TileGrid {
            width: cw,
            height: ch,
            cells: ov.c,
        },
        entities,
    }
}

impl Level {
    /// A copy of this level with the procedural generator applied.
    pub fn with_generated(&self, g: &Generated) -> Level {
        let mut l = self.clone();
        l.solids = g.solids.clone();
        l.bg = g.bg.clone();
        l.bg_tiles = g.bg_tiles.clone();
        l.entities = g.entities.clone();
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::installs;
    use crate::towers::TowerIndex;

    fn mirror_ok(b: &BitGrid) -> bool {
        (0..b.height).all(|y| (0..b.width).all(|x| b.get(x, y) == b.get(b.width - 1 - x, y)))
    }

    #[test]
    fn cataclysm_levels_generate_symmetric_geometry() {
        for inst in installs().iter() {
            let idx = TowerIndex::build(inst).unwrap();
            let Some(t) = idx.versus.iter().find(|t| t.procedural) else {
                continue;
            };
            for p in &t.levels {
                let l = Level::load(p).unwrap();
                for seed in [0, 1, 12345, -77, 2_000_000_000] {
                    let g = generate(&l, seed, false);
                    assert!(mirror_ok(&g.bg), "{p:?} seed {seed}: BG not mirrored");
                    assert!(
                        g.entities.iter().any(|e| e.name == "TreasureChest"),
                        "{p:?} seed {seed}: no chests"
                    );
                    assert!(g.solids.count() >= l.solids.count() / 2);
                }
            }
        }
    }

    fn dump(g: &Generated) -> String {
        let bits = |b: &BitGrid| {
            (0..b.height)
                .map(|y| {
                    (0..b.width)
                        .map(|x| if b.get(x, y) { '1' } else { '0' })
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("|")
        };
        let mut s = format!("solids {}\nbg {}\n", bits(&g.solids), bits(&g.bg));
        s.push_str("ov ");
        s.push_str(
            &g.bg_tiles
                .cells
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
        s.push('\n');
        for e in &g.entities {
            let mut keys: Vec<String> = e.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
            keys.push(format!("x={}", e.x));
            keys.push(format!("y={}", e.y));
            keys.sort();
            s.push_str(&format!("ent {}", e.name));
            for k in keys {
                s.push(' ');
                s.push_str(&k);
            }
            for (x, y) in &e.nodes {
                s.push_str(&format!(" node={x}:{y}"));
            }
            s.push('\n');
        }
        s
    }

    /// Compares against dumps of the reference generator, if a developer
    /// produced them in `scratch/procgen-oracle/out/` (see docs/formats/procedural.md).
    #[test]
    fn matches_reference_dumps() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scratch/procgen-oracle/out");
        let mut checked = 0;
        for inst in installs().iter() {
            let sub = match inst.kind {
                crate::install::InstallKind::Steam => "steam",
                crate::install::InstallKind::EightPlayer => "8p",
            };
            let dir = root.join(sub);
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            let idx = TowerIndex::build(inst).unwrap();
            let t = idx.versus.iter().find(|t| t.procedural).unwrap();
            let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            let mut failures = Vec::new();
            for f in files {
                let stem = f.file_stem().unwrap().to_string_lossy().into_owned();
                let parts: Vec<&str> = stem.split('_').collect();
                let (lvl, seed, teams) =
                    (parts[0], parts[1].parse::<i32>().unwrap(), parts.len() > 2);
                let p = t
                    .levels
                    .iter()
                    .find(|p| p.file_stem().unwrap() == lvl)
                    .unwrap();
                let l = Level::load(p).unwrap();
                let want = std::fs::read_to_string(&f).unwrap();
                let got = dump(&generate(&l, seed, teams));
                if got != want {
                    if std::env::var_os("TFP_DUMP_DIFF").is_some() {
                        let _ = std::fs::write(f.with_extension("rust"), &got);
                    }
                    let line = want.lines().zip(got.lines()).position(|(a, b)| a != b);
                    failures.push(format!("{sub}/{stem}: first differing line {line:?}"));
                }
                checked += 1;
            }
            assert!(
                failures.is_empty(),
                "{} of {checked} differ: {:#?}",
                failures.len(),
                failures
            );
        }
        eprintln!("{checked} reference dumps compared");
    }
}
