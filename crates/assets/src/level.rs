//! Ogmo Editor levels (`*.oel`) and the `<level>` elements inside Workshop
//! `.tower` files. See `docs/formats/levels.md`.
//!
//! A level is a grid of 10×10-pixel cells (32×24 for 320×240, 42×24 for the
//! 8-player build's 420×240) with four layers — `BG` and `Solids` as
//! bitstrings, `BGTiles` and `SolidTiles` as trimmed CSV of hand-placed tile
//! overrides — and an `Entities` list.

use crate::error::{Error, Result};
use crate::xml::{self, BitGrid, TileGrid, XmlFile};
use roxmltree::Node;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CELL: u32 = 10;

#[derive(Clone, Debug, PartialEq)]
pub enum LayerData {
    Bits(BitGrid),
    Tiles(TileGrid),
    Entities,
    /// Anything else (e.g. `LoadSeed` in Workshop levels): its text.
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    /// `Bitstring`, `TrimmedCSV`, … (empty when not given).
    pub export_mode: String,
    /// The tileset named in the file. The game ignores it and uses the
    /// theme's tilesets.
    pub tileset: Option<String>,
    pub data: LayerData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    /// Element name = entity type (`PlayerSpawn`, `TreasureChest`, …).
    pub name: String,
    pub id: Option<i32>,
    pub x: f32,
    pub y: f32,
    /// Every attribute except `id`, `x`, `y`, as written.
    pub attrs: BTreeMap<String, String>,
    /// `<node x y/>` children (paths, spawn portal targets).
    pub nodes: Vec<(f32, f32)>,
}

impl Entity {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(|s| s.as_str())
    }
    pub fn attr_i32(&self, k: &str) -> Option<i32> {
        self.attr(k).and_then(|v| v.trim().parse().ok())
    }
    pub fn attr_f32(&self, k: &str) -> Option<f32> {
        self.attr(k).and_then(xml::parse_f32)
    }
    pub fn attr_bool(&self, k: &str) -> Option<bool> {
        self.attr(k).and_then(xml::parse_bool)
    }
    /// `width`/`height` for sized entities (in pixels).
    pub fn size(&self) -> Option<(i32, i32)> {
        Some((self.attr_i32("width")?, self.attr_i32("height")?))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub path: PathBuf,
    /// Pixels.
    pub width: u32,
    pub height: u32,
    /// Root attributes other than `width`/`height` (`Darkness`, `WrapMode`,
    /// `CanUnlockMoonstone`, `CanUnlockPurple`, `MiasmaH`, `QuestMode`,
    /// `Timer`; Workshop: `ffa`, `teams`).
    pub attrs: BTreeMap<String, String>,
    pub solids: BitGrid,
    pub bg: BitGrid,
    pub solid_tiles: TileGrid,
    pub bg_tiles: TileGrid,
    /// All layers in file order.
    pub layers: Vec<Layer>,
    pub entities: Vec<Entity>,
}

impl Level {
    pub fn load(path: &Path) -> Result<Level> {
        let file = XmlFile::read(path)?;
        let doc = file.doc()?;
        let root = doc.root_element();
        if root.tag_name().name() != "level" {
            return Err(Error::format(path, "root is not <level>"));
        }
        Level::from_node(path, root, None)
    }

    /// Parses a `<level>` element. `default_size` (pixels) applies when the
    /// element has no `width`/`height` (Workshop levels: 320×240).
    pub fn from_node(path: &Path, root: Node, default_size: Option<(u32, u32)>) -> Result<Level> {
        let (dw, dh) = default_size.unwrap_or((320, 240));
        let width = xml::attr_parse(root, "width").unwrap_or(dw);
        let height = xml::attr_parse(root, "height").unwrap_or(dh);
        if width % CELL != 0 || height % CELL != 0 || width == 0 || height == 0 {
            return Err(Error::format(
                path,
                format!("odd level size {width}x{height}"),
            ));
        }
        let (cw, ch) = ((width / CELL) as usize, (height / CELL) as usize);
        let attrs = root
            .attributes()
            .filter(|a| a.name() != "width" && a.name() != "height")
            .map(|a| (a.name().to_owned(), a.value().to_owned()))
            .collect();
        let mut layers = Vec::new();
        let mut entities = Vec::new();
        for el in xml::elements(root) {
            let name = el.tag_name().name().to_owned();
            let export_mode = el.attribute("exportMode").unwrap_or("").to_owned();
            let text = xml::inner_text(el);
            let data = if name == "Entities" {
                for e in xml::elements(el) {
                    entities.push(parse_entity(path, e)?);
                }
                LayerData::Entities
            } else if export_mode == "Bitstring"
                || (export_mode.is_empty() && matches!(name.as_str(), "Solids" | "BG"))
            {
                LayerData::Bits(BitGrid::from_bitstring(&text, cw, ch))
            } else if export_mode.contains("CSV")
                || (export_mode.is_empty() && name.ends_with("Tiles"))
            {
                LayerData::Tiles(TileGrid::from_trimmed_csv(&text, cw, ch))
            } else if export_mode.is_empty() {
                LayerData::Text(text.trim().to_owned())
            } else {
                return Err(Error::format(
                    path,
                    format!("layer {name}: export mode {export_mode} not supported"),
                ));
            };
            layers.push(Layer {
                name,
                export_mode,
                tileset: el.attribute("tileset").map(str::to_owned),
                data,
            });
        }
        let bits = |n: &str| {
            layers
                .iter()
                .find(|l| l.name == n)
                .and_then(|l| match &l.data {
                    LayerData::Bits(b) => Some(b.clone()),
                    _ => None,
                })
        };
        let tiles = |n: &str| {
            layers
                .iter()
                .find(|l| l.name == n)
                .and_then(|l| match &l.data {
                    LayerData::Tiles(t) => Some(t.clone()),
                    _ => None,
                })
        };
        Ok(Level {
            path: path.to_owned(),
            width,
            height,
            attrs,
            solids: bits("Solids").ok_or_else(|| Error::format(path, "no Solids layer"))?,
            bg: bits("BG").unwrap_or_else(|| BitGrid::new(cw, ch)),
            solid_tiles: tiles("SolidTiles").unwrap_or_else(|| TileGrid::new(cw, ch)),
            bg_tiles: tiles("BGTiles").unwrap_or_else(|| TileGrid::new(cw, ch)),
            layers,
            entities,
        })
    }

    pub fn cells(&self) -> (usize, usize) {
        ((self.width / CELL) as usize, (self.height / CELL) as usize)
    }

    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(|s| s.as_str())
    }

    pub fn entities_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Entity> + 'a {
        self.entities.iter().filter(move |e| e.name == name)
    }

    /// Free-for-all spawn count (`PlayerSpawn`).
    pub fn player_spawns(&self) -> usize {
        self.entities_named("PlayerSpawn").count()
    }

    /// Team spawns per side (`min(TeamSpawnA, TeamSpawnB)`).
    pub fn team_spawns(&self) -> usize {
        self.entities_named("TeamSpawnA")
            .count()
            .min(self.entities_named("TeamSpawnB").count())
    }
}

fn parse_entity(path: &Path, e: Node) -> Result<Entity> {
    let name = e.tag_name().name().to_owned();
    let num = |k: &str| xml::attr_f32(e, k);
    let x = num("x").ok_or_else(|| Error::format(path, format!("entity {name} without x")))?;
    let y = num("y").ok_or_else(|| Error::format(path, format!("entity {name} without y")))?;
    let attrs = e
        .attributes()
        .filter(|a| !matches!(a.name(), "id" | "x" | "y"))
        .map(|a| (a.name().to_owned(), a.value().to_owned()))
        .collect();
    let nodes = xml::children(e, "node")
        .filter_map(|n| Some((xml::attr_f32(n, "x")?, xml::attr_f32(n, "y")?)))
        .collect();
    Ok(Entity {
        id: xml::attr_parse(e, "id"),
        name,
        x,
        y,
        attrs,
        nodes,
    })
}

/// Every `*.oel` under `dir`, recursively, sorted.
pub fn find_oel_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "oel") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn every_oel_parses() {
        let mut n = 0;
        for inst in installs().iter() {
            let mut count = 0;
            for set in inst.content_sets() {
                for p in find_oel_files(&set.levels_dir()) {
                    let l = Level::load(&p).unwrap_or_else(|e| panic!("{e}"));
                    assert!(l.width == 320 || l.width == 420, "{p:?}");
                    assert_eq!(l.height, 240);
                    assert!(l.solids.count() > 0, "{p:?} has no solids");
                    for t in l.bg_tiles.cells.iter().chain(&l.solid_tiles.cells) {
                        assert!(*t >= -1 && *t < 400, "{p:?}: tile {t}");
                    }
                    count += 1;
                }
            }
            eprintln!("{}: {count} levels", inst.summary());
            n += count;
            match inst.kind {
                crate::install::InstallKind::Steam => assert_eq!(count, 296),
                crate::install::InstallKind::EightPlayer => assert_eq!(count, 193),
            }
        }
        let _ = n;
    }

    #[test]
    fn sacred_ground_first_level() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let p = steam
            .content
            .levels_dir()
            .join("Versus/00 - Sacred Ground/00.oel");
        let l = Level::load(&p).unwrap();
        assert_eq!(l.cells(), (32, 24));
        assert_eq!(l.player_spawns(), 4);
        assert_eq!(l.attr("WrapMode"), Some("Both"));
        // Top row of solids: "11111111111111100111111111111111".
        assert!(l.solids.get(0, 0) && !l.solids.get(15, 0) && !l.solids.get(16, 0));
        // BGTiles text starts with a newline, so its first line is row 1.
        assert_eq!(l.bg_tiles.get(4, 1), 54);
        assert_eq!(l.bg_tiles.get(4, 0), -1);
        let chain = l.entities_named("Chain").next().unwrap();
        assert_eq!(chain.attr_i32("height"), Some(50));
        assert_eq!((chain.x, chain.y), (160.0, 110.0));
        let quest = Level::load(&steam.content.levels_dir().join("Quest/00.oel")).unwrap();
        let sp = quest.entities_named("Spawner").next().unwrap();
        assert_eq!(sp.attr("name"), Some("TopLeft"));
        assert_eq!(sp.nodes.len(), 3);
    }
}
