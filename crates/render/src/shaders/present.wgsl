// Presents the game canvas into the window viewport: nearest sampling,
// wrap-around offset (screen shake / scrolling), and the original's
// ScreenEffects wave (docs/spec/rendering.md 8.1), applied in the canvas'
// texture space after the offset like the original's effect on each blit.

struct Params {
    // Viewport origin and size in window pixels.
    viewport: vec4<f32>,
    // Offset in whole window pixels; source size in game pixels.
    offset: vec2<f32>,
    src_size: vec2<f32>,
    // Wave: counter, amplitude (fraction of the width), frequency, unused.
    wave: vec4<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn shade(pos: vec4<f32>) -> vec4<f32> {
    let local = pos.xy - params.viewport.xy - params.offset;
    // Canvas UV of this window pixel, wrapped into [0,1).
    var uv = fract(local / params.viewport.zw);
    if params.wave.y != 0.0 {
        uv.x = fract(uv.x + params.wave.y * sin(uv.y * params.wave.z + params.wave.x));
    }
    // Sample at the centre of the game pixel (robust nearest at any scale).
    let texel = floor(uv * params.src_size) + 0.5;
    let c = textureSampleLevel(src, samp, texel / params.src_size, 0.0);
    return vec4<f32>(c.rgb, 1.0);
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
    return vec4<f32>(to_linear(c.rgb), 1.0);
}
