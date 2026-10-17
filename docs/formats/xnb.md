# XNB (`Content/Misc/*.xnb`)

XNA 4.0 content: `XNB`, platform `w`, version 5, flags byte (`0x80` LZX,
`0x40` LZ4 compression, `0x01` HiDef), u32 file size. **None of the shipped
files are compressed** (flags 0x00, ScreenEffects 0x01 = HiDef only), so no
LZX decoder exists; compressed files are rejected. Then a 7-bit-encoded count
of type readers (each a length-prefixed assembly-qualified name and an i32
version), a 7-bit shared-resource count (0), and the primary object, which
starts with its 1-based type-reader index.

## Sprite fonts (`Archer`, `OpenSans`, `MonocleDefault`)

`SpriteFontReader` object: a `Texture2D` (reader index, surface format i32,
width, height, mip count, then per mip a size and the bytes), `List<Rectangle>`
glyph source rects, `List<Rectangle>` cropping (pen offset and cell size),
`List<char>` characters (UTF-8 encoded), line spacing (i32), spacing (f32),
`List<Vector3>` per-glyph (left bearing, width, right bearing), and a nullable
default character (bool + char). Each list is preceded by its reader index
and an i32 count.

All three textures are **DXT3** (surface format 5): 4×4 blocks of 16 bytes,
64 bits of explicit 4-bit alpha then a BC1 colour block (two RGB565 end
points, 2-bit indices, always 4-colour mode). Content-pipeline font textures
are premultiplied (white glyphs, so colour = alpha). Archer: 64×64, 95 glyphs
(ASCII 32–126), line spacing 5; OpenSans and MonocleDefault: 128×128,
95 glyphs, line spacing 22.

## Effects (`LightingEffect`, `ScreenEffects`)

`EffectReader`: u32 length + a compiled D3D9 effect (magic `0xBCF00BCF`,
`fx_2_0`) containing one technique/pass with a `ps_2_0` pixel shader
(HLSL compiler 9.26.952.2844). LightingEffect (792-byte blob): samplers
`SourceSampler`, `LightSampler`, parameter `Alpha`. ScreenEffects
(1028 bytes): `TextureSampler`, `Counter`, `Amplitude`, `Frequency`. We only
report these (`xnb::report`); the shaders are rewritten in WGSL from the
rendering spec.
