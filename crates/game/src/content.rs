//! Glue from `tfp-assets` (the original's data) to renderer and mixer
//! types.

use std::collections::BTreeMap;
use std::sync::Arc;

use tfp_assets::atlas::LoadedAtlas;
use tfp_render::{Glyph, Gpu, Image, Rect, Renderer, SpriteFont, SubTexture, TextureId};

use crate::audio::SoundData;

/// An atlas uploaded to the GPU: texture plus named regions.
pub struct GpuAtlas {
    pub texture: TextureId,
    pub regions: BTreeMap<String, Rect>,
}

impl GpuAtlas {
    /// Uploads the image the way FNA loads PNGs (transparent pixels black,
    /// straight alpha otherwise).
    pub fn upload(gpu: &Gpu, renderer: &mut Renderer, atlas: &LoadedAtlas) -> Self {
        let img = &atlas.image;
        let mut image = Image::from_pixels(img.width, img.height, img.pixels.clone());
        image.fna_clear_transparent();
        let texture = renderer.create_texture(gpu, &image);
        let regions = atlas
            .atlas
            .subtextures
            .iter()
            .map(|(k, r)| (k.clone(), Rect::new(r.x, r.y, r.w, r.h)))
            .collect();
        Self { texture, regions }
    }

    pub fn get(&self, name: &str) -> Option<SubTexture> {
        self.regions
            .get(name)
            .map(|&r| SubTexture::new(self.texture, r))
    }
}

/// Uploads an XNB SpriteFont's sheet and builds the renderer's font.
pub fn sprite_font(
    gpu: &Gpu,
    renderer: &mut Renderer,
    f: &tfp_assets::xnb::SpriteFont,
) -> SpriteFont {
    let t = &f.texture;
    // XNA content-pipeline font sheets are premultiplied; the reader keeps
    // them as stored, which is what the AlphaBlend state expects.
    let image = Image::from_pixels(t.width, t.height, t.pixels.clone());
    let texture = renderer.create_texture(gpu, &image);
    let glyphs = f
        .characters
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let b = f.glyphs[i];
            let crop = f.cropping[i];
            (
                c,
                Glyph {
                    bounds: Rect::new(b.x, b.y, b.w, b.h),
                    offset: (crop.x, crop.y),
                    kerning: f.kerning[i],
                },
            )
        })
        .collect();
    SpriteFont {
        texture,
        line_spacing: f.line_spacing,
        spacing: f.spacing,
        glyphs,
        default_char: f.default_character,
    }
}

/// Converts decoded 16-bit PCM to mixer data.
pub fn sound(pcm: &tfp_assets::audio::Pcm) -> Arc<SoundData> {
    Arc::new(SoundData {
        sample_rate: pcm.sample_rate,
        channels: pcm.channels.clamp(1, 2),
        samples: if pcm.channels <= 2 {
            pcm.samples.iter().map(|&s| s as f32 / 32768.0).collect()
        } else {
            pcm.samples
                .chunks_exact(pcm.channels as usize)
                .flat_map(|f| [f[0] as f32 / 32768.0, f[1] as f32 / 32768.0])
                .collect()
        },
    })
}
