//! Minimal WAV I/O: a 16-bit stereo writer (headless audio output) and a
//! PCM reader (the original's SFX are plain RIFF WAVs).

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::mixer::SoundData;

pub struct WavWriter {
    out: BufWriter<File>,
    frames: u32,
    sample_rate: u32,
}

impl WavWriter {
    pub fn create(path: impl AsRef<Path>, sample_rate: u32) -> Result<Self> {
        let path = path.as_ref();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut w = Self {
            out: BufWriter::new(File::create(path)?),
            frames: 0,
            sample_rate,
        };
        w.write_header()?;
        Ok(w)
    }

    fn write_header(&mut self) -> Result<()> {
        let data_len = self.frames * 4;
        let rate = self.sample_rate;
        let o = &mut self.out;
        o.write_all(b"RIFF")?;
        o.write_all(&(36 + data_len).to_le_bytes())?;
        o.write_all(b"WAVEfmt ")?;
        o.write_all(&16u32.to_le_bytes())?;
        o.write_all(&1u16.to_le_bytes())?; // PCM
        o.write_all(&2u16.to_le_bytes())?; // channels
        o.write_all(&rate.to_le_bytes())?;
        o.write_all(&(rate * 4).to_le_bytes())?; // byte rate
        o.write_all(&4u16.to_le_bytes())?; // block align
        o.write_all(&16u16.to_le_bytes())?; // bits
        o.write_all(b"data")?;
        o.write_all(&data_len.to_le_bytes())?;
        Ok(())
    }

    /// Appends interleaved stereo samples in -1..1.
    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            self.out.write_all(&v.to_le_bytes())?;
        }
        self.frames += (samples.len() / 2) as u32;
        Ok(())
    }

    /// Patches the header sizes and flushes.
    pub fn finish(mut self) -> Result<()> {
        self.out.seek(SeekFrom::Start(0))?;
        self.write_header()?;
        self.out.flush()?;
        Ok(())
    }
}

/// Decodes a PCM WAV (8/16/24/32-bit integer or 32-bit float, mono or
/// stereo; more channels keep the first two).
pub fn decode_wav(bytes: &[u8]) -> Result<SoundData> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("not a RIFF WAVE file");
    }
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut fmt = None;
    let mut data = None;
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32_at(bytes, i + 4) as usize;
        let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                let tag = u16_at(body, 0);
                // WAVE_FORMAT_EXTENSIBLE: the real tag starts the subformat GUID.
                let tag = if tag == 0xFFFE && body.len() >= 26 {
                    u16_at(body, 24)
                } else {
                    tag
                };
                fmt = Some((tag, u16_at(body, 2), u32_at(body, 4), u16_at(body, 14)))
            }
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + len + (len & 1);
    }
    let (format, channels, sample_rate, bits) = fmt.context("WAV without fmt chunk")?;
    let data = data.context("WAV without data chunk")?;
    if channels == 0 {
        bail!("WAV with 0 channels");
    }
    let float = format == 3;
    if !matches!(format, 1 | 3) {
        bail!("unsupported WAV format tag {format:#x} (only PCM/float)");
    }
    let bps = (bits as usize).div_ceil(8);
    let frame_len = bps * channels as usize;
    let out_ch = channels.min(2);
    let mut samples = Vec::with_capacity(data.len() / bps.max(1));
    for frame in data.chunks_exact(frame_len) {
        for c in 0..out_ch as usize {
            let s = &frame[c * bps..(c + 1) * bps];
            let v = match (bps, float) {
                (1, _) => (s[0] as f32 - 128.0) / 128.0,
                (2, _) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                (3, _) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
                (4, true) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                (4, false) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                _ => bail!("unsupported WAV bit depth {bits}"),
            };
            samples.push(v);
        }
    }
    Ok(SoundData {
        sample_rate,
        channels: out_ch,
        samples,
    })
}

pub fn load_wav(path: impl AsRef<Path>) -> Result<SoundData> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    decode_wav(&bytes).with_context(|| format!("decoding {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read() {
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp"));
        let path = dir.join("roundtrip.wav");
        let mut w = WavWriter::create(&path, 22050).unwrap();
        w.write(&[0.5, -0.5, 1.0, 0.0]).unwrap();
        w.finish().unwrap();
        let s = load_wav(&path).unwrap();
        assert_eq!((s.sample_rate, s.channels, s.frames()), (22050, 2, 2));
        assert!((s.samples[0] - 0.5).abs() < 1e-3 && (s.samples[1] + 0.5).abs() < 1e-3);
        assert!(decode_wav(b"RIFF....WAVX").is_err());
    }
}
