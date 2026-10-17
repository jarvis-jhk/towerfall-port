//! RIFF/WAVE files (`SFX/*.wav`). The game's effects are all 16-bit PCM at
//! 44.1 kHz, mono or stereo, with assorted metadata chunks (`bext`, `minf`,
//! `elm1`, `regn`, `umid`, `DGDA`, `smpl`, `JUNK`) that are skipped except
//! `smpl` loop points. 8-bit PCM and MS-ADPCM are decoded too.

use super::{Pcm, Reader, adpcm};
use crate::error::{Error, Result, read_file};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WavFormat {
    pub tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub block_align: u16,
    pub bits: u16,
}

pub struct Wav;

/// Format, data chunk and `smpl` loop (start, length in frames).
pub type WavChunks<'a> = (WavFormat, &'a [u8], Option<(u32, u32)>);

impl Wav {
    pub fn load(path: &Path) -> Result<Pcm> {
        let bytes = read_file(path)?;
        Wav::decode(&bytes).map_err(|m| Error::format(path, m))
    }

    pub fn decode(bytes: &[u8]) -> std::result::Result<Pcm, String> {
        let (fmt, data, lp) = Wav::chunks(bytes)?;
        let samples = match (fmt.tag, fmt.bits) {
            (1, 16) => data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect(),
            (1, 8) => data.iter().map(|&b| ((b as i16) - 128) << 8).collect(),
            (2, _) => adpcm::decode(data, fmt.block_align as usize, fmt.channels as usize, None),
            (3, 32) => data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| (f32::from_le_bytes(*b).clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect(),
            (t, b) => return Err(format!("unsupported WAVE format tag {t} / {b} bits")),
        };
        Ok(Pcm {
            channels: fmt.channels,
            sample_rate: fmt.sample_rate,
            samples,
            loop_region: lp,
        })
    }

    /// Format, data chunk and `smpl` loop (start, length in frames).
    pub fn chunks(bytes: &[u8]) -> std::result::Result<WavChunks<'_>, String> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("not a RIFF/WAVE file".into());
        }
        let mut r = Reader::at(bytes, 12);
        let mut fmt = None;
        let mut data = None;
        let mut lp = None;
        while let Some(id) = r.bytes(4) {
            let Some(len) = r.u32() else { break };
            let start = r.pos;
            let body = &bytes[start..(start + len as usize).min(bytes.len())];
            match id {
                b"fmt " => {
                    let mut f = Reader::new(body);
                    let tag = f.u16().ok_or("short fmt")?;
                    let channels = f.u16().ok_or("short fmt")?;
                    let sample_rate = f.u32().ok_or("short fmt")?;
                    let _byte_rate = f.u32();
                    let block_align = f.u16().ok_or("short fmt")?;
                    let bits = f.u16().unwrap_or(0);
                    fmt = Some(WavFormat {
                        tag,
                        channels,
                        sample_rate,
                        block_align,
                        bits,
                    });
                }
                b"data" => data = Some(body),
                b"smpl" => {
                    let mut s = Reader::new(body);
                    s.pos = 28;
                    if let Some(n) = s.u32()
                        && n > 0
                    {
                        s.pos = 36 + 8; // first loop: id, type, then start, end
                        if let (Some(a), Some(b)) = (s.u32(), s.u32()) {
                            lp = Some((a, b.saturating_sub(a) + 1));
                        }
                    }
                }
                _ => {}
            }
            r.pos = start + len as usize + (len as usize & 1);
        }
        Ok((fmt.ok_or("no fmt chunk")?, data.ok_or("no data chunk")?, lp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::installs;

    #[test]
    fn roundtrip() {
        let p = Pcm {
            channels: 2,
            sample_rate: 22050,
            samples: vec![1, -2, 300, -32768],
            loop_region: None,
        };
        assert_eq!(Wav::decode(&p.to_wav_bytes()).unwrap(), p);
    }

    #[test]
    fn every_sfx_decodes() {
        let mut n = 0;
        let mut secs = 0.0;
        for inst in installs().iter() {
            for set in inst.content_sets() {
                let Ok(rd) = std::fs::read_dir(set.sfx_dir()) else {
                    continue;
                };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")) {
                        let pcm = Wav::load(&p).unwrap_or_else(|e| panic!("{e}"));
                        assert_eq!(pcm.sample_rate, 44100, "{p:?}");
                        assert!(pcm.frames() > 0, "{p:?}");
                        secs += pcm.seconds();
                        n += 1;
                    }
                }
            }
        }
        eprintln!("{n} SFX, {secs:.0} s total");
    }
}
