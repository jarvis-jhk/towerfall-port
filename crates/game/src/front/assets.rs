//! Everything the menus draw and play, loaded from the player's install
//! (read only). Without game data the front end still runs with the debug
//! font, coloured boxes and the archer table of ui.md 6.5.

use std::collections::BTreeMap;
use std::sync::Arc;

use tfp_assets::atlas::{AtlasKind, LoadedAtlas};
use tfp_assets::sprite_data::SpriteData;
use tfp_render::{Gpu, Rect, Renderer, SpriteFont, SubTexture, debug_font, debug_font_image};

use crate::audio::SoundData;
use crate::content::{GpuAtlas, sound, sprite_font};

/// The menu-relevant part of an archer.
#[derive(Clone, Debug)]
pub struct Archer {
    pub name0: String,
    pub name1: String,
    pub color_a: [u8; 4],
    pub color_b: [u8; 4],
    pub not_joined: String,
    pub joined: String,
    #[allow(dead_code)]
    pub gem: String,
    /// Voice set index (audio.md 4).
    pub voice: usize,
    pub alt: Option<Box<Archer>>,
    pub secret: Option<Box<Archer>>,
    /// Needs the Dark World content (Red).
    pub dark_world: bool,
    /// `VictoryMusic` (the jingle is `"Victory" + this`).
    pub victory: String,
}

/// A versus tower for the map.
#[derive(Clone, Debug)]
pub struct Tower {
    pub name: String,
    pub icon: String,
    pub map_pos: (f32, f32),
    pub block: &'static str,
    pub procedural: bool,
    pub dark_world: bool,
    #[allow(dead_code)]
    pub theme: String,
    #[allow(dead_code)]
    pub levels: Vec<std::path::PathBuf>,
}

pub struct VoiceSet {
    pub ready: Vec<Arc<SoundData>>,
    pub deselect: Option<Arc<SoundData>>,
}

pub struct UiAssets {
    pub menu: Option<GpuAtlas>,
    /// The 8-player build's menu atlas (60x60 portraits, `title/eight`).
    pub menu8: Option<GpuAtlas>,
    pub main: Option<GpuAtlas>,
    pub sprites: Option<SpriteData>,
    pub font: SpriteFont,
    pub archers: Vec<Archer>,
    pub towers: Vec<Tower>,
    pub dark_world: bool,
    pub sounds: BTreeMap<&'static str, Arc<SoundData>>,
    pub voices: Vec<VoiceSet>,
    pub install: Option<tfp_assets::GameInstall>,
}

/// UI sound field -> WAV base name (audio.md 10).
const UI_SOUNDS: &[(&str, &str)] = &[
    ("ui_click", "UI_CLICK_ST"),
    ("ui_clickSpecial", "UI_CLICK_SPECIAL"),
    ("ui_clickSpecialAsc", "UI_CLICK_SPECIAL_ASC"),
    ("ui_clickBack", "UI_CLICKBACK_ST"),
    ("ui_move1", "UI_MOVE_1_ST"),
    ("ui_move2", "UI_MOVE_2_ST"),
    ("ui_moveVersus", "UI_MOVE_VERSUS_ST"),
    ("ui_moveRandMap", "UI_MOVE_RANDMAP"),
    ("ui_pause", "UI_PAUSE_ST"),
    ("ui_unpause", "UI_UNPAUSE_ST"),
    ("ui_selectVersus", "UI_SELECT_VERSUS_ST"),
    ("ui_startGame", "UI_STARTGAME_ST"),
    ("ui_startGameType", "UI_STARTGAMETYPE_ST"),
    ("ui_swipeIn", "UI_SWIPEIN_ST"),
    ("ui_swipeOut", "UI_SWIPEOUT_ST"),
    ("ui_characterLock", "UI_CHARACTER_LOCK"),
    (
        "ui_characterSelectSwipeOut",
        "UI_CHARACTERSELECT_SWIPEOUT_ST",
    ),
    ("ui_mapZoom", "UI_MAP_ZOOM_ST"),
    ("ui_subclickOn", "UI_SUBCLICK_ON"),
    ("ui_subclickOff", "UI_SUBCLICK_OFF"),
    ("ui_mapDark", "UI_MAP_DARK"),
    ("ui_mapCataclysm", "UI_MAP_CATACLYSM"),
    ("ui_altCostumeShift", "UI_ALTCOSTUME_SHIFT"),
    ("ui_seedSfx", "UI_SEED_SFX"),
    ("ui_invalid", "UI_INVALID"),
    ("ui_levelLock", "UI_LEVEL_LOCK"),
    ("sfx_trainingSuccess", "SFX_TRAINING_SUCCESS"),
    ("mmg", "MMG"),
];

/// Voice set prefixes by index (audio.md 4).
const VOICES: [&str; 14] = [
    "CHAR_B", "CHAR_C", "CHAR_D", "CHAR_A", "CHAR_F", "CHAR_E", "CHAR_H", "CHAR_G", "CHAR_I",
    "CHAR_E2", "CHAR_B2", "CHAR_A2", "CHAR_D2", "CHAR_I2",
];
const VOICE_FALLBACK: [usize; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 5, 0, 3, 2, 8];

/// ui.md 6.5, for running without game data.
const FALLBACK_ARCHERS: [(&str, &str, u32, u32); 9] = [
    ("VIGILANTE", "THIEF", 0x00B800, 0x45E045),
    ("LAST OF", "THE ORDER", 0x3C9FFC, 0x90C7F9),
    ("ASSASSIN", "PRINCE", 0xF878F8, 0xF8B8F8),
    ("TURNCLOAK", "SOLDIER", 0xEF8C21, 0xFFB15E),
    ("ANCIENT", "EXILE", 0xE5E5E5, 0xFFFFFF),
    ("FORGOTTEN", "MASTER", 0xF2FF00, 0xFFFF66),
    ("PRANCING", "PUPPET", 0x00FFF6, 0x6DFFF6),
    ("VICIOUS", "VESSEL", 0x7A42FF, 0xA77FFF),
    ("VAINGLORIOUS", "GHOUL", 0xD30000, 0xFF0000),
];

/// The versus map positions (ui.md 9.2) by theme name, used when the
/// theme data has none.
const BLOCKS: [&str; 16] = [
    "levelBlock",
    "levelBlock",
    "levelBlock",
    "levelBlock",
    "levelBlock",
    "levelBlock",
    "levelBlock",
    "levelBlock",
    "specialLevelBlock",
    "specialLevelBlock",
    "specialLevelBlock",
    "superSpecialLevelBlock",
    "darkLevelBlock",
    "darkLevelBlock",
    "darkLevelBlock",
    "vortexLevelBlock",
];

fn rgba(hex: u32) -> [u8; 4] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255]
}

fn convert(a: &tfp_assets::game_data::ArcherDef, voice_fallback: usize) -> Archer {
    Archer {
        name0: a.name0.to_uppercase(),
        name1: a.name1.to_uppercase(),
        color_a: a.color_a,
        color_b: a.color_b,
        not_joined: a.portraits.not_joined.clone(),
        joined: a.portraits.joined.clone(),
        gem: a.gem_menu.clone(),
        voice: if a.sfx >= 0 {
            a.sfx as usize
        } else {
            voice_fallback
        },
        alt: None,
        secret: None,
        dark_world: false,
        victory: a.victory_music.clone(),
    }
}

impl UiAssets {
    pub fn load(gpu: &Gpu, renderer: &mut Renderer, no_assets: bool) -> Self {
        let font_tex = renderer.create_texture(gpu, &debug_font_image());
        let mut ui = Self::placeholder(font_tex);
        if no_assets {
            return ui;
        }
        let installs = tfp_assets::Installs::discover();
        let Some(install) = installs.steam.clone().or(installs.eight_player.clone()) else {
            log::warn!("menus: game data not found; placeholders");
            return ui;
        };
        if let Err(e) = ui.load_install(gpu, renderer, &install, installs.eight_player.as_ref()) {
            log::warn!("menus: loading game data: {e:#}");
        }
        ui.install = Some(install);
        ui
    }

    /// Frees the uploaded atlases and font texture.
    pub fn destroy(&self, r: &mut tfp_render::Renderer) {
        for a in [&self.menu, &self.menu8, &self.main].into_iter().flatten() {
            r.destroy_texture(a.texture);
        }
        r.destroy_texture(self.font.texture);
    }

    /// No game data: debug font (on `font_tex`), the archer table, no art.
    pub fn placeholder(font_tex: tfp_render::TextureId) -> Self {
        UiAssets {
            menu: None,
            menu8: None,
            main: None,
            sprites: None,
            font: debug_font(font_tex),
            archers: FALLBACK_ARCHERS
                .iter()
                .enumerate()
                .map(|(i, &(n0, n1, a, b))| Archer {
                    name0: n0.into(),
                    name1: n1.into(),
                    color_a: rgba(a),
                    color_b: rgba(b),
                    not_joined: format!("portraits/notJoined{i}"),
                    joined: format!("portraits/joined{i}"),
                    gem: format!("Gem{i}"),
                    voice: i,
                    alt: None,
                    secret: None,
                    dark_world: i == 8,
                    victory: tfp_assets::game_data::ARCHER_COLORS[i].to_string(),
                })
                .collect(),
            towers: Vec::new(),
            dark_world: false,
            sounds: BTreeMap::new(),
            voices: Vec::new(),
            install: None,
        }
    }

    fn load_install(
        &mut self,
        gpu: &Gpu,
        renderer: &mut Renderer,
        install: &tfp_assets::GameInstall,
        eight: Option<&tfp_assets::GameInstall>,
    ) -> anyhow::Result<()> {
        self.dark_world = install.dark_world.is_some();
        let up =
            |r: &mut Renderer, p: Option<std::path::PathBuf>| -> anyhow::Result<Option<GpuAtlas>> {
                Ok(match p {
                    Some(p) => Some(GpuAtlas::upload(gpu, r, &LoadedAtlas::load(&p)?)),
                    None => None,
                })
            };
        self.menu = up(renderer, install.atlas_xml(AtlasKind::Menu))?;
        self.main = up(renderer, install.atlas_xml(AtlasKind::Main))?;
        if let Some(e) = eight
            && e.root != install.root
        {
            self.menu8 = up(renderer, e.atlas_xml(AtlasKind::Menu)).unwrap_or(None);
        }
        let sd = install.content.sprite_data_dir().join("menuSpriteData.xml");
        self.sprites = SpriteData::load(&sd).ok();
        if let Ok(f) =
            tfp_assets::xnb::SpriteFont::load(&install.content.misc_dir().join("Archer.xnb"))
        {
            renderer.destroy_texture(self.font.texture);
            self.font = sprite_font(gpu, renderer, &f);
        }

        // Archers (with alt / secret costumes).
        let gd = tfp_assets::game_data::GameData::load(&install.content)?;
        let ad = &gd.archers;
        self.archers = ad
            .archers
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let mut x = convert(a, i);
                x.dark_world = i == 8;
                if let Some(Some(alt)) = ad.alts.get(i) {
                    let mut al = convert(alt, x.voice);
                    al.dark_world = true;
                    x.alt = Some(Box::new(al));
                }
                if let Some(Some(sec)) = ad.secrets.get(i) {
                    x.secret = Some(Box::new(convert(sec, x.voice)));
                }
                x
            })
            .collect();

        // Versus towers, in index order, with theme names / icons / map spots.
        let index = tfp_assets::towers::TowerIndex::build(install)?;
        for (i, t) in index.versus.iter().enumerate() {
            let theme = gd.themes.iter().find(|th| th.id == t.theme);
            let name = theme.map(|th| th.name.to_uppercase()).unwrap_or_else(|| {
                t.dir_name
                    .split_once(" - ")
                    .map_or(t.dir_name.clone(), |(_, n)| n.to_uppercase())
            });
            self.towers.push(Tower {
                name,
                icon: theme
                    .map(|th| format!("towerIcons/{}", th.icon))
                    .unwrap_or_default(),
                map_pos: theme.map(|th| th.map_position).unwrap_or((240.0, 240.0)),
                block: if t.procedural {
                    "vortexLevelBlock"
                } else {
                    BLOCKS.get(i).copied().unwrap_or("levelBlock")
                },
                procedural: t.procedural,
                dark_world: i >= 12,
                theme: t.theme.clone(),
                levels: t.levels.clone(),
            });
        }

        // Sounds: base content, then Dark World content.
        let dirs: Vec<std::path::PathBuf> = std::iter::once(install.content.sfx_dir())
            .chain(install.dark_world.iter().map(|d| d.sfx_dir()))
            .collect();
        let find = |base: &str| -> Option<Arc<SoundData>> {
            dirs.iter().find_map(|d| {
                let p = d.join(format!("{base}.wav"));
                p.exists()
                    .then(|| tfp_assets::audio::Wav::load(&p).ok())
                    .flatten()
                    .map(|pcm| sound(&pcm))
            })
        };
        for &(field, base) in UI_SOUNDS {
            if let Some(s) = find(base) {
                self.sounds.insert(field, s);
            }
        }
        for (i, prefix) in VOICES.iter().enumerate() {
            let mut ready: Vec<_> = (1..=3)
                .filter_map(|n| find(&format!("{prefix}_READY_{n:02}")))
                .collect();
            if ready.is_empty()
                && let Some(s) = find(&format!("{prefix}_READY"))
            {
                ready.push(s);
            }
            let deselect = find(&format!("{prefix}_DESELECT"));
            let fb = VOICE_FALLBACK[i];
            if ready.is_empty() && fb < self.voices.len() {
                ready = self.voices[fb].ready.clone();
            }
            let deselect =
                deselect.or_else(|| self.voices.get(fb).and_then(|v| v.deselect.clone()));
            self.voices.push(VoiceSet { ready, deselect });
        }
        Ok(())
    }

    /// A menu-atlas region.
    pub fn m(&self, name: &str) -> Option<SubTexture> {
        self.menu.as_ref().and_then(|a| a.get(name))
    }

    /// From the 8-player menu atlas, else the Steam one.
    #[allow(dead_code)]
    pub fn m8(&self, name: &str) -> Option<SubTexture> {
        self.menu8
            .as_ref()
            .and_then(|a| a.get(name))
            .or_else(|| self.m(name))
    }

    /// A main-atlas region.
    pub fn g(&self, name: &str) -> Option<SubTexture> {
        self.main.as_ref().and_then(|a| a.get(name))
    }

    /// Short (60x60) portrait: the 8-player art, else the top of the tall one.
    pub fn short_portrait(&self, name: &str) -> Option<SubTexture> {
        if let Some(p) = self.menu8.as_ref().and_then(|a| a.get(name))
            && p.height() <= 60
        {
            return Some(p);
        }
        self.m(name)
            .map(|p| p.sub(0, 10, p.width(), 60.min(p.height())))
    }

    /// Frame `frame` of a menu sprite-data sprite (e.g. `Gem0`).
    pub fn sprite_frame(&self, id: &str, frame: usize) -> Option<(SubTexture, (f32, f32))> {
        let d = self.sprites.as_ref()?.get(id)?;
        let tex = self.menu.as_ref()?.regions.get(&d.texture).copied()?;
        let atlas = self.menu.as_ref()?.texture;
        let frames = d.frame_rects(tfp_assets::image::Rect::new(tex.x, tex.y, tex.w, tex.h));
        let r = frames.get(frame.min(frames.len().saturating_sub(1)))?;
        Some((
            SubTexture::new(atlas, Rect::new(r.x, r.y, r.w, r.h)),
            d.origin,
        ))
    }

    /// Animation frame index of `anim` of sprite `id` at `tick` (60 Hz).
    pub fn anim_frame(&self, id: &str, anim: &str, tick: u64) -> usize {
        let Some(a) = self
            .sprites
            .as_ref()
            .and_then(|s| s.get(id))
            .and_then(|d| d.animation(anim))
        else {
            return 0;
        };
        if a.frames.is_empty() {
            return 0;
        }
        let per = (a.delay * 60.0).max(1.0) as u64;
        let i = (tick / per) as usize;
        let i = if a.looped {
            i % a.frames.len()
        } else {
            i.min(a.frames.len() - 1)
        };
        a.frames[i].max(0) as usize
    }

    pub fn sound(&self, name: &str) -> Option<&Arc<SoundData>> {
        self.sounds.get(name)
    }
}
