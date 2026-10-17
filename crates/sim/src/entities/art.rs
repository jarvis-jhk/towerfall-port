//! Atlas textures and sprite-data sprites of the tower entities, resolved
//! once when content is loaded (names from tower-entities.md).

use crate::content::{Content, SpriteId, TexId};
use std::collections::BTreeMap;
use tfp_assets::atlas::AtlasKind;

/// Main-atlas subtextures used by tower entities.
pub const TEXTURES: &[&str] = &[
    "movingTiles",
    "cataclysmMovingTiles",
    "shiftBlock",
    "shiftBlockFace",
    "shiftBlockFaceHorizontal",
    "shiftBlockFaceVertical",
    "loopPlatformTiles",
    "sensorBlock",
    "cataclysmSensorBlock",
    "sensorGem",
    "sensorLights",
    "crackedPlatform",
    "crackedWall",
    "crumbleBlockTiles",
    "crumbleWall",
    "crumbleWallLines",
    "moonGlassTiles",
    "graniteTiles",
    "redSwitchBlock",
    "blueSwitchBlock",
    "redSwitchLines",
    "blueSwitchLines",
    "jumpPadOff",
    "jumpPadOn",
    "ice",
    "mud",
    "hotCoals",
    "hotCoalsGreen",
    "details/snowDeposit",
    "spikeBall",
    "spikeBallChain",
    "orb",
    "lava",
    "lavaGreen",
    "lantern",
    "chain",
    "details/cobwebs",
    "details/lantern",
    "details/skullLantern",
    "details/torchLantern",
    "details/wingTorch",
    "details/wallMushroom",
    "details/bigMushroomBase",
    "details/bigMushroom",
    "details/bones",
    "details/pirateBannerA",
    "details/pirateBannerB",
    "details/pirateBannerC",
    "details/pirateBannerD",
];

/// Sprite-data ids used by tower entities.
pub const SPRITES: &[&str] = &[
    "ProximityBlock",
    "WaterDrop",
    "icicle",
    "ExplodingOrb",
    "ShipWindow",
    "KingReaperIntro",
    "WhiteArcherPrison",
    "ExplodingSpikeball",
    "RainDrops",
    "BGCrystal",
    "mudSplash",
    "Ogmo",
    "Dummy",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TowerArt {
    tex: BTreeMap<&'static str, TexId>,
    sprites: BTreeMap<&'static str, SpriteId>,
}

impl TowerArt {
    pub fn tex(&self, name: &str) -> Option<TexId> {
        self.tex.get(name).copied()
    }
    pub fn sprite(&self, name: &str) -> SpriteId {
        self.sprites.get(name).copied().unwrap_or(SpriteId::NONE)
    }
}

/// Register the art (call before `Content::set_texture_sizes`).
pub fn register(c: &mut Content, sprites: &tfp_assets::sprite_data::SpriteData) {
    let mut art = TowerArt::default();
    for &n in TEXTURES {
        let t = c.tex(AtlasKind::Main, n);
        art.tex.insert(n, t);
    }
    for &n in SPRITES {
        if let Some(d) = sprites.get(n) {
            let s = c.add_sprite(AtlasKind::Main, d);
            art.sprites.insert(n, s);
        }
    }
    c.tower_art = art;
}
