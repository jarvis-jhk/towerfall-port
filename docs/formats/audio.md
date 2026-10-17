# Audio files

Playback behaviour: `docs/spec/audio.md`. Layout notes:

## Sound effects (`SFX/*.wav`)

RIFF/WAVE. All 1539 files (Steam 594 + 232 Dark World, 8-player 713) are
16-bit PCM, 44 100 Hz, mono or stereo. Many carry editor chunks (`bext`,
`minf`, `elm1`, `regn`, `umid`, `DGDA`, `JUNK`), sometimes *before* `fmt `;
walk the chunk list (pad odd sizes) instead of assuming offsets. A few have a
`smpl` chunk; its first loop (start, inclusive end, in frames) is kept. The
reader also accepts 8-bit PCM, MS-ADPCM and 32-bit float.

## XACT music (`Content/Music/Win/`)

* `TowerFallMusic.xgs` — global settings, magic `XGSF`, version 46. Header
  counts (categories, variables, RPCs, DSP presets) and offsets to the
  category and variable tables and their NUL-separated name lists. Category:
  instance limit (u8), fade in/out ms (u16 ×2), instance behaviour (u8),
  parent (u16, `FFFF` = none), volume byte, visibility. Variable: flags, then
  initial/min/max as f32. Here: categories Global, Default, Music; 7 stock
  variables.
* `MusicWaveBank.xwb` — magic `WBND`, version 46, then five (offset, length)
  segments: bank data, entry metadata, seek tables, entry names, wave data.
  Bank data: flags, entry count, 64-byte name, metadata entry size, name
  size, alignment, compact-format word, build time. Each 24-byte entry:
  `flags | duration << 4` (duration in frames), packed format, play offset and
  length (bytes, relative to the wave-data segment), loop start and length
  (frames). Packed format bits, low to high: codec (2: PCM/XMA/ADPCM/WMA),
  channels (3), sample rate (18), block align (8), 16-bit flag (1); for ADPCM
  the block size in bytes is `(align + 22) × channels`.
  **All 45 entries are MS-ADPCM stereo, 524-byte blocks (512 frames each),
  rates 44 089–44 122 Hz (nominally 44.1 kHz), 81 minutes in total.** No xWMA.
  The odd rates are really in the metadata (18-bit rate field; codec,
  channel, block and duration fields all cross-check), e.g. entry 0 is
  44 101 Hz; ten entries are exactly 44 100. XACT/FAudio play each entry at its
  stored rate, so the mixer should resample from `format.sample_rate` to the
  device rate. The deviation is ≤ 0.05 % (≤ 0.9 cent, ≈ 0.1 s over a 4-minute
  track), so treating everything as 44 100 Hz would also be inaudible; loops
  are counted in frames, so either choice keeps them seamless.
* `MusicSoundBank.xsb` — magic `SDBK`, version 46. Header: counts of simple
  and complex cues, wave banks and sounds, offsets of the simple-cue,
  complex-cue, variation, wave-bank-name, cue-name-hash and cue-name-index
  tables and of the sounds, and the bank name. Simple cue = flags + sound
  offset (5 bytes); complex cue = flags + (sound or variation offset) +
  transition offset + limits/fades (15 bytes); flag 4 means "direct sound".
  The name index has one (name offset u32, next u16) pair per cue in cue order
  (simple first). A sound: flags, category, volume, pitch, priority, size,
  then either (track, bank) for a simple sound or a clip count; optional RPC
  (flags & 0x0E) and DSP (flags & 0x10) blocks with a leading u16 size; then
  per clip a volume, clip offset and filter words. A clip is a count of
  events; event word low 5 bits = type, next 16 = time in ms, then a u16
  random offset and a `FF` byte. Type 1 = play one wave (flags, track, bank,
  loop count, position, angle); type 3 = play one of a track list (flags,
  loop count, position, angle, count, variation flags, 4 unknown bytes, then
  track/bank/weight-min/weight-max per entry). Loop count 255 = forever.
  Here: 45 cues → 45 sounds → every wave once; level/menu cues are one
  looping wave; the 13 `Victory*` cues play their jingle (once) and then,
  6.4–13 s in (per cue), one of two looping tracks (7 or 43).

## MS-ADPCM

Standard Microsoft ADPCM: per block and channel a predictor index (0–6, into
the fixed table of seven coefficient pairs), initial delta, and the two
latest samples (output oldest first); then 4-bit codes, high nibble first,
alternating channels. Each code: predict `(s1·c1 + s2·c2) >> 8`, add
`code × delta` (signed code), clamp to 16 bits, then
`delta = max(16, adapt[code] × delta >> 8)`. The last block of an entry may be
short; output is trimmed to the entry's duration.

Decoding all music takes ≈ 2 s on one core (dev build), so no decoded cache
is written; decode a track (≈ 50 ms) when it's needed, or stream blocks.

## Catalogue and playback helpers (`audio::catalogue`, `audio::stream`)

* `SfxCatalogue::open(inst)`: the 470 named effects of the original (table
  generated from `docs/spec/audio.md` §10 by `scripts/dev/gen-sfx-table.py`),
  each with kind (`Plain`, `Instanced(n)`, `Varied(n)`, `Looped`,
  `Array(n)`), files (`_01…` suffixes resolved), `obeys_pitch` and whether it
  is Dark World content; `available(def)` checks the files exist (the 8-player
  build lacks the Dark World boss and editor sounds). PCM is decoded on first
  use and cached (`pcm(file)`, `effect_pcm(name)`; thread-safe).
* Voice sets: 14 (`VOICES`, indexed by the archers' `SFX`), each mapping the
  22 `VoiceAction`s to files: READY varied, DUCK/REVIVE plain with a varied
  fallback, WALLSLIDE/SLEEP looped, the rest plain; alternates 9–13 inherit
  what they lack from their base voice. Sets 8–13 come from
  `DarkWorldContent/SFX` only.
* Music: `MusicBank::stream_cue(cue)` gives a `MusicStream` (block-by-block
  ADPCM decode straight from the wave bank, looping when the cue's loop count
  is 255) — `read(&mut [i16])` fills interleaved stereo frames;
  `MusicBank::cue_events` lists timed play events (victory jingles chain a
  second, looping track).
