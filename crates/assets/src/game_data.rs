//! `Atlas/GameData/*.xml`: archers, themes, tilesets (autotile rules),
//! backgrounds and loading-screen tips. See `docs/formats/game-data.md`.
//!
//! Lookups by id are case-insensitive, as in the game.

use crate::autotile::AutotileData;
use crate::error::{Error, Result};
use crate::install::ContentSet;
use crate::xml::{self, XmlFile};
use roxmltree::Node;
use std::collections::BTreeMap;
use std::path::Path;

fn key(id: &str) -> String {
    id.to_ascii_lowercase()
}

// ---------------------------------------------------------------- tilesets

#[derive(Clone, Debug, PartialEq)]
pub struct TilesetDef {
    pub id: String,
    /// Main-atlas subtexture holding the 10×10 tiles, row-major.
    pub image: String,
    pub autotile: AutotileData,
}

pub fn load_tilesets(path: &Path) -> Result<Vec<TilesetDef>> {
    let file = XmlFile::read(path)?;
    let doc = file.doc()?;
    let mut out = Vec::new();
    for t in xml::descendants(doc.root_element(), "Tileset") {
        let id = t.attribute("id").unwrap_or("").to_owned();
        let autotile = AutotileData::from_xml(t)
            .map_err(|e| Error::format(path, format!("tileset {id}: {e}")))?;
        out.push(TilesetDef {
            image: t.attribute("image").unwrap_or("").to_owned(),
            id,
            autotile,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- themes

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeDef {
    pub id: String,
    pub name: String,
    /// `towerIcons/<icon>` in the menu atlas.
    pub icon: String,
    /// Normal, Secret, Ascension, Dark, Cataclysm, Random.
    pub tower_type: String,
    pub map_position: (f32, f32),
    /// Music cue name (empty = none).
    pub music: String,
    /// As written (default black); the game inverts it for its light pass.
    pub darkness_color: [u8; 4],
    pub darkness_opacity: f32,
    pub wind: i32,
    /// CathedralTorch, DarkHallsTorch, CavesTorch, DayMountainTorch, MoonstoneCrystal.
    pub lanterns: String,
    /// Normal or Dark.
    pub world: String,
    pub raining: bool,
    pub cold: bool,
    /// Key into bgData.
    pub background: String,
    pub tileset: String,
    pub bg_tileset: String,
    pub drill_particle_color: [u8; 4],
    pub cracked_block_color: [u8; 4],
    /// Per archer colour (Green, Blue, Pink, Orange, White, Yellow, Cyan,
    /// Purple, Red): opacity while invisible = 0.2 + 0.1 × value.
    pub player_invisibility: Option<[i32; 9]>,
}

pub const ARCHER_COLORS: [&str; 9] = [
    "Green", "Blue", "Pink", "Orange", "White", "Yellow", "Cyan", "Purple", "Red",
];

pub fn load_themes(path: &Path) -> Result<Vec<ThemeDef>> {
    let file = XmlFile::read(path)?;
    let doc = file.doc()?;
    let mut out = Vec::new();
    for t in xml::descendants(doc.root_element(), "Theme") {
        let text = |n: &str| {
            xml::child_text(t, n)
                .map(|s| s.trim().to_owned())
                .unwrap_or_default()
        };
        let inv = xml::child(t, "PlayerInvisibility").map(|p| {
            let mut v = [0; 9];
            for (i, c) in ARCHER_COLORS.iter().enumerate() {
                v[i] = xml::child_parse(p, c).unwrap_or(0);
            }
            v
        });
        out.push(ThemeDef {
            id: t.attribute("id").unwrap_or("").to_owned(),
            name: text("Name"),
            icon: text("Icon"),
            tower_type: text("TowerType"),
            map_position: xml::child_point(t, "MapPosition").unwrap_or((0.0, 0.0)),
            music: text("Music"),
            darkness_color: xml::child_color(t, "DarknessColor").unwrap_or([0, 0, 0, 255]),
            darkness_opacity: xml::child_f32(t, "DarknessOpacity").unwrap_or(0.0),
            wind: xml::child_parse(t, "Wind").unwrap_or(0),
            lanterns: text("Lanterns"),
            world: xml::child_text(t, "World")
                .map(|s| s.trim().to_owned())
                .unwrap_or("Normal".into()),
            raining: xml::child_bool(t, "Raining").unwrap_or(false),
            cold: xml::child_bool(t, "Cold").unwrap_or(false),
            background: text("Background"),
            tileset: text("Tileset"),
            bg_tileset: text("BGTileset"),
            drill_particle_color: xml::child_color(t, "DrillParticleColor")
                .unwrap_or([255, 0, 0, 255]),
            cracked_block_color: xml::child_color(t, "CrackedBlockColor")
                .unwrap_or([0x4e, 0xb1, 0xe9, 255]),
            player_invisibility: inv,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- backgrounds

/// One layer of a background or foreground (`Backdrop`, `ScrollLayer`,
/// `FadeLayer`, `WavyLayer`, `AnimatedLayer`, `NodeLayer`, special ones like
/// `SacredGroundMoonLayer`, …). Kept generic: kind, attributes, text (the
/// bg-atlas / bg-sprite name) and child layers (`Node`s, `Overtime` groups).
#[derive(Clone, Debug, PartialEq)]
pub struct BgLayer {
    pub kind: String,
    pub attrs: BTreeMap<String, String>,
    pub text: String,
    pub children: Vec<BgLayer>,
}

impl BgLayer {
    fn from_xml(n: Node) -> BgLayer {
        let mut text = String::new();
        for c in n.children() {
            if c.is_text() {
                text.push_str(c.text().unwrap_or(""));
            }
        }
        BgLayer {
            kind: n.tag_name().name().to_owned(),
            attrs: n
                .attributes()
                .map(|a| (a.name().to_owned(), a.value().to_owned()))
                .collect(),
            text: text.trim().to_owned(),
            children: xml::elements(n).map(BgLayer::from_xml).collect(),
        }
    }

    pub fn attr_f32(&self, k: &str) -> Option<f32> {
        self.attrs.get(k).and_then(|v| xml::parse_f32(v))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BgDef {
    pub id: String,
    /// `bgColor` of `<Background>` (RGBA), if given.
    pub bg_color: Option<[u8; 4]>,
    pub background: Vec<BgLayer>,
    pub foreground: Vec<BgLayer>,
}

pub fn load_backgrounds(path: &Path) -> Result<Vec<BgDef>> {
    let file = XmlFile::read(path)?;
    let doc = file.doc()?;
    let mut out = Vec::new();
    for b in xml::descendants(doc.root_element(), "BG") {
        let bgn = xml::child(b, "Background");
        let layers = |n: Option<Node>| {
            n.map(|n| xml::elements(n).map(BgLayer::from_xml).collect())
                .unwrap_or_default()
        };
        out.push(BgDef {
            id: b.attribute("id").unwrap_or("").to_owned(),
            bg_color: bgn
                .and_then(|n| n.attribute("bgColor"))
                .and_then(xml::parse_hex_color),
            background: layers(bgn),
            foreground: layers(xml::child(b, "Foreground")),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- archers

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ArcherSprites {
    pub body: String,
    pub head_normal: String,
    pub head_no_hat: String,
    pub head_crown: String,
    pub head_back: String,
    pub bow: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ArcherHat {
    /// Default, Wood, … (`Material`).
    pub material: String,
    pub normal: String,
    pub blue: String,
    pub red: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ArcherPortraits {
    pub not_joined: String,
    pub joined: String,
    pub win: String,
    pub lose: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ArcherBreathing {
    /// Frames between breaths; -1 = none.
    pub interval: i32,
    pub offset: (f32, f32),
    pub ducking_offset: (f32, f32),
}

/// One archer (a base archer, or an alt/secret variant with the base's
/// values filled in where the variant doesn't override them).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ArcherDef {
    pub name0: String,
    pub name1: String,
    pub color_a: [u8; 4],
    pub color_b: [u8; 4],
    pub lightbar_color: [u8; 4],
    /// Main-atlas subtexture.
    pub aimer: String,
    pub hair: bool,
    pub sfx: i32,
    /// corpseSpriteData id.
    pub corpse: String,
    pub start_no_hat: bool,
    pub victory_music: String,
    pub purple_particles: bool,
    pub sleep_head_frame: i32,
    pub gender: String,
    pub sprites: ArcherSprites,
    pub hat: Option<ArcherHat>,
    /// Menu-atlas subtextures.
    pub portraits: ArcherPortraits,
    pub statue_image: String,
    pub statue_glow: String,
    pub gem_menu: String,
    pub gem_gameplay: String,
    pub breathing: ArcherBreathing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TeamDef {
    pub color_a: [u8; 4],
    pub color_b: [u8; 4],
    pub aimer: String,
    pub victory_music: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArcherData {
    /// The base archers in select-screen order.
    pub archers: Vec<ArcherDef>,
    /// Same index as `archers`; `None` if that archer has no alt (Dark World).
    pub alts: Vec<Option<ArcherDef>>,
    pub secrets: Vec<Option<ArcherDef>>,
    pub blue_team: TeamDef,
    pub red_team: TeamDef,
    pub enemies: TeamDef,
}

fn archer_from_xml(x: Node, base: Option<&ArcherDef>) -> ArcherDef {
    let d = base.cloned().unwrap_or_else(|| ArcherDef {
        victory_music: "Team".into(),
        sleep_head_frame: -1,
        gender: "Female".into(),
        breathing: ArcherBreathing {
            interval: -1,
            ..Default::default()
        },
        ..Default::default()
    });
    let s = |n: Node, k: &str, def: &str| {
        xml::child_text(n, k)
            .map(|t| t.trim().to_owned())
            .unwrap_or_else(|| def.to_owned())
    };
    let mut a = ArcherDef {
        name0: s(x, "Name0", &d.name0),
        name1: s(x, "Name1", &d.name1),
        color_a: xml::child_color(x, "ColorA").unwrap_or(d.color_a),
        color_b: xml::child_color(x, "ColorB").unwrap_or(d.color_b),
        lightbar_color: xml::child_color(x, "LightbarColor").unwrap_or(d.lightbar_color),
        aimer: s(x, "Aimer", &d.aimer),
        hair: xml::child_bool(x, "Hair").unwrap_or(d.hair),
        sfx: xml::child_parse(x, "SFX").unwrap_or(d.sfx),
        corpse: s(x, "Corpse", &d.corpse),
        start_no_hat: xml::child_bool(x, "StartNoHat").unwrap_or(d.start_no_hat),
        victory_music: s(x, "VictoryMusic", &d.victory_music),
        purple_particles: xml::child_bool(x, "PurpleParticles").unwrap_or(d.purple_particles),
        sleep_head_frame: xml::child_parse(x, "SleepHeadFrame").unwrap_or(d.sleep_head_frame),
        gender: s(x, "Gender", &d.gender),
        ..d.clone()
    };
    if let Some(sp) = xml::child(x, "Sprites") {
        let o = &d.sprites;
        a.sprites = ArcherSprites {
            body: s(sp, "Body", &o.body),
            head_normal: s(sp, "HeadNormal", &o.head_normal),
            head_no_hat: s(sp, "HeadNoHat", &o.head_no_hat),
            head_crown: s(sp, "HeadCrown", &o.head_crown),
            head_back: s(sp, "HeadBack", &o.head_back),
            bow: s(sp, "Bow", &o.bow),
        };
    }
    if let Some(h) = xml::child(x, "Hat") {
        let o = d.hat.clone().unwrap_or_else(|| ArcherHat {
            material: "Default".into(),
            ..Default::default()
        });
        a.hat = Some(ArcherHat {
            material: s(h, "Material", &o.material),
            normal: s(h, "Normal", &o.normal),
            blue: s(h, "Blue", &o.blue),
            red: s(h, "Red", &o.red),
        });
    }
    if let Some(p) = xml::child(x, "Portraits") {
        let o = &d.portraits;
        a.portraits = ArcherPortraits {
            not_joined: s(p, "NotJoined", &o.not_joined),
            joined: s(p, "Joined", &o.joined),
            win: s(p, "Win", &o.win),
            lose: s(p, "Lose", &o.lose),
        };
    }
    if let Some(st) = xml::child(x, "Statue") {
        a.statue_image = s(st, "Image", &d.statue_image);
        a.statue_glow = s(st, "Glow", &d.statue_glow);
    }
    if let Some(g) = xml::child(x, "Gems") {
        a.gem_menu = s(g, "Menu", &d.gem_menu);
        a.gem_gameplay = s(g, "Gameplay", &d.gem_gameplay);
    }
    if let Some(b) = xml::child(x, "Breathing") {
        a.breathing = ArcherBreathing {
            interval: xml::child_parse(b, "Interval").unwrap_or(d.breathing.interval),
            offset: xml::child_point(b, "Offset").unwrap_or(d.breathing.offset),
            ducking_offset: xml::child_point(b, "DuckingOffset")
                .unwrap_or(d.breathing.ducking_offset),
        };
    }
    a
}

fn team_from_xml(path: &Path, root: Node, name: &str) -> Result<TeamDef> {
    let t = xml::child(root, name).ok_or_else(|| Error::format(path, format!("no <{name}>")))?;
    Ok(TeamDef {
        color_a: xml::child_color(t, "ColorA").unwrap_or([255; 4]),
        color_b: xml::child_color(t, "ColorB").unwrap_or([255; 4]),
        aimer: xml::child_text(t, "Aimer")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        victory_music: xml::child_text(t, "VictoryMusic")
            .unwrap_or("Team".into())
            .trim()
            .to_owned(),
    })
}

pub fn load_archers(path: &Path) -> Result<ArcherData> {
    let file = XmlFile::read(path)?;
    let doc = file.doc()?;
    let root = doc.root_element();
    let mut archers: Vec<ArcherDef> = Vec::new();
    let mut alts = Vec::new();
    let mut secrets = Vec::new();
    for el in xml::elements(root) {
        match el.tag_name().name() {
            "Archer" => {
                archers.push(archer_from_xml(el, None));
                alts.push(None);
                secrets.push(None);
            }
            "AltArcher" | "SecretArcher" => {
                let base = archers
                    .last()
                    .ok_or_else(|| Error::format(path, "variant before any <Archer>"))?;
                let v = archer_from_xml(el, Some(base));
                let slot = if el.tag_name().name() == "AltArcher" {
                    &mut alts
                } else {
                    &mut secrets
                };
                *slot.last_mut().unwrap() = Some(v);
            }
            _ => {}
        }
    }
    Ok(ArcherData {
        archers,
        alts,
        secrets,
        blue_team: team_from_xml(path, root, "BlueTeam")?,
        red_team: team_from_xml(path, root, "RedTeam")?,
        enemies: team_from_xml(path, root, "Enemies")?,
    })
}

// ---------------------------------------------------------------- tips

#[derive(Clone, Debug, PartialEq)]
pub struct Tip {
    /// Versus tips: shown from this many matches played on.
    pub matches: Option<i32>,
    /// Versus tips: menu-atlas picture.
    pub image: Option<String>,
    pub dir: Option<i32>,
    /// `|` separates lines.
    pub text: String,
}

pub fn load_tips(path: &Path) -> Result<Vec<Tip>> {
    let file = XmlFile::read(path)?;
    let doc = file.doc()?;
    Ok(xml::descendants(doc.root_element(), "tip")
        .map(|t| Tip {
            matches: xml::child_parse(t, "matches"),
            image: xml::child_text(t, "image").map(|s| s.trim().to_owned()),
            dir: xml::child_parse(t, "dir"),
            text: xml::child_text(t, "text")
                .unwrap_or_default()
                .trim()
                .to_owned(),
        })
        .collect())
}

// ---------------------------------------------------------------- bundle

/// All of `Atlas/GameData` for one install.
#[derive(Clone, Debug)]
pub struct GameData {
    pub tilesets: Vec<TilesetDef>,
    pub themes: Vec<ThemeDef>,
    pub backgrounds: Vec<BgDef>,
    pub archers: ArcherData,
    pub versus_tips: Vec<Tip>,
    pub quest_tips: Vec<Tip>,
    tileset_index: BTreeMap<String, usize>,
    theme_index: BTreeMap<String, usize>,
    bg_index: BTreeMap<String, usize>,
}

impl GameData {
    pub fn load(content: &ContentSet) -> Result<Self> {
        let dir = content.game_data_dir();
        let tilesets = load_tilesets(&dir.join("tilesetData.xml"))?;
        let themes = load_themes(&dir.join("themeData.xml"))?;
        let backgrounds = load_backgrounds(&dir.join("bgData.xml"))?;
        let index = |ids: Vec<&str>| {
            ids.into_iter()
                .enumerate()
                .map(|(i, id)| (key(id), i))
                .collect()
        };
        Ok(GameData {
            tileset_index: index(tilesets.iter().map(|t| t.id.as_str()).collect()),
            theme_index: index(themes.iter().map(|t| t.id.as_str()).collect()),
            bg_index: index(backgrounds.iter().map(|t| t.id.as_str()).collect()),
            archers: load_archers(&dir.join("archerData.xml"))?,
            versus_tips: load_tips(&dir.join("versusTips.xml"))?,
            quest_tips: load_tips(&dir.join("questTips.xml"))?,
            tilesets,
            themes,
            backgrounds,
        })
    }

    pub fn tileset(&self, id: &str) -> Option<&TilesetDef> {
        self.tileset_index.get(&key(id)).map(|&i| &self.tilesets[i])
    }

    pub fn theme(&self, id: &str) -> Option<&ThemeDef> {
        self.theme_index.get(&key(id)).map(|&i| &self.themes[i])
    }

    pub fn background(&self, id: &str) -> Option<&BgDef> {
        self.bg_index.get(&key(id)).map(|&i| &self.backgrounds[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{Atlas, AtlasKind};
    use crate::testutil::installs;

    #[test]
    fn game_data_is_consistent() {
        for inst in installs().iter() {
            let gd = GameData::load(&inst.content).unwrap();
            let main = Atlas::load(&inst.atlas_xml(AtlasKind::Main).unwrap()).unwrap();
            let menu = Atlas::load(&inst.atlas_xml(AtlasKind::Menu).unwrap()).unwrap();
            assert!(gd.tilesets.len() >= 30, "{}", gd.tilesets.len());
            for t in &gd.tilesets {
                assert!(main.contains(&t.image), "tileset image {}", t.image);
                assert!(!t.autotile.center.is_empty(), "{}", t.id);
            }
            for th in &gd.themes {
                assert!(
                    gd.tileset(&th.tileset).is_some(),
                    "{}: {}",
                    th.id,
                    th.tileset
                );
                assert!(
                    gd.tileset(&th.bg_tileset).is_some(),
                    "{}: {}",
                    th.id,
                    th.bg_tileset
                );
                assert!(
                    gd.background(&th.background).is_some(),
                    "{}: bg {}",
                    th.id,
                    th.background
                );
                assert!(
                    menu.contains(&format!("towerIcons/{}", th.icon)),
                    "{} icon",
                    th.id
                );
            }
            let sg = gd.theme("sacredground").unwrap();
            assert_eq!(sg.name, "SACRED GROUND");
            assert_eq!(sg.map_position, (71.0, 114.0));
            assert_eq!(gd.archers.archers.len(), 9);
            assert_eq!(gd.archers.archers[0].name1, "THIEF");
            let alt = gd.archers.alts[0].as_ref().unwrap();
            assert_eq!(alt.name1, "MASK");
            assert_eq!(
                alt.color_a, gd.archers.archers[0].color_a,
                "alt inherits colours"
            );
            for a in gd
                .archers
                .archers
                .iter()
                .chain(gd.archers.alts.iter().flatten())
            {
                assert!(main.contains(&a.aimer), "{}", a.aimer);
                assert!(menu.contains(&a.portraits.win), "{}", a.portraits.win);
            }
            assert!(gd.versus_tips.len() > 10);
            assert!(!gd.quest_tips.is_empty());
            assert!(gd.backgrounds.len() > 10);
        }
    }
}
