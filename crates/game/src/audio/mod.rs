//! Audio: SDL2 output device driven by our own [`Mixer`], or (headless)
//! the same mixer rendered offline into a WAV file.
//!
//! The game talks to [`Audio`] only; commands go over a channel to the SDL
//! audio thread, or straight into the offline mixer.

mod mixer;
mod wav;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};

use anyhow::Result;

pub use mixer::{
    Bus, Command, Mixer, MusicSource, PcmMusic, PlayParams, SoundData, VoiceId, xna_pitch,
};
pub use wav::{WavWriter, load_wav};

/// Output sample rate requested from the device and used offline (the
/// original's SFX are 44.1 kHz).
pub const SAMPLE_RATE: u32 = 44100;

struct DeviceCallback {
    mixer: Mixer,
    rx: Receiver<Command>,
}

impl sdl2::audio::AudioCallback for DeviceCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        while let Ok(cmd) = self.rx.try_recv() {
            self.mixer.apply(cmd);
        }
        self.mixer.mix(out);
    }
}

/// Runs a (possibly slow: file reads, decoding) music source on its own
/// thread and hands finished chunks to the audio thread, which never
/// blocks: on an underrun it plays silence.
pub struct StreamingSource {
    rx: Receiver<Vec<f32>>,
    chunk: Vec<f32>,
    at: usize,
    sample_rate: u32,
    ended: bool,
    underruns: u64,
}

impl StreamingSource {
    /// Frames per chunk and chunks buffered ahead (~0.4 s at 44.1 kHz).
    const CHUNK: usize = 2048;
    const AHEAD: usize = 8;

    pub fn spawn(mut inner: Box<dyn MusicSource>) -> Self {
        let sample_rate = inner.sample_rate();
        let (tx, rx): (SyncSender<Vec<f32>>, _) = mpsc::sync_channel(Self::AHEAD);
        std::thread::Builder::new()
            .name("music stream".into())
            .spawn(move || {
                loop {
                    let mut buf = vec![0.0; Self::CHUNK * 2];
                    let n = inner.read(&mut buf);
                    buf.truncate(n * 2);
                    let last = n < Self::CHUNK;
                    if tx.send(buf).is_err() || last {
                        break;
                    }
                }
            })
            .expect("spawning the music thread");
        Self {
            rx,
            chunk: Vec::new(),
            at: 0,
            sample_rate,
            ended: false,
            underruns: 0,
        }
    }

    /// Reads that found no data ready (played as silence).
    #[allow(dead_code)]
    pub fn underruns(&self) -> u64 {
        self.underruns
    }
}

impl StreamingSource {
    /// Copies ready frames into `out`; returns (frames copied, whether the
    /// producer fell behind before `out` was full).
    fn read_available(&mut self, out: &mut [f32]) -> (usize, bool) {
        let mut n = 0;
        while n < out.len() {
            if self.at >= self.chunk.len() {
                if self.ended {
                    break;
                }
                match self.rx.try_recv() {
                    Ok(c) => {
                        self.chunk = c;
                        self.at = 0;
                        continue;
                    }
                    Err(TryRecvError::Empty) => return (n / 2, true),
                    Err(TryRecvError::Disconnected) => {
                        self.ended = true;
                        break;
                    }
                }
            }
            let k = (self.chunk.len() - self.at).min(out.len() - n);
            out[n..n + k].copy_from_slice(&self.chunk[self.at..self.at + k]);
            self.at += k;
            n += k;
        }
        (n / 2, false)
    }
}

impl MusicSource for StreamingSource {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn read(&mut self, out: &mut [f32]) -> usize {
        let (n, underrun) = self.read_available(out);
        if underrun {
            // Silence for the rest; keep the stream alive.
            self.underruns += 1;
            out[n * 2..].fill(0.0);
            return out.len() / 2;
        }
        n
    }
}

enum Output {
    Device {
        tx: Sender<Command>,
        _device: sdl2::audio::AudioDevice<DeviceCallback>,
    },
    Offline {
        mixer: Mixer,
        wav: Option<WavWriter>,
        /// Fractional frames carried between `advance_offline` calls.
        carry: f64,
        buf: Vec<f32>,
    },
}

/// The game's audio front end.
#[allow(dead_code)] // Part of the API the game proper (M3+) uses.
pub struct Audio {
    out: Output,
    next_id: u64,
    sample_rate: u32,
    description: String,
}

#[allow(dead_code)] // Part of the API the game proper (M3+) uses.
impl Audio {
    /// Opens the default SDL playback device (stereo f32).
    pub fn open_device(sdl_audio: &sdl2::AudioSubsystem) -> Result<Self> {
        let desired = sdl2::audio::AudioSpecDesired {
            freq: Some(SAMPLE_RATE as i32),
            channels: Some(2),
            samples: Some(512),
        };
        let (tx, rx) = mpsc::channel();
        let device = sdl_audio
            .open_playback(None, &desired, |spec| DeviceCallback {
                mixer: Mixer::new(spec.freq as u32),
                rx,
            })
            .map_err(|e| anyhow::anyhow!("opening audio device: {e}"))?;
        let spec = *device.spec();
        if spec.channels != 2 {
            anyhow::bail!(
                "audio device opened with {} channels, need stereo",
                spec.channels
            );
        }
        device.resume();
        let description = format!(
            "SDL audio ({}), {} Hz, {} ch, {} frames",
            sdl_audio.current_audio_driver(),
            spec.freq,
            spec.channels,
            spec.samples
        );
        log::info!("{description}");
        Ok(Self {
            out: Output::Device {
                tx,
                _device: device,
            },
            next_id: 1,
            sample_rate: spec.freq as u32,
            description,
        })
    }

    /// An offline mixer; with a path, everything mixed is written to a WAV.
    pub fn offline(wav_path: Option<&std::path::Path>) -> Result<Self> {
        let wav = wav_path
            .map(|p| WavWriter::create(p, SAMPLE_RATE))
            .transpose()?;
        let description = match wav_path {
            Some(p) => format!("offline mixer -> {}", p.display()),
            None => "offline mixer (discarded)".into(),
        };
        Ok(Self {
            out: Output::Offline {
                mixer: Mixer::new(SAMPLE_RATE),
                wav,
                carry: 0.0,
                buf: Vec::new(),
            },
            next_id: 1,
            sample_rate: SAMPLE_RATE,
            description,
        })
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn is_realtime(&self) -> bool {
        matches!(self.out, Output::Device { .. })
    }

    fn send(&mut self, cmd: Command) {
        match &mut self.out {
            Output::Device { tx, .. } => {
                let _ = tx.send(cmd);
            }
            Output::Offline { mixer, .. } => mixer.apply(cmd),
        }
    }

    pub fn play(&mut self, sound: &Arc<SoundData>, params: PlayParams) -> VoiceId {
        let id = VoiceId(self.next_id);
        self.next_id += 1;
        self.send(Command::Play {
            id,
            sound: sound.clone(),
            params,
        });
        id
    }

    pub fn stop(&mut self, id: VoiceId) {
        self.send(Command::Stop(id));
    }

    pub fn set_volume(&mut self, id: VoiceId, volume: f32) {
        self.send(Command::SetVolume(id, volume));
    }

    pub fn set_pitch(&mut self, id: VoiceId, pitch: f32) {
        self.send(Command::SetPitch(id, pitch));
    }

    pub fn set_pan(&mut self, id: VoiceId, pan: f32) {
        self.send(Command::SetPan(id, pan));
    }

    /// Pauses / resumes one voice (instanced and looped effects on pause).
    pub fn set_paused(&mut self, id: VoiceId, paused: bool) {
        self.send(Command::SetPaused(id, paused));
    }

    /// Stops every effect voice; the music goes on (scene changes, so the
    /// next scene's music request can crossfade from it).
    pub fn stop_voices(&mut self) {
        self.send(Command::StopVoices);
    }

    pub fn set_bus_volume(&mut self, bus: Bus, volume: f32) {
        self.send(Command::SetBusVolume(bus, volume));
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.send(Command::SetMasterVolume(volume));
    }

    pub fn stop_all(&mut self) {
        self.send(Command::StopAll);
    }

    /// Plays music, crossfading from the current track over `fade` s. On a
    /// real device the source is read on a streaming thread; offline it is
    /// pulled directly (deterministic).
    pub fn play_music(&mut self, source: Box<dyn MusicSource>, volume: f32, fade: f32) {
        let source: Box<dyn MusicSource> = if self.is_realtime() {
            Box::new(StreamingSource::spawn(source))
        } else {
            source
        };
        self.send(Command::PlayMusic {
            source,
            volume,
            fade,
        });
    }

    pub fn stop_music(&mut self, fade: f32) {
        self.send(Command::StopMusic { fade });
    }

    pub fn set_music_volume(&mut self, volume: f32) {
        self.send(Command::SetMusicVolume(volume));
    }

    /// Offline only: mixes `seconds` of audio (appending to the WAV). No-op
    /// for a real device, which pulls audio itself.
    pub fn advance_offline(&mut self, seconds: f64) -> Result<()> {
        if let Output::Offline {
            mixer,
            wav,
            carry,
            buf,
        } = &mut self.out
        {
            let exact = seconds * mixer.sample_rate() as f64 + *carry;
            let frames = exact.floor();
            *carry = exact - frames;
            buf.resize(frames as usize * 2, 0.0);
            mixer.mix(buf);
            if let Some(w) = wav {
                w.write(buf)?;
            }
        }
        Ok(())
    }

    /// Flushes the WAV file, if any.
    pub fn finish(self) -> Result<()> {
        if let Output::Offline { wav: Some(w), .. } = self.out {
            w.finish()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_source_delivers_everything() {
        let data = Arc::new(SoundData {
            sample_rate: 1000,
            channels: 2,
            samples: (0..10_000).map(|i| i as f32).collect(),
        });
        let mut s = StreamingSource::spawn(Box::new(PcmMusic::new(data, None)));
        let mut got = Vec::new();
        let mut buf = vec![0.0; 777 * 2];
        for _ in 0..100_000 {
            let (n, underrun) = s.read_available(&mut buf);
            got.extend_from_slice(&buf[..n * 2]);
            if underrun {
                std::thread::sleep(std::time::Duration::from_micros(200));
            } else if n < 777 {
                break;
            }
        }
        assert_eq!(got.len(), 10_000);
        assert!(got.iter().enumerate().all(|(i, &x)| x == i as f32));
    }
}
