// The original's LightingEffect (docs/spec/rendering.md 7.3): draws the
// foreground canvas over the target, darkened by *subtracting*
// Alpha * light.a * light.rgb per channel (alpha kept); the result is
// clamped by the target write and blended with AlphaBlend. Transparent
// foreground pixels stay transparent, so the background is never darkened.

struct Params {
    alpha: f32,
    _pad0: f32,
    // light canvas size / target size
    light_ratio: vec2<f32>,
};

@group(0) @binding(0) var fg: texture_2d<f32>;
@group(0) @binding(1) var light: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn shade(pos: vec4<f32>) -> vec4<f32> {
    let src = textureLoad(fg, vec2<i32>(pos.xy), 0);
    let l = textureLoad(light, vec2<i32>(pos.xy * params.light_ratio), 0);
    let rgb = clamp(src.rgb - params.alpha * (l.a * l.rgb), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, src.a);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return shade(pos);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_srgb(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = shade(pos);
    return vec4<f32>(to_linear(c.rgb), c.a);
}
