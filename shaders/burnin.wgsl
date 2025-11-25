// ABOUTME: Burn-in shader for phosphor persistence effect.
// ABOUTME: Blends current frame with decayed previous frame for CRT phosphor decay.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct BurnInUniforms {
    decay: f32,        // How much the previous frame fades (0.0 = instant, 0.99 = slow fade)
    brightness: f32,   // Current frame brightness multiplier
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: BurnInUniforms;

@group(0) @binding(1)
var current_texture: texture_2d<f32>;  // Current frame (text rendered)

@group(0) @binding(2)
var previous_texture: texture_2d<f32>; // Previous burn-in state

@group(0) @binding(3)
var tex_sampler: sampler;

// Fullscreen triangle vertices (more efficient than quad)
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;

    // Generate fullscreen triangle
    let x = f32(i32(vertex_index & 1u) * 4 - 1);
    let y = f32(i32(vertex_index >> 1u) * 4 - 1);

    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample current frame
    let current = textureSample(current_texture, tex_sampler, in.uv).rgb;

    // Sample previous burn-in state (decayed)
    let previous = textureSample(previous_texture, tex_sampler, in.uv).rgb * uniforms.decay;

    // Combine: current frame at full brightness, previous frame decayed
    // Use max to preserve the brighter of current or decayed previous
    let combined = max(current * uniforms.brightness, previous);

    return vec4<f32>(combined, 1.0);
}
