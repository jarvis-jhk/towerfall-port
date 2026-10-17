//! Platform demo scene: exercises renderer, input and audio until the real
//! game exists. With the original's data installed it draws from the real
//! atlases (background, tiles, archers, arrows, light sprites, XNB font)
//! and plays real SFX and music; without it, placeholder shapes and tones.
//!
//! One archer per input device (keyboard first, then controllers in
//! connection order, any number); archers without a device walk on their
//! own. Direction moves, A/Space jumps, X/J shoots (with screen shake),
//! Y/T toggles the slow-motion screen wave.

use std::f32::consts::PI;
use std::sync::Arc;

use anyhow::Result;
use sdl2::controller::Button;
use sdl2::keyboard::Scancode;
use tfp_assets::atlas::{AtlasKind, LoadedAtlas};
use tfp_render::glam::Vec2;
use tfp_render::{
    Blend, Color, DrawList, LightParams, Rect, SpriteFont, SpriteParams, SubTexture, TextureId,
    Viewport, Wave, debug_font, debug_font_image,
};

use crate::app::{DrawContext, LoadContext, Scene, TickContext};
use crate::audio::{PcmMusic, PlayParams, SoundData};
use crate::content::{GpuAtlas, sound, sprite_font};
use crate::input::RawInput;

/// Archer colours for placeholders and name tags (12 distinct).
const COLORS: [u32; 12] = [
    0x5ad35a, 0x4a8cf0, 0xf05050, 0xf0d040, 0xe070e0, 0x50e0e0, 0xf09030, 0xffffff, 0x9070f0,
    0x80c080, 0xc06040, 0x808080,
];

/// Demo options from the command line.
#[derive(Clone, Debug, Default)]
pub struct DemoOptions {
    pub game_size: (u32, u32),
    /// Music: index into the original's music wave bank.
    pub music_track: Option<usize>,
    /// Music: a WAV file instead.
    pub music_wav: Option<std::path::PathBuf>,
    /// Ignore the installed game data (placeholder mode).
    pub no_assets: bool,
}

/// What we found in the original's data.
struct Content {
    bg: Option<SubTexture>,
    /// Per archer colour: body frames (12x20, origin 8,20 in the data).
    bodies: Vec<Vec<SubTexture>>,
    body_origin: Vec2,
    run_frames: Vec<i32>,
    arrow: Option<SubTexture>,
    light: Option<SubTexture>,
    tiles: Option<SubTexture>,
    font: Option<SpriteFont>,
    /// The original's pillar-box images (`aspectBarLeft/Right`).
    aspect: Option<(SubTexture, SubTexture)>,
}

struct Archer {
    pos: Vec2,
    vel: Vec2,
    facing: f32,
    on_ground: bool,
    anim: f32,
    /// Device label for the name tag.
    label: String,
}

struct Arrow {
    pos: Vec2,
    vel: Vec2,
    life: u32,
}

pub struct Demo {
    opts: DemoOptions,
    content: Option<Content>,
    debug_font: Option<SpriteFont>,
    fg: Option<TextureId>,
    light: Option<TextureId>,
    canvas_size: (u32, u32),
    archers: Vec<Archer>,
    arrows: Vec<Arrow>,
    shake: u32,
    tick: u64,
    sfx_jump: Option<Arc<SoundData>>,
    sfx_shoot: Option<Arc<SoundData>>,
    devices: usize,
    /// Slow-motion wave (T / pad Y toggles): amplitude and counter.
    wave_on: bool,
    wave_amp: f32,
    wave_counter: f32,
}

const FLOOR_Y: f32 = 200.0;
/// Lighting alpha (a theme's `DarknessOpacity`; the original uses ~0.2-0.4,
/// 1 for dark orbs).
const DARKNESS: f32 = 0.4;

fn invert(c: Color) -> Color {
    Color::rgba(255 - c.r, 255 - c.g, 255 - c.b, c.a)
}
const GRAVITY: f32 = 0.3;

impl Demo {
    pub fn new(opts: DemoOptions) -> Self {
        Self {
            opts,
            content: None,
            debug_font: None,
            fg: None,
            light: None,
            canvas_size: (0, 0),
            archers: Vec::new(),
            arrows: Vec::new(),
            shake: 0,
            tick: 0,
            sfx_jump: None,
            sfx_shoot: None,
            devices: 0,
            wave_on: false,
            wave_amp: 0.0,
            wave_counter: 0.0,
        }
    }

    fn load_content(&mut self, ctx: &mut LoadContext<'_>) -> Result<Option<Content>> {
        let installs = tfp_assets::Installs::discover();
        let Some(install) = installs.iter().next() else {
            println!("demo: original game data not found (set TF_GAME_DIR); using placeholders");
            return Ok(None);
        };
        println!("demo: using {}", install.summary());
        let load = |kind| -> Result<Option<LoadedAtlas>> {
            Ok(match install.atlas_xml(kind) {
                Some(p) => Some(LoadedAtlas::load(&p)?),
                None => None,
            })
        };
        let main = load(AtlasKind::Main)?.ok_or_else(|| anyhow::anyhow!("no main atlas"))?;
        let main = GpuAtlas::upload(ctx.gpu, ctx.renderer, &main);
        let bg = load(AtlasKind::Bg)?
            .map(|a| GpuAtlas::upload(ctx.gpu, ctx.renderer, &a))
            .and_then(|a| a.get("forestSky").or_else(|| a.get("daySky")));

        // Archer bodies: sprite data gives frame size and origin.
        let sprites = tfp_assets::sprite_data::SpriteData::load(
            &install.content.sprite_data_dir().join("spriteData.xml"),
        )?;
        let mut bodies = Vec::new();
        let mut body_origin = Vec2::new(8.0, 20.0);
        let mut run_frames = vec![0];
        for i in 0..9 {
            let Some(def) = sprites.get(&format!("PlayerBody{i}")) else {
                continue;
            };
            let Some(sub) = main.get(&def.texture) else {
                continue;
            };
            let a = &main.regions[&def.texture];
            let frames = def.frame_rects(tfp_assets::image::Rect::new(a.x, a.y, a.w, a.h));
            bodies.push(
                frames
                    .iter()
                    .map(|r| SubTexture::new(sub.texture, Rect::new(r.x, r.y, r.w, r.h)))
                    .collect(),
            );
            body_origin = Vec2::new(def.origin.0, def.origin.1);
            if let Some(run) = def.animation("run") {
                run_frames = run.frames.clone();
            }
        }

        let font = ["Archer.xnb", "MonocleDefault.xnb"]
            .iter()
            .map(|f| install.content.misc_dir().join(f))
            .find_map(|p| match tfp_assets::xnb::SpriteFont::load(&p) {
                Ok(f) => Some(f),
                Err(e) => {
                    log::warn!("font {}: {e}", p.display());
                    None
                }
            })
            .map(|f| sprite_font(ctx.gpu, ctx.renderer, &f));

        let sfx = |name: &str| {
            let p = install.content.sfx_dir().join(name);
            match tfp_assets::audio::Wav::load(&p) {
                Ok(pcm) => Some(sound(&pcm)),
                Err(e) => {
                    log::warn!("sfx {}: {e}", p.display());
                    None
                }
            }
        };
        self.sfx_jump = sfx("CHAR_A_JUMP.wav");
        self.sfx_shoot = sfx("CHAR_A_FIRE_ARROW.wav");

        if let Some(track) = self.opts.music_track {
            let bank = install.content.music_dir().join("MusicWaveBank.xwb");
            match tfp_assets::audio::xwb::WaveBank::open(&bank).and_then(|b| {
                let name = b.entries.get(track).and_then(|e| e.name.clone());
                b.decode(track).map(|pcm| (pcm, name))
            }) {
                Ok((pcm, name)) => {
                    println!(
                        "demo: music track {track} {} ({:.1} s, {} Hz, loop {:?})",
                        name.unwrap_or_default(),
                        pcm.seconds(),
                        pcm.sample_rate,
                        pcm.loop_region
                    );
                    let region = pcm.loop_region.map(|(s, l)| (s as usize, (s + l) as usize));
                    let source = PcmMusic::new(sound(&pcm), region.or(Some((0, pcm.frames()))));
                    ctx.audio.play_music(Box::new(source), 0.6, 1.0);
                }
                Err(e) => log::warn!("music {}: {e}", bank.display()),
            }
        }

        Ok(Some(Content {
            arrow: main.get("arrows/arrow"),
            light: main
                .get("light")
                .map(|l| l.sub(2, 2, l.width() - 4, l.height() - 4)),
            tiles: main.get("tilesets/sacredGround"),
            aspect: main.get("aspectBarLeft").zip(main.get("aspectBarRight")),
            bg,
            bodies,
            body_origin,
            run_frames,
            font,
        }))
    }

    fn ensure_archers(&mut self, input: &RawInput) {
        let devices = 1 + input.pads.len();
        self.devices = devices;
        let want = devices.max(4);
        while self.archers.len() < want {
            let i = self.archers.len();
            self.archers.push(Archer {
                pos: Vec2::new(40.0 + (i as f32 * 53.0) % 240.0, FLOOR_Y),
                vel: Vec2::ZERO,
                facing: 1.0,
                on_ground: true,
                anim: 0.0,
                label: String::new(),
            });
        }
        self.archers.truncate(want);
        for (i, a) in self.archers.iter_mut().enumerate() {
            a.label = match i {
                0 => "KEYBOARD".into(),
                n if n <= input.pads.len() => format!("PAD {}", n),
                _ => "CPU".into(),
            };
        }
    }

    fn step_archer(
        &mut self,
        i: usize,
        dir: Vec2,
        jump: bool,
        shoot: bool,
        audio: &mut crate::audio::Audio,
    ) {
        let w = self.canvas_size.0 as f32;
        let a = &mut self.archers[i];
        a.vel.x = dir.x * 1.5;
        if dir.x != 0.0 {
            a.facing = dir.x.signum();
            a.anim += 0.15;
        } else {
            a.anim = 0.0;
        }
        if jump && a.on_ground {
            a.vel.y = -4.5;
            a.on_ground = false;
            if let Some(s) = &self.sfx_jump {
                audio.play(
                    s,
                    PlayParams {
                        volume: 0.5,
                        pan: (a.pos.x / w * 2.0 - 1.0) * 0.5,
                        ..Default::default()
                    },
                );
            }
        }
        a.vel.y += GRAVITY;
        a.pos += a.vel;
        if a.pos.y >= FLOOR_Y {
            a.pos.y = FLOOR_Y;
            a.vel.y = 0.0;
            a.on_ground = true;
        }
        // Screen wrap, like the original.
        a.pos.x = a.pos.x.rem_euclid(w);
        if shoot {
            let aim = if dir == Vec2::ZERO {
                Vec2::new(a.facing, 0.0)
            } else {
                dir.normalize()
            };
            self.arrows.push(Arrow {
                pos: a.pos - Vec2::new(0.0, 10.0),
                vel: aim * 5.0 + Vec2::new(0.0, -1.0),
                life: 120,
            });
            self.shake = self.shake.max(6);
            if let Some(s) = &self.sfx_shoot {
                audio.play(
                    s,
                    PlayParams {
                        volume: 0.5,
                        pan: (a.pos.x / w * 2.0 - 1.0) * 0.5,
                        pitch: 1.0 + (i as f32 * 0.05),
                        ..Default::default()
                    },
                );
            }
        }
    }

    fn draw_world(&self, list: &mut DrawList) {
        let (w, h) = (self.canvas_size.0 as f32, self.canvas_size.1 as f32);
        let c = self.content.as_ref();
        // Rows of 10 px tiles: the tileset's top-edge row, then fill rows.
        let floor = |list: &mut DrawList, x0: f32, x1: f32, y: f32, y1: f32| match c
            .and_then(|c| c.tiles)
        {
            Some(t) => {
                let mut x = x0;
                let mut n = 0;
                while x < x1 {
                    list.sprite_at(t.sub((n % 4) * 10, 10, 10, 10), Vec2::new(x, y));
                    let mut yy = y + 10.0;
                    while yy < y1 {
                        let row = 30 + ((yy as i32 / 10 + n) % 3) * 10;
                        list.sprite_at(t.sub((n % 4) * 10, row, 10, 10), Vec2::new(x, yy));
                        yy += 10.0;
                    }
                    x += 10.0;
                    n += 1;
                }
            }
            None => list.rect(x0, y, x1 - x0, y1 - y, Color::rgb(70, 60, 90)),
        };
        floor(list, 0.0, w, FLOOR_Y, h);
        floor(list, 60.0, 120.0, 140.0, 160.0);
        floor(list, w - 120.0, w - 60.0, 120.0, 140.0);

        for (i, a) in self.archers.iter().enumerate() {
            let p = a.pos.floor();
            match c.filter(|c| !c.bodies.is_empty()) {
                Some(c) => {
                    let frames = &c.bodies[i % c.bodies.len()];
                    let f = if a.on_ground {
                        c.run_frames[a.anim as usize % c.run_frames.len()] as usize
                    } else {
                        4
                    };
                    list.sprite(
                        frames[f.min(frames.len() - 1)],
                        &SpriteParams::at(p)
                            .origin(c.body_origin)
                            .flip(a.facing < 0.0, false),
                    );
                }
                None => {
                    let col = Color::hex(COLORS[i % COLORS.len()]);
                    list.rect(p.x - 4.0, p.y - 14.0, 8.0, 14.0, col);
                    list.rect(
                        p.x - 3.0 + a.facing * 2.0,
                        p.y - 12.0,
                        2.0,
                        2.0,
                        Color::BLACK,
                    );
                }
            }
        }
        for ar in &self.arrows {
            let angle = ar.vel.y.atan2(ar.vel.x);
            match c.and_then(|c| c.arrow) {
                Some(s) => list.sprite(
                    s,
                    &SpriteParams::at(ar.pos.floor())
                        .origin(Vec2::new(6.0, 3.0))
                        .rotation(angle),
                ),
                None => list.line(ar.pos, ar.pos - ar.vel, Color::WHITE, 1.0),
            }
        }
    }

    fn draw_lights(&self, list: &mut DrawList) {
        let sine = ((self.tick as f32) / 240.0 * 2.0 * PI).sin() * 0.4;
        let light = self.content.as_ref().and_then(|c| c.light);
        let mut draw = |pos: Vec2, radius: f32, color: Color| {
            // The original's light sprite is 40 px across: scale = radius / 20.
            let scale = (radius + radius * 0.4 * sine) / 20.0;
            match light {
                Some(l) => list.sprite(
                    l,
                    &SpriteParams::at(pos.floor())
                        .origin(l.rect.size() / 2.0)
                        .scale(Vec2::splat(scale))
                        .color(color),
                ),
                None => list.fill_circle(pos, radius * 0.8, color),
            }
        };
        // Players carry a black light of radius 40 ("remove all darkness").
        for a in &self.archers {
            draw(a.pos - Vec2::new(0.0, 10.0), 40.0, Color::BLACK);
        }
        for ar in &self.arrows {
            draw(ar.pos, 20.0, Color::BLACK * 0.8);
        }
        // A coloured light stores the inverted colour (like BGMushroom's
        // inv(B4F9C8)): the lit area keeps that hue.
        let w = self.canvas_size.0 as f32;
        draw(
            Vec2::new(w - 90.0, 100.0),
            40.0,
            invert(Color::hex(0xB4F9C8)),
        );
    }
}

impl Scene for Demo {
    fn game_size(&self) -> (u32, u32) {
        self.opts.game_size
    }

    fn load(&mut self, ctx: &mut LoadContext<'_>) -> Result<()> {
        self.prepare_canvases(ctx.gpu, ctx.renderer);
        let tex = ctx.renderer.create_texture(ctx.gpu, &debug_font_image());
        self.debug_font = Some(debug_font(tex));
        if !self.opts.no_assets {
            match self.load_content(ctx) {
                Ok(c) => self.content = c,
                Err(e) => log::warn!("demo: loading game data failed ({e:#}); using placeholders"),
            }
        }
        if self.sfx_shoot.is_none() {
            self.sfx_shoot = Some(Arc::new(SoundData::sine(880.0, 0.08, 44100, 0.3)));
            self.sfx_jump = Some(Arc::new(SoundData::sine(440.0, 0.06, 44100, 0.3)));
        }
        if let Some(path) = &self.opts.music_wav {
            let data = Arc::new(crate::audio::load_wav(path)?);
            ctx.audio
                .play_music(Box::new(PcmMusic::looping(data)), 0.8, 1.0);
        }
        Ok(())
    }

    fn tick(&mut self, ctx: &mut TickContext<'_>) {
        self.tick = ctx.tick;
        if self.canvas_size == (0, 0) {
            self.canvas_size = self.opts.game_size;
        }
        self.ensure_archers(ctx.input);
        for i in 0..self.archers.len() {
            let (dir, jump, shoot) = if i == 0 {
                let k = &ctx.input.keyboard;
                let d = |a: Scancode, b: Scancode| {
                    (k.held.contains(a) || k.held.contains(b)) as i32 as f32
                };
                let dir = Vec2::new(
                    d(Scancode::Right, Scancode::D) - d(Scancode::Left, Scancode::A),
                    d(Scancode::Down, Scancode::S) - d(Scancode::Up, Scancode::W),
                );
                let active = !k.held.is_empty();
                if active {
                    (
                        dir,
                        k.pressed.contains(Scancode::Space) || k.pressed.contains(Scancode::C),
                        k.pressed.contains(Scancode::J) || k.pressed.contains(Scancode::X),
                    )
                } else {
                    autopilot(i, ctx.tick)
                }
            } else if let Some(p) = ctx.input.pads.get(i - 1) {
                let (x, y) = p.direction(0.3);
                // Quantise like the original's 8-way aim.
                let q = |v: f32| {
                    if v > 0.5 {
                        1.0
                    } else if v < -0.5 {
                        -1.0
                    } else {
                        0.0
                    }
                };
                (
                    Vec2::new(q(x), q(y)),
                    p.pressed.contains(Button::A),
                    p.pressed.contains(Button::X) || p.pressed.contains(Button::RightShoulder),
                )
            } else {
                autopilot(i, ctx.tick)
            };
            self.step_archer(i, dir, jump, shoot, ctx.audio);
        }
        let w = self.canvas_size.0 as f32;
        self.arrows.retain_mut(|a| {
            a.vel.y += GRAVITY * 0.5;
            a.pos += a.vel;
            a.pos.x = a.pos.x.rem_euclid(w);
            a.life = a.life.saturating_sub(1);
            a.life > 0 && a.pos.y < FLOOR_Y
        });
        self.shake = self.shake.saturating_sub(1);
        // Slow-motion wave: amplitude 0.05 * (1 - TimeRate) with TimeRate
        // easing to 0.5 at 0.01/frame, i.e. 0 -> 0.025 in 50 frames.
        let toggle = ctx.input.keyboard.pressed.contains(Scancode::T)
            || ctx.input.pads.iter().any(|p| p.pressed.contains(Button::Y));
        if toggle {
            self.wave_on = !self.wave_on;
        }
        let target = if self.wave_on { 0.025 } else { 0.0 };
        self.wave_amp += (target - self.wave_amp).clamp(-0.0005, 0.0005);
        if self.wave_amp > 0.0 {
            self.wave_counter = (self.wave_counter + 0.01) % std::f32::consts::TAU;
        }
    }

    fn draw(&mut self, ctx: &mut DrawContext<'_>) {
        let (w, h) = (ctx.game_size.0 as f32, ctx.game_size.1 as f32);
        let (Some(fg), Some(light)) = (self.fg, self.light) else {
            return;
        };
        let list = &mut *ctx.list;
        // 1. Light canvas: white ("subtract everything" = 255 - a theme's
        //    darkness colour), plus light sprites.
        list.begin_pass(Some(light), Some(Color::WHITE));
        self.draw_lights(list);
        // 2. Foreground layer (tiles, archers, arrows) on transparent.
        list.begin_pass(Some(fg), Some(Color::TRANSPARENT));
        self.draw_world(list);
        // 3. The screen: background, lit foreground, HUD.
        list.begin_pass(None, Some(Color::rgb(20, 16, 32)));
        match self.content.as_ref().and_then(|c| c.bg) {
            Some(bg) => list.sprite_rect(bg, Rect::new(0, 0, w as i32, h as i32), Color::WHITE),
            None => {
                for y in 0..(h as i32 / 8) {
                    let t = y as f32 / (h / 8.0);
                    list.rect(
                        0.0,
                        y as f32 * 8.0,
                        w,
                        8.0,
                        Color::rgb(40, 50, 90).lerp(Color::rgb(120, 70, 90), t),
                    );
                }
            }
        }
        list.light_composite(fg, light, LightParams { alpha: DARKNESS });
        // HUD: game-pixel text with the original's font if loaded.
        let title = format!(
            "TOWERFALL-PORT PLATFORM DEMO  {}x{}",
            ctx.game_size.0, ctx.game_size.1
        );
        let font = self
            .content
            .as_ref()
            .and_then(|c| c.font.as_ref())
            .or(self.debug_font.as_ref());
        if let Some(f) = font {
            f.draw_anchored(
                list,
                &title,
                Vec2::new(w / 2.0, 6.0),
                Vec2::new(0.5, 0.0),
                Color::WHITE,
            );
        }
        if let Some(f) = &self.debug_font {
            list.set_blend(Blend::Alpha);
            let info = format!(
                "TICK {}  DEVICES {}  ARROWS {}",
                ctx.tick,
                self.devices,
                self.arrows.len()
            );
            f.draw(
                list,
                &info,
                Vec2::new(4.0, h - 8.0),
                Color::rgb(255, 230, 150),
                1.0,
            );
            for (i, a) in self.archers.iter().enumerate() {
                let col = Color::hex(COLORS[i % COLORS.len()]);
                f.draw_anchored(
                    list,
                    &a.label,
                    a.pos.floor() - Vec2::new(0.0, 26.0),
                    Vec2::new(0.5, 1.0),
                    col,
                );
            }
        }
        // Screen shake via the present offset (the original's
        // `Screen.OffsetAdd` of +-2 px per axis; the image wraps around).
        ctx.present.offset = if self.shake > 0 {
            let k = ctx.tick.wrapping_mul(2_654_435_761);
            let pick = |bit: u64| if k >> bit & 1 == 0 { -2.0 } else { 2.0 };
            Vec2::new(pick(7), pick(13))
        } else {
            Vec2::ZERO
        };
        ctx.present.wave = (self.wave_amp > 0.0).then_some(Wave {
            counter: self.wave_counter,
            amplitude: self.wave_amp,
            frequency: 6.0,
        });
        // Overlay, drawn at window resolution in game pixels: the
        // original's aspect bars just outside the canvas (PadRender)...
        if let Some((l, r)) = self.content.as_ref().and_then(|c| c.aspect) {
            ctx.overlay.sprite_rect(
                l,
                Rect::new(-l.width(), 0, l.width(), h as i32),
                Color::WHITE,
            );
            ctx.overlay
                .sprite_rect(r, Rect::new(w as i32, 0, r.width(), h as i32), Color::WHITE);
        }
        // ...and sharp text (one window pixel per
        // font pixel), bottom-right of the canvas.
        if let Some(f) = &self.debug_font {
            let vp = Viewport::compute(ctx.game_size, ctx.window, ctx.present.mode);
            let k = 1.0 / vp.scale.x.max(1.0);
            let text = format!(
                "OVERLAY {}x{} {:?}",
                ctx.window.0, ctx.window.1, ctx.present.mode
            );
            let size = f.measure(&text) * k;
            f.draw(
                ctx.overlay,
                &text,
                Vec2::new(w - size.x - 2.0, h - size.y - 1.0),
                Color::WHITE,
                k,
            );
        }
    }
}

/// Archers without a device walk back and forth and hop.
fn autopilot(i: usize, tick: u64) -> (Vec2, bool, bool) {
    let t = tick + i as u64 * 37;
    let dir = if (t / 90).is_multiple_of(2) {
        1.0
    } else {
        -1.0
    };
    (Vec2::new(dir, 0.0), t.is_multiple_of(70), t % 50 == 25)
}

impl Demo {
    /// Creates the light-map and foreground canvases at the game size.
    fn prepare_canvases(&mut self, gpu: &tfp_render::Gpu, renderer: &mut tfp_render::Renderer) {
        let size = self.opts.game_size;
        for slot in [&mut self.fg, &mut self.light] {
            if let Some(id) = slot.take() {
                renderer.destroy_texture(id);
            }
            *slot = Some(renderer.create_canvas(gpu, size));
        }
        self.canvas_size = size;
    }
}
