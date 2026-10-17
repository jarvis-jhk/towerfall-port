// Instanced sprite quads (XNA SpriteBatch semantics): fragment =
// texel * tint, colours in gamma space, premultiplied blending in the
// pipeline's blend state. Positions arrive in target pixels.

struct Globals {
    // Output size in pixels.
    target_size: vec2<f32>,
    // Extra mapping applied to positions: out = pos * scale + offset
    // (identity for canvases; game -> window pixels for overlays).
    pre_scale: vec2<f32>,
    pre_offset: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Instance {
    @location(0) p0: vec2<f32>,
    @location(1) ax: vec2<f32>,
    @location(2) ay: vec2<f32>,
    @location(3) uv0: vec2<f32>,
    @location(4) uv1: vec2<f32>,
    @location(5) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    // Texel coordinates.
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let c = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let p = (inst.p0 + inst.ax * c.x + inst.ay * c.y) * globals.pre_scale + globals.pre_offset;
    let ndc = p / globals.target_size * 2.0 - 1.0;
    var out: VsOut;
    out.pos = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = mix(inst.uv0, inst.uv1, c);
    out.color = inst.color;
    return out;
}

fn shade(in: VsOut) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(tex));
    return textureSample(tex, samp, in.uv / dims) * in.color;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return shade(in);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// For sRGB outputs: the hardware re-encodes, so the stored bytes equal the
// gamma-space values (blending then happens in linear light).
@fragment
fn fs_srgb(in: VsOut) -> @location(0) vec4<f32> {
    let c = shade(in);
    return vec4<f32>(to_linear(c.rgb), c.a);
}
