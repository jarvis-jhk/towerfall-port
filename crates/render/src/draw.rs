//! The draw list: what a frame draws, in order, recorded on the CPU.
//!
//! It mirrors how the original renders through XNA's `SpriteBatch`: a
//! sequence of *passes* (render into a canvas, optionally clearing it), and
//! inside a pass a sequence of textured quads, each drawn with the current
//! blend state and transform (the `SpriteBatch.Begin(blend, ..., matrix)`
//! arguments). Shapes are quads of a white pixel like Monocle's `Draw`.
//! Quads are drawn in submission order (XNA's `Deferred` sort mode);
//! consecutive quads with the same texture and blend become one draw call.

use glam::{Affine2, Vec2};

use crate::color::Color;
use crate::texture::TextureId;

/// An integer rectangle in pixels (XNA `Rectangle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn size(self) -> Vec2 {
        Vec2::new(self.w as f32, self.h as f32)
    }
}

/// A region of a texture (Monocle `Subtexture`: atlas texture + rect).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubTexture {
    pub texture: TextureId,
    pub rect: Rect,
}

impl SubTexture {
    pub const fn new(texture: TextureId, rect: Rect) -> Self {
        Self { texture, rect }
    }

    /// A sub-rectangle relative to this one (Monocle `GetSubtexture`).
    pub fn sub(self, x: i32, y: i32, w: i32, h: i32) -> Self {
        Self::new(
            self.texture,
            Rect::new(self.rect.x + x, self.rect.y + y, w, h),
        )
    }

    pub fn width(self) -> i32 {
        self.rect.w
    }

    pub fn height(self) -> i32 {
        self.rect.h
    }
}

/// Blend states (the XNA ones the original uses, plus the light-layer one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Blend {
    /// XNA `BlendState.AlphaBlend`: premultiplied, `src + dst*(1-src.a)`.
    #[default]
    Alpha,
    /// XNA `BlendState.Additive`: `src*src.a + dst`.
    Additive,
    /// XNA `BlendState.NonPremultiplied`: `src*src.a + dst*(1-src.a)`.
    NonPremultiplied,
    /// XNA `BlendState.Opaque`: `src`.
    Opaque,
    /// Reverse subtract, `dst - src*src.a` on colour, alpha kept (the
    /// original's lighting layer state).
    Subtract,
    /// `src * dst` (multiplies the target by the source colour).
    Multiply,
    /// `dst * (1 - src.a)` on colour and alpha: punches transparent holes
    /// (the Gunn Style overlay's reverse subtract with white shapes over
    /// opaque black, rendering.md 9).
    Erase,
}

impl Blend {
    pub(crate) const ALL: [Blend; 7] = [
        Blend::Alpha,
        Blend::Additive,
        Blend::NonPremultiplied,
        Blend::Opaque,
        Blend::Subtract,
        Blend::Multiply,
        Blend::Erase,
    ];

    pub(crate) fn state(self) -> Option<wgpu::BlendState> {
        use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
        let c = |src_factor, dst_factor, operation| C {
            src_factor,
            dst_factor,
            operation,
        };
        let s = |color, alpha| Some(wgpu::BlendState { color, alpha });
        match self {
            Blend::Alpha => s(
                c(F::One, F::OneMinusSrcAlpha, O::Add),
                c(F::One, F::OneMinusSrcAlpha, O::Add),
            ),
            Blend::Additive => s(
                c(F::SrcAlpha, F::One, O::Add),
                c(F::SrcAlpha, F::One, O::Add),
            ),
            Blend::NonPremultiplied => s(
                c(F::SrcAlpha, F::OneMinusSrcAlpha, O::Add),
                c(F::SrcAlpha, F::OneMinusSrcAlpha, O::Add),
            ),
            Blend::Opaque => None,
            Blend::Subtract => s(
                c(F::SrcAlpha, F::One, O::ReverseSubtract),
                c(F::Zero, F::One, O::Add),
            ),
            Blend::Multiply => s(c(F::Dst, F::Zero, O::Add), c(F::Dst, F::Zero, O::Add)),
            Blend::Erase => s(
                c(F::Zero, F::OneMinusSrcAlpha, O::Add),
                c(F::Zero, F::OneMinusSrcAlpha, O::Add),
            ),
        }
    }
}

/// How one sprite is placed (the arguments of XNA's
/// `SpriteBatch.Draw(texture, position, source, color, rotation, origin,
/// scale, effects)`).
///
/// The quad covers `source` scaled by `scale`, positioned so that `origin`
/// (in source pixels, measured on the un-flipped quad) lands on `position`,
/// rotated by `rotation` radians (clockwise on screen, y down) around it.
/// Flipping mirrors the image inside the quad; the origin is not mirrored
/// (XNA/FNA semantics).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteParams {
    pub position: Vec2,
    pub origin: Vec2,
    pub scale: Vec2,
    pub rotation: f32,
    pub color: Color,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl Default for SpriteParams {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            origin: Vec2::ZERO,
            scale: Vec2::ONE,
            rotation: 0.0,
            color: Color::WHITE,
            flip_x: false,
            flip_y: false,
        }
    }
}

impl SpriteParams {
    pub fn at(position: Vec2) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    pub fn origin(mut self, origin: Vec2) -> Self {
        self.origin = origin;
        self
    }

    pub fn scale(mut self, scale: Vec2) -> Self {
        self.scale = scale;
        self
    }

    pub fn rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn flip(mut self, x: bool, y: bool) -> Self {
        self.flip_x = x;
        self.flip_y = y;
        self
    }
}

/// Parameters of the lighting composite (see [`DrawList::light_composite`]).
///
/// The original's `LightingEffect`: `out.rgb = fg.rgb - alpha * light.a *
/// light.rgb` (clamped), `out.a = fg.a`, blended with [`Blend::Alpha`] over
/// the target. The light canvas is cleared to white (or the theme's
/// inverted darkness colour) and lights are drawn into it as premultiplied
/// tints of the 40x40 `light` cone (black = remove all darkness).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightParams {
    /// Strength of the darkness (the original's lighting `Alpha`, 0..1).
    pub alpha: f32,
}

/// One instanced quad, ready for the GPU. Positions are in target pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct QuadInstance {
    /// Corner at local (0,0).
    pub p0: [f32; 2],
    /// Edge vectors: local x (0,0)->(1,0) and y (0,0)->(0,1).
    pub ax: [f32; 2],
    pub ay: [f32; 2],
    /// Texel coordinates at local (0,0) and (1,1).
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    pub color: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cmd {
    /// Start rendering into `target` (`None` = the frame's default target).
    Pass {
        target: Option<TextureId>,
        clear: Option<Color>,
    },
    Quads {
        texture: TextureId,
        blend: Blend,
        start: u32,
        end: u32,
    },
    Light {
        foreground: TextureId,
        light: TextureId,
        params: LightParams,
    },
}

/// A frame's drawing, recorded in order. Reuse it: [`clear`](Self::clear)
/// keeps the allocations.
#[derive(Clone, Debug)]
pub struct DrawList {
    pub(crate) cmds: Vec<Cmd>,
    pub(crate) quads: Vec<QuadInstance>,
    transform: Affine2,
    blend: Blend,
}

impl Default for DrawList {
    fn default() -> Self {
        Self::new()
    }
}

impl DrawList {
    pub fn new() -> Self {
        Self {
            cmds: Vec::new(),
            quads: Vec::new(),
            transform: Affine2::IDENTITY,
            blend: Blend::Alpha,
        }
    }

    pub fn clear(&mut self) {
        self.cmds.clear();
        self.quads.clear();
        self.transform = Affine2::IDENTITY;
        self.blend = Blend::Alpha;
    }

    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    pub fn quad_count(&self) -> usize {
        self.quads.len()
    }

    /// Starts a pass into `target` (a canvas; `None` is the default target
    /// given to [`Renderer::render`](crate::Renderer::render)), clearing it
    /// first if `clear` is set. Resets transform and blend.
    pub fn begin_pass(&mut self, target: Option<TextureId>, clear: Option<Color>) {
        self.cmds.push(Cmd::Pass { target, clear });
        self.transform = Affine2::IDENTITY;
        self.blend = Blend::Alpha;
    }

    /// Like `SpriteBatch.Begin(blend, ..., matrix)`: state for the quads
    /// that follow. `transform` maps drawing coordinates to target pixels
    /// (a camera matrix; identity for screen space).
    pub fn set_state(&mut self, blend: Blend, transform: Affine2) {
        self.blend = blend;
        self.transform = transform;
    }

    pub fn set_blend(&mut self, blend: Blend) {
        self.blend = blend;
    }

    pub fn set_transform(&mut self, transform: Affine2) {
        self.transform = transform;
    }

    pub fn transform(&self) -> Affine2 {
        self.transform
    }

    fn ensure_pass(&mut self) {
        if self.cmds.is_empty() {
            self.cmds.push(Cmd::Pass {
                target: None,
                clear: None,
            });
        }
    }

    pub(crate) fn push_quad(&mut self, texture: TextureId, q: QuadInstance) {
        self.ensure_pass();
        let i = self.quads.len() as u32;
        self.quads.push(q);
        let blend = self.blend;
        if let Some(Cmd::Quads {
            texture: t,
            blend: b,
            end,
            ..
        }) = self.cmds.last_mut()
            && *t == texture
            && *b == blend
        {
            *end = i + 1;
            return;
        }
        self.cmds.push(Cmd::Quads {
            texture,
            blend,
            start: i,
            end: i + 1,
        });
    }

    /// Draws a region of a texture (`SpriteBatch.Draw` with all arguments).
    pub fn sprite(&mut self, sub: SubTexture, p: &SpriteParams) {
        let size = sub.rect.size() * p.scale;
        let (sin, cos) = p.rotation.sin_cos();
        let rot = |v: Vec2| Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
        let local0 = -p.origin * p.scale;
        let p0 = p.position + rot(local0);
        let ax = rot(Vec2::new(size.x, 0.0));
        let ay = rot(Vec2::new(0.0, size.y));
        // Apply the batch transform (affine: corner, then edge vectors).
        let t = self.transform;
        let p0 = t.transform_point2(p0);
        let ax = t.transform_vector2(ax);
        let ay = t.transform_vector2(ay);
        let r = sub.rect;
        let (mut u0, mut u1) = (r.x as f32, (r.x + r.w) as f32);
        let (mut v0, mut v1) = (r.y as f32, (r.y + r.h) as f32);
        if p.flip_x {
            std::mem::swap(&mut u0, &mut u1);
        }
        if p.flip_y {
            std::mem::swap(&mut v0, &mut v1);
        }
        self.push_quad(
            sub.texture,
            QuadInstance {
                p0: p0.into(),
                ax: ax.into(),
                ay: ay.into(),
                uv0: [u0, v0],
                uv1: [u1, v1],
                color: p.color.packed(),
            },
        );
    }

    /// Draws a region at a position, untinted (`Draw(tex, pos, rect, White)`).
    pub fn sprite_at(&mut self, sub: SubTexture, position: Vec2) {
        self.sprite(sub, &SpriteParams::at(position));
    }

    /// Stretches a region over a destination rectangle (`Draw(tex, destRect,
    /// srcRect, color)`).
    pub fn sprite_rect(&mut self, sub: SubTexture, dest: Rect, color: Color) {
        let scale = Vec2::new(
            dest.w as f32 / sub.rect.w.max(1) as f32,
            dest.h as f32 / sub.rect.h.max(1) as f32,
        );
        self.sprite(
            sub,
            &SpriteParams::at(Vec2::new(dest.x as f32, dest.y as f32))
                .scale(scale)
                .color(color),
        );
    }

    // ---- shapes (Monocle `Draw` style: a white pixel, scaled) ----------

    fn pixel() -> SubTexture {
        SubTexture::new(TextureId::WHITE, Rect::new(0, 0, 1, 1))
    }

    /// Filled rectangle; arguments are truncated to whole pixels like
    /// Monocle's `Draw.Rect`.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let t = |v: f32| v.trunc();
        self.sprite(
            Self::pixel(),
            &SpriteParams::at(Vec2::new(t(x), t(y)))
                .scale(Vec2::new(t(w), t(h)))
                .color(color),
        );
    }

    /// One-pixel outline inside the rectangle.
    pub fn hollow_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.rect(x, y, w, 1.0, color);
        self.rect(x, y + h - 1.0, w, 1.0, color);
        self.rect(x, y + 1.0, 1.0, h - 2.0, color);
        self.rect(x + w - 1.0, y + 1.0, 1.0, h - 2.0, color);
    }

    /// A single pixel.
    pub fn point(&mut self, at: Vec2, color: Color) {
        self.rect(at.x, at.y, 1.0, 1.0, color);
    }

    /// A line of `thickness` from `a` to `b` (a rotated, stretched pixel).
    pub fn line(&mut self, a: Vec2, b: Vec2, color: Color, thickness: f32) {
        let d = b - a;
        self.sprite(
            Self::pixel(),
            &SpriteParams::at(a)
                .scale(Vec2::new(d.length(), thickness))
                .rotation(d.y.atan2(d.x))
                .color(color),
        );
    }

    /// Circle outline made of `segments` lines.
    pub fn circle(&mut self, center: Vec2, radius: f32, color: Color, segments: u32) {
        let n = segments.max(3);
        let at = |i: u32| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            center + Vec2::new(a.cos(), a.sin()) * radius
        };
        for i in 0..n {
            self.line(at(i), at(i + 1), color, 1.0);
        }
    }

    /// Filled circle as pixel-aligned spans (crisp at low resolution).
    pub fn fill_circle(&mut self, center: Vec2, radius: f32, color: Color) {
        let r = radius.max(0.0);
        let y0 = (center.y - r).floor() as i32;
        let y1 = (center.y + r).ceil() as i32;
        for y in y0..y1 {
            let cy = y as f32 + 0.5 - center.y;
            let half = (r * r - cy * cy).max(0.0).sqrt();
            let x0 = (center.x - half).round();
            let x1 = (center.x + half).round();
            if x1 > x0 {
                self.rect(x0, y as f32, x1 - x0, 1.0, color);
            }
        }
    }

    // ---- lighting ------------------------------------------------------

    /// Composites `foreground` (a canvas the size of the current target)
    /// over the current target, darkened by the `light` canvas (the
    /// original's lighting layer). See [`LightParams`].
    pub fn light_composite(
        &mut self,
        foreground: TextureId,
        light: TextureId,
        params: LightParams,
    ) {
        self.ensure_pass();
        self.cmds.push(Cmd::Light {
            foreground,
            light,
            params,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(n: u32) -> TextureId {
        TextureId(n)
    }

    #[test]
    fn batching_merges_runs() {
        let mut dl = DrawList::new();
        let a = SubTexture::new(tex(5), Rect::new(0, 0, 8, 8));
        let b = SubTexture::new(tex(6), Rect::new(0, 0, 8, 8));
        dl.sprite_at(a, Vec2::ZERO);
        dl.sprite_at(a, Vec2::ONE);
        dl.sprite_at(b, Vec2::ONE);
        dl.set_blend(Blend::Additive);
        dl.sprite_at(b, Vec2::ONE);
        // implicit pass + 3 runs
        assert_eq!(dl.cmds.len(), 4);
        assert!(matches!(
            dl.cmds[1],
            Cmd::Quads {
                start: 0,
                end: 2,
                ..
            }
        ));
        assert!(matches!(
            dl.cmds[3],
            Cmd::Quads {
                blend: Blend::Additive,
                ..
            }
        ));
    }

    #[test]
    fn sprite_geometry() {
        let mut dl = DrawList::new();
        let s = SubTexture::new(tex(5), Rect::new(16, 32, 10, 20));
        dl.sprite(
            s,
            &SpriteParams::at(Vec2::new(100.0, 50.0))
                .origin(Vec2::new(5.0, 10.0))
                .scale(Vec2::new(2.0, 1.0))
                .flip(true, false),
        );
        let q = dl.quads[0];
        assert_eq!(q.p0, [90.0, 40.0]);
        assert_eq!(q.ax, [20.0, 0.0]);
        assert_eq!(q.ay, [0.0, 20.0]);
        assert_eq!(q.uv0, [26.0, 32.0]);
        assert_eq!(q.uv1, [16.0, 52.0]);
        // Rotation by 90 degrees (clockwise on a y-down screen).
        dl.sprite(
            s,
            &SpriteParams::at(Vec2::ZERO).rotation(std::f32::consts::FRAC_PI_2),
        );
        let q = dl.quads[1];
        assert!((q.ax[0]).abs() < 1e-5 && (q.ax[1] - 10.0).abs() < 1e-5);
        assert!((q.ay[0] + 20.0).abs() < 1e-5);
        // Transform applies on top.
        dl.set_transform(Affine2::from_translation(Vec2::new(-3.0, 4.0)));
        dl.sprite_at(s, Vec2::ZERO);
        assert_eq!(dl.quads[2].p0, [-3.0, 4.0]);
    }

    #[test]
    fn filled_circle_spans() {
        let mut dl = DrawList::new();
        dl.fill_circle(Vec2::new(10.0, 10.0), 3.0, Color::WHITE);
        assert_eq!(dl.quad_count(), 6);
        let widest = dl.quads.iter().map(|q| q.ax[0]).fold(0.0, f32::max);
        assert_eq!(widest, 6.0);
    }
}
