//! `tfp-render`: the wgpu pixel-art renderer of towerfall-port.
//!
//! The game draws into low-resolution *canvases* (the 320x240 / 420x240 /
//! wider game screen, light maps, foreground layers) with a [`DrawList`]
//! that works like XNA's `SpriteBatch`: textured quads from atlas
//! subtextures with tint, flip, rotation, scale and origin, nearest
//! sampling, XNA blend states, per-batch transforms (camera matrices). Then
//! [`Renderer::present`] scales the game canvas into the window (the
//! original's fractional fit, integer or stretch; letterbox colour,
//! wrap-around screen offset for shake, the screen wave effect) and
//! optionally draws an overlay at window resolution.
//!
//! Everything works offscreen: [`Gpu::headless`] needs no window or
//! display server, canvases and [`OffscreenTarget`]s can be captured to an
//! [`Image`] and saved as PNG.
//!
//! Colours are gamma-space bytes blended as they are, like XNA/FNA (no sRGB
//! decoding anywhere); see [`Color`] for the premultiplied tint convention.
//!
//! ```no_run
//! use tfp_render::*;
//! use glam::Vec2;
//! let gpu = Gpu::headless().unwrap();
//! let mut r = Renderer::new(&gpu);
//! let screen = r.create_canvas(&gpu, (320, 240));
//! let mut list = DrawList::new();
//! list.begin_pass(None, Some(Color::hex(0x202030)));
//! list.rect(10.0, 10.0, 50.0, 20.0, Color::RED);
//! r.render(&gpu, &list, screen).unwrap();
//! let window = OffscreenTarget::new(&gpu, (960, 720), OFFSCREEN_FORMAT);
//! r.present(&gpu, screen, window.target(), &PresentSettings::default(), None).unwrap();
//! window.capture(&gpu).unwrap().save_png("scratch/example.png").unwrap();
//! ```

mod color;
mod draw;
mod font;
mod gpu;
mod image;
mod present;
mod renderer;
mod target;
mod texture;

pub use color::Color;
pub use draw::{Blend, DrawList, LightParams, Rect, SpriteParams, SubTexture};
pub use font::{Glyph, SpriteFont, debug_font, debug_font_image};
pub use gpu::{Gpu, new_instance, new_instance_for_display};
pub use image::Image;
pub use present::{PresentSettings, ScaleMode, Viewport, Wave};
pub use renderer::{FrameStats, Renderer};
pub use target::{OFFSCREEN_FORMAT, OffscreenTarget, SurfaceFrame, SurfaceTarget, TargetView};
pub use texture::TextureId;

/// Re-exports so dependents use the same versions.
pub use glam;
pub use wgpu;

/// Renderer errors.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter: {0}")]
    NoAdapter(String),
    #[error("GPU device error: {0}")]
    Device(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("unknown texture id")]
    BadTexture,
    #[error("PNG error: {0}")]
    Png(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
