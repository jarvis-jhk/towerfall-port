//! Packed texture atlases: `<TextureAtlas imagePath=".."><SubTexture name x y
//! width height/>…` next to a PNG. See `docs/formats/atlas.md`.

use crate::error::{Error, Result};
use crate::image::{Rect, RgbaImage};
use crate::xml::{self, XmlFile};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The atlases the game loads (file stem in `Atlas/`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum AtlasKind {
    /// `atlas` — gameplay sprites, tilesets, details. The Dark World copy is
    /// a superset and replaces the base one when the expansion is present.
    Main,
    /// `menuAtlas` — menus, portraits, tower icons.
    Menu,
    /// `bgAtlas` — level backgrounds.
    Bg,
    /// `bossAtlas` — bosses (Dark World copy replaces the base one).
    Boss,
    /// `editorAtlas` — Dark World only (level editor / workshop UI).
    Editor,
}

impl AtlasKind {
    pub const ALL: [AtlasKind; 5] = [
        AtlasKind::Main,
        AtlasKind::Menu,
        AtlasKind::Bg,
        AtlasKind::Boss,
        AtlasKind::Editor,
    ];

    pub fn stem(self) -> &'static str {
        match self {
            AtlasKind::Main => "atlas",
            AtlasKind::Menu => "menuAtlas",
            AtlasKind::Bg => "bgAtlas",
            AtlasKind::Boss => "bossAtlas",
            AtlasKind::Editor => "editorAtlas",
        }
    }
}

/// An atlas description (names → rectangles) plus the path of its image.
/// The image itself is loaded on demand with [`Atlas::load_image`].
#[derive(Clone, Debug)]
pub struct Atlas {
    pub xml_path: PathBuf,
    pub image_path: PathBuf,
    pub subtextures: BTreeMap<String, Rect>,
}

impl Atlas {
    /// Reads `<dir>/<stem>.xml`; the image is `<dir>/<stem>.png` (the game
    /// ignores the `imagePath` attribute and pairs files by name).
    pub fn load(xml_path: &Path) -> Result<Self> {
        let file = XmlFile::read(xml_path)?;
        let doc = file.doc()?;
        let root = doc.root_element();
        if root.tag_name().name() != "TextureAtlas" {
            return Err(Error::format(xml_path, "root is not <TextureAtlas>"));
        }
        let mut subtextures = BTreeMap::new();
        for st in xml::descendants(root, "SubTexture") {
            let name = st
                .attribute("name")
                .ok_or_else(|| Error::format(xml_path, "SubTexture without name"))?;
            let num = |a: &str| {
                xml::attr_parse::<i32>(st, a)
                    .ok_or_else(|| Error::format(xml_path, format!("SubTexture {name}: bad {a}")))
            };
            let r = Rect::new(num("x")?, num("y")?, num("width")?, num("height")?);
            if subtextures.insert(name.to_owned(), r).is_some() {
                return Err(Error::format(
                    xml_path,
                    format!("duplicate SubTexture {name}"),
                ));
            }
        }
        Ok(Atlas {
            xml_path: xml_path.to_owned(),
            image_path: xml_path.with_extension("png"),
            subtextures,
        })
    }

    pub fn get(&self, name: &str) -> Option<Rect> {
        self.subtextures.get(name).copied()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.subtextures.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.subtextures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.subtextures.is_empty()
    }

    pub fn load_image(&self) -> Result<RgbaImage> {
        RgbaImage::load_png(&self.image_path)
    }
}

/// An atlas together with its decoded image.
pub struct LoadedAtlas {
    pub atlas: Atlas,
    pub image: RgbaImage,
}

impl LoadedAtlas {
    pub fn load(xml_path: &Path) -> Result<Self> {
        let atlas = Atlas::load(xml_path)?;
        let image = atlas.load_image()?;
        for (name, r) in &atlas.subtextures {
            if r.x < 0
                || r.y < 0
                || (r.x + r.w) as u32 > image.width
                || (r.y + r.h) as u32 > image.height
            {
                return Err(Error::format(
                    xml_path,
                    format!("SubTexture {name} {r:?} outside the {image:?}"),
                ));
            }
        }
        Ok(LoadedAtlas { atlas, image })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn every_atlas_loads_and_rects_fit() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let mut total = 0;
        for set in steam.content_sets() {
            for kind in AtlasKind::ALL {
                let p = set.atlas_xml(kind);
                if !p.exists() {
                    continue;
                }
                let a = LoadedAtlas::load(&p).unwrap();
                assert!(!a.atlas.is_empty(), "{p:?}");
                total += a.atlas.len();
            }
        }
        assert!(total > 2000, "only {total} subtextures");
    }

    #[test]
    fn dark_world_atlas_is_a_superset() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let dw = require!(steam.dark_world.as_ref(), "Dark World content");
        let base = Atlas::load(&steam.content.atlas_xml(AtlasKind::Main)).unwrap();
        let dwa = Atlas::load(&dw.atlas_xml(AtlasKind::Main)).unwrap();
        let missing: Vec<_> = base
            .subtextures
            .keys()
            .filter(|k| !dwa.contains(k))
            .collect();
        assert!(missing.is_empty(), "missing from DW atlas: {missing:?}");
        assert!(dwa.contains("tilesets/sacredGround"));
    }
}
