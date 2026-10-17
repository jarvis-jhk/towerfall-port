//! The renderer: textures and canvases, pipelines, and the two entry
//! points [`Renderer::render`] (draw list into a canvas) and
//! [`Renderer::present`] (canvas into the window or an offscreen target).

use std::collections::HashMap;

use glam::Vec2;

use crate::RenderError;
use crate::color::Color;
use crate::draw::{Blend, Cmd, DrawList, QuadInstance};
use crate::gpu::Gpu;
use crate::image::Image;
use crate::present::{PresentSettings, Viewport};
use crate::target::{OFFSCREEN_FORMAT, TargetView, capture_texture};
use crate::texture::{GpuTexture, TextureId, write_image};

/// Uniform slots are aligned to this (wgpu's minimum dynamic offset
/// alignment default).
const UNIFORM_ALIGN: usize = 256;

/// Per-frame counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub quads: u32,
    pub draw_calls: u32,
    pub passes: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    target_size: [f32; 2],
    pre_scale: [f32; 2],
    pre_offset: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniform {
    alpha: f32,
    _pad: f32,
    light_ratio: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PresentUniform {
    viewport: [f32; 4],
    offset: [f32; 2],
    src_size: [f32; 2],
    wave: [f32; 4],
}

/// A buffer that grows (to the next power of two) when data does not fit.
struct GrowBuffer {
    buffer: wgpu::Buffer,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl GrowBuffer {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 4096,
                usage,
                mapped_at_creation: false,
            }),
            usage,
            label,
        }
    }

    /// Writes `data`; returns true if the buffer was reallocated.
    fn write(&mut self, gpu: &Gpu, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        let len = data.len() as u64;
        let grew = len > self.buffer.size();
        if grew {
            self.buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: len.next_power_of_two(),
                usage: self.usage,
                mapped_at_creation: false,
            });
        }
        gpu.queue.write_buffer(&self.buffer, 0, data);
        grew
    }
}

/// Uniform data for one submission, 256-byte aligned slots.
#[derive(Default)]
struct Uniforms(Vec<u8>);

impl Uniforms {
    fn push<T: bytemuck::Pod>(&mut self, v: &T) -> u32 {
        let off = self.0.len();
        self.0.extend_from_slice(bytemuck::bytes_of(v));
        self.0.resize(off + UNIFORM_ALIGN, 0);
        off as u32
    }
}

/// Pipelines for one output format.
struct FormatPipelines {
    sprite: HashMap<Blend, wgpu::RenderPipeline>,
    light: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
}

/// Where a list's `None` (default) passes go.
#[derive(Clone, Copy)]
enum DefaultTarget<'a> {
    Canvas(TextureId),
    View(TargetView<'a>),
}

/// A pass resolved before recording.
struct PassPlan {
    target: Option<TextureId>,
    clear: Option<Color>,
    globals_offset: u32,
    /// Range in the list's commands (after the `Pass` command).
    cmds: std::ops::Range<usize>,
}

/// Everything prepared for recording one list.
struct ListPlan {
    passes: Vec<PassPlan>,
    /// Uniform offsets of the light commands, in command order.
    light_offsets: Vec<u32>,
    /// Instance base of this list in the shared instance buffer.
    instance_base: u32,
}

/// The pixel-art renderer.
pub struct Renderer {
    textures: Vec<Option<GpuTexture>>,
    globals_bgl: wgpu::BindGroupLayout,
    texture_bgl: wgpu::BindGroupLayout,
    light_bgl: wgpu::BindGroupLayout,
    present_bgl: wgpu::BindGroupLayout,
    sprite_layout: wgpu::PipelineLayout,
    light_layout: wgpu::PipelineLayout,
    present_layout: wgpu::PipelineLayout,
    sprite_shader: wgpu::ShaderModule,
    light_shader: wgpu::ShaderModule,
    present_shader: wgpu::ShaderModule,
    pipelines: HashMap<wgpu::TextureFormat, FormatPipelines>,
    nearest_clamp: wgpu::Sampler,
    nearest_repeat: wgpu::Sampler,
    instances: GrowBuffer,
    uniforms: GrowBuffer,
    globals_bg: wgpu::BindGroup,
    stats: FrameStats,
    warned_feedback: bool,
}

fn shader(device: &wgpu::Device, label: &str, src: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    })
}

fn tex_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn uniform_entry(
    binding: u32,
    dynamic: bool,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

fn sampler(device: &wgpu::Device, wrap: wgpu::AddressMode) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("nearest"),
        address_mode_u: wrap,
        address_mode_v: wrap,
        address_mode_w: wrap,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

fn uniform_binding(buffer: &wgpu::Buffer, offset: u32, size: usize) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
        buffer,
        offset: offset as u64,
        size: wgpu::BufferSize::new(size as u64),
    })
}

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x2, 5 => Unorm8x4
];

impl Renderer {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let bgl = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let globals_bgl = bgl(
            "globals",
            &[uniform_entry(0, true, wgpu::ShaderStages::VERTEX)],
        );
        let texture_bgl = bgl("texture", &[tex_entry(0, true), sampler_entry(1)]);
        let light_bgl = bgl(
            "light",
            &[
                tex_entry(0, false),
                tex_entry(1, false),
                uniform_entry(2, false, wgpu::ShaderStages::FRAGMENT),
            ],
        );
        let present_bgl = bgl(
            "present",
            &[
                tex_entry(0, true),
                sampler_entry(1),
                uniform_entry(2, false, wgpu::ShaderStages::FRAGMENT),
            ],
        );
        let layout = |label, bgls: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: bgls,
                immediate_size: 0,
            })
        };
        let sprite_layout = layout("sprite", &[Some(&globals_bgl), Some(&texture_bgl)]);
        let light_layout = layout("light", &[Some(&light_bgl)]);
        let present_layout = layout("present", &[Some(&present_bgl)]);
        let uniforms = GrowBuffer::new(device, "uniforms", wgpu::BufferUsages::UNIFORM);
        let globals_bg = Self::make_globals_bg(device, &globals_bgl, &uniforms.buffer);
        let mut r = Self {
            textures: Vec::new(),
            sprite_shader: shader(device, "sprite.wgsl", include_str!("shaders/sprite.wgsl")),
            light_shader: shader(
                device,
                "lighting.wgsl",
                include_str!("shaders/lighting.wgsl"),
            ),
            present_shader: shader(device, "present.wgsl", include_str!("shaders/present.wgsl")),
            globals_bgl,
            texture_bgl,
            light_bgl,
            present_bgl,
            sprite_layout,
            light_layout,
            present_layout,
            pipelines: HashMap::new(),
            nearest_clamp: sampler(device, wgpu::AddressMode::ClampToEdge),
            nearest_repeat: sampler(device, wgpu::AddressMode::Repeat),
            instances: GrowBuffer::new(device, "instances", wgpu::BufferUsages::VERTEX),
            uniforms,
            globals_bg,
            stats: FrameStats::default(),
            warned_feedback: false,
        };
        let white = r.create_texture(gpu, &Image::solid(1, 1, [255; 4]));
        debug_assert_eq!(white, TextureId::WHITE);
        r.ensure_pipelines(gpu, OFFSCREEN_FORMAT);
        r
    }

    fn make_globals_bg(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_binding(buffer, 0, size_of::<Globals>()),
            }],
        })
    }

    fn ensure_pipelines(&mut self, gpu: &Gpu, format: wgpu::TextureFormat) {
        if self.pipelines.contains_key(&format) {
            return;
        }
        let device = &gpu.device;
        let fs = if format.is_srgb() {
            "fs_srgb"
        } else {
            "fs_main"
        };
        let make = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    module: &wgpu::ShaderModule,
                    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                    topology,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let inst = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<QuadInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRS,
        })];
        let sprite = Blend::ALL
            .iter()
            .map(|&b| {
                (
                    b,
                    make(
                        "sprite",
                        &self.sprite_layout,
                        &self.sprite_shader,
                        &inst,
                        wgpu::PrimitiveTopology::TriangleStrip,
                        b.state(),
                    ),
                )
            })
            .collect();
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let light = make(
            "lighting",
            &self.light_layout,
            &self.light_shader,
            &[],
            tri,
            Blend::Alpha.state(),
        );
        let present = make(
            "present",
            &self.present_layout,
            &self.present_shader,
            &[],
            tri,
            None,
        );
        self.pipelines.insert(
            format,
            FormatPipelines {
                sprite,
                light,
                present,
            },
        );
    }

    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    // ---- textures and canvases ------------------------------------------

    fn insert(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        canvas: bool,
        pixels: Option<&[u8]>,
    ) -> TextureId {
        let (w, h) = (size.0.max(1), size.1.max(1));
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        if canvas {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(if canvas { "canvas" } else { "texture" }),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage,
            view_formats: &[],
        });
        if let Some(px) = pixels {
            write_image(&gpu.queue, &texture, px, (w, h));
        }
        let view = texture.create_view(&Default::default());
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture"),
            layout: &self.texture_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.nearest_clamp),
                },
            ],
        });
        let tex = GpuTexture {
            texture,
            view,
            bind_group,
            size: (w, h),
            canvas,
        };
        if let Some(i) = self.textures.iter().skip(1).position(Option::is_none) {
            self.textures[i + 1] = Some(tex);
            TextureId(i as u32 + 1)
        } else {
            self.textures.push(Some(tex));
            TextureId(self.textures.len() as u32 - 1)
        }
    }

    /// Uploads an RGBA8 image (bytes used as they are; see
    /// [`Image::fna_clear_transparent`] for atlas PNGs).
    pub fn create_texture(&mut self, gpu: &Gpu, img: &Image) -> TextureId {
        self.insert(gpu, (img.width, img.height), false, Some(&img.pixels))
    }

    /// Creates a canvas (render target that can also be drawn as a
    /// texture and captured), e.g. the 320x240 game screen or a light map.
    pub fn create_canvas(&mut self, gpu: &Gpu, size: (u32, u32)) -> TextureId {
        self.insert(gpu, size, true, None)
    }

    /// Replaces a texture's pixels (same size).
    pub fn update_texture(
        &mut self,
        gpu: &Gpu,
        id: TextureId,
        img: &Image,
    ) -> Result<(), RenderError> {
        let t = self.get(id)?;
        if t.size != (img.width, img.height) {
            return Err(RenderError::Unsupported(
                "update_texture with a different size".into(),
            ));
        }
        write_image(&gpu.queue, &t.texture, &img.pixels, t.size);
        Ok(())
    }

    /// Textures and canvases currently allocated (leak checks).
    pub fn live_textures(&self) -> usize {
        self.textures.iter().filter(|t| t.is_some()).count()
    }

    /// Frees a texture or canvas; the id may be reused.
    pub fn destroy_texture(&mut self, id: TextureId) {
        if id != TextureId::WHITE
            && let Some(slot) = self.textures.get_mut(id.0 as usize)
        {
            *slot = None;
        }
    }

    pub fn texture_size(&self, id: TextureId) -> Option<(u32, u32)> {
        self.get(id).ok().map(|t| t.size)
    }

    fn get(&self, id: TextureId) -> Result<&GpuTexture, RenderError> {
        self.textures
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or(RenderError::BadTexture)
    }

    /// Reads a canvas back (blocks until the GPU is done).
    pub fn capture(&self, gpu: &Gpu, canvas: TextureId) -> Result<Image, RenderError> {
        let t = self.get(canvas)?;
        if !t.canvas {
            return Err(RenderError::Unsupported(
                "capture of a non-canvas texture".into(),
            ));
        }
        capture_texture(gpu, &t.texture, t.size, OFFSCREEN_FORMAT)
    }

    // ---- frames ---------------------------------------------------------

    /// Renders `list` with `target` (a canvas) as its default target.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        list: &DrawList,
        target: TextureId,
    ) -> Result<(), RenderError> {
        if !self.get(target)?.canvas {
            return Err(RenderError::Unsupported(
                "render target must be a canvas".into(),
            ));
        }
        let mut uniforms = Uniforms::default();
        let plan = self.plan(
            list,
            DefaultTarget::Canvas(target),
            (Vec2::ONE, Vec2::ZERO),
            &mut uniforms,
            0,
        )?;
        self.upload(gpu, &list.quads, &uniforms);
        let lights = self.light_bind_groups(gpu, list, &plan, DefaultTarget::Canvas(target))?;
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        self.stats = FrameStats::default();
        self.record(
            &mut enc,
            list,
            &plan,
            DefaultTarget::Canvas(target),
            &lights,
        )?;
        gpu.queue.submit([enc.finish()]);
        Ok(())
    }

    /// Draws canvas `source` scaled into `out` (window surface or offscreen
    /// target), then `overlay` (if any) in game pixels mapped through the
    /// same viewport but at output resolution (like the original's HUD,
    /// drawn with the screen matrix). Returns the viewport used.
    pub fn present(
        &mut self,
        gpu: &Gpu,
        source: TextureId,
        out: TargetView<'_>,
        settings: &PresentSettings,
        overlay: Option<&DrawList>,
    ) -> Result<Viewport, RenderError> {
        self.ensure_pipelines(gpu, out.format);
        let src_size = self.get(source)?.size;
        let vp = Viewport::compute(src_size, out.size, settings.mode);
        let offset = (settings.offset * vp.scale).round();
        let mut uniforms = Uniforms::default();
        let present_off = uniforms.push(&PresentUniform {
            viewport: [vp.x, vp.y, vp.w, vp.h],
            offset: offset.into(),
            src_size: [src_size.0 as f32, src_size.1 as f32],
            wave: settings
                .wave
                .map_or([0.0; 4], |w| [w.counter, w.amplitude, w.frequency, 0.0]),
        });
        let default = DefaultTarget::View(out);
        let empty = DrawList::new();
        let list = overlay.unwrap_or(&empty);
        let plan = self.plan(
            list,
            default,
            (vp.scale, Vec2::new(vp.x, vp.y)),
            &mut uniforms,
            0,
        )?;
        self.upload(gpu, &list.quads, &uniforms);
        let lights = self.light_bind_groups(gpu, list, &plan, default)?;
        let present_bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &self.present_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.get(source)?.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.nearest_repeat),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_binding(
                        &self.uniforms.buffer,
                        present_off,
                        size_of::<PresentUniform>(),
                    ),
                },
            ],
        });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        self.stats = FrameStats::default();
        {
            let mut pass = begin_pass(&mut enc, "present", out.view, Some(settings.letterbox));
            // The viewport must lie inside the target.
            let (ow, oh) = (out.size.0 as f32, out.size.1 as f32);
            let x0 = vp.x.clamp(0.0, ow);
            let y0 = vp.y.clamp(0.0, oh);
            let x1 = (vp.x + vp.w).clamp(0.0, ow);
            let y1 = (vp.y + vp.h).clamp(0.0, oh);
            if x1 > x0 && y1 > y0 {
                pass.set_viewport(x0, y0, x1 - x0, y1 - y0, 0.0, 1.0);
                pass.set_pipeline(&self.pipelines[&out.format].present);
                pass.set_bind_group(0, &present_bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.stats.passes += 1;
        self.stats.draw_calls += 1;
        self.record(&mut enc, list, &plan, default, &lights)?;
        gpu.queue.submit([enc.finish()]);
        Ok(vp)
    }

    fn resolve<'a>(
        &'a self,
        target: Option<TextureId>,
        default: DefaultTarget<'a>,
    ) -> Result<(&'a wgpu::TextureView, (u32, u32), wgpu::TextureFormat), RenderError> {
        let canvas = |id| -> Result<_, RenderError> {
            let t = self.get(id)?;
            if !t.canvas {
                return Err(RenderError::Unsupported(
                    "pass target must be a canvas".into(),
                ));
            }
            Ok((&t.view, t.size, OFFSCREEN_FORMAT))
        };
        match (target, default) {
            (Some(id), _) | (None, DefaultTarget::Canvas(id)) => canvas(id),
            (None, DefaultTarget::View(v)) => Ok((v.view, v.size, v.format)),
        }
    }

    fn plan(
        &self,
        list: &DrawList,
        default: DefaultTarget<'_>,
        pre: (Vec2, Vec2),
        uniforms: &mut Uniforms,
        instance_base: u32,
    ) -> Result<ListPlan, RenderError> {
        let mut passes = Vec::new();
        let mut light_offsets = Vec::new();
        let mut i = 0;
        while i < list.cmds.len() {
            let Cmd::Pass { target, clear } = list.cmds[i] else {
                unreachable!("draw lists start with a pass")
            };
            let start = i + 1;
            let mut end = start;
            while end < list.cmds.len() && !matches!(list.cmds[end], Cmd::Pass { .. }) {
                end += 1;
            }
            let (_, size, _) = self.resolve(target, default)?;
            // Pre-mapping only applies to the default (overlay) target.
            let (scale, offset) = if target.is_none() {
                pre
            } else {
                (Vec2::ONE, Vec2::ZERO)
            };
            let globals_offset = uniforms.push(&Globals {
                target_size: [size.0 as f32, size.1 as f32],
                pre_scale: scale.into(),
                pre_offset: offset.into(),
                _pad: [0.0; 2],
            });
            for c in &list.cmds[start..end] {
                if let Cmd::Light { light, params, .. } = c {
                    let ls = self.get(*light)?.size;
                    light_offsets.push(uniforms.push(&LightUniform {
                        alpha: params.alpha,
                        _pad: 0.0,
                        light_ratio: [ls.0 as f32 / size.0 as f32, ls.1 as f32 / size.1 as f32],
                    }));
                }
            }
            passes.push(PassPlan {
                target,
                clear,
                globals_offset,
                cmds: start..end,
            });
            i = end;
        }
        Ok(ListPlan {
            passes,
            light_offsets,
            instance_base,
        })
    }

    fn upload(&mut self, gpu: &Gpu, quads: &[QuadInstance], uniforms: &Uniforms) {
        self.instances.write(gpu, bytemuck::cast_slice(quads));
        if self.uniforms.write(gpu, &uniforms.0) {
            self.globals_bg =
                Self::make_globals_bg(&gpu.device, &self.globals_bgl, &self.uniforms.buffer);
        }
    }

    fn light_bind_groups(
        &self,
        gpu: &Gpu,
        list: &DrawList,
        plan: &ListPlan,
        _default: DefaultTarget<'_>,
    ) -> Result<Vec<wgpu::BindGroup>, RenderError> {
        let mut out = Vec::new();
        let lights = list.cmds.iter().filter_map(|c| match c {
            Cmd::Light {
                foreground, light, ..
            } => Some((*foreground, *light)),
            _ => None,
        });
        for ((fg, light), &off) in lights.zip(&plan.light_offsets) {
            out.push(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("light composite"),
                layout: &self.light_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.get(fg)?.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.get(light)?.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform_binding(
                            &self.uniforms.buffer,
                            off,
                            size_of::<LightUniform>(),
                        ),
                    },
                ],
            }));
        }
        Ok(out)
    }

    fn record(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        list: &DrawList,
        plan: &ListPlan,
        default: DefaultTarget<'_>,
        lights: &[wgpu::BindGroup],
    ) -> Result<(), RenderError> {
        let mut stats = self.stats;
        let mut feedback = false;
        let mut light_i = 0;
        for p in &plan.passes {
            let (view, _, format) = self.resolve(p.target, default)?;
            let pipes = &self.pipelines[&format];
            let mut pass = begin_pass(enc, "draw list", view, p.clear);
            stats.passes += 1;
            pass.set_vertex_buffer(0, self.instances.buffer.slice(..));
            pass.set_bind_group(0, &self.globals_bg, &[p.globals_offset]);
            let current = p.target.or(match default {
                DefaultTarget::Canvas(id) => Some(id),
                DefaultTarget::View(_) => None,
            });
            for c in &list.cmds[p.cmds.clone()] {
                match c {
                    Cmd::Quads {
                        texture,
                        blend,
                        start,
                        end,
                    } => {
                        if Some(*texture) == current {
                            feedback = true;
                            continue;
                        }
                        let t = self.get(*texture)?;
                        pass.set_pipeline(&pipes.sprite[blend]);
                        pass.set_bind_group(1, &t.bind_group, &[]);
                        let b = plan.instance_base;
                        pass.draw(0..4, b + start..b + end);
                        stats.quads += end - start;
                        stats.draw_calls += 1;
                    }
                    Cmd::Light {
                        foreground, light, ..
                    } => {
                        let bg = &lights[light_i];
                        light_i += 1;
                        if Some(*foreground) == current || Some(*light) == current {
                            feedback = true;
                            continue;
                        }
                        pass.set_pipeline(&pipes.light);
                        pass.set_bind_group(0, bg, &[]);
                        pass.draw(0..3, 0..1);
                        stats.draw_calls += 1;
                        // Restore the sprite globals for following quads.
                        pass.set_bind_group(0, &self.globals_bg, &[p.globals_offset]);
                    }
                    Cmd::Pass { .. } => unreachable!(),
                }
            }
        }
        if feedback && !self.warned_feedback {
            log::warn!("draw list samples its own render target; those draws were skipped");
            self.warned_feedback = true;
        }
        self.stats = stats;
        Ok(())
    }
}

fn begin_pass<'e>(
    enc: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &'e wgpu::TextureView,
    clear: Option<Color>,
) -> wgpu::RenderPass<'e> {
    let load = match clear {
        Some(c) => wgpu::LoadOp::Clear(c.wgpu()),
        None => wgpu::LoadOp::Load,
    };
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}
