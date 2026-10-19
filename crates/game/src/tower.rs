//! Drawing a level from `tfp_assets::look::LevelLook` (rendering.md 5-7):
//! background elements on the screen, the BG tile layer, decorations, solid
//! tiles and foreground elements in the foreground canvas, and the
//! subtractive lighting composite with the light canvas. Animated: scrolling
//! and wavy layers, sprite animations, pulsing lights.
//!
//! [`TowerViewer`] is the `--tower NAME --level N` scene.

use std::path::Path;

use anyhow::{Context, Result, bail};
use tfp_assets::atlas::{AtlasKind, LoadedAtlas};
use tfp_assets::look::{
    BgElement, Decoration, ImageRef, LevelLook, LookAssets, LookOptions, Part, SpriteSet, TileLayer,
};
use tfp_render::glam::Vec2;
use tfp_render::{
    Blend, Color, DrawList, Gpu, Image, LightParams, Rect, Renderer, SpriteParams, SubTexture,
    TextureId,
};

use crate::app::{DrawContext, LoadContext, Scene, TickContext};

/// GPU-side assets for drawing looks of one install.
pub struct TowerGfx {
    pub la: LookAssets,
    pub main: TextureId,
    pub bg: TextureId,
    pub install: tfp_assets::GameInstall,
    pub index: tfp_assets::towers::TowerIndex,
    /// `main` was uploaded here (not shared with the UI).
    owns_main: bool,
    /// The boss atlas (King Reaper art), uploaded by [`TowerGfx::load_boss`].
    pub boss: Option<(TextureId, tfp_assets::atlas::Atlas)>,
}

fn upload(gpu: &Gpu, r: &mut Renderer, path: &Path) -> Result<TextureId> {
    let a = LoadedAtlas::load(path)?;
    let mut img = Image::from_pixels(a.image.width, a.image.height, a.image.pixels.clone());
    img.fna_clear_transparent();
    Ok(r.create_texture(gpu, &img))
}

impl TowerGfx {
    pub fn load(gpu: &Gpu, r: &mut Renderer, install: &tfp_assets::GameInstall) -> Result<Self> {
        Self::load_with_main(gpu, r, install, None)
    }

    /// Like [`load`](Self::load), reusing an already uploaded main atlas.
    pub fn load_with_main(
        gpu: &Gpu,
        r: &mut Renderer,
        install: &tfp_assets::GameInstall,
        main: Option<TextureId>,
    ) -> Result<Self> {
        let la = LookAssets::load(install)?;
        let owns_main = main.is_none();
        let main = match main {
            Some(m) => m,
            None => upload(
                gpu,
                r,
                &install
                    .atlas_xml(AtlasKind::Main)
                    .context("no main atlas")?,
            )?,
        };
        let bg = upload(
            gpu,
            r,
            &install.atlas_xml(AtlasKind::Bg).context("no bg atlas")?,
        )?;
        let index = tfp_assets::towers::TowerIndex::build(install)?;
        Ok(Self {
            la,
            main,
            bg,
            install: install.clone(),
            index,
            owns_main,
            boss: None,
        })
    }

    /// Frees the atlases uploaded by `load*` (not a shared main atlas).
    pub fn destroy(&self, r: &mut Renderer) {
        r.destroy_texture(self.bg);
        if self.owns_main {
            r.destroy_texture(self.main);
        }
        if let Some((t, _)) = &self.boss {
            r.destroy_texture(*t);
        }
    }

    /// Uploads the boss atlas (once) so sim drawables on it can be drawn.
    pub fn load_boss(&mut self, gpu: &Gpu, r: &mut Renderer) -> Result<()> {
        if self.boss.is_none() {
            let xml = self
                .install
                .atlas_xml(AtlasKind::Boss)
                .context("no boss atlas")?;
            let t = upload(gpu, r, &xml)?;
            self.boss = Some((t, tfp_assets::atlas::Atlas::load(&xml)?));
        }
        Ok(())
    }

    fn tex(&self, k: AtlasKind) -> TextureId {
        match (k, &self.boss) {
            (AtlasKind::Bg, _) => self.bg,
            (AtlasKind::Boss, Some((t, _))) => *t,
            _ => self.main,
        }
    }

    fn sub(&self, i: &ImageRef) -> SubTexture {
        let r = i.src;
        SubTexture::new(self.tex(i.atlas), Rect::new(r.x, r.y, r.w, r.h))
    }

    /// A main-atlas image by name.
    pub fn main_sub(&self, name: &str) -> Option<SubTexture> {
        let r = self.la.main_atlas.get(name)?;
        Some(SubTexture::new(self.main, Rect::new(r.x, r.y, r.w, r.h)))
    }

    /// Frame of a sprite-data sprite at `t` frames (from `start` index):
    /// (subtexture, origin, sprite position offset).
    pub(crate) fn sprite(
        &self,
        set: SpriteSet,
        id: &str,
        anim: &str,
        start: u32,
        t: u64,
    ) -> Option<(SubTexture, Vec2, Vec2)> {
        let (sd, atlas, tex) = match set {
            SpriteSet::Main => (&self.la.sprites, &self.la.main_atlas, self.main),
            SpriteSet::Bg => (&self.la.bg_sprites, &self.la.bg_atlas, self.bg),
        };
        let def = sd.get(id)?;
        let r = atlas.get(&def.texture)?;
        let frames = def.frame_rects(r);
        let f = match def.animation(anim) {
            Some(a) if !a.frames.is_empty() => {
                let per = (a.delay * 60.0).max(1.0);
                let i = start as usize + (t as f32 / per) as usize;
                let i = if a.looped || a.delay <= 0.0 {
                    i % a.frames.len()
                } else {
                    i.min(a.frames.len() - 1)
                };
                a.frames[i]
            }
            _ => 0,
        };
        let fr = frames.get(f.max(0) as usize).copied()?;
        Some((
            SubTexture::new(tex, Rect::new(fr.x, fr.y, fr.w, fr.h)),
            Vec2::new(def.origin.0, def.origin.1),
            Vec2::new(def.position.0, def.position.1),
        ))
    }

    /// Builds the look of level `level` (index into the tower's levels).
    pub fn look(&self, tower: usize, level: usize, seed: Option<i32>) -> Result<LevelLook> {
        let t = self
            .index
            .versus
            .get(tower)
            .context("no such versus tower")?;
        let p = t
            .levels
            .get(level)
            .with_context(|| format!("tower has {} levels", t.levels.len()))?;
        let lvl = tfp_assets::level::Level::load(p)?;
        let theme = self
            .la
            .game_data
            .theme(&t.theme)
            .with_context(|| format!("unknown theme {}", t.theme))?;
        let rel = tfp_assets::towers::game_relative_path(&self.install, p).unwrap_or_default();
        let load_seed = tfp_assets::towers::versus_load_seed(&rel);
        let opts = LookOptions {
            procedural: t.procedural.then_some((seed.unwrap_or(load_seed), false)),
            ..Default::default()
        };
        Ok(LevelLook::build(&self.la, &lvl, theme, load_seed, &opts)?)
    }

    /// [`look`](Self::look) of a procedural tower with a match's layout
    /// key (seed, Team Deathmatch) — the same layout the sim's
    /// `loader::content_for_match` generates (online matches).
    pub fn look_procedural(
        &self,
        tower: usize,
        level: usize,
        key: (i32, bool),
    ) -> Result<LevelLook> {
        let t = self
            .index
            .versus
            .get(tower)
            .context("no such versus tower")?;
        let p = t
            .levels
            .get(level)
            .with_context(|| format!("tower has {} levels", t.levels.len()))?;
        let lvl = tfp_assets::level::Level::load(p)?;
        let theme = self
            .la
            .game_data
            .theme(&t.theme)
            .with_context(|| format!("unknown theme {}", t.theme))?;
        let rel = tfp_assets::towers::game_relative_path(&self.install, p).unwrap_or_default();
        let load_seed = tfp_assets::towers::versus_load_seed(&rel);
        let opts = LookOptions {
            procedural: t.procedural.then_some(key),
            ..Default::default()
        };
        Ok(LevelLook::build(&self.la, &lvl, theme, load_seed, &opts)?)
    }

    /// The tower theme's music cue and whether it is a Dark World theme
    /// (overtime then plays `DarkBoss` instead of `Boss`; audio.md 6.2).
    pub fn music(&self, tower: usize) -> Option<(String, bool)> {
        let t = self.index.versus.get(tower)?;
        let th = self.la.game_data.theme(&t.theme)?;
        Some((th.music.clone(), th.world.eq_ignore_ascii_case("dark")))
    }

    /// Finds a versus tower by index or (part of) its directory/theme name.
    pub fn find(&self, q: &str) -> Option<usize> {
        if let Ok(i) = q.parse::<usize>() {
            return (i < self.index.versus.len()).then_some(i);
        }
        let q = q.to_ascii_lowercase().replace([' ', '\'', '-'], "");
        self.index.versus.iter().position(|t| {
            let n = t
                .dir_name
                .to_ascii_lowercase()
                .replace([' ', '\'', '-'], "");
            let th = t.theme.to_ascii_lowercase().replace([' ', '\'', '-'], "");
            n.contains(&q) || th.contains(&q)
        })
    }
}

/// Where the scene's in-level drawing goes (`TowerView::draw_full`): over
/// the back layer (-3), at a depth of the gameplay layer, or on top of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InLevel {
    Back,
    Depth(i32),
    Top,
}

/// Gameplay-layer depths the scene draws at (rendering.md 4.1, 9.1):
/// explosions -40, `Particles` -80, `ParticlesFG` -490, the Versus
/// skulls and P-tags -1000.
pub const IN_LEVEL_DEPTHS: [i32; 4] = [-40, -80, -490, -1000];

/// A level's look on the GPU: canvases + the look, animated by frame.
pub struct TowerView {
    pub look: LevelLook,
    bg_layer: TextureId,
    solid_layer: TextureId,
    fg: TextureId,
    light: TextureId,
    pub size: (u32, u32),
    /// Extra darkness (dark orbs etc.) on top of the theme's.
    pub alpha_add: f32,
    /// The dark orb's share of `alpha_add`, eased 0.02 per frame
    /// (rendering.md 7.4).
    pub dark_ease: f32,
    /// Final-kill spotlight: only the extra lights shine.
    pub spotlight: bool,
    /// Composite the screen into this canvas instead of the default target
    /// (a camera zoom draws it scaled afterwards).
    pub screen_target: Option<TextureId>,
    lightning: Vec<Lightning>,
    /// Per static light: it belongs to a torch the sim owns (the sim's
    /// view lights it while lit; the look's copy is skipped then).
    sim_owned: Vec<bool>,
    /// The sim entities' coloured lights this frame (set by the scene;
    /// `match_draw::prepare`).
    pub dyn_lights: Vec<tfp_assets::look::Light>,
    /// Lit regions this frame: rect (x, y, w, h), stored colour, alpha.
    pub dyn_rects: Vec<([f32; 4], [u8; 4], f32)>,
    /// Ghost lights seen last frame, and those fading out after death.
    pub ghost_seen: Vec<(tfp_sim::EntityId, tfp_assets::look::Light)>,
    pub ghost_fading: Vec<tfp_assets::look::Light>,
}

/// `LightningFlash` (rendering.md 6.4): D idle 180+Next(540), A on 6,
/// B off 6+Next(10), C on 4+Next(10), back to D. Cosmetic RNG.
#[derive(Clone, Debug)]
struct Lightning {
    color: [u8; 4],
    state: u8,
    left: u32,
    rng: u64,
}

impl Lightning {
    fn on(&self) -> bool {
        matches!(self.state, 1 | 3)
    }

    fn next(&mut self, n: u64) -> u32 {
        (crate::front::variants::next_rand(&mut self.rng) % n) as u32
    }

    /// One frame; true when a strike starts (play the thunder). A phase
    /// of N frames is on for exactly N frames (the switch frame counts).
    fn update(&mut self) -> bool {
        if self.left > 1 {
            self.left -= 1;
            return false;
        }
        self.state = (self.state + 1) % 4;
        self.left = match self.state {
            0 => 180 + self.next(540),
            1 => 6,
            2 => 6 + self.next(10),
            _ => 4 + self.next(10),
        };
        self.state == 1
    }
}

impl TowerView {
    pub fn new(gpu: &Gpu, r: &mut Renderer, g: &TowerGfx, look: LevelLook) -> Result<Self> {
        let size = (look.width, look.height);
        let bg_layer = r.create_canvas(gpu, size);
        let solid_layer = r.create_canvas(gpu, size);
        let fg = r.create_canvas(gpu, size);
        let light = r.create_canvas(gpu, size);
        // Tile layers: tiles copied opaquely (alpha included), stamps blended.
        let mut dl = DrawList::new();
        for (canvas, layer) in [(bg_layer, &look.bg_tiles), (solid_layer, &look.solid_tiles)] {
            dl.begin_pass(Some(canvas), Some(Color::TRANSPARENT));
            Self::draw_tiles(&mut dl, g, layer);
        }
        r.render(gpu, &dl, fg)?;
        let lightning = look
            .background
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                BgElement::LightningFlash { color, init_delay } => Some(Lightning {
                    color: *color,
                    state: 0,
                    left: (*init_delay).max(0) as u32,
                    rng: 0x11AB_0000 + i as u64,
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        let torch_lights: Vec<&tfp_assets::look::Light> = look
            .decorations
            .iter()
            .filter(|d| matches!(d.kind, tfp_assets::look::DecorationKind::Torch { .. }))
            .filter_map(|d| d.light.as_ref())
            .collect();
        let sim_owned = look
            .lighting
            .lights
            .iter()
            .map(|l| torch_lights.contains(&l))
            .collect();
        Ok(Self {
            sim_owned,
            dyn_lights: Vec::new(),
            dyn_rects: Vec::new(),
            ghost_seen: Vec::new(),
            ghost_fading: Vec::new(),
            look,
            bg_layer,
            solid_layer,
            fg,
            light,
            size,
            alpha_add: 0.0,
            dark_ease: 0.0,
            spotlight: false,
            screen_target: None,
            lightning,
        })
    }

    /// One frame of the look's own state (lightning). Returns true when a
    /// lightning strike starts (the caller plays `ENV_LIGHTNING`).
    pub fn update(&mut self) -> bool {
        let mut strike = false;
        for l in &mut self.lightning {
            strike |= l.update();
        }
        strike
    }

    pub fn destroy(&self, r: &mut Renderer) {
        for t in [self.bg_layer, self.solid_layer, self.fg, self.light] {
            r.destroy_texture(t);
        }
    }

    fn draw_tiles(dl: &mut DrawList, g: &TowerGfx, layer: &TileLayer) {
        dl.set_blend(Blend::Opaque);
        for t in &layer.tiles {
            let s = SubTexture::new(g.main, Rect::new(t.src.x, t.src.y, t.src.w, t.src.h));
            dl.sprite_at(s, Vec2::new(t.x as f32, t.y as f32));
        }
        dl.set_blend(Blend::Alpha);
        for s in &layer.stamps {
            dl.sprite_at(g.sub(&s.image), Vec2::new(s.x as f32, s.y as f32));
        }
    }

    fn canvas_sprite(&self, id: TextureId) -> SubTexture {
        SubTexture::new(id, Rect::new(0, 0, self.size.0 as i32, self.size.1 as i32))
    }

    /// Emits the passes for frame `t` (frames since the level started) and
    /// ends with the default target holding the composited level (its pass
    /// stays open for overlays).
    pub fn draw(&self, dl: &mut DrawList, g: &TowerGfx, t: u64) {
        self.draw_with(dl, g, t, None, &[]);
    }

    /// Like [`draw`](Self::draw), with the sim's drawables merged into the
    /// gameplay layer by depth and extra black lights `(pos, radius)`.
    pub fn draw_with(
        &self,
        dl: &mut DrawList,
        g: &TowerGfx,
        t: u64,
        actors: Option<(&tfp_sim::View, &tfp_sim::Content)>,
        extra_lights: &[(Vec2, f32)],
    ) {
        self.draw_full(dl, g, t, actors, extra_lights, &|_, _| {});
    }

    /// The full version: `fg_extra` draws on top of the gameplay layer
    /// inside the foreground (darkened) canvas, e.g. particles.
    pub fn draw_full(
        &self,
        dl: &mut DrawList,
        g: &TowerGfx,
        t: u64,
        actors: Option<(&tfp_sim::View, &tfp_sim::Content)>,
        extra_lights: &[(Vec2, f32)],
        in_level: &dyn Fn(&mut DrawList, InLevel),
    ) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let l = &self.look.lighting;
        // 1. Light canvas.
        let cc = l.clear_color;
        dl.begin_pass(
            Some(self.light),
            Some(Color::rgba(cc[0], cc[1], cc[2], 255)),
        );
        let sine = 0.4 * (t as f32 / 240.0 * std::f32::consts::TAU).sin();
        if let Some(cone) = g.la.main_atlas.get("light") {
            let cone = SubTexture::new(
                g.main,
                Rect::new(cone.x + 2, cone.y + 2, cone.w - 4, cone.h - 4),
            );
            let extra: Vec<tfp_assets::look::Light> = extra_lights
                .iter()
                .map(|&(p, r)| tfp_assets::look::Light {
                    x: p.x,
                    y: p.y,
                    radius: r,
                    color: [0, 0, 0, 255],
                    alpha: 1.0,
                    wrap: true,
                    kind: tfp_assets::look::LightKind::Decoration,
                })
                .collect();
            // With the sim's view the torches are its light sources.
            let statics = l
                .lights
                .iter()
                .zip(&self.sim_owned)
                .filter(|(_, owned)| !(**owned && actors.is_some()))
                .map(|(l, _)| l)
                .chain(&self.dyn_lights)
                .filter(|_| !self.spotlight);
            for light in statics.chain(&extra) {
                let c = light.color;
                let col = Color::rgba(c[0], c[1], c[2], c[3]) * light.alpha.clamp(0.0, 1.0);
                let scale = (light.radius + light.radius * 0.4 * sine) / 20.0;
                let mut spots = vec![Vec2::new(light.x, light.y).floor()];
                if light.wrap {
                    let p = spots[0];
                    spots.push(Vec2::new(p.x + if light.x < w / 2.0 { w } else { -w }, p.y));
                    spots.push(Vec2::new(
                        p.x,
                        p.y + if light.y < 120.0 { 240.0 } else { -240.0 },
                    ));
                }
                for p in spots {
                    dl.sprite(
                        cone,
                        &SpriteParams::at(p)
                            .origin(Vec2::splat(20.0))
                            .scale(Vec2::splat(scale))
                            .color(col),
                    );
                }
            }
        }
        if !self.spotlight {
            for &([x, y, rw, rh], c, a) in &self.dyn_rects {
                dl.rect(
                    x,
                    y,
                    rw,
                    rh,
                    Color::rgba(c[0], c[1], c[2], c[3]) * a.clamp(0.0, 1.0),
                );
            }
        }
        // 2. Foreground canvas: layer -3 (BG tiles, back decorations), then
        //    layer 0 by depth with the solid tiles at -150, then the
        //    theme's foreground.
        dl.begin_pass(Some(self.fg), Some(Color::TRANSPARENT));
        dl.sprite_at(self.canvas_sprite(self.bg_layer), Vec2::ZERO);
        let mut back: Vec<&Decoration> = self
            .look
            .decorations
            .iter()
            .filter(|d| d.layer == -3)
            .collect();
        back.sort_by_key(|d| std::cmp::Reverse(d.depth));
        for d in back {
            draw_decoration(dl, g, d, t);
        }
        in_level(dl, InLevel::Back);
        // Layer 0: decorations, solid tiles (-150) and the sim's drawables,
        // larger depth first; ties keep list order (decorations, then sim).
        enum Item<'a> {
            Deco(&'a Decoration),
            Solids,
            Actor(&'a tfp_sim::Drawable),
            Slot(i32),
        }
        let mut front: Vec<(i32, usize, Item)> = Vec::new();
        for d in self.look.decorations.iter().filter(|d| d.layer != -3) {
            front.push((d.depth, front.len(), Item::Deco(d)));
        }
        front.push((-150, front.len(), Item::Solids));
        for d in IN_LEVEL_DEPTHS {
            front.push((d, front.len(), Item::Slot(d)));
        }
        if let Some((v, _)) = actors {
            for d in &v.items {
                front.push((d.depth, front.len(), Item::Actor(d)));
            }
        }
        front.sort_by_key(|(d, i, _)| (std::cmp::Reverse(*d), *i));
        for (_, _, it) in front {
            match it {
                Item::Deco(d) => draw_decoration(dl, g, d, t),
                Item::Solids => dl.sprite_at(self.canvas_sprite(self.solid_layer), Vec2::ZERO),
                Item::Actor(d) => {
                    if let Some((_, c)) = actors {
                        draw_actor(dl, g, c, d);
                    }
                }
                Item::Slot(d) => in_level(dl, InLevel::Depth(d)),
            }
        }
        in_level(dl, InLevel::Top);
        for e in &self.look.foreground {
            draw_element(dl, g, e, t, w, h);
        }
        // 3. The screen: background, then the lit foreground.
        dl.begin_pass(self.screen_target, Some(Color::BLACK));
        // Lightning: at its element's place among the background layers,
        // the layer colour fills the screen while a flash is on (farther
        // layers hidden, nearer ones drawn over it; rendering.md 6.4).
        let mut flashes = self.lightning.iter();
        let mut flashing = false;
        for e in &self.look.background {
            if let BgElement::LightningFlash { .. } = e {
                if let Some(f) = flashes.next()
                    && f.on()
                {
                    let c = f.color;
                    dl.rect(0.0, 0.0, w, h, Color::rgba(c[0], c[1], c[2], 255));
                    flashing = true;
                }
                continue;
            }
            draw_element(dl, g, e, t, w, h);
        }
        let mut alpha = (l.alpha + self.alpha_add).clamp(0.0, 1.0);
        if flashing {
            alpha = (alpha * 1.5).min(1.0);
        }
        dl.light_composite(self.fg, self.light, LightParams { alpha });
    }
}

/// One sim drawable (view.rs): atlas cell or rectangle, XNA placement.
fn draw_actor(dl: &mut DrawList, g: &TowerGfx, c: &tfp_sim::Content, d: &tfp_sim::Drawable) {
    use tfp_sim::Visual;
    let [r, gr, b, a] = d.tint;
    let color = Color::rgb(r, gr, b) * (a as f32 / 255.0);
    let params = SpriteParams {
        position: Vec2::new(d.pos.x, d.pos.y),
        origin: Vec2::new(d.origin.x, d.origin.y),
        scale: Vec2::new(d.scale.x, d.scale.y),
        rotation: d.rotation,
        color,
        flip_x: d.flip_x,
        flip_y: d.flip_y,
    };
    match d.visual {
        Visual::Sprite {
            tex,
            frame,
            frame_w,
            frame_h,
        } => {
            let tr = c.texture(tex);
            let atlas = match (tr.atlas, &g.boss) {
                (AtlasKind::Bg, _) => &g.la.bg_atlas,
                (AtlasKind::Boss, Some((_, a))) => a,
                _ => &g.la.main_atlas,
            };
            let Some(rect) = atlas.get(&tr.name) else {
                return;
            };
            let mut src = Rect::new(rect.x, rect.y, rect.w, rect.h);
            if frame >= 0 && frame_w > 0 && frame_h > 0 {
                let cols = (rect.w / frame_w as i32).max(1);
                let f = frame as i32;
                src = Rect::new(
                    rect.x + (f % cols) * frame_w as i32,
                    rect.y + (f / cols) * frame_h as i32,
                    frame_w as i32,
                    frame_h as i32,
                );
            }
            if d.clip_h < 1.0 {
                src.h = (src.h as f32 * d.clip_h.max(0.0)) as i32;
                if src.h <= 0 {
                    return;
                }
            }
            dl.sprite(SubTexture::new(g.tex(tr.atlas), src), &params);
        }
        Visual::Rect { w, h } => {
            let px = SubTexture::new(TextureId::WHITE, Rect::new(0, 0, 1, 1));
            let scale = params.scale * Vec2::new(w, h);
            dl.sprite(px, &SpriteParams { scale, ..params });
        }
        Visual::Outline { w, h } => {
            let p = params.position - params.origin;
            dl.hollow_rect(p.x.floor(), p.y.floor(), w, h, color);
        }
    }
}

fn tint(a: f32) -> Color {
    Color::WHITE * a.clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn sprite_at(
    dl: &mut DrawList,
    g: &TowerGfx,
    set: SpriteSet,
    id: &str,
    anim: &str,
    start: u32,
    t: u64,
    pos: Vec2,
    flip: bool,
    a: f32,
) {
    if let Some((s, origin, off)) = g.sprite(set, id, anim, start, t) {
        dl.sprite(
            s,
            &SpriteParams::at((pos + off).floor())
                .origin(origin)
                .flip(flip, false)
                .color(tint(a)),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn tiled(
    dl: &mut DrawList,
    s: SubTexture,
    x: f32,
    y: f32,
    tx: bool,
    ty: bool,
    a: f32,
    w: f32,
    h: f32,
) {
    let (iw, ih) = (s.width().max(1) as f32, s.height().max(1) as f32);
    let xs: Vec<f32> = if tx {
        let mut v = Vec::new();
        let mut p = x.rem_euclid(iw) - iw;
        while p < w {
            v.push(p);
            p += iw;
        }
        v
    } else {
        vec![x]
    };
    let ys: Vec<f32> = if ty {
        let mut v = Vec::new();
        let mut p = y.rem_euclid(ih) - ih;
        while p < h {
            v.push(p);
            p += ih;
        }
        v
    } else {
        vec![y]
    };
    for &yy in &ys {
        for &xx in &xs {
            dl.sprite(
                s,
                &SpriteParams::at(Vec2::new(xx, yy).floor()).color(tint(a)),
            );
        }
    }
}

fn draw_element(dl: &mut DrawList, g: &TowerGfx, e: &BgElement, t: u64, w: f32, h: f32) {
    use std::f32::consts::{PI, TAU};
    let tf = t as f32;
    match e {
        BgElement::Backdrop {
            image,
            x,
            y,
            opacity,
        } => {
            dl.sprite(
                g.sub(image),
                &SpriteParams::at(Vec2::new(*x, *y).floor()).color(tint(*opacity)),
            );
        }
        BgElement::Scroll {
            image,
            x,
            y,
            speed,
            opacity,
            tile_x,
            tile_y,
        } => {
            tiled(
                dl,
                g.sub(image),
                x + speed.0 * tf,
                y + speed.1 * tf,
                *tile_x,
                *tile_y,
                *opacity,
                w,
                h,
            );
        }
        BgElement::Wavy {
            image,
            x,
            y,
            opacity,
            wave_frames,
            amplitude,
            slice_size,
            slice_add,
            horizontal,
        } => {
            let s = g.sub(image);
            let phase = tf * TAU / (*wave_frames).max(1) as f32;
            let step = TAU * slice_add;
            let n = (*slice_size).max(1);
            let len = if *horizontal { s.width() } else { s.height() };
            let mut o = 0;
            let mut k = 0;
            while o < len {
                let off = ((phase + step * k as f32).sin() * amplitude).round();
                let (part, pos) = if *horizontal {
                    (
                        s.sub(o, 0, n.min(len - o), s.height()),
                        Vec2::new(x.floor() + o as f32, y.floor() + off),
                    )
                } else {
                    (
                        s.sub(0, o, s.width(), n.min(len - o)),
                        Vec2::new(x.floor() + off, y.floor() + o as f32),
                    )
                };
                dl.sprite(part, &SpriteParams::at(pos).color(tint(*opacity)));
                o += n;
                k += 1;
            }
        }
        BgElement::Animated {
            sprite,
            x,
            y,
            opacity,
        } => {
            sprite_at(
                dl,
                g,
                SpriteSet::Bg,
                sprite,
                "0",
                0,
                t,
                Vec2::new(*x, *y),
                false,
                *opacity,
            );
        }
        BgElement::Fade {
            sprite,
            opacity,
            opacity_range,
            fade_speed,
            init_frame,
            init_phase,
            init_pos,
            ..
        } => {
            let a = opacity + opacity_range * (init_phase + fade_speed * TAU * tf).sin();
            sprite_at(
                dl,
                g,
                SpriteSet::Bg,
                sprite,
                "0",
                *init_frame as u32,
                t,
                Vec2::new(init_pos.0, init_pos.1),
                false,
                a,
            );
        }
        BgElement::FadeMove {
            sprite,
            max_alpha,
            time_frames,
            init_pos,
            init_rate,
            ..
        } => {
            // Fade in and out over `time_frames` while drifting (approximate).
            let period = time_frames.max(1.0);
            let k = (tf % period) / period;
            let a = max_alpha * (k * PI).sin();
            let pos = Vec2::new(init_pos.0 + init_rate * (tf % period) * 0.2, init_pos.1);
            sprite_at(dl, g, SpriteSet::Bg, sprite, "0", 0, t, pos, false, a);
        }
        BgElement::Node {
            sprite,
            opacity,
            nodes,
            init_node,
            init_pos,
            ..
        } => {
            let anim = nodes
                .get(*init_node)
                .map_or("0".to_string(), |n| n.anim.to_string());
            sprite_at(
                dl,
                g,
                SpriteSet::Bg,
                sprite,
                &anim,
                0,
                t,
                Vec2::new(init_pos.0, init_pos.1),
                false,
                *opacity,
            );
        }
        BgElement::Ocean {
            sprite,
            x,
            y,
            opacity,
            time_min,
            time_add,
            init_delay,
        } => {
            let period = (*time_min + time_add / 2).max(1) as u64;
            let local = (t + period - (*init_delay as u64 % period)) % period;
            sprite_at(
                dl,
                g,
                SpriteSet::Bg,
                sprite,
                "0",
                0,
                local,
                Vec2::new(*x, *y),
                false,
                *opacity,
            );
        }
        BgElement::SacredGroundMoon { x, y, image, .. }
        | BgElement::FlightMoon { x, y, image, .. } => {
            dl.sprite_at(g.sub(image), Vec2::new(*x, *y).floor());
        }
        BgElement::GhostShip {
            image,
            x,
            y,
            scale,
            init_phases,
        } => {
            let periods = [2400.0, 1200.0, 300.0, 600.0];
            let s: Vec<f32> = init_phases
                .iter()
                .zip(periods)
                .map(|(p, per)| (p + tf * TAU / per).sin())
                .collect();
            let a = 0.2 + 0.3 * s[3];
            if a > 0.0 {
                let sub = g.sub(image);
                dl.sprite(
                    sub,
                    &SpriteParams::at(Vec2::new(x + s[0] * 80.0, y + s[1] * 10.0))
                        .origin(Vec2::new(sub.width() as f32 * 0.5, sub.height() as f32))
                        .scale(Vec2::splat(*scale))
                        .rotation(PI / 36.0 * s[2])
                        .color(tint(a)),
                );
            }
        }
        BgElement::Vortex {
            ring,
            init_angles,
            init_bezier,
            ..
        } => {
            dl.rect(0.0, 0.0, w, h, Color::rgb(4, 2, 5));
            let [b, e, c] = *init_bezier;
            let sub = g.sub(ring);
            for (i, ang) in init_angles.iter().enumerate() {
                let v = i as f32 / 16.0;
                let k = v * v * v;
                let u = 1.0 - k;
                let p = Vec2::new(
                    u * u * b.0 + 2.0 * u * k * c.0 + k * k * e.0,
                    u * u * b.1 + 2.0 * u * k * c.1 + k * k * e.1,
                );
                let scale = 0.05 + (2.0 - 0.05) * k;
                let gr = 0.02 + (0.45 - 0.02) * k;
                let q = (gr * 255.0) as u8;
                dl.sprite(
                    sub,
                    &SpriteParams::at(p)
                        .origin(sub.rect.size() / 2.0)
                        .scale(Vec2::splat(scale))
                        .rotation(ang + tf * 0.01)
                        .color(Color::rgb(q, q, q)),
                );
            }
        }
        BgElement::LightningFlash { .. } | BgElement::AscensionLightning { .. } => {}
    }
}

fn draw_decoration(dl: &mut DrawList, g: &TowerGfx, d: &Decoration, t: u64) {
    let at = Vec2::new(d.x, d.y);
    for p in &d.parts {
        match p {
            Part::Sprite {
                set,
                id,
                anim,
                start_index,
                offset,
                flip_x,
            } => {
                sprite_at(
                    dl,
                    g,
                    *set,
                    id,
                    anim,
                    *start_index,
                    t,
                    at + Vec2::new(offset.0, offset.1),
                    *flip_x,
                    1.0,
                );
            }
            Part::Image {
                image,
                offset,
                origin,
                flip_x,
                alpha,
            } => {
                dl.sprite(
                    g.sub(image),
                    &SpriteParams::at((at + Vec2::new(offset.0, offset.1)).floor())
                        .origin(Vec2::new(origin.0, origin.1))
                        .flip(*flip_x, false)
                        .color(tint(*alpha)),
                );
            }
            Part::Strips { image, strip } => {
                let s = g.sub(image);
                let (w, h) = (s.width(), s.height());
                let n = (*strip).max(1);
                for i in 0..h / n {
                    let part = s.sub(0, h - (i + 1) * n, w, n);
                    dl.sprite_at(
                        part,
                        Vec2::new(d.x - w as f32 / 2.0, d.y - ((i + 1) * n) as f32).floor(),
                    );
                }
            }
            Part::Banner { image, init_phase } => {
                let s = g.sub(image);
                let h = s.height();
                let tf = t as f32;
                let sl = (tf / 240.0 * std::f32::consts::TAU).sin();
                let phase = init_phase + tf * (0.13 + 0.03 * sl);
                let amp = 2.5 + 0.4 * sl;
                let mut y = 0;
                let mut n = 0;
                while y < h {
                    let sh = 4.min(h - y);
                    let off =
                        ((phase + std::f32::consts::FRAC_PI_4 * n as f32).sin() * amp * y as f32
                            / h as f32)
                            .round();
                    dl.sprite_at(
                        s.sub(0, y, s.width(), sh),
                        Vec2::new(d.x - 10.0 + off, d.y + y as f32).floor(),
                    );
                    y += 4;
                    n += 1;
                }
            }
        }
    }
}

// ---- the viewer scene ------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ViewerOptions {
    pub tower: String,
    pub level: usize,
    /// Use the 8-player install's data.
    pub eight_player: bool,
    pub seed: Option<i32>,
}

/// `--tower NAME --level N`: draws a full level, animated.
pub struct TowerViewer {
    opts: ViewerOptions,
    gfx: Option<TowerGfx>,
    view: Option<TowerView>,
    size: (u32, u32),
    tick: u64,
    title: String,
}

impl TowerViewer {
    pub fn new(opts: ViewerOptions) -> Self {
        Self {
            opts,
            gfx: None,
            view: None,
            size: (320, 240),
            tick: 0,
            title: String::new(),
        }
    }
}

impl Scene for TowerViewer {
    fn game_size(&self) -> (u32, u32) {
        self.size
    }

    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()> {
        let installs = tfp_assets::Installs::discover();
        let inst = if self.opts.eight_player {
            installs.eight_player.clone()
        } else {
            installs.steam.clone().or(installs.eight_player.clone())
        };
        let Some(inst) = inst else {
            bail!("game data not found (set TF_GAME_DIR / TF8_GAME_DIR)")
        };
        let g = TowerGfx::load(ctx.gpu, ctx.renderer, &inst)?;
        let Some(ti) = g.find(&self.opts.tower) else {
            let names: Vec<&str> = g.index.versus.iter().map(|t| t.dir_name.as_str()).collect();
            bail!(
                "no versus tower {:?}; towers: {}",
                self.opts.tower,
                names.join(", ")
            );
        };
        let look = g.look(ti, self.opts.level, self.opts.seed)?;
        if !look.missing.is_empty() {
            log::warn!("art missing in this install: {}", look.missing.join(", "));
        }
        self.title = format!(
            "{} level {} ({}x{})",
            g.index.versus[ti].dir_name, self.opts.level, look.width, look.height
        );
        println!(
            "tower: {}, {} decorations, {} lights, {} bg / {} fg elements, darkness {:.2}",
            self.title,
            look.decorations.len(),
            look.lighting.lights.len(),
            look.background.len(),
            look.foreground.len(),
            look.lighting.alpha
        );
        self.size = (look.width, look.height);
        self.view = Some(TowerView::new(ctx.gpu, ctx.renderer, &g, look)?);
        self.gfx = Some(g);
        Ok(())
    }

    fn tick(&mut self, ctx: &mut TickContext<'_>) {
        self.tick = ctx.tick;
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        if let (Some(v), Some(g)) = (&self.view, &self.gfx) {
            v.draw(ctx.list, g, self.tick);
        }
    }
}
