//! Boss art (darkworld.md 7): the four bosses, their parts and projectiles
//! and the exit portal as sim `Drawable`s from the boss sprite data and the
//! boss atlas, replacing the sim's debug boxes in the frame's `View`.
//!
//! The sim keeps the fight state (phases, coroutine steps, eye / shield /
//! tentacle state); everything cosmetic that the original keeps in its
//! sprites (animation clocks, wigglers, jitter, fades, the look direction,
//! horns and dust, parts breaking) is presentation state here (`BossFx`),
//! keyed on the sim state so it restarts when a phase changes.

use std::collections::BTreeMap;

use tfp_assets::atlas::{Atlas, AtlasKind};
use tfp_assets::sprite_data::SpriteData;
use tfp_sim::content::{SpriteId, TexId};
use tfp_sim::darkworld::boss::{BossKind, BossState};
use tfp_sim::darkworld::{Part, amaranth, cataclysm, cyclops, dreadwood};
use tfp_sim::entity::{EntityId, Kind};
use tfp_sim::math::{PI, angle_to_vector};
use tfp_sim::{Content, Drawable, Vec2, View, Visual, World};

use crate::coop_ui::art::frame_at;

/// Boss sprite-data ids (`bossSpriteData.xml`), registered as `boss:<id>`.
const BOSS_SPRITES: &[&str] = &[
    "AmaranthBg",
    "AmaranthFlash",
    "AmaranthBody",
    "AmaranthEye",
    "AmaranthShield",
    "AmaranthShot",
    "AmaranthIntro",
    "AmaranthDead",
    "AmaranthDeadFade",
    "DreadEye",
    "Spore",
    "DarkfangEye",
    "DarkfangSkeleton",
    "DarkfangGhost",
    "GhostLamp",
    "CataclysmBody",
    "CataclysmPupil",
    "CataclysmShieldOrb",
    "CataclysmShieldOrbOutline",
    "CataclysmBlade",
    "CataclysmBullet",
    "CataclysmBulletOutline",
    "KingBeam",
];

/// Whole boss-atlas images.
const BOSS_TEX: &[&str] = &[
    "amaranth/eyeBack",
    "amaranth/dead2",
    "dreadwood/ceiling0",
    "dreadwood/ceiling1",
    "dreadwood/ceiling2",
    "dreadwood/ceiling0B",
    "dreadwood/ceiling1B",
    "dreadwood/ceiling2B",
    "dreadwood/eyeFlash",
    "dreadwood/flower",
    "dreadwood/flowerCenter",
    "dreadwood/tentacle",
    "dreadwood/tentacleBg",
    "dreadwood/tentacleFlash",
    "darkfang/head",
    "darkfang/headB",
    "darkfang/headC",
    "darkfang/headWavy",
    "darkfang/headWavyB",
    "darkfang/headWavyC",
    "darkfang/hornLeft",
    "darkfang/hornRight",
    "darkfang/leftFist",
    "darkfang/leftFistB",
    "darkfang/leftFistC",
    "darkfang/rightFist",
    "darkfang/rightFistB",
    "darkfang/rightFistC",
    "darkfang/risingPlatform",
    "darkfang/eyeFlash",
    "darkfang/ceiling",
    "darkfang/floorBones",
    "darkfang/floorBonesLeft",
    "darkfang/floorBonesRight",
    "darkfang/skeletonFade",
    "cataclysm/eyeball",
    "cataclysm/eyeballFlash",
    "cataclysm/heartFlash",
    "cataclysm/heartFade",
    "cataclysm/missile",
    "cataclysm/block2x3",
    "cataclysm/block3x1",
    "cataclysm/block4x1",
    "cataclysm/meat",
];

/// `Darkfang` head image origin (`CyclopsEye.HeadOrigin`).
const HEAD_ORIGIN: Vec2 = Vec2::new(61.0, 6.0);
/// Depth of the Darkfang head layer (the original draws it on the
/// background layer, behind the arena).
const BACKDROP: i32 = 1100;

/// Ids of the registered Dark World art.
#[derive(Clone, Debug, Default)]
pub struct DwArt {
    sprites: BTreeMap<String, SpriteId>,
    tex: BTreeMap<&'static str, TexId>,
}

impl DwArt {
    /// Registers the boss art into `c` (after `art::Art::register`).
    pub fn register(c: &mut Content, inst: &tfp_assets::GameInstall) -> DwArt {
        let mut art = DwArt::default();
        let dirs = inst
            .dark_world
            .iter()
            .map(|d| d.sprite_data_dir())
            .chain(std::iter::once(inst.content.sprite_data_dir()));
        for dir in dirs {
            let Ok(boss) = SpriteData::load(&dir.join("bossSpriteData.xml")) else {
                continue;
            };
            for &n in BOSS_SPRITES {
                if let Some(d) = boss.get(n) {
                    let mut d = d.clone();
                    d.id = format!("boss:{n}");
                    let id = c.add_sprite(AtlasKind::Boss, &d);
                    art.sprites.insert(d.id, id);
                }
            }
            break;
        }
        if let Ok(main) = SpriteData::load(&inst.content.sprite_data_dir().join("spriteData.xml"))
            && let Some(d) = main.get("NextLevelPortal")
        {
            art.sprites
                .insert("NextLevelPortal".into(), c.add_sprite(AtlasKind::Main, d));
        }
        for &n in BOSS_TEX {
            art.tex.insert(n, c.tex(AtlasKind::Boss, n));
        }
        for kind in [AtlasKind::Main, AtlasKind::Boss] {
            if let Some(x) = inst.atlas_xml(kind)
                && let Ok(a) = Atlas::load(&x)
            {
                c.set_texture_sizes(kind, &a);
            }
        }
        art
    }

    pub fn sprite(&self, name: &str) -> SpriteId {
        self.sprites.get(name).copied().unwrap_or(SpriteId::NONE)
    }

    pub fn tex(&self, name: &str) -> Option<TexId> {
        self.tex.get(name).copied()
    }
}

// --------------------------------------------------------------- helpers

fn fl(v: Vec2) -> Vec2 {
    Vec2::new(v.x.floor(), v.y.floor())
}

fn cube_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    t * t * t + 1.0
}

fn cube_in(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t
}

fn cube_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let u = 2.0 * t - 2.0;
        0.5 * u * u * u + 1.0
    }
}

fn back_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    t * t * (2.70158 * t + 1.70158) + 1.0
}

fn back_in(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (2.70158 * t - 1.70158)
}

/// A decaying wiggle (`Wiggler`): `len` ticks, `osc` oscillations.
fn wiggle(t: f32, len: f32, osc: f32) -> f32 {
    if !(0.0..len).contains(&t) {
        return 0.0;
    }
    let k = t / len;
    (k * osc * std::f32::consts::TAU).sin() * (1.0 - k)
}

/// Cosmetic jitter in `[-r, r]` per axis from a frame and a salt.
fn jitter(frame: u32, salt: u32, r: f32) -> Vec2 {
    let mut h = (frame as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((salt as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    let a = (h & 0xffff) as f32 / 65535.0;
    let b = ((h >> 16) & 0xffff) as f32 / 65535.0;
    Vec2::new((a * 2.0 - 1.0) * r, (b * 2.0 - 1.0) * r).floor2()
}

trait Floor2 {
    fn floor2(self) -> Self;
}

impl Floor2 for Vec2 {
    fn floor2(self) -> Vec2 {
        fl(self)
    }
}

fn alpha(a: f32) -> [u8; 4] {
    [255, 255, 255, (a.clamp(0.0, 1.0) * 255.0) as u8]
}

fn gray(v: f32, a: f32) -> [u8; 4] {
    let g = (v.clamp(0.0, 1.0) * 255.0) as u8;
    [g, g, g, (a.clamp(0.0, 1.0) * 255.0) as u8]
}

/// Builds drawables for one entity.
struct Out<'a> {
    c: &'a Content,
    art: &'a DwArt,
    items: Vec<Drawable>,
    id: EntityId,
    depth: i32,
}

impl Out<'_> {
    fn push(&mut self, visual: Visual, pos: Vec2, origin: Vec2) -> &mut Drawable {
        self.items.push(Drawable {
            visual,
            pos,
            origin,
            scale: Vec2::new(1.0, 1.0),
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            tint: [255; 4],
            depth: self.depth,
            entity: self.id,
        });
        self.items.last_mut().unwrap()
    }

    /// A cell of a registered sprite (its origin and position offset).
    fn cell(&mut self, name: &str, frame: i16, pos: Vec2) -> Option<&mut Drawable> {
        let d = self.c.sprite(self.art.sprite(name))?;
        let (tex, fw, fh, origin, off) = (d.texture, d.frame_w, d.frame_h, d.origin, d.position);
        Some(self.push(
            Visual::Sprite {
                tex,
                frame,
                frame_w: fw,
                frame_h: fh,
            },
            pos + off,
            origin,
        ))
    }

    /// The frame of animation `anim` after `t` ticks.
    fn anim(&mut self, name: &str, anim: &str, t: f32, pos: Vec2) -> Option<&mut Drawable> {
        let f = self
            .c
            .sprite(self.art.sprite(name))
            .map(|d| frame_at(d, anim, t))?;
        self.cell(name, f, pos)
    }

    /// Whole image with `origin` given as a fraction of its size.
    fn img(&mut self, name: &str, pos: Vec2, just: (f32, f32)) -> Option<&mut Drawable> {
        let t = self.art.tex(name)?;
        let (w, h) = self.c.texture(t).size?;
        let origin = Vec2::new((w as f32 * just.0).floor(), (h as f32 * just.1).floor());
        Some(self.push(
            Visual::Sprite {
                tex: t,
                frame: -1,
                frame_w: 0,
                frame_h: 0,
            },
            pos,
            origin,
        ))
    }

    /// Whole image with a pixel origin.
    fn img_at(&mut self, name: &str, pos: Vec2, origin: Vec2) -> Option<&mut Drawable> {
        let t = self.art.tex(name)?;
        self.c.texture(t).size?;
        Some(self.push(
            Visual::Sprite {
                tex: t,
                frame: -1,
                frame_w: 0,
                frame_h: 0,
            },
            pos,
            origin,
        ))
    }

    /// `Draw.SineTextureV`: rows of `slice` px shifted by
    /// `round(sin(counter + add * i) * amp)`, pixel origin `origin`.
    #[allow(clippy::too_many_arguments)]
    fn sine_img(
        &mut self,
        name: &str,
        pos: Vec2,
        origin: Vec2,
        slice: u16,
        counter: f32,
        amp: f32,
        add: f32,
        tint: [u8; 4],
    ) {
        let Some(t) = self.art.tex(name) else { return };
        let Some((w, h)) = self.c.texture(t).size else {
            return;
        };
        let rows = h.div_ceil(slice);
        for i in 0..rows {
            let dx = ((counter + add * i as f32).sin() * amp).round();
            let d = self.push(
                Visual::Sprite {
                    tex: t,
                    frame: i as i16,
                    frame_w: w,
                    frame_h: slice,
                },
                fl(pos) + Vec2::new(dx, 0.0),
                Vec2::new(origin.x, origin.y - (i * slice) as f32),
            );
            d.tint = tint;
        }
    }

    /// Black copies one pixel around everything pushed since `from`
    /// (`DrawOutline`), drawn before those items.
    fn outline(&mut self, from: usize) {
        let src: Vec<Drawable> = self.items[from..].to_vec();
        let mut out = Vec::with_capacity(src.len() * 9);
        for (dx, dy) in [
            (-1.0, -1.0),
            (0.0, -1.0),
            (1.0, -1.0),
            (-1.0, 0.0),
            (1.0, 0.0),
            (-1.0, 1.0),
            (0.0, 1.0),
            (1.0, 1.0),
        ] {
            for d in &src {
                let mut b = *d;
                b.pos += Vec2::new(dx, dy);
                b.tint = [0, 0, 0, d.tint[3]];
                out.push(b);
            }
        }
        out.extend(src);
        self.items.truncate(from);
        self.items.extend(out);
    }
}

// ----------------------------------------------------------------- state

/// A cosmetic left behind (a part breaking, a horn, dust).
#[derive(Clone, Debug)]
enum Fx {
    /// A one-shot animation of a sprite.
    Anim {
        sprite: &'static str,
        anim: &'static str,
        rot: f32,
        flip: bool,
    },
    /// A held sprite frame (Dreadwood's dead eyes).
    Hold { sprite: &'static str, frame: i16 },
    /// A Darkfang horn flying off and fading.
    Horn { right: bool, vel: Vec2, rot: f32 },
    /// "Turn to dust": rows of an image falling away one after another.
    Dust {
        tex: &'static str,
        origin: Vec2,
        alpha: f32,
    },
    /// A falling Dreadwood flower.
    /// A falling flower (`DreadFlower`): starts at `start`, sways on a
    /// 180-tick sine.
    Flower { start: f32, rot: f32 },
}

#[derive(Clone, Debug)]
struct Effect {
    fx: Fx,
    pos: Vec2,
    t: f32,
    life: f32,
    depth: i32,
}

/// Presentation state of the boss level.
#[derive(Default)]
pub struct BossFx {
    clocks: BTreeMap<(u32, u8), (u64, u32)>,
    /// Last tick's Dark World parts: (part, position, hitbox size).
    last: BTreeMap<EntityId, (Part, Vec2, Vec2)>,
    effects: Vec<Effect>,
    /// Smoothed look direction (Amaranth eye, Cataclysm pupil).
    look: Vec2,
    last_health: i32,
    hit: f32,
    heart_last: i32,
    heart_hit: f32,
    /// White screen flash (boss deaths, Cataclysm's heart), 0..1.
    pub screen_flash: f32,
    level_key: Option<(usize, usize)>,
    phase: Option<(u8, u8)>,
    frame: u32,
    flowers_down: bool,
    heart_at: Option<u32>,
    /// Dreadwood background tentacles: the sway phase (a 300-tick sine)
    /// and the wiggle counter, both advanced at the current rate.
    bg_sway: f32,
    bg_counter: f32,
}

/// Clock kinds (one clock per entity and kind).
mod clk {
    pub const PHASE: u8 = 0;
    pub const OPEN: u8 = 1;
    pub const STEP: u8 = 2;
    pub const INTRO: u8 = 3;
    pub const BEAM: u8 = 4;
    pub const BG: u8 = 5;
    pub const LIT: u8 = 6;
    pub const CHARGE: u8 = 7;
    pub const BURST: u8 = 8;
    pub const SUB: u8 = 10;
}

impl BossFx {
    pub fn clear(&mut self) {
        *self = BossFx::default();
    }

    /// A hit flash is showing (proof captures avoid it).
    pub fn flashing(&self) -> bool {
        self.hit > 0.0 || self.heart_hit > 0.0
    }

    /// Ticks since Cataclysm's heart appeared.
    pub fn heart_since(&self) -> Option<u32> {
        self.heart_at.map(|f| self.frame.saturating_sub(f))
    }

    /// Ticks since `key` was first seen for (`slot`, `kind`).
    fn since(&mut self, slot: u32, kind: u8, key: u64) -> f32 {
        let frame = self.frame;
        let e = self.clocks.entry((slot, kind)).or_insert((key, frame));
        if e.0 != key {
            *e = (key, frame);
        }
        frame.saturating_sub(e.1) as f32
    }

    fn effect(&mut self, fx: Fx, pos: Vec2, life: f32, depth: i32) {
        self.effects.push(Effect {
            fx,
            pos,
            t: 0.0,
            life,
            depth,
        });
    }

    /// After each sim step: parts that went away leave their break
    /// animation, phase changes flash the screen. Returns particle bursts
    /// `(catalogue name, count, position)`.
    pub fn after_step(&mut self, w: &World, c: &Content) -> Vec<(&'static str, usize, Vec2)> {
        let mut bursts = Vec::new();
        self.frame = w.frame;
        self.screen_flash = (self.screen_flash - 0.04).max(0.0);
        self.hit = (self.hit - 1.0).max(0.0);
        self.heart_hit = (self.heart_hit - 1.0).max(0.0);
        let Some(d) = w.dark_world.as_deref() else {
            return bursts;
        };
        let key = (d.round_index, w.level);
        if self.level_key != Some(key) {
            self.clear();
            self.frame = w.frame;
            self.level_key = Some(key);
        }
        for e in &mut self.effects {
            e.t += 1.0;
            if let Fx::Flower { start, rot } = &mut e.fx
                && e.t >= *start
            {
                // `DreadFlower`: rotation (4 - 2|s|) degrees, velocity
                // (0.4 s, 0.6 - 0.5|s|) per tick; gone below y = 260.
                let sv = ((e.t - *start) / 180.0 * std::f32::consts::TAU).sin();
                *rot += (4.0 - 2.0 * sv.abs()) * PI / 180.0;
                e.pos += Vec2::new(sv * 0.4, 0.6 - (sv * 0.5).abs());
                if e.pos.y >= 260.0 {
                    e.life = e.t;
                }
            }
            if let Fx::Horn { vel, rot, right } = &mut e.fx {
                let len = vel.length();
                if len > 0.05 {
                    *vel *= (len - 0.05) / len;
                } else {
                    *vel = Vec2::ZERO;
                }
                e.pos += *vel;
                *rot += if *right { 1.0 } else { -1.0 } * 1.5 * PI / 180.0;
            }
        }
        self.effects.retain(|e| e.t < e.life);
        // Boss phase and health (hit flashes where the sim keeps none).
        let bs = d.boss.as_deref();
        let phase = tfp_sim::darkworld::boss_phase(w);
        if let Some(b) = bs {
            if b.base.health < self.last_health {
                self.hit = 20.0;
            }
            self.last_health = b.base.health;
            if let BossKind::Cataclysm(s) = &b.kind {
                if s.heart && s.heart_health < self.heart_last {
                    self.heart_hit = 20.0;
                }
                self.heart_last = s.heart_health;
                if s.heart && self.phase.is_some_and(|p| p == (3, cataclysm::DEAD)) {
                    // The heart appears with a white flash.
                    if self.since(b.body.index, clk::SUB + 2, s.heart as u64) == 0.0 {
                        self.screen_flash = 1.0;
                        self.heart_at = Some(self.frame);
                    }
                }
            }
            if let BossKind::Cyclops(s) = &b.kind
                && s.state == cyclops::MAD
                && s.mad_seq.step == 3
                && self.since(b.body.index, clk::SUB, (s.mad * 16 + 3) as u64) == 0.0
            {
                // The horn breaks off.
                let right = s.mad >= 1;
                let side = if right { 1.0 } else { -1.0 };
                let at = w.core(b.body).pos + Vec2::new(45.0 * side, 21.0);
                self.effect(
                    Fx::Horn {
                        right,
                        vel: Vec2::new(2.0 * side, -0.5),
                        rot: 0.0,
                    },
                    at,
                    40.0,
                    BACKDROP + 1,
                );
                bursts.push(("EnLichHornBreak", 20, at + Vec2::new(-15.0 * side, 1.0)));
            }
        }
        if phase != self.phase {
            let dead = matches!(
                phase,
                Some((0, amaranth::DEAD))
                    | Some((1, dreadwood::DEAD))
                    | Some((2, cyclops::DEAD))
                    | Some((3, cataclysm::EXPLODING))
            );
            if dead {
                self.screen_flash = 1.0;
            }
            self.phase = phase;
        }
        // Dreadwood background tentacles: rate 1, then during the death
        // 1 -> 0 over 20 ticks, 0 -> 4 over 120, 4 -> 0 over 300 (CubeOut).
        if let Some(b) = bs
            && let BossKind::Dreadwood(s) = &b.kind
        {
            let slot = b.body.index;
            let rate = if s.phase != dreadwood::DEAD || s.death.step <= 2 {
                1.0
            } else if s.death.step == 3 {
                1.0 - (self.since(slot, clk::SUB + 4, 3) / 20.0).min(1.0)
            } else if s.death.step <= 5 {
                4.0 * (self.since(slot, clk::SUB + 4, 4) / 120.0).min(1.0)
            } else {
                4.0 * (1.0 - cube_out(self.since(slot, clk::SUB + 4, 6) / 300.0))
            };
            self.bg_sway =
                (self.bg_sway + rate * std::f32::consts::TAU / 300.0) % std::f32::consts::TAU;
            self.bg_counter = (self.bg_counter + rate / 8.0) % std::f32::consts::TAU;
        }
        // Dreadwood: the flowers fall after the eyes burst.
        if let Some(b) = bs
            && let BossKind::Dreadwood(s) = &b.kind
            && s.phase == dreadwood::DEAD
            && s.death.step >= 6
            && !self.flowers_down
        {
            self.flowers_down = true;
            // Shuffled, one every 30 + {0, 5, 10} ticks.
            let mut flowers = c.level(w.level).markers.boss_markers_b.clone();
            let n = flowers.len();
            for i in (1..n).rev() {
                let j = (jitter(w.frame, i as u32, 1000.0).x.abs() as usize) % (i + 1);
                flowers.swap(i, j);
            }
            let mut start = 0.0;
            for (k, at) in flowers.into_iter().enumerate() {
                self.effect(Fx::Flower { start, rot: 0.0 }, at, 1.0e6, -260);
                let gap =
                    [0.0, 5.0, 10.0][(jitter(w.frame, 100 + k as u32, 100.0).x.abs() as usize) % 3];
                start += 30.0 + gap;
            }
        }
        // Parts that are gone.
        let mut now = BTreeMap::new();
        for id in w.entities() {
            if let Some(Kind::Dw(p)) = w.kind(id) {
                let core = w.core(id);
                let hb = core.hitbox();
                now.insert(id, (*p, core.pos, Vec2::new(hb.w, hb.h)));
            }
        }
        let gone: Vec<(Part, Vec2, Vec2)> = self
            .last
            .iter()
            .filter(|(id, _)| !now.contains_key(id))
            .map(|(_, v)| *v)
            .collect();
        for (p, pos, size) in gone {
            match p {
                Part::AmaShot { heading, .. } => self.effect(
                    Fx::Anim {
                        sprite: "boss:AmaranthShot",
                        anim: "1",
                        rot: heading,
                        flip: false,
                    },
                    pos,
                    30.0,
                    -260,
                ),
                Part::CycShot { heading, .. } => self.effect(
                    Fx::Anim {
                        sprite: "boss:DarkfangGhost",
                        anim: "1",
                        rot: 0.0,
                        flip: heading.cos() < 0.0,
                    },
                    pos,
                    29.0,
                    -252,
                ),
                Part::Bullet { .. } => self.effect(
                    Fx::Anim {
                        sprite: "boss:CataclysmBullet",
                        anim: "2",
                        rot: 0.0,
                        flip: false,
                    },
                    pos,
                    20.0,
                    -260,
                ),
                Part::Spore { .. } => {
                    self.effect(
                        Fx::Anim {
                            sprite: "boss:Spore",
                            anim: "death",
                            rot: 0.0,
                            flip: false,
                        },
                        pos,
                        18.0,
                        -70,
                    );
                    bursts.push(("EnBrambleSpore", 6, pos));
                }
                Part::ShieldOrb { .. } => self.effect(
                    Fx::Anim {
                        sprite: "boss:CataclysmShieldOrb",
                        anim: "destroy",
                        rot: 0.0,
                        flip: false,
                    },
                    pos,
                    34.0,
                    -255,
                ),
                Part::Block { .. } => {
                    let n = ((size.x * size.y) / 25.0).clamp(4.0, 30.0) as usize;
                    bursts.push((
                        "CrackedBlockBreak",
                        n,
                        pos + Vec2::new(size.x / 2.0, size.y / 2.0),
                    ));
                }
                Part::Fist(k) if d.phase != tfp_sim::darkworld::DwPhase::Transition => {
                    bursts.push(("BGSkeletonBreak", 24, pos));
                    let _ = k;
                    self.effect(
                        Fx::Dust {
                            tex: "darkfang/skeletonFade",
                            origin: Vec2::new(10.0, 14.0),
                            alpha: 1.0,
                        },
                        pos,
                        // Bottom-up per-pixel fall: 4 ticks a row, then
                        // 80 ticks to fade.
                        260.0,
                        -148,
                    );
                }
                Part::Eye(_) => {
                    // A burst Dreadwood eye stays as an empty socket.
                    self.effect(
                        Fx::Hold {
                            sprite: "boss:DreadEye",
                            frame: 2,
                        },
                        pos,
                        1.0e9,
                        -69,
                    );
                }
                Part::Body => {
                    if let Some((0, _)) = self.phase {
                        // Amaranth: nothing is left.
                    }
                }
                _ => {}
            }
        }
        self.last = now;
        bursts
    }

    /// Swaps the art into `view` for every Dark World part and boss.
    /// Returns extra lights `(pos, radius)`.
    pub fn apply(
        &mut self,
        view: &mut View,
        w: &World,
        c: &Content,
        art: &DwArt,
    ) -> Vec<(Vec2, f32)> {
        self.frame = w.frame;
        let mut lights = Vec::new();
        let mut replaced = std::collections::BTreeSet::new();
        let mut add: Vec<Drawable> = Vec::new();
        let boss = w.dark_world.as_deref().and_then(|d| d.boss.as_deref());
        for id in w.entities() {
            let Some(Kind::Dw(p)) = w.kind(id) else {
                continue;
            };
            replaced.insert(id);
            let core = w.core(id);
            let mut o = Out {
                c,
                art,
                items: Vec::new(),
                id,
                depth: core.depth,
            };
            let pos = fl(core.pos);
            match *p {
                Part::ExitPortal { t } => {
                    self.portal(&mut o, w, pos, t);
                    lights.push((pos, 80.0));
                }
                Part::Body => {
                    if let Some(b) = boss
                        && b.body == id
                    {
                        self.body(&mut o, w, c, b, pos, &mut lights);
                    }
                }
                Part::AmaShot { t, heading, grace } => {
                    let a = if grace > 0.0 { 0.7 } else { 1.0 };
                    let from = o.items.len();
                    if let Some(d) = o.anim("boss:AmaranthShot", "0", t, pos) {
                        d.rotation = heading;
                        d.tint = alpha(a);
                    }
                    o.outline(from);
                    lights.push((pos, 30.0));
                }
                Part::AmaBeam { t, angle } => {
                    let (a, wy) = if t < 60.0 {
                        (0.3 + 0.4 * t / 60.0, 0.2 + 0.5 * t / 60.0)
                    } else if t > 130.0 {
                        ((160.0 - t) / 30.0, 1.0)
                    } else {
                        (1.0, 1.0)
                    };
                    if let Some(d) = o.anim("boss:KingBeam", "0", t, core.pos) {
                        d.rotation = angle;
                        d.scale = Vec2::new(2.0, wy);
                        d.tint = alpha(a);
                    }
                }
                Part::Eye(i) => {
                    if let Some(b) = boss
                        && let BossKind::Dreadwood(s) = &b.kind
                    {
                        self.dread_eye(&mut o, w, b, s, i as usize, pos, &mut lights);
                    }
                }
                Part::Tentacle(i) => {
                    if let Some(b) = boss
                        && let BossKind::Dreadwood(s) = &b.kind
                    {
                        self.tentacle(&mut o, w, s, i as usize, pos);
                    }
                }
                Part::Spore { t, phase, .. } => {
                    let from = o.items.len();
                    if let Some(d) = o.anim("boss:Spore", "idle", t, pos) {
                        d.rotation = -PI / 8.0 * (phase - PI / 4.0).sin();
                    }
                    o.outline(from);
                }
                Part::Fist(k) => {
                    if let Some(b) = boss
                        && let BossKind::Cyclops(s) = &b.kind
                    {
                        self.fist(&mut o, w, b, s, k, pos, &mut lights);
                    }
                }
                Part::Pedestal => {
                    o.img("darkfang/risingPlatform", pos, (0.0, 0.0));
                }
                Part::Bones => {
                    o.depth = 200;
                    o.img("darkfang/floorBonesLeft", pos, (0.0, 0.0));
                    let mut x = 20.0;
                    while x < 200.0 {
                        o.img("darkfang/floorBones", pos + Vec2::new(x, 0.0), (0.0, 0.0));
                        x += 40.0;
                    }
                    o.img(
                        "darkfang/floorBonesRight",
                        pos + Vec2::new(200.0, 0.0),
                        (0.0, 0.0),
                    );
                }
                Part::CycShot { t, heading } => {
                    let flip = heading.cos() < 0.0;
                    let a = (t / 15.0).clamp(0.4, 1.0);
                    let bob = (t / 30.0 * std::f32::consts::TAU).sin();
                    let from = o.items.len();
                    if let Some(d) = o.anim("boss:DarkfangGhost", "0", t, pos) {
                        d.scale.x = if flip { -1.0 } else { 1.0 };
                        d.tint = alpha(a);
                    }
                    o.outline(from);
                    if let Some(d) = o.anim(
                        "boss:GhostLamp",
                        "0",
                        t,
                        pos + Vec2::new(if flip { -6.0 } else { 6.0 }, 2.0),
                    ) {
                        d.rotation = 0.2 * bob;
                        d.tint = alpha(a);
                    }
                    lights.push((pos, 30.0));
                }
                Part::Bullet { t, .. } => {
                    let a = if t < 15.0 { 0.6 } else { 1.0 };
                    if let Some(d) = o.cell("boss:CataclysmBulletOutline", 0, pos) {
                        d.tint = alpha(a);
                    }
                    if let Some(d) = o.anim("boss:CataclysmBullet", "0", t, pos) {
                        d.tint = alpha(a);
                    }
                }
                Part::Missile { heading, .. } => {
                    let from = o.items.len();
                    if let Some(d) = o.img("cataclysm/missile", pos, (0.5, 0.5)) {
                        d.rotation = heading;
                    }
                    o.outline(from);
                    lights.push((pos, 20.0));
                }
                Part::Blade { t, spin, .. } => {
                    let anim = if t < 60.0 { "setup" } else { "idle" };
                    let from = o.items.len();
                    if let Some(d) = o.anim("boss:CataclysmBlade", anim, t, pos) {
                        d.rotation = spin;
                    }
                    o.outline(from);
                }
                Part::ShieldOrb { t, .. } => {
                    o.cell("boss:CataclysmShieldOrbOutline", 0, pos);
                    let create_len = 6.0 * 0.08 * 60.0;
                    if t < create_len {
                        o.anim("boss:CataclysmShieldOrb", "create", t, pos);
                    } else {
                        o.anim("boss:CataclysmShieldOrb", "idle", t - create_len, pos);
                    }
                    lights.push((pos + Vec2::new(0.0, 4.0), 40.0));
                }
                Part::Block { shake, .. } => {
                    let hb = core.hitbox();
                    let name = if hb.h > 20.0 {
                        "cataclysm/block2x3"
                    } else if hb.w >= 40.0 {
                        "cataclysm/block4x1"
                    } else {
                        "cataclysm/block3x1"
                    };
                    let j = if shake > 0.0 {
                        jitter(w.frame, id.index, 1.0)
                    } else {
                        Vec2::ZERO
                    };
                    o.img(name, pos + Vec2::new(hb.x, hb.y) + j, (0.0, 0.0));
                }
            }
            add.extend(o.items);
        }
        // Cosmetics left behind.
        let mut o = Out {
            c,
            art,
            items: Vec::new(),
            id: EntityId {
                index: u32::MAX - 1,
                generation: 1,
            },
            depth: 0,
        };
        let effects = self.effects.clone();
        for e in &effects {
            o.depth = e.depth;
            self.draw_effect(&mut o, w, e);
        }
        add.extend(o.items);
        view.items.retain(|d| !replaced.contains(&d.entity));
        // The sim's generic light for these parts gives way to ours.
        let at: Vec<Vec2> = replaced.iter().map(|&e| w.core(e).pos).collect();
        view.lights.retain(|(p, _)| !at.contains(p));
        view.items.extend(add);
        lights
    }

    fn draw_effect(&mut self, o: &mut Out, w: &World, e: &Effect) {
        let pos = fl(e.pos);
        match &e.fx {
            Fx::Anim {
                sprite,
                anim,
                rot,
                flip,
            } => {
                if let Some(d) = o.anim(sprite, anim, e.t, pos) {
                    d.rotation = *rot;
                    if *flip {
                        d.scale.x = -1.0;
                    }
                }
            }
            Fx::Hold { sprite, frame } => {
                o.cell(sprite, *frame, pos);
            }
            Fx::Horn { right, rot, .. } => {
                let name = if *right {
                    "darkfang/hornRight"
                } else {
                    "darkfang/hornLeft"
                };
                let side = if *right { 1.0 } else { -1.0 };
                let origin = HEAD_ORIGIN + Vec2::new(45.0 * side, 21.0);
                let a = (2.0 - 0.05 * e.t).min(1.0);
                if let Some(d) = o.img_at(name, pos, origin) {
                    d.rotation = *rot;
                    d.tint = alpha(a);
                }
            }
            Fx::Dust { tex, origin, alpha } => {
                dust(o, tex, pos, *origin, e.t, *alpha);
            }
            Fx::Flower { rot, .. } => {
                if let Some(d) = o.img("dreadwood/flower", pos, (0.5, 0.5)) {
                    d.rotation = *rot;
                }
                let _ = w;
            }
        }
    }

    // ------------------------------------------------------------ portal

    fn portal(&mut self, o: &mut Out, w: &World, pos: Vec2, t: f32) {
        let d = w.dark_world.as_deref();
        let closing = d
            .filter(|d| d.phase == tfp_sim::darkworld::DwPhase::Transition)
            .map(|d| d.phase_t);
        let mut scale = 0.8 * back_out(t / 30.0);
        if let Some(pt) = closing {
            // Transition: wait(30), zoom 40 ... the portal closes at ~61.
            scale *= 1.0 - cube_in((pt - 61.0) / 40.0).max(0.0);
        }
        let a = if t >= 60.0 { 1.0 } else { 0.5 };
        let wig = if t >= 60.0 {
            1.0 + 0.2 * wiggle(t - 60.0, 30.0, 3.0)
        } else {
            1.0
        };
        if scale <= 0.01 {
            return;
        }
        if let Some(it) = o.anim("NextLevelPortal", "0", t, pos) {
            it.scale = Vec2::new(scale * wig, scale * wig);
            it.tint = alpha(a);
        }
    }

    // ------------------------------------------------------------ bodies

    fn body(
        &mut self,
        o: &mut Out,
        w: &World,
        c: &Content,
        b: &BossState,
        pos: Vec2,
        lights: &mut Vec<(Vec2, f32)>,
    ) {
        let flash = b.base.flash.max(self.hit) > 0.0
            && ((b.base.flash.max(self.hit) as u32) / 3).is_multiple_of(2);
        match &b.kind {
            BossKind::Amaranth(s) => self.amaranth(o, w, b, s, pos, flash, lights),
            BossKind::Dreadwood(s) => self.dreadwood(o, w, c, b, s),
            BossKind::Cyclops(s) => self.darkfang(o, w, b, s, pos, flash, lights),
            BossKind::Cataclysm(s) => self.cataclysm(o, w, b, s, pos, flash, lights),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn amaranth(
        &mut self,
        o: &mut Out,
        w: &World,
        b: &BossState,
        s: &amaranth::State,
        pos: Vec2,
        flash: bool,
        lights: &mut Vec<(Vec2, f32)>,
    ) {
        let slot = b.body.index;
        let t_state = self.since(slot, clk::PHASE, s.state as u64);
        // The eye's look: random turns while idle, eased like the original.
        let target = angle_to_vector(s.look, 1.0);
        self.look = approach(self.look, target, 0.2);
        let look = self.look;
        let shake = if s.shake > 0.0 {
            jitter(w.frame, slot, 1.0)
        } else {
            Vec2::ZERO
        };
        let tent = 1.0 + 0.1 * (w.frame as f32 / 120.0 * std::f32::consts::TAU).sin();
        let (shield_rot, shield_scale) = if s.state == amaranth::INTRO {
            // The shield grows and spins in over 240 ticks (CubeOut) from
            // the boss music cue; the sim snaps it at the end of the intro.
            if s.intro.step >= 2 {
                let k = self.since(slot, clk::SUB + 5, 1);
                let e = cube_out((k / 240.0).min(1.0));
                (29.84513 + (-PI / 2.0 - 29.84513) * e, e)
            } else {
                let _ = self.since(slot, clk::SUB + 5, 0);
                (-PI / 2.0, 0.0)
            }
        } else {
            (s.shield, s.shield_scale)
        };
        match s.state {
            amaranth::INTRO => {
                if shield_scale > 0.0 {
                    let from = o.items.len();
                    if let Some(d) = o.cell("boss:AmaranthShield", 0, pos) {
                        d.rotation = shield_rot;
                        d.scale = Vec2::new(shield_scale, shield_scale);
                    }
                    o.outline(from);
                }
                if s.intro.step >= 3 {
                    let t = self.since(slot, clk::INTRO, 3);
                    let f =
                        o.c.sprite(o.art.sprite("boss:AmaranthIntro"))
                            .map_or(0, |d| frame_at(d, "0", t));
                    let j = if f >= 3 {
                        jitter(w.frame, slot + 7, 2.0)
                    } else {
                        Vec2::ZERO
                    };
                    let sc = 1.0 + 0.1 * wiggle(t - 14.4, 20.0, 4.0);
                    let from = o.items.len();
                    if let Some(d) = o.cell("boss:AmaranthIntro", f, pos + j) {
                        d.scale = Vec2::new(sc, sc);
                    }
                    o.outline(from);
                }
                lights.push((pos, 90.0 * shield_scale.max(0.2)));
            }
            amaranth::NORMAL => {
                let beaming = s.beams.iter().any(|x| x.is_some());
                let bt = self.since(slot, clk::BEAM, beaming as u64);
                let red = if beaming {
                    (bt / 30.0).min(1.0)
                } else {
                    1.0 - (bt / 30.0).min(1.0)
                };
                let eye_tint = [
                    255,
                    (255.0 - 127.0 * red) as u8,
                    (255.0 - 127.0 * red) as u8,
                    255,
                ];
                let bg_pos = pos - look * 2.0 + shake;
                let from = o.items.len();
                if let Some(d) = o.anim("boss:AmaranthBg", "idle", w.frame as f32, bg_pos) {
                    d.scale = Vec2::new(tent, tent);
                }
                o.outline(from);
                let from = o.items.len();
                if let Some(d) = o.cell("boss:AmaranthShield", 0, pos + shake) {
                    d.rotation = shield_rot;
                    d.scale = Vec2::new(shield_scale, shield_scale);
                }
                o.outline(from);
                let hb = self.hit;
                let body_sc = 1.0 + 0.1 * wiggle(20.0 - hb, 20.0, 3.0);
                if let Some(d) = o.img("amaranth/eyeBack", pos + shake, (0.5, 0.5)) {
                    d.scale = Vec2::new(body_sc, body_sc);
                    d.tint = eye_tint;
                }
                if let Some(d) = o.cell("boss:AmaranthBody", 0, pos + shake) {
                    d.rotation = -(shield_rot + PI / 2.0);
                    d.scale = Vec2::new(body_sc, body_sc);
                }
                if let Some(d) = o.cell("boss:AmaranthEye", 0, fl(pos + look * 5.0) + shake) {
                    d.scale = Vec2::new(body_sc, body_sc);
                    d.tint = eye_tint;
                }
                if flash
                    && let Some(d) = o.anim("boss:AmaranthFlash", "idle", w.frame as f32, bg_pos)
                {
                    d.scale = Vec2::new(tent, tent);
                }
                // The shield again over the body (the original adds it last).
                if let Some(d) = o.cell("boss:AmaranthShield", 0, pos + shake) {
                    d.rotation = shield_rot;
                    d.scale = Vec2::new(shield_scale, shield_scale);
                }
                lights.push((pos, 90.0));
            }
            _ => {
                // Dead: flash, backdrop shrinks, dead sprite, shield flies
                // off spinning, the body fades and turns to dust.
                let t = t_state;
                let st = if t < 60.0 {
                    1.0
                } else {
                    let k = cube_in_out((t - 60.0) / 240.0);
                    1.0 - k
                };
                let sp = if t < 60.0 {
                    pos
                } else {
                    let k = cube_in_out((t - 60.0) / 240.0);
                    fl(pos + (Vec2::new(160.0, 90.0) - pos) * k)
                };
                let srot = shield_rot
                    + if t < 60.0 {
                        0.0
                    } else {
                        10.0 * PI * cube_in_out((t - 60.0) / 240.0)
                    };
                if st > 0.01 {
                    let from = o.items.len();
                    if let Some(d) = o.cell("boss:AmaranthShield", 0, sp) {
                        d.rotation = srot;
                        d.scale = Vec2::new(st, st);
                    }
                    o.outline(from);
                }
                let bg = if t < 30.0 {
                    tent
                } else {
                    tent * (1.0 - back_in((t - 30.0) / 30.0)).max(0.0)
                };
                if bg > 0.01 {
                    let from = o.items.len();
                    if let Some(d) = o.anim("boss:AmaranthBg", "idle", w.frame as f32, pos) {
                        d.scale = Vec2::new(bg, bg);
                    }
                    o.outline(from);
                }
                if t < 60.0 {
                    o.cell("boss:AmaranthBody", 0, pos + jitter(w.frame, slot, 1.0));
                    o.cell("boss:AmaranthEye", 0, pos + jitter(w.frame, slot, 1.0));
                } else if t < 210.0 {
                    let j = if t >= 120.0 {
                        jitter(w.frame, slot + 3, 1.0)
                    } else {
                        Vec2::ZERO
                    };
                    let sc = 1.0 + 0.1 * wiggle(t - 60.0, 30.0, 3.0);
                    if let Some(d) = o.cell("boss:AmaranthDead", 0, pos + j) {
                        d.scale = Vec2::new(sc, sc);
                    }
                    let fade = 1.0 - ((t - 120.0) / 60.0).clamp(0.0, 1.0);
                    if fade > 0.0
                        && let Some(d) = o.cell("boss:AmaranthDeadFade", 0, pos + j)
                    {
                        d.tint = alpha(fade);
                    }
                } else {
                    dust(
                        o,
                        "amaranth/dead2",
                        pos,
                        Vec2::new(23.0, 22.0),
                        t - 210.0,
                        1.0,
                    );
                }
                let la = 1.0 - ((t - 300.0) / 200.0).clamp(0.0, 1.0);
                lights.push((pos, 90.0 * la));
            }
        }
    }

    fn dreadwood(
        &mut self,
        o: &mut Out,
        w: &World,
        c: &Content,
        b: &BossState,
        s: &dreadwood::State,
    ) {
        let slot = b.body.index;
        let d = b.base.difficulty.clamp(0, 2);
        let t_state = self.since(slot, clk::PHASE, s.phase as u64);
        // Leaf ceiling: at the end of the death sequence the withered
        // leaves (`ceiling<d>B`) show while the green ones fade over 300
        // ticks (CubeOut).
        let dying = s.phase == dreadwood::DEAD && s.death.step >= 6;
        let fade = if dying {
            1.0 - cube_out(self.since(slot, clk::SUB + 3, 6) / 300.0)
        } else {
            1.0
        };
        let _ = t_state;
        o.depth = -250;
        if dying {
            o.img(
                [
                    "dreadwood/ceiling0B",
                    "dreadwood/ceiling1B",
                    "dreadwood/ceiling2B",
                ][d as usize],
                Vec2::new(160.0, 0.0),
                (0.5, 0.0),
            );
        }
        if fade > 0.0
            && let Some(it) = o.img(
                [
                    "dreadwood/ceiling0",
                    "dreadwood/ceiling1",
                    "dreadwood/ceiling2",
                ][d as usize],
                Vec2::new(160.0, 0.0),
                (0.5, 0.0),
            )
        {
            it.tint = alpha(fade);
        }
        // Flowers (until they fall).
        if !self.flowers_down {
            for at in c.level(w.level).markers.boss_markers_b.iter() {
                o.img("dreadwood/flower", fl(*at), (0.5, 0.5));
            }
        }
        // Background tentacles rise and leave with `bg_in`.
        let bt = self.since(slot, clk::BG, s.bg_in as u64);
        o.depth = 10100;
        for (k, (x, ty)) in [(60.0, 60.0), (120.0, 50.0), (200.0, 50.0), (260.0, 60.0)]
            .into_iter()
            .enumerate()
        {
            let phase = k as f32 * 1.7;
            let sway = (self.bg_sway + phase).sin();
            let target = Vec2::new(x + 10.0 * sway, ty + 5.0 * (sway * 0.5));
            let y = if s.bg_in {
                240.0 + (target.y - 240.0) * cube_out(bt / 60.0)
            } else {
                // Leaving (or never entered: stays below the screen).
                target.y + (240.0 - target.y) * cube_in(bt / 60.0)
            };
            if y >= 239.0 {
                continue;
            }
            let counter = self.bg_counter + phase;
            o.sine_img(
                "dreadwood/tentacleBg",
                Vec2::new(target.x, y),
                Vec2::new(16.0, 0.0),
                4,
                counter,
                1.0,
                PI / 16.0,
                [255; 4],
            );
        }
        o.depth = -250;
    }

    #[allow(clippy::too_many_arguments)]
    fn dread_eye(
        &mut self,
        o: &mut Out,
        w: &World,
        b: &BossState,
        s: &dreadwood::State,
        i: usize,
        pos: Vec2,
        lights: &mut Vec<(Vec2, f32)>,
    ) {
        let Some(e) = s.eyes.get(i) else { return };
        let slot = o.id.index;
        let ever = self.clocks.contains_key(&(slot, clk::OPEN)) || e.open;
        let t = self.since(slot, clk::OPEN, e.open as u64);
        let shaking = e.flash > 0.0 || (s.phase == dreadwood::DEAD && s.death.step >= 4);
        let j = if shaking {
            jitter(w.frame, slot, 2.0)
        } else {
            Vec2::ZERO
        };
        let at = pos + j;
        let (anim, at_t) = if !ever {
            ("startClosed", 0.0)
        } else if e.open {
            let open_len = 3.0 * 6.0;
            if t < open_len {
                ("open", t)
            } else {
                ("idle", t - open_len)
            }
        } else {
            ("close", 0.0)
        };
        let mut scale = Vec2::new(1.0 + 0.1 * wiggle(20.0 - e.flash, 40.0, 4.0), 1.0);
        scale.y = scale.x;
        if e.burst {
            let bt = self.since(slot, clk::BURST, 1);
            let k = cube_out(bt / 60.0);
            scale = Vec2::new(1.0 + 0.1 * k, 1.0 + k);
        }
        let from = o.items.len();
        if let Some(d) = o.anim("boss:DreadEye", anim, at_t, at) {
            d.scale = scale;
        }
        o.outline(from);
        let burst_flash = e.burst && {
            let bt = self.since(slot, clk::BURST, 1);
            (100.0..120.0).contains(&bt)
        };
        if (e.flash > 0.0 && ((e.flash as u32) / 3).is_multiple_of(2)) || burst_flash {
            let orig =
                o.c.sprite(o.art.sprite("boss:DreadEye"))
                    .map_or(Vec2::new(20.0, 5.0), |d| d.origin);
            if let Some(d) = o.img_at("dreadwood/eyeFlash", at, orig) {
                d.scale = scale;
            }
        }
        if e.open {
            lights.push((pos + Vec2::new(0.0, 4.0), 100.0 * (t / 10.0).min(1.0)));
        }
        let _ = b;
    }

    fn tentacle(&mut self, o: &mut Out, w: &World, s: &dreadwood::State, i: usize, pos: Vec2) {
        let Some(t) = s.tentacles.get(i) else { return };
        if !t.seq.on && pos.y >= t.start.y - 0.5 {
            return;
        }
        let slot = o.id.index;
        let counter = w.frame as f32 / 8.0 + i as f32;
        o.sine_img(
            "dreadwood/tentacle",
            pos,
            Vec2::new(16.0, 0.0),
            4,
            counter,
            1.0,
            PI / 16.0,
            [255; 4],
        );
        // Warning: the silhouette flashes until it strikes.
        if t.seq.on && t.seq.step == 2 && (w.frame / 4).is_multiple_of(2) {
            o.sine_img(
                "dreadwood/tentacleFlash",
                pos,
                Vec2::new(16.0, 0.0),
                4,
                counter,
                1.0,
                PI / 16.0,
                [255; 4],
            );
        }
        let _ = slot;
    }

    #[allow(clippy::too_many_arguments)]
    fn darkfang(
        &mut self,
        o: &mut Out,
        w: &World,
        b: &BossState,
        s: &cyclops::State,
        pos: Vec2,
        flash: bool,
        lights: &mut Vec<(Vec2, f32)>,
    ) {
        let slot = b.body.index;
        // Ceiling bones (in front of the arena's top).
        o.depth = -151;
        o.img_at(
            "darkfang/ceiling",
            Vec2::new(160.0, 0.0),
            Vec2::new(70.0, 15.0),
        );
        if !w.core(b.body).visible {
            return;
        }
        o.depth = BACKDROP;
        let head_tex = |mad: i32, wavy: bool| match (mad.min(2), wavy) {
            (0, false) => "darkfang/head",
            (1, false) => "darkfang/headB",
            (_, false) => "darkfang/headC",
            (0, true) => "darkfang/headWavy",
            (1, true) => "darkfang/headWavyB",
            (_, true) => "darkfang/headWavyC",
        };
        let wavy_counter = w.frame as f32 * 0.1;
        // (solid head?, wavy alpha, head offset, eye shown, eye offset)
        let mut solid = false;
        let mut wavy_a = 0.0;
        let mut head_tex_mad = s.mad;
        let mut off = Vec2::ZERO;
        let mut eye = false;
        let mut eye_off = Vec2::ZERO;
        let mut eye_rate = 1.0;
        let mut shake_head = false;
        match s.state {
            cyclops::INTRO => {
                if s.intro.step >= 2 {
                    let t = self.since(slot, clk::INTRO, s.intro.step.min(3) as u64);
                    let t = if s.intro.step == 2 { t } else { t + 180.0 };
                    wavy_a = (t / 240.0).min(1.0);
                }
            }
            cyclops::IDLE => {
                solid = true;
                eye = true;
                let t = self.since(slot, clk::PHASE, s.state as u64 + 16 * s.mad as u64);
                if s.mad > 0 && t < 30.0 {
                    wavy_a = 1.0 - t / 30.0;
                }
            }
            cyclops::MAD => {
                let st = s.mad_seq.step;
                let t = self.since(slot, clk::STEP, st as u64 + 32 * s.mad as u64);
                match st {
                    0..=2 => {
                        solid = true;
                        eye = false;
                        shake_head = st == 2;
                        off.y = 20.0 * cube_out(t / 20.0).min(1.0) * f32::from(st == 2);
                        head_tex_mad = s.mad;
                    }
                    3 => {
                        // The horn is off: the wavy head relaxes and shakes.
                        head_tex_mad = s.mad + 1;
                        wavy_a = 1.0;
                        off.y = 20.0 - 20.0 * back_out((t - 15.0) / 45.0).max(0.0);
                        if t < 15.0 || (60.0..75.0).contains(&t) {
                            off += jitter(w.frame, slot, 2.0);
                        }
                    }
                    4..=11 => {
                        head_tex_mad = s.mad + 1;
                        let t4 = if st == 4 { t } else { 120.0 };
                        off.y = -40.0 * cube_out(t4 / 40.0);
                        wavy_a = 1.0 - 0.7 * (t4 / 120.0).min(1.0);
                    }
                    _ => {
                        head_tex_mad = s.mad + 1;
                        let k = (t / 60.0).min(1.0);
                        off.y = -40.0 + 40.0 * cube_out(k);
                        wavy_a = 0.3 + 0.7 * ((t - 60.0) / 150.0).clamp(0.0, 1.0);
                        if t > 300.0 {
                            solid = true;
                            eye = true;
                        }
                    }
                }
            }
            _ => {
                // Dead.
                head_tex_mad = 2;
                let st = s.death.step;
                let t = self.since(slot, clk::STEP, st as u64 + 64);
                solid = true;
                eye = true;
                match st {
                    0..=3 => {}
                    4 => {
                        off.y = -20.0 * cube_out(t / 30.0);
                    }
                    5 => {
                        off.y = -20.0;
                        shake_head = true;
                        eye_rate = 3.0;
                        if t >= 200.0 {
                            // The head falls away as a fading wavy image.
                            solid = false;
                            let k = ((t - 200.0) / 90.0).min(1.0);
                            wavy_a = 1.0 - k;
                            off.y = -20.0 + 50.0 * k;
                            eye_off.y = -30.0 * cube_out((t - 210.0) / 60.0).max(0.0);
                        }
                    }
                    _ => {
                        solid = false;
                        eye = false;
                    }
                }
            }
        }
        let headp = fl(pos + off)
            + if shake_head {
                jitter(w.frame, slot + 11, 2.0)
            } else {
                Vec2::ZERO
            };
        if wavy_a > 0.0 {
            let amp = 6.0 * (1.0 - wavy_a);
            o.sine_img(
                head_tex(head_tex_mad, true),
                headp,
                HEAD_ORIGIN,
                3,
                wavy_counter,
                amp,
                PI / 8.0,
                alpha(wavy_a),
            );
        }
        if solid {
            o.img_at(head_tex(head_tex_mad, false), headp, HEAD_ORIGIN);
        }
        if eye {
            let et = self.since(slot, clk::OPEN, s.eye_open as u64);
            let ever_open = s.state != cyclops::INTRO;
            let (anim, at) = if s.eye_open {
                let len = 5.0 * 0.08 * 60.0;
                if et * eye_rate < len {
                    ("open", et * eye_rate)
                } else {
                    ("idle", et * eye_rate - len)
                }
            } else if ever_open && et < 30.0 {
                ("close", et)
            } else if ever_open {
                ("close", 99.0)
            } else {
                ("startClosed", 0.0)
            };
            let ep = fl(pos + eye_off)
                + if b.base.flash > 0.0 || shake_head {
                    jitter(w.frame, slot + 5, 2.0)
                } else {
                    Vec2::ZERO
                };
            let sc = 1.0 + 0.1 * wiggle(20.0 - b.base.flash, 30.0, 3.0);
            o.depth = BACKDROP - 1;
            let from = o.items.len();
            if let Some(d) = o.anim("boss:DarkfangEye", anim, at, ep) {
                d.scale = Vec2::new(sc, sc);
            }
            o.outline(from);
            if flash {
                let (orig, offy) =
                    o.c.sprite(o.art.sprite("boss:DarkfangEye"))
                        .map_or((Vec2::new(18.0, 14.0), 9.0), |d| (d.origin, d.position.y));
                if let Some(d) = o.img_at("darkfang/eyeFlash", ep + Vec2::new(0.0, offy), orig) {
                    d.scale = Vec2::new(sc, sc);
                }
            }
            if s.eye_open {
                lights.push((pos + Vec2::new(0.0, 8.0), 60.0));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fist(
        &mut self,
        o: &mut Out,
        w: &World,
        b: &BossState,
        s: &cyclops::State,
        k: u8,
        pos: Vec2,
        lights: &mut Vec<(Vec2, f32)>,
    ) {
        let slot = b.body.index;
        // Lit (white, skeleton animated) or dormant (grey).
        let lit = match s.state {
            cyclops::INTRO => s.intro.step >= 3,
            cyclops::IDLE => true,
            cyclops::MAD => !(8..=10).contains(&s.mad_seq.step),
            _ => s.death.step <= 1,
        };
        let lt = self.since(slot, clk::LIT, lit as u64);
        let v = if lit {
            0.5 + 0.5 * (lt / 30.0).min(1.0)
        } else {
            1.0 - 0.5 * (lt / 30.0).min(1.0)
        };
        let started = !(s.state == cyclops::INTRO && s.intro.step < 3) || lt > 0.0;
        let v = if started { v } else { 0.5 };
        // Cracks: one per finished Mad clap.
        let crack = (s.mad + i32::from(s.state == cyclops::MAD && s.mad_seq.step >= 6)).min(2);
        let name = match (k, crack) {
            (0, 0) => "darkfang/leftFist",
            (0, 1) => "darkfang/leftFistB",
            (0, _) => "darkfang/leftFistC",
            (_, 0) => "darkfang/rightFist",
            (_, 1) => "darkfang/rightFistB",
            (_, _) => "darkfang/rightFistC",
        };
        let shaking = (s.state == cyclops::MAD && s.mad_seq.step <= 2)
            || (s.state == cyclops::DEAD && (2..=3).contains(&s.death.step));
        let j = if shaking {
            jitter(w.frame, slot + k as u32 * 13 + 1, 1.0)
        } else {
            Vec2::ZERO
        };
        let rise = if s.state == cyclops::DEAD && s.death.step == 3 {
            Vec2::new(
                0.0,
                -20.0 * cube_out(self.since(slot, clk::SUB + 1, 3) / 20.0),
            )
        } else {
            Vec2::ZERO
        };
        if let Some(d) = o.img(name, pos + j + rise, (0.5, 0.5)) {
            d.tint = gray(v, 1.0);
        }
        let anim = if lit { "idle" } else { "still" };
        if let Some(d) = o.anim(
            "boss:DarkfangSkeleton",
            anim,
            w.frame as f32,
            pos + j + rise,
        ) {
            d.tint = [255, 255, 255, 128];
            if k == 1 {
                d.scale.x = -1.0;
            }
        }
        if lit {
            lights.push((pos, 50.0 * (lt / 20.0).min(1.0)));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cataclysm(
        &mut self,
        o: &mut Out,
        w: &World,
        b: &BossState,
        s: &cataclysm::State,
        pos: Vec2,
        flash: bool,
        lights: &mut Vec<(Vec2, f32)>,
    ) {
        let slot = b.body.index;
        // Pupil look: toward the target player, eased.
        let target = s
            .target
            .filter(|&t| w.exists(t))
            .map(|t| (w.core(t).pos - w.core(b.body).pos).safe_normalize())
            .unwrap_or(Vec2::new(0.0, 0.3));
        self.look = approach(self.look, target, 0.1);
        let look = self.look;
        let intro_frozen = s.state == cataclysm::INTRO && s.seq.step <= 3;
        let shaking = (s.state == cataclysm::INTRO && s.seq.step == 3)
            || (s.closed_t > 0.0 && s.closed_t < 30.0);
        let p = pos
            + if shaking {
                jitter(w.frame, slot, 1.0)
            } else {
                Vec2::ZERO
            };
        let wig = 1.0 + 0.1 * wiggle(20.0 - self.hit, 30.0, 4.0);
        lights.push((pos, 120.0));
        if intro_frozen {
            let f = 7 + (s.seq.k.clamp(0, 4)) as i16;
            let from = o.items.len();
            o.cell("boss:CataclysmBody", f, p);
            o.outline(from);
            return;
        }
        let heart_dead = s.heart && s.heart_health <= 0;
        let dying = s.state == cataclysm::DEAD && !s.heart && s.seq.step >= 4;
        // Eyeball and pupil (hidden once the shell bursts).
        if !s.heart && !dying {
            let enraged = s.state == cataclysm::EXPLODING && s.seq.step >= 8;
            let ct = self.since(
                slot,
                clk::CHARGE,
                (s.charging as u64) | ((enraged as u64) << 1),
            );
            let red = if s.charging || enraged {
                (ct / 60.0).min(1.0)
            } else {
                0.0
            };
            let tint = [
                255,
                (255.0 - 155.0 * red) as u8,
                (255.0 - 155.0 * red) as u8,
                255,
            ];
            if let Some(d) = o.img("cataclysm/eyeball", p, (0.5, 0.5)) {
                d.tint = tint;
            }
            let ot = self.since(slot, clk::OPEN, s.open as u64);
            let (anim, at) = if enraged {
                ("die", ct)
            } else if s.charging {
                ("charge", ct)
            } else if s.open {
                ("open", ot)
            } else {
                ("close", ot)
            };
            let psc = if s.charging {
                1.0 - 0.3 * cube_in(ct / 60.0)
            } else {
                1.0
            };
            if let Some(d) = o.anim(
                "boss:CataclysmPupil",
                anim,
                at,
                fl(p + look * Vec2::new(7.5, 5.0)),
            ) {
                d.scale = Vec2::new(psc, psc);
            }
            if flash {
                o.img("cataclysm/eyeballFlash", p, (0.5, 0.5));
            }
        }
        // The shell / the heart.
        let (anim, at, frame_override) = if s.heart {
            if heart_dead {
                ("heart", 0.0, Some(20))
            } else {
                ("heart", w.frame as f32 % 54.0, None)
            }
        } else if dying || (s.state == cataclysm::DEAD && s.seq.step >= 3) {
            let t = self.since(slot, clk::STEP, 3);
            ("die", t, None)
        } else {
            let ot = self.since(slot, clk::OPEN, s.open as u64);
            if s.state == cataclysm::INTRO && s.seq.step >= 4 && !s.open {
                ("close", ot, None)
            } else if s.open {
                ("open", ot, None)
            } else {
                ("close", ot, None)
            }
        };
        let body_a = if heart_dead {
            let t = self.since(slot, clk::SUB, 8);
            1.0 - (t / 180.0).min(1.0)
        } else {
            1.0
        };
        if heart_dead && body_a > 0.0 {
            o.cell("boss:CataclysmBody", 23, p);
        }
        if heart_dead && body_a <= 0.0 {
            let t = self.since(slot, clk::SUB, 8);
            dust(
                o,
                "cataclysm/heartFade",
                p,
                Vec2::new(14.0, 36.0),
                t - 180.0,
                1.0,
            );
        }
        if body_a > 0.0 {
            let f = match frame_override {
                Some(f) => f,
                None => {
                    o.c.sprite(o.art.sprite("boss:CataclysmBody"))
                        .map_or(0, |d| frame_at(d, anim, at))
                }
            };
            let from = o.items.len();
            if let Some(d) = o.cell("boss:CataclysmBody", f, p) {
                d.scale = Vec2::new(wig, wig);
                d.tint = alpha(body_a);
            }
            if !s.heart {
                o.outline(from);
            }
            if s.heart && self.heart_hit > 0.0 && ((self.heart_hit as u32) / 3).is_multiple_of(2) {
                let hf = (f - 20).clamp(0, 2);
                if let Some(t) = o.art.tex("cataclysm/heartFlash") {
                    o.push(
                        Visual::Sprite {
                            tex: t,
                            frame: hf,
                            frame_w: 85,
                            frame_h: 85,
                        },
                        p,
                        Vec2::new(42.0, 42.0),
                    );
                }
            }
        }
    }
}

fn approach(v: Vec2, t: Vec2, step: f32) -> Vec2 {
    let ax = |a: f32, b: f32| {
        if a < b {
            (a + step).min(b)
        } else {
            (a - step).max(b)
        }
    };
    Vec2::new(ax(v.x, t.x), ax(v.y, t.y))
}

/// "Turn to dust" (`TurnToDustImage`, FallAndSweep): every pixel of the
/// image falls on its own: rows go from the bottom up (row `k` from the
/// bottom waits `4 k` + 0..9 ticks), then each pixel drops at 0.2 px/tick
/// while a leftward sweep grows from 0 to 0.6 px/tick between 60 and 480
/// ticks; its alpha starts at 4 and falls 0.05 per tick.
fn dust(o: &mut Out, tex: &str, pos: Vec2, origin: Vec2, t: f32, a: f32) {
    if t < 0.0 {
        return;
    }
    let Some(id) = o.art.tex(tex) else { return };
    let Some((w, h)) = o.c.texture(id).size else {
        return;
    };
    // The sweep's travel: integral of 0.6 * clamp((tau - 60) / 420) dtau.
    let sweep = |tau: f32| {
        if tau <= 60.0 {
            0.0
        } else if tau <= 480.0 {
            0.6 * (tau - 60.0) * (tau - 60.0) / 840.0
        } else {
            0.6 * 210.0 + 0.6 * (tau - 480.0)
        }
    };
    let origin = Vec2::new(origin.x.round(), origin.y.round());
    let base = fl(pos);
    for y in 0..h {
        let row_from_bottom = (h - 1 - y) as f32;
        for x in 0..w {
            let hsh = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
            let delay = 4.0 * row_from_bottom + (hsh % 10) as f32;
            let moving = (t - delay).max(0.0);
            let al = (4.0 - 0.05 * moving).min(1.0);
            if al <= 0.0 {
                continue;
            }
            let dx = if t > delay {
                -(sweep(t) - sweep(delay))
            } else {
                0.0
            };
            let off = Vec2::new(x as f32, y as f32) - origin + Vec2::new(dx, 0.2 * moving);
            let d = o.push(
                Visual::Sprite {
                    tex: id,
                    frame: (y as u32 * w as u32 + x as u32) as i16,
                    frame_w: 1,
                    frame_h: 1,
                },
                base + Vec2::new(off.x.floor(), off.y.floor()),
                Vec2::ZERO,
            );
            d.tint = alpha(al * a);
        }
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn easings_and_jitter() {
        assert!((cube_out(1.0) - 1.0).abs() < 1e-6);
        assert!((back_out(1.0) - 1.0).abs() < 1e-5);
        assert!(back_in(0.3) < 0.0, "BackIn dips below zero");
        assert_eq!(wiggle(50.0, 30.0, 3.0), 0.0);
        let j = jitter(10, 3, 2.0);
        assert!(j.x.abs() <= 2.0 && j.y.abs() <= 2.0);
        assert_eq!(jitter(10, 3, 2.0), j, "deterministic");
        let a = approach(Vec2::ZERO, Vec2::new(1.0, -1.0), 0.25);
        assert_eq!(a, Vec2::new(0.25, -0.25));
    }
}
