//! Streaming decode of a wave-bank entry: reads and decodes one ADPCM (or
//! PCM) block at a time from the file, optionally looping over the entry's
//! loop region (the whole track for TowerFall's music), so the game only has
//! to pull interleaved 16-bit frames into its audio callback.

use super::adpcm;
use super::xwb::{Codec, WaveBank, WaveEntry};
use crate::error::{Error, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

pub struct MusicStream {
    path: PathBuf,
    file: File,
    entry: WaveEntry,
    data_start: u64,
    looping: bool,
    block_bytes: usize,
    frames_per_block: usize,
    /// Next frame to emit.
    frame: usize,
    /// Decoded frames of the current block and the block's first frame.
    buf: Vec<i16>,
    buf_start: usize,
    raw: Vec<u8>,
}

impl MusicStream {
    pub fn open(bank: &WaveBank, index: usize, looping: bool) -> Result<MusicStream> {
        let entry = bank
            .entries
            .get(index)
            .cloned()
            .ok_or_else(|| Error::format(&bank.path, format!("no wave {index}")))?;
        let fm = &entry.format;
        let ch = fm.channels.max(1) as usize;
        let (block_bytes, frames_per_block) = match fm.codec {
            Codec::Adpcm => (
                fm.block_align as usize,
                adpcm::samples_per_block(fm.block_align as usize, ch),
            ),
            Codec::Pcm => {
                let bpf = ch * if fm.bits == 16 { 2 } else { 1 };
                (bpf * 4096, 4096)
            }
            c => {
                return Err(Error::format(
                    &bank.path,
                    format!("wave {index}: codec {c:?} not supported"),
                ));
            }
        };
        let file = File::open(&bank.path).map_err(|e| Error::io(&bank.path, e))?;
        Ok(MusicStream {
            path: bank.path.clone(),
            file,
            data_start: bank.entry_file_offset(index).unwrap_or(0),
            entry,
            looping,
            block_bytes,
            frames_per_block,
            frame: 0,
            buf: Vec::new(),
            buf_start: usize::MAX,
            raw: Vec::new(),
        })
    }

    pub fn channels(&self) -> u16 {
        self.entry.format.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.entry.format.sample_rate
    }

    /// Length of the track in frames.
    pub fn frames(&self) -> usize {
        if self.entry.duration > 0 {
            self.entry.duration as usize
        } else {
            (self.entry.length as usize / self.block_bytes) * self.frames_per_block
        }
    }

    /// Current position in frames.
    pub fn position(&self) -> usize {
        self.frame
    }

    pub fn seek(&mut self, frame: usize) {
        self.frame = frame.min(self.frames());
    }

    /// Loop region (start, end) in frames: the entry's loop region if it has
    /// one, else the whole track.
    fn loop_region(&self) -> (usize, usize) {
        let total = self.frames();
        if self.entry.loop_length > 0 {
            let s = self.entry.loop_start as usize;
            (
                s.min(total),
                (s + self.entry.loop_length as usize).min(total),
            )
        } else {
            (0, total)
        }
    }

    fn load_block(&mut self, block: usize) -> Result<()> {
        let off = block * self.block_bytes;
        let len = self
            .block_bytes
            .min((self.entry.length as usize).saturating_sub(off));
        self.raw.resize(len, 0);
        let io = |e| Error::io(&self.path, e);
        self.file
            .seek(SeekFrom::Start(self.data_start + off as u64))
            .map_err(io)?;
        self.file
            .read_exact(&mut self.raw)
            .map_err(|e| Error::io(&self.path, e))?;
        self.buf.clear();
        let fm = &self.entry.format;
        match fm.codec {
            Codec::Adpcm => {
                adpcm::decode_block(&self.raw, fm.channels as usize, &mut self.buf);
            }
            _ if fm.bits == 16 => self.buf.extend(
                self.raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_le_bytes(*b)),
            ),
            _ => self
                .buf
                .extend(self.raw.iter().map(|&b| ((b as i16) - 128) << 8)),
        }
        self.buf_start = block * self.frames_per_block;
        Ok(())
    }

    /// Fills `out` with interleaved samples; returns how many samples were
    /// written (less than `out.len()` only at the end of a non-looping track).
    pub fn read(&mut self, out: &mut [i16]) -> Result<usize> {
        let ch = self.entry.format.channels.max(1) as usize;
        let (loop_start, loop_end) = self.loop_region();
        let end = if self.looping {
            loop_end
        } else {
            self.frames()
        };
        let mut written = 0;
        while written + ch <= out.len() {
            if self.frame >= end {
                if self.looping && loop_end > loop_start {
                    self.frame = loop_start;
                } else {
                    break;
                }
            }
            let block = self.frame / self.frames_per_block;
            if self.buf_start != block * self.frames_per_block {
                self.load_block(block)?;
            }
            let i = (self.frame - self.buf_start) * ch;
            if i + ch > self.buf.len() {
                break;
            }
            out[written..written + ch].copy_from_slice(&self.buf[i..i + ch]);
            written += ch;
            self.frame += 1;
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use crate::audio::MusicBank;
    use crate::testutil::{installs, require};

    #[test]
    fn stream_matches_full_decode_and_loops() {
        let steam = require!(installs().steam.as_ref(), "Steam install");
        let m = MusicBank::open(&steam.content).unwrap();
        let wb = &m.wave_bank;
        let (i, e) = wb
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.length)
            .unwrap();
        let full = wb.decode(i).unwrap();
        let mut s = super::MusicStream::open(wb, i, true).unwrap();
        let mut got = vec![0i16; full.samples.len() + 1000];
        let mut n = 0;
        while n < got.len() {
            let len = got.len();
            let k = s.read(&mut got[n..(n + 777).min(len)]).unwrap();
            assert!(k > 0);
            n += k;
        }
        assert_eq!(&got[..full.samples.len()], &full.samples[..]);
        // After the end it wraps to the loop start (frame 0 here).
        assert_eq!(e.loop_start, 0);
        assert_eq!(&got[full.samples.len()..], &full.samples[..1000]);
        let mut once = super::MusicStream::open(wb, i, false).unwrap();
        let mut buf = vec![0i16; full.samples.len() + 64];
        let mut total = 0;
        loop {
            let k = once.read(&mut buf[total..]).unwrap();
            total += k;
            if k == 0 || total == buf.len() {
                break;
            }
        }
        assert_eq!(total, full.samples.len());
        let cue = m.stream_cue("SacredGround").unwrap();
        assert_eq!(cue.channels(), 2);
    }
}
