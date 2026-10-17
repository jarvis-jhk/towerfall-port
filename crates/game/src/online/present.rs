//! Sounds and screen shake for gated sim events (see [`super::gate`]).
//! Same sound naming as the offline play mode (audio.md 4): shared SFX by
//! name, archer cues by voice set.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tfp_sim::Event;
use tfp_sim::events::CharSound;

use super::gate::{LoopKey, Released};
use crate::audio::{Audio, SoundData, VoiceId};

/// Voice-set prefixes by SFX index (audio.md 4).
const VOICES: [&str; 14] = [
    "CHAR_B", "CHAR_C", "CHAR_D", "CHAR_A", "CHAR_F", "CHAR_E", "CHAR_H", "CHAR_G", "CHAR_I",
    "CHAR_E2", "CHAR_B2", "CHAR_A2", "CHAR_D2", "CHAR_I2",
];
const VOICE_FALLBACK: [usize; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 5, 0, 3, 2, 8];

fn cue_name(s: CharSound) -> &'static str {
    use CharSound::*;
    match s {
        Aim => "AIM",
        AimCancel => "AIM_CANCEL",
        AimDir => "AIM_DIR",
        FireArrow => "FIRE_ARROW",
        NoFire => "NOFIRE",
        Jump => "JUMP",
        Land => "LAND",
        Duck => "DUCK",
        Grab => "GRAB",
        ArrowGrab => "ARROW_GRAB",
        ArrowRecover => "ARROW_RECOVER",
        ArrowSteal => "ARROW_STEAL",
        Die => "DIE",
        DieBomb => "DIE_BOMB",
        DieEnv => "DIE_ENV",
        DieLaser => "DIE_LASER",
        DieStomp => "DIE_STOMP",
        Revive => "REVIVE",
    }
}

/// A score index as shown in the feed (player or team, both 1-based).
fn side(index: u8) -> String {
    format!("P{}", index + 1)
}

/// Presents released events: one-shot sounds, loops, screen shake, and a
/// short log of match-flow events for the HUD.
pub struct Presenter {
    dirs: Vec<PathBuf>,
    cache: BTreeMap<String, Vec<Arc<SoundData>>>,
    /// Voice set per content archer index.
    voices: Vec<usize>,
    loops: BTreeMap<LoopKey, VoiceId>,
    pub shake: u32,
    pub width: f32,
    /// Recent confirmed match-flow lines (newest last).
    pub feed: Vec<(u64, String)>,
    pub tick: u64,
    /// The Sounds option (0..1).
    pub volume: f32,
    /// Rumble requests released this tick: (slot, strength, frames); the
    /// scene sends them to the slot's local pad.
    pub rumble: Vec<(u8, f32, u32)>,
    /// Effect kinds and the master pitch (`crate::sfx`).
    pub sfxp: crate::sfx::SfxPlayer,
}

impl Presenter {
    pub fn new(inst: &tfp_assets::GameInstall) -> Presenter {
        let voices = tfp_assets::game_data::GameData::load(&inst.content)
            .map(|gd| {
                gd.archers
                    .archers
                    .iter()
                    .chain(gd.archers.alts.iter().flatten())
                    .enumerate()
                    .map(|(i, a)| if a.sfx >= 0 { a.sfx as usize } else { i % 9 })
                    .collect()
            })
            .unwrap_or_default();
        Presenter {
            dirs: std::iter::once(inst.content.sfx_dir())
                .chain(inst.dark_world.iter().map(|d| d.sfx_dir()))
                .collect(),
            cache: BTreeMap::new(),
            voices,
            loops: BTreeMap::new(),
            shake: 0,
            width: 320.0,
            feed: Vec::new(),
            tick: 0,
            volume: 1.0,
            rumble: Vec::new(),
            sfxp: Default::default(),
        }
    }

    fn get(&mut self, base: &str) -> &[Arc<SoundData>] {
        if !self.cache.contains_key(base) {
            // Catalogue names, array members and file bases (shared with
            // the local match).
            let v = crate::play::load_sfx_set(&self.dirs, base);
            self.cache.insert(base.to_owned(), v);
        }
        &self.cache[base]
    }

    fn pan(&self, x: f32) -> f32 {
        ((x / self.width).clamp(0.0, 1.0) - 0.5).clamp(-0.5, 0.5)
    }

    fn one_shot(&mut self, audio: &mut Audio, name: &str, x: f32, _pick: usize) {
        let pan = if x == 0.0 { 0.0 } else { self.pan(x) };
        let v = self.get(name).to_vec();
        let vol = self.volume;
        self.sfxp.play(audio, name, &v, pan, vol);
    }

    /// A recorded sound event of a replay frame (the Versus replay).
    pub fn replay_sound(&mut self, audio: &mut Audio, e: Event) {
        if !matches!(e, Event::ScreenShake { .. } | Event::Rumble { .. }) {
            self.immediate(audio, e, 0);
        }
    }

    pub fn present(&mut self, audio: &mut Audio, mut r: Released) {
        for (k, (frame, e)) in std::mem::take(&mut r.immediate).into_iter().enumerate() {
            self.immediate(audio, e, frame as usize + k);
        }
        self.confirmed(audio, r);
    }

    fn immediate(&mut self, audio: &mut Audio, e: Event, pick: usize) {
        {
            match e {
                Event::Sound { name, x } => self.one_shot(audio, name, x, pick),
                Event::Env(tfp_sim::entities::EnvEvent::Sound { name, x, .. }) => {
                    self.one_shot(audio, name, x, pick)
                }
                Event::SoundIndexed { name, index, x } => {
                    let n = format!("{name}{}", index as u32 + 1);
                    self.one_shot(audio, &n, x, pick);
                }
                Event::CharSound {
                    sound, archer, x, ..
                } => {
                    let set = self
                        .voices
                        .get(archer as usize)
                        .copied()
                        .unwrap_or(0)
                        .min(13);
                    let a = format!("{}_{}", VOICES[set], cue_name(sound));
                    let b = format!("{}_{}", VOICES[VOICE_FALLBACK[set]], cue_name(sound));
                    if self.get(&a).is_empty() {
                        self.one_shot(audio, &b, x, pick);
                    } else {
                        self.one_shot(audio, &a, x, pick);
                    }
                }
                Event::ScreenShake { frames } => self.shake = self.shake.max(frames as u32),
                Event::Rumble {
                    player,
                    strength,
                    frames,
                } => self.rumble.push((player, strength, frames as u32)),
                _ => {}
            }
        }
    }

    fn confirmed(&mut self, audio: &mut Audio, r: Released) {
        for (_, e) in r.confirmed {
            use tfp_sim::versus::VersusEvent as V;
            let line = match e {
                Event::Versus(V::RoundStart) => "FIGHT!".to_string(),
                Event::Versus(V::MiasmaSpawned) => "MIASMA!".to_string(),
                Event::Versus(V::PointGained { index }) => format!("{} +1", side(index)),
                Event::Versus(V::PointLost { index }) => format!("{} -1", side(index)),
                Event::Versus(V::FinalKill { victor }) => format!("FINAL KILL: P{}", victor + 1),
                Event::Versus(V::MatchResults { winner }) if winner >= 0 => {
                    format!("{} WINS THE MATCH", side(winner as u8))
                }
                Event::Versus(_) => continue,
                Event::Death { player, killer, .. } if killer >= 0 => {
                    format!("P{} KILLED P{}", killer + 1, player + 1)
                }
                Event::Death { player, .. } => format!("P{} DIED", player + 1),
                _ => continue,
            };
            self.feed.push((self.tick, line));
        }
        if self.feed.len() > 6 {
            let n = self.feed.len() - 6;
            self.feed.drain(..n);
        }
        for k in r.loop_stop {
            if let Some(id) = self.loops.remove(&k) {
                self.sfxp.stop_loop(audio, id);
            }
        }
        for ((player, name), x) in r.loop_start {
            let pan = self.pan(x);
            let v = self.get(name);
            if let Some(s) = v.first().cloned()
                && self.volume > 0.0
            {
                let vol = self.volume;
                let id = self.sfxp.play_loop(audio, name, &s, pan, vol);
                self.loops.insert((player, name), id);
            }
        }
    }

    /// A sound the Versus presentation asks for (results, replay).
    pub fn cue(&mut self, audio: &mut Audio, name: &str, x: f32) {
        let pick = self.tick as usize;
        self.one_shot(audio, name, x, pick);
    }

    /// One presentation tick (shake countdown, feed ageing).
    pub fn tick(&mut self) {
        self.tick += 1;
        self.shake = self.shake.saturating_sub(1);
        let now = self.tick;
        self.feed.retain(|(t, _)| now - t < 300);
    }

    /// Stop every loop (match over / aborted).
    pub fn stop_all(&mut self, audio: &mut Audio) {
        for (_, id) in std::mem::take(&mut self.loops) {
            audio.stop(id);
        }
    }
}
