# Sprite data

`Content/Atlas/SpriteData/{spriteData,corpseSpriteData,menuSpriteData,bgSpriteData,bossSpriteData}.xml`
(no Dark World copies). Which atlas a file refers to: sprite and corpse data →
main atlas, menu → menu atlas, bg → bg atlas, boss → boss atlas.

Root `<SpriteData>`; each child element is one sprite keyed by `id`:

* element name = kind: `sprite_string` (animations keyed by name),
  `sprite_int` (keyed by integer), `image` (one picture).
* `<Texture>`: atlas subtexture name (required).
* `<FrameWidth>`, `<FrameHeight>`: frame cell size. Frames are cut from the
  subtexture left→right then top→bottom; only whole cells count
  (`floor(w/fw) × floor(h/fh)`), no spacing.
* `<OriginX>`, `<OriginY>` (default 0), `<X>`, `<Y>` (offset, default 0),
  `<Color>` (hex RGB tint, default white).
* `<Animations><Anim id=".." delay=".." loop=".." frames="0,1,2"/>`: `delay`
  in seconds per frame (missing/0 = static), `loop` defaults to true, `frames`
  index the frame cells.
* Any other single-text children are alternative textures, e.g. corpses'
  `BlueTeam`/`RedTeam`/`Flash`, player parts' `RedTexture`/`BlueTexture`;
  the model keeps them in `extra`.

Totals: 325 sprites, 1581 animations. Data quirks (left alone): five
`PlayerHeadWarlord*` and `CataclysmBodyCrystals` name textures that aren't in
any atlas (never used); `Orange_AltHead`'s team textures are missing; menu
`DarkWorldTower` animates frames 0–9 though its sheet holds 9 cells.
