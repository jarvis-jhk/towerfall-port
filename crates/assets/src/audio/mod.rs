//! Audio: WAV sound effects and the XACT music banks (wave bank, sound bank,
//! global settings). See `docs/formats/audio.md`.

pub mod adpcm;
pub mod catalogue;
mod sfx_table;
pub mod stream;
pub mod wav;
pub mod xgs;
pub mod xsb;
pub mod xwb;

pub use wav::Wav;

/// Decoded audio: interleaved signed 16-bit samples.
#[derive(Clone, PartialEq, Eq)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    /// Interleaved, `frames() * channels` long.
    pub samples: Vec<i16>,
    /// Loop region in frames (`start`, `length`), if the source has one.
    pub loop_region: Option<(u32, u32)>,
}

impl std::fmt::Debug for Pcm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pcm({} ch, {} Hz, {} frames, loop {:?})",
            self.channels,
            self.sample_rate,
            self.frames(),
            self.loop_region
        )
    }
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }

    /// Serialises as a canonical 16-bit PCM RIFF/WAVE file.
    pub fn to_wav_bytes(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut v = Vec::with_capacity(44 + data_len as usize);
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&self.channels.to_le_bytes());
        v.extend_from_slice(&self.sample_rate.to_le_bytes());
        let block = self.channels as u32 * 2;
        v.extend_from_slice(&(self.sample_rate * block).to_le_bytes());
        v.extend_from_slice(&(block as u16).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for s in &self.samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }
}

/// Little-endian cursor over a byte slice; every read is bounds-checked.
pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Reader { data, pos }
    }
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))
    }
    pub fn i16(&mut self) -> Option<i16> {
        Some(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    pub fn i32(&mut self) -> Option<i32> {
        Some(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.bytes(8)?.try_into().ok()?))
    }
    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_bits(self.u32()?))
    }
    /// NUL-terminated string at the cursor.
    pub fn cstr(&mut self) -> Option<String> {
        let rest = self.data.get(self.pos..)?;
        let n = rest.iter().position(|&b| b == 0)?;
        self.pos += n + 1;
        Some(String::from_utf8_lossy(&rest[..n]).into_owned())
    }
    /// Fixed-size, NUL-padded string.
    pub fn fixed_str(&mut self, n: usize) -> Option<String> {
        let b = self.bytes(n)?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(n);
        Some(String::from_utf8_lossy(&b[..end]).into_owned())
    }
}

/// The game's music: sound bank (cue names) + wave bank (ADPCM audio).
/// Decoding is fast (all 81 minutes in ~2 s single-threaded), so no
/// decoded cache is kept; decode a track when it's needed.
pub struct MusicBank {
    pub sound_bank: xsb::SoundBank,
    pub wave_bank: xwb::WaveBank,
    pub settings: Option<xgs::GlobalSettings>,
}

impl MusicBank {
    /// Opens `Music/Win/{MusicSoundBank.xsb, MusicWaveBank.xwb, TowerFallMusic.xgs}`.
    pub fn open(content: &crate::install::ContentSet) -> crate::Result<MusicBank> {
        let dir = content.music_dir();
        Ok(MusicBank {
            sound_bank: xsb::SoundBank::load(&dir.join("MusicSoundBank.xsb"))?,
            wave_bank: xwb::WaveBank::open(&dir.join("MusicWaveBank.xwb"))?,
            settings: xgs::GlobalSettings::load(&dir.join("TowerFallMusic.xgs")).ok(),
        })
    }

    pub fn cue_names(&self) -> impl Iterator<Item = &str> {
        self.sound_bank.cues.iter().map(|c| c.name.as_str())
    }

    /// The play events of a cue's (first) sound: what to start and when.
    pub fn cue_events(&self, cue: &str) -> Vec<xsb::PlayEvent> {
        let Some(c) = self.sound_bank.cue(cue) else {
            return Vec::new();
        };
        let Some(s) = c.sounds.first().and_then(|o| self.sound_bank.sound(*o)) else {
            return Vec::new();
        };
        if s.events.is_empty() {
            return s
                .waves
                .iter()
                .map(|w| xsb::PlayEvent {
                    clip: 0,
                    timestamp_ms: 0,
                    loop_count: 0,
                    tracks: vec![*w],
                })
                .collect();
        }
        s.events.clone()
    }

    /// A streaming decoder for the first wave a cue plays, looping forever
    /// if the cue's play event says so (loop count 255; all level/menu
    /// music does).
    pub fn stream_cue(&self, cue: &str) -> crate::Result<stream::MusicStream> {
        let ev = self.cue_events(cue);
        let e = ev
            .first()
            .ok_or_else(|| crate::Error::format(&self.sound_bank.path, format!("no cue {cue}")))?;
        let w = e
            .tracks
            .first()
            .copied()
            .unwrap_or(xsb::WaveRef { bank: 0, track: 0 });
        stream::MusicStream::open(&self.wave_bank, w.track as usize, e.loop_count == 255)
    }

    /// Decodes the first wave a cue plays.
    pub fn decode_cue(&self, cue: &str) -> crate::Result<Pcm> {
        let ev = self.cue_events(cue);
        let w = ev
            .first()
            .and_then(|e| e.tracks.first())
            .ok_or_else(|| crate::Error::format(&self.sound_bank.path, format!("no cue {cue}")))?;
        self.wave_bank.decode(w.track as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn music_bank_decodes_a_cue() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let m = MusicBank::open(&steam.content).unwrap();
        assert_eq!(m.cue_names().count(), 45);
        let ev = m.cue_events("SacredGround");
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].loop_count, 255, "level music loops forever");
        let pcm = m.decode_cue("SacredGround").unwrap();
        assert_eq!(pcm.channels, 2);
        assert!(pcm.seconds() > 60.0, "{pcm:?}");
    }
}
