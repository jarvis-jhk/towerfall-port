//! Level-file placements of tower entities (tower-entities.md §3, levels.md
//! §6.1): every `.oel` entity tag is classified as a sim entity, a marker
//! read by round logic, or unsupported (listed, never silently dropped).

use crate::math::Vec2;

/// The tower entity a placement creates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlacementKind {
    MovingPlatform,
    ShiftBlock,
    LoopPlatform,
    GhostPlatform,
    RotatePlatformsCenter,
    SensorBlock,
    PrismBlock,
    CrackedPlatform,
    CrackedWall,
    CrumbleBlock,
    CrumbleWall,
    MoonGlassBlock,
    GraniteBlock,
    RedSwitchBlock,
    BlueSwitchBlock,
    ProximityBlock,
    FakeWall,
    JumpPad,
    Ice,
    Mud,
    HotCoals,
    SnowClump,
    SpikeBall,
    Icicle,
    Orb,
    ExplodingOrb,
    Lantern,
    Chain,
    Cobwebs,
    BGLantern,
    BGCrystal,
    BGMushroom,
    BGBigMushroom,
    BGSkeleton,
    GhostShipWindow,
    PirateBanner,
    WaterDrop,
    RainDrops,
    KingIntro,
    WhiteArcherPrison,
    Dummy,
    Ogmo,
}

impl PlacementKind {
    /// Skipped by the NoHazards variant (tower-entities.md §3 gating).
    pub fn is_hazard(self) -> bool {
        use PlacementKind::*;
        matches!(
            self,
            Orb | ExplodingOrb | Icicle | MovingPlatform | ShiftBlock | SpikeBall | SensorBlock
        )
    }
}

/// How the loader treats an `.oel` tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagClass {
    Entity(PlacementKind),
    /// Read straight from the level by round logic (spawns, chests,
    /// portals, procedural rectangles, Quest/Dark World markers).
    Marker,
    /// Known editor-only or mode-only object this port does not create.
    Unsupported,
}

/// Classify an `.oel` entity tag (`Level.LoadEntity`).
pub fn classify(tag: &str) -> TagClass {
    use PlacementKind as K;
    let k = match tag {
        "MovingPlatform" => K::MovingPlatform,
        "ShiftBlock" => K::ShiftBlock,
        "LoopPlatform" => K::LoopPlatform,
        "GhostPlatform" => K::GhostPlatform,
        "RotatePlatformsCenter" => K::RotatePlatformsCenter,
        "SensorBlock" => K::SensorBlock,
        "PrismBlock" => K::PrismBlock,
        "CrackedPlatform" => K::CrackedPlatform,
        "CrackedWall" => K::CrackedWall,
        "CrumbleBlock" => K::CrumbleBlock,
        "CrumbleWall" => K::CrumbleWall,
        "MoonGlassBlock" => K::MoonGlassBlock,
        "GraniteBlock" | "GraniteBlockH" | "GraniteBlockV" => K::GraniteBlock,
        "RedSwitchBlock" => K::RedSwitchBlock,
        "BlueSwitchBlock" => K::BlueSwitchBlock,
        "ProximityBlock" => K::ProximityBlock,
        "FakeWall" => K::FakeWall,
        "JumpPad" => K::JumpPad,
        "Ice" => K::Ice,
        "Mud" => K::Mud,
        "HotCoals" => K::HotCoals,
        "SnowClump" | "SnowEd" => K::SnowClump,
        "SpikeBall" | "SpikeBallEd" | "ExplodingSpikeBallEd" => K::SpikeBall,
        "Icicle" => K::Icicle,
        "Orb" | "OrbEd" => K::Orb,
        "ExplodingOrb" => K::ExplodingOrb,
        "Lantern" => K::Lantern,
        "Chain" => K::Chain,
        "Cobwebs" => K::Cobwebs,
        "BGLantern" => K::BGLantern,
        "BGCrystal" => K::BGCrystal,
        "BGMushroom" | "MushroomEd" => K::BGMushroom,
        "BGBigMushroom" => K::BGBigMushroom,
        "BGSkeleton" => K::BGSkeleton,
        "GhostShipWindow" => K::GhostShipWindow,
        "PirateBanner" => K::PirateBanner,
        "WaterDrop" => K::WaterDrop,
        "RainDrops" => K::RainDrops,
        "KingIntro" => K::KingIntro,
        "WhiteArcherPrison" => K::WhiteArcherPrison,
        "Dummy" => K::Dummy,
        "Ogmo" => K::Ogmo,
        "PlayerSpawn" | "TeamSpawn" | "TeamSpawnA" | "TeamSpawnB" | "TreasureChest"
        | "BigTreasureChest" | "Spawner" | "Event" | "RandomBlock" | "EndlessPortal"
        | "BossMarker" | "BossMarkerB" | "FloorMiasma" => return TagClass::Marker,
        // Trials-only unlock statue (needs save data) and the unused Warlord pad.
        "YellowStatue" | "ChalicePad" => return TagClass::Unsupported,
        _ => return TagClass::Unsupported,
    };
    TagClass::Entity(k)
}

/// Movement direction of a `LoopPlatform`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dir4 {
    #[default]
    Right,
    Left,
    Up,
    Down,
}

/// One placed tower entity, in file order.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub kind: PlacementKind,
    pub pos: Vec2,
    /// `width`, `height` (0 when absent).
    pub size: Vec2,
    /// The first `<node>` (moving platforms, shift blocks, spike ball pivot).
    pub node: Option<Vec2>,
    /// `Explodes`, `Lit`, `Facing="Left"`, … (per kind).
    pub flag: bool,
    pub dir: Dir4,
    /// `RotatePlatformsCenter`: `Amount`, `Radius`, `Width`, `DegSpeed`.
    pub amount: i32,
    pub radius: f32,
    pub ring_width: f32,
    pub deg_speed: f32,
    /// `SnowEd` expands to a row; `MushroomEd` picks big/small: the loader
    /// resolves both into plain placements.
    pub editor: bool,
}

impl Placement {
    pub fn new(kind: PlacementKind, pos: Vec2, size: Vec2) -> Placement {
        Placement {
            kind,
            pos,
            size,
            node: None,
            flag: false,
            dir: Dir4::Right,
            amount: 0,
            radius: 0.0,
            ring_width: 0.0,
            deg_speed: 0.0,
            editor: false,
        }
    }
}

/// `Lanterns` type of a theme (BGTorch art).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LanternType {
    #[default]
    CathedralTorch,
    DarkHallsTorch,
    CavesTorch,
    DayMountainTorch,
    MoonstoneCrystal,
}

/// Theme flags that touch tower entities (tower-entities.md §13.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeInfo {
    pub crack_color: [u8; 4],
    /// Dark World palette (green fire/lava/explosions).
    pub dark: bool,
    /// Cataclysm tileset (moving platform / sensor block art).
    pub cataclysm_tiles: bool,
    pub lanterns: LanternType,
    pub wind: f32,
    pub cold: bool,
    pub raining: bool,
    /// `InvisibleOpacities`: invisible archers' target opacity per archer
    /// colour (Green, Blue, ..., Red).
    pub invisibility: [f32; 9],
}

impl Default for ThemeInfo {
    fn default() -> Self {
        ThemeInfo {
            crack_color: [0x4e, 0xb1, 0xe9, 0xff],
            dark: false,
            cataclysm_tiles: false,
            lanterns: LanternType::CathedralTorch,
            wind: 0.0,
            cold: false,
            raining: false,
            // The default theme object.
            invisibility: [0.4; 9],
        }
    }
}

impl ThemeInfo {
    /// From a `themeData.xml` theme.
    pub fn from_theme(t: &tfp_assets::game_data::ThemeDef) -> ThemeInfo {
        let lanterns = match t.lanterns.as_str() {
            "DarkHallsTorch" => LanternType::DarkHallsTorch,
            "CavesTorch" => LanternType::CavesTorch,
            "DayMountainTorch" => LanternType::DayMountainTorch,
            "MoonstoneCrystal" => LanternType::MoonstoneCrystal,
            _ => LanternType::CathedralTorch,
        };
        ThemeInfo {
            crack_color: t.cracked_block_color,
            dark: t.world.eq_ignore_ascii_case("dark"),
            cataclysm_tiles: t.tileset == "Cataclysm",
            lanterns,
            wind: t.wind as f32,
            cold: t.cold,
            raining: t.raining,
            invisibility: t
                .player_invisibility
                .map_or([0.2; 9], |v| v.map(|n| 0.2 + 0.1 * n as f32)),
        }
    }
}

/// The tower-entity part of a level definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TowerLevel {
    pub placements: Vec<Placement>,
    /// Tags of this level the port does not create (sorted, unique).
    pub unsupported: Vec<String>,
    pub theme: ThemeInfo,
}

impl TowerLevel {
    /// Read the placements of a parsed level (file order; editor helpers
    /// resolved against the solid grid).
    pub fn from_level(lvl: &tfp_assets::level::Level) -> TowerLevel {
        let mut out = TowerLevel::default();
        let (cw, ch) = lvl.cells();
        let solid = |x: f32, y: f32| -> bool {
            if x < 0.0 || y < 0.0 {
                return false;
            }
            let (cx, cy) = ((x / 10.0) as usize, (y / 10.0) as usize);
            cx < cw && cy < ch && lvl.solids.cells[cy * cw + cx]
        };
        for e in &lvl.entities {
            let kind = match classify(&e.name) {
                TagClass::Entity(k) => k,
                TagClass::Marker => continue,
                TagClass::Unsupported => {
                    if !out.unsupported.contains(&e.name) {
                        out.unsupported.push(e.name.clone());
                    }
                    continue;
                }
            };
            let pos = Vec2::new(e.x, e.y);
            let size = Vec2::new(
                e.attr_f32("width").unwrap_or(0.0),
                e.attr_f32("height").unwrap_or(0.0),
            );
            let node = e.nodes.first().map(|&(x, y)| Vec2::new(x, y));
            let mut p = Placement::new(kind, pos, size);
            p.node = node;
            match e.name.as_str() {
                "SnowEd" => {
                    let w = if size.x > 0.0 { size.x } else { 10.0 };
                    let mut x = 0.0;
                    while x < w {
                        let mut s = Placement::new(kind, pos + Vec2::new(x, 0.0), Vec2::ZERO);
                        s.editor = true;
                        out.placements.push(s);
                        x += 10.0;
                    }
                    continue;
                }
                "MushroomEd" => {
                    if !solid(e.x, e.y + 12.0) && solid(e.x, e.y + 22.0) {
                        p.kind = PlacementKind::BGBigMushroom;
                        p.pos.y += 20.0;
                    }
                    p.editor = true;
                }
                "SpikeBallEd" | "ExplodingSpikeBallEd" => {
                    p.flag = e.name == "ExplodingSpikeBallEd";
                    p.node = Some(pos + Vec2::new(0.0, 10.0));
                    p.pos = pos + Vec2::new(0.0, size.y);
                    p.editor = true;
                }
                "GraniteBlock" | "GraniteBlockH" | "GraniteBlockV" => {
                    if p.size.x <= 0.0 {
                        p.size.x = 10.0;
                    }
                    if p.size.y <= 0.0 {
                        p.size.y = 10.0;
                    }
                }
                _ => {}
            }
            match kind {
                PlacementKind::SpikeBall if !p.editor => {
                    p.flag = e.attr_bool("Explodes").unwrap_or(false);
                }
                PlacementKind::BGLantern => p.flag = e.attr_bool("Lit").unwrap_or(true),
                PlacementKind::Dummy => p.flag = e.attr("Facing") == Some("Left"),
                PlacementKind::PirateBanner => p.flag = solid(e.x, e.y),
                PlacementKind::LoopPlatform => {
                    p.dir = match e.attr("Direction") {
                        Some("Left") => Dir4::Left,
                        Some("Up") => Dir4::Up,
                        Some("Down") => Dir4::Down,
                        _ => Dir4::Right,
                    }
                }
                PlacementKind::RotatePlatformsCenter => {
                    p.amount = e.attr_i32("Amount").unwrap_or(3);
                    p.radius = e.attr_f32("Radius").unwrap_or(45.0);
                    p.ring_width = e.attr_f32("Width").unwrap_or(40.0);
                    p.deg_speed = e.attr_f32("DegSpeed").unwrap_or(0.6);
                }
                PlacementKind::Chain if p.size.y <= 0.0 => p.size.y = 10.0,
                _ => {}
            }
            out.placements.push(p);
        }
        out.unsupported.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisibility_table_defaults() {
        assert_eq!(ThemeInfo::default().invisibility, [0.4; 9]);
    }

    #[test]
    fn every_versus_tag_is_classified() {
        for t in [
            "PlayerSpawn",
            "TeamSpawnA",
            "TreasureChest",
            "Spawner",
            "Event",
            "RandomBlock",
        ] {
            assert_eq!(classify(t), TagClass::Marker, "{t}");
        }
        assert_eq!(
            classify("GraniteBlockV"),
            TagClass::Entity(PlacementKind::GraniteBlock)
        );
        assert_eq!(classify("Whatever"), TagClass::Unsupported);
    }
}
