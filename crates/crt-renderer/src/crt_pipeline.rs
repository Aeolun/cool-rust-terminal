// ABOUTME: CRT post-processing pipeline for retro monitor effects.
// ABOUTME: Renders fullscreen quad with barrel distortion, scanlines, and bloom.
// ABOUTME: Supports per-pane mode where each pane gets independent CRT effects.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const MAX_PANES: usize = 16;

// Embedded bezel image
const BEZEL_IMAGE_BYTES: &[u8] = include_bytes!("../../../fallout.png");

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct PaneRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CrtUniforms {
    screen_size: [f32; 2],
    time: f32,
    curvature: f32,
    scanline_intensity: f32,
    bloom_intensity: f32,
    per_pane_mode: u32,
    pane_count: u32,
    focused_pane: i32,
    focus_glow_radius: f32,
    focus_glow_width: f32,
    focus_glow_intensity: f32,
    // Additional effect settings
    static_noise: f32,
    flicker: f32,
    brightness: f32,
    vignette: f32,
    // Bezel settings
    bezel_enabled: u32,
    scanline_mode: u32,         // 0 = row-based, 1 = pixel-level
    bezel_size: [f32; 2],       // Bezel image size (width, height)
    // 9-patch borders: top, right, bottom, left (in pixels)
    bezel_border_top: f32,
    bezel_border_right: f32,
    bezel_border_bottom: f32,
    bezel_border_left: f32,
    // Content scale - adjusts how big the content is drawn (like H-SIZE/V-SIZE knobs)
    content_scale_x: f32,
    content_scale_y: f32,
    // Cell height for scanline alignment (one scanline per text row)
    cell_height: f32,
    _pad1: f32,  // Padding for vec4 alignment
    // Focus glow color (follows font color) - uses vec4 for alignment (w ignored)
    glow_color: [f32; 4],
    // Pane rects (max 16 panes)
    panes: [PaneRect; MAX_PANES],
}

pub struct CrtPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    #[allow(dead_code)] // Kept alive for bezel_view
    bezel_texture: wgpu::Texture,
    bezel_view: wgpu::TextureView,
    time: f32,
}

impl CrtPipeline {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("CRT Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/crt.wgsl").into()),
        });

        // Load bezel image
        let bezel_image = image::load_from_memory(BEZEL_IMAGE_BYTES)
            .expect("Failed to load embedded bezel image")
            .to_rgba8();
        let bezel_dimensions = bezel_image.dimensions();

        let bezel_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Bezel Texture"),
            size: wgpu::Extent3d {
                width: bezel_dimensions.0,
                height: bezel_dimensions.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &bezel_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bezel_image,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * bezel_dimensions.0),
                rows_per_image: Some(bezel_dimensions.1),
            },
            wgpu::Extent3d {
                width: bezel_dimensions.0,
                height: bezel_dimensions.1,
                depth_or_array_layers: 1,
            },
        );

        let bezel_view = bezel_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("CRT Uniform Buffer"),
            contents: bytemuck::cast_slice(&[CrtUniforms {
                screen_size: [800.0, 600.0],
                time: 0.0,
                curvature: 0.03,
                scanline_intensity: 0.45,
                bloom_intensity: 0.3,
                per_pane_mode: 0,
                pane_count: 0,
                focused_pane: -1,
                focus_glow_radius: 0.05,
                focus_glow_width: 0.06,
                focus_glow_intensity: 0.6,
                static_noise: 0.02,
                flicker: 0.25,
                brightness: 1.0,
                vignette: 0.25,
                bezel_enabled: 0,
                scanline_mode: 0,  // Row-based by default
                bezel_size: [bezel_dimensions.0 as f32, bezel_dimensions.1 as f32],
                bezel_border_top: 52.0,
                bezel_border_right: 52.0,
                bezel_border_bottom: 116.0,
                bezel_border_left: 52.0,
                content_scale_x: 1.0,
                content_scale_y: 1.0,
                cell_height: 18.0,  // Default font size
                _pad1: 0.0,
                glow_color: [1.0, 0.7, 0.0, 1.0],  // Default amber
                panes: [PaneRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }; MAX_PANES],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("CRT Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("CRT Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Bezel texture
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("CRT Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("CRT Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            sampler,
            bezel_texture,
            bezel_view,
            time: 0.0,
        }
    }

    pub fn create_bind_group(
        &self,
        device: &wgpu::Device,
        input_texture_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("CRT Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(input_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.bezel_view),
                },
            ],
        })
    }

    /// Update CRT uniforms
    /// pane_rects: slice of (x, y, width, height) in normalized coordinates (0-1)
    /// focused_pane: index of the focused pane (-1 if none/single pane)
    /// cell_height: height of a text cell in pixels (for scanline alignment)
    /// effect settings from config
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        width: f32,
        height: f32,
        dt: f32,
        per_pane_mode: bool,
        pane_rects: &[(f32, f32, f32, f32)],
        focused_pane: i32,
        cell_height: f32,
        curvature: f32,
        scanline_intensity: f32,
        scanline_mode: u32,
        bloom_intensity: f32,
        focus_glow_radius: f32,
        focus_glow_width: f32,
        focus_glow_intensity: f32,
        static_noise: f32,
        flicker: f32,
        brightness: f32,
        vignette: f32,
        bezel_enabled: bool,
        content_scale_x: f32,
        content_scale_y: f32,
        glow_color: [f32; 4],
    ) {
        self.time += dt;

        let mut panes = [PaneRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }; MAX_PANES];
        let pane_count = pane_rects.len().min(MAX_PANES);
        for (i, &(x, y, w, h)) in pane_rects.iter().take(MAX_PANES).enumerate() {
            panes[i] = PaneRect { x, y, w, h };
        }

        // Bezel image dimensions: 715x600, borders: 52px top/left/right, 116px bottom
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[CrtUniforms {
                screen_size: [width, height],
                time: self.time,
                curvature,
                scanline_intensity,
                bloom_intensity,
                per_pane_mode: if per_pane_mode { 1 } else { 0 },
                pane_count: pane_count as u32,
                focused_pane,
                focus_glow_radius,
                focus_glow_width,
                focus_glow_intensity,
                static_noise,
                flicker,
                brightness,
                vignette,
                bezel_enabled: if bezel_enabled { 1 } else { 0 },
                scanline_mode,
                bezel_size: [715.0, 600.0],
                bezel_border_top: 52.0,
                bezel_border_right: 52.0,
                bezel_border_bottom: 116.0,
                bezel_border_left: 52.0,
                content_scale_x,
                content_scale_y,
                cell_height,
                _pad1: 0.0,
                glow_color,
                panes,
            }]),
        );
    }

    /// Reset the time to replay the power-on animation
    pub fn reset_time(&mut self) {
        self.time = 0.0;
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>, bind_group: &'a wgpu::BindGroup) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1); // Fullscreen triangle
    }
}
