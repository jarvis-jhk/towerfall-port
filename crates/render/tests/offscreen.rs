//! Offscreen rendering of known scenes, checked pixel by pixel.
//!
//! Needs a GPU adapter (any Vulkan/GL device, or lavapipe). Without one the
//! tests print a note and pass, so machines without GPUs are not blocked.

use glam::{Affine2, Vec2};
use tfp_render::*;

fn setup() -> Option<(Gpu, Renderer)> {
    match Gpu::headless() {
        Ok(g) => {
            let r = Renderer::new(&g);
            Some((g, r))
        }
        Err(e) => {
            eprintln!("skipping: {e}");
            None
        }
    }
}

#[track_caller]
fn assert_px(img: &Image, x: u32, y: u32, want: [u8; 4]) {
    let got = img.pixel(x, y);
    let ok = got
        .iter()
        .zip(want)
        .all(|(&a, b)| (a as i32 - b as i32).abs() <= 1);
    assert!(ok, "pixel ({x},{y}) = {got:?}, want {want:?}");
}

fn save(img: &Image, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    img.save_png(dir.join(name)).unwrap();
}

/// A 4x2 test texture: red, green, blue, white / transparent, half-alpha
/// grey (straight), black, yellow.
fn test_texture() -> Image {
    let mut img = Image::new(4, 2);
    let px = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
        [9, 9, 9, 0],
        [100, 100, 100, 128],
        [0, 0, 0, 255],
        [255, 255, 0, 255],
    ];
    for (i, p) in px.iter().enumerate() {
        img.set_pixel(i as u32 % 4, i as u32 / 4, *p);
    }
    img.fna_clear_transparent();
    img
}

#[test]
fn shapes_and_sprites() {
    let Some((gpu, mut r)) = setup() else { return };
    let screen = r.create_canvas(&gpu, (320, 240));
    let tex = r.create_texture(&gpu, &test_texture());
    let sub = SubTexture::new(tex, Rect::new(0, 0, 4, 2));
    let mut dl = DrawList::new();
    dl.begin_pass(None, Some(Color::rgb(10, 20, 30)));
    dl.rect(10.0, 10.0, 5.0, 3.0, Color::RED);
    dl.hollow_rect(30.0, 10.0, 6.0, 6.0, Color::LIME);
    dl.line(
        Vec2::new(0.0, 100.5),
        Vec2::new(50.0, 100.5),
        Color::WHITE,
        1.0,
    );
    // Plain, flipped, scaled, tinted.
    dl.sprite_at(sub, Vec2::new(100.0, 50.0));
    dl.sprite(
        sub,
        &SpriteParams::at(Vec2::new(110.0, 50.0)).flip(true, false),
    );
    dl.sprite(
        sub,
        &SpriteParams::at(Vec2::new(120.0, 50.0)).scale(Vec2::splat(2.0)),
    );
    dl.sprite(
        sub,
        &SpriteParams::at(Vec2::new(130.0, 50.0)).color(Color::WHITE * 0.5),
    );
    // Origin + 90 degree rotation: (0,0) of the source ends up at x=141.
    dl.sprite(
        sub,
        &SpriteParams::at(Vec2::new(140.0, 60.0))
            .origin(Vec2::new(0.0, 1.0))
            .rotation(std::f32::consts::FRAC_PI_2),
    );
    // Camera transform for a batch.
    dl.set_state(
        Blend::Alpha,
        Affine2::from_translation(Vec2::new(-100.0, 100.0)),
    );
    dl.rect(300.0, 0.0, 2.0, 2.0, Color::BLUE);
    dl.set_state(Blend::Additive, Affine2::IDENTITY);
    dl.rect(10.0, 10.0, 1.0, 1.0, Color::rgb(0, 0, 255));
    r.render(&gpu, &dl, screen).unwrap();
    let img = r.capture(&gpu, screen).unwrap();
    save(&img, "shapes_and_sprites.png");

    let bg = [10, 20, 30, 255];
    assert_px(&img, 0, 0, bg);
    assert_px(&img, 11, 11, [255, 0, 0, 255]);
    assert_px(&img, 10, 10, [255, 0, 255, 255]); // additive blue on red
    assert_px(&img, 15, 11, bg);
    assert_px(&img, 30, 10, [0, 255, 0, 255]);
    assert_px(&img, 32, 12, bg); // hollow inside
    assert_px(&img, 35, 15, [0, 255, 0, 255]);
    assert_px(&img, 25, 100, [255, 255, 255, 255]);
    assert_px(&img, 25, 101, bg);
    // Plain sprite.
    assert_px(&img, 100, 50, [255, 0, 0, 255]);
    assert_px(&img, 103, 50, [255, 255, 255, 255]);
    assert_px(&img, 100, 51, bg); // transparent texel
    // Premultiplied blend of the straight half-alpha texel: src + dst*(1-a).
    let half = |c: u8, d: u8| (c as f32 + d as f32 * (1.0 - 128.0 / 255.0)).round() as u8;
    assert_px(
        &img,
        101,
        51,
        [half(100, 10), half(100, 20), half(100, 30), 255],
    );
    // Flipped: white first, red last.
    assert_px(&img, 110, 50, [255, 255, 255, 255]);
    assert_px(&img, 113, 50, [255, 0, 0, 255]);
    // Scaled x2.
    assert_px(&img, 121, 51, [255, 0, 0, 255]);
    assert_px(&img, 122, 50, [0, 255, 0, 255]);
    assert_px(&img, 127, 53, [255, 255, 0, 255]);
    // Tinted with White*0.5 (premultiplied half transparency).
    let t =
        |c: u8, d: u8| (c as f32 * 127.0 / 255.0 + d as f32 * (1.0 - 127.0 / 255.0)).round() as u8;
    assert_px(&img, 130, 50, [t(255, 10), t(0, 20), t(0, 30), 255]);
    // Rotated: source row 0 becomes column 140 going down from y=60.
    assert_px(&img, 140, 60, [255, 0, 0, 255]);
    assert_px(&img, 140, 63, [255, 255, 255, 255]);
    assert_px(&img, 141, 60, bg); // row 1, texel 0 is transparent
    // Transformed batch.
    assert_px(&img, 201, 101, [0, 0, 255, 255]);
    assert!(r.stats().draw_calls >= 3);
}

#[test]
fn lighting_composite_and_canvases() {
    let Some((gpu, mut r)) = setup() else { return };
    let screen = r.create_canvas(&gpu, (64, 32));
    let fg = r.create_canvas(&gpu, (64, 32));
    let light = r.create_canvas(&gpu, (64, 32));
    let mut dl = DrawList::new();
    // Foreground layer: a white bar on transparent.
    dl.begin_pass(Some(fg), Some(Color::TRANSPARENT));
    dl.rect(0.0, 10.0, 64.0, 4.0, Color::WHITE);
    // Light canvas: white ("subtract everything"), a black light at x < 16
    // ("remove all darkness") and a tinted one (inverted colour) at 32..40.
    dl.begin_pass(Some(light), Some(Color::WHITE));
    dl.rect(0.0, 0.0, 16.0, 32.0, Color::BLACK);
    dl.rect(32.0, 0.0, 8.0, 32.0, Color::rgb(0, 255, 255)); // inv(red)
    // Background, then the lit foreground on top.
    dl.begin_pass(None, Some(Color::rgb(0, 0, 100)));
    dl.light_composite(fg, light, LightParams { alpha: 1.0 });
    dl.rect(60.0, 0.0, 2.0, 2.0, Color::RED); // quads after the composite
    r.render(&gpu, &dl, screen).unwrap();
    let img = r.capture(&gpu, screen).unwrap();
    save(&img, "lighting.png");
    assert_px(&img, 5, 11, [255, 255, 255, 255]); // in the light
    assert_px(&img, 50, 11, [0, 0, 0, 255]); // fully darkened, still opaque
    assert_px(&img, 35, 11, [255, 0, 0, 255]); // tinted light keeps red
    assert_px(&img, 50, 20, [0, 0, 100, 255]); // background never darkened
    assert_px(&img, 61, 1, [255, 0, 0, 255]);
    // Subtraction, not multiplication: 255 - 0.5 * 255.
    dl.clear();
    dl.begin_pass(None, Some(Color::BLACK));
    dl.light_composite(fg, light, LightParams { alpha: 0.5 });
    r.render(&gpu, &dl, screen).unwrap();
    let img = r.capture(&gpu, screen).unwrap();
    assert_px(&img, 50, 11, [128, 128, 128, 255]);
}

#[test]
fn present_scales_and_wraps() {
    let Some((gpu, mut r)) = setup() else { return };
    let screen = r.create_canvas(&gpu, (320, 240));
    let mut dl = DrawList::new();
    dl.begin_pass(None, Some(Color::rgb(0, 0, 80)));
    dl.rect(0.0, 0.0, 1.0, 1.0, Color::RED);
    dl.rect(319.0, 239.0, 1.0, 1.0, Color::LIME);
    r.render(&gpu, &dl, screen).unwrap();

    let out = OffscreenTarget::new(&gpu, (1000, 800), OFFSCREEN_FORMAT);
    let font_tex = r.create_texture(&gpu, &debug_font_image());
    let font = debug_font(font_tex);
    let mut overlay = DrawList::new();
    font.draw(&mut overlay, "1", Vec2::new(10.0, 10.0), Color::WHITE, 1.0);
    let settings = PresentSettings {
        mode: ScaleMode::Integer,
        letterbox: Color::rgb(30, 30, 30),
        ..Default::default()
    };
    let vp = r
        .present(&gpu, screen, out.target(), &settings, Some(&overlay))
        .unwrap();
    assert_eq!((vp.x, vp.y, vp.w, vp.h), (20.0, 40.0, 960.0, 720.0));
    let img = out.capture(&gpu).unwrap();
    save(&img, "present.png");
    assert_px(&img, 0, 0, [30, 30, 30, 255]);
    assert_px(&img, 20, 40, [255, 0, 0, 255]);
    assert_px(&img, 22, 42, [255, 0, 0, 255]);
    assert_px(&img, 23, 43, [0, 0, 80, 255]);
    assert_px(&img, 979, 759, [0, 255, 0, 255]);
    assert_px(&img, 980, 760, [30, 30, 30, 255]);
    // Overlay glyph '1' top row ".#." at game (10,10) -> window (20+33, 40+30).
    assert_px(&img, 20 + 33, 40 + 30, [255, 255, 255, 255]);
    assert_px(&img, 20 + 30, 40 + 30, [0, 0, 80, 255]);

    // Shake offset (+1, 0) game px: the image shifts right by 3 window px
    // and the last column wraps around to the left edge.
    let shaken = PresentSettings {
        offset: Vec2::new(1.0, 0.0),
        ..settings
    };
    r.present(&gpu, screen, out.target(), &shaken, None)
        .unwrap();
    let img = out.capture(&gpu).unwrap();
    assert_px(&img, 23, 40, [255, 0, 0, 255]);
    assert_px(&img, 20, 757, [0, 255, 0, 255]);
    assert_px(&img, 22, 40, [0, 0, 80, 255]);

    // Wave: amplitude 0.25 at sin = 1 everywhere shifts every row by a
    // quarter of the width (wrapping), so game x 240 shows canvas x 0.
    let wavy = PresentSettings {
        wave: Some(Wave {
            counter: std::f32::consts::FRAC_PI_2,
            amplitude: 0.25,
            frequency: 0.0,
        }),
        ..settings
    };
    r.present(&gpu, screen, out.target(), &wavy, None).unwrap();
    let img = out.capture(&gpu).unwrap();
    assert_px(&img, 20 + 240 * 3, 40, [255, 0, 0, 255]);
    assert_px(&img, 20, 40, [0, 0, 80, 255]);

    // The default (the original's fractional fit) fills 1000 px wide.
    let vp = r
        .present(
            &gpu,
            screen,
            out.target(),
            &PresentSettings::default(),
            None,
        )
        .unwrap();
    assert_eq!((vp.x, vp.y, vp.w, vp.h), (0.0, 25.0, 1000.0, 750.0));

    // Stretch fills the whole target.
    let stretch = PresentSettings {
        mode: ScaleMode::Stretch,
        ..settings
    };
    r.present(&gpu, screen, out.target(), &stretch, None)
        .unwrap();
    let img = out.capture(&gpu).unwrap();
    assert_px(&img, 0, 0, [255, 0, 0, 255]);
    assert_px(&img, 999, 799, [0, 255, 0, 255]);
}

#[test]
fn wide_canvases() {
    let Some((gpu, mut r)) = setup() else { return };
    for size in [(420, 240), (640, 240)] {
        let c = r.create_canvas(&gpu, size);
        let mut dl = DrawList::new();
        dl.begin_pass(None, Some(Color::BLACK));
        dl.rect(size.0 as f32 - 1.0, 0.0, 1.0, 1.0, Color::WHITE);
        r.render(&gpu, &dl, c).unwrap();
        let img = r.capture(&gpu, c).unwrap();
        assert_eq!((img.width, img.height), size);
        assert_px(&img, size.0 - 1, 0, [255; 4]);
        r.destroy_texture(c);
    }
}
