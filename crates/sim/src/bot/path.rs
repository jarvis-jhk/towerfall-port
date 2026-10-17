//! Reachability for the shared bot: a graph of *standing cells* (10 px
//! tiles where an archer can stand) with walk, fall, drop-through and jump
//! edges, built once per level from the static tiles and jump-thrus
//! (moving platforms and tower solids are ignored), plus a breadth-first
//! search from the bot's cell. Deterministic and cheap: 768 cells on a
//! 320 px level, built in well under a millisecond.

use crate::content::LevelDef;

/// How to take an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// Walk (or step) sideways.
    Walk,
    /// Walk off a ledge and fall.
    Fall,
    /// Duck on a jump-thru and drop through it.
    Drop,
    /// A full jump (held), steering toward the target column.
    Jump,
}

/// Jump reach in cells: a held jump rises ~34 px (3 cells) and covers ~3
/// cells sideways on the way up.
const JUMP_UP: i32 = 3;
const JUMP_SIDE: i32 = 3;
/// A running jump across a gap (same height or lower).
const LONG_JUMP: i32 = 5;

#[derive(Clone, Debug)]
pub struct Nav {
    pub level: usize,
    cols: i32,
    rows: i32,
    solid: Vec<bool>,
    jt: Vec<bool>,
    stand: Vec<bool>,
    edges: Vec<Vec<(u16, Move)>>,
}

impl Nav {
    pub fn build(level: usize, lvl: &LevelDef) -> Nav {
        let cols = lvl.cells_x as i32;
        let rows = lvl.cells_y as i32;
        let n = (cols * rows) as usize;
        let mut solid = vec![false; n];
        for y in 0..rows {
            for x in 0..cols {
                solid[(y * cols + x) as usize] = lvl.solid(x, y);
            }
        }
        let mut jt = vec![false; n];
        for j in &lvl.jump_thrus {
            let y = (j.pos.y / 10.0).floor() as i32;
            let x0 = (j.pos.x / 10.0).floor() as i32;
            let x1 = ((j.pos.x + j.width) / 10.0).ceil() as i32;
            if (0..rows).contains(&y) {
                for x in x0..x1 {
                    let x = x.rem_euclid(cols);
                    jt[(y * cols + x) as usize] = true;
                }
            }
        }
        let mut nav = Nav {
            level,
            cols,
            rows,
            solid,
            jt,
            stand: vec![false; n],
            edges: vec![vec![]; n],
        };
        for y in 0..rows {
            for x in 0..cols {
                let s = nav.free(x, y) && nav.free(x, y - 1) && nav.support(x, y + 1);
                nav.stand[(y * cols + x) as usize] = s;
            }
        }
        for y in 0..rows {
            for x in 0..cols {
                if nav.is_stand(x, y) {
                    let e = nav.edges_from(x, y);
                    let i = nav.idx(x, y);
                    nav.edges[i] = e;
                }
            }
        }
        nav
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        (y.rem_euclid(self.rows) * self.cols + x.rem_euclid(self.cols)) as usize
    }
    pub fn cell(&self, i: usize) -> (i32, i32) {
        (i as i32 % self.cols, i as i32 / self.cols)
    }
    fn is_solid(&self, x: i32, y: i32) -> bool {
        self.solid[self.idx(x, y)]
    }
    fn is_jt(&self, x: i32, y: i32) -> bool {
        self.jt[self.idx(x, y)]
    }
    /// Room for a body part (jump-thrus are passable).
    fn free(&self, x: i32, y: i32) -> bool {
        !self.is_solid(x, y)
    }
    fn support(&self, x: i32, y: i32) -> bool {
        self.is_solid(x, y) || self.is_jt(x, y)
    }
    pub fn is_stand(&self, x: i32, y: i32) -> bool {
        self.stand[self.idx(x, y)]
    }

    /// Where something falling down column `x` from row `y` lands.
    fn land(&self, x: i32, mut y: i32) -> Option<(i32, i32)> {
        for _ in 0..self.rows {
            if !self.free(x, y) {
                return None;
            }
            if self.is_stand(x, y) {
                return Some((x.rem_euclid(self.cols), y.rem_euclid(self.rows)));
            }
            y += 1;
        }
        None
    }

    fn edges_from(&self, x: i32, y: i32) -> Vec<(u16, Move)> {
        let mut out: Vec<(u16, Move)> = vec![];
        let add = |out: &mut Vec<(u16, Move)>, to: (i32, i32), m: Move| {
            let i = self.idx(to.0, to.1) as u16;
            if !out.iter().any(|e| e.0 == i) {
                out.push((i, m));
            }
        };
        for dir in [-1, 1] {
            let nx = x + dir;
            if self.is_stand(nx, y) {
                add(&mut out, (nx, y), Move::Walk);
            } else if self.free(nx, y)
                && self.free(nx, y - 1)
                && let Some(l) = self.land(nx, y + 1)
            {
                add(&mut out, l, Move::Fall);
            }
        }
        // Drop through the jump-thru underfoot.
        if self.is_jt(x, y + 1)
            && !self.is_solid(x, y + 1)
            && let Some(l) = self.land(x, y + 2)
        {
            add(&mut out, l, Move::Drop);
        }
        // Jumps up (and straight up): the column rises clear of solids, the
        // top rows are clear to the target column.
        for dy in 1..=JUMP_UP {
            if !(1..=dy + 1).all(|k| self.free(x, y - k)) {
                break;
            }
            for dx in -JUMP_SIDE..=JUMP_SIDE {
                let (tx, ty) = (x + dx, y - dy);
                if !self.is_stand(tx, ty) {
                    continue;
                }
                let s = dx.signum();
                let mut ok = true;
                let mut cx = x;
                while cx != tx {
                    cx += s;
                    if !self.free(cx, ty) || !self.free(cx, ty - 1) {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    add(&mut out, (tx, ty), Move::Jump);
                }
            }
        }
        // Running jumps across gaps, landing level or lower.
        for dir in [-1, 1] {
            for dx in 2..=LONG_JUMP {
                let tx = x + dir * dx;
                let clear = (1..=dx)
                    .all(|k| self.free(x + dir * k, y - 1) && self.free(x + dir * k, y - 2));
                if !clear {
                    break;
                }
                for down in 0..=2 {
                    if self.is_stand(tx, y + down) {
                        add(&mut out, (tx, y + down), Move::Jump);
                        break;
                    }
                }
            }
        }
        out
    }

    /// The standing cell of an archer whose feet are at `(x, bottom)`: its
    /// cell, or the first standing cell below (mid-air).
    pub fn stand_at(&self, x: f32, bottom: f32) -> Option<usize> {
        let cx = (x / 10.0).floor() as i32;
        let cy = ((bottom - 1.0) / 10.0).floor() as i32;
        for k in 0..self.rows {
            if self.is_stand(cx, cy + k) {
                return Some(self.idx(cx, cy + k));
            }
        }
        None
    }

    /// Breadth-first distances (in edges) and parents from `start`.
    pub fn bfs(&self, start: usize) -> (Vec<u16>, Vec<u16>) {
        let n = self.stand.len();
        let mut dist = vec![u16::MAX; n];
        let mut parent = vec![u16::MAX; n];
        let mut q = std::collections::VecDeque::new();
        dist[start] = 0;
        q.push_back(start);
        while let Some(c) = q.pop_front() {
            for &(to, _) in &self.edges[c] {
                let t = to as usize;
                if dist[t] == u16::MAX {
                    dist[t] = dist[c] + 1;
                    parent[t] = c as u16;
                    q.push_back(t);
                }
            }
        }
        (dist, parent)
    }

    /// The first edge on the path `start -> goal` (from a BFS of `start`).
    pub fn first_step(&self, start: usize, goal: usize, parent: &[u16]) -> Option<(usize, Move)> {
        if goal == start || parent[goal] == u16::MAX {
            return None;
        }
        let mut c = goal;
        while parent[c] as usize != start {
            c = parent[c] as usize;
            if c == u16::MAX as usize {
                return None;
            }
        }
        self.edges[start]
            .iter()
            .find(|e| e.0 as usize == c)
            .map(|e| (c, e.1))
    }

    /// Standing cells (index, centre position).
    pub fn stands(&self) -> impl Iterator<Item = (usize, (f32, f32))> + '_ {
        (0..self.stand.len()).filter(|&i| self.stand[i]).map(|i| {
            let (x, y) = self.cell(i);
            (i, (x as f32 * 10.0 + 5.0, y as f32 * 10.0 + 10.0))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;

    #[test]
    fn reaches_platforms_by_jumps_and_stairs() {
        // A floor, a low ledge (reachable by one jump), and a high shelf
        // only reachable from the ledge.
        let mut map = vec!["................................"; 24];
        map[23] = "################################";
        map[22] = "################################";
        map[19] = "..........#####.................";
        map[16] = "................######..........";
        let c = Content::from_ascii(&map);
        let nav = Nav::build(0, c.level(0));
        let start = nav.stand_at(30.0, 220.0).expect("standing on the floor");
        let (dist, parent) = nav.bfs(start);
        let ledge = nav.stand_at(120.0, 190.0).expect("ledge cell");
        let shelf = nav.stand_at(190.0, 160.0).expect("shelf cell");
        assert!(dist[ledge] != u16::MAX, "ledge reachable");
        assert!(dist[shelf] != u16::MAX, "shelf reachable");
        assert!(dist[shelf] > dist[ledge], "the shelf takes the ledge");
        let (_, mv) = nav.first_step(start, shelf, &parent).expect("a first step");
        assert!(matches!(mv, Move::Walk | Move::Jump));
    }
}
