//! Software mixer: many simultaneous SFX voices (volume, pitch, pan,
//! looping, buses) and a streaming music channel with crossfades. Pure
//! Rust, no I/O; driven by the SDL audio callback or by the headless WAV
//! renderer.

use std::sync::Arc;

/// Decoded PCM, interleaved `f32`, mono or stereo, at its own rate.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundData {
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl SoundData {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    /// Frame `i` as (left, right).
    #[inline]
    fn frame(&self, i: usize) -> (f32, f32) {
        if self.channels == 1 {
            let s = self.samples[i];
            (s, s)
        } else {
            (self.samples[i * 2], self.samples[i * 2 + 1])
        }
    }

    /// A mono sine tone with short fades (test signal / placeholder SFX).
    pub fn sine(freq: f32, seconds: f32, sample_rate: u32, amplitude: f32) -> Self {
        let n = (seconds * sample_rate as f32) as usize;
        let fade = (0.005 * sample_rate as f32) as usize;
        let samples = (0..n)
            .map(|i| {
                let env = (i.min(n - 1 - i) as f32 / fade.max(1) as f32).min(1.0);
                (i as f32 / sample_rate as f32 * freq * std::f32::consts::TAU).sin()
                    * amplitude
                    * env
            })
            .collect();
        Self {
            sample_rate,
            channels: 1,
            samples,
        }
    }
}

/// XNA `SoundEffect` pitch (-1..1 octaves) to a playback-rate multiplier.
pub fn xna_pitch(pitch: f32) -> f32 {
    2f32.powf(pitch.clamp(-1.0, 1.0))
}

/// Mixer buses; each has its own volume under the master volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Music = 0,
    Sfx = 1,
}

pub const BUS_COUNT: usize = 2;

/// How a voice plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayParams {
    pub volume: f32,
    /// Playback rate multiplier (1 = original pitch; see [`xna_pitch`]).
    pub pitch: f32,
    /// -1 left .. 0 centre .. 1 right (XNA balance law; the original uses
    /// -0.5..0.5 across the screen).
    pub pan: f32,
    pub looping: bool,
    pub bus: Bus,
}

impl Default for PlayParams {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            pan: 0.0,
            looping: false,
            bus: Bus::Sfx,
        }
    }
}

/// Identifies a playing voice (allocated by the caller, never reused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId(pub u64);

/// A source of music frames, pulled by the mixer (on the audio thread for
/// a real device). Must not block: wrap file/decoder sources in
/// [`super::StreamingSource`].
pub trait MusicSource: Send {
    fn sample_rate(&self) -> u32;
    /// Fills `out` with interleaved stereo frames; returns how many frames
    /// were written. Fewer than requested means the stream ended.
    fn read(&mut self, out: &mut [f32]) -> usize;
}

/// In-memory PCM music with an optional loop region (frames).
pub struct PcmMusic {
    data: Arc<SoundData>,
    pos: usize,
    /// `[start, end)`; when set, playback jumps back to `start` at `end`.
    loop_region: Option<(usize, usize)>,
}

impl PcmMusic {
    pub fn new(data: Arc<SoundData>, loop_region: Option<(usize, usize)>) -> Self {
        let n = data.frames();
        let loop_region = loop_region
            .map(|(s, e)| (s.min(n), e.min(n)))
            .filter(|(s, e)| e > s);
        Self {
            data,
            pos: 0,
            loop_region,
        }
    }

    /// Loops the whole sound.
    pub fn looping(data: Arc<SoundData>) -> Self {
        let n = data.frames();
        Self::new(data, Some((0, n)))
    }
}

impl MusicSource for PcmMusic {
    fn sample_rate(&self) -> u32 {
        self.data.sample_rate
    }

    fn read(&mut self, out: &mut [f32]) -> usize {
        let want = out.len() / 2;
        let end = self.loop_region.map_or(self.data.frames(), |r| r.1);
        let mut n = 0;
        while n < want {
            if self.pos >= end {
                match self.loop_region {
                    Some((s, _)) => self.pos = s,
                    None => break,
                }
            }
            let (l, r) = self.data.frame(self.pos);
            out[n * 2] = l;
            out[n * 2 + 1] = r;
            self.pos += 1;
            n += 1;
        }
        n
    }
}

/// Commands from the game thread to the mixer.
pub enum Command {
    Play {
        id: VoiceId,
        sound: Arc<SoundData>,
        params: PlayParams,
    },
    Stop(VoiceId),
    SetVolume(VoiceId, f32),
    SetPitch(VoiceId, f32),
    SetPan(VoiceId, f32),
    /// Holds a voice where it is (pause) or lets it go on.
    SetPaused(VoiceId, bool),
    /// Stops every voice but leaves the music playing (scene changes).
    StopVoices,
    SetBusVolume(Bus, f32),
    SetMasterVolume(f32),
    StopAll,
    /// Starts music, crossfading from the current track over `fade` s.
    PlayMusic {
        source: Box<dyn MusicSource>,
        volume: f32,
        fade: f32,
    },
    /// Fades the current track out over `fade` s.
    StopMusic {
        fade: f32,
    },
    SetMusicVolume(f32),
}

struct Voice {
    id: VoiceId,
    sound: Arc<SoundData>,
    params: PlayParams,
    /// Position in source frames (fractional).
    pos: f64,
    /// Gains at the end of the previous buffer (click-free ramps).
    last_gain: [f32; 2],
    stopping: bool,
    paused: bool,
}

/// A music track being played: pulls from its source and resamples.
struct Track {
    source: Box<dyn MusicSource>,
    /// Source frames (stereo) and read position into them.
    buf: Vec<f32>,
    pos: f64,
    ended: bool,
    volume: f32,
    /// Fade position 0..1 and change per output frame.
    fade: f32,
    fade_step: f32,
}

impl Track {
    fn new(source: Box<dyn MusicSource>, volume: f32) -> Self {
        Self {
            source,
            buf: Vec::new(),
            pos: 0.0,
            ended: false,
            volume,
            fade: 0.0,
            fade_step: 0.0,
        }
    }

    fn frames(&self) -> usize {
        self.buf.len() / 2
    }

    /// Makes sure frames `floor(pos)` and `floor(pos) + 1` are buffered.
    fn fill(&mut self) {
        let i = self.pos as usize;
        if i + 1 < self.frames() || self.ended {
            return;
        }
        // Drop consumed frames, then read a chunk.
        let keep_from = i.min(self.frames());
        self.buf.drain(..keep_from * 2);
        self.pos -= keep_from as f64;
        let have = self.buf.len();
        const CHUNK: usize = 1024;
        self.buf.resize(have + CHUNK * 2, 0.0);
        let n = self.source.read(&mut self.buf[have..]);
        self.buf.truncate(have + n.min(CHUNK) * 2);
        if n < CHUNK {
            self.ended = true;
        }
    }

    /// Next output frame (linear interpolation), or None at the end.
    fn next(&mut self, step: f64) -> Option<(f32, f32)> {
        self.fill();
        let i = self.pos as usize;
        let n = self.frames();
        if i >= n {
            return None;
        }
        let j = (i + 1).min(n - 1);
        let t = (self.pos - i as f64) as f32;
        let (l0, r0) = (self.buf[i * 2], self.buf[i * 2 + 1]);
        let (l1, r1) = (self.buf[j * 2], self.buf[j * 2 + 1]);
        self.pos += step;
        Some((l0 + (l1 - l0) * t, r0 + (r1 - r0) * t))
    }
}

/// Mixes voices and music into interleaved stereo `f32`.
pub struct Mixer {
    sample_rate: u32,
    voices: Vec<Voice>,
    bus_volume: [f32; BUS_COUNT],
    master: f32,
    max_voices: usize,
    music: Option<Track>,
    /// Tracks fading out.
    outgoing: Vec<Track>,
}

/// XNA/FNA pan law ("balance"): centre is unity on both channels; panning
/// attenuates the opposite channel linearly (pan 1 = left silent, right
/// unchanged). Applies to mono and stereo sources alike.
fn pan_gains(pan: f32) -> [f32; 2] {
    let p = pan.clamp(-1.0, 1.0);
    [
        if p > 0.0 { 1.0 - p } else { 1.0 },
        if p < 0.0 { 1.0 + p } else { 1.0 },
    ]
}

/// Equal-power fade curve.
fn fade_amp(t: f32) -> f32 {
    (t.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin()
}

#[allow(dead_code)]
impl Mixer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            voices: Vec::new(),
            bus_volume: [1.0; BUS_COUNT],
            master: 1.0,
            max_voices: 256,
            music: None,
            outgoing: Vec::new(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    /// Music tracks audible (current + fading out).
    pub fn music_tracks(&self) -> usize {
        self.music.iter().count() + self.outgoing.len()
    }

    fn fade_step(&self, seconds: f32) -> f32 {
        if seconds <= 0.0 {
            f32::INFINITY
        } else {
            1.0 / (seconds * self.sample_rate as f32)
        }
    }

    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Play { id, sound, params } => {
                if self.voices.len() >= self.max_voices {
                    // Steal the oldest non-looping voice (else the oldest).
                    let i = self
                        .voices
                        .iter()
                        .position(|v| !v.params.looping)
                        .unwrap_or(0);
                    self.voices.remove(i);
                }
                let g = self.gains(&params);
                self.voices.push(Voice {
                    id,
                    sound,
                    params,
                    pos: 0.0,
                    last_gain: g,
                    stopping: false,
                    paused: false,
                });
            }
            Command::Stop(id) => self.with_voice(id, |v| v.stopping = true),
            Command::SetVolume(id, x) => self.with_voice(id, |v| v.params.volume = x),
            Command::SetPitch(id, x) => self.with_voice(id, |v| v.params.pitch = x),
            Command::SetPan(id, x) => self.with_voice(id, |v| v.params.pan = x),
            Command::SetPaused(id, x) => self.with_voice(id, |v| v.paused = x),
            Command::StopVoices => self.voices.iter_mut().for_each(|v| v.stopping = true),
            Command::SetBusVolume(bus, x) => self.bus_volume[bus as usize] = x,
            Command::SetMasterVolume(x) => self.master = x,
            Command::StopAll => {
                self.voices.iter_mut().for_each(|v| v.stopping = true);
                self.apply(Command::StopMusic { fade: 0.0 });
            }
            Command::PlayMusic {
                source,
                volume,
                fade,
            } => {
                let step = self.fade_step(fade);
                self.apply(Command::StopMusic { fade });
                let mut t = Track::new(source, volume);
                t.fade_step = step;
                if step.is_infinite() {
                    t.fade = 1.0;
                }
                self.music = Some(t);
            }
            Command::StopMusic { fade } => {
                let step = self.fade_step(fade);
                if let Some(mut t) = self.music.take() {
                    t.fade_step = -step;
                    if step.is_infinite() {
                        return;
                    }
                    self.outgoing.push(t);
                }
            }
            Command::SetMusicVolume(v) => {
                if let Some(t) = &mut self.music {
                    t.volume = v;
                }
            }
        }
    }

    fn with_voice(&mut self, id: VoiceId, f: impl FnOnce(&mut Voice)) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.id == id) {
            f(v);
        }
    }

    fn gains(&self, p: &PlayParams) -> [f32; 2] {
        let k = p.volume * self.bus_volume[p.bus as usize] * self.master;
        let [l, r] = pan_gains(p.pan);
        [l * k, r * k]
    }

    /// Mixes into `out` (interleaved stereo), overwriting it.
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        if frames == 0 {
            return;
        }
        self.mix_voices(out, frames);
        self.mix_music(out, frames);
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }

    fn mix_voices(&mut self, out: &mut [f32], frames: usize) {
        let rate = self.sample_rate as f64;
        let mut i = 0;
        while i < self.voices.len() {
            if self.voices[i].paused && !self.voices[i].stopping {
                // Resumes with a ramp from silence.
                self.voices[i].last_gain = [0.0, 0.0];
                i += 1;
                continue;
            }
            let target = if self.voices[i].stopping {
                [0.0, 0.0]
            } else {
                self.gains(&self.voices[i].params)
            };
            let v = &mut self.voices[i];
            let step = v.sound.sample_rate as f64 / rate * v.params.pitch.max(0.0) as f64;
            let len = v.sound.frames();
            let mut finished = len == 0;
            let g0 = v.last_gain;
            let inv = 1.0 / frames as f32;
            for f in 0..frames {
                if finished {
                    break;
                }
                let t = (f + 1) as f32 * inv;
                let gl = g0[0] + (target[0] - g0[0]) * t;
                let gr = g0[1] + (target[1] - g0[1]) * t;
                let p = v.pos as usize;
                let frac = (v.pos - p as f64) as f32;
                let next = if p + 1 < len {
                    p + 1
                } else if v.params.looping {
                    0
                } else {
                    p
                };
                let (l0, r0) = v.sound.frame(p);
                let (l1, r1) = v.sound.frame(next);
                out[f * 2] += (l0 + (l1 - l0) * frac) * gl;
                out[f * 2 + 1] += (r0 + (r1 - r0) * frac) * gr;
                v.pos += step;
                if v.pos >= len as f64 {
                    if v.params.looping {
                        v.pos %= len as f64;
                    } else {
                        finished = true;
                    }
                }
            }
            v.last_gain = target;
            if finished || v.stopping {
                self.voices.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn mix_music(&mut self, out: &mut [f32], frames: usize) {
        let bus = self.bus_volume[Bus::Music as usize] * self.master;
        let rate = self.sample_rate as f64;
        let mut play = |t: &mut Track| -> bool {
            let step = t.source.sample_rate() as f64 / rate;
            for f in 0..frames {
                t.fade = (t.fade + t.fade_step).clamp(0.0, 1.0);
                let k = fade_amp(t.fade) * t.volume * bus;
                let Some((l, r)) = t.next(step) else {
                    return false;
                };
                out[f * 2] += l * k;
                out[f * 2 + 1] += r * k;
            }
            !(t.fade_step < 0.0 && t.fade <= 0.0)
        };
        if let Some(t) = &mut self.music
            && !play(t)
        {
            self.music = None;
        }
        self.outgoing.retain_mut(|t| play(t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(m: &mut Mixer, id: u64, sound: &Arc<SoundData>, params: PlayParams) {
        m.apply(Command::Play {
            id: VoiceId(id),
            sound: sound.clone(),
            params,
        });
    }

    fn dc(frames: usize, rate: u32, v: f32, channels: u16) -> Arc<SoundData> {
        Arc::new(SoundData {
            sample_rate: rate,
            channels,
            samples: vec![v; frames * channels as usize],
        })
    }

    #[test]
    fn voices_sum_and_finish() {
        let mut m = Mixer::new(48000);
        let s = dc(100, 48000, 0.25, 1);
        play(&mut m, 1, &s, PlayParams::default());
        play(&mut m, 2, &s, PlayParams::default());
        let mut out = vec![0.0; 2 * 64];
        m.mix(&mut out);
        assert!((out[10] - 0.5).abs() < 1e-6);
        assert_eq!(m.active_voices(), 2);
        m.mix(&mut out);
        assert_eq!(m.active_voices(), 0);
        assert!((out[2 * 35] - 0.5).abs() < 1e-6);
        assert_eq!(out[2 * 40], 0.0);
    }

    #[test]
    fn pitch_and_resampling() {
        let mut m = Mixer::new(48000);
        play(&mut m, 1, &dc(100, 24000, 0.1, 2), PlayParams::default());
        let mut out = vec![0.0; 2 * 199];
        m.mix(&mut out);
        assert_eq!(m.active_voices(), 1);
        m.mix(&mut out);
        assert_eq!(m.active_voices(), 0);
        play(
            &mut m,
            2,
            &dc(100, 24000, 0.1, 2),
            PlayParams {
                pitch: xna_pitch(1.0),
                ..Default::default()
            },
        );
        let mut out = vec![0.0; 2 * 100];
        m.mix(&mut out);
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn pan_buses_and_loop() {
        let mut m = Mixer::new(48000);
        let s = dc(10, 48000, 0.5, 1);
        let p = PlayParams {
            pan: 1.0,
            looping: true,
            ..Default::default()
        };
        play(&mut m, 7, &s, p);
        let mut out = vec![0.0; 2 * 64];
        m.mix(&mut out);
        let (l, r) = (out[2 * 63], out[2 * 63 + 1]);
        assert!(l.abs() < 1e-6 && (r - 0.5).abs() < 1e-6, "{l} {r}");
        assert_eq!(pan_gains(-0.5), [1.0, 0.5]);
        assert_eq!(m.active_voices(), 1, "looping voice survives");
        m.apply(Command::SetBusVolume(Bus::Sfx, 0.0));
        m.mix(&mut out);
        m.mix(&mut out);
        assert!(out.iter().all(|s| s.abs() < 1e-6));
        m.apply(Command::Stop(VoiceId(7)));
        m.mix(&mut out);
        assert_eq!(m.active_voices(), 0);
    }

    #[test]
    fn many_voices() {
        let mut m = Mixer::new(48000);
        let s = dc(1000, 48000, 0.001, 1);
        for i in 0..300 {
            play(&mut m, i, &s, PlayParams::default());
        }
        assert_eq!(m.active_voices(), 256);
        let mut out = vec![0.0; 2 * 256];
        m.mix(&mut out);
        assert!((out[100] - 0.256).abs() < 1e-3);
    }

    #[test]
    fn music_loops_and_resamples() {
        let mut m = Mixer::new(48000);
        // 24 kHz looping DC at 0.5: output stays at 0.5 forever.
        m.apply(Command::PlayMusic {
            source: Box::new(PcmMusic::looping(dc(300, 24000, 0.5, 2))),
            volume: 1.0,
            fade: 0.0,
        });
        let mut out = vec![0.0; 2 * 4800];
        for _ in 0..5 {
            m.mix(&mut out);
            assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-5));
        }
        // A non-looping track ends and is removed.
        m.apply(Command::PlayMusic {
            source: Box::new(PcmMusic::new(dc(1000, 48000, 0.25, 1), None)),
            volume: 1.0,
            fade: 0.0,
        });
        m.mix(&mut out);
        assert!((out[2 * 999] - 0.25).abs() < 1e-6);
        assert_eq!(out[2 * 1001], 0.0);
        assert_eq!(m.music_tracks(), 0);
    }

    #[test]
    fn music_crossfade_is_equal_power() {
        let mut m = Mixer::new(1000);
        let a = || Box::new(PcmMusic::looping(dc(10, 1000, 1.0, 1)));
        m.apply(Command::PlayMusic {
            source: a(),
            volume: 0.5,
            fade: 0.0,
        });
        let mut out = vec![0.0; 2 * 10];
        m.mix(&mut out);
        assert!((out[0] - 0.5).abs() < 1e-6);
        // Crossfade over 1 s (1000 frames) to a track of opposite sign.
        m.apply(Command::PlayMusic {
            source: Box::new(PcmMusic::looping(dc(10, 1000, -1.0, 1))),
            volume: 0.5,
            fade: 1.0,
        });
        assert_eq!(m.music_tracks(), 2);
        let mut out = vec![0.0; 2 * 500];
        m.mix(&mut out);
        // Halfway: both at sin(pi/4) = 0.707 -> they cancel.
        assert!(out[2 * 499].abs() < 0.01, "{}", out[2 * 499]);
        m.mix(&mut out);
        assert!((out[2 * 499] + 0.5).abs() < 1e-3);
        m.mix(&mut out);
        assert_eq!(m.music_tracks(), 1, "old track removed after its fade");
        m.apply(Command::StopMusic { fade: 0.0 });
        assert_eq!(m.music_tracks(), 0);
    }
}
