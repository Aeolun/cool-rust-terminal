// ABOUTME: Burn-in shader for phosphor persistence effect.
// ABOUTME: Blends current frame with decayed previous frame for CRT phosphor decay.
// ABOUTME: Supports beam sweep simulation for authentic CRT timing.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct BurnInUniforms {
    decay: f32,            // How much the previous frame fades (0.0 = instant, 0.99 = slow fade)
    brightness: f32,       // Current frame brightness multiplier
    // Beam sweep simulation
    beam_y_start: f32,     // Start of current beam band (0.0-1.0)
    beam_y_end: f32,       // End of current beam band (0.0-1.0)
    current_field: u32,    // 0 = even lines, 1 = odd lines
    interlace_enabled: u32, // 0 = disabled, 1 = enabled
    screen_height: f32,    // Screen height in pixels
    _padding: f32,
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

    // Check if this pixel is in the current beam band
    // Handle wrapping when beam_y_end > 1.0 (band extends past bottom, wraps to top)
    var in_beam_band: bool;
    var position_in_band: f32;
    let band_height = uniforms.beam_y_end - uniforms.beam_y_start;

    if (uniforms.beam_y_end <= 1.0) {
        // Normal case: no wrapping
        in_beam_band = (in.uv.y >= uniforms.beam_y_start) && (in.uv.y < uniforms.beam_y_end);
        position_in_band = (in.uv.y - uniforms.beam_y_start) / band_height;
    } else {
        // Wrapping case: band spans from beam_y_start to 1.0, and 0.0 to (beam_y_end - 1.0)
        let wrap_end = uniforms.beam_y_end - 1.0;
        let in_main_band = in.uv.y >= uniforms.beam_y_start;  // Upper portion (start to 1.0)
        let in_wrap_band = in.uv.y < wrap_end;                 // Wrapped portion (0.0 to wrap_end)
        in_beam_band = in_main_band || in_wrap_band;

        // Calculate position within band accounting for wrap
        if (in_main_band) {
            position_in_band = (in.uv.y - uniforms.beam_y_start) / band_height;
        } else {
            // Wrapped portion: continues from where main band left off
            position_in_band = (1.0 - uniforms.beam_y_start + in.uv.y) / band_height;
        }
    }

    // Check if this pixel is on the current field (for interlacing)
    let screen_y = in.uv.y * uniforms.screen_height;
    let scanline = u32(screen_y);
    let on_current_field = (scanline % 2u) == uniforms.current_field;

    // Determine if this pixel should receive fresh content
    // If interlacing disabled, ignore field check
    let should_paint = in_beam_band && (uniforms.interlace_enabled == 0u || on_current_field);

    // Combine: only add current frame if we're in the beam band (and on correct field)
    var combined: vec3<f32>;
    if (should_paint) {
        // Only "paint" pixels that actually have content (phosphor glow from text)
        // Don't refresh dark background - let it decay naturally
        // This prevents the beam from flashing the background brighter
        let content_brightness = max(current.r, max(current.g, current.b));
        if (content_brightness > 0.05) {
            // The top of the band was painted earlier, so it has decayed more
            // Bottom was just painted, so no decay yet
            // This creates a smooth gradient instead of uniform blocks
            // (position_in_band is calculated above, accounting for wrap)
            let in_band_decay = pow(uniforms.decay, 1.0 - position_in_band);

            // Has content - refresh the phosphor with position-based decay
            combined = max(current * uniforms.brightness * in_band_decay, previous);
        } else {
            // Dark pixel - just decay, don't "refresh" with fresh black
            combined = previous;
        }
    } else {
        // Outside beam or wrong field: just show decayed previous
        combined = previous;
    }

    return vec4<f32>(combined, 1.0);
}
