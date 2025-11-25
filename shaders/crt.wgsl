// ABOUTME: CRT post-processing shader for retro monitor effects.
// ABOUTME: Applies barrel distortion, scanlines, and bloom to terminal output.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct CrtUniforms {
    screen_size: vec2<f32>,
    time: f32,
    curvature: f32,      // 0.0 = flat, ~0.1 = subtle curve
    scanline_intensity: f32,  // 0.0 = none, 1.0 = full
    bloom_intensity: f32,     // 0.0 = none, 1.0 = strong
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> uniforms: CrtUniforms;

@group(0) @binding(1)
var input_texture: texture_2d<f32>;

@group(0) @binding(2)
var input_sampler: sampler;

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

// Barrel distortion - attempt to curve UV coords like a CRT screen
fn barrel_distort(uv: vec2<f32>, curvature: f32) -> vec2<f32> {
    let centered = uv * 2.0 - 1.0;
    let r2 = dot(centered, centered);
    let distorted = centered * (1.0 + curvature * r2);
    return distorted * 0.5 + 0.5;
}

// Check if UV is outside [0,1] range (for vignette/border)
fn is_outside(uv: vec2<f32>) -> bool {
    return uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0;
}

// Pseudo-random noise
fn noise(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// Scanline effect with subtle movement
fn scanline(uv: vec2<f32>, intensity: f32, screen_height: f32, time: f32) -> f32 {
    // Slow vertical drift (very subtle)
    let drift = time * 0.25;
    let line = sin((uv.y + drift * 0.005) * screen_height * 3.14159) * 0.5 + 0.5;
    return 1.0 - intensity * (1.0 - line * line);
}

// Flicker effect - subtle brightness variation
fn flicker(time: f32) -> f32 {
    let fast = sin(time * 60.0) * 0.005;
    let slow = sin(time * 5.0) * 0.01;
    return 1.0 + fast + slow;
}

// Simple bloom by sampling neighbors
fn bloom(uv: vec2<f32>, texel_size: vec2<f32>) -> vec3<f32> {
    var color = vec3<f32>(0.0);
    let offsets = array<vec2<f32>, 9>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0,  0.0), vec2<f32>(0.0,  0.0), vec2<f32>(1.0,  0.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>(0.0,  1.0), vec2<f32>(1.0,  1.0)
    );
    let weights = array<f32, 9>(
        0.0625, 0.125, 0.0625,
        0.125,  0.25,  0.125,
        0.0625, 0.125, 0.0625
    );

    for (var i = 0u; i < 9u; i = i + 1u) {
        let sample_uv = uv + offsets[i] * texel_size * 2.0;
        color = color + textureSample(input_texture, input_sampler, sample_uv).rgb * weights[i];
    }

    return color;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Apply barrel distortion
    let distorted_uv = barrel_distort(in.uv, uniforms.curvature);

    // Black outside the curved screen area
    if (is_outside(distorted_uv)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Sample the terminal texture
    var color = textureSample(input_texture, input_sampler, distorted_uv).rgb;

    // Add bloom
    if (uniforms.bloom_intensity > 0.0) {
        let texel_size = 1.0 / uniforms.screen_size;
        let bloomed = bloom(distorted_uv, texel_size);
        color = mix(color, bloomed + color * 0.5, uniforms.bloom_intensity * 0.5);
    }

    // Apply animated scanlines
    let scan = scanline(distorted_uv, uniforms.scanline_intensity, uniforms.screen_size.y, uniforms.time);
    color = color * scan;

    // Apply flicker
    color = color * flicker(uniforms.time);

    // Add subtle noise (like old CRT static)
    let noise_val = noise(distorted_uv * uniforms.screen_size + vec2<f32>(uniforms.time * 100.0, 0.0));
    color = color + (noise_val - 0.5) * 0.02;

    // Vignette - darken edges
    let vignette_uv = distorted_uv * 2.0 - 1.0;
    let vignette = 1.0 - dot(vignette_uv, vignette_uv) * 0.2;
    color = color * vignette;

    return vec4<f32>(color, 1.0);
}
