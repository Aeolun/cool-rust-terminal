// ABOUTME: CRT post-processing shader for retro monitor effects.
// ABOUTME: Applies barrel distortion, scanlines, and bloom to terminal output.
// ABOUTME: Supports per-pane mode where each pane gets independent CRT effects.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// Pane rect: x, y, width, height (normalized 0-1)
struct PaneRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

struct CrtUniforms {
    screen_size: vec2<f32>,
    time: f32,
    curvature: f32,           // 0.0 = flat, ~0.1 = subtle curve
    scanline_intensity: f32,  // 0.0 = none, 1.0 = full
    bloom_intensity: f32,     // 0.0 = none, 1.0 = strong
    per_pane_mode: u32,       // 0 = whole screen, 1 = per-pane effects
    pane_count: u32,          // Number of active panes
    focused_pane: i32,        // Index of focused pane (-1 if none)
    focus_glow_radius: f32,   // Corner radius for focus glow
    focus_glow_width: f32,    // How far glow extends inward
    focus_glow_intensity: f32, // Glow brightness
    static_noise: f32,        // Static noise intensity
    flicker: f32,             // Flicker intensity
    brightness: f32,          // Overall brightness
    vignette: f32,            // Vignette intensity (edge darkening)
    // Bezel settings
    bezel_enabled: u32,       // 0 = no bezel, 1 = show bezel
    bezel_size: vec2<f32>,    // Bezel image size (width, height) in pixels
    // 9-patch borders in pixels
    bezel_border_top: f32,
    bezel_border_right: f32,
    bezel_border_bottom: f32,
    bezel_border_left: f32,
    // Content scale - adjusts how big the content is drawn (like H-SIZE/V-SIZE knobs)
    content_scale_x: f32,
    content_scale_y: f32,
    // Cell height in pixels for scanline alignment (one scanline per text row)
    cell_height: f32,
    _pad1: f32,
    // Focus glow color (follows font color) - vec4 for alignment (w ignored)
    glow_color: vec4<f32>,
    // Pane rects (max 16 panes)
    panes: array<PaneRect, 16>,
}

@group(0) @binding(0)
var<uniform> uniforms: CrtUniforms;

@group(0) @binding(1)
var input_texture: texture_2d<f32>;

@group(0) @binding(2)
var input_sampler: sampler;

@group(0) @binding(3)
var bezel_texture: texture_2d<f32>;

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

// Pseudo-random noise - basic hash function
fn noise(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// Temporal noise - changes every frame, no spatial correlation with scanlines
fn temporal_noise(screen_pos: vec2<f32>, time: f32) -> f32 {
    // Use a 3D hash: x, y, and time all contribute to randomness
    // This prevents any fixed spatial patterns from forming
    let p3 = vec3<f32>(screen_pos, time * 60.0);
    let h = fract(sin(dot(p3, vec3<f32>(12.9898, 78.233, 45.164))) * 43758.5453);
    // Add a second hash iteration for better distribution
    return fract(h * 17.0 + sin(dot(p3.zxy, vec3<f32>(93.989, 67.345, 28.764))) * 23421.6312);
}

// Scanline effect - aligned to text rows for readable text
// Each text row gets one scanline cycle, so dark bands fall BETWEEN rows, not through characters
fn scanline(uv: vec2<f32>, intensity: f32, region_height: f32, time: f32) -> f32 {
    // Calculate number of text rows in this region
    // This aligns scanlines to character cells so text stays readable
    let row_count = region_height / uniforms.cell_height;

    // Slow drift - moves in whole scanline increments to avoid moiré
    let drift = time * 0.3; // Slow roll speed

    let row_y = uv.y * row_count + drift;
    let frac_y = fract(row_y);

    // Triangle wave: brightest in middle of row (where text is), darkest at edges (between rows)
    // This keeps text bright while darkening the gaps between lines
    let line_mask = 1.0 - abs(frac_y * 2.0 - 1.0);

    return 1.0 - intensity * (1.0 - line_mask);
}

// Flicker effect - realistic power supply fluctuation (scaled by flicker uniform)
// Real CRT flicker came from power line frequency (~60Hz) with harmonics and noise
fn flicker(time: f32, intensity: f32) -> f32 {
    // Primary power line frequency (60Hz fundamental)
    let power_60hz = sin(time * 60.0 * 6.28318) * 0.15;
    // Second harmonic (120Hz) - rectifier ripple
    let harmonic_120hz = sin(time * 120.0 * 6.28318) * 0.08;
    // Low frequency drift from voltage sag
    let drift = sin(time * 0.5) * 0.1;
    // High frequency noise to break up clean sine patterns
    let noise_time = time * 1000.0;
    let hf_noise = (fract(sin(noise_time) * 43758.5453) - 0.5) * 0.15;

    let total = power_60hz + harmonic_120hz + drift + hf_noise;
    return 1.0 + total * intensity;
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

// Find which pane contains this UV, returns pane index or -1 if none
fn find_pane(uv: vec2<f32>) -> i32 {
    for (var i = 0u; i < uniforms.pane_count; i = i + 1u) {
        let p = uniforms.panes[i];
        if (uv.x >= p.x && uv.x < p.x + p.w && uv.y >= p.y && uv.y < p.y + p.h) {
            return i32(i);
        }
    }
    return -1;
}

// Convert global UV to local pane UV (0-1 within the pane)
fn global_to_local_uv(uv: vec2<f32>, pane_idx: i32) -> vec2<f32> {
    let p = uniforms.panes[pane_idx];
    return vec2<f32>(
        (uv.x - p.x) / p.w,
        (uv.y - p.y) / p.h
    );
}

// Convert local pane UV back to global UV
fn local_to_global_uv(local_uv: vec2<f32>, pane_idx: i32) -> vec2<f32> {
    let p = uniforms.panes[pane_idx];
    return vec2<f32>(
        p.x + local_uv.x * p.w,
        p.y + local_uv.y * p.h
    );
}

// Apply CRT effects for whole-screen mode
fn apply_whole_screen_crt(uv: vec2<f32>) -> vec4<f32> {
    let distorted_uv = barrel_distort(uv, uniforms.curvature);

    // Anti-aliased edge mask instead of hard cutoff
    let edge_alpha = edge_mask_aa(distorted_uv);
    if (edge_alpha <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    var color = textureSample(input_texture, input_sampler, distorted_uv).rgb;

    if (uniforms.bloom_intensity > 0.0) {
        let texel_size = 1.0 / uniforms.screen_size;
        let bloomed = bloom(distorted_uv, texel_size);
        color = mix(color, bloomed + color * 0.5, uniforms.bloom_intensity * 0.5);
    }

    let scan = scanline(distorted_uv, uniforms.scanline_intensity, uniforms.screen_size.y, uniforms.time);
    color = color * scan;
    color = color * flicker(uniforms.time, uniforms.flicker);

    // Static noise - use temporal noise to avoid moiré with scanlines
    let noise_val = temporal_noise(distorted_uv * uniforms.screen_size, uniforms.time);
    color = color + (noise_val - 0.5) * uniforms.static_noise;

    let vignette_uv = distorted_uv * 2.0 - 1.0;
    let vignette = 1.0 - dot(vignette_uv, vignette_uv) * uniforms.vignette;
    color = color * vignette;

    // Apply brightness
    color = color * uniforms.brightness;

    // Apply AA edge fade
    color = color * edge_alpha;

    return vec4<f32>(color, 1.0);
}

// Signed distance to a rounded rectangle (negative inside, positive outside)
// p: point in centered coords, b: half-extents, r: corner radius
fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Calculate edge glow intensity for focused pane
// Rounded rect shape - invisible until near the edge, then fades in
fn edge_glow(local_uv: vec2<f32>, is_focused: bool) -> vec3<f32> {
    if (!is_focused) {
        return vec3<f32>(0.0);
    }

    // Convert UV (0-1) to centered coordinates (-0.5 to 0.5)
    let centered = local_uv - 0.5;

    // Rounded rect parameters from uniforms
    let half_extents = vec2<f32>(0.5, 0.5);
    let corner_radius = uniforms.focus_glow_radius;

    // Distance to edge (negative inside, 0 at edge)
    let dist_to_edge = sd_rounded_box(centered, half_extents, corner_radius);

    // How far inside the glow fades over (in UV space) - from uniforms
    let glow_width = uniforms.focus_glow_width;

    // Full intensity at edge (dist=0), fades to zero glow_width units inside
    let glow_intensity = smoothstep(-glow_width, 0.0, dist_to_edge);

    // Use glow color from uniforms (follows font color)
    return uniforms.glow_color.rgb * glow_intensity * uniforms.focus_glow_intensity;
}

// Anti-aliased edge mask for CRT border (smooth transition to black)
fn edge_mask_aa(uv: vec2<f32>) -> f32 {
    // Distance from edge (negative inside 0-1 range, positive outside)
    let edge_dist = max(
        max(-uv.x, uv.x - 1.0),
        max(-uv.y, uv.y - 1.0)
    );

    // Use fwidth for screen-space anti-aliasing
    let aa = fwidth(edge_dist) * 1.5;

    // Smooth transition: 1.0 inside, 0.0 outside
    return 1.0 - smoothstep(-aa, aa, edge_dist);
}

// Apply CRT effects relative to a single pane
fn apply_per_pane_crt(uv: vec2<f32>, pane_idx: i32) -> vec4<f32> {
    let p = uniforms.panes[pane_idx];
    let pane_size = vec2<f32>(p.w * uniforms.screen_size.x, p.h * uniforms.screen_size.y);
    let is_focused = (pane_idx == uniforms.focused_pane);

    // Convert to local UV within the pane
    let local_uv = global_to_local_uv(uv, pane_idx);

    // Apply barrel distortion in local space
    let distorted_local = barrel_distort(local_uv, uniforms.curvature);

    // Anti-aliased edge mask instead of hard cutoff
    let edge_alpha = edge_mask_aa(distorted_local);
    if (edge_alpha <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Convert back to global UV for sampling
    let sample_uv = local_to_global_uv(distorted_local, pane_idx);

    var color = textureSample(input_texture, input_sampler, sample_uv).rgb;

    // Add edge glow for focused pane BEFORE CRT effects so it gets processed too
    color = color + edge_glow(distorted_local, is_focused);

    // Bloom in local space
    if (uniforms.bloom_intensity > 0.0) {
        let texel_size = 1.0 / uniforms.screen_size;
        let bloomed = bloom(sample_uv, texel_size);
        color = mix(color, bloomed + color * 0.5, uniforms.bloom_intensity * 0.5);
    }

    // Scanlines relative to pane height
    let scan = scanline(distorted_local, uniforms.scanline_intensity, pane_size.y, uniforms.time);
    color = color * scan;

    // Flicker (same for all panes, but could vary per-pane with pane_idx)
    color = color * flicker(uniforms.time + f32(pane_idx) * 0.1, uniforms.flicker);

    // Noise in local space
    // Static noise - use temporal noise to avoid moiré with scanlines
    let noise_val = temporal_noise(distorted_local * pane_size, uniforms.time);
    color = color + (noise_val - 0.5) * uniforms.static_noise;

    // Vignette relative to pane center
    let vignette_uv = distorted_local * 2.0 - 1.0;
    let vignette = 1.0 - dot(vignette_uv, vignette_uv) * uniforms.vignette;
    color = color * vignette;

    // Apply brightness
    color = color * uniforms.brightness;

    // Apply AA edge fade
    color = color * edge_alpha;

    return vec4<f32>(color, 1.0);
}

// ============ BEZEL RENDERING (9-PATCH IMAGE) ============

// 9-patch sampling: maps screen UV to bezel texture UV
// The bezel image has borders that should stay fixed size, while the middle stretches
fn sample_bezel_9patch(screen_uv: vec2<f32>) -> vec4<f32> {
    let screen_w = uniforms.screen_size.x;
    let screen_h = uniforms.screen_size.y;

    // Border sizes in pixels
    let top = uniforms.bezel_border_top;
    let right = uniforms.bezel_border_right;
    let bottom = uniforms.bezel_border_bottom;
    let left = uniforms.bezel_border_left;

    // Bezel image size
    let img_w = uniforms.bezel_size.x;
    let img_h = uniforms.bezel_size.y;

    // Screen position in pixels
    let px = screen_uv.x * screen_w;
    let py = screen_uv.y * screen_h;

    // Calculate texture UV using 9-patch logic
    var tex_u: f32;
    var tex_v: f32;

    // Horizontal: left border, middle stretch, right border
    if (px < left) {
        // Left border - direct mapping
        tex_u = px / img_w;
    } else if (px > screen_w - right) {
        // Right border - map from right edge
        tex_u = (img_w - (screen_w - px)) / img_w;
    } else {
        // Middle - stretch the center region
        let middle_screen = screen_w - left - right;
        let middle_img = img_w - left - right;
        let t = (px - left) / middle_screen;
        tex_u = (left + t * middle_img) / img_w;
    }

    // Vertical: top border, middle stretch, bottom border
    if (py < top) {
        // Top border - direct mapping
        tex_v = py / img_h;
    } else if (py > screen_h - bottom) {
        // Bottom border - map from bottom edge
        tex_v = (img_h - (screen_h - py)) / img_h;
    } else {
        // Middle - stretch the center region
        let middle_screen = screen_h - top - bottom;
        let middle_img = img_h - top - bottom;
        let t = (py - top) / middle_screen;
        tex_v = (top + t * middle_img) / img_h;
    }

    return textureSample(bezel_texture, input_sampler, vec2<f32>(tex_u, tex_v));
}

// Get the screen content area in normalized UV (where terminal content shows through)
fn get_screen_content_rect() -> vec4<f32> {
    let screen_w = uniforms.screen_size.x;
    let screen_h = uniforms.screen_size.y;

    // The content area is inside the bezel borders
    let left = uniforms.bezel_border_left / screen_w;
    let top = uniforms.bezel_border_top / screen_h;
    let right = 1.0 - uniforms.bezel_border_right / screen_w;
    let bottom = 1.0 - uniforms.bezel_border_bottom / screen_h;

    return vec4<f32>(left, top, right, bottom);
}

// Transform window UV to content area UV (used for non-bezel mode)
fn window_to_content_uv(uv: vec2<f32>) -> vec2<f32> {
    let rect = get_screen_content_rect();
    return vec2<f32>(
        (uv.x - rect.x) / (rect.z - rect.x),
        (uv.y - rect.y) / (rect.w - rect.y)
    );
}

// Check if UV is in the content area (where terminal shows)
fn is_in_content_area(uv: vec2<f32>) -> bool {
    let rect = get_screen_content_rect();
    return uv.x >= rect.x && uv.x <= rect.z && uv.y >= rect.y && uv.y <= rect.w;
}

// Alias for get_screen_content_rect (used by fs_main)
fn get_screen_rect() -> vec4<f32> {
    return get_screen_content_rect();
}

// Check if UV is in the bezel area (outside the screen content area)
fn is_in_bezel(uv: vec2<f32>) -> bool {
    return !is_in_content_area(uv);
}

// Transform window UV to screen content UV (for sampling terminal content)
fn window_to_screen_uv(uv: vec2<f32>) -> vec2<f32> {
    return window_to_content_uv(uv);
}

// Render bezel pixel - samples 9-patch bezel texture with optional screen reflection
fn render_bezel(uv: vec2<f32>, screen_sample: vec3<f32>) -> vec4<f32> {
    // Sample the bezel texture using 9-patch scaling
    let bezel_color = sample_bezel_9patch(uv);

    // Mix in a subtle reflection of the screen content on the bezel
    let reflection_strength = 0.05;
    let final_color = bezel_color.rgb + screen_sample * reflection_strength * bezel_color.a;

    return vec4<f32>(final_color, 1.0);
}

// Scale content UV for sampling the input texture
// This adjusts where the character grid is drawn on the screen
// Scale > 1 = content fills more of screen (text appears larger/zoomed)
// Scale < 1 = content fills less of screen (text appears smaller)
// The CRT "glass" shape stays fixed - only the text sampling position changes
// Also applies bottom margin offset (80px) to account for thicker bottom bezel
fn scale_for_sampling(uv: vec2<f32>) -> vec2<f32> {
    // Bottom margin: 80px offset (in normalized coords based on screen height)
    // This accounts for the asymmetric bezel (thicker at bottom)
    let bottom_margin = 80.0 / uniforms.screen_size.y;

    // Center point is shifted up slightly to account for asymmetric bezel
    let center_y = 0.5 - bottom_margin * 0.5;
    let center = vec2<f32>(0.5, center_y);

    // Apply scale around the adjusted center
    let scale = vec2<f32>(uniforms.content_scale_x, uniforms.content_scale_y);
    return (uv - center) / scale + vec2<f32>(0.5, 0.5);
}

// Apply CRT effects for bezel mode - screen shape is FIXED, only text sampling is scaled
fn apply_bezel_mode_crt(screen_uv: vec2<f32>) -> vec4<f32> {
    // Apply barrel distortion to the FIXED screen UV (not scaled)
    // This keeps the CRT "glass" shape constant regardless of scale settings
    let distorted_uv = barrel_distort(screen_uv, uniforms.curvature);

    // Anti-aliased edge mask based on fixed screen shape
    let edge_alpha = edge_mask_aa(distorted_uv);
    if (edge_alpha <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Scale the distorted UV for sampling the text texture
    // This is where content_scale affects things - it moves where we sample
    // The texture sampler uses ClampToEdge, so out-of-bounds samples get edge pixels
    let sample_uv = scale_for_sampling(distorted_uv);

    // Sample the input texture - no bounds check here!
    // The screen shape is defined ONLY by the barrel distortion edge above
    var color = textureSample(input_texture, input_sampler, sample_uv).rgb;

    // Bloom
    if (uniforms.bloom_intensity > 0.0) {
        let texel_size = 1.0 / uniforms.screen_size;
        let bloomed = bloom(sample_uv, texel_size);
        color = mix(color, bloomed + color * 0.5, uniforms.bloom_intensity * 0.5);
    }

    // Scanlines relative to FIXED screen (not scaled) - like real CRT phosphor lines
    let scan = scanline(distorted_uv, uniforms.scanline_intensity, uniforms.screen_size.y, uniforms.time);
    color = color * scan;

    // Flicker
    color = color * flicker(uniforms.time, uniforms.flicker);

    // Static noise relative to fixed screen
    let noise_val = temporal_noise(distorted_uv * uniforms.screen_size, uniforms.time);
    color = color + (noise_val - 0.5) * uniforms.static_noise;

    // Vignette relative to FIXED screen shape
    let vignette_uv = distorted_uv * 2.0 - 1.0;
    let vignette = 1.0 - dot(vignette_uv, vignette_uv) * uniforms.vignette;
    color = color * vignette;

    // Apply brightness
    color = color * uniforms.brightness;

    // Apply AA edge fade (based on fixed screen shape)
    color = color * edge_alpha;

    return vec4<f32>(color, 1.0);
}

// Sample bezel for a specific pane (bezel scaled to pane bounds)
fn sample_pane_bezel(screen_uv: vec2<f32>, pane_idx: i32) -> vec4<f32> {
    let p = uniforms.panes[pane_idx];

    // Convert screen UV to pane-local UV (0-1 within the pane)
    let local_uv = vec2<f32>(
        (screen_uv.x - p.x) / p.w,
        (screen_uv.y - p.y) / p.h
    );

    // Pane size in pixels
    let pane_w = p.w * uniforms.screen_size.x;
    let pane_h = p.h * uniforms.screen_size.y;

    // Scale bezel borders proportionally to pane size
    // Use the smaller dimension to scale borders so they fit
    let scale_factor = min(pane_w / uniforms.bezel_size.x, pane_h / uniforms.bezel_size.y);
    let top = uniforms.bezel_border_top * scale_factor;
    let right = uniforms.bezel_border_right * scale_factor;
    let bottom = uniforms.bezel_border_bottom * scale_factor;
    let left = uniforms.bezel_border_left * scale_factor;

    // Bezel image size
    let img_w = uniforms.bezel_size.x;
    let img_h = uniforms.bezel_size.y;

    // Position within pane in pixels
    let px = local_uv.x * pane_w;
    let py = local_uv.y * pane_h;

    // 9-patch UV calculation (same logic as global, but for pane)
    var tex_u: f32;
    var tex_v: f32;

    if (px < left) {
        tex_u = px / scale_factor / img_w;
    } else if (px > pane_w - right) {
        tex_u = (img_w - (pane_w - px) / scale_factor) / img_w;
    } else {
        let middle_pane = pane_w - left - right;
        let middle_img = img_w - uniforms.bezel_border_left - uniforms.bezel_border_right;
        let t = (px - left) / middle_pane;
        tex_u = (uniforms.bezel_border_left + t * middle_img) / img_w;
    }

    if (py < top) {
        tex_v = py / scale_factor / img_h;
    } else if (py > pane_h - bottom) {
        tex_v = (img_h - (pane_h - py) / scale_factor) / img_h;
    } else {
        let middle_pane = pane_h - top - bottom;
        let middle_img = img_h - uniforms.bezel_border_top - uniforms.bezel_border_bottom;
        let t = (py - top) / middle_pane;
        tex_v = (uniforms.bezel_border_top + t * middle_img) / img_h;
    }

    return textureSample(bezel_texture, input_sampler, vec2<f32>(tex_u, tex_v));
}

// Apply CRT effects for a pane with bezel - screen shape is FIXED per-pane
fn apply_pane_bezel_crt(screen_uv: vec2<f32>, pane_idx: i32) -> vec4<f32> {
    let p = uniforms.panes[pane_idx];
    let pane_size = vec2<f32>(p.w * uniforms.screen_size.x, p.h * uniforms.screen_size.y);
    let is_focused = (pane_idx == uniforms.focused_pane);

    // Convert to local UV within the pane (0-1)
    let local_uv = global_to_local_uv(screen_uv, pane_idx);

    // Apply barrel distortion to the FIXED pane UV (not scaled)
    // This keeps the CRT "glass" shape constant within this pane
    let distorted_local = barrel_distort(local_uv, uniforms.curvature);

    // Edge mask based on fixed pane shape - THIS is the only screen boundary
    let edge_alpha = edge_mask_aa(distorted_local);
    if (edge_alpha <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Scale the distorted UV for sampling - this is where content_scale takes effect
    let scaled_local = scale_for_sampling(distorted_local);

    // Convert back to global UV for sampling the texture
    let sample_uv = local_to_global_uv(scaled_local, pane_idx);

    // Sample the input texture - no bounds check!
    // The screen shape is defined ONLY by the barrel distortion edge above
    var color = textureSample(input_texture, input_sampler, sample_uv).rgb;

    // Add edge glow for focused pane (uses FIXED distorted_local coordinates)
    color = color + edge_glow(distorted_local, is_focused);

    // Bloom
    if (uniforms.bloom_intensity > 0.0) {
        let texel_size = 1.0 / uniforms.screen_size;
        let bloomed = bloom(sample_uv, texel_size);
        color = mix(color, bloomed + color * 0.5, uniforms.bloom_intensity * 0.5);
    }

    // Scanlines relative to FIXED pane shape (not scaled)
    let scan = scanline(distorted_local, uniforms.scanline_intensity, pane_size.y, uniforms.time);
    color = color * scan;

    // Flicker
    color = color * flicker(uniforms.time + f32(pane_idx) * 0.1, uniforms.flicker);

    // Static noise relative to fixed pane
    let noise_val = temporal_noise(distorted_local * pane_size, uniforms.time);
    color = color + (noise_val - 0.5) * uniforms.static_noise;

    // Vignette relative to FIXED pane center
    let vignette_uv = distorted_local * 2.0 - 1.0;
    let vignette = 1.0 - dot(vignette_uv, vignette_uv) * uniforms.vignette;
    color = color * vignette;

    // Apply brightness and edge fade
    color = color * uniforms.brightness * edge_alpha;

    return vec4<f32>(color, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Check if bezel is enabled
    if (uniforms.bezel_enabled != 0u) {
        if (uniforms.per_pane_mode != 0u) {
            // Per-pane + bezel: each pane is its own CRT with its own bezel
            let pane_idx = find_pane(in.uv);
            if (pane_idx < 0) {
                // Outside all panes - black
                return vec4<f32>(0.0, 0.0, 0.0, 1.0);
            }

            // Render CRT content for this pane (fixed screen shape, scaled content)
            let screen_color = apply_pane_bezel_crt(in.uv, pane_idx);

            // Render bezel for this pane
            let bezel_sample = sample_pane_bezel(in.uv, pane_idx);
            let final_color = mix(screen_color.rgb, bezel_sample.rgb, bezel_sample.a);

            return vec4<f32>(final_color, 1.0);
        } else {
            // Single screen + bezel: one CRT for the whole window
            let screen_color = apply_bezel_mode_crt(in.uv);

            let bezel_sample = sample_bezel_9patch(in.uv);
            let final_color = mix(screen_color.rgb, bezel_sample.rgb, bezel_sample.a);

            return vec4<f32>(final_color, 1.0);
        }
    }

    // No bezel - original behavior
    if (uniforms.per_pane_mode == 0u) {
        return apply_whole_screen_crt(in.uv);
    }

    // Per-pane mode without bezel: each pane is its own mini-CRT
    let pane_idx = find_pane(in.uv);
    if (pane_idx < 0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    return apply_per_pane_crt(in.uv, pane_idx);
}
