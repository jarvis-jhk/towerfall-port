//! Playing sound effects the way their catalogue kind says (audio.md 3):
//! Varied effects pick at random but never the same file twice in a row,
//! Instanced effects own a ring of voices (each play cuts the next one),
//! and everything that obeys the master pitch follows the slow-motion
//! time rate, live voices included. Instanced and looped voices pause with
//! the level. One player per match scene (versus, co-op, online); all
//! choices are presentation-only (their own PRNG, never the sim's).

use std::collections::BTreeMap;
use std::sync::Arc;

use tfp_assets::audio::catalogue::{SfxDef, SfxKind, sfx_table};

use crate::audio::{Audio, PlayParams, SoundData, VoiceId, xna_pitch};

/// The master pitch (XNA units, -1..1) for a time rate (audio.md 3).
pub fn pitch_for_rate(rate: f32) -> f32 {
    let p = if rate < 1.0 {
        (rate - 0.5) / 0.5 - 1.0
    } else if rate > 1.0 {
        (rate - 1.0) / 1.5
    } else {
        0.0
    };
    p.clamp(-1.0, 1.0)
}

/// The catalogue entry of an effect name (`pu_brambleGrow`), if any.
pub fn def(name: &str) -> Option<&'static SfxDef> {
    sfx_table().iter().find(|d| d.name == name)
}

/// Effect names are looked up by catalogue name; array members carry their
/// number (`char_fireLaserDeflect3`) and share the array's flags.
fn def_or_stem(name: &str) -> Option<&'static SfxDef> {
    def(name).or_else(|| def(name.trim_end_matches(|c: char| c.is_ascii_digit())))
}

#[derive(Debug)]
pub struct SfxPlayer {
    rng: u64,
    /// Varied: the file played last per effect.
    last: BTreeMap<String, usize>,
    /// Instanced: the ring of voices and the next slot.
    rings: BTreeMap<String, (Vec<Option<VoiceId>>, usize)>,
    /// Master pitch (XNA units).
    pitch: f32,
    /// Instanced and looped voices: (voice, obeys the master pitch); they
    /// are retuned and paused with the level.
    live: Vec<(VoiceId, bool)>,
    paused: bool,
}

impl Default for SfxPlayer {
    fn default() -> Self {
        Self::new(0x5EED_50FD)
    }
}

impl SfxPlayer {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: seed | 1,
            last: BTreeMap::new(),
            rings: BTreeMap::new(),
            pitch: 0.0,
            live: Vec::new(),
            paused: false,
        }
    }

    fn next(&mut self) -> u64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Which of `n` files to play: uniform, never the last one again.
    pub fn pick(&mut self, name: &str, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        let i = match self.last.get(name) {
            Some(&l) if l < n => {
                let k = (self.next() % (n as u64 - 1)) as usize;
                if k >= l { k + 1 } else { k }
            }
            _ => (self.next() % n as u64) as usize,
        };
        self.last.insert(name.to_owned(), i);
        i
    }

    /// The playback rate for an effect that does / does not obey pitch.
    fn rate(&self, obeys: bool) -> f32 {
        if obeys { xna_pitch(self.pitch) } else { 1.0 }
    }

    /// Plays one of `sounds` (the effect's files) as its kind says.
    /// `pan` is -0.5..0.5; plays at volume 0 are dropped.
    pub fn play(
        &mut self,
        audio: &mut Audio,
        name: &str,
        sounds: &[Arc<SoundData>],
        pan: f32,
        volume: f32,
    ) -> Option<VoiceId> {
        if sounds.is_empty() || volume <= 0.0 {
            return None;
        }
        let d = def_or_stem(name);
        let obeys = d.is_none_or(|d| d.obeys_pitch);
        let params = PlayParams {
            pan,
            volume,
            pitch: self.rate(obeys),
            ..Default::default()
        };
        // Files of an unlisted base (`CHAR_B_DUCK_01..`) are varied too.
        let s = sounds[self.pick(name, sounds.len())].clone();
        match d.map(|d| d.kind) {
            Some(SfxKind::Instanced(n)) => {
                let n = n.max(1) as usize;
                let (ring, at) = self
                    .rings
                    .entry(name.to_owned())
                    .or_insert_with(|| (vec![None; n], 0));
                let slot = *at % ring.len();
                *at = slot + 1;
                if let Some(old) = ring[slot].take() {
                    audio.stop(old);
                    self.live.retain(|(v, _)| *v != old);
                }
                let id = audio.play(&s, params);
                ring[slot] = Some(id);
                self.live.push((id, obeys));
                Some(id)
            }
            _ => Some(audio.play(&s, params)),
        }
    }

    /// Starts a looped effect (the caller keeps the id and stops it).
    pub fn play_loop(
        &mut self,
        audio: &mut Audio,
        name: &str,
        sound: &Arc<SoundData>,
        pan: f32,
        volume: f32,
    ) -> VoiceId {
        let obeys = def_or_stem(name).is_none_or(|d| d.obeys_pitch);
        let id = audio.play(
            sound,
            PlayParams {
                pan,
                volume,
                pitch: self.rate(obeys),
                looping: true,
                ..Default::default()
            },
        );
        if self.paused {
            audio.set_paused(id, true);
        }
        self.live.push((id, obeys));
        id
    }

    /// A looped voice was stopped by its owner.
    pub fn stop_loop(&mut self, audio: &mut Audio, id: VoiceId) {
        audio.stop(id);
        self.live.retain(|(v, _)| *v != id);
    }

    /// Follows the sim's time rate; retunes the live voices that obey it.
    pub fn set_time_rate(&mut self, audio: &mut Audio, rate: f32) {
        let p = pitch_for_rate(rate);
        if (p - self.pitch).abs() < 1e-4 {
            return;
        }
        self.pitch = p;
        let r = xna_pitch(p);
        for &(id, obeys) in &self.live {
            if obeys {
                audio.set_pitch(id, r);
            }
        }
    }

    /// Pause / resume the instanced and looped effects with the level.
    pub fn set_paused(&mut self, audio: &mut Audio, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        for &(id, _) in &self.live {
            audio.set_paused(id, paused);
        }
    }

    /// A new level / scene: stops the instanced voices, pitch back to 0
    /// (the owners stop their loops).
    pub fn reset(&mut self, audio: &mut Audio) {
        for (ring, _) in self.rings.values_mut() {
            for v in ring.iter_mut().filter_map(Option::take) {
                audio.stop(v);
            }
        }
        self.live.clear();
        self.pitch = 0.0;
        self.paused = false;
    }
}

/// The looped effects a match scene runs (audio.md 3, Looped): archer
/// loops (`Event::Loop`, keyed by slot) and entity loops (`EnvEvent::Loop`,
/// `Event::EntityLoop`, keyed by the owner); their pan follows the owner
/// every tick (`follow`).
#[derive(Default, Debug)]
pub struct Loops {
    player: BTreeMap<(u8, &'static str), VoiceId>,
    entity: BTreeMap<(tfp_sim::EntityId, &'static str), VoiceId>,
}

/// audio.md 3: -0.5..0.5 across the screen.
pub fn pan_at(x: f32, width: f32) -> f32 {
    (x / width.max(1.0)).clamp(0.0, 1.0) - 0.5
}

impl Loops {
    /// An archer loop on / off; `sound` resolves the file when it starts.
    #[allow(clippy::too_many_arguments)]
    pub fn player(
        &mut self,
        sfxp: &mut SfxPlayer,
        audio: &mut Audio,
        key: (u8, &'static str),
        on: bool,
        pan: f32,
        volume: f32,
        sound: impl FnOnce() -> Option<Arc<SoundData>>,
    ) {
        match (on, self.player.get(&key).copied()) {
            (true, Some(id)) => audio.set_pan(id, pan),
            (true, None) => {
                if volume > 0.0
                    && let Some(s) = sound()
                {
                    let id = sfxp.play_loop(audio, key.1, &s, pan, volume);
                    self.player.insert(key, id);
                }
            }
            (false, Some(id)) => {
                sfxp.stop_loop(audio, id);
                self.player.remove(&key);
            }
            (false, None) => {}
        }
    }

    /// An entity loop on / off (calls while on update its volume).
    #[allow(clippy::too_many_arguments)]
    pub fn entity(
        &mut self,
        sfxp: &mut SfxPlayer,
        audio: &mut Audio,
        key: (tfp_sim::EntityId, &'static str),
        on: bool,
        pan: f32,
        volume: f32,
        sound: impl FnOnce() -> Option<Arc<SoundData>>,
    ) {
        match (on, self.entity.get(&key).copied()) {
            (true, Some(id)) => audio.set_volume(id, volume),
            (true, None) => {
                if volume > 0.0
                    && let Some(s) = sound()
                {
                    let id = sfxp.play_loop(audio, key.1, &s, pan, volume);
                    self.entity.insert(key, id);
                }
            }
            (false, Some(id)) => {
                sfxp.stop_loop(audio, id);
                self.entity.remove(&key);
            }
            (false, None) => {}
        }
    }

    /// Every tick: each loop's pan follows its archer / entity; loops of
    /// entities that are gone stop.
    pub fn follow(
        &mut self,
        sfxp: &mut SfxPlayer,
        audio: &mut Audio,
        w: &tfp_sim::World,
        width: f32,
    ) {
        for (&(slot, _), &id) in &self.player {
            if let Some(p) = w.player_by_index(slot).and_then(|e| w.try_core(e)) {
                audio.set_pan(id, pan_at(p.pos.x, width));
            }
        }
        let mut gone = Vec::new();
        for (&(owner, name), &id) in &self.entity {
            match w.try_core(owner) {
                Some(c) => audio.set_pan(id, pan_at(c.pos.x, width)),
                None => gone.push((owner, name)),
            }
        }
        for k in gone {
            if let Some(id) = self.entity.remove(&k) {
                sfxp.stop_loop(audio, id);
            }
        }
    }

    /// A new level / leaving: every loop stops.
    pub fn stop_all(&mut self, sfxp: &mut SfxPlayer, audio: &mut Audio) {
        for id in std::mem::take(&mut self.player)
            .into_values()
            .chain(std::mem::take(&mut self.entity).into_values())
        {
            sfxp.stop_loop(audio, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_from_rate() {
        assert_eq!(pitch_for_rate(1.0), 0.0);
        assert_eq!(pitch_for_rate(0.5), -1.0);
        assert!((pitch_for_rate(0.75) + 0.5).abs() < 1e-6);
        assert!((pitch_for_rate(2.5) - 1.0).abs() < 1e-6);
        assert_eq!(pitch_for_rate(0.1), -1.0);
    }

    #[test]
    fn varied_never_repeats_and_covers_all() {
        let mut p = SfxPlayer::new(7);
        let mut seen = [0u32; 4];
        let mut last = usize::MAX;
        for _ in 0..400 {
            let i = p.pick("x", 4);
            assert_ne!(i, last);
            seen[i] += 1;
            last = i;
        }
        assert!(seen.iter().all(|&n| n > 60), "{seen:?}");
        assert_eq!(p.pick("one", 1), 0);
    }

    #[test]
    fn instanced_ring_cuts_the_next_voice() {
        let mut audio = Audio::offline(None).unwrap();
        let s = Arc::new(SoundData::sine(440.0, 1.0, 44100, 0.1));
        let mut p = SfxPlayer::default();
        // sfx_finalKill is instanced with one instance: every play cuts.
        let d = def("sfx_finalKill").expect("in the catalogue");
        assert_eq!(d.kind, SfxKind::Instanced(1));
        let a = p.play(
            &mut audio,
            "sfx_finalKill",
            std::slice::from_ref(&s),
            0.0,
            1.0,
        );
        let b = p.play(
            &mut audio,
            "sfx_finalKill",
            std::slice::from_ref(&s),
            0.0,
            1.0,
        );
        assert_ne!(a, b);
        assert_eq!(p.live.len(), 1);
        assert_eq!(p.rings["sfx_finalKill"].0, vec![b]);
        // Muted plays are dropped.
        assert!(
            p.play(&mut audio, "sfx_finalKill", &[s], 0.0, 0.0)
                .is_none()
        );
    }
}
