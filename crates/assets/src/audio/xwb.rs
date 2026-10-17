//! XACT wave banks (`*.xwb`, XACT 3 / content version 46). The header points
//! at five segments: bank data, entry metadata, seek tables, entry names and
//! the wave data. Each entry has a packed "mini wave format" (codec,
//! channels, rate, block align, bit depth), a play region in the wave data
//! segment and an optional loop region. TowerFall's music bank is MS-ADPCM.

use super::{Pcm, Reader, adpcm};
use crate::error::{Error, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const FLAG_ENTRY_NAMES: u32 = 0x0001_0000;
const FLAG_COMPACT: u32 = 0x0002_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Pcm,
    Xma,
    Adpcm,
    Wma,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaveFormat {
    pub codec: Codec,
    pub channels: u16,
    pub sample_rate: u32,
    /// Actual bytes per block (for ADPCM: `(packed + 22) × channels`).
    pub block_align: u32,
    pub bits: u16,
}

impl WaveFormat {
    /// Unpacks the 32-bit mini format: tag 2 bits, channels 3, rate 18,
    /// block align 8, bits-per-sample flag 1.
    pub fn from_packed(v: u32) -> WaveFormat {
        let codec = match v & 3 {
            0 => Codec::Pcm,
            1 => Codec::Xma,
            2 => Codec::Adpcm,
            _ => Codec::Wma,
        };
        let channels = ((v >> 2) & 7) as u16;
        let sample_rate = (v >> 5) & 0x3_ffff;
        let packed_align = (v >> 23) & 0xff;
        let bits = if v >> 31 != 0 { 16 } else { 8 };
        let block_align = match codec {
            Codec::Adpcm => (packed_align + 22) * channels as u32,
            _ => packed_align,
        };
        WaveFormat {
            codec,
            channels,
            sample_rate,
            block_align,
            bits,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaveEntry {
    pub name: Option<String>,
    pub flags: u32,
    /// Length in frames (from the entry header).
    pub duration: u32,
    pub format: WaveFormat,
    /// Byte offset/length inside the wave-data segment.
    pub offset: u32,
    pub length: u32,
    /// Loop start/length in frames (length 0 = no loop region).
    pub loop_start: u32,
    pub loop_length: u32,
}

#[derive(Clone, Debug)]
pub struct WaveBank {
    pub path: PathBuf,
    pub version: u32,
    pub name: String,
    pub flags: u32,
    pub entries: Vec<WaveEntry>,
    data_offset: u64,
}

impl WaveBank {
    /// Reads the header and entry table (the wave data stays on disk).
    pub fn open(path: &Path) -> Result<WaveBank> {
        let mut f = File::open(path).map_err(|e| Error::io(path, e))?;
        let mut head = vec![0u8; 0x34];
        f.read_exact(&mut head).map_err(|e| Error::io(path, e))?;
        let bad = |m: &str| Error::format(path, m.to_owned());
        if &head[0..4] != b"WBND" {
            return Err(bad("not a wave bank (no WBND)"));
        }
        let mut r = Reader::new(&head);
        r.pos = 4;
        let version = r.u32().unwrap();
        if version != 46 && version != 45 && version != 44 {
            return Err(bad(&format!("unsupported wave bank version {version}")));
        }
        let _header_version = r.u32().unwrap();
        let mut seg = [(0u32, 0u32); 5];
        for s in &mut seg {
            *s = (r.u32().unwrap(), r.u32().unwrap());
        }
        let read_seg = |f: &mut File, (off, len): (u32, u32)| -> Result<Vec<u8>> {
            let mut v = vec![0u8; len as usize];
            f.seek(SeekFrom::Start(off as u64))
                .map_err(|e| Error::io(path, e))?;
            f.read_exact(&mut v).map_err(|e| Error::io(path, e))?;
            Ok(v)
        };
        let bank = read_seg(&mut f, seg[0])?;
        let mut b = Reader::new(&bank);
        let flags = b.u32().ok_or_else(|| bad("short bank data"))?;
        let count = b.u32().ok_or_else(|| bad("short bank data"))? as usize;
        let name = b.fixed_str(64).ok_or_else(|| bad("short bank data"))?;
        let meta_size = b.u32().ok_or_else(|| bad("short bank data"))? as usize;
        let name_size = b.u32().unwrap_or(64) as usize;
        let alignment = b.u32().unwrap_or(1);
        let compact_format = b.u32().unwrap_or(0);
        let meta = read_seg(&mut f, seg[1])?;
        let names = if flags & FLAG_ENTRY_NAMES != 0 && seg[3].1 > 0 {
            Some(read_seg(&mut f, seg[3])?)
        } else {
            None
        };
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let name = names
                .as_ref()
                .and_then(|n| Reader::at(n, i * name_size).fixed_str(name_size));
            if flags & FLAG_COMPACT != 0 {
                let mut m = Reader::at(&meta, i * 4);
                let v = m.u32().ok_or_else(|| bad("short metadata"))?;
                let offset = (v & 0x1f_ffff) * alignment;
                let next = if i + 1 < count {
                    (Reader::at(&meta, (i + 1) * 4).u32().unwrap_or(0) & 0x1f_ffff) * alignment
                } else {
                    seg[4].1
                };
                entries.push(WaveEntry {
                    name,
                    flags: 0,
                    duration: 0,
                    format: WaveFormat::from_packed(compact_format),
                    offset,
                    length: next.saturating_sub(offset),
                    loop_start: 0,
                    loop_length: 0,
                });
                continue;
            }
            let mut m = Reader::at(&meta, i * meta_size);
            let fd = m.u32().ok_or_else(|| bad("short metadata"))?;
            let fmt = m.u32().ok_or_else(|| bad("short metadata"))?;
            let offset = m.u32().ok_or_else(|| bad("short metadata"))?;
            let length = m.u32().ok_or_else(|| bad("short metadata"))?;
            let loop_start = m.u32().unwrap_or(0);
            let loop_length = m.u32().unwrap_or(0);
            entries.push(WaveEntry {
                name,
                flags: fd & 0xf,
                duration: fd >> 4,
                format: WaveFormat::from_packed(fmt),
                offset,
                length,
                loop_start,
                loop_length,
            });
        }
        Ok(WaveBank {
            path: path.to_owned(),
            version,
            name,
            flags,
            entries,
            data_offset: seg[4].0 as u64,
        })
    }

    /// Absolute file offset of entry `i`'s data.
    pub fn entry_file_offset(&self, i: usize) -> Option<u64> {
        self.entries
            .get(i)
            .map(|e| self.data_offset + e.offset as u64)
    }

    /// The raw encoded bytes of entry `i`.
    pub fn raw(&self, i: usize) -> Result<Vec<u8>> {
        let e = self
            .entries
            .get(i)
            .ok_or_else(|| Error::format(&self.path, format!("no wave {i}")))?;
        let mut f = File::open(&self.path).map_err(|e| Error::io(&self.path, e))?;
        f.seek(SeekFrom::Start(self.data_offset + e.offset as u64))
            .map_err(|e| Error::io(&self.path, e))?;
        let mut v = vec![0u8; e.length as usize];
        f.read_exact(&mut v).map_err(|e| Error::io(&self.path, e))?;
        Ok(v)
    }

    /// Decodes entry `i` to 16-bit PCM (PCM and MS-ADPCM entries; xWMA and
    /// XMA are reported as unsupported).
    pub fn decode(&self, i: usize) -> Result<Pcm> {
        let e = &self.entries[i];
        let raw = self.raw(i)?;
        let fm = &e.format;
        let frames = (e.duration > 0).then_some(e.duration as usize);
        let samples = match fm.codec {
            Codec::Adpcm => {
                adpcm::decode(&raw, fm.block_align as usize, fm.channels as usize, frames)
            }
            Codec::Pcm if fm.bits == 16 => raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect(),
            Codec::Pcm => raw.iter().map(|&b| ((b as i16) - 128) << 8).collect(),
            c => {
                return Err(Error::format(
                    &self.path,
                    format!("wave {i}: codec {c:?} not supported"),
                ));
            }
        };
        Ok(Pcm {
            channels: fm.channels,
            sample_rate: fm.sample_rate,
            samples,
            loop_region: (e.loop_length > 0).then_some((e.loop_start, e.loop_length)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{installs, require};

    #[test]
    fn packed_format() {
        let f = WaveFormat::from_packed(0x7815_888a);
        assert_eq!(f.codec, Codec::Adpcm);
        assert_eq!(f.channels, 2);
        assert_eq!(f.sample_rate, 44100);
        assert_eq!(f.block_align, 524);
    }

    #[test]
    fn music_bank_entries() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let wb = WaveBank::open(&steam.content.music_dir().join("MusicWaveBank.xwb")).unwrap();
        assert_eq!(wb.name, "MusicWaveBank");
        assert_eq!(wb.entries.len(), 45);
        for e in &wb.entries {
            assert_eq!(e.format.codec, Codec::Adpcm);
            let per =
                adpcm::samples_per_block(e.format.block_align as usize, e.format.channels as usize);
            let blocks = (e.length as usize).div_ceil(e.format.block_align as usize);
            assert!(blocks * per >= e.duration as usize, "{e:?}");
        }
        // Decode the shortest entry fully and check it isn't silent.
        let (i, _) = wb
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.length)
            .unwrap();
        let pcm = wb.decode(i).unwrap();
        assert_eq!(pcm.frames(), wb.entries[i].duration as usize);
        let peak = pcm.samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak > 1000, "peak {peak}");
    }
}
