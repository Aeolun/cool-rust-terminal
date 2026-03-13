// Vertex shader for instanced text/glyph rendering
// A shared unit quad is instanced per-glyph with position, UV, size, and color

struct VertexInput {
    @location(0) quad_pos: vec2<f32>,  // Unit quad corner: (0,0), (1,0), (1,1), (0,1)
}

struct InstanceInput {
    @location(1) glyph_pos: vec2<f32>,   // Screen-space top-left position
    @location(2) glyph_size: vec2<f32>,  // Pixel width/height
    @location(3) uv_pos: vec2<f32>,      // Atlas UV top-left
    @location(4) uv_size: vec2<f32>,     // Atlas UV width/height
    @location(5) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct Uniforms {
    screen_size: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var atlas_texture: texture_2d<f32>;

@group(0) @binding(2)
var atlas_sampler: sampler;

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    // Scale unit quad by glyph size and translate to glyph position
    let pixel_pos = instance.glyph_pos + vertex.quad_pos * instance.glyph_size;

    // Convert from pixel coordinates to clip space (-1 to 1)
    let x = (pixel_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let y = 1.0 - (pixel_pos.y / uniforms.screen_size.y) * 2.0;

    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.tex_coords = instance.uv_pos + vertex.quad_pos * instance.uv_size;
    out.color = instance.color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_texture, atlas_sampler, in.tex_coords).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
