//! The final tile ids of a level, as the game draws them: both layers are
//! autotiled with wrap-around edges from a fresh `System.Random(load_seed)`
//! each, the BG layer treats solid cells as filled neighbours, and the
//! hand-placed `BGTiles` / `SolidTiles` overrides replace the computed tiles.
//! See `docs/formats/autotile.md`.

use crate::autotile::{self, AutotileData, EdgeBehavior};
use crate::dotnet_random::DotNetRandom;
use crate::image::Rect;
use crate::level::{CELL, Level};
use crate::xml::TileGrid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelTiles {
    /// Background layer tile ids (BG tileset), `-1` = nothing.
    pub bg: TileGrid,
    /// The BG layer before the hand-placed overrides: the game looks up
    /// tile-id lights in this grid (BG overrides never emit light), while
    /// solid overrides do count.
    pub bg_autotiled: TileGrid,
    /// Solid layer tile ids (solid tileset), `-1` = nothing.
    pub solid: TileGrid,
}

impl LevelTiles {
    pub fn compute(
        level: &Level,
        solid_set: &AutotileData,
        bg_set: &AutotileData,
        load_seed: i32,
    ) -> LevelTiles {
        let mut rng = DotNetRandom::new(load_seed);
        let mut solid =
            autotile::autotile(&level.solids, None, solid_set, EdgeBehavior::Wrap, &mut rng);
        autotile::apply_overwrite(&mut solid, &level.solid_tiles);
        let mut rng = DotNetRandom::new(load_seed);
        let bg_autotiled = autotile::autotile(
            &level.bg,
            Some(&level.solids),
            bg_set,
            EdgeBehavior::Wrap,
            &mut rng,
        );
        let mut bg = bg_autotiled.clone();
        autotile::apply_overwrite(&mut bg, &level.bg_tiles);
        LevelTiles {
            bg,
            bg_autotiled,
            solid,
        }
    }
}

/// Source rectangle of tile `id` in a tileset image occupying `sheet` in its
/// atlas (10×10 tiles, row-major); `None` if out of range.
pub fn tile_rect(sheet: Rect, id: i32) -> Option<Rect> {
    let c = CELL as i32;
    let cols = sheet.w / c;
    let rows = sheet.h / c;
    if id < 0 || cols == 0 || id >= cols * rows {
        return None;
    }
    Some(Rect::new(
        sheet.x + (id % cols) * c,
        sheet.y + (id / cols) * c,
        c,
        c,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_data::GameData;
    use crate::testutil::{installs, require};
    use crate::towers::{TowerIndex, game_relative_path, versus_load_seed};

    #[test]
    fn versus_levels_tile_within_their_tilesets() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let gd = GameData::load(&steam.content).unwrap();
        let atlas =
            crate::atlas::Atlas::load(&steam.atlas_xml(crate::atlas::AtlasKind::Main).unwrap())
                .unwrap();
        let idx = TowerIndex::build(steam).unwrap();
        for t in &idx.versus {
            let th = gd.theme(&t.theme).unwrap();
            let ss = gd.tileset(&th.tileset).unwrap();
            let bs = gd.tileset(&th.bg_tileset).unwrap();
            let (sr, br) = (atlas.get(&ss.image).unwrap(), atlas.get(&bs.image).unwrap());
            for p in &t.levels {
                let l = Level::load(p).unwrap();
                let seed = versus_load_seed(&game_relative_path(steam, p).unwrap());
                let tiles = LevelTiles::compute(&l, &ss.autotile, &bs.autotile, seed);
                for (i, &id) in tiles.solid.cells.iter().enumerate() {
                    if l.solids.cells[i] {
                        assert!(id >= 0, "{p:?}: solid cell {i} without a tile");
                    }
                    if id >= 0 {
                        assert!(tile_rect(sr, id).is_some(), "{p:?}: solid tile {id}");
                    }
                }
                for &id in &tiles.bg.cells {
                    if id >= 0 {
                        assert!(tile_rect(br, id).is_some(), "{p:?}: bg tile {id}");
                    }
                }
            }
        }
    }
}
