//! Sprite definitions (`Atlas/SpriteData/*.xml`): which atlas subtexture a
//! sprite uses, how it is cut into frames, its origin and its animations.
//! See `docs/formats/sprite-data.md`.

use crate::error::{Error, Result};
use crate::image::Rect;
use crate::xml::{self, XmlFile};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The sprite data files and the atlas each one refers to.
pub const SPRITE_DATA_FILES: [(&str, crate::atlas::AtlasKind); 5] = [
    ("spriteData", crate::atlas::AtlasKind::Main),
    ("corpseSpriteData", crate::atlas::AtlasKind::Main),
    ("menuSpriteData", crate::atlas::AtlasKind::Menu),
    ("bgSpriteData", crate::atlas::AtlasKind::Bg),
    ("bossSpriteData", crate::atlas::AtlasKind::Boss),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpriteKind {
    /// `<image>`: a single (optionally clipped) picture.
    Image,
    /// `<sprite_int>`: animations keyed by integer ids.
    SpriteInt,
    /// `<sprite_string>`: animations keyed by names.
    SpriteString,
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    /// The id as written (an integer for `sprite_int`).
    pub id: String,
    /// Seconds per frame (the sprite timer accumulates the frame's delta
    /// time × rate); 0 = never advances.
    pub delay: f32,
    /// Defaults to `true`.
    pub looped: bool,
    /// Indices into the sprite's frame grid.
    pub frames: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteDef {
    pub id: String,
    pub kind: SpriteKind,
    /// Atlas subtexture name.
    pub texture: String,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub origin: (f32, f32),
    pub position: (f32, f32),
    /// Tint, RGBA (default white).
    pub color: [u8; 4],
    pub animations: Vec<Animation>,
    /// Other single-text children, e.g. the corpse `BlueTeam`/`RedTeam`/
    /// `Flash` alternative textures.
    pub extra: BTreeMap<String, String>,
}

impl SpriteDef {
    pub fn animation(&self, id: &str) -> Option<&Animation> {
        self.animations.iter().find(|a| a.id == id)
    }

    /// Cuts `tex` (the subtexture rectangle in its atlas) into frames:
    /// left to right, then top to bottom, as many whole frames as fit.
    pub fn frame_rects(&self, tex: Rect) -> Vec<Rect> {
        let (Some(fw), Some(fh)) = (self.frame_width, self.frame_height) else {
            return vec![tex];
        };
        let (fw, fh) = (fw as i32, fh as i32);
        if fw <= 0 || fh <= 0 {
            return Vec::new();
        }
        let nx = tex.w / fw;
        let ny = tex.h / fh;
        let mut v = Vec::with_capacity((nx * ny) as usize);
        for j in 0..ny {
            for i in 0..nx {
                v.push(Rect::new(tex.x + i * fw, tex.y + j * fh, fw, fh));
            }
        }
        v
    }
}

#[derive(Clone, Debug)]
pub struct SpriteData {
    pub path: PathBuf,
    /// In file order.
    pub sprites: Vec<SpriteDef>,
    index: BTreeMap<String, usize>,
}

impl SpriteData {
    pub fn load(path: &Path) -> Result<Self> {
        let file = XmlFile::read(path)?;
        let doc = file.doc()?;
        let root = doc.root_element();
        if root.tag_name().name() != "SpriteData" {
            return Err(Error::format(path, "root is not <SpriteData>"));
        }
        let mut sprites = Vec::new();
        let mut index = BTreeMap::new();
        for el in xml::elements(root) {
            let id = el
                .attribute("id")
                .ok_or_else(|| Error::format(path, "sprite without id"))?
                .to_owned();
            let kind = match el.tag_name().name() {
                "image" => SpriteKind::Image,
                "sprite_int" => SpriteKind::SpriteInt,
                "sprite_string" => SpriteKind::SpriteString,
                o => SpriteKind::Other(o.to_owned()),
            };
            let texture = xml::child_text(el, "Texture")
                .map(|t| t.trim().to_owned())
                .ok_or_else(|| Error::format(path, format!("sprite {id}: no Texture")))?;
            let mut animations = Vec::new();
            if let Some(anims) = xml::child(el, "Animations") {
                for a in xml::descendants(anims, "Anim") {
                    let aid = a.attribute("id").unwrap_or("").trim().to_owned();
                    let frames = xml::parse_csv_ints(a.attribute("frames").unwrap_or(""))
                        .map_err(|e| Error::format(path, format!("sprite {id} anim {aid}: {e}")))?;
                    animations.push(Animation {
                        id: aid,
                        delay: xml::attr_f32(a, "delay").unwrap_or(0.0),
                        looped: xml::attr_bool(a, "loop").unwrap_or(true),
                        frames,
                    });
                }
            }
            const KNOWN: [&str; 10] = [
                "Texture",
                "FrameWidth",
                "FrameHeight",
                "OriginX",
                "OriginY",
                "X",
                "Y",
                "Color",
                "Animations",
                "",
            ];
            let mut extra = BTreeMap::new();
            for c in xml::elements(el) {
                let n = c.tag_name().name();
                if !KNOWN.contains(&n) {
                    extra.insert(n.to_owned(), xml::inner_text(c).trim().to_owned());
                }
            }
            let def = SpriteDef {
                kind,
                texture,
                frame_width: xml::child_parse(el, "FrameWidth"),
                frame_height: xml::child_parse(el, "FrameHeight"),
                origin: (
                    xml::child_f32(el, "OriginX").unwrap_or(0.0),
                    xml::child_f32(el, "OriginY").unwrap_or(0.0),
                ),
                position: (
                    xml::child_f32(el, "X").unwrap_or(0.0),
                    xml::child_f32(el, "Y").unwrap_or(0.0),
                ),
                color: xml::child_color(el, "Color").unwrap_or([255; 4]),
                animations,
                extra,
                id: id.clone(),
            };
            if index.insert(id.clone(), sprites.len()).is_some() {
                return Err(Error::format(path, format!("duplicate sprite id {id}")));
            }
            sprites.push(def);
        }
        Ok(SpriteData {
            path: path.to_owned(),
            sprites,
            index,
        })
    }

    pub fn get(&self, id: &str) -> Option<&SpriteDef> {
        self.index.get(id).map(|&i| &self.sprites[i])
    }

    pub fn len(&self) -> usize {
        self.sprites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::Atlas;
    use crate::testutil::{installs, require};

    #[test]
    fn all_sprite_data_resolves_against_atlases() {
        let mut checked = 0;
        for inst in installs().iter() {
            for (stem, kind) in SPRITE_DATA_FILES {
                let p = inst.content.sprite_data_dir().join(format!("{stem}.xml"));
                let sd = SpriteData::load(&p).unwrap();
                let atlas = Atlas::load(&inst.atlas_xml(kind).unwrap()).unwrap();
                assert!(!sd.is_empty());
                let mut missing = Vec::new();
                for s in &sd.sprites {
                    let Some(r) = atlas.get(&s.texture) else {
                        missing.push(s.id.clone());
                        continue;
                    };
                    let n = s.frame_rects(r).len() as i32;
                    for a in &s.animations {
                        for &f in &a.frames {
                            // Data quirk: menu "DarkWorldTower" lists frame 9
                            // but its 220×249 sheet holds 3×3 frames of 56×74.
                            let known_quirk = s.id == "DarkWorldTower" && f == 9;
                            assert!(
                                f >= 0 && (f < n || known_quirk),
                                "{stem}/{}: frame {f} of {n}",
                                s.id
                            );
                        }
                    }
                    for t in s.extra.values().filter(|t| t.contains('/')) {
                        if !atlas.contains(t) {
                            // A few alternative team textures are listed but
                            // were never packed (the game doesn't load them).
                            eprintln!("{stem}/{}: extra texture {t} not in atlas", s.id);
                        }
                    }
                    checked += 1;
                }
                // Leftover "Warlord" heads reference a texture that was never
                // packed (the game never instantiates them).
                // Same for the unused "CataclysmBodyCrystals" boss part.
                missing.retain(|id| {
                    !id.starts_with("PlayerHeadWarlord") && id != "CataclysmBodyCrystals"
                });
                // The 8-player build's bgAtlas lacks a few textures its bg
                // sprite data names (Bubble, Leak3, IgnisFatuus…).
                if inst.kind == crate::install::InstallKind::EightPlayer && stem == "bgSpriteData" {
                    eprintln!("8p {stem}: textures missing for {missing:?}");
                    missing.clear();
                }
                assert!(
                    missing.is_empty(),
                    "{stem}: textures missing for {missing:?}"
                );
            }
        }
        if installs().steam.is_some() {
            assert!(checked > 300, "{checked}");
        }
    }

    #[test]
    fn gem_sprite_model() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let sd = SpriteData::load(&steam.content.sprite_data_dir().join("spriteData.xml")).unwrap();
        let gem = sd.get("Gem0").unwrap();
        assert_eq!(gem.kind, SpriteKind::SpriteInt);
        assert_eq!(gem.texture, "pickups/greenGem");
        assert_eq!((gem.frame_width, gem.frame_height), (Some(10), Some(10)));
        assert_eq!(gem.origin, (5.0, 5.0));
        let a = gem.animation("0").unwrap();
        assert!(a.looped);
        assert!((a.delay - 0.1).abs() < 1e-6);
        assert_eq!(a.frames.len(), 18);
    }
}
