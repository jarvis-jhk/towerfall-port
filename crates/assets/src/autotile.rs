//! Autotiling: turning a solid/BG bit grid into tile indices of a tileset,
//! following the original's rules (see `docs/formats/autotile.md`).
//!
//! A tileset lists, for each neighbourhood class (centre, edges, corners,
//! inside corners, one-tile-wide runs, below-an-edge), the tile indices that
//! may be used; when there are several, one is picked with the level's
//! seeded `System.Random`, so the exact pick depends on the visiting order:
//! column by column (x outer, y inner).

use crate::dotnet_random::DotNetRandom;
use crate::xml::{self, BitGrid, TileGrid};
use roxmltree::Node;

/// Candidate tiles for each neighbourhood class; empty = draw nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutotileData {
    pub center: Vec<i32>,
    pub single: Vec<i32>,
    pub single_horizontal_left: Vec<i32>,
    pub single_horizontal_center: Vec<i32>,
    pub single_horizontal_right: Vec<i32>,
    pub single_vertical_top: Vec<i32>,
    pub single_vertical_center: Vec<i32>,
    pub single_vertical_bottom: Vec<i32>,
    pub top: Vec<i32>,
    pub bottom: Vec<i32>,
    pub left: Vec<i32>,
    pub right: Vec<i32>,
    pub top_left: Vec<i32>,
    pub top_right: Vec<i32>,
    pub bottom_left: Vec<i32>,
    pub bottom_right: Vec<i32>,
    pub inside_top_left: Vec<i32>,
    pub inside_top_right: Vec<i32>,
    pub inside_bottom_left: Vec<i32>,
    pub inside_bottom_right: Vec<i32>,
    /// Drawn in an *empty* cell whose upper neighbour is filled.
    pub below: Vec<i32>,
    /// Tiles that emit light when placed.
    pub lights: Vec<TileLight>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TileLight {
    pub tiles: Vec<i32>,
    /// RGBA as written (the game inverts it for its subtractive light pass).
    pub color: [u8; 4],
    /// Light radius in pixels (default 12).
    pub radius: f32,
}

impl AutotileData {
    /// Reads the class lists from the children of a `<Tileset>` element
    /// (first child of each name; a missing child means "no tile").
    pub fn from_xml(node: Node) -> Result<Self, String> {
        let list = |name: &str| -> Result<Vec<i32>, String> {
            match xml::child_text(node, name) {
                Some(t) => xml::parse_csv_ints(&t).map_err(|e| format!("{name}: {e}")),
                None => Ok(Vec::new()),
            }
        };
        let mut lights = Vec::new();
        for l in xml::descendants(node, "Lights") {
            let tiles = xml::parse_csv_ints(&xml::inner_text(l))?;
            let color = l
                .attribute("color")
                .and_then(xml::parse_hex_color)
                .ok_or("Lights without color")?;
            let radius = xml::attr_f32(l, "radius").unwrap_or(12.0);
            lights.push(TileLight {
                tiles,
                color,
                radius,
            });
        }
        Ok(AutotileData {
            center: list("Center")?,
            single: list("Single")?,
            single_horizontal_left: list("SingleHorizontalLeft")?,
            single_horizontal_center: list("SingleHorizontalCenter")?,
            single_horizontal_right: list("SingleHorizontalRight")?,
            single_vertical_top: list("SingleVerticalTop")?,
            single_vertical_center: list("SingleVerticalCenter")?,
            single_vertical_bottom: list("SingleVerticalBottom")?,
            top: list("Top")?,
            bottom: list("Bottom")?,
            left: list("Left")?,
            right: list("Right")?,
            top_left: list("TopLeft")?,
            top_right: list("TopRight")?,
            bottom_left: list("BottomLeft")?,
            bottom_right: list("BottomRight")?,
            inside_top_left: list("InsideTopLeft")?,
            inside_top_right: list("InsideTopRight")?,
            inside_bottom_left: list("InsideBottomLeft")?,
            inside_bottom_right: list("InsideBottomRight")?,
            below: list("Below")?,
            lights,
        })
    }

    /// The candidate list for a cell with neighbourhood `n` (`None` for an
    /// empty cell with nothing above it).
    pub fn classify(&self, n: Neighbours) -> Option<&[i32]> {
        let Neighbours {
            here,
            l,
            r,
            u,
            d,
            ul,
            ur,
            dl,
            dr,
        } = n;
        if !here {
            return u.then_some(self.below.as_slice());
        }
        let v = if l && r && u && d && ul && ur && dl && dr {
            &self.center
        } else if !u && !d {
            // One tile tall.
            match (l, r) {
                (true, true) => &self.single_horizontal_center,
                (false, false) => &self.single,
                (true, false) => &self.single_horizontal_right,
                (false, true) => &self.single_horizontal_left,
            }
        } else if !l && !r {
            // One tile wide.
            match (u, d) {
                (true, true) => &self.single_vertical_center,
                (false, true) => &self.single_vertical_top,
                _ => &self.single_vertical_bottom,
            }
        } else if u && d && l && !r {
            &self.right
        } else if u && d && !l && r {
            &self.left
        } else if u && !l && r && !d {
            &self.bottom_left
        } else if u && l && !r && !d {
            &self.bottom_right
        } else if d && r && !l && !u {
            &self.top_left
        } else if d && !r && l && !u {
            &self.top_right
        } else if u && d && !dr && dl {
            &self.inside_top_left
        } else if u && d && dr && !dl {
            &self.inside_top_right
        } else if u && d && ul && !ur {
            &self.inside_bottom_left
        } else if u && d && !ul && ur {
            &self.inside_bottom_right
        } else if !d {
            &self.bottom
        } else {
            &self.top
        };
        Some(v.as_slice())
    }
}

/// Occupancy of a cell and its eight neighbours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Neighbours {
    pub here: bool,
    pub l: bool,
    pub r: bool,
    pub u: bool,
    pub d: bool,
    pub ul: bool,
    pub ur: bool,
    pub dl: bool,
    pub dr: bool,
}

/// What lies beyond the grid edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeBehavior {
    /// Outside counts as filled.
    Filled,
    /// Outside counts as empty.
    Empty,
    /// The grid repeats (levels use this: they wrap around).
    Wrap,
    /// Outside copies the nearest edge cell.
    Clamp,
}

fn neighbours(
    grid: &BitGrid,
    also: Option<&BitGrid>,
    x: usize,
    y: usize,
    edges: EdgeBehavior,
) -> Neighbours {
    let (w, h) = (grid.width as i64, grid.height as i64);
    let at = |dx: i64, dy: i64| -> bool {
        let (mut nx, mut ny) = (x as i64 + dx, y as i64 + dy);
        let outside = nx < 0 || ny < 0 || nx >= w || ny >= h;
        if outside {
            match edges {
                EdgeBehavior::Filled => return true,
                EdgeBehavior::Empty => return false,
                EdgeBehavior::Wrap => {
                    nx = nx.rem_euclid(w);
                    ny = ny.rem_euclid(h);
                }
                EdgeBehavior::Clamp => {
                    nx = nx.clamp(0, w - 1);
                    ny = ny.clamp(0, h - 1);
                }
            }
        }
        let (ux, uy) = (nx as usize, ny as usize);
        grid.get(ux, uy) || also.is_some_and(|a| a.get(ux, uy))
    };
    Neighbours {
        here: grid.get(x, y),
        l: at(-1, 0),
        r: at(1, 0),
        u: at(0, -1),
        d: at(0, 1),
        ul: at(-1, -1),
        ur: at(1, -1),
        dl: at(-1, 1),
        dr: at(1, 1),
    }
}

/// Autotiles `grid`. Cells of `also` count as filled *neighbours* (the BG
/// layer is tiled with the solids as `also`, so BG edges don't show against
/// solid blocks) but never get a tile themselves. Picks among several
/// candidates with `rng.next_max(len)`, visiting x-major.
pub fn autotile(
    grid: &BitGrid,
    also: Option<&BitGrid>,
    data: &AutotileData,
    edges: EdgeBehavior,
    rng: &mut DotNetRandom,
) -> TileGrid {
    let mut out = TileGrid::new(grid.width, grid.height);
    for x in 0..grid.width {
        for y in 0..grid.height {
            let n = neighbours(grid, also, x, y, edges);
            let tile = match data.classify(n) {
                None | Some([]) => -1,
                Some([one]) => *one,
                Some(list) => list[rng.next_max(list.len() as i32) as usize],
            };
            out.set(x, y, tile);
        }
    }
    out
}

/// Draws `overwrite` (≠ -1) over `tiles` — the hand-placed tiles of a level.
pub fn apply_overwrite(tiles: &mut TileGrid, overwrite: &TileGrid) {
    for y in 0..tiles.height.min(overwrite.height) {
        for x in 0..tiles.width.min(overwrite.width) {
            let v = overwrite.get(x, y);
            if v != -1 {
                tiles.set(x, y, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> AutotileData {
        AutotileData {
            center: vec![0],
            single: vec![1],
            top: vec![2],
            bottom: vec![3],
            left: vec![4],
            right: vec![5],
            top_left: vec![6],
            top_right: vec![7],
            bottom_left: vec![8],
            bottom_right: vec![9],
            below: vec![10],
            single_horizontal_left: vec![11],
            single_horizontal_center: vec![12],
            single_horizontal_right: vec![13],
            ..Default::default()
        }
    }

    #[test]
    fn block_classes() {
        let g = BitGrid::from_bitstring("00000\n01110\n01110\n01110\n00000\n01110\n00000", 5, 7);
        let mut rng = DotNetRandom::new(0);
        let t = autotile(&g, None, &data(), EdgeBehavior::Empty, &mut rng);
        assert_eq!(t.get(1, 1), 6);
        assert_eq!(t.get(2, 1), 2);
        assert_eq!(t.get(3, 1), 7);
        assert_eq!(t.get(1, 2), 4);
        assert_eq!(t.get(2, 2), 0);
        assert_eq!(t.get(3, 2), 5);
        assert_eq!(t.get(1, 3), 8);
        assert_eq!(t.get(2, 3), 3);
        assert_eq!(t.get(3, 3), 9);
        assert_eq!(t.get(2, 4), 10, "below");
        assert_eq!(t.get(0, 0), -1);
        assert_eq!((t.get(1, 5), t.get(2, 5), t.get(3, 5)), (11, 12, 13));
        assert_eq!(t.get(2, 6), 10);
    }

    #[test]
    fn wrap_edges() {
        let g = BitGrid::from_bitstring("1\n1\n1", 1, 3);
        let mut rng = DotNetRandom::new(0);
        let d = data();
        let t = autotile(&g, None, &d, EdgeBehavior::Wrap, &mut rng);
        // Wrapping a 1-wide column onto itself: every neighbour is filled.
        assert_eq!(t.get(0, 1), 0);
    }
}
