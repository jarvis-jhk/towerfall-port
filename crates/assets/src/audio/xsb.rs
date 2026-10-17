//! XACT sound banks (`*.xsb`, content version 46): cue names → sounds →
//! (wave bank, track) references. Only what's needed to find the wave(s) a
//! cue plays is parsed: simple/complex cues, the cue name table, sounds and
//! their clips' play-wave events.

use super::Reader;
use crate::error::{Error, Result, read_file};
use std::path::{Path, PathBuf};

/// A wave reference: wave bank index (into [`SoundBank::wave_banks`]) and
/// track (entry index in that bank).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WaveRef {
    pub bank: u8,
    pub track: u16,
}

/// A play-wave event of a clip: at `timestamp_ms` after the sound starts,
/// play one of `tracks` (several = a random/shuffled variation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayEvent {
    pub clip: usize,
    pub timestamp_ms: u32,
    /// 0 = once, 255 = forever.
    pub loop_count: u8,
    pub tracks: Vec<WaveRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub offset: u32,
    pub flags: u8,
    pub category: u16,
    /// Raw XACT volume byte.
    pub volume: u8,
    pub pitch: i16,
    pub priority: u8,
    /// Every wave the sound can play (union over its events).
    pub waves: Vec<WaveRef>,
    /// Clip events, in clip order (empty for a simple sound).
    pub events: Vec<PlayEvent>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub name: String,
    /// Sounds this cue may pick (one for a plain cue, several for a
    /// variation table).
    pub sounds: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct SoundBank {
    pub path: PathBuf,
    pub name: String,
    pub wave_banks: Vec<String>,
    pub cues: Vec<Cue>,
    pub sounds: Vec<Sound>,
}

impl SoundBank {
    pub fn load(path: &Path) -> Result<SoundBank> {
        let data = read_file(path)?;
        Self::parse(path, &data)
    }

    pub fn parse(path: &Path, d: &[u8]) -> Result<SoundBank> {
        let bad = |m: String| Error::format(path, m);
        let short = || Error::format(path, "truncated sound bank");
        if d.len() < 0x8a || &d[0..4] != b"SDBK" {
            return Err(bad("not a sound bank (no SDBK)".into()));
        }
        let mut r = Reader::at(d, 4);
        let version = r.u16().ok_or_else(short)?;
        if version != 46 {
            return Err(bad(format!("unsupported sound bank version {version}")));
        }
        let _tool = r.u16();
        let _crc = r.u16();
        let _time = r.u64();
        let _platform = r.u8();
        let n_simple = r.u16().ok_or_else(short)? as usize;
        let n_complex = r.u16().ok_or_else(short)? as usize;
        let _unk = r.u16();
        let _n_total = r.u16();
        let n_banks = r.u8().ok_or_else(short)? as usize;
        let _n_sounds = r.u16();
        let _name_len = r.u16();
        let _unk2 = r.u16();
        let simple_off = r.u32().ok_or_else(short)? as usize;
        let complex_off = r.u32().ok_or_else(short)? as usize;
        let _names_off = r.u32();
        let _unk3 = r.u32();
        let variations_off = r.u32().ok_or_else(short)?;
        let _unk4 = r.u32();
        let banks_off = r.u32().ok_or_else(short)? as usize;
        let _hash_off = r.u32();
        let hash_vals_off = r.u32().ok_or_else(short)?;
        let _sounds_off = r.u32();
        let name = r.fixed_str(64).ok_or_else(short)?;
        let wave_banks = (0..n_banks)
            .map(|i| {
                Reader::at(d, banks_off + 64 * i)
                    .fixed_str(64)
                    .ok_or_else(short)
            })
            .collect::<Result<Vec<_>>>()?;

        let mut cue_sounds: Vec<Vec<u32>> = Vec::new();
        let mut sr = Reader::at(d, simple_off);
        for _ in 0..n_simple {
            let _flags = sr.u8().ok_or_else(short)?;
            cue_sounds.push(vec![sr.u32().ok_or_else(short)?]);
        }
        let mut cr = Reader::at(d, complex_off);
        for _ in 0..n_complex {
            let flags = cr.u8().ok_or_else(short)?;
            let target = cr.u32().ok_or_else(short)?;
            cr.bytes(4 + 1 + 2 + 2 + 1).ok_or_else(short)?;
            if flags & 4 != 0 {
                cue_sounds.push(vec![target]);
            } else {
                if variations_off == u32::MAX {
                    return Err(bad("variation cue without a variation table".into()));
                }
                cue_sounds.push(variation_sounds(d, target as usize).ok_or_else(short)?);
            }
        }

        let mut cues = Vec::with_capacity(cue_sounds.len());
        for (i, sounds) in cue_sounds.into_iter().enumerate() {
            let name = if hash_vals_off != u32::MAX {
                let mut h = Reader::at(d, hash_vals_off as usize + 6 * i);
                let off = h.u32().ok_or_else(short)? as usize;
                Reader::at(d, off).cstr().ok_or_else(short)?
            } else {
                format!("cue{i}")
            };
            cues.push(Cue { name, sounds });
        }

        let mut offsets: Vec<u32> = cues.iter().flat_map(|c| c.sounds.iter().copied()).collect();
        offsets.sort_unstable();
        offsets.dedup();
        let sounds = offsets
            .into_iter()
            .map(|o| parse_sound(d, o).ok_or_else(|| bad(format!("bad sound at {o:#x}"))))
            .collect::<Result<Vec<_>>>()?;
        Ok(SoundBank {
            path: path.to_owned(),
            name,
            wave_banks,
            cues,
            sounds,
        })
    }

    pub fn cue(&self, name: &str) -> Option<&Cue> {
        self.cues.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn sound(&self, offset: u32) -> Option<&Sound> {
        self.sounds.iter().find(|s| s.offset == offset)
    }

    /// Every wave a cue can play.
    pub fn cue_waves(&self, name: &str) -> Vec<WaveRef> {
        let mut v: Vec<WaveRef> = self
            .cue(name)
            .into_iter()
            .flat_map(|c| c.sounds.iter())
            .filter_map(|o| self.sound(*o))
            .flat_map(|s| s.waves.iter().copied())
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

/// Sounds referenced by a variation table (types with sound offsets only;
/// wave-only tables yield nothing here).
fn variation_sounds(d: &[u8], off: usize) -> Option<Vec<u32>> {
    let mut r = Reader::at(d, off);
    let n = r.u16()? as usize;
    let flags = r.u16()?;
    r.u32()?;
    let ty = (flags >> 3) & 7;
    let mut v = Vec::new();
    for _ in 0..n {
        match ty {
            0 => {
                r.bytes(5)?;
            }
            1 => {
                v.push(r.u32()?);
                r.bytes(2)?;
            }
            3 => {
                v.push(r.u32()?);
                r.bytes(12)?;
            }
            4 => {
                r.bytes(3)?;
            }
            _ => return None,
        }
    }
    Some(v)
}

fn parse_sound(d: &[u8], off: u32) -> Option<Sound> {
    let mut r = Reader::at(d, off as usize);
    let flags = r.u8()?;
    let category = r.u16()?;
    let volume = r.u8()?;
    let pitch = r.i16()?;
    let priority = r.u8()?;
    let _entry_len = r.u16()?;
    let mut waves = Vec::new();
    let mut events = Vec::new();
    let complex = flags & 1 != 0;
    let mut n_clips = 0;
    if complex {
        n_clips = r.u8()? as usize;
    } else {
        let track = r.u16()?;
        let bank = r.u8()?;
        waves.push(WaveRef { bank, track });
    }
    if flags & 0x0e != 0 {
        let len = r.u16()? as usize;
        r.bytes(len.checked_sub(2)?)?;
    }
    if flags & 0x10 != 0 {
        let len = r.u16()? as usize;
        r.bytes(len.checked_sub(2)?)?;
    }
    for clip in 0..n_clips {
        let _vol = r.u8()?;
        let clip_off = r.u32()? as usize;
        r.bytes(4)?;
        let mut c = Reader::at(d, clip_off);
        let n_events = c.u8()?;
        for _ in 0..n_events {
            let info = c.u32()?;
            let _random_offset = c.u16()?;
            let ty = info & 0x1f;
            let timestamp_ms = (info >> 5) & 0xffff;
            match ty {
                // Play wave.
                1 => {
                    c.u8()?; // separator
                    let _flags = c.u8()?;
                    let track = c.u16()?;
                    let bank = c.u8()?;
                    let lc = c.u8()?;
                    c.bytes(4)?; // position, angle
                    waves.push(WaveRef { bank, track });
                    events.push(PlayEvent {
                        clip,
                        timestamp_ms,
                        loop_count: lc,
                        tracks: vec![WaveRef { bank, track }],
                    });
                }
                // Play wave from a track variation list.
                3 => {
                    c.u8()?;
                    let _flags = c.u8()?;
                    let lc = c.u8()?;
                    c.bytes(4)?;
                    let n = c.u16()? as usize;
                    c.bytes(2 + 4)?;
                    let mut tracks = Vec::new();
                    for _ in 0..n {
                        let track = c.u16()?;
                        let bank = c.u8()?;
                        c.bytes(2)?; // weight min/max
                        tracks.push(WaveRef { bank, track });
                    }
                    waves.extend(tracks.iter().copied());
                    events.push(PlayEvent {
                        clip,
                        timestamp_ms,
                        loop_count: lc,
                        tracks,
                    });
                }
                // Other event kinds (pitch/volume/marker…): sizes vary; the
                // music bank has none, so stop reading this clip.
                _ => break,
            }
        }
    }
    Some(Sound {
        offset: off,
        flags,
        category,
        volume,
        pitch,
        priority,
        waves,
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn music_cues_map_to_waves() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let dir = steam.content.music_dir();
        let sb = SoundBank::load(&dir.join("MusicSoundBank.xsb")).unwrap();
        let wb = crate::audio::xwb::WaveBank::open(&dir.join("MusicWaveBank.xwb")).unwrap();
        assert_eq!(sb.wave_banks, vec!["MusicWaveBank"]);
        assert_eq!(sb.cues.len(), 45);
        let mut used = std::collections::BTreeSet::new();
        for c in &sb.cues {
            let w = sb.cue_waves(&c.name);
            assert!(!w.is_empty(), "{}", c.name);
            for r in &w {
                assert!((r.track as usize) < wb.entries.len());
                used.insert(r.track);
            }
            if w.len() > 1 {
                eprintln!("{} plays {w:?}", c.name);
            }
        }
        assert_eq!(used.len(), 45, "every wave used");
        assert!(sb.cue("SacredGround").is_some());
        let team = sb.sound(sb.cue("VictoryTeam").unwrap().sounds[0]).unwrap();
        assert_eq!(team.events.len(), 2);
        assert_eq!(team.events[1].tracks.len(), 2);
        assert!(team.events[1].timestamp_ms > 0);
        assert!(sb.cue("Title").is_some() || sb.cue("AltTitle").is_some());
    }
}
