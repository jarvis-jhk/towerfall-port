//! Cosmetic death effects of enemies and dummies (enemies.md 4.x "Die",
//! trials.md 8): ragdoll corpses, loose heads, slime eyes, halos, wings,
//! weapons, staffs, mole shells, fallen crystals, the mimic's shadow
//! burst. The sim removes these enemies (or keeps only a death
//! animation); the presentation spawns the leftovers when it sees an enemy
//! die and simulates them locally against the level's solid grid (screen
//! wrap) with the original's three debris families:
//!
//! * light (`SlimeEye`): gravity 0.15 (half near the apex), terminal 3,
//!   bounces keep -0.2;
//! * heavy (`HeavyPhysicsObject`, `MoleShell`): gravity 0.3 (half near the
//!   apex), terminal 3, walls keep -0.5, floors bounce -0.5 from vy >= 2
//!   with a random sideways kick, friction 0.2 on the ground where the
//!   piece settles upright (pi/30 per tick toward 0 or pi); a bounce
//!   re-rolls the spin (4..15 degrees the other way) and plays the piece's
//!   drop sound; 180 ticks, then a 120-tick fade;
//! * floaty (bat wings): gravity 0.08 to 1.2, swaying on a 50..80-tick sine
//!   (rotation -40 degrees per px of vx), friction 0.1 on the ground,
//!   settling at pi/36; the drop sound on landing.

use std::collections::BTreeMap;

use tfp_sim::content::SpriteId;
use tfp_sim::enemies::{EnemyType, SkelKind};
use tfp_sim::entity::{EntityId, Kind};
use tfp_sim::math::PI;
use tfp_sim::{Content, Drawable, Vec2, View, Visual, World};

use super::art::{Art, frame_at};

#[derive(Clone, Debug)]
enum Look {
    Tex(&'static str),
    /// A `w x h` cell `frame` of a texture (the fallen crystal).
    Cell(&'static str, i16, u16, u16),
    /// Corpse sprite: "fall" in the air, "ground" when resting.
    Corpse(CorpseRef),
    /// A sprite animation (`MoleShell` switches to `shellGround`).
    Sprite(&'static str, &'static str),
}

/// A corpse sprite: registered by name (`corpse:<id>`) or an archer's.
#[derive(Clone, Copy, Debug)]
enum CorpseRef {
    Name(&'static str),
    Id(SpriteId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phys {
    Light,
    Heavy,
    Floaty,
    /// Ragdoll corpses (no spin, slow fall).
    Corpse,
    /// In place (one-shot effects).
    Still,
    /// The King Reaper's head (`KingHead`): heavy, stays; on the ground it
    /// rolls (`rot -= vx`) and settles on its side; floors bounce -0.8.
    KingHead,
}

#[derive(Clone, Debug)]
struct Piece {
    look: Look,
    phys: Phys,
    pos: Vec2,
    vel: Vec2,
    rot: f32,
    spin: f32,
    facing: f32,
    age: u32,
    /// Ticks before fading, then `fade` ticks of fade.
    life: u32,
    fade: u32,
    on_ground: bool,
    /// Played on each bounce (heavy) / landing (floaty).
    sound: Option<&'static str>,
    /// Floaty sway: period and phase.
    sway: (f32, f32),
    /// `MoleShell`: its sprite switches to `shellGround` once it landed.
    landed: bool,
}

/// What the tracker remembers of a living enemy.
#[derive(Clone, Copy, Debug)]
struct Seen {
    ty: EnemyType,
    pos: Vec2,
    speed: Vec2,
    facing: i8,
    alive: bool,
}

/// Particle bursts the scene should emit (catalogue name, count, position).
pub type Burst = (&'static str, usize, Vec2);

#[derive(Default)]
pub struct Debris {
    pieces: Vec<Piece>,
    seen: BTreeMap<EntityId, Seen>,
    dummies: BTreeMap<EntityId, (Vec2, i8)>,
    /// Crystals (alive or dead) and where they were: the fallen crystal
    /// drops when the entity goes.
    crystals: BTreeMap<EntityId, (Vec2, EnemyType)>,
    /// Mimics that copied a player: the archer (content index).
    mimics: BTreeMap<EntityId, u16>,
    /// King Reapers: position and whether dying (the head drops when the
    /// dying boss goes).
    reapers: BTreeMap<EntityId, (Vec2, bool)>,
    sounds: Vec<(&'static str, f32)>,
    rng: u64,
}

fn crystal_tex(ty: EnemyType) -> &'static str {
    match ty {
        EnemyType::BoltCrystal => "enemies/crystal/greenCrystal",
        EnemyType::PrismCrystal => "enemies/crystal/prismCrystal",
        _ => "enemies/crystal/crystal",
    }
}

impl Debris {
    pub fn clear(&mut self) {
        self.pieces.clear();
        self.seen.clear();
        self.dummies.clear();
        self.crystals.clear();
        self.mimics.clear();
        self.reapers.clear();
        self.sounds.clear();
    }

    /// Drop / bounce sounds since the last call `(name, x)`.
    pub fn take_sounds(&mut self) -> Vec<(&'static str, f32)> {
        std::mem::take(&mut self.sounds)
    }

    fn rand(&mut self) -> f32 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.rng >> 33) as f32) / (1u64 << 31) as f32
    }

    /// `Calc.Random.Range(min, add)` for integers: `[min, min + add)`.
    fn range_i(&mut self, min: i32, add: i32) -> f32 {
        (min + (self.rand() * add as f32) as i32).min(min + add - 1) as f32
    }

    fn spin(&mut self) -> f32 {
        (4.0 + 12.0 * self.rand()).to_radians() * if self.rand() < 0.5 { -1.0 } else { 1.0 }
    }

    fn piece(&mut self, look: Look, phys: Phys, pos: Vec2, vel: Vec2, facing: f32) -> &mut Piece {
        let spin = self.spin();
        let sway = (
            50.0 + 30.0 * self.rand(),
            self.rand() * std::f32::consts::TAU,
        );
        self.pieces.push(Piece {
            look,
            phys,
            pos,
            vel,
            rot: 0.0,
            spin,
            facing,
            age: 0,
            life: 180,
            fade: 120,
            on_ground: false,
            sound: None,
            sway,
            landed: false,
        });
        self.pieces.last_mut().unwrap()
    }

    /// A `HeavyPhysicsObject` with its random start speed
    /// (`x in {-1, 0}`, `y in {-4, -3}`) and drop sound.
    fn heavy(&mut self, tex: &'static str, pos: Vec2, sound: &'static str, facing: f32) {
        let v = Vec2::new(self.range_i(-1, 2), self.range_i(-4, 2));
        let p = self.piece(Look::Tex(tex), Phys::Heavy, pos, v, facing);
        p.sound = Some(sound);
    }

    fn corpse(&mut self, sprite: CorpseRef, pos: Vec2, vel: Vec2, facing: f32) {
        let p = self.piece(
            Look::Corpse(sprite),
            Phys::Corpse,
            pos,
            vel + Vec2::new(0.0, -1.5),
            facing,
        );
        p.spin = 0.0;
        p.life = 900;
        p.fade = 60;
    }

    /// The mimic's shadow burst (`DoMimicExplodeEffect`): `ShadowExplosion`
    /// one-shot (12 frames of 0.05 s) in place.
    fn shadow_burst(&mut self, pos: Vec2) {
        let p = self.piece(
            Look::Sprite("ShadowExplosion", "0"),
            Phys::Still,
            pos,
            Vec2::ZERO,
            1.0,
        );
        p.life = 36;
        p.fade = 0;
    }

    /// After each sim step: spawns leftovers for enemies that died or
    /// vanished alive (removed with their death), and for dead dummies.
    pub fn after_step(&mut self, w: &World, c: &Content, bursts: &mut Vec<Burst>) {
        let mut now: BTreeMap<EntityId, Seen> = BTreeMap::new();
        let mut dummies = BTreeMap::new();
        let mut crystals = BTreeMap::new();
        let mut reapers = BTreeMap::new();
        for id in w.entities() {
            match w.kind(id) {
                Some(Kind::Enemy(e)) => {
                    let pos = w.core(id).pos;
                    now.insert(
                        id,
                        Seen {
                            ty: e.ty,
                            pos,
                            speed: e.speed,
                            facing: e.facing,
                            alive: e.alive(),
                        },
                    );
                    if e.ty == EnemyType::KingReaper {
                        reapers.insert(id, (pos, e.state == tfp_sim::enemies::reaper::DEAD));
                    }
                    if matches!(
                        e.ty,
                        EnemyType::EvilCrystal
                            | EnemyType::BlueCrystal
                            | EnemyType::BoltCrystal
                            | EnemyType::PrismCrystal
                    ) {
                        crystals.insert(id, (pos, e.ty));
                    }
                    // A mimic copying a player: shadow burst, its look kept.
                    if let EnemyType::Skeleton(k) = e.ty
                        && k.mimic
                        && tfp_sim::enemies::skeleton::mimicked(e)
                        && !self.mimics.contains_key(&id)
                    {
                        let archer = e.target.and_then(|t| w.player(t)).map_or(0, |p| p.archer);
                        self.mimics.insert(id, archer);
                        self.shadow_burst(pos);
                    }
                }
                Some(Kind::Dummy(d)) if !d.dead => {
                    dummies.insert(id, (w.core(id).pos, d.facing));
                }
                _ => {}
            }
        }
        let old = std::mem::take(&mut self.seen);
        for (id, s) in &old {
            let died = match now.get(id) {
                Some(n) => s.alive && !n.alive,
                None => s.alive,
            };
            if died {
                let n = now.get(id).copied().unwrap_or(*s);
                let mimic = self.mimics.get(id).copied();
                self.death(c, n.ty, n.pos, n.speed, n.facing, mimic, bursts);
            }
        }
        // Crystals: the fallen crystal when the entity is gone (after its
        // death animation and posthumous shots; enemies.md 4.9).
        for (id, (pos, ty)) in std::mem::take(&mut self.crystals) {
            if !crystals.contains_key(&id) {
                let p = self.piece(
                    Look::Cell(crystal_tex(ty), 5, 20, 20),
                    Phys::Heavy,
                    pos,
                    Vec2::ZERO,
                    1.0,
                );
                p.sound = Some("en_crystalFall");
            }
        }
        self.crystals = crystals;
        // The King Reaper's death ends with its head falling (`KingHead`).
        for (id, (pos, dying)) in std::mem::take(&mut self.reapers) {
            if dying && !reapers.contains_key(&id) {
                let p = self.piece(
                    Look::Tex("kingReaper/kingHead"),
                    Phys::KingHead,
                    pos + Vec2::new(0.0, -10.0),
                    Vec2::ZERO,
                    1.0,
                );
                p.life = u32::MAX / 2;
                p.fade = 0;
            }
        }
        self.reapers = reapers;
        self.mimics.retain(|id, _| w.exists(*id));
        // Squished enemies vanish without a body; the sim's kill flags are
        // not visible here, so every alive-vanish gets the death leftovers.
        for (id, (pos, facing)) in std::mem::take(&mut self.dummies) {
            if !dummies.contains_key(&id) {
                self.corpse(
                    CorpseRef::Name("corpse:Dummy"),
                    pos + Vec2::new(0.0, 12.0),
                    Vec2::new(0.0, -1.0),
                    facing as f32,
                );
                let v = Vec2::new(self.rand() * 2.0 - 1.0, -2.5);
                self.piece(
                    Look::Tex("dummy/head"),
                    Phys::Light,
                    pos + Vec2::new(0.0, 2.0),
                    v,
                    1.0,
                );
                bursts.push(("DummyDie", 12, pos + Vec2::new(0.0, 10.0)));
            }
        }
        self.dummies = dummies;
        self.seen = now;
        self.update(w, c);
    }

    #[allow(clippy::too_many_arguments)]
    fn death(
        &mut self,
        c: &Content,
        ty: EnemyType,
        pos: Vec2,
        speed: Vec2,
        facing: i8,
        mimic: Option<u16>,
        bursts: &mut Vec<Burst>,
    ) {
        use EnemyType::*;
        let f = facing as f32;
        match ty {
            Slime | RedSlime | BlueSlime => {
                // `SlimeEye`: x in {-1, 0}, y = -2; 120 ticks + 60 of fade.
                let v = Vec2::new(self.range_i(-1, 2), -2.0);
                let p = self.piece(
                    Look::Tex("enemies/slime/eye"),
                    Phys::Light,
                    pos + Vec2::new(0.0, -4.0),
                    v,
                    f,
                );
                p.life = 120;
                p.fade = 60;
            }
            Ghost | GreenGhost => {
                self.heavy(
                    "enemies/ghost/halo",
                    pos + Vec2::new(0.0, -5.0),
                    "sfx_endropHalo",
                    f,
                );
            }
            Bat | BombBat | SuperBombBat => {
                let wing = if ty == Bat {
                    "enemies/bat/eyeBatWing"
                } else {
                    "enemies/bat/bomBatWing"
                };
                for s in [-1.0, 1.0] {
                    let vy = -1.0 - 1.2 * self.rand();
                    let p = self.piece(
                        Look::Tex(wing),
                        Phys::Floaty,
                        pos + Vec2::new(3.0 * s, -3.0),
                        Vec2::new(0.0, vy),
                        s,
                    );
                    p.sound = Some("sfx_endropWings");
                }
            }
            Crow => bursts.push(("EnBirdDie", 14, pos)),
            Cultist | ScytheCultist | BossCultist => {
                let name = match ty {
                    Cultist => "corpse:PurpleCultist",
                    ScytheCultist => "corpse:GreenCultist",
                    _ => "corpse:RedCultist",
                };
                self.corpse(CorpseRef::Name(name), pos + Vec2::new(0.0, -8.0), speed, f);
                if ty == ScytheCultist {
                    self.heavy(
                        "enemies/cultist/scytheHandle",
                        pos + Vec2::new(0.0, -8.0),
                        "sfx_endropScyHandle",
                        f,
                    );
                    self.heavy(
                        "enemies/cultist/scytheHead",
                        pos + Vec2::new(4.0 * f, -8.0),
                        "sfx_endropKnife",
                        f,
                    );
                } else {
                    self.heavy(
                        "enemies/cultist/knife",
                        pos + Vec2::new(4.0 * f, -8.0),
                        "sfx_endropKnife",
                        f,
                    );
                }
            }
            Birdman | DarkBirdman => {
                let name = if ty == Birdman {
                    "corpse:Birdman"
                } else {
                    "corpse:DarkBirdman"
                };
                self.corpse(CorpseRef::Name(name), pos + Vec2::new(0.0, -8.0), speed, f);
                bursts.push((
                    if ty == Birdman {
                        "EnBirdDie"
                    } else {
                        "EnDarkBirdman"
                    },
                    20,
                    pos,
                ));
            }
            Skeleton(k) => self.skeleton(c, k, pos, speed, f, mimic, bursts),
            Mole => {
                // `MoleShell`: x in {-2, -1, 0, 1}, y = -3 (6x6, heavy).
                let v = Vec2::new(self.range_i(-2, 4), -3.0);
                self.piece(Look::Sprite("Mole", "shell"), Phys::Heavy, pos, v, f);
            }
            TechnoMage => {
                self.heavy(
                    "enemies/technomage/Staff",
                    pos + Vec2::new(5.0 * f, -4.0),
                    "sfx_endropStaffHead",
                    f,
                );
                bursts.push(("EnTechnoMageDead", 10, pos));
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn skeleton(
        &mut self,
        c: &Content,
        k: SkelKind,
        pos: Vec2,
        speed: Vec2,
        f: f32,
        mimic: Option<u16>,
        bursts: &mut Vec<Burst>,
    ) {
        if k.wings {
            bursts.push(("WingsDie", 12, pos + Vec2::new(0.0, -4.0)));
        }
        // A mimic that copied a player leaves that archer's corpse (and a
        // shadow burst); an unused mimic its own.
        if let Some(a) = mimic {
            let sprite = c
                .archers
                .get(a as usize)
                .map_or(SpriteId::NONE, |d| d.corpse);
            self.corpse(CorpseRef::Id(sprite), pos, speed, f);
            self.shadow_burst(pos);
            return;
        }
        if k.jester {
            let sprite = c.archers.get(6).map_or(SpriteId::NONE, |d| d.corpse);
            self.corpse(CorpseRef::Id(sprite), pos, speed, f);
            return;
        }
        if k.mimic {
            self.corpse(CorpseRef::Name("corpse:Mimic"), pos, speed, f);
            return;
        }
        self.corpse(CorpseRef::Name("corpse:Skeleton"), pos, speed, f);
        let head = if k.arrow == tfp_sim::quiver::ArrowType::Bomb {
            "enemies/bombSkeleton/looseHead"
        } else {
            "enemies/skeleton/looseHead"
        };
        self.heavy(head, pos + Vec2::new(0.0, -6.0), "sfx_endropSkull", f);
    }

    fn update(&mut self, w: &World, c: &Content) {
        let ws = w.world_size(c);
        let c_size = (ws.w, ws.h);
        let lvl = c.level(w.level);
        let solid = |p: Vec2| {
            let (x, y) = (
                p.x.rem_euclid(c_size.0).floor() as i32 / 10,
                p.y.rem_euclid(c_size.1).floor() as i32 / 10,
            );
            lvl.solid(x, y)
        };
        let mut sounds = Vec::new();
        let mut rolls: Vec<f32> = (0..self.pieces.len() * 2).map(|_| 0.0).collect();
        for r in rolls.iter_mut() {
            *r = self.rand();
        }
        for (i, p) in self.pieces.iter_mut().enumerate() {
            p.age += 1;
            if p.phys == Phys::Still {
                continue;
            }
            let (r0, r1) = (rolls[2 * i], rolls[2 * i + 1]);
            let reroll = |spin: f32| {
                let s = (4.0 + (r0 * 12.0).floor().min(11.0)).to_radians();
                if spin >= 0.0 { -s } else { s }
            };
            p.on_ground = solid(p.pos + Vec2::new(0.0, 1.0));
            match p.phys {
                Phys::Floaty => {
                    if p.on_ground {
                        p.rot = approach_angle(p.rot, 0.0, PI / 36.0);
                        p.vel.x = approach(p.vel.x, 0.0, 0.1);
                    } else {
                        let sine =
                            (p.age as f32 / p.sway.0 * std::f32::consts::TAU + p.sway.1).sin();
                        p.vel.x = approach(p.vel.x, sine, 1.0);
                        p.rot = p.vel.x * -40.0_f32.to_radians();
                        p.vel.y = (p.vel.y + 0.08).min(1.2);
                    }
                }
                Phys::Light | Phys::Heavy => {
                    let g = if p.phys == Phys::Heavy { 0.3 } else { 0.15 };
                    if p.on_ground {
                        p.vel.x = approach(p.vel.x, 0.0, 0.2);
                        // Settle upright (toward 0 or pi, the nearer).
                        let target = if (p.rot.rem_euclid(2.0 * PI) - PI).abs() < PI / 2.0 {
                            PI
                        } else {
                            0.0
                        };
                        p.rot = approach_angle(p.rot, target, PI / 30.0);
                    } else {
                        let k = if p.vel.y.abs() <= 0.5 { 0.5 } else { 1.0 };
                        p.vel.y = (p.vel.y + g * k).min(3.0);
                        p.rot += p.spin;
                    }
                }
                Phys::KingHead => {
                    if p.on_ground {
                        p.vel.x = approach(p.vel.x, 0.0, 0.2);
                        if p.vel.x.abs() < 0.2 {
                            let side = if (p.rot.rem_euclid(2.0 * PI)) < PI {
                                PI / 2.0
                            } else {
                                -PI / 2.0
                            };
                            p.rot = approach_angle(p.rot, side, PI / 30.0);
                        } else {
                            p.rot -= p.vel.x;
                        }
                    } else {
                        let k = if p.vel.y.abs() <= 0.5 { 0.5 } else { 1.0 };
                        p.vel.y = (p.vel.y + 0.3 * k).min(3.0);
                        p.rot += p.spin;
                    }
                }
                Phys::Corpse => {
                    p.vel.y = (p.vel.y + 0.15).min(2.7);
                    if p.on_ground {
                        p.vel.x = approach(p.vel.x, 0.0, 0.2);
                    }
                }
                Phys::Still => {}
            }
            // Horizontal move.
            let nx = p.pos + Vec2::new(p.vel.x, 0.0);
            if p.vel.x != 0.0 && solid(nx) {
                match p.phys {
                    Phys::Light => p.vel.x *= -0.2,
                    Phys::Heavy => {
                        p.vel.x *= -0.5;
                        p.spin = reroll(p.spin);
                        if let Some(s) = p.sound {
                            sounds.push((s, p.pos.x));
                        }
                    }
                    Phys::Corpse => p.vel.x *= -0.3,
                    Phys::KingHead => {
                        p.vel.x *= -0.8;
                        p.spin = reroll(p.spin);
                    }
                    _ => p.vel.x = 0.0,
                }
            } else {
                p.pos = nx;
            }
            // Vertical move.
            let ny = p.pos + Vec2::new(0.0, p.vel.y);
            if p.vel.y != 0.0 && solid(ny) {
                let down = p.vel.y > 0.0;
                match p.phys {
                    Phys::Light | Phys::Heavy => {
                        let keep = if p.phys == Phys::Heavy { -0.5 } else { -0.2 };
                        p.spin = reroll(p.spin);
                        if p.vel.y >= 2.0 {
                            p.vel.x += r1 * 2.0 - 1.0;
                            p.vel.y *= keep;
                        } else {
                            p.vel.y = 0.0;
                        }
                        if down {
                            p.landed = true;
                        }
                        if p.phys == Phys::Heavy
                            && let Some(s) = p.sound
                        {
                            sounds.push((s, p.pos.x));
                        }
                    }
                    Phys::KingHead => {
                        p.spin = reroll(p.spin);
                        if p.vel.y >= 2.0 {
                            sounds.push(("boss_deathHeadFall", p.pos.x));
                            p.vel.x += if r1 < 0.5 { -1.0 } else { 1.0 };
                            p.vel.y *= -0.8;
                        } else {
                            p.vel.y = 0.0;
                        }
                    }
                    Phys::Floaty => {
                        p.vel.y = 0.0;
                        if let Some(s) = p.sound.take() {
                            sounds.push((s, p.pos.x));
                        }
                    }
                    _ => {
                        p.vel.y = if p.vel.y > 1.5 { p.vel.y * -0.3 } else { 0.0 };
                    }
                }
            } else {
                p.pos = ny;
            }
            p.pos = Vec2::new(p.pos.x.rem_euclid(c_size.0), p.pos.y.rem_euclid(c_size.1));
        }
        self.sounds.extend(sounds);
        self.pieces.retain(|p| p.age < p.life + p.fade);
        if self.pieces.len() > 80 {
            let cut = self.pieces.len() - 80;
            self.pieces.drain(..cut);
        }
    }

    /// Appends the pieces to the frame's drawables (depth -60, in front of
    /// the tiles, behind the archers).
    pub fn draw(&self, view: &mut View, c: &Content, art: &Art) {
        let id = EntityId {
            index: u32::MAX,
            generation: 0,
        };
        for p in &self.pieces {
            let fade = if p.age > p.life && p.fade > 0 {
                1.0 - (p.age - p.life) as f32 / p.fade as f32
            } else {
                1.0
            };
            let tint = [255, 255, 255, (fade.clamp(0.0, 1.0) * 255.0) as u8];
            let (visual, origin, offset, rot) = match &p.look {
                Look::Tex(name) => {
                    let Some(t) = art.tex(name) else { continue };
                    let Some(size) = c.texture(t).size else {
                        continue;
                    };
                    (
                        Visual::Sprite {
                            tex: t,
                            frame: -1,
                            frame_w: 0,
                            frame_h: 0,
                        },
                        Vec2::new(size.0 as f32 / 2.0, size.1 as f32 / 2.0),
                        Vec2::new(0.0, 0.0),
                        p.rot,
                    )
                }
                Look::Cell(name, frame, fw, fh) => {
                    let Some(t) = art.tex(name) else { continue };
                    (
                        Visual::Sprite {
                            tex: t,
                            frame: *frame,
                            frame_w: *fw,
                            frame_h: *fh,
                        },
                        Vec2::new(*fw as f32 / 2.0, *fh as f32 / 2.0),
                        Vec2::ZERO,
                        p.rot,
                    )
                }
                Look::Corpse(r) => {
                    let sprite = match r {
                        CorpseRef::Name(n) => art.sprite(n),
                        CorpseRef::Id(id) => *id,
                    };
                    let Some(s) = c.sprite(sprite) else {
                        continue;
                    };
                    let anim = if p.on_ground { "ground" } else { "fall" };
                    (
                        Visual::Sprite {
                            tex: s.texture,
                            frame: frame_at(s, anim, p.age as f32),
                            frame_w: s.frame_w,
                            frame_h: s.frame_h,
                        },
                        s.origin,
                        s.position,
                        0.0,
                    )
                }
                Look::Sprite(name, anim) => {
                    let Some(s) = c.sprite(art.sprite(name)) else {
                        continue;
                    };
                    let anim = if *anim == "shell" && p.landed {
                        "shellGround"
                    } else {
                        anim
                    };
                    (
                        Visual::Sprite {
                            tex: s.texture,
                            frame: frame_at(s, anim, p.age as f32),
                            frame_w: s.frame_w,
                            frame_h: s.frame_h,
                        },
                        s.origin,
                        s.position,
                        p.rot,
                    )
                }
            };
            view.items.push(Drawable {
                visual,
                pos: p.pos + offset,
                origin,
                scale: Vec2::new(p.facing.signum(), 1.0),
                rotation: rot,
                flip_x: false,
                flip_y: false,
                tint,
                depth: -60,
                entity: id,
            });
        }
    }
}

fn approach(v: f32, t: f32, step: f32) -> f32 {
    if v < t {
        (v + step).min(t)
    } else {
        (v - step).max(t)
    }
}

/// Rotate `v` toward `t` by at most `step` (shortest way).
fn approach_angle(v: f32, t: f32, step: f32) -> f32 {
    let mut d = (t - v).rem_euclid(2.0 * PI);
    if d > PI {
        d -= 2.0 * PI;
    }
    v + d.clamp(-step, step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let mut d = Debris::default();
        for _ in 0..50 {
            let x = d.range_i(-2, 4);
            assert!((-2.0..=1.0).contains(&x) && x.fract() == 0.0);
        }
        assert!((approach_angle(0.1, 0.0, 0.05) - 0.05).abs() < 1e-6);
        assert!((approach_angle(6.2, 0.0, 0.5) - 2.0 * PI).abs() < 0.1);
    }
}
