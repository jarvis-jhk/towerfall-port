//! Immutable game content the simulation reads: level geometry, sprite
//! animation tables (gameplay reads animation state in places) and archer
//! data. Built once from `tfp-assets` data (no file I/O here: callers load
//! the assets) or from an ASCII map for tests and sandboxes. Never cloned
//! per frame; share it behind an `Arc`.

use crate::components::AnimDef;
use crate::math::Vec2;
use std::collections::BTreeMap;
use tfp_assets::atlas::AtlasKind;

/// Index into [`Content::textures`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TexId(pub u16);

/// Index into [`Content::sprites`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpriteId(pub u16);

impl Default for SpriteId {
    fn default() -> Self {
        SpriteId::NONE
    }
}

impl SpriteId {
    pub const NONE: SpriteId = SpriteId(u16::MAX);
    pub fn is_none(self) -> bool {
        self == SpriteId::NONE
    }
}

/// An atlas subtexture, by name (the renderer resolves names to rects).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureRef {
    pub atlas: AtlasKind,
    pub name: String,
    /// Subtexture size in pixels, when known (see [`Content::set_texture_sizes`]).
    pub size: Option<(u16, u16)>,
}

/// A sprite: texture, frame grid, origin, animations, and the per-frame
/// tables archer sheets carry.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteDef {
    pub name: String,
    pub texture: TexId,
    /// Alternative team textures (`BlueTexture` / `RedTexture`).
    pub blue_texture: Option<TexId>,
    pub red_texture: Option<TexId>,
    /// 0 = the whole subtexture is one frame.
    pub frame_w: u16,
    pub frame_h: u16,
    pub origin: Vec2,
    pub position: Vec2,
    pub anims: Vec<AnimDef>,
    pub head_y_origins: Vec<i32>,
    pub head_x_origins: Vec<i32>,
    pub bow_x_offsets: Vec<i32>,
    pub bow_y_offsets: Vec<i32>,
    pub down_y: Option<f32>,
    pub slide_head: bool,
    pub hide_bow: bool,
}

impl SpriteDef {
    /// Animation index by id (`u8::MAX` if absent).
    pub fn anim(&self, id: &str) -> u8 {
        self.anims
            .iter()
            .position(|a| a.id == id)
            .map(|i| i as u8)
            .unwrap_or(u8::MAX)
    }
}

/// Body animations the archer selects (player.md §19.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BodyAnim {
    Stand,
    Run,
    Jump,
    Fall,
    Glide,
    Ledge,
    Dodge,
    Duck,
    SlideNoHat,
    SlideNormal,
    SlideCrown,
    StandAim,
    RunAim,
    JumpAim,
    FallAim,
    GlideAim,
}
pub const BODY_ANIM_IDS: [&str; 16] = [
    "stand",
    "run",
    "jump",
    "fall",
    "glide",
    "ledge",
    "dodge",
    "duck",
    "slide_nohat",
    "slide_normal",
    "slide_crown",
    "stand_aim",
    "run_aim",
    "jump_aim",
    "fall_aim",
    "glide_aim",
];

/// Head animations (player.md §19.3), in look order idle/up/down/back for
/// ground, jump and fall, then `duck`.
pub const HEAD_ANIM_IDS: [&str; 13] = [
    "idle",
    "lookUp",
    "lookDown",
    "lookBack",
    "idleJump",
    "lookUpJump",
    "lookDownJump",
    "lookBackJump",
    "idleFall",
    "lookUpFall",
    "lookDownFall",
    "lookBackFall",
    "duck",
];

pub const BOW_ANIM_IDS: [&str; 3] = ["idle", "drawn", "drawnEmpty"];

/// The physics material of a dropped hat (player.md §15.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HatMaterial {
    Default,
    Wood,
    Steel,
}

/// One playable archer (base, alt or secret) as the sim needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcherDef {
    pub name: String,
    pub body: SpriteId,
    pub head_normal: SpriteId,
    pub head_no_hat: SpriteId,
    pub head_crown: SpriteId,
    pub bow: SpriteId,
    pub corpse: SpriteId,
    /// Animation indices in `body`, by [`BodyAnim`].
    pub body_anims: [u8; 16],
    /// Per head sprite (normal, no hat, crown): animation indices by
    /// [`HEAD_ANIM_IDS`].
    pub head_anims: [[u8; 13]; 3],
    pub bow_anims: [u8; 3],
    pub corpse_anims: [u8; 6],
    pub color_a: [u8; 4],
    pub color_b: [u8; 4],
    pub aimer: Option<TexId>,
    pub start_no_hat: bool,
    pub hair: bool,
    pub hat_material: HatMaterial,
    /// Dropped-hat texture (normal team colour).
    pub hat_texture: Option<TexId>,
    /// Voice set (`CharacterSounds` index, audio.md 4) for `CharSound`
    /// events: the archer data's `SFX` (alts inherit their base's).
    pub voice: u8,
    /// `CharacterIndex` of the base archer (alts map to their base;
    /// kill stats count by it).
    pub base: u16,
}

impl ArcherDef {
    /// An archer with no art (tests, missing data): boxes in the view.
    pub fn placeholder(name: &str, color: [u8; 4]) -> ArcherDef {
        ArcherDef {
            name: name.into(),
            body: SpriteId::NONE,
            head_normal: SpriteId::NONE,
            head_no_hat: SpriteId::NONE,
            head_crown: SpriteId::NONE,
            bow: SpriteId::NONE,
            corpse: SpriteId::NONE,
            body_anims: [u8::MAX; 16],
            head_anims: [[u8::MAX; 13]; 3],
            bow_anims: [u8::MAX; 3],
            corpse_anims: [u8::MAX; 6],
            color_a: color,
            color_b: color,
            aimer: None,
            start_no_hat: false,
            hair: false,
            hat_material: HatMaterial::Default,
            hat_texture: None,
            voice: 0,
            base: 0,
        }
    }
}

pub const CORPSE_ANIM_IDS: [&str; 6] = ["ground", "fall", "pinned", "slouched", "flying", "ledge"];

/// A static jump-through platform from the level file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpThruDef {
    pub pos: Vec2,
    pub width: f32,
}

/// A static solid block (ASCII sandboxes: `J` jump-pad cells; level files
/// place their tower entities through [`LevelDef::tower`]). `jump_pad` tags
/// it `JumpPad` (ground jumps from it are super jumps).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidDef {
    pub pos: Vec2,
    pub w: f32,
    pub h: f32,
    pub jump_pad: bool,
}

/// One level's geometry and markers.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelDef {
    pub name: String,
    /// Pixels (320 Steam / 420 8-player).
    pub width: u32,
    pub height: u32,
    pub cells_x: usize,
    pub cells_y: usize,
    /// Row-major solid bits (`y * cells_x + x`).
    pub solids: Vec<bool>,
    pub jump_thrus: Vec<JumpThruDef>,
    pub static_solids: Vec<SolidDef>,
    pub player_spawns: Vec<Vec2>,
    pub team_spawns_a: Vec<Vec2>,
    pub team_spawns_b: Vec<Vec2>,
    /// `TreasureChest` element positions (file order).
    pub treasure: Vec<Vec2>,
    /// `BigTreasureChest` element positions (file order).
    pub big_treasure: Vec<Vec2>,
    /// `Level.LoadSeed` (Versus: sum of the path's UTF-16 units).
    pub load_seed: i32,
    /// Tower-entity placements, unsupported tags and theme flags.
    pub tower: crate::entities::TowerLevel,
    /// Co-op/Trials markers (spawn portals, dummies, events, ...).
    pub markers: Markers,
}

/// A `Spawner` marker: enemy portal (Quest / Dark World / Dark Portals).
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnerDef {
    pub name: String,
    pub pos: Vec2,
    pub nodes: Vec<Vec2>,
}

/// A Trials `Dummy` marker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DummyDef {
    pub pos: Vec2,
    /// +1 right, -1 left.
    pub facing: i8,
}

/// A `FloorMiasma` marker (Quest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloorMiasmaDef {
    pub pos: Vec2,
    pub width: f32,
    pub group: i32,
}

/// A `TreasureChest` / `BigTreasureChest` marker with its attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct ChestDef {
    pub pos: Vec2,
    pub big: bool,
    /// `Type`, `Mode`, `Treasure` attributes (Trials reads them).
    pub kind: String,
    pub mode: String,
    pub treasure: String,
}

/// Markers read by the co-op and Trials logic.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Markers {
    pub spawners: Vec<SpawnerDef>,
    pub dummies: Vec<DummyDef>,
    pub events: Vec<Vec2>,
    pub floor_miasma: Vec<FloorMiasmaDef>,
    pub king_intro: Option<Vec2>,
    pub chests: Vec<ChestDef>,
    /// Dark World: `BossMarker` (position + nodes), `BossMarkerB`
    /// positions and the `EndlessPortal` (exit portal) position.
    pub boss_markers: Vec<SpawnerDef>,
    pub boss_markers_b: Vec<Vec2>,
    pub endless_portal: Option<Vec2>,
}

impl Markers {
    fn from_level(lvl: &tfp_assets::level::Level) -> Markers {
        let mut m = Markers::default();
        for e in &lvl.entities {
            let pos = Vec2::new(e.x, e.y);
            match e.name.as_str() {
                "Spawner" => m.spawners.push(SpawnerDef {
                    name: e.attr("name").unwrap_or("").trim().to_owned(),
                    pos,
                    nodes: e.nodes.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
                }),
                "Dummy" => m.dummies.push(DummyDef {
                    pos,
                    facing: if e.attr("Facing") == Some("Left") {
                        -1
                    } else {
                        1
                    },
                }),
                "Event" => m.events.push(pos),
                "FloorMiasma" => m.floor_miasma.push(FloorMiasmaDef {
                    pos,
                    width: e.attr_f32("width").unwrap_or(10.0),
                    group: e.attr_i32("Group").unwrap_or(0),
                }),
                "KingIntro" => m.king_intro = Some(pos),
                "BossMarker" => m.boss_markers.push(SpawnerDef {
                    name: String::new(),
                    pos,
                    nodes: e.nodes.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
                }),
                "BossMarkerB" => m.boss_markers_b.push(pos),
                "EndlessPortal" => m.endless_portal = Some(pos),
                "TreasureChest" | "BigTreasureChest" => m.chests.push(ChestDef {
                    pos,
                    big: e.name == "BigTreasureChest",
                    kind: e.attr("Type").unwrap_or("Normal").to_owned(),
                    mode: e.attr("Mode").unwrap_or("Normal").to_owned(),
                    treasure: e.attr("Treasure").unwrap_or("Arrows").to_owned(),
                }),
                _ => {}
            }
        }
        m
    }
}

impl LevelDef {
    #[inline]
    pub fn solid(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.cells_x
            && (y as usize) < self.cells_y
            && self.solids[y as usize * self.cells_x + x as usize]
    }

    /// From an ASCII map: one character per 10 px cell; `#` solid, `J` a
    /// 10x10 jump-pad block, `-` a
    /// one-cell jump-thru (adjacent `-` merge), `P` player spawn, `A`/`B` team spawn, `S` spawn portal (named
    /// `S0`, `S1`, ... in reading order, at the cell centre), `D` Trials dummy
    /// standing on the cell below (centre
    /// bottom of the cell, like an Ogmo `PlayerSpawn` at the cell top-left +
    /// (5, 0)...), anything else empty.
    pub fn from_ascii(name: &str, rows: &[&str]) -> LevelDef {
        let cells_y = rows.len();
        let cells_x = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let mut solids = vec![false; cells_x * cells_y];
        let mut jump_thrus = vec![];
        let mut static_solids = vec![];
        let mut player_spawns = vec![];
        let mut team_spawns_a = vec![];
        let mut markers = Markers::default();
        let mut team_spawns_b = vec![];
        let mut treasure = vec![];
        let mut big_treasure = vec![];
        for (y, row) in rows.iter().enumerate() {
            let mut run: Option<usize> = None;
            let chars: Vec<char> = row.chars().collect();
            for x in 0..=cells_x {
                let c = chars.get(x).copied().unwrap_or(' ');
                if c == '-' {
                    run.get_or_insert(x);
                } else if let Some(s) = run.take() {
                    jump_thrus.push(JumpThruDef {
                        pos: Vec2::new(s as f32 * 10.0, y as f32 * 10.0),
                        width: (x - s) as f32 * 10.0,
                    });
                }
                if x >= cells_x {
                    continue;
                }
                match c {
                    '#' => solids[y * cells_x + x] = true,
                    'J' => static_solids.push(SolidDef {
                        pos: Vec2::new(x as f32 * 10.0, y as f32 * 10.0),
                        w: 10.0,
                        h: 10.0,
                        jump_pad: true,
                    }),
                    'P' => player_spawns.push(Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0)),
                    'S' => markers.spawners.push(SpawnerDef {
                        name: format!("S{}", markers.spawners.len()),
                        pos: Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0 + 5.0),
                        nodes: vec![],
                    }),
                    'D' => markers.dummies.push(DummyDef {
                        pos: Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0 - 10.0),
                        facing: if x * 2 < cells_x { 1 } else { -1 },
                    }),
                    'A' => team_spawns_a.push(Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0)),
                    'B' => team_spawns_b.push(Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0)),
                    'T' => treasure.push(Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0)),
                    'G' => big_treasure.push(Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0)),
                    'E' => {
                        markers.endless_portal =
                            Some(Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0 + 5.0))
                    }
                    'M' => markers.boss_markers.push(SpawnerDef {
                        name: String::new(),
                        pos: Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0 + 5.0),
                        nodes: vec![],
                    }),
                    'm' => markers
                        .boss_markers_b
                        .push(Vec2::new(x as f32 * 10.0 + 5.0, y as f32 * 10.0 + 5.0)),
                    _ => {}
                }
            }
        }
        LevelDef {
            name: name.into(),
            width: cells_x as u32 * 10,
            height: cells_y as u32 * 10,
            cells_x,
            cells_y,
            solids,
            jump_thrus,
            static_solids,
            player_spawns,
            team_spawns_a,
            team_spawns_b,
            treasure,
            big_treasure,
            load_seed: 0,
            tower: Default::default(),
            markers,
        }
    }

    /// From a parsed `.oel` level. `load_seed` comes from the caller (the
    /// Versus seed depends on the path string as the game builds it).
    pub fn from_level(name: &str, lvl: &tfp_assets::level::Level, load_seed: i32) -> LevelDef {
        let (cx, cy) = (lvl.solids.width, lvl.solids.height);
        let jump_thrus = vec![];
        let static_solids = vec![];
        let pts =
            |n: &str| -> Vec<Vec2> { lvl.entities_named(n).map(|e| Vec2::new(e.x, e.y)).collect() };
        let player_spawns = pts("PlayerSpawn");
        let team_spawns_a = pts("TeamSpawnA");
        let team_spawns_b = pts("TeamSpawnB");
        let treasure = pts("TreasureChest");
        let big_treasure = pts("BigTreasureChest");
        LevelDef {
            name: name.into(),
            width: lvl.width,
            height: lvl.height,
            cells_x: cx,
            cells_y: cy,
            solids: lvl.solids.cells.clone(),
            jump_thrus,
            static_solids,
            player_spawns,
            team_spawns_a,
            team_spawns_b,
            treasure,
            big_treasure,
            load_seed,
            tower: crate::entities::TowerLevel::from_level(lvl),
            markers: Markers::from_level(lvl),
        }
    }
}

/// Textures and sprites the sim refers to by id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Common {
    pub arrow: Option<TexId>,
    pub arrow_buried: Option<TexId>,
    pub crown: Option<TexId>,
    pub shield: SpriteId,
    pub wings: SpriteId,
    /// Art of the M5 items (arrow types, pick-ups, chests, explosions).
    pub items: crate::view_items::ItemArt,
}

/// Everything immutable. See the module docs.
#[derive(Clone, Debug)]
pub struct Content {
    pub levels: Vec<LevelDef>,
    pub textures: Vec<TextureRef>,
    pub sprites: Vec<SpriteDef>,
    pub archers: Vec<ArcherDef>,
    pub common: Common,
    /// Tower-entity art (`entities::art`).
    pub tower_art: crate::entities::art::TowerArt,
    /// Quest wave scripts (index = `QuestSettings::script`).
    pub quest_scripts: Vec<crate::quest::QuestScript>,
    /// Dark World towers (darkworld.md §2).
    pub dark_world: Vec<crate::darkworld::DwTower>,
    sprite_index: BTreeMap<String, SpriteId>,
    texture_index: BTreeMap<(AtlasKind, String), TexId>,
}

const PLACEHOLDER_COLORS: [[u8; 4]; 12] = [
    [0x00, 0xb8, 0x00, 255],
    [0x3c, 0x9f, 0xfc, 255],
    [0xf8, 0x78, 0xf8, 255],
    [0xef, 0x8c, 0x21, 255],
    [0xe5, 0xe5, 0xe5, 255],
    [0xf2, 0xff, 0x00, 255],
    [0x00, 0xff, 0xf6, 255],
    [0x7a, 0x42, 0xff, 255],
    [0xd3, 0x00, 0x00, 255],
    [0x80, 0x80, 0x80, 255],
    [0xa0, 0x60, 0x20, 255],
    [0x20, 0x60, 0xa0, 255],
];

impl Content {
    /// Content with the given levels and placeholder (art-less) archers.
    pub fn with_levels(levels: Vec<LevelDef>) -> Content {
        Content {
            levels,
            textures: vec![],
            sprites: vec![],
            archers: (0..12)
                .map(|i| ArcherDef::placeholder(&format!("archer{i}"), PLACEHOLDER_COLORS[i]))
                .collect(),
            common: Common {
                shield: SpriteId::NONE,
                wings: SpriteId::NONE,
                ..Default::default()
            },
            tower_art: Default::default(),
            quest_scripts: vec![],
            dark_world: vec![],
            sprite_index: BTreeMap::new(),
            texture_index: BTreeMap::new(),
        }
    }

    /// A sandbox from an ASCII map (tests).
    pub fn from_ascii(rows: &[&str]) -> Content {
        Content::with_levels(vec![LevelDef::from_ascii("ascii", rows)])
    }

    pub fn level(&self, i: usize) -> &LevelDef {
        &self.levels[i]
    }

    pub fn sprite(&self, id: SpriteId) -> Option<&SpriteDef> {
        self.sprites.get(id.0 as usize)
    }

    pub fn sprite_by_name(&self, name: &str) -> SpriteId {
        self.sprite_index
            .get(name)
            .copied()
            .unwrap_or(SpriteId::NONE)
    }

    pub fn texture(&self, id: TexId) -> &TextureRef {
        &self.textures[id.0 as usize]
    }

    pub fn tex(&mut self, atlas: AtlasKind, name: &str) -> TexId {
        if let Some(&t) = self.texture_index.get(&(atlas, name.to_owned())) {
            return t;
        }
        let id = TexId(self.textures.len() as u16);
        self.textures.push(TextureRef {
            atlas,
            name: name.to_owned(),
            size: None,
        });
        self.texture_index.insert((atlas, name.to_owned()), id);
        id
    }

    /// Record subtexture sizes from a loaded atlas (for centred origins).
    pub fn set_texture_sizes(&mut self, atlas: AtlasKind, a: &tfp_assets::atlas::Atlas) {
        for t in &mut self.textures {
            if t.atlas == atlas
                && let Some(r) = a.get(&t.name)
            {
                t.size = Some((r.w as u16, r.h as u16));
            }
        }
    }

    /// Import one sprite definition from sprite data.
    pub fn add_sprite(
        &mut self,
        atlas: AtlasKind,
        d: &tfp_assets::sprite_data::SpriteDef,
    ) -> SpriteId {
        if let Some(&s) = self.sprite_index.get(&d.id) {
            return s;
        }
        let ints = |k: &str| -> Vec<i32> {
            d.extra
                .get(k)
                .map(|t| t.split(',').filter_map(|v| v.trim().parse().ok()).collect())
                .unwrap_or_default()
        };
        let flag = |k: &str, def: bool| -> bool {
            d.extra
                .get(k)
                .map(|t| t.trim().eq_ignore_ascii_case("true"))
                .unwrap_or(def)
        };
        let texture = self.tex(atlas, &d.texture);
        let blue_texture = d.extra.get("BlueTexture").map(|t| self.tex(atlas, t));
        let red_texture = d.extra.get("RedTexture").map(|t| self.tex(atlas, t));
        let def = SpriteDef {
            name: d.id.clone(),
            texture,
            blue_texture,
            red_texture,
            frame_w: d.frame_width.unwrap_or(0) as u16,
            frame_h: d.frame_height.unwrap_or(0) as u16,
            origin: Vec2::new(d.origin.0, d.origin.1),
            position: Vec2::new(d.position.0, d.position.1),
            anims: d
                .animations
                .iter()
                .map(|a| AnimDef {
                    id: a.id.clone(),
                    delay: a.delay,
                    looped: a.looped,
                    frames: a.frames.clone(),
                })
                .collect(),
            head_y_origins: ints("HeadYOrigins"),
            head_x_origins: ints("HeadXOrigins"),
            bow_x_offsets: ints("BowXOffsets"),
            bow_y_offsets: ints("BowYOffsets"),
            down_y: d.extra.get("DownY").and_then(|t| t.trim().parse().ok()),
            slide_head: flag("SlideHead", true),
            hide_bow: flag("HideBow", false),
        };
        let id = SpriteId(self.sprites.len() as u16);
        self.sprite_index.insert(d.id.clone(), id);
        self.sprites.push(def);
        id
    }

    /// Fill sprites and archers from the game's sprite data and archer data.
    /// `sprite_data` is `spriteData.xml`, `corpse_data` `corpseSpriteData.xml`
    /// (both on the main atlas).
    pub fn load_archers(
        &mut self,
        sprite_data: &tfp_assets::sprite_data::SpriteData,
        corpse_data: Option<&tfp_assets::sprite_data::SpriteData>,
        archers: &tfp_assets::game_data::ArcherData,
    ) {
        let main = AtlasKind::Main;
        let sprite = |c: &mut Content, data: &tfp_assets::sprite_data::SpriteData, id: &str| {
            data.get(id)
                .map(|d| c.add_sprite(main, d))
                .unwrap_or(SpriteId::NONE)
        };
        self.common.arrow = Some(self.tex(main, "arrows/arrow"));
        self.common.arrow_buried = Some(self.tex(main, "arrows/arrowBuried"));
        self.common.crown = Some(self.tex(main, "player/hat/crown"));
        self.common.shield = sprite(self, sprite_data, "Shield");
        self.common.wings = sprite(self, sprite_data, "Wings");
        let mut list: Vec<(&tfp_assets::game_data::ArcherDef, u16)> = archers
            .archers
            .iter()
            .enumerate()
            .map(|(i, a)| (a, i as u16))
            .collect();
        list.extend(
            archers
                .alts
                .iter()
                .enumerate()
                .filter_map(|(i, a)| a.as_ref().map(|a| (a, i as u16))),
        );
        let mut out = vec![];
        for (a, base) in list {
            let body = sprite(self, sprite_data, &a.sprites.body);
            let head_normal = sprite(self, sprite_data, &a.sprites.head_normal);
            let head_no_hat = sprite(self, sprite_data, &a.sprites.head_no_hat);
            let head_crown = sprite(self, sprite_data, &a.sprites.head_crown);
            let bow = sprite(self, sprite_data, &a.sprites.bow);
            let corpse = corpse_data
                .map(|d| sprite(self, d, &a.corpse))
                .unwrap_or(SpriteId::NONE);
            let anims_of = |c: &Content, s: SpriteId, ids: &[&str]| -> Vec<u8> {
                ids.iter()
                    .map(|id| c.sprite(s).map(|d| d.anim(id)).unwrap_or(u8::MAX))
                    .collect()
            };
            let mut body_anims = [u8::MAX; 16];
            body_anims.copy_from_slice(&anims_of(self, body, &BODY_ANIM_IDS));
            let mut head_anims = [[u8::MAX; 13]; 3];
            for (k, h) in [head_normal, head_no_hat, head_crown]
                .into_iter()
                .enumerate()
            {
                head_anims[k].copy_from_slice(&anims_of(self, h, &HEAD_ANIM_IDS));
            }
            let mut bow_anims = [u8::MAX; 3];
            bow_anims.copy_from_slice(&anims_of(self, bow, &BOW_ANIM_IDS));
            let mut corpse_anims = [u8::MAX; 6];
            corpse_anims.copy_from_slice(&anims_of(self, corpse, &CORPSE_ANIM_IDS));
            let hat_material = match a.hat.as_ref().map(|h| h.material.as_str()) {
                Some("Wood") => HatMaterial::Wood,
                Some("Steel") => HatMaterial::Steel,
                _ => HatMaterial::Default,
            };
            let hat_texture = a
                .hat
                .as_ref()
                .filter(|h| !h.normal.is_empty())
                .map(|h| self.tex(main, &h.normal));
            let aimer = (!a.aimer.is_empty()).then(|| self.tex(main, &a.aimer));
            out.push(ArcherDef {
                name: format!("{} {}", a.name0, a.name1),
                body,
                head_normal,
                head_no_hat,
                head_crown,
                bow,
                corpse,
                body_anims,
                head_anims,
                bow_anims,
                corpse_anims,
                color_a: a.color_a,
                color_b: a.color_b,
                aimer,
                start_no_hat: a.start_no_hat,
                hair: a.hair,
                hat_material,
                hat_texture,
                voice: a.sfx.max(0) as u8,
                base,
            });
        }
        if !out.is_empty() {
            self.archers = out;
        }
    }
}
