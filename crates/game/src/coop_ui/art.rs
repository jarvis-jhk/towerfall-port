//! Enemy, portal, hazard, dummy and reviver art (enemies.md, trials.md 8,
//! versus.md 7.3) as sim `Drawable`s: the sprites come from the player's
//! sprite data (registered into the sim's `Content` once, like the archers'
//! art) and replace the sim's debug boxes in the frame's `View`, so they go
//! through the same depth-sorted renderer path as the archers.
//!
//! Animation clocks are presentation state: one per entity, restarted when
//! the chosen (sprite, animation) changes, counted in sim frames.

use std::collections::{BTreeMap, BTreeSet};

use tfp_assets::atlas::{Atlas, AtlasKind};
use tfp_assets::sprite_data::SpriteData;
use tfp_sim::content::{SpriteDef, SpriteId, TexId};
use tfp_sim::enemies::hazard::HazardType;
use tfp_sim::enemies::{self, Enemy, EnemyType};
use tfp_sim::entity::{EntityId, Kind};
use tfp_sim::revive::ReviveMode;
use tfp_sim::{Content, Drawable, Vec2, View, Visual, World};

use std::f32::consts::PI;

/// Main-atlas sprite-data ids used here.
const MAIN_SPRITES: &[&str] = &[
    "SpawnPortal",
    "Slime",
    "RedSlime",
    "BlueSlime",
    "EyeBat",
    "BomBat",
    "SuperBomBat",
    "Crow",
    "Ghost",
    "GreenGhost",
    "FireElemental",
    "GreenFireElemental",
    "Cultist",
    "ScytheCultist",
    "RedCultist",
    "Birdman",
    "BirdmanAlt",
    "Whirlwind",
    "SkeletonBody",
    "SkeletonHead",
    "BombSkeletonBody",
    "BombSkeletonHead",
    "ShadowBody",
    "ShadowHead",
    "SkeletonBow",
    "Mole",
    "MoleExplosion",
    "Worm",
    "flamingSkull",
    "Exploder",
    "Crystal",
    "BlueCrystal",
    "GreenCrystal",
    "PrismCrystal",
    "Technomage",
    "PlayerGhost",
    "Dummy",
    "Wings",
    "Shield",
    "ShadowExplosion",
    "StunStars",
    "WaveNumerals",
    "Gem0",
    "Gem1",
    "Gem2",
    "Gem3",
    "Gem4",
    "Gem5",
    "Gem6",
    "Gem7",
    "Gem8",
    "MiasmaSurface",
    "FloorMiasma",
];

/// Enemy corpses (`corpseSpriteData.xml`), registered as `corpse:<id>`.
const CORPSES: &[&str] = &[
    "Dummy",
    "PurpleCultist",
    "GreenCultist",
    "RedCultist",
    "Birdman",
    "DarkBirdman",
    "Skeleton",
    "Mimic",
];

/// Boss-atlas sprites (`bossSpriteData.xml`).
const BOSS_SPRITES: &[&str] = &[
    "KingReaper",
    "KingBullet",
    "KingBeam",
    "KingShield",
    "KingCrystal",
    "KingBoltCrystal",
];

/// Plain main-atlas images (debris, icons).
pub const TEXTURES: &[&str] = &[
    "enemies/slime/eye",
    "enemies/ghost/halo",
    "enemies/bat/eyeBatWing",
    "enemies/bat/bomBatWing",
    "enemies/cultist/knife",
    "enemies/cultist/scytheHandle",
    "enemies/cultist/scytheHead",
    "enemies/skeleton/looseHead",
    "enemies/bombSkeleton/looseHead",
    "enemies/technomage/Staff",
    "enemies/technomage/missile",
    "enemies/crystal/crystal",
    "enemies/crystal/greenCrystal",
    "enemies/crystal/prismCrystal",
    "dummy/head",
    "versus/playerIndicator",
    "versus/teamRevive",
    "player/arrowHUD/arrow",
    "player/arrowHUD/bombArrow",
    "player/arrowHUD/superBombArrow",
    "player/arrowHUD/laserArrow",
    "player/arrowHUD/brambleArrow",
    "player/arrowHUD/drillArrow",
    "player/arrowHUD/boltArrow",
    "player/arrowHUD/squeakyArrow",
    "player/arrowHUD/featherArrow",
    "player/arrowHUD/triggerArrow",
    "player/arrowHUD/prismArrow",
    "miasma/tentacles",
];

/// Arrow-HUD icon of an arrow type (index = `ArrowType` order).
const ARROW_HUD: [&str; 11] = [
    "player/arrowHUD/arrow",
    "player/arrowHUD/bombArrow",
    "player/arrowHUD/superBombArrow",
    "player/arrowHUD/laserArrow",
    "player/arrowHUD/brambleArrow",
    "player/arrowHUD/drillArrow",
    "player/arrowHUD/boltArrow",
    "player/arrowHUD/squeakyArrow",
    "player/arrowHUD/featherArrow",
    "player/arrowHUD/triggerArrow",
    "player/arrowHUD/prismArrow",
];

/// Ids of the registered art.
#[derive(Clone, Debug, Default)]
pub struct Art {
    sprites: BTreeMap<String, SpriteId>,
    tex: BTreeMap<&'static str, TexId>,
}

impl Art {
    /// Registers the enemy art into `c` (call before building worlds).
    pub fn register(c: &mut Content, inst: &tfp_assets::GameInstall) -> Art {
        let mut art = Art::default();
        let sd = inst.content.sprite_data_dir();
        if let Ok(main) = SpriteData::load(&sd.join("spriteData.xml")) {
            for &n in MAIN_SPRITES {
                if let Some(d) = main.get(n) {
                    art.sprites
                        .insert(n.to_owned(), c.add_sprite(AtlasKind::Main, d));
                }
            }
        }
        if let Ok(corpses) = SpriteData::load(&sd.join("corpseSpriteData.xml")) {
            for &n in CORPSES {
                if let Some(d) = corpses.get(n) {
                    let mut d = d.clone();
                    d.id = format!("corpse:{n}");
                    let id = c.add_sprite(AtlasKind::Main, &d);
                    art.sprites.insert(d.id, id);
                }
            }
        }
        // The boss sprite data lives with the boss atlas (Dark World copy first).
        let boss_dirs = inst
            .dark_world
            .iter()
            .map(|d| d.sprite_data_dir())
            .chain(std::iter::once(sd.clone()));
        for dir in boss_dirs {
            if let Ok(boss) = SpriteData::load(&dir.join("bossSpriteData.xml")) {
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
        }
        for &n in TEXTURES {
            art.tex.insert(n, c.tex(AtlasKind::Main, n));
        }
        // Boss-atlas images (the King Reaper's falling head).
        art.tex.insert(
            "kingReaper/kingHead",
            c.tex(AtlasKind::Boss, "kingReaper/kingHead"),
        );
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

/// Frame (grid cell) of `anim` after `t` frames (`SpriteData` delays are
/// seconds per frame; one-shots hold their last frame).
pub fn frame_at(s: &SpriteDef, anim: &str, t: f32) -> i16 {
    let Some(a) = s.anims.iter().find(|a| a.id == anim) else {
        return 0;
    };
    if a.frames.is_empty() {
        return 0;
    }
    let per = a.delay * 60.0;
    let i = if per <= 0.0 { 0 } else { (t / per) as usize };
    let i = if a.looped {
        i % a.frames.len()
    } else {
        i.min(a.frames.len() - 1)
    };
    a.frames[i] as i16
}

/// Per-entity animation clocks.
#[derive(Default)]
pub struct Anims {
    clocks: BTreeMap<(EntityId, u8), (u64, u32)>,
    /// Mimics that copied a player: their archer.
    pub mimic_looks: BTreeMap<EntityId, u16>,
}

impl Anims {
    /// Frames since `key` was first seen for (`id`, `part`).
    fn t(&mut self, id: EntityId, part: u8, key: u64, frame: u32) -> f32 {
        let e = self.clocks.entry((id, part)).or_insert((key, frame));
        if e.0 != key {
            *e = (key, frame);
        }
        frame.saturating_sub(e.1) as f32
    }

    /// Forgets entities that left the scene.
    pub fn retain(&mut self, w: &World) {
        self.clocks.retain(|(id, _), _| w.exists(*id));
        self.mimic_looks.retain(|id, _| w.exists(*id));
    }

    pub fn clear(&mut self) {
        self.clocks.clear();
        self.mimic_looks.clear();
    }
}

fn key(s: SpriteId, anim: &str, extra: u32) -> u64 {
    let mut h: u64 = s.0 as u64 | ((extra as u64) << 16);
    for b in anim.bytes() {
        h = h.wrapping_mul(131).wrapping_add(b as u64);
    }
    h
}

/// Builds sprite drawables for one entity.
struct B<'a> {
    c: &'a Content,
    art: &'a Art,
    out: Vec<Drawable>,
    id: EntityId,
    depth: i32,
}

impl B<'_> {
    fn sprite(&mut self, s: SpriteId, frame: i16, pos: Vec2) -> Option<&mut Drawable> {
        let d = self.c.sprite(s)?;
        self.out.push(Drawable {
            visual: Visual::Sprite {
                tex: d.texture,
                frame,
                frame_w: d.frame_w,
                frame_h: d.frame_h,
            },
            pos: pos + d.position,
            origin: d.origin,
            scale: Vec2::new(1.0, 1.0),
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            tint: [255; 4],
            depth: self.depth,
            entity: self.id,
            outline: 0,
            clip_h: 1.0,
        });
        self.out.last_mut()
    }

    fn anim(&mut self, s: SpriteId, anim: &str, t: f32, pos: Vec2) -> Option<&mut Drawable> {
        let f = self.c.sprite(s).map(|d| frame_at(d, anim, t))?;
        self.sprite(s, f, pos)
    }

    fn tex(&mut self, name: &str, pos: Vec2, rotation: f32) -> Option<&mut Drawable> {
        let t = self.art.tex(name)?;
        let size = self.c.texture(t).size?;
        self.out.push(Drawable {
            visual: Visual::Sprite {
                tex: t,
                frame: -1,
                frame_w: 0,
                frame_h: 0,
            },
            pos,
            origin: Vec2::new(size.0 as f32 / 2.0, size.1 as f32 / 2.0),
            scale: Vec2::new(1.0, 1.0),
            rotation,
            flip_x: false,
            flip_y: false,
            tint: [255; 4],
            depth: self.depth,
            entity: self.id,
            outline: 0,
            clip_h: 1.0,
        });
        self.out.last_mut()
    }
}

fn alpha(a: f32) -> [u8; 4] {
    [255, 255, 255, (a.clamp(0.0, 1.0) * 255.0) as u8]
}

/// What a frame needs besides the world.
pub struct Ctx<'a> {
    pub c: &'a Content,
    pub art: &'a Art,
    /// Archer of each slot (player ghosts, mimics).
    pub slot_archers: &'a [Option<u16>],
    pub tick: u64,
}

/// Replaces the sim's placeholder items for enemies, portals, hazards,
/// dummies and revivers with sprites (wrapped copies included). Returns
/// extra lights `(pos, radius)` for the lighting layer.
pub fn apply(view: &mut View, w: &World, cx: &Ctx, anims: &mut Anims) -> Vec<(Vec2, f32)> {
    let mut replaced = BTreeSet::new();
    let mut add = Vec::new();
    let mut lights = Vec::new();
    let ws = w.world_size(cx.c);
    for id in w.entities() {
        let core = w.core(id);
        let mut b = B {
            c: cx.c,
            art: cx.art,
            out: Vec::new(),
            id,
            depth: core.depth,
        };
        match w.kind(id) {
            Some(Kind::Enemy(e)) => {
                replaced.insert(id);
                if core.visible {
                    let from = b.out.len();
                    enemy(&mut b, w, cx, anims, id, e);
                    let a = enemy_outline(e);
                    if a > 0 {
                        for d in &mut b.out[from..] {
                            d.outline = a;
                        }
                    }
                    if e.alive() {
                        lights.push((core.pos, light_radius(e.ty)));
                    }
                }
            }
            Some(Kind::Portal(p)) => {
                replaced.insert(id);
                if p.scale > 0.0 {
                    let s = cx.art.sprite("SpawnPortal");
                    let (anim, t) = match p.spawn_t {
                        Some(t) => ("1", t),
                        None => ("0", w.frame as f32),
                    };
                    if let Some(d) = b.anim(s, anim, t, core.pos) {
                        d.scale = Vec2::new(p.scale, p.scale);
                    }
                    lights.push((core.pos, 30.0 * p.scale.min(1.2)));
                }
            }
            Some(Kind::Hazard(h)) => {
                replaced.insert(id);
                hazard(&mut b, w, h, core.pos);
            }
            Some(Kind::Dummy(d)) => {
                replaced.insert(id);
                if !d.dead {
                    // trials.md 8 / `Dummy`: frame 1 hanging from a chain,
                    // rotated with its last link; the head image on top;
                    // both outlined; one pixel right when facing left.
                    let rot = d.chain.and_then(|ch| match w.kind(ch) {
                        Some(Kind::Tower(tfp_sim::entities::Tower::Chain(c))) if c.n > 0 => {
                            Some(c.theta[c.n as usize - 1])
                        }
                        _ => None,
                    });
                    let pos = core.pos + Vec2::new(if d.facing < 0 { 1.0 } else { 0.0 }, 0.0);
                    let from = b.out.len();
                    let s = cx.art.sprite("Dummy");
                    if let Some(it) = b.sprite(s, i16::from(d.chain.is_some()), pos) {
                        it.scale.x = d.facing as f32;
                        it.rotation = rot.unwrap_or(0.0);
                    }
                    if let Some(it) = b.tex("dummy/head", pos + Vec2::new(0.0, 5.0), 0.0) {
                        it.origin = Vec2::new(3.0, 4.0);
                        it.scale.x = d.facing as f32;
                    }
                    let src: Vec<Drawable> = b.out[from..].to_vec();
                    let mut outlined = Vec::with_capacity(src.len() * 9);
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
                        for it in &src {
                            let mut o = *it;
                            o.pos += Vec2::new(dx, dy);
                            o.tint = [0, 0, 0, 255];
                            outlined.push(o);
                        }
                    }
                    outlined.extend(src);
                    b.out.truncate(from);
                    b.out.extend(outlined);
                    lights.push((core.pos + Vec2::new(0.0, 10.0), 50.0));
                }
            }
            Some(Kind::Reviver(r)) => {
                reviver(&mut b, w, cx, r);
            }
            _ => continue,
        }
        if b.out.is_empty() {
            continue;
        }
        let n = b.out.len();
        if core.screen_wrap {
            let dx = if core.pos.x > ws.w / 2.0 { -ws.w } else { ws.w };
            let dy = if core.pos.y > ws.h / 2.0 { -ws.h } else { ws.h };
            for off in [Vec2::new(dx, 0.0), Vec2::new(0.0, dy), Vec2::new(dx, dy)] {
                for k in 0..n {
                    let mut d = b.out[k];
                    d.pos += off;
                    b.out.push(d);
                }
            }
        }
        add.extend(b.out);
    }
    view.items.retain(|d| !replaced.contains(&d.entity));
    view.items.extend(add);
    // Cooldown stars (enemies.md 1.10, Show Dodge Cooldown).
    let mut b = B {
        c: cx.c,
        art: cx.art,
        out: Vec::new(),
        id: EntityId {
            index: u32::MAX,
            generation: 1,
        },
        depth: -1000,
    };
    let stars = cx.art.sprite("StunStars");
    for &p in &view.cooldowns {
        b.anim(stars, "0", w.frame as f32, p);
    }
    view.items.extend(b.out);
    super::miasma::apply(view, w, cx.c, cx.art);
    // King's Court: the intro King became the King Reaper (quest.md 4.4).
    // The sim draws its transform frames; once the Reaper is out (transform
    // frame 16) the King and its light go away.
    if let Some((k, Some(t))) = king_transform(w)
        && t >= tfp_sim::entities::scenery::KING_TRANSFORM_REAPER
    {
        let at = w.core(k).pos;
        view.items.retain(|d| d.entity != k);
        view.lights.retain(|(p, _)| *p != at);
    }
    anims.retain(w);
    lights
}

/// King's Court: the intro King's entity and, once its transformation
/// has been triggered (`Quest::king_transformed`), the ticks into its
/// transform animation (`None` before the animation starts).
pub fn king_transform(w: &World) -> Option<(EntityId, Option<f32>)> {
    use tfp_sim::entities::Tower;
    if !w.quest.as_deref().is_some_and(|q| q.king_transformed()) {
        return None;
    }
    let k = w.tower.king.filter(|&k| w.exists(k))?;
    let t = match w.kind(k) {
        Some(Kind::Tower(Tower::KingIntro(e))) if e.anim == 3 => Some(e.t),
        _ => None,
    };
    Some((k, t))
}

/// The same art for other scenes (Versus: player ghosts, Team Revive,
/// dark-portal enemies): registered once, applied to each frame's view.
#[derive(Default)]
pub struct SceneArt {
    art: Art,
    anims: Anims,
}

impl SceneArt {
    /// From art registered elsewhere (online: when the shared content was
    /// built).
    pub fn from_art(art: Art) -> SceneArt {
        SceneArt {
            art,
            anims: Anims::default(),
        }
    }

    pub fn register(c: &mut Content, inst: &tfp_assets::GameInstall) -> SceneArt {
        SceneArt {
            art: Art::register(c, inst),
            anims: Anims::default(),
        }
    }

    /// Swaps the art into `view`; slot archers come from the match.
    pub fn apply(&mut self, view: &mut View, w: &World, c: &Content) -> Vec<(Vec2, f32)> {
        let slots: Vec<Option<u16>> = (0..tfp_sim::world::MAX_PLAYERS)
            .map(|i| {
                w.versus
                    .as_ref()
                    .and_then(|m| m.settings.slots[i].map(|s| s.archer))
            })
            .collect();
        let cx = Ctx {
            c,
            art: &self.art,
            slot_archers: &slots,
            tick: w.frame as u64,
        };
        apply(view, w, &cx, &mut self.anims)
    }
}

fn light_radius(t: EnemyType) -> f32 {
    use EnemyType::*;
    match t {
        Elemental | GreenElemental => 90.0,
        Cultist | ScytheCultist | BossCultist | Skeleton(_) | TechnoMage => 60.0,
        KingReaper => 100.0,
        _ => 40.0,
    }
}

fn facing_scale(e: &Enemy) -> Vec2 {
    Vec2::new(e.scale.x * e.facing as f32, e.scale.y)
}

/// Picks an enemy's look (enemies.md section 4, the sprite data's
/// animation names) from its state and speed.
/// Alpha of the black outline an enemy's sprite gets (`DrawOutline` in
/// the originals' render): slimes, cultists and crystals while alive,
/// exploders and techno mages always, flaming skulls until dead, ghosts
/// and elementals fading with their alpha above 1. Skeletons outline their
/// body parts themselves (not their arrow HUD).
fn enemy_outline(e: &Enemy) -> u8 {
    use EnemyType::*;
    let a = match e.ty {
        Slime | RedSlime | BlueSlime | Cultist | ScytheCultist | BossCultist | EvilCrystal
        | BlueCrystal | BoltCrystal | PrismCrystal
            if e.alive() =>
        {
            1.0
        }
        Exploder | TechnoMage => 1.0,
        FlamingSkull if e.state != enemies::skull::DEAD => 1.0,
        Ghost | GreenGhost | Elemental | GreenElemental if e.alive() && e.alpha > 1.0 => {
            (e.alpha - 1.0) * 5.0
        }
        _ => 0.0,
    };
    (a.clamp(0.0, 1.0) * 255.0) as u8
}

fn enemy(b: &mut B, w: &World, cx: &Ctx, anims: &mut Anims, id: EntityId, e: &Enemy) {
    use EnemyType::*;
    use enemies::*;
    let pos = w.core(id).pos;
    let frame = w.frame;
    let a = e.alpha.clamp(0.0, 1.0);
    let simple = |b: &mut B,
                  anims: &mut Anims,
                  name: &str,
                  anim: &str,
                  extra: u32,
                  scale: Vec2,
                  rot: f32| {
        let s = cx.art.sprite(name);
        let t = anims.t(id, 0, key(s, anim, extra), frame);
        if let Some(d) = b.anim(s, anim, t, pos) {
            d.scale = scale;
            d.rotation = rot;
            d.tint = alpha(a);
        }
    };
    match e.ty {
        Slime | RedSlime | BlueSlime => {
            let name = match e.ty {
                Slime => "Slime",
                RedSlime => "RedSlime",
                _ => "BlueSlime",
            };
            let anim = match e.state {
                slime::LAND => "land",
                slime::DIE => "death",
                slime::AIR if e.speed.y >= 1.8 => "fall",
                _ => "idle",
            };
            simple(b, anims, name, anim, 0, facing_scale(e), 0.0);
        }
        Bat | BombBat | SuperBombBat | Crow => {
            let name = match e.ty {
                Bat => "EyeBat",
                BombBat => "BomBat",
                SuperBombBat => "SuperBomBat",
                _ => "Crow",
            };
            let anim = match e.state {
                bat::SWOOP => "swoop",
                bat::DEAD if e.ty == Crow => {
                    if e.speed.y < 0.0 {
                        "fallingUp"
                    } else if e.speed.y > 0.0 {
                        "fallingDown"
                    } else {
                        "dead"
                    }
                }
                bat::DEAD => "dead",
                _ => "fly",
            };
            simple(b, anims, name, anim, 0, facing_scale(e), e.rotation);
            // Eye bat / crow: the 120-tick fade before removal (enemies.md
            // 4.2; the sim's coroutine waits at step 3).
            let t = anims.t(id, 4, ((e.state as u64) << 16) | e.co.step as u64, frame);
            if e.state == bat::DEAD
                && e.co.step == 3
                && let Some(d) = b.out.last_mut()
            {
                d.tint[3] = (d.tint[3] as f32 * (1.0 - t / 120.0).max(0.0)) as u8;
            }
        }
        Ghost | GreenGhost | Elemental | GreenElemental => {
            let name = match e.ty {
                Ghost => "Ghost",
                GreenGhost => "GreenGhost",
                Elemental => "FireElemental",
                _ => "GreenFireElemental",
            };
            let anim = ghost::anim(e);
            simple(b, anims, name, anim, 0, facing_scale(e), 0.0);
        }
        Cultist | ScytheCultist | BossCultist => {
            let name = match e.ty {
                Cultist => "Cultist",
                ScytheCultist => "ScytheCultist",
                _ => "RedCultist",
            };
            let anim = cultist::anim(e);
            simple(b, anims, name, anim, 0, facing_scale(e), 0.0);
        }
        Birdman | DarkBirdman => {
            let name = if e.ty == Birdman {
                "Birdman"
            } else {
                "BirdmanAlt"
            };
            let anim = match e.state {
                birdman::ATTACK => "attack",
                _ if e.speed.y < -0.3 => "flap",
                _ => "idle",
            };
            simple(b, anims, name, anim, 0, facing_scale(e), 0.0);
        }
        Skeleton(k) => skeleton(b, w, cx, anims, id, e, k),
        Mole => {
            // JumpDown: after wait(10) a 12-tick CubeInOut turn to pi, and
            // the dig animation after a further wait(6).
            let t = anims.t(id, 4, e.state as u64, frame);
            let anim = match e.state {
                mole::JUMP_DOWN if t >= 18.0 => "dig",
                mole::JUMP_DOWN => "jump",
                mole::DRILL => "dig",
                mole::ROLL => "roll",
                mole::DEAD => {
                    // The explosion plays once (5 frames of 0.08 s) and the
                    // mole is gone with it (enemies.md 4.7).
                    let s = cx.art.sprite("MoleExplosion");
                    let t = anims.t(id, 0, key(s, "0", 0), frame);
                    if t < 24.0 {
                        b.anim(s, "0", t, pos);
                    }
                    return;
                }
                _ if !mole::on_ground(e) => "jump",
                _ => "idle",
            };
            let rot = if e.state == mole::JUMP_DOWN && t >= 11.0 {
                let x = ((t - 11.0) / 12.0).clamp(0.0, 1.0);
                let k = if x < 0.5 {
                    4.0 * x * x * x
                } else {
                    let u = 2.0 * x - 2.0;
                    0.5 * u * u * u + 1.0
                };
                PI * k
            } else {
                e.rotation
            };
            simple(b, anims, "Mole", anim, 0, facing_scale(e), rot);
        }
        Worm => {
            let anim = match e.state {
                worm::FALL => "fall",
                worm::DEAD => "death",
                _ => "walk",
            };
            let sc = Vec2::new(-(e.facing as f32) * e.scale.x, e.scale.y);
            simple(b, anims, "Worm", anim, 0, sc, e.rotation);
        }
        FlamingSkull => {
            let anim = if e.state == skull::DEAD {
                "dead"
            } else {
                "idle"
            };
            simple(
                b,
                anims,
                "flamingSkull",
                anim,
                0,
                facing_scale(e),
                e.rotation,
            );
        }
        Exploder => {
            let anim = if matches!(e.state, exploder::DEAD | exploder::ARMED) {
                "1"
            } else {
                "0"
            };
            let t_scale = if e.state == exploder::CHASE { 3 } else { 1 };
            let s = cx.art.sprite("Exploder");
            let t = anims.t(id, 0, key(s, anim, 0), frame) * t_scale as f32;
            // Armed / dead: the sprite swells 1 -> 1.4 over 30 ticks (CubeIn).
            let swell = if anim == "1" {
                let x = (t / 30.0).min(1.0);
                1.0 + 0.4 * x * x * x
            } else {
                1.0
            };
            if let Some(d) = b.anim(s, anim, t, pos) {
                d.scale = facing_scale(e) * swell;
                d.rotation = e.rotation;
            }
        }
        EvilCrystal | BlueCrystal | BoltCrystal | PrismCrystal => {
            let name = match e.ty {
                EvilCrystal => "Crystal",
                BlueCrystal => "BlueCrystal",
                BoltCrystal => "GreenCrystal",
                _ => "PrismCrystal",
            };
            let anim = match e.state {
                crystal::SHOOT => "shoot",
                crystal::DEAD => "die",
                _ => "idle",
            };
            simple(b, anims, name, anim, 0, e.scale, e.rotation);
        }
        TechnoMage => {
            let anim = match e.state {
                techno::DEAD => "death",
                techno::ALERT => {
                    if e.co.step % 2 == 1 {
                        "attack"
                    } else {
                        "alert"
                    }
                }
                _ => "idle",
            };
            simple(b, anims, "Technomage", anim, 0, facing_scale(e), e.rotation);
        }
        KingReaper => {
            let intro = e.has(reaper::F_INTRO);
            let anim = match e.state {
                reaper::APPEAR if intro => "transform",
                reaper::APPEAR => "appear",
                reaper::HURT => "hurt",
                reaper::DISAPPEAR => "disappear",
                reaper::DEAD => "death",
                _ => {
                    let beaming = w.entities().any(|h| {
                        matches!(w.kind(h), Some(Kind::Hazard(hz))
                            if hz.owner == Some(id) && hz.ty == HazardType::ReaperBeam)
                    });
                    if beaming { "laser" } else { "idle" }
                }
            };
            simple(b, anims, "boss:KingReaper", anim, 0, e.scale, e.rotation);
            let grow = reaper::shield_height(e);
            if grow > 0.0 {
                let s = cx.art.sprite("boss:KingShield");
                if let Some(d) = b.anim(s, "0", w.frame as f32, pos) {
                    d.scale = Vec2::new(grow, grow);
                    d.tint = alpha(0.85);
                }
            }
        }
        PlayerGhost => {
            let anim = match e.state {
                player_ghost::SPAWN => "spawn",
                player_ghost::DODGE => "dodge",
                player_ghost::DEAD => "death",
                player_ghost::DESPAWN => "despawn",
                _ => "normal",
            };
            let archer = cx
                .slot_archers
                .get(e.owner.max(0) as usize)
                .copied()
                .flatten()
                .unwrap_or(0);
            let col =
                cx.c.archers
                    .get(archer as usize)
                    .map_or([255; 4], |a| a.color_b);
            let s = cx.art.sprite("PlayerGhost");
            let t = anims.t(id, 0, key(s, anim, 0), frame);
            if let Some(d) = b.anim(s, anim, t, pos) {
                d.scale = facing_scale(e);
                d.tint = [col[0], col[1], col[2], (a * 230.0) as u8];
            }
        }
    }
}

/// The humanoid skeleton: wings, body, head, bow (aimed while shooting),
/// shield bubble (enemies.md 4.6; the archer layer order of player.md 19.1).
fn skeleton(
    b: &mut B,
    w: &World,
    cx: &Ctx,
    anims: &mut Anims,
    id: EntityId,
    e: &Enemy,
    k: enemies::SkelKind,
) {
    use enemies::skeleton as sk;
    let pos = w.core(id).pos;
    let f = e.facing as f32;
    let from = b.out.len();
    // Mimics copy the look of the player they mimicked; jesters are Cyan.
    if sk::mimicked(e)
        && let Some(t) = e.target
        && let Some(p) = w.player(t)
    {
        anims.mimic_looks.entry(id).or_insert(p.archer);
    }
    let archer = if k.jester {
        Some(6u16)
    } else {
        anims.mimic_looks.get(&id).copied()
    };
    // Copying squashes the body to (1.5, 0.5), relaxing back at 0.05/tick.
    let mt = anims.t(id, 5, sk::mimicked(e) as u64, w.frame);
    let squash = if k.mimic && sk::mimicked(e) {
        Vec2::new((1.5 - 0.05 * mt).max(1.0), (0.5 + 0.05 * mt).min(1.0))
    } else {
        Vec2::new(1.0, 1.0)
    };
    let (body, head, bow) = match archer.and_then(|a| cx.c.archers.get(a as usize)) {
        Some(a) => (a.body, a.head_normal, a.bow),
        None if k.mimic => (
            cx.art.sprite("ShadowBody"),
            cx.art.sprite("ShadowHead"),
            SpriteId::NONE,
        ),
        None if k.arrow == tfp_sim::quiver::ArrowType::Bomb => (
            cx.art.sprite("BombSkeletonBody"),
            cx.art.sprite("BombSkeletonHead"),
            cx.art.sprite("SkeletonBow"),
        ),
        None => (
            cx.art.sprite("SkeletonBody"),
            cx.art.sprite("SkeletonHead"),
            cx.art.sprite("SkeletonBow"),
        ),
    };
    let grounded = e.speed.y == 0.0;
    let body_anim = if e.state == sk::DODGE {
        "dodge"
    } else if grounded {
        if e.speed.x.abs() > 0.1 {
            "run"
        } else {
            "stand"
        }
    } else if e.speed.y < -0.5 {
        "jump"
    } else if e.speed.y > 0.5 {
        "fall"
    } else {
        "glide"
    };
    let frame = w.frame;
    let tint = if w.core(id).flash.shown {
        [255; 4]
    } else {
        [255, 255, 255, 90]
    };
    // Wings behind the body.
    if k.wings {
        let ws = cx.art.sprite("Wings");
        let wa = if e.speed.y < -0.5 { "flap" } else { "glide" };
        let t = anims.t(id, 3, key(ws, wa, 0), frame);
        if let Some(d) = b.anim(ws, wa, t, pos) {
            d.scale = Vec2::new(f, 1.0);
            d.tint = tint;
        }
    }
    let Some(bd) = cx.c.sprite(body) else { return };
    let bt = anims.t(id, 0, key(body, body_anim, 0), frame);
    let bframe = frame_at(bd, body_anim, bt);
    let bpos = pos + bd.position;
    let bi = bframe.max(0) as usize;
    b.out.push(Drawable {
        visual: Visual::Sprite {
            tex: bd.texture,
            frame: bframe,
            frame_w: bd.frame_w,
            frame_h: bd.frame_h,
        },
        pos: bpos,
        origin: bd.origin,
        scale: Vec2::new(f * squash.x, squash.y),
        rotation: 0.0,
        flip_x: false,
        flip_y: false,
        tint,
        depth: b.depth,
        entity: id,
        outline: 0,
        clip_h: 1.0,
    });
    // Head.
    if e.state != sk::DODGE
        && let Some(hd) = cx.c.sprite(head)
    {
        let aim = sk::aim_view(e);
        let ha = match aim {
            Some(a) if a.sin() < -0.5 => "lookUp",
            Some(a) if a.sin() > 0.5 => "lookDown",
            _ => "idle",
        };
        let hf = frame_at(hd, ha, frame as f32);
        let ox = bd.head_x_origins.get(bi).map_or(hd.origin.x, |&v| v as f32);
        let oy = bd.head_y_origins.get(bi).map_or(hd.origin.y, |&v| v as f32);
        b.out.push(Drawable {
            visual: Visual::Sprite {
                tex: hd.texture,
                frame: hf,
                frame_w: hd.frame_w,
                frame_h: hd.frame_h,
            },
            pos: pos + hd.position,
            origin: Vec2::new(ox, oy),
            scale: Vec2::new(f * squash.x, squash.y),
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
            tint,
            depth: b.depth,
            entity: id,
            outline: 0,
            clip_h: 1.0,
        });
    }
    // Bow: aimed while shooting, else at the facing direction.
    if e.state != sk::DODGE
        && let Some(bw) = cx.c.sprite(bow)
    {
        let dir = sk::aim_view(e).unwrap_or(if f < 0.0 { PI } else { 0.0 });
        let drawn = sk::aim_view(e).is_some();
        let ba = if drawn {
            if e.arrows.is_empty() {
                "drawnEmpty"
            } else {
                "drawn"
            }
        } else {
            "idle"
        };
        let bf = frame_at(bw, ba, frame as f32);
        let mirrored = dir.cos() < 0.0;
        let bx = bw.position.x + bd.bow_x_offsets.get(bi).copied().unwrap_or(0) as f32 * f;
        let by = bw.position.y + bd.bow_y_offsets.get(bi).copied().unwrap_or(0) as f32;
        b.out.push(Drawable {
            visual: Visual::Sprite {
                tex: bw.texture,
                frame: bf,
                frame_w: bw.frame_w,
                frame_h: bw.frame_h,
            },
            pos: pos + Vec2::new(bx, by),
            origin: bw.origin,
            scale: Vec2::new(if mirrored { -1.0 } else { 1.0 }, 1.0),
            rotation: if mirrored { dir + PI } else { dir },
            flip_x: false,
            flip_y: false,
            tint,
            depth: b.depth,
            entity: id,
            outline: 0,
            clip_h: 1.0,
        });
    }
    // Wings, body, head and bow are outlined (`HumanoidEnemyRenderer`).
    for d in &mut b.out[from..] {
        d.outline = 255;
    }
    // Arrow HUD (enemies.md 1.9): held arrows centred at (0,-22), 4 px apart
    // (the first one is the next to be shot; specials go first).
    let n = e.arrows.len();
    if n > 0 {
        for (i, t) in e.arrows.iter().enumerate() {
            let t = t as usize;
            let x = -2.0 * (n as f32 - 1.0) + 4.0 * i as f32;
            let old = b.depth;
            b.depth = -1000;
            b.tex(ARROW_HUD[t.min(10)], pos + Vec2::new(x, -22.0), 0.0);
            b.depth = old;
        }
    }
    // Shield bubble.
    if e.has(sk::F_SHIELD) {
        let s = cx.art.sprite("Shield");
        if let Some(d) = b.anim(s, "0", frame as f32, pos) {
            d.scale = Vec2::new(0.8, 0.6);
            d.tint = [200, 200, 255, 200];
        }
    }
}

fn hazard(b: &mut B, w: &World, h: &tfp_sim::enemies::hazard::Hazard, pos: Vec2) {
    let frame = w.frame as f32;
    match h.ty {
        HazardType::Tornado => {
            let s = b.art.sprite("Whirlwind");
            if let Some(d) = b.anim(s, "0", frame, pos) {
                d.tint = alpha(h.alpha);
            }
        }
        HazardType::TechnoMissile => {
            b.tex("enemies/technomage/missile", pos, h.dir.angle());
        }
        HazardType::ReaperBomb => {
            let s = b.art.sprite("boss:KingBullet");
            // The 20-tick swell before it explodes: scale 1 -> 2 (CubeOut).
            let swell = if h.t[3] > 0.0 {
                let k = ((20.0 - h.t[1]) / 20.0).clamp(0.0, 1.0);
                1.0 + (1.0 - (1.0 - k).powi(3))
            } else {
                1.0
            };
            if let Some(d) = b.anim(s, "0", frame, pos) {
                d.scale = Vec2::new(swell, swell);
            }
        }
        HazardType::ReaperCrystal => {
            let name = if h.bolt {
                "boss:KingBoltCrystal"
            } else {
                "boss:KingCrystal"
            };
            let s = b.art.sprite(name);
            match enemies::reaper::crystal_breaking(h) {
                Some(t) => b.anim(s, "1", t, pos),
                None => b.anim(s, "0", frame, pos),
            };
        }
        HazardType::ReaperBeam => {
            let s = b.art.sprite("boss:KingBeam");
            let t = h.t[1];
            let (a, wy) = if t < 60.0 {
                (0.3 + 0.4 * t / 60.0, 0.2 + 0.5 * t / 60.0)
            } else {
                (1.0 - (t - 60.0) / 30.0, 1.0)
            };
            // A quick fade (the King was hurt: `t[2] > 0`) uses the
            // hazard's alpha on top of the timeline's.
            let a = if h.t[2] > 0.0 { a * h.alpha } else { a };
            for rot in [h.angle, h.angle + PI] {
                if let Some(d) = b.anim(s, "0", frame, pos) {
                    d.rotation = rot;
                    d.scale = Vec2::new(2.0, wy);
                    d.tint = alpha(a);
                }
            }
        }
    }
}

/// Team Revive arrows over a revivable corpse (versus.md 7.3 / 12 item 5),
/// flashing the archer's colours; a glow while a corpse is being revived.
fn reviver(b: &mut B, w: &World, cx: &Ctx, r: &tfp_sim::revive::TeamReviver) {
    if r.revived || r.seq.is_some() || !w.exists(r.corpse) {
        return;
    }
    let corpse = w.core(r.corpse).pos;
    b.depth = -1000;
    let archer = cx
        .slot_archers
        .get(r.player_index as usize)
        .copied()
        .flatten()
        .unwrap_or(0);
    let (ca, cb) =
        cx.c.archers
            .get(archer as usize)
            .map_or(([255; 4], [255; 4]), |a| (a.color_a, a.color_b));
    let col = if (cx.tick / 3).is_multiple_of(2) {
        ca
    } else {
        cb
    };
    if r.reviving {
        // Ours: a pulsing ring of the revive progress.
        let k = 1.0 - (r.counter / 60.0).clamp(0.0, 1.0);
        if let Some(d) = b.tex("versus/teamRevive", corpse + Vec2::new(0.0, -14.0), 0.0) {
            let s = 1.0 + 0.5 * k;
            d.scale = Vec2::new(s, s);
            d.tint = [col[0], col[1], col[2], 255];
        }
        return;
    }
    if r.mode == ReviveMode::Quest || r.age < 60.0 {
        return;
    }
    let bob = 2.0 * (cx.tick as f32 * PI * 2.0 / 40.0).sin();
    if let Some(d) = b.tex(
        "versus/playerIndicator",
        corpse + Vec2::new(0.0, -18.0 + bob),
        0.0,
    ) {
        d.tint = [col[0], col[1], col[2], 255];
    }
    if let Some(d) = b.tex(
        "versus/teamRevive",
        corpse + Vec2::new(0.0, -28.0 + bob),
        0.0,
    ) {
        d.tint = [col[0], col[1], col[2], 255];
    }
}
