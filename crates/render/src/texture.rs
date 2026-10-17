//! Texture handles and GPU texture storage.

/// Handle to a texture or canvas owned by the [`Renderer`](crate::Renderer).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureId(pub(crate) u32);

impl TextureId {
    /// A 1x1 opaque white texture (used by shapes).
    pub const WHITE: TextureId = TextureId(0);
}

pub(crate) struct GpuTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// Group 1 of the sprite pipeline (texture + nearest/clamp sampler).
    pub bind_group: wgpu::BindGroup,
    pub size: (u32, u32),
    /// Canvases can be render targets and captured.
    pub canvas: bool,
}

/// Writes a full RGBA8 image into `texture`.
pub(crate) fn write_image(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    pixels: &[u8],
    size: (u32, u32),
) {
    let (w, h) = size;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}
