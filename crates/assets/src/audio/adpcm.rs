//! Microsoft ADPCM (WAVE format tag 2) decoding, as used by the XACT music
//! wave bank. Each block starts with a per-channel header (predictor index,
//! initial delta, the two most recent samples), followed by 4-bit codes,
//! high nibble first, channels interleaved per nibble.

/// The seven standard predictor coefficient pairs.
const COEFS: [(i32, i32); 7] = [
    (256, 0),
    (512, -256),
    (0, 0),
    (192, 64),
    (240, 0),
    (460, -208),
    (392, -232),
];

/// Step-size adaptation per code.
const ADAPT: [i32; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];

/// Frames a full block of `block_align` bytes holds.
pub fn samples_per_block(block_align: usize, channels: usize) -> usize {
    (block_align - 7 * channels) * 2 / channels + 2
}

#[derive(Clone, Copy)]
struct Chan {
    c1: i32,
    c2: i32,
    delta: i32,
    s1: i32,
    s2: i32,
}

impl Chan {
    #[inline]
    fn step(&mut self, code: u8) -> i16 {
        let signed = if code & 8 != 0 {
            code as i32 - 16
        } else {
            code as i32
        };
        let predicted = (self.s1 * self.c1 + self.s2 * self.c2) >> 8;
        let s = (predicted as i64 + signed as i64 * self.delta as i64).clamp(-32768, 32767) as i32;
        self.s2 = self.s1;
        self.s1 = s;
        // Real streams keep the step within i16; clamp so hostile data can't overflow.
        self.delta =
            ((ADAPT[code as usize] as i64 * self.delta as i64) >> 8).clamp(16, 1 << 24) as i32;
        s as i16
    }
}

/// Decodes one block (possibly truncated) and appends interleaved samples.
/// Returns `false` if the block header is incomplete or malformed.
pub fn decode_block(block: &[u8], channels: usize, out: &mut Vec<i16>) -> bool {
    let ch = channels;
    if ch == 0 || ch > 2 || block.len() < 7 * ch {
        return false;
    }
    let mut st = [Chan {
        c1: 0,
        c2: 0,
        delta: 0,
        s1: 0,
        s2: 0,
    }; 2];
    let rd16 = |i: usize| i16::from_le_bytes([block[i], block[i + 1]]) as i32;
    for c in 0..ch {
        let p = block[c] as usize;
        if p >= COEFS.len() {
            return false;
        }
        st[c].c1 = COEFS[p].0;
        st[c].c2 = COEFS[p].1;
        st[c].delta = rd16(ch + 2 * c);
        st[c].s1 = rd16(3 * ch + 2 * c);
        st[c].s2 = rd16(5 * ch + 2 * c);
    }
    // The header's two samples come out oldest first.
    for s in st.iter().take(ch) {
        out.push(s.s2 as i16);
    }
    for s in st.iter().take(ch) {
        out.push(s.s1 as i16);
    }
    let mut c = 0;
    for &b in &block[7 * ch..] {
        out.push(st[c].step(b >> 4));
        c = (c + 1) % ch;
        out.push(st[c].step(b & 0x0f));
        c = (c + 1) % ch;
    }
    true
}

/// Decodes a whole stream of `block_align`-sized blocks; the last one may be
/// short. Output is trimmed to `max_frames` if given.
pub fn decode(
    data: &[u8],
    block_align: usize,
    channels: usize,
    max_frames: Option<usize>,
) -> Vec<i16> {
    let per = samples_per_block(block_align, channels);
    let blocks = data.len().div_ceil(block_align);
    let mut out = Vec::with_capacity(blocks * per * channels);
    for block in data.chunks(block_align) {
        if !decode_block(block, channels, &mut out) {
            break;
        }
    }
    if let Some(m) = max_frames {
        out.truncate(m * channels);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_math_and_silence() {
        assert_eq!(samples_per_block(524, 2), 512);
        assert_eq!(samples_per_block(256, 1), 500);
        // A silent mono block: predictor 0, delta 16, samples 0, codes 0.
        let mut b = vec![0u8, 16, 0, 0, 0, 0, 0];
        b.extend([0u8; 10]);
        let out = decode(&b, b.len(), 1, None);
        assert_eq!(out.len(), 22);
        assert!(out.iter().all(|&s| s == 0));
    }

    #[test]
    fn codes_move_the_signal() {
        // Code 7 (max positive) repeatedly must rise; code 9 (-7) must fall.
        let mut up = vec![0u8, 16, 0, 0, 0, 0, 0];
        up.extend([0x77u8; 8]);
        let o = decode(&up, up.len(), 1, None);
        assert!(o.windows(2).skip(2).all(|w| w[1] >= w[0]));
        assert!(*o.last().unwrap() > 1000);
    }
}
