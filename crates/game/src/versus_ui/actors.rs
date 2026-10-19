//! Cosmetic layers of archers, corpses and arrows that the sim does not
//! draw by itself (player.md 3.5, 14, 16.1-16.2, 18.2, 19.x; arrows.md 14b).
//! Read-only on the world; own state and RNG; never fed back.
//!
//! * `update` (once per tick after the sim step): hair physics, wing
//!   pose / gain tweens, shield gain tween, cold breath puffs, the purple
//!   ambience, the Yellow archer's sleep (ZZZ + snore loop requests), arrow
//!   eye animations and burning / moonglass particles.
//! * `apply` (per frame on the built `View`): team head / corpse / aimer /
//!   hat textures, the wing and shield frames and scales, the corpse revive
//!   flash and levitation, arrow strip frames and wiggle, the trigger eye
//!   layers; hair links and breath puffs inserted as drawables so the
//!   depth sort places them.

use std::collections::BTreeMap;
use std::f32::consts::PI;

use tfp_render::glam::Vec2 as G;
use tfp_sim::arrow::ArrowState;
use tfp_sim::content::{SpriteId, TexId};
use tfp_sim::entity::{Allegiance, Kind};
use tfp_sim::player::{HatState, PState};
use tfp_sim::quiver::ArrowType;
use tfp_sim::view::owner_color;
use tfp_sim::{Content, Drawable, EntityId, View, Visual, World};

use super::particles::Particles;

fn g(v: tfp_sim::Vec2) -> G {
    G::new(v.x, v.y)
}

fn s(v: G) -> tfp_sim::Vec2 {
    tfp_sim::Vec2::new(v.x, v.y)
}

/// What the game data says about one content archer.
#[derive(Clone, Debug, Default)]
pub struct ArcherInfo {
    pub hair: bool,
    pub purple: bool,
    pub sleep_frame: i32,
    /// Frames between breaths (<= 0: none), offset, ducking offset.
    pub breath: (i32, G, G),
    pub color_b: [u8; 4],
    /// Team hat textures (Blue, Red) of the dropped hat.
    pub team_hat: [Option<TexId>; 2],
    /// Corpse `Flash` texture (revive / explode flash).
    pub corpse_flash: Option<TexId>,
    /// Snore loop cue (`CHAR_x_SLEEP_LOOP`), when the voice set has one.
    pub snore: Option<String>,
}

/// Content ids the layers need (registered in the game's `Content`).
#[derive(Clone, Debug, Default)]
pub struct ArcherArt {
    pub infos: Vec<ArcherInfo>,
    pub hair: Option<TexId>,
    pub hair_end: Option<TexId>,
    pub team_aimer: [Option<TexId>; 2],
    pub trigger: SpriteId,
    pub trigger_buried: SpriteId,
    pub eyeball: SpriteId,
    pub pupil: SpriteId,
    pub prism: SpriteId,
    pub prism_buried: SpriteId,
    pub drill: SpriteId,
    pub cold_puff: SpriteId,
    /// Textures with team variants: tex -> (blue, red).
    pub team_tex: BTreeMap<TexId, (Option<TexId>, Option<TexId>)>,
    /// The game data's archer aimer textures, by content archer.
    pub aimers: Vec<Option<TexId>>,
}

impl ArcherArt {
    /// Builds the table from the game data (registers the textures and
    /// sprites it needs in `c`). `corpse_flash`: corpse sprite id ->
    /// flash texture name (corpse sprite data).
    pub fn build(
        c: &mut Content,
        gd: &tfp_assets::game_data::GameData,
        sprites: &[&tfp_assets::sprite_data::SpriteData],
        corpse_flash: &BTreeMap<String, String>,
    ) -> Self {
        use tfp_assets::atlas::AtlasKind::Main;
        let mut a = ArcherArt::default();
        let defs: Vec<&tfp_assets::game_data::ArcherDef> = gd
            .archers
            .archers
            .iter()
            .chain(gd.archers.alts.iter().flatten())
            .collect();
        for (i, d) in defs.iter().enumerate() {
            let hat = d.hat.as_ref();
            let tex = |c: &mut Content, n: &str| (!n.is_empty()).then(|| c.tex(Main, n));
            let voice = c.archers.get(i).map_or(0, |x| x.voice as usize);
            a.infos.push(ArcherInfo {
                hair: d.hair,
                purple: d.purple_particles,
                sleep_frame: d.sleep_head_frame,
                breath: (
                    d.breathing.interval,
                    G::new(d.breathing.offset.0, d.breathing.offset.1),
                    G::new(d.breathing.ducking_offset.0, d.breathing.ducking_offset.1),
                ),
                color_b: d.color_b,
                team_hat: [
                    hat.and_then(|h| tex(c, &h.blue)),
                    hat.and_then(|h| tex(c, &h.red)),
                ],
                corpse_flash: corpse_flash.get(&d.corpse).map(|n| c.tex(Main, n)),
                snore: (d.sleep_head_frame >= 0)
                    .then(|| format!("{}_SLEEP_LOOP", crate::play::VOICES[voice.min(13)])),
            });
            a.aimers.push(tex(c, &d.aimer));
        }
        a.hair = Some(c.tex(Main, "player/hair"));
        a.hair_end = Some(c.tex(Main, "player/hairEnd"));
        a.team_aimer = [
            Some(c.tex(Main, &gd.archers.blue_team.aimer)),
            Some(c.tex(Main, &gd.archers.red_team.aimer)),
        ];
        let sprite = |c: &mut Content, name: &str| {
            sprites
                .iter()
                .find_map(|sd| sd.get(name))
                .map_or(SpriteId::NONE, |d| c.add_sprite(Main, d))
        };
        a.trigger = sprite(c, "TriggerArrow");
        a.trigger_buried = sprite(c, "TriggerArrowBuried");
        a.eyeball = sprite(c, "TriggerArrowEyeball");
        a.pupil = sprite(c, "TriggerArrowPupil");
        a.prism = sprite(c, "PrismArrow");
        a.prism_buried = sprite(c, "PrismArrowBuried");
        a.drill = sprite(c, "drillArrow");
        a.cold_puff = sprite(c, "ColdPuff");
        let mut id = 0u16;
        while let Some(d) = c.sprite(SpriteId(id)) {
            if d.blue_texture.is_some() || d.red_texture.is_some() {
                a.team_tex
                    .insert(d.texture, (d.blue_texture, d.red_texture));
            }
            id += 1;
        }
        a
    }
}

#[derive(Clone, Debug, Default)]
struct Hair {
    offsets: Vec<G>,
    spacing: f32,
    scale: f32,
    prev_pos: G,
    prev_anchor: G,
    add: G,
    sine_t: f32,
    anchor: G,
    facing: f32,
    visible: bool,
    alpha: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Pose {
    #[default]
    Idle,
    Flap,
    Glide,
    FastFall,
}

#[derive(Clone, Debug, Default)]
struct Archer {
    entity: Option<EntityId>,
    archer: usize,
    prev_speed: G,
    prev_flap_gravity: f32,
    had_shield: bool,
    shield_gain: u32,
    had_wings: bool,
    wings_gain: u32,
    pose: Pose,
    pose_t: u32,
    wing_scale: G,
    hair: Option<Hair>,
    breath_alarm: f32,
    breath_set: bool,
    snore: u32,
    was_on_ground: bool,
}

#[derive(Clone, Debug)]
struct Puff {
    player: u8,
    offset: G,
    vel: G,
    t: f32,
    rate: f32,
    alpha: f32,
    flip: bool,
}

#[derive(Clone, Copy, Debug)]
struct Eye {
    anim: usize,
    rate: f32,
    t: f32,
    linked: bool,
}

/// Snore loop requests for the owner: (player index, cue, on).
pub type SnoreRequest = (u8, String, bool);

pub struct ActorFx {
    pub art: ArcherArt,
    /// The level's theme is cold (breath puffs).
    pub cold: bool,
    archers: Vec<Archer>,
    puffs: Vec<Puff>,
    eyes: BTreeMap<EntityId, Eye>,
    rng: u64,
    frame: u64,
    pub snore_requests: Vec<SnoreRequest>,
}

const MAXP: usize = tfp_sim::world::MAX_PLAYERS;

impl ActorFx {
    pub fn new(art: ArcherArt) -> Self {
        Self {
            art,
            cold: false,
            archers: vec![Archer::default(); MAXP],
            puffs: Vec::new(),
            eyes: BTreeMap::new(),
            rng: 0x41_5243_4845_5253,
            frame: 0,
            snore_requests: Vec::new(),
        }
    }

    fn rand(&mut self) -> f32 {
        let x = crate::front::variants::next_rand(&mut self.rng);
        (x >> 11) as f32 / (1u64 << 53) as f32
    }

    /// A new level: per-level state goes (hair, puffs, eyes).
    pub fn reset(&mut self) {
        for (i, a) in self.archers.iter_mut().enumerate() {
            if a.snore > 0
                && let Some(cue) = self.art.infos.get(a.archer).and_then(|x| x.snore.clone())
            {
                self.snore_requests.push((i as u8, cue, false));
            }
            *a = Archer::default();
        }
        self.puffs.clear();
        self.eyes.clear();
    }

    /// Once per tick after the sim step.
    #[allow(clippy::needless_range_loop)] // per-slot arrays
    pub fn update(&mut self, w: &World, c: &Content, particles: &mut Particles) {
        self.frame += 1;
        let frame = self.frame;
        let mut alive = [None; MAXP];
        for id in w.players() {
            if let Some(p) = w.player(id) {
                alive[p.index as usize % MAXP] = Some(id);
            }
        }
        for slot in 0..MAXP {
            let Some(id) = alive[slot] else {
                // Corpses keep the hair (the hat comes off at death); the
                // rest goes.
                let a = &mut self.archers[slot];
                if a.entity.is_some()
                    && let Some(h) = &mut a.hair
                {
                    h.visible = true;
                    h.alpha = 1.0;
                }
                if a.entity.is_some() && a.snore > 0 {
                    if let Some(cue) = self.art.infos.get(a.archer).and_then(|x| x.snore.clone()) {
                        self.snore_requests.push((slot as u8, cue, false));
                    }
                    a.snore = 0;
                }
                a.entity = None;
                a.had_shield = false;
                a.had_wings = false;
                continue;
            };
            let p = w.player(id).expect("player");
            let pos = g(w.core(id).pos);
            let info = self
                .art
                .infos
                .get(p.archer as usize)
                .cloned()
                .unwrap_or_default();
            let new_entity = self.archers[slot].entity != Some(id);
            if new_entity {
                let keep_hair = self.archers[slot].hair.take();
                self.archers[slot] = Archer {
                    entity: Some(id),
                    archer: p.archer as usize,
                    wing_scale: G::ONE,
                    was_on_ground: p.on_ground,
                    prev_speed: g(p.speed),
                    ..Default::default()
                };
                let _ = keep_hair;
            }
            let state = p.state();
            let ducking = state == PState::Ducking;
            let a = &mut self.archers[slot];
            // Shield and wings gain tweens.
            if p.has_shield && !a.had_shield {
                a.shield_gain = 1;
            }
            a.had_shield = p.has_shield;
            if a.shield_gain > 0 {
                a.shield_gain += 1;
                if a.shield_gain > 30 {
                    a.shield_gain = 0;
                }
            }
            if p.has_wings && !a.had_wings {
                a.wings_gain = 1;
            }
            a.had_wings = p.has_wings;
            if a.wings_gain > 0 {
                a.wings_gain += 1;
                if a.wings_gain > 40 {
                    a.wings_gain = 0;
                }
            }
            // Wing pose (player.md 6.3, 16.2): a flap resets the flap
            // gravity; glide / fast fall from the archer's state.
            let flapped = p.has_wings
                && !p.on_ground
                && p.flap_gravity < a.prev_flap_gravity - 0.05
                && p.speed.y < -1.0;
            let pose = if p.on_ground || ducking {
                Pose::Idle
            } else if flapped || (a.pose == Pose::Flap && a.pose_t < 15) {
                Pose::Flap
            } else if p.gliding {
                Pose::Glide
            } else if p.move_y == 1 && p.speed.y > 0.0 {
                Pose::FastFall
            } else {
                Pose::Idle
            };
            if pose != a.pose || flapped {
                a.pose = pose;
                a.pose_t = 0;
            } else {
                a.pose_t += 1;
            }
            let (target, step) = match pose {
                Pose::Glide => (G::new(1.2, 0.8), 0.05),
                Pose::FastFall => (G::new(0.5, 1.5), 0.05),
                _ => (G::ONE, 0.02),
            };
            let approach = |v: f32, t: f32| {
                if v < t {
                    (v + step).min(t)
                } else {
                    (v - step).max(t)
                }
            };
            a.wing_scale = G::new(
                approach(a.wing_scale.x, target.x),
                approach(a.wing_scale.y, target.y),
            );
            a.prev_flap_gravity = p.flap_gravity;
            // Hair (player.md 3.5).
            if info.hair {
                let big = tfp_sim::pickups::variant(
                    w,
                    tfp_sim::variants::Variant::BigHeads,
                    p.index as i32,
                );
                let scale = if big { 2.0 } else { 1.0 };
                let (links, spacing) = if scale > 1.0 { (4, 5.0) } else { (5, 3.0) };
                let body = c
                    .archers
                    .get(p.archer as usize)
                    .and_then(|x| c.sprite(x.body));
                let hy = body
                    .and_then(|b| b.head_y_origins.get(p.body.frame.max(0) as usize))
                    .copied()
                    .unwrap_or(10) as f32;
                let facing = p.facing as f32;
                let mut anchor = G::new(-facing * 3.0, 12.0 - hy * p.scale.y);
                if big {
                    anchor += G::new(-2.0 * facing, -6.0);
                }
                let h = a.hair.get_or_insert_with(|| Hair {
                    offsets: (0..links).map(|i| G::new(0.0, 3.0 * i as f32)).collect(),
                    spacing,
                    scale,
                    prev_pos: pos,
                    prev_anchor: anchor,
                    ..Default::default()
                });
                let d = pos - h.prev_pos;
                let d = if d.length() > 40.0 { G::ZERO } else { d };
                let mut v = (anchor - h.prev_anchor) * 0.2 + d + h.add;
                if v == G::ZERO {
                    v = G::new(0.0, -0.2);
                }
                h.add = h.add.move_towards(G::ZERO, 0.1);
                let k = v.length_squared() / 9.0;
                h.sine_t += 1.0;
                let sine = (h.sine_t * 2.0 * PI / 30.0).sin();
                let n = h.offsets.len();
                for i in 1..n {
                    let mut wv = -v * k * 0.5;
                    wv = wv.lerp(h.offsets[i] - h.offsets[i - 1], (i as f32 / n as f32) * 0.3);
                    wv.y += 0.06 + 0.04 * sine * d.x.abs();
                    if d.x.abs() <= 0.2 {
                        wv.x += 0.08 * sine * d.y.abs();
                    }
                    let wv = wv.normalize_or(G::new(0.0, 1.0)) * h.spacing;
                    h.offsets[i] = h.offsets[i - 1] + wv;
                }
                h.prev_pos = pos;
                h.prev_anchor = anchor;
                h.anchor = anchor;
                h.facing = facing;
                h.visible = p.hat != HatState::Normal;
                h.alpha = p.invis_opacity.clamp(0.0, 1.0).powi(2);
            }
            // Cold breath (player.md 19.7).
            if self.cold && info.breath.0 > 0 && !p.invisible {
                let interval = info.breath.0 as f32;
                if !a.breath_set {
                    a.breath_set = true;
                    a.breath_alarm = 10.0 * p.index as f32
                        + interval
                        + (crate::front::variants::next_rand(&mut self.rng) % 10) as f32;
                }
                a.breath_alarm -= 1.0;
                let landed = p.on_ground && !a.was_on_ground && a.prev_speed.y >= 1.0;
                let fired = a.breath_alarm <= 0.0;
                if (fired || landed) && !p.aiming && a.breath_alarm < interval - 20.0 {
                    let duck = ducking || p.dodge_sliding;
                    let off = if duck { info.breath.2 } else { info.breath.1 };
                    let r1 = (crate::front::variants::next_rand(&mut self.rng) >> 11) as f32
                        / (1u64 << 53) as f32;
                    let r2 = (crate::front::variants::next_rand(&mut self.rng) >> 11) as f32
                        / (1u64 << 53) as f32;
                    self.puffs.push(Puff {
                        player: slot as u8,
                        offset: G::new(off.x * p.facing as f32, off.y),
                        vel: G::new(0.2 * p.facing as f32, 0.0),
                        t: 0.0,
                        rate: 0.8 + 0.3 * r1,
                        alpha: 0.4 + 0.2 * r2,
                        flip: p.facing < 0,
                    });
                    let again =
                        interval + (crate::front::variants::next_rand(&mut self.rng) % 10) as f32;
                    a.breath_alarm = if duck { again * 2.0 } else { again };
                } else if fired && p.aiming {
                    // Not restarted while aiming: the next hard landing puffs.
                    a.breath_alarm = 0.0;
                }
            }
            // Purple ambience (player.md 19.6).
            if info.purple
                && p.invis_opacity >= 1.0
                && frame.is_multiple_of(if ducking { 5 } else { 3 })
                && let Some(t) = particles.by_name("PurpleAmbience")
            {
                {
                    let (at, range) = if ducking {
                        (pos + G::new(0.0, 4.0), G::new(7.0, 6.0))
                    } else {
                        (pos, G::new(7.0, 11.0))
                    };
                    particles.emit(t, 1, at, range / 2.0, None);
                }
            }
            // The Yellow archer's sleep (player.md 7.x DuckingUpdate).
            if info.sleep_frame >= 0 {
                let asleep = ducking && p.head.frame == info.sleep_frame as i16;
                if asleep {
                    if frame.is_multiple_of(60) {
                        if a.snore == 0
                            && let Some(cue) = info.snore.clone()
                        {
                            self.snore_requests.push((slot as u8, cue, true));
                        }
                        a.snore = (a.snore + 1) % 4;
                        if let Some(t) = particles.by_name("YellowSleeping") {
                            particles.emit(t, 1, pos + G::new(3.0, -3.0), G::ZERO, None);
                        }
                    }
                } else if a.snore > 0 {
                    a.snore = 0;
                    if let Some(cue) = info.snore.clone() {
                        self.snore_requests.push((slot as u8, cue, false));
                    }
                }
            }
            a.was_on_ground = p.on_ground;
            a.prev_speed = g(p.speed);
        }
        // Breath puffs (sprite ColdPuff: 9 frames, 0.08 s).
        for pf in &mut self.puffs {
            pf.t += pf.rate;
            pf.vel.x = if pf.vel.x.abs() <= 0.02 {
                0.0
            } else {
                pf.vel.x - 0.02 * pf.vel.x.signum()
            };
            pf.vel.y -= 0.01;
            pf.offset += pf.vel;
        }
        self.puffs.retain(|pf| pf.t < 9.0 * 4.8);
        // Arrows: trigger eyes, burning tails, moonglass sparkles.
        let moonglass: Vec<(G, G)> = w
            .tagged(tfp_sim::Tag::Moonglass)
            .iter()
            .filter_map(|&m| {
                w.hitbox_of(m)
                    .map(|h| (g(w.core(m).pos) + G::new(h.x, h.y), G::new(h.w, h.h)))
            })
            .collect();
        let mut seen = Vec::new();
        for id in w.entities() {
            let Some(Kind::Arrow(ar)) = w.kind(id) else {
                continue;
            };
            let pos = g(w.core(id).pos);
            if ar.arrow_type == ArrowType::Trigger {
                seen.push(id);
                let linked = ar.extra.detonator.is_some();
                let n_anims = c.sprite(self.art.trigger).map_or(1, |d| d.anims.len());
                let r = self.rand();
                let e = self.eyes.entry(id).or_insert(Eye {
                    anim: 0,
                    rate: 1.0,
                    t: 0.0,
                    linked: false,
                });
                if linked && !e.linked && n_anims > 1 {
                    e.anim = 1 + (r * (n_anims - 1) as f32) as usize % (n_anims - 1);
                    e.rate = 0.8 + 0.2 * r;
                    e.t = 0.0;
                } else if !linked && e.linked {
                    *e = Eye {
                        anim: 0,
                        rate: 1.0,
                        t: 0.0,
                        linked: false,
                    };
                }
                e.linked = linked;
                e.t += e.rate;
                if linked
                    && let Some(d) = c.sprite(self.art.trigger)
                    && let Some(an) = d.anims.get(e.anim)
                {
                    let len = an.frames.len() as f32 * (an.delay * 60.0).max(1.0);
                    if e.t >= len && n_anims > 1 {
                        let r = (crate::front::variants::next_rand(&mut self.rng) >> 11) as f32
                            / (1u64 << 53) as f32;
                        e.anim = 1 + (r * (n_anims - 1) as f32) as usize % (n_anims - 1);
                        e.rate = 0.8 + 0.2 * r;
                        e.t = 0.0;
                    }
                }
            }
            if ar.extra.burning
                && frame.is_multiple_of(2)
                && ar.state != ArrowState::Buried
                && let Some(t) = particles.by_name("Fire")
            {
                let back = G::new(ar.direction.cos(), ar.direction.sin()) * -8.0;
                particles.emit(t, 1, pos + back + G::new(0.0, 0.5), G::ONE, None);
            }
            if (ar.speed.x != 0.0 || ar.speed.y != 0.0)
                && moonglass.iter().any(|(o, sz)| {
                    pos.x >= o.x && pos.y >= o.y && pos.x < o.x + sz.x && pos.y < o.y + sz.y
                })
                && let Some(t) = particles.by_name("ArrowInMoonglass")
            {
                particles.emit(t, 1, pos, G::ONE, None);
            }
        }
        self.eyes.retain(|id, _| seen.contains(id));
    }

    fn team_of(w: &World, id: EntityId, index: i8) -> Allegiance {
        match w.kind(id) {
            Some(Kind::Player(p)) => p.team_color,
            Some(Kind::Corpse(cp)) => cp.team_color,
            Some(Kind::Arrow(a)) => a.team_color,
            Some(Kind::Hat(h)) => w
                .versus
                .as_ref()
                .filter(|m| m.settings.team_mode())
                .and_then(|m| m.settings.slots.get(h.owner as usize).copied().flatten())
                .map_or(Allegiance::Neutral, |s| match s.team {
                    0 => Allegiance::Blue,
                    1 => Allegiance::Red,
                    _ => Allegiance::Neutral,
                }),
            _ => {
                let _ = index;
                Allegiance::Neutral
            }
        }
    }

    fn sprite_frame(
        c: &Content,
        id: SpriteId,
        anim: usize,
        t: f32,
    ) -> Option<(TexId, i16, u16, u16, tfp_sim::Vec2)> {
        let d = c.sprite(id)?;
        let frame = d
            .anims
            .get(anim)
            .and_then(|a| {
                if a.frames.is_empty() {
                    return None;
                }
                let per = (a.delay * 60.0).max(1.0);
                let i = (t / per) as usize;
                let i = if a.looped {
                    i % a.frames.len()
                } else {
                    i.min(a.frames.len() - 1)
                };
                Some(a.frames[i])
            })
            .unwrap_or(0);
        Some((d.texture, frame as i16, d.frame_w, d.frame_h, d.origin))
    }

    /// Per frame, on the view about to be drawn.
    pub fn apply(&self, view: &mut View, w: &World, c: &Content, tick: u64) {
        let t = tick as f32;
        let wings_sprite = c.sprite(c.common.wings).cloned();
        let shield_sprite = c.sprite(c.common.shield).cloned();
        // Revivers: the corpse levitates (with a bob) and flashes.
        let mut reviving: BTreeMap<EntityId, f32> = BTreeMap::new();
        for id in w.entities() {
            if let Some(Kind::Reviver(r)) = w.kind(id)
                && r.reviving
            {
                reviving.insert(r.corpse, r.levitate);
            }
        }
        let mut out: Vec<Drawable> = Vec::with_capacity(view.items.len() + 32);
        let mut hair_done = [false; MAXP];
        let items = std::mem::take(&mut view.items);
        for mut it in items {
            let ent = it.entity;
            let kind = w.kind(ent);
            let tex = match it.visual {
                Visual::Sprite { tex, .. } => Some(tex),
                _ => None,
            };
            match kind {
                Some(Kind::Player(p)) => {
                    let slot = p.index as usize % MAXP;
                    let st = &self.archers[slot];
                    // Hair goes first (behind every archer layer).
                    if !hair_done[slot] {
                        hair_done[slot] = true;
                        self.push_hair(&mut out, slot, ent, w, it.depth);
                    }
                    let team = Self::team_of(w, ent, p.index as i8);
                    if let Some(tx) = tex {
                        self.team_swap(&mut it, tx, team, p.archer as usize);
                    }
                    if let (Some(ws), Some(tx)) = (&wings_sprite, tex)
                        && tx == ws.texture
                    {
                        let anim = match st.pose {
                            Pose::Flap => ("flap", st.pose_t as f32),
                            Pose::Glide => ("glide", st.pose_t as f32),
                            _ => ("idle", 0.0),
                        };
                        let ai = ws.anims.iter().position(|a| a.id == anim.0).unwrap_or(0);
                        if let Some((tex, frame, fw, fh, _)) =
                            Self::sprite_frame(c, c.common.wings, ai, anim.1)
                        {
                            it.visual = Visual::Sprite {
                                tex,
                                frame,
                                frame_w: fw,
                                frame_h: fh,
                            };
                        }
                        let gain = if st.wings_gain > 0 {
                            back_out_big(st.wings_gain as f32 / 40.0)
                        } else {
                            1.0
                        };
                        it.scale = tfp_sim::Vec2::new(
                            it.scale.x * st.wing_scale.x * gain,
                            st.wing_scale.y * gain,
                        );
                        let duck = p.state() == PState::Ducking;
                        let oy = if duck {
                            6.0 - 2.0 * st.wing_scale.y
                        } else {
                            6.0 - 6.0 * st.wing_scale.y
                        };
                        it.pos = w.core(ent).pos + tfp_sim::Vec2::new(0.0, oy);
                    }
                    if let (Some(ss), Some(tx)) = (&shield_sprite, tex)
                        && tx == ss.texture
                    {
                        let n = ss.anims.first().map_or(7, |a| a.frames.len().max(1));
                        let frame = ((t / 6.0) as usize % n) as i16;
                        it.visual = Visual::Sprite {
                            tex: ss.texture,
                            frame,
                            frame_w: ss.frame_w,
                            frame_h: ss.frame_h,
                        };
                        let sn = (t * 2.0 * PI / 120.0).sin();
                        // The gain reveals the bubble from the top (player.md §16.1).
                        if st.shield_gain > 0 {
                            it.clip_h = ease_cube_in_out(st.shield_gain as f32 / 30.0);
                        }
                        it.scale = tfp_sim::Vec2::new(0.7 + 0.1 * sn, 0.7 - 0.1 * sn);
                    }
                    out.push(it);
                }
                Some(Kind::Corpse(cp)) => {
                    let team = Self::team_of(w, ent, cp.player_index);
                    if let Some(tx) = tex {
                        self.team_swap(&mut it, tx, team, cp.archer as usize);
                        if let Some(&lev) = reviving.get(&ent) {
                            if (tick / 4).is_multiple_of(2)
                                && let Some(f) = self
                                    .art
                                    .infos
                                    .get(cp.archer as usize)
                                    .and_then(|x| x.corpse_flash)
                                && let Visual::Sprite { tex, .. } = &mut it.visual
                            {
                                *tex = f;
                            }
                            // (The sim raises the corpse itself.)
                            let _ = lev;
                        }
                    }
                    // The hair stays with the corpse (anchored at its head).
                    let slot = (cp.player_index.max(0) as usize) % MAXP;
                    if !hair_done[slot] && self.archers[slot].entity.is_none() {
                        hair_done[slot] = true;
                        let anchor = G::new(-3.0 * cp.facing as f32, -6.0);
                        // The hair fades with the body (prism vanish, 14.6).
                        let alpha = cp.hair_alpha();
                        self.push_hair_at(&mut out, slot, ent, w, it.depth, Some(anchor), alpha);
                    }
                    out.push(it);
                }
                Some(Kind::Hat(h)) => {
                    let team = Self::team_of(w, ent, h.owner as i8);
                    let k = match team {
                        Allegiance::Blue => Some(0),
                        Allegiance::Red => Some(1),
                        _ => None,
                    };
                    if h.hat != HatState::Crown
                        && let Some(k) = k
                        && let Some(t) = self
                            .art
                            .infos
                            .get(h.archer as usize)
                            .and_then(|i| i.team_hat[k])
                        && let Visual::Sprite { tex, .. } = &mut it.visual
                    {
                        *tex = t;
                    }
                    out.push(it);
                }
                Some(Kind::Arrow(ar)) => {
                    self.arrow_item(&mut out, it, ar, ent, w, c, t);
                }
                _ => out.push(it),
            }
        }
        // Breath puffs (depth -490), following their archer.
        if let Some(d) = c.sprite(self.art.cold_puff) {
            for pf in &self.puffs {
                let Some(id) = self.archers[pf.player as usize % MAXP].entity else {
                    continue;
                };
                if !w.exists(id) {
                    continue;
                }
                let frame = ((pf.t / 4.8) as usize).min(8) as i16;
                let pos = g(w.core(id).pos) + pf.offset;
                out.push(Drawable {
                    visual: Visual::Sprite {
                        tex: d.texture,
                        frame,
                        frame_w: d.frame_w,
                        frame_h: d.frame_h,
                    },
                    pos: s(pos),
                    origin: d.origin,
                    scale: tfp_sim::Vec2::new(if pf.flip { -1.0 } else { 1.0 }, 1.0),
                    rotation: 0.0,
                    flip_x: false,
                    flip_y: false,
                    tint: [255, 255, 255, (pf.alpha * 255.0) as u8],
                    depth: -490,
                    entity: id,
                    outline: 0,
                    clip_h: 1.0,
                });
            }
        }
        view.items = out;
    }

    fn team_swap(&self, it: &mut Drawable, tx: TexId, team: Allegiance, archer: usize) {
        let k = match team {
            Allegiance::Blue => 0,
            Allegiance::Red => 1,
            _ => return,
        };
        let new = if let Some(&(b, r)) = self.art.team_tex.get(&tx) {
            if k == 0 { b } else { r }
        } else if self.art.aimers.get(archer).copied().flatten() == Some(tx) {
            self.art.team_aimer[k]
        } else {
            None
        };
        if let Some(n) = new
            && let Visual::Sprite { tex, .. } = &mut it.visual
        {
            *tex = n;
        }
    }

    fn push_hair(
        &self,
        out: &mut Vec<Drawable>,
        slot: usize,
        ent: EntityId,
        w: &World,
        depth: i32,
    ) {
        self.push_hair_at(out, slot, ent, w, depth, None, 1.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn push_hair_at(
        &self,
        out: &mut Vec<Drawable>,
        slot: usize,
        ent: EntityId,
        w: &World,
        depth: i32,
        anchor: Option<G>,
        alpha: f32,
    ) {
        let Some(h) = &self.archers[slot].hair else {
            return;
        };
        if !h.visible || h.offsets.is_empty() {
            return;
        }
        let base = g(w.core(ent).pos) + anchor.unwrap_or(h.anchor);
        // A living archer's hair is outlined with the body (player.md §19.1).
        let outline = if w.player(ent).is_some_and(|p| p.invis_opacity >= 0.8) {
            255
        } else {
            0
        };
        let n = h.offsets.len();
        for i in 0..n {
            let Some(tex) = (if i + 1 == n {
                self.art.hair_end
            } else {
                self.art.hair
            }) else {
                continue;
            };
            let p = base + h.offsets[i];
            let prev = if i == 0 {
                p - G::new(0.0, 1.0)
            } else {
                base + h.offsets[i - 1]
            };
            let d = prev - p;
            out.push(Drawable {
                visual: Visual::Sprite {
                    tex,
                    frame: -1,
                    frame_w: 0,
                    frame_h: 0,
                },
                pos: s(p),
                origin: tfp_sim::Vec2::new(2.0, 2.0),
                scale: tfp_sim::Vec2::new(h.scale, h.scale),
                rotation: d.y.atan2(d.x) - PI / 2.0,
                flip_x: false,
                flip_y: false,
                tint: [
                    255,
                    255,
                    255,
                    (h.alpha * alpha.clamp(0.0, 1.0) * 255.0) as u8,
                ],
                depth,
                entity: ent,
                outline,
                clip_h: 1.0,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn arrow_item(
        &self,
        out: &mut Vec<Drawable>,
        mut it: Drawable,
        ar: &tfp_sim::arrow::Arrow,
        ent: EntityId,
        w: &World,
        c: &Content,
        t: f32,
    ) {
        let buried = ar.state == ArrowState::Buried;
        // Stick wiggle.
        if ar.state != ArrowState::Drilling {
            it.rotation += ar.wiggle.value * PI / 10.0;
        }
        let grid = |tex: TexId, frame: i16, fw: u16, fh: u16| Visual::Sprite {
            tex,
            frame,
            frame_w: fw,
            frame_h: fh,
        };
        let Visual::Sprite { tex, .. } = it.visual else {
            out.push(it);
            return;
        };
        let lit = ar.extra.fuse_on;
        match ar.arrow_type {
            ArrowType::Bomb => {
                let f = if lit {
                    ((t / 6.0) as i16) % 2
                } else if buried {
                    3
                } else {
                    0
                };
                it.visual = grid(tex, f, 15, 6);
                it.origin = tfp_sim::Vec2::new(13.0, 3.0);
            }
            ArrowType::SuperBomb => {
                let f = if lit { ((t / 6.0) as i16) % 3 } else { 2 };
                it.visual = grid(tex, f, 15, 6);
                it.origin = tfp_sim::Vec2::new(13.0, 3.0);
            }
            ArrowType::Bolt => {
                let per = if buried { 2.4 } else { 1.8 };
                it.visual = grid(tex, ((t / per) as i16) % 4, 13, 6);
            }
            ArrowType::Drill if !buried => {
                let moving = matches!(
                    ar.state,
                    ArrowState::Shooting | ArrowState::Gravity | ArrowState::Drilling
                );
                if let Some((tx, f, fw, fh, o)) =
                    Self::sprite_frame(c, self.art.drill, 0, if moving { t } else { 0.0 })
                {
                    it.visual = grid(tx, f, fw, fh);
                    it.origin = o;
                }
            }
            ArrowType::Prism => {
                let sp = if buried {
                    self.art.prism_buried
                } else {
                    self.art.prism
                };
                if let Some((tx, f, fw, fh, o)) = Self::sprite_frame(c, sp, 0, t) {
                    it.visual = grid(tx, f, fw, fh);
                    it.origin = o;
                }
            }
            ArrowType::Trigger => {
                let e = self.eyes.get(&ent).copied().unwrap_or(Eye {
                    anim: 0,
                    rate: 1.0,
                    t: 0.0,
                    linked: false,
                });
                let Some((_, frame, fw, fh, o)) =
                    Self::sprite_frame(c, self.art.trigger, e.anim, e.t)
                else {
                    out.push(it);
                    return;
                };
                let body = if buried {
                    self.art.trigger_buried
                } else {
                    self.art.trigger
                };
                if let Some(d) = c.sprite(body) {
                    it.visual = grid(d.texture, frame, fw, fh);
                    it.origin = o;
                }
                out.push(it);
                // Eyeball (colour B toward white) and pupil (colour A).
                let oc = |light| owner_color(w, c, ar.player_index, ar.team_color, light);
                let (ca, cb) = (oc(false), oc(true));
                let eyeball = [
                    (cb[0] as f32 + (255.0 - cb[0] as f32) * 0.75) as u8,
                    (cb[1] as f32 + (255.0 - cb[1] as f32) * 0.75) as u8,
                    (cb[2] as f32 + (255.0 - cb[2] as f32) * 0.75) as u8,
                    255,
                ];
                for (sp, col) in [(self.art.eyeball, eyeball), (self.art.pupil, ca)] {
                    if let Some(d) = c.sprite(sp) {
                        let mut l = it;
                        l.visual = grid(d.texture, frame, fw, fh);
                        l.tint = [col[0], col[1], col[2], it.tint[3]];
                        out.push(l);
                    }
                }
                return;
            }
            _ => {}
        }
        out.push(it);
    }
}

fn ease_cube_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - 4.0 * (1.0 - t) * (1.0 - t) * (1.0 - t)
    }
}

/// Monocle's `BigBackOut` (overshoot 2.5).
fn back_out_big(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    let s = 2.5;
    t * t * ((s + 1.0) * t + s) + 1.0
}

/// The particle types of these layers that copy archer palettes
/// (rendering.md 9.4): `ArrowInMoonglass`, `PurpleAmbience` (archer 7's
/// colour B), `YellowSleeping` (archer 5's colour B).
pub fn register_particles(p: &mut Particles, infos: &[ArcherInfo]) {
    use super::particles::archer_type;
    use tfp_render::Color;
    let col = |i: usize| {
        infos.get(i).map_or(Color::WHITE, |x| {
            Color::rgba(x.color_b[0], x.color_b[1], x.color_b[2], 255)
        })
    };
    if let Some(mut t) = archer_type("Dash", Color::WHITE, Color::WHITE) {
        t.name = "ArrowInMoonglass";
        t.c1 = Color::hex(0xE29BFF);
        t.c2 = Color::hex(0xDFC5EA);
        t.accel = G::new(0.0, -0.01);
        p.type_index(t);
    }
    let purple = col(7);
    if let Some(mut t) = archer_type("PlayerDust", purple, purple) {
        t.name = "PurpleAmbience";
        t.source = Some("ringParticle");
        t.c1 = purple * 0.8;
        t.c2 = purple * 0.4;
        t.switch = 6;
        t.speed = 0.0;
        t.speed_range = 0.0;
        t.life = 20;
        t.life_range = 4;
        t.scale_out = true;
        p.type_index(t);
    }
    let yellow = col(5);
    if let Some(mut t) = archer_type("Dash", yellow, yellow) {
        t.name = "YellowSleeping";
        t.source = Some("player/z");
        t.c1 = yellow;
        t.c2 = yellow * 0.4;
        t.switch = 16;
        t.size = 1.0;
        t.size_range = 0.2;
        t.speed = 0.3;
        t.speed_range = 0.1;
        t.accel = G::ZERO;
        t.dir = -PI / 4.0;
        t.dir_range = PI / 6.0;
        t.life = 20;
        t.life_range = 5;
        t.rotated = false;
        t.random_rotate = false;
        p.type_index(t);
    }
}

/// The cosmetics' art for a scene: reads the install's sprite data (main +
/// Dark World, and the corpse `Flash` textures) and registers what it needs
/// in `content` (once per content).
pub fn build_art(
    content: &mut Content,
    inst: &tfp_assets::GameInstall,
    gd: &tfp_assets::game_data::GameData,
) -> ArcherArt {
    use tfp_assets::sprite_data::SpriteData;
    let load = |set: &tfp_assets::ContentSet, f: &str| {
        SpriteData::load(&set.sprite_data_dir().join(f)).ok()
    };
    let mut sds: Vec<SpriteData> = load(&inst.content, "spriteData.xml").into_iter().collect();
    if let Some(dw) = &inst.dark_world {
        sds.extend(load(dw, "spriteData.xml"));
    }
    let mut flash = BTreeMap::new();
    for set in std::iter::once(&inst.content).chain(inst.dark_world.iter()) {
        for d in load(set, "corpseSpriteData.xml")
            .iter()
            .flat_map(|x| x.sprites.iter())
        {
            if let Some(f) = d.extra.get("Flash") {
                flash.insert(d.id.clone(), f.trim().to_owned());
            }
        }
    }
    let refs: Vec<&SpriteData> = sds.iter().collect();
    ArcherArt::build(content, gd, &refs, &flash)
}

/// Installs the cosmetics (and their particle types) in a presentation.
pub fn attach(ui: &mut super::VersusUi, art: ArcherArt) {
    register_particles(&mut ui.particles, &art.infos);
    ui.actors = Some(ActorFx::new(art));
}

/// `build_art` + `attach`.
pub fn install(
    ui: &mut super::VersusUi,
    content: &mut Content,
    inst: &tfp_assets::GameInstall,
    gd: &tfp_assets::game_data::GameData,
) {
    attach(ui, build_art(content, inst, gd));
}
