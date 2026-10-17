//! Presentation data for a level: everything a renderer needs to draw and
//! animate a tower without knowing the game's XML — tile layers (with atlas
//! source rects), the theme's background and foreground elements, background
//! decorations placed by the level, and the darkness/light parameters.
//! GPU-free; behaviour of each element: `docs/spec/rendering.md` §6–7.
//!
//! Construction mirrors the original's load order so seeded choices match:
//! a `System.Random(load_seed)` builds the background, then the foreground
//! (`<Random>` groups and the elements' start positions/phases come from it),
//! then the level's decorations draw from the same stream. The background is
//! bit-exact with the game; decoration picks (cobweb frames, chain links…)
//! are deterministic per seed but can differ from the original where
//! gameplay entities between them consume draws we don't model.

use crate::atlas::{Atlas, AtlasKind};
use crate::dotnet_random::DotNetRandom;
use crate::error::{Error, Result};
use crate::game_data::{BgLayer, GameData, ThemeDef};
use crate::image::Rect;
use crate::install::GameInstall;
use crate::level::{CELL, Entity, Level};
use crate::level_tiles::{LevelTiles, tile_rect};
use crate::sprite_data::SpriteData;
use crate::xml::{self, TileGrid};
use std::path::Path;

/// Which sprite-data file a sprite id belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpriteSet {
    /// `spriteData.xml` (main atlas).
    Main,
    /// `bgSpriteData.xml` (bg atlas).
    Bg,
}

/// An atlas picture: atlas, subtexture name and its rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRef {
    pub atlas: AtlasKind,
    pub name: String,
    pub src: Rect,
}

// ------------------------------------------------------------- tiles

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedTile {
    /// Top-left in level pixels.
    pub x: i32,
    pub y: i32,
    pub id: i32,
    /// Source rectangle in the main atlas.
    pub src: Rect,
}

/// An extra picture baked into a tile layer after the tiles (alpha-blended).
#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    pub image: ImageRef,
    pub x: i32,
    pub y: i32,
}

/// One tile layer. Draw it into its own transparent W×240 image, copying each
/// tile opaquely (alpha included), then the stamps alpha-blended; composite
/// that image as a unit (BG layer at depth 10000 of layer −3, solid layer at
/// depth −150 of layer 0).
#[derive(Clone, Debug, PartialEq)]
pub struct TileLayer {
    pub tileset: String,
    /// Main-atlas subtexture of the tileset and its rectangle.
    pub image: String,
    pub sheet: Rect,
    pub ids: TileGrid,
    pub tiles: Vec<PlacedTile>,
    pub stamps: Vec<Stamp>,
}

// ------------------------------------------------------------- background

#[derive(Clone, Debug, PartialEq)]
pub struct BgNode {
    pub x: f32,
    pub y: f32,
    pub x_range: f32,
    pub y_range: f32,
    pub speed: f32,
    pub speed_range: f32,
    pub anim: i32,
    pub delay: i32,
    pub delay_range: i32,
}

/// A background/foreground element with its parameters resolved (defaults
/// applied) and, where the original randomises at construction, the values
/// it drew (`init_*`). Positions are screen pixels (identity camera).
#[derive(Clone, Debug, PartialEq)]
pub enum BgElement {
    /// Static image.
    Backdrop {
        image: ImageRef,
        x: f32,
        y: f32,
        opacity: f32,
    },
    /// Tiled image moving by `speed` px/frame, wrapping.
    Scroll {
        image: ImageRef,
        x: f32,
        y: f32,
        speed: (f32, f32),
        opacity: f32,
        tile_x: bool,
        tile_y: bool,
    },
    /// Image cut into strips that sway on a sine; `phase_step` per frame =
    /// 2π / wave_frames, strip phase offset = 2π × slice_add.
    Wavy {
        image: ImageRef,
        x: f32,
        y: f32,
        opacity: f32,
        wave_frames: i32,
        amplitude: f32,
        slice_size: i32,
        slice_add: f32,
        horizontal: bool,
    },
    /// A bg sprite looping animation 0.
    Animated {
        sprite: String,
        x: f32,
        y: f32,
        opacity: f32,
    },
    /// Pulsing sprite that hops around (x,y)±range at each loop.
    Fade {
        sprite: String,
        x: f32,
        y: f32,
        x_range: f32,
        y_range: f32,
        opacity: f32,
        opacity_range: f32,
        /// Sine advance per frame in cycles (×2π for radians).
        fade_speed: f32,
        init_frame: i32,
        init_phase: f32,
        init_pos: (f32, f32),
        init_rate: f32,
    },
    /// Sprite that fades in/out (CubeOut × max_alpha) then moves.
    FadeMove {
        sprite: String,
        x: f32,
        y: f32,
        x_range: f32,
        y_range: f32,
        max_alpha: f32,
        time_frames: f32,
        rate_range: f32,
        init_pos: (f32, f32),
        init_rate: f32,
        init_counter_add: f32,
    },
    /// A sprite flying node to node.
    Node {
        sprite: String,
        opacity: f32,
        end_warp: bool,
        nodes: Vec<BgNode>,
        init_pos: (f32, f32),
        init_node: usize,
        init_target: (f32, f32),
        init_speed: f32,
        init_delay: i32,
    },
    /// Sky flash; first strike after `init_delay` frames.
    LightningFlash { color: [u8; 4], init_delay: i32 },
    /// Sprite playing its animation once every `time_min + Next(time_add)` frames.
    Ocean {
        sprite: String,
        x: f32,
        y: f32,
        opacity: f32,
        time_min: i32,
        time_add: i32,
        init_delay: i32,
    },
    /// `moon` (or `broken` once the Moonstone is unlocked). The 8-player
    /// build only ships the broken pictures, so `image` falls back to them.
    SacredGroundMoon {
        x: f32,
        y: f32,
        image: ImageRef,
        broken: Option<ImageRef>,
    },
    FlightMoon {
        x: f32,
        y: f32,
        image: ImageRef,
        broken: Option<ImageRef>,
    },
    /// Sprite `Lightning` at y = 80; first strike after `init_delay` frames.
    AscensionLightning { init_delay: f32 },
    /// Drifting ghost ship (four sine waves, periods 2400/1200/300/600).
    GhostShip {
        image: ImageRef,
        x: f32,
        y: f32,
        scale: f32,
        init_phases: [f32; 4],
    },
    /// Cataclysm tunnel (16 rings of `vortex0`); `init_angles` per ring.
    Vortex {
        ring: ImageRef,
        init_angles: Vec<f32>,
        init_bezier: [(f32, f32); 3],
        next_bezier: [(f32, f32); 3],
    },
}

// ------------------------------------------------------------- decorations

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    /// A sprite from sprite data, playing `anim` from animation index
    /// `start_index` (random start where the original randomises).
    Sprite {
        set: SpriteSet,
        id: String,
        anim: String,
        start_index: u32,
        offset: (f32, f32),
        flip_x: bool,
    },
    /// One frame cell / picture. `origin` is subtracted from the position.
    Image {
        image: ImageRef,
        offset: (f32, f32),
        origin: (f32, f32),
        flip_x: bool,
        alpha: f32,
    },
    /// Big mushroom cap: drawn bottom-centre-justified in 4-px strips, strip
    /// `i` from the bottom shifted by `round(wiggle × i)` (wiggle 0 at rest).
    Strips { image: ImageRef, strip: i32 },
    /// Pirate banner: origin (10, 0) at the entity, drawn in 4-px rows; row
    /// `n` (top `y`) shifted right by `round(sin(phase + n·π/4) × amp × y/h)`
    /// with `amp = 2.5 + 0.4·s`; per frame `phase += 0.13 + 0.03·s`, where
    /// `s` is a 240-frame sine starting at 0.
    Banner { image: ImageRef, init_phase: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DecorationKind {
    /// `BGLantern`; extinguishable/relightable at run time.
    Torch {
        lit: bool,
    },
    /// `BGCrystal` (also `BGLantern` in Moonstone-crystal themes, at y + 5).
    Crystal,
    Mushroom,
    BigMushroom,
    Skeleton,
    Cobweb,
    SnowClump,
    RainDrops,
    WaterDrop,
    Chain,
    Lantern,
    PirateBanner {
        over_solids: bool,
    },
    ShipWindow,
    Ogmo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decoration {
    pub kind: DecorationKind,
    /// Entity position (level pixels).
    pub x: f32,
    pub y: f32,
    /// Scene layer (−3 = background layer, 0 = gameplay layer).
    pub layer: i32,
    /// Higher = further back within the layer.
    pub depth: i32,
    pub parts: Vec<Part>,
    pub light: Option<Light>,
    /// Index of the source element in [`LevelLook::entities`].
    pub entity: usize,
}

// ------------------------------------------------------------- lighting

#[derive(Clone, Debug, PartialEq)]
pub enum LightKind {
    /// From a tileset `<Lights>` entry.
    Tile,
    /// Attached to a decoration.
    Decoration,
}

/// A light-canvas source (`rendering.md` §7.2). `color` is the stored light
/// colour: black erases darkness, a coloured light stores the *inverted*
/// colour it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub color: [u8; 4],
    pub alpha: f32,
    /// Also drawn at ±W / ±240 near the edges (no diagonal copy).
    pub wrap: bool,
    pub kind: LightKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Light canvas clear colour: the theme's darkness colour inverted
    /// (white when the theme has none).
    pub clear_color: [u8; 4],
    /// Base darkness (`DarknessOpacity`): the shader's `Alpha`.
    pub alpha: f32,
    /// Static lights: tile lights and decorations' lights.
    pub lights: Vec<Light>,
}

impl Lighting {
    pub fn from_theme(theme: &ThemeDef) -> Lighting {
        let c = theme.darkness_color;
        Lighting {
            clear_color: [255 - c[0], 255 - c[1], 255 - c[2], 255],
            alpha: theme.darkness_opacity,
            lights: Vec::new(),
        }
    }
}

// ------------------------------------------------------------- the look

#[derive(Clone, Debug, PartialEq)]
pub struct LevelLook {
    pub width: u32,
    pub height: u32,
    pub load_seed: i32,
    pub theme: String,
    pub bg_tiles: TileLayer,
    pub solid_tiles: TileLayer,
    /// Drawn on the screen first (never darkened).
    pub background: Vec<BgElement>,
    /// Drawn over the gameplay, inside the darkened foreground image.
    pub foreground: Vec<BgElement>,
    pub decorations: Vec<Decoration>,
    pub lighting: Lighting,
    /// The level's entities after procedural generation (gameplay owners use
    /// these; decorations refer to them by index).
    pub entities: Vec<Entity>,
    /// The solid grid actually used (generated for procedural towers).
    pub solids: crate::xml::BitGrid,
    /// Background art named by the data but absent from this install's
    /// atlases (those elements are left out; the original would fail).
    pub missing: Vec<String>,
}

/// Everything needed to build looks for an install.
pub struct LookAssets {
    pub game_data: GameData,
    pub main_atlas: Atlas,
    pub bg_atlas: Atlas,
    pub sprites: SpriteData,
    pub bg_sprites: SpriteData,
}

impl LookAssets {
    pub fn load(inst: &GameInstall) -> Result<LookAssets> {
        let atlas = |k: AtlasKind| -> Result<Atlas> {
            let p = inst
                .atlas_xml(k)
                .ok_or_else(|| Error::format(&inst.root, format!("no {} atlas", k.stem())))?;
            Atlas::load(&p)
        };
        let sd =
            |n: &str| SpriteData::load(&inst.content.sprite_data_dir().join(format!("{n}.xml")));
        Ok(LookAssets {
            game_data: GameData::load(&inst.content)?,
            main_atlas: atlas(AtlasKind::Main)?,
            bg_atlas: atlas(AtlasKind::Bg)?,
            sprites: sd("spriteData")?,
            bg_sprites: sd("bgSpriteData")?,
        })
    }

    fn img(&self, kind: AtlasKind, name: &str) -> Option<ImageRef> {
        let atlas = match kind {
            AtlasKind::Bg => &self.bg_atlas,
            _ => &self.main_atlas,
        };
        Some(ImageRef {
            atlas: kind,
            name: name.to_owned(),
            src: atlas.get(name)?,
        })
    }

    fn anim_len(&self, set: SpriteSet, id: &str, anim: &str) -> i32 {
        let sd = match set {
            SpriteSet::Main => &self.sprites,
            SpriteSet::Bg => &self.bg_sprites,
        };
        sd.get(id)
            .and_then(|s| s.animation(anim))
            .map(|a| a.frames.len() as i32)
            .unwrap_or(1)
    }
}

/// Options that the original takes from the session / save.
#[derive(Clone, Debug, Default)]
pub struct LookOptions {
    /// Read `<Overtime>` background groups.
    pub overtime: bool,
    /// Procedural towers: run the generator with this seed (`None` = use the
    /// file's base layout). `teams` = Team Deathmatch.
    pub procedural: Option<(i32, bool)>,
    /// "Remove screen flash effects": drops `LightningFlash` elements.
    pub no_flashes: bool,
}

impl LevelLook {
    /// Builds the look of `level` in `theme` with the level's `load_seed`.
    pub fn build(
        a: &LookAssets,
        level: &Level,
        theme: &ThemeDef,
        load_seed: i32,
        opts: &LookOptions,
    ) -> Result<LevelLook> {
        let gd = &a.game_data;
        let missing = |what: String| Error::format(&level.path, format!("missing {what}"));
        let level = match opts.procedural {
            Some((seed, teams)) => {
                level.with_generated(&crate::procedural::generate(level, seed, teams))
            }
            None => level.clone(),
        };
        let solid_set = gd
            .tileset(&theme.tileset)
            .ok_or_else(|| missing(format!("tileset {}", theme.tileset)))?;
        let bg_set = gd
            .tileset(&theme.bg_tileset)
            .ok_or_else(|| missing(format!("tileset {}", theme.bg_tileset)))?;
        let tiles = LevelTiles::compute(&level, &solid_set.autotile, &bg_set.autotile, load_seed);
        let layer = |set: &crate::game_data::TilesetDef, ids: &TileGrid| -> Result<TileLayer> {
            let sheet = a
                .main_atlas
                .get(&set.image)
                .ok_or_else(|| missing(format!("atlas image {}", set.image)))?;
            let mut placed = Vec::new();
            for y in 0..ids.height {
                for x in 0..ids.width {
                    let id = ids.get(x, y);
                    if let Some(src) = tile_rect(sheet, id) {
                        placed.push(PlacedTile {
                            x: (x as u32 * CELL) as i32,
                            y: (y as u32 * CELL) as i32,
                            id,
                            src,
                        });
                    }
                }
            }
            Ok(TileLayer {
                tileset: set.id.clone(),
                image: set.image.clone(),
                sheet,
                ids: ids.clone(),
                tiles: placed,
                stamps: Vec::new(),
            })
        };
        let mut bg_tiles = layer(bg_set, &tiles.bg)?;
        if let Some(holder) = a.img(AtlasKind::Main, "details/orbHolder") {
            for e in level.entities_named("Orb") {
                bg_tiles.stamps.push(Stamp {
                    image: holder.clone(),
                    x: e.x as i32 - 8,
                    y: e.y as i32 + 6,
                });
            }
        }
        let solid_tiles = layer(solid_set, &tiles.solid)?;

        let mut lighting = Lighting::from_theme(theme);
        let mut tile_lights = |set: &crate::game_data::TilesetDef, ids: &TileGrid| {
            for l in &set.autotile.lights {
                for y in 0..ids.height {
                    for x in 0..ids.width {
                        if l.tiles.contains(&ids.get(x, y)) {
                            lighting.lights.push(Light {
                                x: (x * 10 + 5) as f32,
                                y: (y * 10 + 5) as f32,
                                radius: l.radius,
                                color: invert(l.color),
                                alpha: 1.0,
                                wrap: true,
                                kind: LightKind::Tile,
                            });
                        }
                    }
                }
            }
        };
        tile_lights(solid_set, &tiles.solid);
        tile_lights(bg_set, &tiles.bg_autotiled);

        let mut rng = DotNetRandom::new(load_seed);
        let bgdef = gd.background(&theme.background);
        let mut background = Vec::new();
        let mut foreground = Vec::new();
        let mut missing = Vec::new();
        if let Some(def) = bgdef {
            let mut ctx = Reader {
                a,
                opts,
                path: &level.path,
                missing: &mut missing,
            };
            ctx.read(&def.background, &mut rng, &mut background)?;
            ctx.read(&def.foreground, &mut rng, &mut foreground)?;
        }
        let decorations = decorations(a, &level, theme, &mut rng, &mut lighting);
        Ok(LevelLook {
            width: level.width,
            height: level.height,
            load_seed,
            theme: theme.id.clone(),
            bg_tiles,
            solid_tiles,
            background,
            foreground,
            decorations,
            lighting,
            solids: level.solids.clone(),
            entities: level.entities,
            missing,
        })
    }
}

fn invert(c: [u8; 4]) -> [u8; 4] {
    [255 - c[0], 255 - c[1], 255 - c[2], c[3]]
}

fn f(l: &BgLayer, k: &str, def: f32) -> f32 {
    l.attr_f32(k).unwrap_or(def)
}

fn i(l: &BgLayer, k: &str, def: i32) -> i32 {
    l.attrs
        .get(k)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(def)
}

fn b(l: &BgLayer, k: &str, def: bool) -> bool {
    l.attrs
        .get(k)
        .and_then(|v| xml::parse_bool(v))
        .unwrap_or(def)
}

/// `Range(Vector2 min, Vector2 add)`: x drawn first.
fn range_v(rng: &mut DotNetRandom, min: (f32, f32), add: (f32, f32)) -> (f32, f32) {
    let x = add.0 * rng.next_f32();
    let y = add.1 * rng.next_f32();
    (min.0 + x, min.1 + y)
}

struct Reader<'r> {
    a: &'r LookAssets,
    opts: &'r LookOptions,
    path: &'r Path,
    missing: &'r mut Vec<String>,
}

/// Marks a missing picture; the element is skipped (draws still happen).
struct Missing;

impl Reader<'_> {
    fn read(
        &mut self,
        layers: &[BgLayer],
        rng: &mut DotNetRandom,
        out: &mut Vec<BgElement>,
    ) -> Result<()> {
        let mut failed = false;
        for l in layers {
            match self.element(l, rng, &mut failed, out)? {
                Ok(Some(el)) => out.push(el),
                Ok(None) => {}
                Err(Missing) => {}
            }
        }
        Ok(())
    }

    fn element(
        &mut self,
        l: &BgLayer,
        rng: &mut DotNetRandom,
        failed: &mut bool,
        out: &mut Vec<BgElement>,
    ) -> Result<std::result::Result<Option<BgElement>, Missing>> {
        let (a, opts, path) = (self.a, self.opts, self.path);
        let x = f(l, "x", 0.0);
        let y = f(l, "y", 0.0);
        let mut miss: Option<String> = None;
        let mut bg_img = |name: &str| -> ImageRef {
            match a.img(AtlasKind::Bg, name) {
                Some(i) => i,
                None => {
                    miss.get_or_insert_with(|| name.to_owned());
                    ImageRef {
                        atlas: AtlasKind::Bg,
                        name: name.to_owned(),
                        src: Rect::default(),
                    }
                }
            }
        };
        let el = match l.kind.as_str() {
            "Backdrop" => BgElement::Backdrop {
                image: bg_img(&l.text),
                x,
                y,
                opacity: f(l, "opacity", 1.0),
            },
            "ScrollLayer" => BgElement::Scroll {
                image: bg_img(&l.text),
                x,
                y,
                speed: (f(l, "speedX", 0.0), f(l, "speedY", 0.0)),
                opacity: f(l, "opacity", 1.0),
                tile_x: b(l, "tileX", true),
                tile_y: b(l, "tileY", true),
            },
            "WavyLayer" => BgElement::Wavy {
                image: bg_img(&l.text),
                x,
                y,
                opacity: f(l, "opacity", 1.0),
                wave_frames: i(l, "waveFrames", 60),
                amplitude: f(l, "amplitude", 2.0),
                slice_size: i(l, "sliceSize", 10),
                slice_add: f(l, "sliceAdd", 0.125),
                horizontal: b(l, "horizontal", false),
            },
            "AnimatedLayer" => BgElement::Animated {
                sprite: l.text.clone(),
                x,
                y,
                opacity: f(l, "opacity", 0.0),
            },
            "FadeLayer" => {
                let init_frame = rng.next_max(a.anim_len(SpriteSet::Bg, &l.text, "0"));
                let init_phase = rng.next_f32() * std::f32::consts::PI * 2.0 * 2.0;
                let (xr, yr) = (f(l, "xRange", 0.0), f(l, "yRange", 0.0));
                let init_pos = range_v(rng, (x - xr, y - yr), (xr * 2.0, yr * 2.0));
                let init_rate = rng.range_f32(0.8, 0.4);
                BgElement::Fade {
                    sprite: l.text.clone(),
                    x,
                    y,
                    x_range: xr,
                    y_range: yr,
                    opacity: f(l, "opacity", 0.5),
                    opacity_range: f(l, "opacityRange", 0.25),
                    fade_speed: f(l, "fadeSpeed", 0.02),
                    init_frame,
                    init_phase,
                    init_pos,
                    init_rate,
                }
            }
            "FadeMoveLayer" => {
                let (xr, yr) = (f(l, "xRange", 0.0), f(l, "yRange", 0.0));
                let time_frames = f(l, "time", 1.0) * 60.0;
                let rate_range = f(l, "rateRange", 0.4);
                let init_pos = range_v(rng, (x - xr, y - yr), (xr * 2.0, yr * 2.0));
                let init_rate = rng.range_f32(1.0 - rate_range / 2.0, rate_range);
                let init_counter_add = 1.0 / time_frames * rng.range_f32(0.9, 1.1);
                BgElement::FadeMove {
                    sprite: l.text.clone(),
                    x,
                    y,
                    x_range: xr,
                    y_range: yr,
                    max_alpha: f(l, "maxAlpha", 1.0),
                    time_frames,
                    rate_range,
                    init_pos,
                    init_rate,
                    init_counter_add,
                }
            }
            "NodeLayer" => {
                let nodes: Vec<BgNode> = l
                    .children
                    .iter()
                    .filter(|c| c.kind == "Node")
                    .map(|c| BgNode {
                        x: f(c, "x", 0.0),
                        y: f(c, "y", 0.0),
                        x_range: f(c, "xRange", 0.0),
                        y_range: f(c, "yRange", 0.0),
                        speed: f(c, "speed", 1.0),
                        speed_range: f(c, "speedRange", 0.0),
                        anim: i(c, "animID", 0),
                        delay: i(c, "delay", 0),
                        delay_range: i(c, "delayRange", 0),
                    })
                    .collect();
                if nodes.is_empty() {
                    return Err(Error::format(path, "NodeLayer without nodes"));
                }
                let end_warp = b(l, "endWarp", true);
                let start = |rng: &mut DotNetRandom| {
                    let n = &nodes[0];
                    range_v(
                        rng,
                        (n.x - n.x_range / 2.0, n.y - n.y_range / 2.0),
                        (n.x_range, n.y_range),
                    )
                };
                let mut init_pos = start(rng);
                let mut cur = 0usize;
                let mut next = 1 % nodes.len();
                if next == 0 && end_warp {
                    cur = 0;
                    next = 1;
                    init_pos = start(rng);
                }
                let tn = &nodes[next.min(nodes.len() - 1)];
                let init_target = range_v(rng, (tn.x, tn.y), (tn.x_range, tn.y_range));
                let init_speed = rng.range_f32(nodes[cur].speed, nodes[cur].speed_range);
                let init_delay = rng.range_i32(nodes[cur].delay, nodes[cur].delay_range);
                BgElement::Node {
                    sprite: l.attrs.get("sprite").cloned().unwrap_or_default(),
                    opacity: f(l, "opacity", 1.0),
                    end_warp,
                    nodes,
                    init_pos,
                    init_node: cur,
                    init_target,
                    init_speed,
                    init_delay,
                }
            }
            "LightningFlash" => {
                if opts.no_flashes {
                    return Ok(Ok(None));
                }
                let color = l
                    .attrs
                    .get("color")
                    .and_then(|c| xml::parse_hex_color(c))
                    .unwrap_or([255; 4]);
                BgElement::LightningFlash {
                    color,
                    init_delay: rng.range_i32(180, 540),
                }
            }
            "OceanLayer" => {
                let (time_min, time_add) = (i(l, "timeMin", 120), i(l, "timeAdd", 660));
                BgElement::Ocean {
                    sprite: l.text.clone(),
                    x,
                    y,
                    opacity: f(l, "opacity", 0.0),
                    time_min,
                    time_add,
                    init_delay: rng.range_i32(time_min, time_add),
                }
            }
            "SacredGroundMoonLayer" | "FlightMoonLayer" => {
                let (n, nb) = if l.kind == "FlightMoonLayer" {
                    ("flightMoon", "flightMoonBroken")
                } else {
                    ("moon", "moonBroken")
                };
                let broken = a.img(AtlasKind::Bg, nb);
                let image = a
                    .img(AtlasKind::Bg, n)
                    .or_else(|| broken.clone())
                    .unwrap_or_else(|| bg_img(n));
                if l.kind == "FlightMoonLayer" {
                    BgElement::FlightMoon {
                        x,
                        y,
                        image,
                        broken,
                    }
                } else {
                    BgElement::SacredGroundMoon {
                        x,
                        y,
                        image,
                        broken,
                    }
                }
            }
            "AscensionLightningLayer" => {
                let d = if rng.chance(0.5) {
                    rng.range_i32(10, 20)
                } else {
                    rng.range_i32(30, 60)
                };
                BgElement::AscensionLightning {
                    init_delay: d as f32,
                }
            }
            "GhostShipLayer" => {
                let mut ph = [0f32; 4];
                for p in &mut ph {
                    *p = rng.next_f32() * std::f32::consts::PI * 2.0 * 2.0;
                }
                BgElement::GhostShip {
                    image: bg_img("ghostShip"),
                    x,
                    y,
                    scale: f(l, "scale", 1.0),
                    init_phases: ph,
                }
            }
            "VortexLayer" => {
                let init_angles: Vec<f32> = (0..16).map(|_| rng.next_angle()).collect();
                let bez = |rng: &mut DotNetRandom| {
                    let b0 = (rng.range_f32(100.0, 120.0), rng.range_f32(60.0, 120.0));
                    let e = (rng.range_f32(155.0, 10.0), rng.range_f32(115.0, 10.0));
                    let c = (e.0 + (e.0 - b0.0) * 2.0, e.1 + (e.1 - b0.1) * 2.0);
                    [b0, e, c]
                };
                let init_bezier = bez(rng);
                let next_bezier = bez(rng);
                // Initial bolt timer (bar-aligned): one Choose draw.
                let _ = rng.next_max(6);
                BgElement::Vortex {
                    ring: bg_img("vortex0"),
                    init_angles,
                    init_bezier,
                    next_bezier,
                }
            }
            "Random" | "Overtime" | "Else" => {
                let take = match l.kind.as_str() {
                    "Random" => rng.chance(f(l, "chance", 0.5)),
                    "Overtime" => opts.overtime,
                    _ => *failed,
                };
                if take {
                    self.read(&l.children, rng, out)?;
                    *failed = false;
                } else if l.kind != "Else" {
                    *failed = true;
                }
                return Ok(Ok(None));
            }
            other => {
                return Err(Error::format(
                    path,
                    format!("unknown background element {other}"),
                ));
            }
        };
        if let Some(m) = miss {
            if !self.missing.contains(&m) {
                self.missing.push(m);
            }
            return Ok(Err(Missing));
        }
        Ok(Ok(Some(el)))
    }
}

fn decorations(
    a: &LookAssets,
    level: &Level,
    theme: &ThemeDef,
    rng: &mut DotNetRandom,
    lighting: &mut Lighting,
) -> Vec<Decoration> {
    let solid_at = |x: f32, y: f32| -> bool {
        let (w, h) = (level.width as i32, level.height as i32);
        let cx = (x.floor() as i32).rem_euclid(w) / CELL as i32;
        let cy = (y.floor() as i32).rem_euclid(h) / CELL as i32;
        level.solids.get(cx as usize, cy as usize)
    };
    let main = |name: &str| a.img(AtlasKind::Main, name);
    let sprite = |id: &str, anim: &str, start: u32| Part::Sprite {
        set: SpriteSet::Main,
        id: id.to_owned(),
        anim: anim.to_owned(),
        start_index: start,
        offset: (0.0, 0.0),
        flip_x: false,
    };
    let light = |x: f32, y: f32, r: f32, color: [u8; 4]| Light {
        x,
        y,
        radius: r,
        color,
        alpha: 1.0,
        wrap: false,
        kind: LightKind::Decoration,
    };
    let hex = |h: &str| xml::parse_hex_color(h).unwrap_or([0, 0, 0, 255]);
    let black = [0, 0, 0, 255];
    let mut out = Vec::new();
    let mut cobwebs = Vec::new();
    for (idx, e) in level.entities.iter().enumerate() {
        let (x, y) = (e.x, e.y);
        let d = |kind, layer, depth, parts, light: Option<Light>| Decoration {
            kind,
            x,
            y,
            layer,
            depth,
            parts,
            light,
            entity: idx,
        };
        match e.name.as_str() {
            "BGLantern" => {
                if theme.lanterns == "MoonstoneCrystal" {
                    let mut c = d(
                        DecorationKind::Crystal,
                        -3,
                        0,
                        vec![sprite("BGCrystal", "on", 0)],
                        Some(light(x, y + 5.0, 40.0, invert(hex("47C0D1")))),
                    );
                    c.y += 5.0;
                    out.push(c);
                } else {
                    let lit = e.attr_bool("Lit").unwrap_or(true);
                    let l = lit.then(|| light(x, y, 25.0, black));
                    out.push(d(
                        DecorationKind::Torch { lit },
                        -3,
                        0,
                        vec![sprite(&theme.lanterns, "0", 0)],
                        l,
                    ));
                }
            }
            "BGCrystal" => out.push(d(
                DecorationKind::Crystal,
                -3,
                0,
                vec![sprite("BGCrystal", "on", 0)],
                Some(light(x, y, 40.0, invert(hex("47C0D1")))),
            )),
            "BGMushroom" => out.push(d(
                DecorationKind::Mushroom,
                -3,
                0,
                vec![sprite("BGMushroom", "0", 0)],
                Some(light(x, y, 40.0, invert(hex("B4F9C8")))),
            )),
            "BGBigMushroom" => {
                let mut parts = Vec::new();
                if let Some(cap) = main("details/bigMushroom") {
                    parts.push(Part::Strips {
                        image: cap,
                        strip: 4,
                    });
                }
                if let Some(base) = main("details/bigMushroomBase") {
                    let o = (base.src.w as f32 * 0.5, base.src.h as f32);
                    parts.push(Part::Image {
                        image: base,
                        offset: (0.0, 0.0),
                        origin: o,
                        flip_x: false,
                        alpha: 1.0,
                    });
                }
                out.push(d(DecorationKind::BigMushroom, 0, 0, parts, None));
            }
            "BGSkeleton" => {
                if let Some(bones) = main("details/bones") {
                    let cells = (bones.src.w / 20) * (bones.src.h / 20);
                    let frame = rng.next_max(cells.max(1));
                    let flip = !solid_at(x - 11.0, y - 1.0) && solid_at(x + 11.0, y - 1.0);
                    let cols = (bones.src.w / 20).max(1);
                    let src = Rect::new(
                        bones.src.x + (frame % cols) * 20,
                        bones.src.y + (frame / cols) * 20,
                        20,
                        20,
                    );
                    let image = ImageRef { src, ..bones };
                    out.push(d(
                        DecorationKind::Skeleton,
                        0,
                        0,
                        vec![Part::Image {
                            image,
                            offset: (0.0, 0.0),
                            origin: (10.0, 20.0),
                            flip_x: flip,
                            alpha: 1.0,
                        }],
                        None,
                    ));
                }
            }
            "Cobwebs" => {
                let alpha = rng.range_f32(0.4, 0.2);
                cobwebs.push((out.len(), alpha));
                out.push(d(DecorationKind::Cobweb, 0, -110, Vec::new(), None));
            }
            "SnowClump" => {
                if let Some(snow) = main("details/snowDeposit") {
                    let n = rng.next_max(snow.src.w / 14);
                    let image = ImageRef {
                        src: Rect::new(snow.src.x + 14 * n, snow.src.y, 14, 14),
                        ..snow
                    };
                    out.push(d(
                        DecorationKind::SnowClump,
                        0,
                        -151,
                        vec![Part::Image {
                            image,
                            offset: (0.0, 0.0),
                            origin: (2.0, 9.0),
                            flip_x: false,
                            alpha: 1.0,
                        }],
                        None,
                    ));
                }
            }
            "RainDrops" => {
                let w = e.attr_i32("width").unwrap_or(10);
                let len = a.anim_len(SpriteSet::Main, "RainDrops", "0");
                let parts = (0..w)
                    .step_by(10)
                    .map(|i| Part::Sprite {
                        set: SpriteSet::Main,
                        id: "RainDrops".into(),
                        anim: "0".into(),
                        start_index: rng.next_max(len) as u32,
                        offset: ((5 + i) as f32, 10.0),
                        flip_x: false,
                    })
                    .collect();
                out.push(d(DecorationKind::RainDrops, 0, -101, parts, None));
            }
            "WaterDrop" => {
                let _drip_delay = rng.range_i32(60, 300);
                out.push(d(
                    DecorationKind::WaterDrop,
                    0,
                    0,
                    vec![sprite("WaterDrop", "0", 0)],
                    None,
                ));
            }
            "Chain" => {
                if let Some(chain) = main("chain") {
                    let h = e.attr_i32("height").unwrap_or(10);
                    let rows = (chain.src.h / 10).max(1);
                    let parts = (0..h / 10)
                        .map(|i| {
                            let r = rng.next_max(rows);
                            let image = ImageRef {
                                src: Rect::new(chain.src.x, chain.src.y + r * 10, chain.src.w, 10),
                                ..chain.clone()
                            };
                            Part::Image {
                                image,
                                offset: (0.0, (i * 10) as f32),
                                origin: (5.0, 0.0),
                                flip_x: false,
                                alpha: 1.0,
                            }
                        })
                        .collect();
                    out.push(d(DecorationKind::Chain, 0, 0, parts, None));
                }
            }
            "Lantern" => {
                if let Some(l) = main("lantern") {
                    let image = ImageRef {
                        src: Rect::new(l.src.x, l.src.y, 10, 10),
                        ..l
                    };
                    out.push(d(
                        DecorationKind::Lantern,
                        0,
                        109,
                        vec![Part::Image {
                            image,
                            offset: (0.0, 0.0),
                            origin: (5.0, 0.0),
                            flip_x: false,
                            alpha: 1.0,
                        }],
                        Some(Light {
                            wrap: true,
                            ..light(x, y, 40.0, black)
                        }),
                    ));
                }
            }
            "PirateBanner" => {
                let over = solid_at(x, y);
                let names: [&str; 2] = if over {
                    ["details/pirateBannerC", "details/pirateBannerD"]
                } else {
                    ["details/pirateBannerA", "details/pirateBannerB"]
                };
                let name = rng.choose(&names);
                let phase = rng.next_angle();
                if let Some(image) = main(name) {
                    let (layer, depth) = if over { (0, -159) } else { (-3, 10000) };
                    out.push(d(
                        DecorationKind::PirateBanner { over_solids: over },
                        layer,
                        depth,
                        vec![Part::Banner {
                            image,
                            init_phase: phase,
                        }],
                        None,
                    ));
                }
            }
            "GhostShipWindow" => out.push(d(
                DecorationKind::ShipWindow,
                0,
                -160,
                vec![sprite("ShipWindow", "0", 0)],
                Some(light(x, y, 80.0, invert(hex("11FF22")))),
            )),
            "Ogmo" => out.push(d(
                DecorationKind::Ogmo,
                0,
                0,
                vec![sprite("Ogmo", "0", 0)],
                None,
            )),
            _ => {}
        }
    }
    // Cobwebs pick their corner when the scene begins (after every entity
    // was constructed).
    if let Some(webs) = main("details/cobwebs") {
        for (i, alpha) in cobwebs {
            let (x, y) = (out[i].x, out[i].y);
            let (l, r, u, dn) = (
                solid_at(x - 11.0, y),
                solid_at(x + 11.0, y),
                solid_at(x, y - 11.0),
                solid_at(x, y + 11.0),
            );
            let (offset, origin, frames, flip): ((f32, f32), (f32, f32), [i32; 3], bool) = if u && l
            {
                ((-10.0, -10.0), (0.0, 0.0), [0, 1, 2], false)
            } else if u && r {
                ((10.0, -10.0), (20.0, 0.0), [0, 1, 2], true)
            } else if dn && l {
                ((-10.0, 10.0), (0.0, 20.0), [3, 4, 5], false)
            } else {
                // The original throws unless down+right; draw that corner.
                ((10.0, 10.0), (20.0, 20.0), [3, 4, 5], true)
            };
            let frame = rng.choose(&frames);
            let cols = (webs.src.w / 20).max(1);
            let src = Rect::new(
                webs.src.x + (frame % cols) * 20,
                webs.src.y + (frame / cols) * 20,
                20,
                20,
            );
            out[i].parts.push(Part::Image {
                image: ImageRef {
                    src,
                    ..webs.clone()
                },
                offset,
                origin,
                flip_x: flip,
                alpha,
            });
        }
    }
    for dcr in &out {
        if let Some(l) = &dcr.light {
            lighting.lights.push(l.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::installs;
    use crate::towers::{TowerIndex, game_relative_path, versus_load_seed};

    #[test]
    fn every_versus_level_has_a_look() {
        for inst in installs().iter() {
            let a = LookAssets::load(inst).unwrap();
            let idx = TowerIndex::build(inst).unwrap();
            let mut n = 0;
            for t in &idx.versus {
                let theme = a.game_data.theme(&t.theme).unwrap();
                for p in &t.levels {
                    let l = Level::load(p).unwrap();
                    let seed = versus_load_seed(&game_relative_path(inst, p).unwrap());
                    let opts = LookOptions {
                        procedural: t.procedural.then_some((seed, false)),
                        ..Default::default()
                    };
                    let look = LevelLook::build(&a, &l, theme, seed, &opts)
                        .unwrap_or_else(|e| panic!("{p:?}: {e}"));
                    assert!(!look.solid_tiles.tiles.is_empty());
                    assert!(!look.background.is_empty(), "{}: no background", t.dir_name);
                    for d in &look.decorations {
                        for part in &d.parts {
                            if let Part::Sprite {
                                set: SpriteSet::Main,
                                id,
                                anim,
                                ..
                            } = part
                            {
                                let s = a.sprites.get(id).unwrap_or_else(|| panic!("sprite {id}"));
                                assert!(s.animation(anim).is_some(), "{id} anim {anim}");
                            }
                        }
                    }
                    n += 1;
                }
            }
            eprintln!("{}: {n} looks", inst.summary());
        }
    }

    #[test]
    fn sunken_city_random_groups_are_seeded() {
        let Some(steam) = installs().steam.as_ref() else {
            return;
        };
        let a = LookAssets::load(steam).unwrap();
        let theme = a.game_data.theme("SunkenCity").unwrap();
        let idx = TowerIndex::build(steam).unwrap();
        let t = idx.versus.iter().find(|t| t.theme == "SunkenCity").unwrap();
        let l = Level::load(&t.levels[0]).unwrap();
        let counts: Vec<usize> = (0..20)
            .map(|s| {
                LevelLook::build(&a, &l, theme, s, &LookOptions::default())
                    .unwrap()
                    .background
                    .len()
            })
            .collect();
        assert!(
            counts.iter().min() != counts.iter().max(),
            "Random groups never vary: {counts:?}"
        );
        let again = LevelLook::build(&a, &l, theme, 3, &LookOptions::default()).unwrap();
        assert_eq!(
            again,
            LevelLook::build(&a, &l, theme, 3, &LookOptions::default()).unwrap()
        );
    }
}
