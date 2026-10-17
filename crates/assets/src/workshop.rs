//! Workshop / custom towers (`*.tower`): one XML file holding the tower's
//! theme settings and all its levels (same layer format as `.oel`, plus a
//! `LoadSeed` per level). Custom tilesets, backgrounds and music are embedded
//! base64 (PNG / WAV). See `docs/formats/levels.md`.

use crate::autotile::AutotileData;
use crate::error::{Error, Result};
use crate::level::Level;
use crate::xml::{self, XmlFile};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct WorkshopLevel {
    /// Minimum players for free-for-all (`ffa`).
    pub ffa: u32,
    /// Usable for team matches.
    pub teams: bool,
    /// Seed of this level's `System.Random` (autotile picks, decorations).
    pub load_seed: i32,
    pub level: Level,
}

/// A custom tileset: a base64 PNG and its own autotile rules.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomTileset {
    pub png_base64: String,
    pub autotile: AutotileData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomBackground {
    pub png_base64: String,
    pub wavy: bool,
    pub wind: bool,
    pub fog: bool,
    pub rain: bool,
    pub water: bool,
    pub snow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkshopTower {
    pub path: PathBuf,
    pub title: String,
    pub author: String,
    /// 16×16 bitstring for the map icon (row-major).
    pub icon_data: String,
    /// Icon tile style (Normal, Secret, Ascension, Dark, Cataclysm, Random).
    pub icon_tile: String,
    pub map_position: Option<(f32, f32)>,
    pub players_ffa: Option<u32>,
    pub players_teams: Option<bool>,
    pub mode: String,
    /// One digit per pickup type (the game's `Pickups` order): how many
    /// "tickets" it gets in the treasure lottery.
    pub treasure_mask: Vec<u8>,
    pub treasure_arrow_chance: f32,
    pub tileset: String,
    pub bg_tileset: String,
    pub background: String,
    pub music: String,
    pub lanterns: String,
    pub dark_opacity: f32,
    pub cold: bool,
    pub world: String,
    pub custom_tileset: Option<CustomTileset>,
    pub custom_bg_tileset: Option<CustomTileset>,
    pub custom_background: Option<CustomBackground>,
    /// Base64 WAV.
    pub custom_music: Option<String>,
    pub levels: Vec<WorkshopLevel>,
}

impl WorkshopTower {
    pub fn load(path: &Path) -> Result<WorkshopTower> {
        let file = XmlFile::read(path)?;
        let doc = file.doc()?;
        let root = doc.root_element();
        if root.tag_name().name() != "tower" {
            return Err(Error::format(path, "root is not <tower>"));
        }
        let text = |k: &str| {
            xml::child_text(root, k)
                .map(|s| s.trim().to_owned())
                .unwrap_or_default()
        };
        let treasure = xml::child(root, "treasure");
        let treasure_mask = treasure
            .map(xml::inner_text)
            .unwrap_or_default()
            .trim()
            .chars()
            .map(|c| {
                c.to_digit(10)
                    .map(|d| d as u8)
                    .ok_or_else(|| Error::format(path, "bad treasure mask"))
            })
            .collect::<Result<Vec<_>>>()?;
        let tileset_of = |k: &str| -> Result<Option<CustomTileset>> {
            let Some(n) = xml::child(root, k) else {
                return Ok(None);
            };
            let autotile = match xml::child(n, "autotiling") {
                Some(a) => AutotileData::from_xml(a).map_err(|e| Error::format(path, e))?,
                None => AutotileData::default(),
            };
            Ok(Some(CustomTileset {
                png_base64: xml::child_text(n, "data").unwrap_or_default(),
                autotile,
            }))
        };
        let custom_background = xml::child(root, "custombg").map(|n| {
            let b = |k: &str| xml::child_bool(n, k).unwrap_or(false);
            CustomBackground {
                png_base64: xml::child_text(n, "data").unwrap_or_default(),
                wavy: b("wavy"),
                wind: b("wind"),
                fog: b("fog"),
                rain: b("rain"),
                water: b("water"),
                snow: b("snow"),
            }
        });
        let mut levels = Vec::new();
        if let Some(ls) = xml::child(root, "levels") {
            for (i, l) in xml::descendants(ls, "level").enumerate() {
                let level = Level::from_node(path, l, Some((320, 240)))
                    .map_err(|e| Error::format(path, format!("level {i}: {e}")))?;
                levels.push(WorkshopLevel {
                    ffa: xml::attr_parse(l, "ffa").unwrap_or(0),
                    teams: xml::attr_bool(l, "teams").unwrap_or(false),
                    load_seed: xml::child_parse(l, "LoadSeed").unwrap_or(0),
                    level,
                });
            }
        }
        let players = xml::child(root, "players");
        Ok(WorkshopTower {
            path: path.to_owned(),
            title: text("title"),
            author: text("author"),
            icon_data: xml::child(root, "icon")
                .and_then(|i| xml::child_text(i, "data"))
                .unwrap_or_default()
                .trim()
                .to_owned(),
            icon_tile: xml::child(root, "icon")
                .and_then(|i| xml::child_text(i, "tile"))
                .unwrap_or_default()
                .trim()
                .to_owned(),
            map_position: xml::child(root, "map")
                .and_then(|m| Some((xml::attr_f32(m, "x")?, xml::attr_f32(m, "y")?))),
            players_ffa: players.and_then(|p| xml::attr_parse(p, "ffa")),
            players_teams: players.and_then(|p| xml::attr_bool(p, "teams")),
            mode: text("mode"),
            treasure_mask,
            treasure_arrow_chance: treasure
                .and_then(|t| xml::attr_f32(t, "arrows"))
                .unwrap_or(0.6),
            tileset: text("tileset"),
            bg_tileset: text("bgtileset"),
            background: text("background"),
            music: text("music"),
            lanterns: text("lanterns"),
            dark_opacity: xml::child_f32(root, "darkOpacity").unwrap_or(0.0),
            cold: xml::child_bool(root, "cold").unwrap_or(false),
            world: text("world"),
            custom_tileset: tileset_of("customtileset")?,
            custom_bg_tileset: tileset_of("custombgtileset")?,
            custom_background,
            custom_music: xml::child_text(root, "custommusic").map(|s| s.trim().to_owned()),
            levels,
        })
    }
}

/// Every `.tower` file under `dir` (recursive, sorted).
pub fn find_tower_files(dir: &Path) -> Vec<PathBuf> {
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
            } else if p
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("tower"))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Standard base64 (line breaks and whitespace ignored), for the embedded
/// custom PNG/WAV data.
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn base64() {
        assert_eq!(decode_base64("aGVs\nbG8=").unwrap(), b"hello");
    }

    #[test]
    fn example_towers_parse() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let dir = require!(steam.workshop.as_ref(), "Workshop dir");
        let files = find_tower_files(dir);
        assert!(!files.is_empty());
        let mut levels = 0;
        for f in &files {
            let t = WorkshopTower::load(f).unwrap_or_else(|e| panic!("{e}"));
            assert!(!t.levels.is_empty(), "{f:?}");
            assert!(!t.title.is_empty());
            for l in &t.levels {
                assert_eq!(l.level.cells(), (32, 24));
                assert!(l.level.solids.count() > 0);
            }
            levels += t.levels.len();
        }
        eprintln!("{} workshop towers, {levels} levels", files.len());
        let sg = WorkshopTower::load(&dir.join("Examples/01- SacredGround.tower")).unwrap();
        assert_eq!(sg.title, "SACRED GROUND");
        assert_eq!(sg.levels[0].load_seed, 2043104004);
        assert_eq!(sg.treasure_mask.len(), 20);
        assert_eq!(sg.tileset, "SacredGround");
    }
}
