//! `sim-trace`: run scripted inputs on a real Versus level from the
//! player's install (headless) and write a PNG strip of frames to
//! `scratch/` (level solids, entity sprites from the view, hitboxes).
//!
//! Usage: `sim-trace [level-substring] [out.png]`, e.g.
//! `sim-trace "00 - Sacred Ground/00" scratch/sim-trace.png`.

use std::path::PathBuf;
use tfp_assets::atlas::{Atlas, AtlasKind};
use tfp_assets::image::{Rect, RgbaImage};
use tfp_assets::install::Installs;
use tfp_sim::content::Content;
use tfp_sim::view::{View, ViewOptions, Visual};
use tfp_sim::{MatchSetup, PlayerInput, PlayerSetup, Vec2, World};

const COLS: u32 = 4;
const FRAMES: usize = 16;
const EVERY: usize = 12;

fn script(f: usize) -> [PlayerInput; 2] {
    let d = PlayerInput::dpad;
    const J: u8 = PlayerInput::JUMP;
    const S: u8 = PlayerInput::SHOOT;
    const D: u8 = PlayerInput::DODGE;
    let p0 = match f {
        0..=9 => d(0, 0, 0),
        10..=40 => d(1, 0, 0),
        41..=60 => d(1, 0, J),
        61..=70 => d(1, -1, S),
        71..=75 => d(1, -1, 0),
        76..=90 => d(-1, 0, 0),
        91..=93 => d(-1, 0, D),
        94..=120 => d(0, 0, 0),
        121..=125 => d(0, 0, S),
        126..=140 => d(0, 0, 0),
        141..=170 => d(1, 0, J),
        _ => d(0, 1, 0),
    };
    let p1 = match f {
        0..=20 => d(0, 0, 0),
        21..=50 => d(-1, 0, 0),
        51..=55 => d(-1, 0, J),
        56..=80 => d(-1, 0, 0),
        81..=90 => d(0, 0, S),
        91..=95 => d(0, 0, 0),
        96..=98 => d(1, 0, D),
        99..=130 => d(1, 0, J),
        131..=150 => d(0, 1, 0),
        _ => d(-1, 0, 0),
    };
    [p0, p1]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let want = args
        .next()
        .unwrap_or_else(|| "00 - Sacred Ground/00".into());
    let out = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "scratch/sim-trace.png".into()),
    );
    let installs = Installs::discover();
    let Some(inst) = installs.steam.as_ref().or(installs.eight_player.as_ref()) else {
        eprintln!("no TowerFall install found (set TF_GAME_DIR)");
        std::process::exit(1);
    };
    let levels = tfp_sim::loader::versus_levels(inst).expect("tower index");
    let path = levels
        .iter()
        .find(|p| p.to_string_lossy().replace('\\', "/").contains(&want))
        .cloned()
        .unwrap_or_else(|| levels[0].clone());
    let c =
        tfp_sim::loader::content_from_install(inst, std::slice::from_ref(&path)).expect("content");
    let atlas = Atlas::load(&inst.atlas_xml(AtlasKind::Main).expect("atlas")).expect("atlas xml");
    let atlas_img = atlas.load_image().expect("atlas png");
    let lvl = c.level(0);
    println!(
        "{}: {}x{}, {} spawns, {} jump-thrus, seed {}",
        lvl.name,
        lvl.width,
        lvl.height,
        lvl.player_spawns.len(),
        lvl.jump_thrus.len(),
        lvl.load_seed
    );
    let mut setup = MatchSetup {
        seed: lvl.load_seed,
        ..Default::default()
    };
    for i in 0..2 {
        setup.players[i] = Some(PlayerSetup {
            archer: i as u16,
            arrows: 3,
            shield: i == 1,
            ..Default::default()
        });
    }
    let mut w = World::new(&c, &setup);
    if std::env::var_os("SIM_TRACE_DEBUG").is_some() {
        w.step(&c, &script(0));
        for d in View::build(&w, &c, ViewOptions::default())
            .items
            .iter()
            .take(8)
        {
            if let Visual::Sprite { tex, frame, .. } = d.visual {
                println!(
                    "{} frame {frame} pos {:?} origin {:?} scale {:?}",
                    c.texture(tex).name,
                    d.pos,
                    d.origin,
                    d.scale
                );
            }
        }
        let a = &c.archers[0];
        println!(
            "archer0 {} body {:?} {:?}",
            a.name,
            a.body,
            c.sprite(a.body).map(|s| &s.name)
        );
    }
    let (fw, fh) = (lvl.width, lvl.height);
    let rows = (FRAMES as u32).div_ceil(COLS);
    let mut strip = RgbaImage::filled(fw * COLS, fh * rows, [0, 0, 0, 255]);
    // 4x zoom around player 0: 80x60 crops, 4 per row.
    const ZW: u32 = 80;
    const ZH: u32 = 60;
    let mut zoom = RgbaImage::filled(ZW * 4 * COLS, ZH * 4 * rows, [0, 0, 0, 255]);
    for f in 0..FRAMES * EVERY {
        w.step(&c, &script(f));
        for e in w.events() {
            if let tfp_sim::Event::Death {
                player,
                killer,
                cause,
            } = e
            {
                println!("frame {f}: player {player} died (killer {killer}, cause {cause})");
            }
        }
        if f % EVERY == EVERY - 1 {
            let k = (f / EVERY) as u32;
            let frame = render(&w, &c, &atlas, &atlas_img);
            let center = w
                .player_by_index(0)
                .map(|id| w.core(id).pos)
                .unwrap_or(Vec2::new(fw as f32 / 2.0, fh as f32 / 2.0));
            let x0 = (center.x as i32 - ZW as i32 / 2).clamp(0, (fw - ZW) as i32);
            let y0 = (center.y as i32 - ZH as i32 / 2).clamp(0, (fh - ZH) as i32);
            for zy in 0..ZH * 4 {
                for zx in 0..ZW * 4 {
                    let px = frame.get(x0 as u32 + zx / 4, y0 as u32 + zy / 4);
                    zoom.put((k % COLS) * ZW * 4 + zx, (k / COLS) * ZH * 4 + zy, px);
                }
            }
            strip.blit(
                &frame,
                Rect::new(0, 0, fw as i32, fh as i32),
                ((k % COLS) * fw) as i32,
                ((k / COLS) * fh) as i32,
                tfp_assets::image::Blend::Replace,
            );
            // Frame marker: f/EVERY as white ticks in the corner.
            for t in 0..=k {
                strip.fill_rect(
                    Rect::new(
                        ((k % COLS) * fw + 2 + t * 3) as i32,
                        ((k / COLS) * fh + 2) as i32,
                        2,
                        2,
                    ),
                    [255, 255, 255, 255],
                );
            }
        }
    }
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).ok();
    }
    strip.save_png(&out).expect("write png");
    let zoom_path = out.with_file_name(format!(
        "{}-zoom.png",
        out.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    zoom.save_png(&zoom_path).expect("write zoom png");
    println!("wrote {}", zoom_path.display());
    for id in w.players() {
        let p = w.player(id).unwrap();
        println!(
            "end: player {} at {:?} state {:?} arrows {}",
            p.index,
            w.core(id).pos,
            p.state(),
            p.arrows.len()
        );
    }
    println!("wrote {}", out.display());
}

fn render(w: &World, c: &Content, atlas: &Atlas, img: &RgbaImage) -> RgbaImage {
    let lvl = c.level(w.level);
    let mut out = RgbaImage::filled(lvl.width, lvl.height, [24, 22, 40, 255]);
    for y in 0..lvl.cells_y {
        for x in 0..lvl.cells_x {
            if lvl.solids[y * lvl.cells_x + x] {
                out.fill_rect(
                    Rect::new(x as i32 * 10, y as i32 * 10, 10, 10),
                    [92, 88, 110, 255],
                );
                out.fill_rect(
                    Rect::new(x as i32 * 10, y as i32 * 10, 10, 1),
                    [120, 116, 140, 255],
                );
            }
        }
    }
    let view = View::build(w, c, ViewOptions { hitboxes: true });
    for d in &view.items {
        match d.visual {
            Visual::Sprite {
                tex,
                frame,
                frame_w,
                frame_h,
            } => {
                let Some(r) = atlas.get(&c.texture(tex).name) else {
                    continue;
                };
                let src = if frame_w == 0 || frame < 0 {
                    r
                } else {
                    let cols = (r.w / frame_w as i32).max(1);
                    let i = frame as i32;
                    Rect::new(
                        r.x + (i % cols) * frame_w as i32,
                        r.y + (i / cols) * frame_h as i32,
                        frame_w as i32,
                        frame_h as i32,
                    )
                };
                let mut origin = d.origin;
                let mut scale = d.scale;
                let mut flip = d.flip_x;
                if scale.x < 0.0 {
                    scale.x = -scale.x;
                    origin.x = src.w as f32 - origin.x;
                    flip = !flip;
                }
                let t = d.tint;
                out.draw(
                    img,
                    src,
                    (d.pos.x, d.pos.y),
                    (origin.x, origin.y),
                    (scale.x, scale.y),
                    d.rotation,
                    flip,
                    [
                        t[0] as f32 / 255.0,
                        t[1] as f32 / 255.0,
                        t[2] as f32 / 255.0,
                        t[3] as f32 / 255.0,
                    ],
                );
            }
            Visual::Outline { w: rw, h: rh } => {
                let x0 = (d.pos.x - d.origin.x) as i32;
                let y0 = (d.pos.y - d.origin.y) as i32;
                let (rw, rh) = (rw as i32, rh as i32);
                blend_rect(&mut out, x0, y0, rw, 1, d.tint);
                blend_rect(&mut out, x0, y0 + rh - 1, rw, 1, d.tint);
                blend_rect(&mut out, x0, y0, 1, rh, d.tint);
                blend_rect(&mut out, x0 + rw - 1, y0, 1, rh, d.tint);
            }
            Visual::Rect { w: rw, h: rh } => {
                if d.rotation == 0.0 {
                    let x0 = (d.pos.x - d.origin.x) as i32;
                    let y0 = (d.pos.y - d.origin.y) as i32;
                    blend_rect(&mut out, x0, y0, rw as i32, rh as i32, d.tint);
                } else {
                    // Rotated thin rect (placeholder arrows): plot along the axis.
                    let dir = Vec2::new(d.rotation.cos(), d.rotation.sin());
                    for k in 0..rw as i32 {
                        let p = d.pos + dir * (k as f32 - d.origin.x);
                        blend_rect(&mut out, p.x as i32, p.y as i32, 1, 1, d.tint);
                    }
                }
            }
        }
    }
    out
}

fn blend_rect(img: &mut RgbaImage, x0: i32, y0: i32, w: i32, h: i32, c: [u8; 4]) {
    let a = c[3] as u32;
    for y in y0.max(0)..(y0 + h).min(img.height as i32) {
        for x in x0.max(0)..(x0 + w).min(img.width as i32) {
            let d = img.get(x as u32, y as u32);
            let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * (255 - a)) / 255) as u8;
            img.put(
                x as u32,
                y as u32,
                [mix(c[0], d[0]), mix(c[1], d[1]), mix(c[2], d[2]), 255],
            );
        }
    }
}
