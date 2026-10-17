//! `tower-sheet`: renders the Versus towers of every install found into
//! contact sheets (`scratch/tower-sheet.png`, `scratch/tower-sheet-8p.png`)
//! through `tfp_assets::look`: real tilesets with the original autotiling and
//! seeds, the theme background (first frame), background decorations,
//! foreground layers and the darkness/light composite. Procedural towers
//! (Cataclysm) are generated with their level seed. Gameplay entities are
//! not drawn.
//!
//! Usage: `tower-sheet [--all] [--out DIR]`
//!   --all   every level of every tower (one row per tower) instead of the
//!           first level of each tower; writes `tower-sheet[-8p]-all.png`.

use std::path::{Path, PathBuf};
use tfp_assets::atlas::AtlasKind;
use tfp_assets::image::{Blend, Rect, RgbaImage};
use tfp_assets::install::{GameInstall, InstallKind, Installs};
use tfp_assets::level::Level;
use tfp_assets::look::{
    BgElement, Decoration, ImageRef, LevelLook, LookAssets, LookOptions, Part, SpriteSet, TileLayer,
};
use tfp_assets::sprite_data::SpriteData;
use tfp_assets::towers::{TowerIndex, game_relative_path, versus_load_seed};
use tfp_assets::xnb::SpriteFont;

const LABEL_H: u32 = 18;
const GAP: u32 = 4;

struct Ctx {
    a: LookAssets,
    main: RgbaImage,
    bg: RgbaImage,
    font: Option<SpriteFont>,
}

fn main() {
    let mut all = false;
    let mut out_dir: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--all" => all = true,
            "--out" => out_dir = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                eprintln!("usage: tower-sheet [--all] [--out DIR]");
                return;
            }
            o => {
                eprintln!("unknown argument {o}");
                std::process::exit(2);
            }
        }
    }
    let out_dir = out_dir.unwrap_or_else(default_out_dir);
    std::fs::create_dir_all(&out_dir).expect("create output dir");
    let installs = Installs::discover();
    if installs.steam.is_none() && installs.eight_player.is_none() {
        eprintln!("no TowerFall install found (set TF_GAME_DIR / TF8_GAME_DIR)");
        std::process::exit(1);
    }
    for inst in installs.iter() {
        let suffix = match inst.kind {
            InstallKind::Steam => "",
            InstallKind::EightPlayer => "-8p",
        };
        let name = format!("tower-sheet{suffix}{}.png", if all { "-all" } else { "" });
        let path = out_dir.join(name);
        match render_install(inst, all) {
            Ok((img, n)) => {
                img.save_png(&path).expect("write PNG");
                println!("{}: {n} levels -> {}", inst.summary(), path.display());
            }
            Err(e) => {
                eprintln!("{}: {e}", inst.summary());
                std::process::exit(1);
            }
        }
    }
}

/// `<workspace>/scratch` (the crate lives in `<workspace>/crates/assets`).
fn default_out_dir() -> PathBuf {
    if let Some(r) = std::env::var_os("TFP_ROOT") {
        return PathBuf::from(r).join("scratch");
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scratch")
}

fn render_install(inst: &GameInstall, all: bool) -> tfp_assets::Result<(RgbaImage, usize)> {
    let a = LookAssets::load(inst)?;
    let ctx = Ctx {
        main: a.main_atlas.load_image()?,
        bg: a.bg_atlas.load_image()?,
        font: SpriteFont::load(&inst.content.misc_dir().join("Archer.xnb")).ok(),
        a,
    };
    let idx = TowerIndex::build(inst)?;
    let towers: Vec<_> = idx.versus.iter().filter(|t| !t.levels.is_empty()).collect();
    let (lw, lh) = (inst.level_width, 240u32);
    let cell_w = lw + GAP;
    let cell_h = lh + LABEL_H + GAP;
    let (cols, rows) = if all {
        let cols = towers.iter().map(|t| t.levels.len()).max().unwrap_or(1) as u32;
        (cols, towers.len() as u32)
    } else {
        (4, (towers.len() as u32).div_ceil(4))
    };
    let mut sheet = RgbaImage::filled(cols * cell_w + GAP, rows * cell_h + GAP, [24, 22, 30, 255]);
    let mut n = 0;
    for (ti, t) in towers.iter().enumerate() {
        let theme = ctx.a.game_data.theme(&t.theme).ok_or_else(|| {
            tfp_assets::Error::format(&t.levels[0], format!("unknown theme {}", t.theme))
        })?;
        let levels: &[PathBuf] = if all { &t.levels } else { &t.levels[..1] };
        for (li, p) in levels.iter().enumerate() {
            let (cx, cy) = if all {
                (li as u32, ti as u32)
            } else {
                (ti as u32 % cols, ti as u32 / cols)
            };
            let (x0, y0) = (GAP + cx * cell_w, GAP + cy * cell_h);
            let level = Level::load(p)?;
            let seed = versus_load_seed(&game_relative_path(inst, p).unwrap_or_default());
            let opts = LookOptions {
                procedural: t.procedural.then_some((seed, false)),
                ..Default::default()
            };
            let look = LevelLook::build(&ctx.a, &level, theme, seed, &opts)?;
            let img = render_look(&ctx, &look);
            sheet.blit(
                &img,
                Rect::new(0, 0, img.width as i32, img.height as i32),
                x0 as i32,
                (y0 + LABEL_H) as i32,
                Blend::Replace,
            );
            let file = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut label = if all && li > 0 {
                file
            } else {
                format!("{}  {}", theme.name, file)
            };
            if t.procedural && li == 0 {
                label.push_str(&format!("  SEED {seed}"));
            }
            if let Some(f) = &ctx.font {
                draw_text(
                    &mut sheet,
                    f,
                    &label,
                    x0 as i32 + 2,
                    y0 as i32 + 3,
                    2,
                    [255, 236, 170, 255],
                );
            }
            n += 1;
        }
    }
    Ok((sheet, n))
}

const WHITE: [f32; 4] = [1.0; 4];

fn alpha(a: f32) -> [f32; 4] {
    [1.0, 1.0, 1.0, a.clamp(0.0, 1.0)]
}

impl Ctx {
    fn atlas_image(&self, k: AtlasKind) -> &RgbaImage {
        match k {
            AtlasKind::Bg => &self.bg,
            _ => &self.main,
        }
    }

    fn sprites(&self, set: SpriteSet) -> &SpriteData {
        match set {
            SpriteSet::Main => &self.a.sprites,
            SpriteSet::Bg => &self.a.bg_sprites,
        }
    }

    /// Draws frame `index` of animation `anim` of a sprite at `pos`.
    #[allow(clippy::too_many_arguments)]
    fn sprite(
        &self,
        dst: &mut RgbaImage,
        set: SpriteSet,
        id: &str,
        anim: &str,
        index: u32,
        pos: (f32, f32),
        flip: bool,
        tint: [f32; 4],
    ) {
        let sd = self.sprites(set);
        let Some(def) = sd.get(id) else { return };
        let atlas = match set {
            SpriteSet::Main => &self.a.main_atlas,
            SpriteSet::Bg => &self.a.bg_atlas,
        };
        let Some(tex) = atlas.get(&def.texture) else {
            return;
        };
        let frames = def.frame_rects(tex);
        let f = def
            .animation(anim)
            .and_then(|a| {
                a.frames
                    .get(index as usize % a.frames.len().max(1))
                    .copied()
            })
            .unwrap_or(0);
        let Some(&r) = frames.get(f.max(0) as usize) else {
            return;
        };
        let src = match set {
            SpriteSet::Main => &self.main,
            SpriteSet::Bg => &self.bg,
        };
        let p = (pos.0 + def.position.0, pos.1 + def.position.1);
        dst.draw(src, r, p, def.origin, (1.0, 1.0), 0.0, flip, tint);
    }
}

fn draw_ref(
    ctx: &Ctx,
    dst: &mut RgbaImage,
    i: &ImageRef,
    pos: (f32, f32),
    origin: (f32, f32),
    flip: bool,
    tint: [f32; 4],
) {
    dst.draw(
        ctx.atlas_image(i.atlas),
        i.src,
        (pos.0.floor(), pos.1.floor()),
        origin,
        (1.0, 1.0),
        0.0,
        flip,
        tint,
    );
}

/// Tiles an image over the whole target starting at (x, y), per axis.
#[allow(clippy::too_many_arguments)]
fn draw_tiled(
    ctx: &Ctx,
    dst: &mut RgbaImage,
    i: &ImageRef,
    x: f32,
    y: f32,
    tx: bool,
    ty: bool,
    tint: [f32; 4],
) {
    let (w, h) = (i.src.w.max(1) as f32, i.src.h.max(1) as f32);
    let xs: Vec<f32> = if tx {
        let mut s = x.rem_euclid(w) - w;
        let mut v = Vec::new();
        while s < dst.width as f32 {
            v.push(s);
            s += w;
        }
        v
    } else {
        vec![x]
    };
    let ys: Vec<f32> = if ty {
        let mut s = y.rem_euclid(h) - h;
        let mut v = Vec::new();
        while s < dst.height as f32 {
            v.push(s);
            s += h;
        }
        v
    } else {
        vec![y]
    };
    for &yy in &ys {
        for &xx in &xs {
            draw_ref(ctx, dst, i, (xx, yy), (0.0, 0.0), false, tint);
        }
    }
}

/// First frame of a background/foreground element.
fn draw_element(ctx: &Ctx, dst: &mut RgbaImage, e: &BgElement) {
    use std::f32::consts::PI;
    match e {
        BgElement::Backdrop {
            image,
            x,
            y,
            opacity,
        } => {
            // The 8-player data repeats 320-wide pictures itself; draw as given.
            draw_ref(
                ctx,
                dst,
                image,
                (*x, *y),
                (0.0, 0.0),
                false,
                alpha(*opacity),
            );
        }
        BgElement::Scroll {
            image,
            x,
            y,
            opacity,
            tile_x,
            tile_y,
            ..
        } => {
            draw_tiled(ctx, dst, image, *x, *y, *tile_x, *tile_y, alpha(*opacity));
        }
        BgElement::Wavy {
            image,
            x,
            y,
            opacity,
            amplitude,
            slice_size,
            slice_add,
            horizontal,
            ..
        } => {
            let step = PI * 2.0 * slice_add;
            let src = ctx.atlas_image(image.atlas);
            let s = (*slice_size).max(1);
            let (w, h) = (image.src.w, image.src.h);
            let mut n = 0;
            let mut o = 0;
            while o < if *horizontal { w } else { h } {
                let off = ((step * n as f32).sin() * amplitude).round();
                let (r, p) = if *horizontal {
                    (
                        Rect::new(image.src.x + o, image.src.y, s.min(w - o), h),
                        (x.floor() + o as f32, y.floor() + off),
                    )
                } else {
                    (
                        Rect::new(image.src.x, image.src.y + o, w, s.min(h - o)),
                        (x.floor() + off, y.floor() + o as f32),
                    )
                };
                dst.draw(
                    src,
                    r,
                    p,
                    (0.0, 0.0),
                    (1.0, 1.0),
                    0.0,
                    false,
                    alpha(*opacity),
                );
                o += s;
                n += 1;
            }
        }
        BgElement::Animated {
            sprite,
            x,
            y,
            opacity,
        } => ctx.sprite(
            dst,
            SpriteSet::Bg,
            sprite,
            "0",
            0,
            (*x, *y),
            false,
            alpha(*opacity),
        ),
        BgElement::Fade {
            sprite,
            opacity,
            opacity_range,
            init_frame,
            init_phase,
            init_pos,
            ..
        } => {
            let a = opacity + opacity_range * init_phase.sin();
            ctx.sprite(
                dst,
                SpriteSet::Bg,
                sprite,
                "0",
                *init_frame as u32,
                *init_pos,
                false,
                alpha(a),
            );
        }
        BgElement::FadeMove { .. }
        | BgElement::LightningFlash { .. }
        | BgElement::AscensionLightning { .. } => {}
        BgElement::Node {
            sprite,
            opacity,
            nodes,
            init_node,
            init_pos,
            ..
        } => {
            let anim = nodes[*init_node].anim.to_string();
            ctx.sprite(
                dst,
                SpriteSet::Bg,
                sprite,
                &anim,
                0,
                *init_pos,
                false,
                alpha(*opacity),
            );
        }
        BgElement::Ocean {
            sprite,
            x,
            y,
            opacity,
            ..
        } => ctx.sprite(
            dst,
            SpriteSet::Bg,
            sprite,
            "0",
            0,
            (*x, *y),
            false,
            alpha(*opacity),
        ),
        BgElement::SacredGroundMoon { x, y, image, .. }
        | BgElement::FlightMoon { x, y, image, .. } => {
            draw_ref(ctx, dst, image, (*x, *y), (0.0, 0.0), false, WHITE);
        }
        BgElement::GhostShip {
            image,
            x,
            y,
            scale,
            init_phases,
        } => {
            let [sx, sy, sr, sa] = init_phases.map(f32::sin);
            let a = 0.2 + 0.3 * sa;
            if a > 0.0 {
                let o = (image.src.w as f32 * 0.5, image.src.h as f32);
                dst.draw(
                    ctx.atlas_image(image.atlas),
                    image.src,
                    (x + sx * 80.0, y + sy * 10.0),
                    o,
                    (*scale, *scale),
                    PI / 36.0 * sr,
                    false,
                    alpha(a),
                );
            }
        }
        BgElement::Vortex {
            ring,
            init_angles,
            init_bezier,
            ..
        } => {
            dst.fill_rect(
                Rect::new(0, 0, dst.width as i32, dst.height as i32),
                [4, 2, 5, 255],
            );
            let [b, e, c] = *init_bezier;
            for (i, ang) in init_angles.iter().enumerate() {
                let v = i as f32 / 16.0;
                let t = v * v * v;
                let u = 1.0 - t;
                let p = (
                    u * u * b.0 + 2.0 * u * t * c.0 + t * t * e.0,
                    u * u * b.1 + 2.0 * u * t * c.1 + t * t * e.1,
                );
                let scale = 0.05 + (2.0 - 0.05) * t;
                let g = 0.02 + (0.45 - 0.02) * t;
                let o = (ring.src.w as f32 / 2.0, ring.src.h as f32 / 2.0);
                dst.draw(
                    ctx.atlas_image(ring.atlas),
                    ring.src,
                    p,
                    o,
                    (scale, scale),
                    *ang,
                    false,
                    [g, g, g, 1.0],
                );
            }
        }
    }
}

fn draw_layer(ctx: &Ctx, dst: &mut RgbaImage, layer: &TileLayer) {
    let mut canvas = RgbaImage::new(dst.width, dst.height);
    for t in &layer.tiles {
        canvas.blit(&ctx.main, t.src, t.x, t.y, Blend::Replace);
    }
    for s in &layer.stamps {
        draw_ref(
            ctx,
            &mut canvas,
            &s.image,
            (s.x as f32, s.y as f32),
            (0.0, 0.0),
            false,
            WHITE,
        );
    }
    dst.blit(
        &canvas,
        Rect::new(0, 0, dst.width as i32, dst.height as i32),
        0,
        0,
        Blend::Alpha,
    );
}

fn draw_decoration(ctx: &Ctx, dst: &mut RgbaImage, d: &Decoration) {
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
                ctx.sprite(
                    dst,
                    *set,
                    id,
                    anim,
                    *start_index,
                    (d.x + offset.0, d.y + offset.1),
                    *flip_x,
                    WHITE,
                );
            }
            Part::Image {
                image,
                offset,
                origin,
                flip_x,
                alpha: a,
            } => {
                draw_ref(
                    ctx,
                    dst,
                    image,
                    (d.x + offset.0, d.y + offset.1),
                    *origin,
                    *flip_x,
                    alpha(*a),
                );
            }
            Part::Strips { image, strip } => {
                let (w, h) = (image.src.w, image.src.h);
                for i in 0..h / strip {
                    let r = Rect::new(image.src.x, image.src.y + h - (i + 1) * strip, w, *strip);
                    let pos = (d.x - w as f32 / 2.0, d.y - ((i + 1) * strip) as f32);
                    dst.draw(
                        &ctx.main,
                        r,
                        (pos.0.floor(), pos.1.floor()),
                        (0.0, 0.0),
                        (1.0, 1.0),
                        0.0,
                        false,
                        WHITE,
                    );
                }
            }
            Part::Banner { image, init_phase } => {
                let h = image.src.h;
                let mut y = 0;
                let mut n = 0;
                while y < h {
                    let sh = 4.min(h - y);
                    let t = y as f32 / h as f32;
                    let off =
                        ((init_phase + std::f32::consts::FRAC_PI_4 * n as f32).sin() * 2.5 * t)
                            .round();
                    let r = Rect::new(image.src.x, image.src.y + y, image.src.w, sh);
                    dst.draw(
                        &ctx.main,
                        r,
                        (d.x.floor() - 10.0 + off, d.y.floor() + y as f32),
                        (0.0, 0.0),
                        (1.0, 1.0),
                        0.0,
                        false,
                        WHITE,
                    );
                    y += sh;
                    n += 1;
                }
            }
        }
    }
}

/// Composites a level the way `Level.CoreRender` does (first frame).
fn render_look(ctx: &Ctx, look: &LevelLook) -> RgbaImage {
    let (w, h) = (look.width, look.height);
    let mut screen = RgbaImage::filled(w, h, [0, 0, 0, 255]);
    for e in &look.background {
        draw_element(ctx, &mut screen, e);
    }
    let mut fg = RgbaImage::new(w, h);
    // Layer −3: BG tiles (depth 10000, added first) and back decorations.
    let mut back: Vec<&Decoration> = look.decorations.iter().filter(|d| d.layer == -3).collect();
    back.sort_by_key(|d| std::cmp::Reverse(d.depth));
    draw_layer(ctx, &mut fg, &look.bg_tiles);
    for d in back {
        draw_decoration(ctx, &mut fg, d);
    }
    // Layer 0 by depth, solid tiles at −150.
    let mut front: Vec<&Decoration> = look.decorations.iter().filter(|d| d.layer != -3).collect();
    front.sort_by_key(|d| std::cmp::Reverse(d.depth));
    let mut solids_drawn = false;
    for d in front {
        if !solids_drawn && d.depth < -150 {
            draw_layer(ctx, &mut fg, &look.solid_tiles);
            solids_drawn = true;
        }
        draw_decoration(ctx, &mut fg, d);
    }
    if !solids_drawn {
        draw_layer(ctx, &mut fg, &look.solid_tiles);
    }
    for e in &look.foreground {
        draw_element(ctx, &mut fg, e);
    }
    // Light canvas (sine 0) and the subtractive darkness composite.
    let l = &look.lighting;
    let mut canvas: Vec<[f32; 3]> = vec![
        [
            l.clear_color[0] as f32 / 255.0,
            l.clear_color[1] as f32 / 255.0,
            l.clear_color[2] as f32 / 255.0
        ];
        (w * h) as usize
    ];
    for light in &l.lights {
        let mut spots = vec![(light.x.floor(), light.y.floor())];
        if light.wrap {
            spots.push((
                light.x.floor()
                    + if light.x < w as f32 / 2.0 {
                        w as f32
                    } else {
                        -(w as f32)
                    },
                light.y.floor(),
            ));
            spots.push((
                light.x.floor(),
                light.y.floor() + if light.y < 120.0 { 240.0 } else { -240.0 },
            ));
        }
        let c = light.color.map(|v| v as f32 / 255.0 * light.alpha);
        let r = light.radius.max(0.1);
        for (lx, ly) in spots {
            let (xa, xb) = (
                ((lx - r).floor() as i32).max(0),
                ((lx + r).ceil() as i32).min(w as i32),
            );
            let (ya, yb) = (
                ((ly - r).floor() as i32).max(0),
                ((ly + r).ceil() as i32).min(h as i32),
            );
            for y in ya..yb {
                for x in xa..xb {
                    let d = ((x as f32 + 0.5 - lx).powi(2) + (y as f32 + 0.5 - ly).powi(2)).sqrt();
                    let k = (1.0 - d / r).max(0.0);
                    if k > 0.0 {
                        let px = &mut canvas[(y as u32 * w + x as u32) as usize];
                        for ch in 0..3 {
                            px[ch] = k * c[ch] + px[ch] * (1.0 - k * c[3]);
                        }
                    }
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let f = fg.get(x, y);
            let a = f[3] as f32 / 255.0;
            let lc = canvas[(y * w + x) as usize];
            let s = screen.get(x, y);
            let mut o = [0u8; 4];
            for ch in 0..3 {
                let premul = f[ch] as f32 / 255.0 * a;
                let lit = (premul - l.alpha * lc[ch]).clamp(0.0, 1.0);
                let v = lit + s[ch] as f32 / 255.0 * (1.0 - a);
                o[ch] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            o[3] = 255;
            screen.put(x, y, o);
        }
    }
    screen
}

/// Draws `text` with a sprite font, scaled by an integer factor, tinted.
fn draw_text(
    img: &mut RgbaImage,
    f: &SpriteFont,
    text: &str,
    x: i32,
    y: i32,
    scale: i32,
    color: [u8; 4],
) {
    let mut pen = 0f32;
    for (i, ch) in text.chars().enumerate() {
        let Some(g) = f
            .glyph_index(ch)
            .or_else(|| f.default_character.and_then(|d| f.glyph_index(d)))
        else {
            continue;
        };
        if i > 0 {
            pen += f.spacing;
        }
        let [left, width, right] = f.kerning[g];
        pen += left;
        let src = f.glyphs[g];
        let crop = f.cropping[g];
        let gx = x + ((pen + crop.x as f32) as i32) * scale;
        let gy = y + crop.y * scale;
        for sy in 0..src.h {
            for sx in 0..src.w {
                if f.texture.get((src.x + sx) as u32, (src.y + sy) as u32)[3] < 128 {
                    continue;
                }
                for oy in 0..scale {
                    for ox in 0..scale {
                        let (px, py) = (gx + sx * scale + ox, gy + sy * scale + oy);
                        if px >= 0 && py >= 0 && (px as u32) < img.width && (py as u32) < img.height
                        {
                            img.put(px as u32, py as u32, color);
                        }
                    }
                }
            }
        }
        pen += width + right;
    }
}
