//! Music (audio.md 6-7): the XACT banks of the original, played through
//! the mixer's music channel. Waves are streamed from their compressed
//! MS-ADPCM bytes (a 4-minute song is ~11 MB encoded, ~85 MB as f32), the
//! `Music` rules are implemented as in the original: `play` (ignored if
//! current; 1 s crossfade), `play_immediate` (hard cut), `stop` (1 s fade).
//! Victory cues play their jingle once and, a little before it ends, a
//! follow-up song (TheArchives or TheTale, 50/50) that loops.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tfp_assets::audio::adpcm;
use tfp_assets::audio::xsb::SoundBank;
use tfp_assets::audio::xwb::{Codec, WaveBank};

use crate::audio::{Audio, MusicSource};

/// One wave of the bank, encoded.
#[derive(Clone)]
struct Wave {
    data: Arc<Vec<u8>>,
    codec: Codec,
    channels: usize,
    block_align: usize,
    rate: u32,
}

/// Streams a wave: decodes ADPCM blocks as they are needed.
pub struct WaveStream {
    w: Wave,
    pos: usize,
    buf: Vec<i16>,
    at: usize,
    looping: bool,
    /// Frames still to skip (follow-up start offset), as silence.
    delay: usize,
}

impl WaveStream {
    fn new(w: Wave, looping: bool) -> Self {
        Self {
            w,
            pos: 0,
            buf: Vec::new(),
            at: 0,
            looping,
            delay: 0,
        }
    }

    /// Refills `buf`; false at the end (and not looping).
    fn refill(&mut self) -> bool {
        self.buf.clear();
        self.at = 0;
        let w = &self.w;
        if self.pos >= w.data.len() {
            if !self.looping {
                return false;
            }
            self.pos = 0;
        }
        match w.codec {
            Codec::Adpcm => {
                let end = (self.pos + w.block_align).min(w.data.len());
                if !adpcm::decode_block(&w.data[self.pos..end], w.channels, &mut self.buf) {
                    self.pos = w.data.len();
                    return self.looping && self.refill_wrap();
                }
                self.pos = end;
            }
            _ => {
                let end = (self.pos + 4096).min(w.data.len());
                let (pairs, _) = w.data[self.pos..end].as_chunks::<2>();
                self.buf
                    .extend(pairs.iter().map(|b| i16::from_le_bytes(*b)));
                self.pos = end;
            }
        }
        !self.buf.is_empty()
    }

    fn refill_wrap(&mut self) -> bool {
        self.pos = 0;
        self.refill()
    }

    /// Next stereo frame as f32, or None at the end.
    fn frame(&mut self) -> Option<(f32, f32)> {
        if self.delay > 0 {
            self.delay -= 1;
            return Some((0.0, 0.0));
        }
        let ch = self.w.channels.max(1);
        if self.at + ch > self.buf.len() && !self.refill() {
            return None;
        }
        let l = self.buf[self.at] as f32 / 32768.0;
        let r = if ch > 1 {
            self.buf[self.at + 1] as f32 / 32768.0
        } else {
            l
        };
        self.at += ch;
        Some((l, r))
    }
}

impl MusicSource for WaveStream {
    fn sample_rate(&self) -> u32 {
        self.w.rate
    }

    fn read(&mut self, out: &mut [f32]) -> usize {
        let mut n = 0;
        while n * 2 + 1 < out.len() {
            let Some((l, r)) = self.frame() else { break };
            out[n * 2] = l;
            out[n * 2 + 1] = r;
            n += 1;
        }
        n
    }
}

/// A jingle with a looping follow-up starting at `follow_at` frames.
struct Victory {
    jingle: WaveStream,
    follow: WaveStream,
    jingle_done: bool,
}

impl MusicSource for Victory {
    fn sample_rate(&self) -> u32 {
        self.jingle.w.rate
    }

    fn read(&mut self, out: &mut [f32]) -> usize {
        let mut n = 0;
        while n * 2 + 1 < out.len() {
            let a = if self.jingle_done {
                (0.0, 0.0)
            } else {
                self.jingle.frame().unwrap_or_else(|| {
                    self.jingle_done = true;
                    (0.0, 0.0)
                })
            };
            let b = self.follow.frame().unwrap_or((0.0, 0.0));
            out[n * 2] = a.0 + b.0;
            out[n * 2 + 1] = a.1 + b.1;
            n += 1;
        }
        n
    }
}

/// Cues that play once (audio.md 7.3); everything else loops.
fn one_shot(cue: &str) -> bool {
    cue.starts_with("Victory") || cue.starts_with("Unlock") || cue == "WhiteArcherReveal"
}

/// Victory follow-up start (seconds), from the cue table.
fn follow_up_at(cue: &str) -> Option<f32> {
    Some(match cue {
        "VictoryTeam" => 11.33,
        "VictoryWhite" => 10.00,
        "VictoryPink" => 10.83,
        "VictoryOrange" => 9.83,
        "VictoryGreen" => 12.50,
        "VictoryBlue" => 10.83,
        "VictoryYellow" => 10.33,
        "VictoryPurple" => 9.80,
        "VictoryCyan" => 9.80,
        "VictoryRed" => 13.00,
        "VictoryKyle" => 6.40,
        "VictoryOrangeAlt" => 10.40,
        "VictoryWhiteAlt" => 10.00,
        _ => return None,
    })
}

/// Follow-up songs of the victory cues: TheArchives (7) / TheTale (43).
const FOLLOW_UPS: [&str; 2] = ["TheArchives", "TheTale"];

type WaveCache = std::sync::Mutex<std::collections::BTreeMap<usize, Wave>>;

fn read_wave(waves: &WaveBank, track: usize) -> Option<Wave> {
    let e = waves.entries.get(track)?;
    let raw = waves.raw(track).ok()?;
    Some(Wave {
        data: Arc::new(raw),
        codec: e.format.codec,
        channels: e.format.channels as usize,
        block_align: e.format.block_align as usize,
        rate: e.format.sample_rate,
    })
}

pub struct MusicPlayer {
    bank: SoundBank,
    waves: WaveBank,
    /// Encoded waves read so far (shared with the prefetch thread).
    cache: Arc<WaveCache>,
    current: Option<String>,
    rng: u64,
    /// Volume of the music voice (the bus volume is the option).
    pub volume: f32,
}

impl MusicPlayer {
    pub fn open(music_dir: &Path) -> Result<Self> {
        let bank =
            SoundBank::load(&music_dir.join("MusicSoundBank.xsb")).context("music sound bank")?;
        let waves =
            WaveBank::open(&music_dir.join("MusicWaveBank.xwb")).context("music wave bank")?;
        Ok(Self {
            bank,
            waves,
            cache: Arc::default(),
            current: None,
            rng: 0x4D55_5349,
            volume: 0.8,
        })
    }

    fn wave(&mut self, track: usize) -> Option<Wave> {
        if let Some(w) = self.cache.lock().ok().and_then(|c| c.get(&track).cloned()) {
            return Some(w);
        }
        let w = read_wave(&self.waves, track)?;
        if let Ok(mut c) = self.cache.lock() {
            c.insert(track, w.clone());
        }
        Some(w)
    }

    /// Reads the waves of `cues` on a background thread so that starting
    /// them later does not touch the disk (a song is ~11 MB).
    pub fn prefetch(&self, cues: &[String]) {
        let mut tracks: Vec<usize> = Vec::new();
        for c in cues {
            tracks.extend(self.bank.cue_waves(c).iter().map(|w| w.track as usize));
        }
        tracks.dedup();
        let waves = self.waves.clone();
        let cache = self.cache.clone();
        let spawned = std::thread::Builder::new()
            .name("music-prefetch".into())
            .spawn(move || {
                for t in tracks {
                    if cache.lock().map_or(true, |c| c.contains_key(&t)) {
                        continue;
                    }
                    if let Some(w) = read_wave(&waves, t)
                        && let Ok(mut c) = cache.lock()
                    {
                        c.insert(t, w);
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!("music prefetch: {e}");
        }
    }

    /// The main wave of a cue (victory cues: the jingle, not a follow-up).
    fn track_of(&self, cue: &str) -> Option<usize> {
        let follow: Vec<usize> = if cue.starts_with("Victory") {
            FOLLOW_UPS
                .iter()
                .filter_map(|f| self.bank.cue_waves(f).first().map(|w| w.track as usize))
                .collect()
        } else {
            vec![]
        };
        self.bank
            .cue_waves(cue)
            .iter()
            .map(|w| w.track as usize)
            .find(|t| !follow.contains(t))
    }

    fn source(&mut self, cue: &str) -> Option<Box<dyn MusicSource>> {
        let track = self.track_of(cue)?;
        let w = self.wave(track)?;
        if let Some(at) = follow_up_at(cue) {
            let pick = FOLLOW_UPS[(crate::front::variants::next_rand(&mut self.rng) % 2) as usize];
            let ft = self.track_of(pick)?;
            let fw = self.wave(ft)?;
            let mut follow = WaveStream::new(fw, true);
            follow.delay = (at * w.rate as f32) as usize;
            return Some(Box::new(Victory {
                jingle: WaveStream::new(w, false),
                follow,
                jingle_done: false,
            }));
        }
        Some(Box::new(WaveStream::new(w, !one_shot(cue))))
    }

    /// The cue playing (as far as this player knows).
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Another player took over the music voice (a match scene).
    pub fn forget(&mut self) {
        self.current = None;
    }

    /// The bank has this cue.
    pub fn has(&self, cue: &str) -> bool {
        self.track_of(cue).is_some()
    }

    /// `Music.Play`: ignored if `cue` is current; else a 1 s crossfade.
    pub fn play(&mut self, audio: &mut Audio, cue: &str) {
        if self.current.as_deref() == Some(cue) {
            return;
        }
        match self.source(cue) {
            Some(s) => {
                println!("music: play {cue}");
                audio.play_music(s, self.volume, 1.0);
                self.current = Some(cue.to_owned());
            }
            None => log::warn!("music: no cue {cue:?}"),
        }
    }

    /// `Music.PlayImmediate`: hard cut, then the cue.
    pub fn play_immediate(&mut self, audio: &mut Audio, cue: &str) {
        if self.current.as_deref() == Some(cue) {
            return;
        }
        audio.stop_music(0.0);
        if let Some(s) = self.source(cue) {
            println!("music: play immediate {cue}");
            audio.play_music(s, self.volume, 0.0);
            self.current = Some(cue.to_owned());
        }
    }

    /// `Music.Stop`: fades out over 1 s.
    pub fn stop(&mut self, audio: &mut Audio) {
        if self.current.is_some() {
            println!("music: stop");
        }
        self.current = None;
        audio.stop_music(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_rules() {
        assert!(
            one_shot("VictoryGreen") && one_shot("UnlockMoonstone") && !one_shot("SacredGround")
        );
        assert_eq!(follow_up_at("VictoryRed"), Some(13.0));
        assert_eq!(follow_up_at("Boss"), None);
    }

    #[test]
    fn real_bank_streams() {
        let Some(inst) = tfp_assets::Installs::discover().steam else {
            return;
        };
        let mut m = MusicPlayer::open(&inst.content.music_dir()).unwrap();
        let mut s = m.source("SacredGround").expect("cue");
        let mut buf = vec![0.0; 2 * 44100];
        assert_eq!(s.read(&mut buf), 44100);
        assert!(buf.iter().any(|x| x.abs() > 0.01), "audible");
        let mut v = m.source("VictoryGreen").expect("victory");
        let mut total = 0;
        for _ in 0..20 {
            total += v.read(&mut buf);
        }
        assert_eq!(total, 20 * 44100, "jingle then the looping follow-up");
    }
}
