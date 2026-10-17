# Atlases

`Content/Atlas/{atlas,menuAtlas,bgAtlas,bossAtlas}.{xml,png}` and
`DarkWorldContent/Atlas/{atlas,bossAtlas,editorAtlas}.{xml,png}`.

TexturePacker "Sparrow"-style XML:

```xml
<TextureAtlas imagePath="atlas.png">
    <SubTexture name="tilesets/sacredGround" x="…" y="…" width="40" height="110"/>
```

* Names are slash paths (`player/hat/green/normal`); unique per file.
* Only `name x y width height` are used: no trimming/rotation attributes exist.
* The image is the PNG with the same stem (the `imagePath` attribute is not
  consulted). RGBA, straight alpha.
* With Dark World present the game loads the Dark World `atlas` and
  `bossAtlas` instead of the base ones; the Dark World `atlas` contains every
  base subtexture (checked by a test) plus ~95 more. `menuAtlas` and `bgAtlas`
  only exist in `Content/`.

Counts (Steam): atlas 576 / DW atlas 671, menu 700, bg 85, boss 7 / DW boss
81, editor 174. The 8-player build's menu atlas has 710 and its bg atlas 76
entries (a few bg sprites named in its sprite data are missing from it).
