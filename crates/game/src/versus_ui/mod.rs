//! Presentation of the sim's Versus match (versus.md 5.3, 10, 12; rendering.md
//! 8, 9): tower card and round intro, P-tags, death skulls, particles,
//! final-kill spotlight, instant replay, round results and match results
//! with stats and awards. Reads only the sim's view, `HudView` and events;
//! everything here is local and cosmetic.

pub mod actors;
pub mod effects;
pub mod marks;
pub mod particles;
pub mod replay;

use tfp_render::glam::Vec2;
use tfp_render::{Color, DrawList, SubTexture, Wave};
use tfp_sim::events::ParticleKind;
use tfp_sim::versus::{HudView, Mode, Phase, ScoreEvent, VersusEvent};
use tfp_sim::world::MAX_PLAYERS;
use tfp_sim::{Event, PlayerInput, World};

use crate::front::assets::{Archer, Tower};
use crate::front::gfx::{Org, Painter, back_out, cube_in, cube_out, lerp, rgba, sine};
use particles::Particles;
use replay::{Recorder, Viewer};

/// Sounds the presenter wants played (WAV base names).
pub type Cue = (&'static str, f32);

/// A music request of the presentation (audio.md 6.2, versus.md 5.4/7/9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicCue {
    /// `PlayVersusMusic(overtime)`: the tower theme, or the boss song.
    Theme {
        overtime: bool,
    },
    Stop,
    /// `PlayImmediate` of a victory cue.
    Victory(String),
}

#[derive(Clone)]
struct Skull {
    pos: Vec2,
    t: u32,
    tint: Color,
}

#[derive(Clone)]
struct LightFade {
    pos: Vec2,
    alpha: f32,
    hold: u32,
}

/// The presentation of one replay frame (versus.md 10.2: the replay is
/// picture-based, so it shows the effects as they were).
pub struct FxSnap {
    particles: Vec<particles::Particle>,
    effects: effects::Effects,
    skulls: Vec<Skull>,
    /// The lights as `lights()` gave them, and whether the spotlight held.
    lights: Vec<(Vec2, f32)>,
    spot: bool,
    spot_alpha: f32,
    shake: bool,
}

impl FxSnap {
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            particles: Vec::new(),
            effects: Default::default(),
            skulls: Vec::new(),
            lights: Vec::new(),
            spot: false,
            spot_alpha: 0.0,
            shake: false,
        }
    }
}

/// The whole Versus presentation state.
pub struct VersusUi {
    pub particles: Particles,
    skulls: Vec<Skull>,
    fades: Vec<LightFade>,
    pub recorder: Recorder,
    pub viewer: Option<Viewer>,
    /// Local results clocks (start after the replay).
    res_t: u32,
    mres_t: u32,
    results_live: bool,
    tags_t: u32,
    final_kill: Option<u8>,
    /// The final kill's victim (the last death before it) and the last
    /// death seen.
    fk_victim: Option<u8>,
    last_death: Option<u8>,
    pub spot_alpha: f32,
    flash: Option<(Color, f32, f32)>,
    killer: [i8; MAX_PLAYERS],
    death_pos: [Option<Vec2>; MAX_PLAYERS],
    /// Confirm automatically when the screen is ready (no human players).
    pub auto: bool,
    auto_wait: u32,
    pub cues: Vec<Cue>,
    /// Sound events of replay frames entered this tick (the scene plays
    /// them like live ones).
    pub replay_sounds: Vec<Event>,
    /// Rumble every local pad (strength, frames), taken by the scene.
    pub rumble_all: Option<(f32, u32)>,
    phase: Phase,
    phase_ticks: u32,
    looks: Vec<Archer>,
    shake: u32,
    pub replays: bool,
    res_end: u32,
    /// The last music request of this step (taken by the owner).
    pub music: Option<MusicCue>,
    intro_music: bool,
    /// A human holds Alt: the replay plays at half speed (versus.md 10.2).
    slow: bool,
    alt_prev: bool,
    /// Re-watching from the round results: the results clock to restore.
    rewatch: Option<u32>,
    /// Prompt icons of the first human device: confirm, start, alt.
    pub prompt_icons: Option<[String; 3]>,
    /// Explosions and shocks (resolved against the level by the owner:
    /// `effects.resolve(level)` once per tick after `after_step`).
    pub effects: effects::Effects,
    /// Archer / corpse / arrow cosmetics (set by the owner with the game
    /// data; `update` after each step, `apply` on the view).
    pub actors: Option<actors::ActorFx>,
}

/// `BombArrows` -> `BOMB ARROWS`.
fn split_camel(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// Award icon name: the title in camelCase, punctuation removed.
pub fn award_icon(name: &str) -> String {
    let mut s = String::new();
    for (i, w) in name.split_whitespace().enumerate() {
        let w: String = w.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let mut c = w.chars();
        if let Some(f) = c.next() {
            if i == 0 {
                s.push(f.to_ascii_lowercase());
            } else {
                s.push(f);
            }
            s.push_str(c.as_str());
        }
    }
    format!("awards/{s}")
}

/// `GetDrawName` line split: at the first space if 3+ spaces, else the
/// first space at index >= 4.
pub fn award_lines(name: &str) -> (String, Option<String>) {
    let up = name.to_uppercase();
    let spaces = up.matches(' ').count();
    let split = if spaces >= 3 {
        up.find(' ')
    } else {
        up.char_indices()
            .find(|&(i, c)| c == ' ' && i >= 4)
            .map(|(i, _)| i)
    };
    match split {
        Some(i) => (up[..i].to_string(), Some(up[i + 1..].to_string())),
        None => (up, None),
    }
}

/// Round-results row layout (versus.md 10.3; 15.6 beyond 4 rows / 320 px).
pub struct Rows {
    pub ys: Vec<(usize, f32)>,
    pub label_scale: f32,
    pub sep: f32,
    pub coin_x0: f32,
    pub across: usize,
}

pub fn result_rows(hud: &HudView, w: f32) -> Rows {
    let goal = hud.goal.max(1) as f32;
    let team = hud.teams > 0;
    let rows: Vec<usize> = if team {
        (0..hud.teams as usize).collect()
    } else {
        (0..MAX_PLAYERS)
            .filter(|&p| hud.slots[p].is_some())
            .collect()
    };
    let across = if team {
        (0..hud.teams)
            .map(|t| hud.slots.iter().flatten().filter(|s| s.team == t).count())
            .max()
            .unwrap_or(1)
            .max(1)
    } else {
        1
    };
    let base_sep: f32 = if hud.mode == Mode::Headhunters {
        12.0
    } else {
        10.0
    };
    let original = !team && w <= 320.0 && rows.iter().all(|&r| r < 4)
        || (team && hud.teams == 2 && across <= 4);
    let (ys, label_scale, sep): (Vec<(usize, f32)>, f32, f32) = if original {
        let y0 = if w > 320.0 { 40.0 } else { 80.0 };
        let ys = rows.iter().map(|&r| (r, y0 + 20.0 * r as f32)).collect();
        (ys, 2.0, base_sep)
    } else {
        let n = rows.len().max(1);
        let p = ((176 / n) as f32).clamp(14.0, 20.0);
        let top = 110.0 - (n as f32 - 1.0) * p / 2.0;
        let ys = rows
            .iter()
            .enumerate()
            .map(|(k, &r)| (r, (top + k as f32 * p).round()))
            .collect();
        let sep = base_sep.min(((w - 100.0) / goal).floor());
        (ys, if p >= 18.0 { 2.0 } else { 1.0 }, sep)
    };
    // Matches the label step in `draw_round_results` (scale-1 tags, ~16 px).
    let tag_step = if across > 2 { 16.0 } else { 25.0 };
    let width = goal * sep + tag_step * across as f32;
    let coin_x0 = (w / 2.0 - (width / 2.0).floor()).floor() + tag_step * across as f32;
    Rows {
        ys,
        label_scale,
        sep,
        coin_x0,
        across,
    }
}

/// Match-results panel centres for `n` joined players (versus.md 10.4,
/// 15.6): (start, end), and whether panels are small.
pub fn panel_layout(n: usize, w: f32) -> (Vec<(Vec2, Vec2)>, bool) {
    let v = |x: f32, y: f32| Vec2::new(x, y);
    let wide = w > 320.0;
    let off = if wide { 50.0 } else { 0.0 };
    let ends: Vec<(Vec2, Vec2)> = match (n, wide) {
        (0..=2, false) => vec![
            (v(-160.0, 120.0), v(110.0, 120.0)),
            (v(480.0, 120.0), v(210.0, 120.0)),
        ],
        (3, false) => vec![
            (v(-160.0, 120.0), v(80.0, 120.0)),
            (v(160.0, 360.0), v(160.0, 120.0)),
            (v(480.0, 120.0), v(240.0, 120.0)),
        ],
        (4, false) => vec![
            (v(-160.0, 120.0), v(40.0, 120.0)),
            (v(-80.0, 120.0), v(120.0, 120.0)),
            (v(400.0, 120.0), v(200.0, 120.0)),
            (v(480.0, 120.0), v(280.0, 120.0)),
        ],
        (0..=2, true) => vec![
            (v(-160.0, 120.0), v(110.0 + off, 120.0)),
            (v(580.0, 120.0), v(210.0 + off, 120.0)),
        ],
        (3, true) => vec![
            (v(-160.0, 120.0), v(130.0, 120.0)),
            (v(210.0, 360.0), v(210.0, 120.0)),
            (v(580.0, 120.0), v(290.0, 120.0)),
        ],
        (4, true) => vec![
            (v(-160.0, 120.0), v(90.0, 120.0)),
            (v(-80.0, 120.0), v(170.0, 120.0)),
            (v(500.0, 120.0), v(250.0, 120.0)),
            (v(580.0, 120.0), v(330.0, 120.0)),
        ],
        (5, _) => vec![
            (v(-240.0, 120.0), v(50.0, 120.0)),
            (v(-160.0, 120.0), v(130.0, 120.0)),
            (v(210.0, 360.0), v(210.0, 120.0)),
            (v(580.0, 120.0), v(290.0, 120.0)),
            (v(660.0, 120.0), v(370.0, 120.0)),
        ],
        (6..=8, _) => {
            let top = n.div_ceil(2).max(n - 4);
            let top = if n == 6 { 3 } else { top.min(4) };
            let row = |k: usize, count: usize, y: f32, xs0: f32| {
                (0..count).map(move |i| v(xs0 + 80.0 * i as f32 + k as f32 * 0.0, y))
            };
            let (x0, x1) = match n {
                6 => (100.0, 160.0),
                7 => (90.0, 130.0),
                _ => (70.0, 110.0),
            };
            row(0, top, 70.0, x0)
                .map(|e| (e - v(420.0, 0.0), e))
                .chain(row(0, n - top, 180.0, x1).map(|e| (e + v(420.0, 0.0), e)))
                .collect()
        }
        _ => {
            // 9-12: two rows of up to 6 (our design).
            let pitch = 80.0f32.min((w - 12.0) / 6.0);
            let top = n.div_ceil(2);
            let row = |count: usize, y: f32| {
                let x0 = w / 2.0 - (count as f32 - 1.0) * pitch / 2.0;
                (0..count).map(move |i| v(x0 + pitch * i as f32, y))
            };
            row(top, 70.0)
                .map(|e| (e - v(420.0, 0.0), e))
                .chain(row(n - top, 180.0).map(|e| (e + v(420.0, 0.0), e)))
                .collect()
        }
    };
    (ends, n > 5)
}

/// Round-results event schedule: (start tick, event, changed crown indices).
fn schedule(hud: &HudView) -> (Vec<(u32, ScoreEvent, Vec<usize>)>, u32) {
    let mut t = 40;
    let mut out = Vec::new();
    let mut shown = hud.had_crowns;
    for e in &hud.events {
        match *e {
            ScoreEvent::Gain(_) | ScoreEvent::Lose(_) => {
                out.push((t, *e, vec![]));
                t += 20;
            }
            ScoreEvent::Crown(f) => {
                let changed: Vec<usize> = (0..MAX_PLAYERS).filter(|&i| f[i] != shown[i]).collect();
                if !changed.is_empty() {
                    out.push((t, *e, changed.clone()));
                    t += 10 * changed.len() as u32 + 30;
                }
                shown = f;
            }
        }
    }
    (out, t + 10)
}

impl VersusUi {
    pub fn new(looks: Vec<Archer>) -> Self {
        Self {
            particles: Particles::level(),
            skulls: Vec::new(),
            fades: Vec::new(),
            recorder: Recorder::default(),
            viewer: None,
            res_t: 0,
            mres_t: 0,
            results_live: false,
            tags_t: 0,
            final_kill: None,
            fk_victim: None,
            last_death: None,
            spot_alpha: 0.0,
            flash: None,
            killer: [-1; MAX_PLAYERS],
            death_pos: [None; MAX_PLAYERS],
            auto: false,
            auto_wait: 0,
            cues: Vec::new(),
            replay_sounds: Vec::new(),
            rumble_all: None,
            phase: Phase::TowerCard,
            phase_ticks: 0,
            looks,
            shake: 0,
            replays: true,
            res_end: u32::MAX,
            music: None,
            // Round 0's RoundIntro is emitted while the match is built
            // (before the first step), so the first intro is armed here.
            intro_music: true,
            slow: false,
            alt_prev: false,
            rewatch: None,
            prompt_icons: None,
            effects: effects::Effects::default(),
            actors: None,
        }
    }

    /// The winner's victory cue: `VictoryTeam` for teams, else `"Victory"`
    /// + the winning archer's (or skin's) `VictoryMusic`.
    fn victory_cue(&self, h: &HudView) -> Option<String> {
        if h.winner < 0 {
            return None;
        }
        if h.mode == Mode::TeamDeathmatch {
            return Some("VictoryTeam".into());
        }
        let slot = h.slots.get(h.winner as usize).copied().flatten()?;
        let v = self
            .look(slot.archer)
            .map_or("Team", |a| a.victory.as_str());
        Some(format!("Victory{v}"))
    }

    fn look(&self, archer: u16) -> Option<&Archer> {
        self.looks.get(archer as usize)
    }

    fn colors(&self, archer: u16) -> (Color, Color) {
        self.look(archer)
            .map(|a| (rgba(a.color_a), rgba(a.color_b)))
            .unwrap_or((Color::WHITE, Color::WHITE))
    }

    /// A full-screen flash (`ScreenFlash`): alpha 1 falling 0.03/frame.
    /// The sim emits no flash events yet (bosses, chalice...); the hook is
    /// ready for them.
    #[allow(dead_code)]
    pub fn flash(&mut self, color: Color, time_mult: f32) {
        self.flash = Some((color, 1.0, 0.03 / time_mult.max(0.01)));
    }

    /// After `after_step`, with the content: explosions resolved against the
    /// level and the actors' cosmetic state (every match scene calls both).
    pub fn post_step(&mut self, w: &World, c: &tfp_sim::Content) {
        let level = c.level(w.level);
        let th = &level.tower.theme;
        let cc = th.crack_color;
        self.effects.crack_color = Some(Color::rgba(cc[0], cc[1], cc[2], 255));
        self.effects.resolve(level);
        if let Some(a) = &mut self.actors {
            a.cold = th.cold;
            a.update(w, c, &mut self.particles);
        }
    }

    /// After every sim step: events, effects, replay recording.
    pub fn after_step(&mut self, w: &World, hud: Option<&HudView>) {
        let tm = w.time.time_mult;
        let phase = hud.map_or(Phase::Playing, |h| h.phase);
        if phase != self.phase {
            self.phase = phase;
            self.phase_ticks = 0;
            self.auto_wait = 0;
            // Indicators show for 180 ticks from the intro proper (after
            // the tower card in round 0).
            if phase == Phase::Intro {
                self.tags_t = 0;
            }
        }
        self.phase_ticks += 1;
        // Tower card, t = 10 after its 30-tick wait (versus.md 5.3 0c): the
        // stone sound, a 20-tick screen shake, every pad rumbles 1.0 / 30.
        if let Some(h) = hud
            && h.phase == Phase::TowerCard
            && h.phase_t == 40.0
        {
            self.cues.push(("sfx_trainingStartLevelStone", 0.0));
            self.shake = self.shake.max(20);
            self.rumble_all = Some((1.0, 30));
        }
        for e in w.events() {
            match *e {
                Event::Particles {
                    kind,
                    player,
                    pos,
                    count,
                    dir,
                } => {
                    let p = Vec2::new(pos.x, pos.y);
                    let archer = hud
                        .and_then(|h| h.slots.get(player as usize).copied().flatten())
                        .map(|s| s.archer)
                        .unwrap_or(player as u16);
                    match kind {
                        ParticleKind::DeathSkull => {
                            // DeathSkull types (versus.md 13.1 4.): Cursed (curse
                            // deaths), Suicide (self / environment), Normal. The
                            // step's Death event carries killer and cause.
                            let death = w.events().iter().find_map(|e| match *e {
                                Event::Death {
                                    player: dp,
                                    killer,
                                    cause,
                                } if dp == player => Some((killer, cause)),
                                _ => None,
                            });
                            let (k, cause) =
                                death.unwrap_or((self.killer[player as usize % MAX_PLAYERS], 0));
                            let tint = if cause == tfp_sim::player::DeathCause::Curse as u8 {
                                Color::hex(0xD942FF)
                            } else if k < 0 || k as u8 == player {
                                Color::hex(0xFF5335)
                            } else {
                                Color::WHITE
                            };
                            self.skulls.push(Skull { pos: p, t: 0, tint });
                            self.death_pos[player as usize % MAX_PLAYERS] = Some(p);
                        }
                        ParticleKind::Explosion | ParticleKind::SuperExplosion => {
                            self.effects.push(effects::Pending::Explosion {
                                pos: p,
                                sup: kind == ParticleKind::SuperExplosion,
                                green: false,
                            })
                        }
                        ParticleKind::ShockCircle | ParticleKind::SmallShock => {
                            self.effects.push(effects::Pending::Shock {
                                pos: p,
                                small: kind == ParticleKind::SmallShock,
                            })
                        }
                        ParticleKind::LightFade => self.fades.push(LightFade {
                            pos: p,
                            alpha: 1.0,
                            hold: 40,
                        }),
                        _ => {
                            let name = format!("{kind:?}");
                            if let Some((ty, per_archer, range)) = particles::sim_kind(&name) {
                                let idx = if per_archer {
                                    let (a, b) = self.colors(archer);
                                    particles::archer_type(ty, a, b)
                                        .map(|t| self.particles.type_index(t))
                                } else {
                                    self.particles.by_name(ty)
                                };
                                if let Some(i) = idx {
                                    let d = (dir != 0.0).then_some(dir + std::f32::consts::PI);
                                    self.particles.emit(i, count as usize, p, range, d);
                                }
                            } else if let Some(i) = self.particles.by_name(&name) {
                                self.particles
                                    .emit(i, count as usize, p, Vec2::splat(3.0), None);
                            }
                        }
                    }
                }
                // Named particles of the tower entities and explosions.
                Event::Env(tfp_sim::entities::EnvEvent::Particles {
                    kind,
                    pos,
                    count,
                    range,
                    dir,
                }) => {
                    let p = Vec2::new(pos.x, pos.y);
                    let range = Vec2::new(range.x, range.y);
                    if let Some(sup) = match kind {
                        "Explosion" | "ExplosionGreen" => Some(false),
                        "ExplosionSuper" | "ExplosionSuperGreen" => Some(true),
                        _ => None,
                    } {
                        self.effects.push(effects::Pending::Explosion {
                            pos: p,
                            sup,
                            green: kind.ends_with("Green"),
                        });
                    } else if let Some(k) = kind
                        .strip_prefix("BombParticle:")
                        .and_then(effects::DebrisKind::from_name)
                    {
                        self.effects.push(effects::Pending::Debris {
                            kind: k,
                            pos: p,
                            count,
                            range,
                            dir,
                        });
                    } else if let Some(i) = self.particles.by_name(kind) {
                        let d = (dir != 0.0).then_some(dir);
                        self.particles.emit(i, count as usize, p, range, d);
                    }
                }
                Event::Death { player, killer, .. } => {
                    self.killer[player as usize % MAX_PLAYERS] = killer;
                    self.last_death = Some(player);
                }
                Event::ScreenShake { frames } => self.shake = self.shake.max(frames as u32),
                Event::Versus(v) => match v {
                    VersusEvent::RoundIntro { .. } => {
                        self.intro_music = true;
                        self.tags_t = 0;
                        self.final_kill = None;
                        self.spot_alpha = 0.0;
                        self.skulls.clear();
                        self.fades.clear();
                        self.effects.clear();
                        if let Some(a) = &mut self.actors {
                            a.reset();
                        }
                        self.particles.clear();
                        self.recorder.clear();
                        self.results_live = false;
                    }
                    VersusEvent::RoundStart => self.cues.push(("SFX_MULTI_STARTLEVEL_ST", 0.0)),
                    VersusEvent::FinalKill { victor } => {
                        self.final_kill = Some(victor);
                        self.fk_victim = self.last_death;
                        self.cues.push(("SFX_FINALKILL_ST", 0.0));
                        self.music = Some(MusicCue::Stop);
                    }
                    VersusEvent::FinalKillCancelled => {
                        self.final_kill = None;
                        self.music = Some(MusicCue::Theme {
                            overtime: hud.is_some_and(|h| h.overtime),
                        });
                    }
                    VersusEvent::Results { .. } => {
                        self.viewer = if self.replays {
                            self.recorder.viewer()
                        } else {
                            None
                        };
                        if self.viewer.is_some() {
                            self.cues.push(("SFX_REWIND_ST", 0.0));
                        }
                        self.res_t = 0;
                        self.results_live = self.viewer.is_none();
                    }
                    VersusEvent::MatchResults { .. } => {
                        self.mres_t = 0;
                        self.cues.push(("SFX_STATSCREEN_FLYIN_ST", 0.0));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        // The intro starts the tower music at t = 70.
        if let Some(h) = hud
            && self.intro_music
            && h.phase == Phase::Intro
            && h.phase_t >= 70.0
        {
            self.intro_music = false;
            self.music = Some(MusicCue::Theme {
                overtime: h.overtime,
            });
        }
        if !w.frozen && phase == Phase::Playing {
            let snap = FxSnap {
                particles: self.particles.snapshot(),
                effects: self.effects.clone(),
                skulls: self.skulls.clone(),
                lights: Vec::new(),
                spot: false,
                spot_alpha: self.spot_alpha,
                shake: self.shake > 0,
            };
            let (lights, spot) = self.lights_live(w, &[]);
            self.recorder.record(w, tm, move || FxSnap {
                lights,
                spot,
                ..snap
            });
        }
        self.particles.update(tm);
        self.effects.update();
        for s in &mut self.skulls {
            s.t += 1;
        }
        self.skulls.retain(|s| s.t < 180);
        for f in &mut self.fades {
            if f.hold > 0 {
                f.hold -= 1;
            } else {
                f.alpha -= 0.02;
            }
        }
        self.fades.retain(|f| f.alpha > 0.0);
        self.tags_t += 1;
        self.spot_alpha = if self.final_kill.is_some() {
            (self.spot_alpha + 0.05 * tm).min(1.0)
        } else {
            0.0
        };
        if let Some((_, a, step)) = &mut self.flash {
            *a -= *step;
            if *a <= 0.0 {
                self.flash = None;
            }
        }
        self.shake = self.shake.saturating_sub(1);
        // Local results clocks.
        if let Some(v) = &mut self.viewer {
            let (entered, play_cue) = v.update(&self.recorder.frames, self.slow);
            if play_cue {
                self.cues.push(("SFX_PLAY", 0.0));
            }
            // Every recorded sound (effects, indexed ones, voices, the
            // environment's) plays again through the scene's resolver.
            for f in entered {
                self.replay_sounds
                    .extend(self.recorder.frames[f].sounds.iter().copied());
            }
            if v.done() {
                self.viewer = None;
                self.results_live = true;
                // A re-watch returns to the finished results screen.
                self.res_t = self.rewatch.take().unwrap_or(0);
            }
        } else if self.results_live && phase == Phase::Results {
            self.res_t += 1;
            if self.res_t == 10
                && let Some(h) = hud
            {
                if h.decided {
                    self.music = self.victory_cue(h).map(MusicCue::Victory);
                } else if !h.overtime && w.versus.as_ref().is_some_and(|m| m.is_in_overtime()) {
                    self.music = Some(MusicCue::Stop);
                }
            }
            if let Some(h) = hud {
                let (sched, end) = schedule(h);
                self.res_end = end;
                for (t0, e, changed) in &sched {
                    if *t0 == self.res_t {
                        match e {
                            ScoreEvent::Gain(_) => self.cues.push((
                                if h.mode == Mode::Headhunters {
                                    "SFX_MULTI_SKULLEARNED"
                                } else {
                                    "SFX_MULTI_COINEARNED"
                                },
                                0.0,
                            )),
                            ScoreEvent::Lose(_) => self.cues.push(("SFX_MULTI_SKULLNEGATIVE", 0.0)),
                            ScoreEvent::Crown(f) => {
                                if changed.iter().any(|&i| f[i]) {
                                    self.cues.push(("SFX_MULTI_CROWNEARNED", 0.0));
                                }
                            }
                        }
                    }
                }
            }
        }
        if phase == Phase::MatchResults {
            self.mres_t += 1;
            for (t, cue) in [
                (60, "SFX_STATSCREEN_KILLS_ST"),
                (90, "SFX_STATSCREEN_DEATHS_ST"),
                (120, "SFX_STATSCREEN_KILLS_ST"),
            ] {
                if self.mres_t == t {
                    self.cues.push((cue, 0.0));
                }
            }
        }
    }

    /// The presentation decides when "Continue" may reach the sim: no
    /// confirms while the replay or the local animations run; bots never
    /// confirm; with `auto` (no humans) one confirm once ready.
    pub fn gate_inputs(
        &mut self,
        hud: Option<&HudView>,
        inputs: &mut [PlayerInput],
        human: &[bool],
    ) {
        let Some(h) = hud else { return };
        let menu_phase = !matches!(h.phase, Phase::Playing | Phase::Intro);
        if !menu_phase {
            return;
        }
        let ready = match h.phase {
            Phase::TowerCard => h.phase_t >= tfp_sim::versus::CARD_CONFIRM_AT,
            Phase::Results => {
                self.viewer.is_none() && self.results_live && self.res_t >= schedule(h).1
            }
            Phase::MatchResults => self.mres_t >= 150 + 40 * 3,
            _ => false,
        };
        for (i, inp) in inputs.iter_mut().enumerate() {
            if !human.get(i).copied().unwrap_or(false) || !ready {
                inp.buttons &= !(PlayerInput::JUMP | PlayerInput::START);
            }
        }
        if ready && self.auto {
            self.auto_wait += 1;
            if self.auto_wait == 60
                && let Some(p) = (0..MAX_PLAYERS).find(|&p| h.slots[p].is_some())
                && p < inputs.len()
            {
                inputs[p].buttons |= PlayerInput::JUMP;
            }
        }
        // Alt (held) slows the replay; pressed on the finished round results
        // it watches the replay again.
        let alt = inputs
            .iter()
            .zip(human)
            .any(|(i, &hu)| hu && i.held(PlayerInput::DODGE));
        let alt_pressed = alt && !self.alt_prev;
        self.alt_prev = alt;
        self.slow = alt;
        if alt_pressed
            && h.phase == Phase::Results
            && ready
            && self.replays
            && let Some(v) = self.recorder.viewer()
        {
            self.viewer = Some(v);
            self.rewatch = Some(self.res_t);
            self.cues.push(("SFX_REWIND_ST", 0.0));
        }
        // Skip the replay with Start/Confirm from a human.
        if let Some(v) = &mut self.viewer
            && inputs
                .iter()
                .zip(human)
                .any(|(i, &hu)| hu && i.held(PlayerInput::START))
        {
            v.skip();
        }
    }

    /// The state to render: a replay frame while the replay runs.
    pub fn render_world<'a>(&'a self, live: &'a World) -> &'a World {
        match &self.viewer {
            Some(v) => self.recorder.frames.get(v.f).map_or(live, |f| &f.world),
            None => live,
        }
    }

    /// True on the frame the round-results animation has just finished
    /// (+30 ticks), for captures.
    pub fn results_done_at(&self, _phase_frames: u32) -> bool {
        self.results_live && self.res_t == self.res_end.saturating_add(30)
    }

    #[allow(dead_code)]
    pub fn replaying(&self) -> bool {
        self.viewer.is_some()
    }

    /// Screen wave for the replay distortion.
    pub fn wave(&self) -> Option<Wave> {
        let v = self.viewer.as_ref()?;
        (v.amp > 0.0).then_some(Wave {
            counter: v.counter,
            amplitude: v.amp,
            frequency: 15.0,
        })
    }

    pub fn shaking(&self) -> bool {
        match self.replay_fx() {
            Some(s) => s.shake,
            None => self.shake > 0,
        }
    }

    /// The presentation of the replay frame on screen, while one runs.
    fn replay_fx(&self) -> Option<&FxSnap> {
        let v = self.viewer.as_ref()?;
        self.recorder.frames.get(v.f)?.fx.as_deref()
    }

    /// The final-kill darkness on screen (the replay's while it runs).
    pub fn spot_alpha(&self) -> f32 {
        self.replay_fx().map_or(self.spot_alpha, |s| s.spot_alpha)
    }

    /// Explosions and shocks on screen (the replay's while it runs).
    pub fn draw_effects(&self, dl: &mut DrawList, g: &crate::tower::TowerGfx) {
        match self.replay_fx() {
            Some(s) => s.effects.draw(dl, g),
            None => self.effects.draw(dl, g),
        }
    }

    /// Extra lights: fading corpse lights; the final-kill spotlight (the
    /// replay frame's while a replay runs).
    pub fn lights(&self, w: &World, base: &[(Vec2, f32)]) -> (Vec<(Vec2, f32)>, bool) {
        if let Some(s) = self.replay_fx() {
            if s.spot {
                return (s.lights.clone(), true);
            }
            let mut out = base.to_vec();
            out.extend_from_slice(&s.lights);
            return (out, false);
        }
        self.lights_live(w, base)
    }

    fn lights_live(&self, w: &World, base: &[(Vec2, f32)]) -> (Vec<(Vec2, f32)>, bool) {
        let mut out: Vec<(Vec2, f32)> = Vec::new();
        if let Some(victor) = self.final_kill {
            // versus.md 6.7: the victim's corpse and the victor's entities
            // (`victor` is a score index: the slot, or the team in TDM);
            // a dead victor member lights where it fell.
            let side = |slot: usize| {
                w.versus
                    .as_ref()
                    .map_or(slot, |m| m.settings.score_index(slot))
            };
            for slot in 0..MAX_PLAYERS {
                if side(slot) != victor as usize || self.fk_victim == Some(slot as u8) {
                    continue;
                }
                let live = w
                    .player_by_index(slot as u8)
                    .filter(|&id| w.player(id).is_some_and(|p| !p.dead))
                    .map(|id| {
                        let c = w.core(id).pos;
                        Vec2::new(c.x, c.y)
                    });
                if let Some(p) = live.or(self.death_pos[slot]) {
                    out.push((p, 50.0));
                }
            }
            if let Some(v) = self.fk_victim
                && let Some(p) = self.death_pos[v as usize % MAX_PLAYERS]
            {
                out.push((p, 40.0));
            }
            return (out, true);
        }
        out.extend_from_slice(base);
        out.extend(self.fades.iter().map(|f| (f.pos, 40.0 * f.alpha.min(1.0))));
        (out, false)
    }

    /// Particles inside the gameplay layer (darkened with it).
    pub fn draw_particles(
        &self,
        dl: &mut DrawList,
        sprite: &dyn Fn(&str) -> Option<SubTexture>,
        layer: particles::PLayer,
    ) {
        match self.replay_fx() {
            Some(s) => self
                .particles
                .draw_some(s.particles.iter(), dl, sprite, layer),
            None => self.particles.draw_layer(dl, sprite, layer),
        }
    }

    /// The HUD layer over the composited level.
    /// Death skulls and P-tags (world space; drawn inside the level's
    /// gameplay layer at depth -1000, so the darkness covers them).
    pub fn draw_world_marks(&self, p: &mut Painter, hud: &HudView, w: &World, tick: u64) {
        let width = p.w;
        // Death skulls (in world space, wrapped like the original's HUD).
        if let Some(sk) = p.a.g("versus/skull") {
            let skulls = self.replay_fx().map_or(&self.skulls, |s| &s.skulls);
            for s in skulls {
                let e = cube_out(s.t as f32 / 180.0);
                let frame = ((s.t as f32 * 0.1) as i32).min(12);
                let cols = (sk.width() / 12).max(1);
                let f = sk.sub((frame % cols) * 12, (frame / cols) * 10, 12, 10);
                let scale = 1.0 + e + 0.25 * (s.t as f32 * 0.5).sin() * (1.0 - e);
                let amp = 2.0 + 4.0 * e;
                let pos =
                    s.pos + Vec2::new(amp * sine(s.t as u64, 60.0, 0.0), -10.0 * e - 20.0 * e * e);
                p.tex(f, pos, Org::Center, scale, s.tint * (0.7 * (1.0 - e)));
            }
        }
        // P-tags for 180 ticks from the intro start.
        if self.tags_t < 180 && matches!(hud.phase, Phase::Intro | Phase::Playing) {
            let mut tags: Vec<(Vec2, usize, u16)> = Vec::new();
            for id in w.players() {
                if let Some(pl) = w.player(id) {
                    let c = w.core(id).pos;
                    let slot = pl.index as usize;
                    if let Some(s) = hud.slots.get(slot).copied().flatten() {
                        tags.push((Vec2::new(c.x, c.y), slot, s.archer));
                    }
                }
            }
            // Stagger tags of archers spawning close together (15.6).
            tags.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
            let mut raised = vec![0.0f32; tags.len()];
            for i in 0..tags.len() {
                let near = (0..i)
                    .filter(|&j| {
                        (tags[j].0.x - tags[i].0.x).abs() < 16.0
                            && (tags[j].0.y - tags[i].0.y).abs() < 24.0
                    })
                    .count();
                raised[i] = 10.0 * near as f32;
            }
            for (k, (pos, slot, archer)) in tags.iter().enumerate() {
                let (a, b) = self.colors(*archer);
                let col = if (tick / 4).is_multiple_of(2) { a } else { b };
                let bob = 3.0 * sine(self.tags_t as u64, 40.0, std::f32::consts::PI);
                let y = (pos.y - 32.0 - raised[k] + bob).max(10.0);
                let text = format!("P{}", slot + 1);
                for dx in [0.0, -width, width] {
                    let x = pos.x + dx;
                    if x < -20.0 || x > width + 20.0 {
                        continue;
                    }
                    p.text_outlined(&text, Vec2::new(x, y), (0.5, 0.5), 2.0, col);
                    if let Some(ind) = p.a.g("versus/playerIndicator") {
                        p.tex(ind, Vec2::new(x, y + 8.0), Org::Center, 1.0, col);
                    }
                    let si = hud.slots[*slot].map_or(0, |s| s.score_index as usize);
                    if hud.had_crowns.get(si).copied().unwrap_or(false)
                        && let Some(cr) = p.a.g("versus/crown")
                    {
                        p.tex(cr, Vec2::new(x, y - 12.0), Org::Center, 1.0, Color::WHITE);
                    }
                }
            }
        }
    }

    pub fn draw_hud(
        &self,
        p: &mut Painter,
        hud: &HudView,
        tower: Option<&Tower>,
        variant_icons: &[String],
        tick: u64,
    ) {
        let width = p.w;
        let cx = width / 2.0;
        match hud.phase {
            Phase::TowerCard => self.draw_card(p, hud, tower, variant_icons),
            Phase::Intro => {
                self.draw_intro(p, hud, variant_icons);
                self.draw_draft(p, hud, tick);
            }
            Phase::Results => {
                if self.viewer.is_none() {
                    // HUDFade (versus.md 10.1): black, 0 -> 0.8 over 40
                    // ticks (cubic out), under the results.
                    let k = cube_out((self.res_t as f32 / 40.0).min(1.0));
                    p.rect(0.0, 0.0, width, 240.0, Color::BLACK * (0.8 * k));
                    self.draw_round_results(p, hud, 0.0, tick);
                }
            }
            Phase::MatchResults | Phase::Finished => {
                p.rect(0.0, 0.0, width, 240.0, Color::BLACK * 0.8);
                let up = cube_in((self.mres_t as f32 / 30.0).min(1.0));
                self.draw_round_results(p, hud, -240.0 * up, tick);
                self.draw_match_results(p, hud, tick);
            }
            Phase::Playing => {}
        }
        if let Some(v) = &self.viewer {
            self.draw_replay_ui(p, v, cx);
        }
        if let Some((c, a, _)) = self.flash {
            p.rect(0.0, 0.0, width, 240.0, c * a);
        }
    }

    /// The Treasure Draft (pickups.md 11; the layout is ours): the offers in
    /// a row on a panel, the picker's tag over the cursor, the last pick.
    fn draw_draft(&self, p: &mut Painter, hud: &HudView, tick: u64) {
        use tfp_sim::pickups::{ALL_PICKUPS, PICKUP_NAMES};
        use tfp_sim::versus::draft::Stage;
        let Some(d) = &hud.draft else { return };
        if d.stage == Stage::Finished {
            return;
        }
        const TEX: [&str; 21] = [
            "pickups/arrowPickup",
            "pickups/bombArrows",
            "pickups/superBombArrows",
            "pickups/laserArrows",
            "pickups/brambleArrows",
            "pickups/drillArrows",
            "pickups/boltArrows",
            "pickups/featherArrows",
            "pickups/triggerArrows",
            "pickups/prismArrows",
            "pickups/shieldIcon",
            "pickups/wings",
            "pickups/speedBoots",
            "pickups/mirror",
            "pickups/timeOrb",
            "pickups/darkOrb",
            "pickups/fireOrb",
            "pickups/spaceOrb",
            "pickups/chaosOrb",
            "pickups/bomb",
            "pickups/yellowGem",
        ];
        let index = |k| ALL_PICKUPS.iter().position(|&x| x == k).unwrap_or(0);
        // Pick-up images are strips of square frames: the first, <= 16 px.
        let icon = |p: &Painter, k| {
            p.a.g(TEX[index(k)]).map(|mut t: tfp_render::SubTexture| {
                let h = t.rect.h;
                t.rect.w = t.rect.w.min(h);
                (t, (16.0 / h as f32).min(1.0))
            })
        };
        let cx = p.w / 2.0;
        let n = d.offers.len().max(1) as f32;
        let pw = (n * 26.0 + 24.0).max(140.0);
        let (top, ph) = (136.0, 88.0);
        p.panel(cx - pw / 2.0, top, pw, ph);
        p.text_outlined(
            "TREASURE DRAFT",
            Vec2::new(cx, top + 9.0),
            (0.5, 0.5),
            1.0,
            Color::hex(0xFFE23F),
        );
        // Pick order: P-tags in order; the picker flashes, those done are
        // dimmed with their pick under the tag.
        let m = d.order.len().max(1) as f32;
        for (i, &slot) in d.order.iter().enumerate() {
            let x = cx + (i as f32 - (m - 1.0) / 2.0) * 18.0;
            let archer = hud.slots[slot as usize].map_or(0, |s| s.archer);
            let (a, b) = self.colors(archer);
            let done = d.picks.iter().find(|&&(s, _)| s == slot);
            let col = if d.picker == Some(slot) {
                if (tick / 4).is_multiple_of(2) { a } else { b }
            } else if done.is_some() {
                a * 0.45
            } else {
                a
            };
            p.text_outlined(
                &format!("P{}", slot + 1),
                Vec2::new(x, top + 20.0),
                (0.5, 0.5),
                1.0,
                col,
            );
            if let Some(&(_, k)) = done
                && let Some((t, sc)) = icon(p, k)
            {
                p.tex(
                    t,
                    Vec2::new(x, top + 30.0),
                    Org::Center,
                    0.6 * sc,
                    Color::WHITE,
                );
            }
        }
        let y = top + 58.0;
        for (i, &k) in d.offers.iter().enumerate() {
            let x = cx + (i as f32 - (n - 1.0) / 2.0) * 26.0;
            if let Some((t, sc)) = icon(p, k) {
                p.tex(t, Vec2::new(x, y), Org::Center, sc, Color::WHITE);
            }
            if d.picker.is_some() && i == d.cursor && d.stage == Stage::Choosing {
                let c = if (tick / 6).is_multiple_of(2) {
                    Color::WHITE
                } else {
                    Color::hex(0xFFE23F)
                };
                let (bx, by) = (x - 11.0, y - 11.0);
                p.rect(bx, by, 22.0, 1.0, c);
                p.rect(bx, by + 21.0, 22.0, 1.0, c);
                p.rect(bx, by, 1.0, 22.0, c);
                p.rect(bx + 21.0, by, 1.0, 22.0, c);
            }
        }
        if let Some(slot) = d.picker {
            let archer = hud.slots[slot as usize].map_or(0, |s| s.archer);
            let (a, _) = self.colors(archer);
            let x = cx
                + (d.cursor.min(d.offers.len().saturating_sub(1)) as f32 - (n - 1.0) / 2.0) * 26.0;
            p.text_outlined(
                &format!("P{}", slot + 1),
                Vec2::new(x, y - 15.0),
                (0.5, 0.5),
                1.0,
                a,
            );
        }
        if let Some(&(slot, k)) = d.picks.last() {
            let name = PICKUP_NAMES[index(k)];
            p.text_outlined(
                &format!("P{} TOOK {}", slot + 1, split_camel(name)),
                Vec2::new(cx, top + ph - 9.0),
                (0.5, 0.5),
                1.0,
                Color::WHITE,
            );
        }
    }

    fn draw_variant_icons(&self, p: &mut Painter, icons: &[String], t: f32, out: f32) {
        let cx = p.w / 2.0;
        for (i, name) in icons.iter().enumerate() {
            let row = i / 14;
            let in_row = (icons.len() - row * 14).min(14);
            let col = i % 14;
            let x = cx - (in_row as f32 - 1.0) * 9.0 + 18.0 * col as f32;
            let y = 120.0 + 50.0 + 18.0 * row as f32;
            let k = back_out(((t - 5.0 - i as f32) / 12.0).clamp(0.0, 1.0))
                * (1.0 - cube_in(((out - i as f32) / 8.0).clamp(0.0, 1.0)));
            if k > 0.0 {
                p.m_outlined(name, Vec2::new(x, y), Org::Center, k, Color::WHITE);
            }
        }
    }

    fn draw_card(&self, p: &mut Painter, hud: &HudView, tower: Option<&Tower>, icons: &[String]) {
        let t = hud.phase_t;
        let w = p.w;
        let c = Vec2::new(w / 2.0, 120.0);
        // After the 30-tick wait the 0.5 overlay fades in with the titles;
        // from the confirm the card slides out downward over 30 ticks and
        // the overlay goes with it.
        let out = hud.card_confirmed.map_or(0.0, |t0| {
            cube_in(((hud.phase_t - t0) / 30.0).clamp(0.0, 1.0))
        });
        let fade = cube_out(((t - 30.0) / 30.0).clamp(0.0, 1.0)) * (1.0 - out);
        p.rect(0.0, 0.0, w, 240.0, Color::BLACK * (0.5 * fade));
        if t < 30.0 {
            return;
        }
        let t = t - 30.0;
        let slide = out * 260.0;
        let target = c + Vec2::new(0.0, -50.0 + slide);
        let e = cube_in((t / 10.0).min(1.0));
        let scale = 6.0 + (2.0 - 6.0) * e;
        // The +/-10 degree start: a sign per tower name (stable per match).
        let sign = tower.map_or(1.0, |tw| {
            if tw.name.bytes().map(u32::from).sum::<u32>() % 2 == 0 {
                1.0
            } else {
                -1.0
            }
        });
        let pos = lerp(target + Vec2::new(5.0 * sign, -20.0), target, e);
        let rot = sign * std::f32::consts::PI / 18.0 * (1.0 - e);
        if let Some(tw) = tower {
            for (name, tint) in [(tw.block, Color::WHITE), (tw.icon.as_str(), Color::WHITE)] {
                if let Some(s) = p.a.m(name) {
                    p.tex_ex(s, pos, Org::Center, Vec2::splat(scale), rot, tint, false);
                }
            }
        }
        let k = cube_out((t / 30.0).min(1.0));
        let name = tower.map_or("", |t| t.name.as_str());
        let title = lerp(
            target + Vec2::new(0.0, 80.0),
            target + Vec2::new(0.0, 50.0),
            k,
        );
        p.text_outlined(name, title, (0.5, 0.5), 2.0, Color::WHITE * k);
        let (mode, col) = match hud.mode {
            Mode::LastManStanding => ("LAST MAN STANDING", 0xFFE22B),
            Mode::Headhunters => ("HEADHUNTERS", 0xFF5432),
            Mode::TeamDeathmatch => ("TEAM DEATHMATCH", 0x96DDFF),
        };
        let mp = lerp(
            target + Vec2::new(0.0, 92.0),
            target + Vec2::new(0.0, 62.0),
            k,
        );
        p.text_outlined(mode, mp, (0.5, 0.5), 1.0, Color::hex(col) * k);
        if t > 10.0 {
            self.draw_variant_icons(
                p,
                icons,
                t - 10.0,
                if slide > 0.0 { slide / 8.0 } else { 0.0 },
            );
        }
    }

    fn draw_intro(&self, p: &mut Painter, hud: &HudView, icons: &[String]) {
        let t = hud.phase_t;
        let w = p.w;
        let c = Vec2::new(w / 2.0, 120.0);
        let far = if w > 320.0 { 300.0 } else { 200.0 };
        let leave = cube_in(((t - 130.0) / 20.0).clamp(0.0, 1.0));
        let vertical = hud.teams > 0 || hud.round_index.is_multiple_of(2);
        let (round_off, fight_off) = if vertical {
            (
                Vec2::new(0.0, -140.0 * leave),
                Vec2::new(0.0, 140.0 * leave),
            )
        } else {
            (Vec2::new(-far * leave, 0.0), Vec2::new(far * leave, 0.0))
        };
        // Intro arrows slide to the centre at t=30 and spin at t=70.
        if let Some(arrow) = p.a.g("versus/introArrow") {
            let k = cube_out(((t - 30.0) / 20.0).clamp(0.0, 1.0));
            let spin = std::f32::consts::TAU * ((t - 70.0) / 60.0).clamp(0.0, 1.0);
            let pulse =
                1.0 + 0.5 * (std::f32::consts::PI * ((t - 70.0) / 60.0).clamp(0.0, 1.0)).sin();
            for (sx, flip) in [(-1.0, false), (1.0, true)] {
                let from = c + Vec2::new(160.0 * sx, 160.0);
                let pos = lerp(from, c + Vec2::new(0.0, 0.0), k) + Vec2::new(0.0, 140.0 * leave);
                p.tex_ex(
                    arrow,
                    pos,
                    Org::Center,
                    Vec2::splat(pulse),
                    spin,
                    Color::WHITE,
                    flip,
                );
            }
        }
        let round = if hud.overtime {
            "OVERTIME".to_string()
        } else {
            format!("ROUND {}", hud.round_index + 1)
        };
        p.text_outlined(
            &round,
            c + Vec2::new(0.0, -10.0) + round_off,
            (0.5, 0.5),
            2.0,
            Color::hex(0xF878F8),
        );
        if hud.teams > 0 && t >= 30.0 {
            let k = cube_out(((t - 30.0) / 20.0).clamp(0.0, 1.0));
            let n = hud.teams as f32;
            for i in 0..hud.teams {
                let target_x = if hud.teams == 2 {
                    [-60.0, 60.0][i as usize]
                } else {
                    (i as f32 - (n - 1.0) / 2.0) * 66.0
                };
                let from_x = target_x.signum() * (far + 60.0);
                let x = from_x + (target_x - from_x) * k;
                let name = if i % 2 == 0 { "teamA" } else { "teamB" };
                let sq = Vec2::new(0.5 + 0.5 * k, 1.5 - 0.5 * k);
                if let Some(s) = p.a.m(name) {
                    let tint = match i {
                        0 | 1 => Color::WHITE,
                        2 => Color::hex(0x45E045),
                        3 => Color::hex(0xFFFF66),
                        4 => Color::hex(0xA77FFF),
                        _ => Color::hex(0xFFB15E),
                    };
                    p.tex_ex(
                        s,
                        c + Vec2::new(x, -50.0 + round_off.y),
                        Org::Center,
                        sq,
                        0.0,
                        tint,
                        false,
                    );
                }
            }
        }
        if t >= 60.0 {
            let k = ((t - 60.0) / 10.0).min(1.0);
            let scale = 8.0 + (2.0 - 8.0) * k;
            p.text_outlined(
                "FIGHT!!",
                c + Vec2::new(0.0, 10.0) + fight_off,
                (0.5, 0.5),
                scale.round(),
                Color::hex(0xF83800),
            );
        }
        let out = if t >= 120.0 { t - 120.0 } else { -1e9 };
        self.draw_variant_icons(
            p,
            icons,
            t + 30.0,
            out.max(0.0) * if t >= 120.0 { 1.0 } else { 0.0 },
        );
    }

    /// Current shown score per index at the local results time.
    #[allow(clippy::type_complexity)]
    fn shown_scores(
        &self,
        hud: &HudView,
    ) -> (
        [i32; MAX_PLAYERS],
        Vec<(usize, u32, bool)>,
        [f32; MAX_PLAYERS],
    ) {
        let mut cur = hud.old_scores;
        let mut pulses = Vec::new();
        let mut crowns = [0.0f32; MAX_PLAYERS];
        for (i, c) in hud.had_crowns.iter().enumerate() {
            crowns[i] = if *c { 1.0 } else { 0.0 };
        }
        let t = if self.results_live || hud.phase != Phase::Results {
            self.res_t.max(if hud.phase == Phase::Results {
                0
            } else {
                10_000
            })
        } else {
            0
        };
        let (sched, _) = schedule(hud);
        for (t0, e, changed) in sched {
            if t < t0 {
                break;
            }
            match e {
                ScoreEvent::Gain(i) => {
                    cur[i as usize] += 1;
                    pulses.push((i as usize, t - t0, true));
                }
                ScoreEvent::Lose(i) => {
                    cur[i as usize] -= 1;
                    pulses.push((i as usize, t - t0, false));
                }
                ScoreEvent::Crown(f) => {
                    for (k, &i) in changed.iter().enumerate() {
                        let start = t0 + 10 * k as u32;
                        let e = ((t.saturating_sub(start)) as f32 / 30.0).min(1.0);
                        crowns[i] = if f[i] { back_out(e) } else { 1.0 - cube_in(e) };
                    }
                }
            }
        }
        (cur, pulses, crowns)
    }

    fn draw_round_results(&self, p: &mut Painter, hud: &HudView, dy: f32, tick: u64) {
        let w = p.w;
        let rows = result_rows(hud, w);
        let (cur, pulses, crowns) = self.shown_scores(hud);
        let goal = hud.goal.max(1);
        let hh = hud.mode == Mode::Headhunters;
        let sprite = if hh {
            p.a.g("versus/skull")
        } else {
            p.a.g("pickups/coin")
        };
        let (fw, fh) = if hh { (12, 10) } else { (8, 10) };
        let anim_frame = |t: u64| -> i32 {
            if hh {
                [0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12][((t as f32 / 6.0) as usize) % 14]
            } else {
                [0, 0, 0, 1, 2, 3, 4, 5, 6, 7][((t as f32 / 4.8) as usize) % 10]
            }
        };
        let cell = |sheet: SubTexture, f: i32| {
            let cols = (sheet.width() / fw).max(1);
            sheet.sub((f % cols) * fw, (f / cols) * fh, fw, fh)
        };
        // Team rows with many members: smaller tags, stepped by their width.
        let label_scale = if rows.across > 2 {
            1.0
        } else {
            rows.label_scale
        };
        let step = if rows.across > 2 {
            p.measure(&format!("P{}", MAX_PLAYERS)).x * label_scale + 4.0
        } else {
            25.0
        };
        for &(r, y) in &rows.ys {
            let y = y + dy;
            let score = cur[r];
            for j in 0..goal {
                let x = rows.coin_x0 + j as f32 * rows.sep;
                let Some(sh) = sprite else { continue };
                let pulse = pulses
                    .iter()
                    .rev()
                    .find(|(i, _, gain)| *i == r && *gain && cur[r] - 1 == j)
                    .map(|(_, t, _)| *t);
                let s = 1.0
                    + pulse.map_or(0.0, |t| {
                        0.3 * (-(t as f32) / 10.0).exp() * (t as f32 * 0.6).cos()
                    });
                if j < score {
                    let tint = if hh {
                        Color::hex(0xFFEC5E)
                    } else {
                        Color::WHITE
                    };
                    p.tex(
                        cell(sh, anim_frame(tick)),
                        Vec2::new(x, y),
                        Org::Center,
                        s,
                        if j < hud.old_scores[r] {
                            Color::WHITE
                        } else {
                            tint
                        },
                    );
                } else {
                    let lost = pulses
                        .iter()
                        .any(|(i, _, gain)| *i == r && !gain && cur[r] == j);
                    let tint = if lost {
                        Color::hex(0xFF5335) * 0.35
                    } else {
                        Color::WHITE * 0.35
                    };
                    p.tex(cell(sh, 0), Vec2::new(x, y), Org::Center, 1.0, tint);
                }
            }
            if score > goal {
                p.text_outlined(
                    &format!("+{}", score - goal),
                    Vec2::new(rows.coin_x0 + goal as f32 * rows.sep + 2.0, y),
                    (0.0, 0.5),
                    1.0,
                    Color::hex(0xFFEC5E),
                );
            }
            // Labels: the slot, or a team's members leftward.
            let members: Vec<usize> = if hud.teams > 0 {
                (0..MAX_PLAYERS)
                    .rev()
                    .filter(|&s| hud.slots[s].is_some_and(|x| x.team as usize == r))
                    .collect()
            } else {
                vec![r]
            };
            let mut x = rows.coin_x0 - 12.0;
            for s in &members {
                let archer = hud.slots[*s].map_or(0, |x| x.archer);
                let (a, _) = self.colors(archer);
                let col = if hud.slots[*s].is_some_and(|x| x.active) {
                    a
                } else {
                    Color::GRAY
                };
                p.text_outlined(
                    &format!("P{}", s + 1),
                    Vec2::new(x, y + 1.0),
                    (1.0, 0.5),
                    label_scale,
                    col,
                );
                x -= step;
            }
            if let Some(cr) = p.a.g("versus/crown") {
                let cx =
                    rows.coin_x0 - 12.0 - step * members.len() as f32 - cr.width() as f32 / 2.0;
                if crowns[r] > 0.0 {
                    p.tex(cr, Vec2::new(cx, y), Org::Center, crowns[r], Color::WHITE);
                }
            }
        }
        if hud.phase == Phase::Results && self.results_live && self.res_t >= schedule(hud).1 {
            let cont = if hud.decided {
                "MATCH OVER!"
            } else {
                "CONTINUE"
            };
            self.prompt(p, cont, 0, Vec2::new(w - 10.0, 226.0 + dy));
            if self.replays && !self.recorder.frames.is_empty() {
                self.prompt(p, "REPLAY", 2, Vec2::new(w - 10.0, 208.0 + dy));
            }
        }
    }

    fn draw_match_results(&self, p: &mut Painter, hud: &HudView, tick: u64) {
        let joined: Vec<usize> = (0..MAX_PLAYERS)
            .filter(|&s| hud.slots[s].is_some())
            .collect();
        let (layout, small) = panel_layout(joined.len(), p.w);
        let t = self.mres_t as f32;
        let fly = cube_out((t / 30.0).min(1.0));
        for (k, &slot) in joined.iter().enumerate() {
            let Some(&(from, to)) = layout.get(k) else {
                continue;
            };
            let s = hud.slots[slot].expect("joined");
            let won = hud.winner >= 0 && s.score_index as i32 == hud.winner;
            let settle =
                cube_out(((t - 30.0) / 30.0).clamp(0.0, 1.0)) * if won { -4.0 } else { 4.0 };
            let c = lerp(from, to, fly) + Vec2::new(0.0, settle);
            let look = self.look(s.archer);
            let (bg, h) = match (small, won) {
                (false, false) => ("versusResults/bg", 200.0),
                (false, true) => ("versusResults/winnerbg", 200.0),
                (true, false) => ("versusResults/bgSmall", 100.0),
                (true, true) => ("versusResults/winnerBgSmall", 100.0),
            };
            let bg_tex = p.a.m8(bg).or_else(|| {
                p.a.m(if won {
                    "versusResults/winnerbg"
                } else {
                    "versusResults/bg"
                })
            });
            let scale = if joined.len() > 8 {
                (80.0f32.min((p.w - 12.0) / 6.0) / 74.0).min(1.0)
            } else {
                1.0
            };
            match bg_tex {
                Some(b) if small && b.height() > 120 => p.tex_ex(
                    b,
                    c,
                    Org::Center,
                    Vec2::new(scale, 0.5),
                    0.0,
                    Color::WHITE,
                    false,
                ),
                Some(b) => p.tex(b, c, Org::Center, scale, Color::WHITE),
                None => p.rect(c.x - 37.0, c.y - h / 2.0, 74.0, h, Color::BLACK * 0.8),
            }
            let top = c.y - h / 2.0;
            // Gem and portrait.
            let gem = format!("Gem{}", s.archer.min(8));
            let gf = if won {
                p.a.anim_frame(&gem, "on", tick)
            } else {
                0
            };
            if let Some((g, _)) = p.a.sprite_frame(&gem, gf) {
                p.tex(g, Vec2::new(c.x, top + 2.0), Org::Center, 1.0, Color::WHITE);
            }
            // Bottom of the portrait: stats and awards hang below it.
            let mut pb = top + 15.0 + if small { 36.0 } else { 60.0 };
            if let Some(l) = look {
                let name = if won {
                    l.joined.replace("joined", "win")
                } else {
                    l.joined.replace("joined", "lose")
                };
                if let Some(pt) = p.a.m(&name) {
                    let ph = pt.height() as f32 * if small { 0.6 } else { 1.0 };
                    let py = top + 15.0 + ph / 2.0;
                    pb = py + ph / 2.0;
                    let border = if won {
                        Color::hex(0xFFE23F)
                    } else {
                        Color::hex(0x896B61)
                    };
                    let sc = if small { 0.6 } else { 1.0 };
                    let wpx = pt.width() as f32 * sc;
                    p.rect(
                        c.x - wpx / 2.0 - 1.0,
                        py - ph / 2.0 - 1.0,
                        wpx + 2.0,
                        ph + 2.0,
                        border,
                    );
                    p.tex(pt, Vec2::new(c.x, py), Org::Center, sc, Color::WHITE);
                }
            }
            // Stat lines and awards.
            let st = &hud.stats[slot];
            let third = if hud.teams > 0 {
                format!(
                    "TEAM: {}",
                    st.kills.self_kills + st.kills.team_kills + st.deaths.environment()
                )
            } else {
                format!("SELF: {}", st.deaths.self_kills + st.deaths.environment())
            };
            let lines = [
                format!("KILLS: {}", st.kills.kills),
                format!("DEATHS: {}", st.deaths.total() - st.deaths.environment()),
                third,
            ];
            for (i, l) in lines.iter().enumerate() {
                let start = 60.0 + 30.0 * i as f32;
                let k = cube_out(((t - start) / 30.0).clamp(0.0, 1.0));
                if k > 0.0 {
                    p.text_outlined(
                        l,
                        Vec2::new(c.x - 20.0 * (1.0 - k), pb + 10.0 + 10.0 * i as f32),
                        (0.5, 0.5),
                        1.0,
                        Color::WHITE * k,
                    );
                }
            }
            if !small && let Some(aw) = hud.awards.get(slot) {
                for (i, &a) in aw.iter().take(3).enumerate() {
                    // versus.md 10.4: 20 ticks icon in (cubic out), then
                    // 20 ticks icon shrink + name slide-in (cubic in).
                    let start = 150.0 + 40.0 * i as f32;
                    let k1 = cube_out(((t - start) / 20.0).clamp(0.0, 1.0));
                    let k2 = cube_in(((t - start - 20.0) / 20.0).clamp(0.0, 1.0));
                    if t <= start {
                        continue;
                    }
                    let name = tfp_sim::versus::stats::AWARD_NAMES
                        .get(a as usize)
                        .copied()
                        .unwrap_or("?");
                    let y = pb + 46.0 + 30.0 * i as f32;
                    if let Some(ic) = p.a.m(&award_icon(name)) {
                        p.tex_ex(
                            ic,
                            Vec2::new(c.x - 20.0 * (1.0 - k1), y),
                            Org::Center,
                            Vec2::splat(1.5 - 0.5 * k2),
                            -std::f32::consts::PI / 9.0 * (1.0 - k1),
                            Color::WHITE * k1,
                            false,
                        );
                    }
                    if k2 > 0.0 {
                        let (l1, l2) = award_lines(name);
                        let x = c.x - 20.0 * (1.0 - k2);
                        let col = Color::WHITE * k2;
                        p.text_outlined(&l1, Vec2::new(x, y + 10.0), (0.5, 0.5), 1.0, col);
                        if let Some(l2) = l2 {
                            p.text_outlined(&l2, Vec2::new(x, y + 16.0), (0.5, 0.5), 1.0, col);
                        }
                    }
                }
            }
        }
        if hud.ready || self.mres_t > 270 {
            let msg = match hud.winner {
                w if w >= 0 && hud.teams > 0 => format!(
                    "{} TEAM WINS!",
                    ["BLUE", "RED", "GREEN", "YELLOW", "PURPLE", "ORANGE"][w as usize % 6]
                ),
                w if w >= 0 => format!("P{} WINS!", w + 1),
                _ => "MATCH OVER".into(),
            };
            p.text_outlined(
                &msg,
                Vec2::new(p.w / 2.0, 12.0),
                (0.5, 0.5),
                2.0,
                Color::hex(0xFFE23F),
            );
        }
    }

    fn draw_replay_ui(&self, p: &mut Painter, v: &Viewer, cx: f32) {
        let w = p.w;
        // Scan lines (6, 2 px, white 30%).
        if v.line_alpha > 0.0 {
            for i in 0..6 {
                let y = ((i as f32 * 68.571 + v.counter * 7.0).rem_euclid(240.0)).floor();
                p.rect(0.0, y, w, 2.0, Color::WHITE * (0.3 * v.line_alpha));
            }
        }
        if v.icon_visible {
            let icon = if v.icon_play {
                "replays/play"
            } else {
                "replays/rewind"
            };
            if let Some(s) = p.a.g(icon) {
                p.tex_outlined(s, Vec2::new(cx, 115.0), Org::Center, 2.0, Color::WHITE);
            }
            if (v.t / 10).is_multiple_of(2) {
                p.text_outlined(
                    "INSTANT",
                    Vec2::new(cx, 140.0),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE,
                );
                p.text_outlined(
                    "REPLAY",
                    Vec2::new(cx, 148.0),
                    (0.5, 0.5),
                    1.0,
                    Color::WHITE,
                );
            }
        }
        self.prompt(p, "SLOW", 2, Vec2::new(w - 10.0, 212.0));
        self.prompt(p, "SKIP", 1, Vec2::new(w - 10.0, 226.0));
    }

    /// A button prompt, right-aligned at `at`: the first human device's
    /// icon (`which`: 0 confirm, 1 start, 2 alt) then the label.
    fn prompt(&self, p: &mut Painter, label: &str, which: usize, at: Vec2) {
        let mut x = at.x;
        if let Some(icons) = &self.prompt_icons
            && let Some(ic) = p.a.m(&icons[which])
        {
            let iw = ic.width() as f32;
            p.icon(&icons[which], at, Org::Just(1.0, 0.5));
            x -= iw + 4.0;
        }
        p.text_outlined(label, Vec2::new(x, at.y), (1.0, 0.5), 1.0, Color::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn award_names() {
        assert_eq!(award_icon("Most Trigger-Happy"), "awards/mostTriggerHappy");
        assert_eq!(award_icon("Who Needs Arrows?"), "awards/whoNeedsArrows");
        assert_eq!(
            award_icon("Speed of Light Award"),
            "awards/speedofLightAward"
        );
        assert_eq!(award_icon("X-Ray Vision Award"), "awards/xRayVisionAward");
        assert_eq!(
            award_lines("Most Lethal"),
            ("MOST".into(), Some("LETHAL".into()))
        );
        assert_eq!(award_lines("Late To The Party Award").0, "LATE");
        assert_eq!(award_lines("Greediest"), ("GREEDIEST".into(), None));
    }

    #[test]
    fn panels_for_every_count() {
        for n in 1..=12 {
            let (l, small) = panel_layout(n, if n > 4 { 420.0 } else { 320.0 });
            assert!(l.len() >= n, "{n}");
            assert_eq!(small, n > 5);
        }
        let (l, _) = panel_layout(4, 320.0);
        assert_eq!(l[0].1, Vec2::new(40.0, 120.0));
        let (l, _) = panel_layout(8, 420.0);
        assert_eq!(l[3].1, Vec2::new(310.0, 70.0));
        assert_eq!(l[7].1, Vec2::new(350.0, 180.0));
        let (l, _) = panel_layout(7, 420.0);
        assert_eq!(l[4].1, Vec2::new(130.0, 180.0));
    }
}
