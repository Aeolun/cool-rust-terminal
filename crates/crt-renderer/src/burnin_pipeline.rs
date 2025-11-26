// ABOUTME: Burn-in pipeline for phosphor persistence effect.
// ABOUTME: Uses ping-pong buffers to blend current frame with decayed previous frames.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct BurnInUniforms {
    decay: f32,
    brightness: f32,
    // Beam sweep simulation
    beam_y_start: f32,    // 0.0-1.0, start of current beam band
    beam_y_end: f32,      // 0.0-1.0, end of current beam band
    current_field: u32,   // 0 = even lines, 1 = odd lines
    interlace_enabled: u32, // 0 = disabled, 1 = enabled
    screen_height: f32,   // Screen height in pixels (for scanline calc)
    _padding: f32,
}

pub struct BurnInPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    // Ping-pong textures
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    current_target: usize, // Which texture to write to (0 or 1)
    bind_groups: [Option<wgpu::BindGroup>; 2],
}

impl BurnInPipeline {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Burn-in Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/burnin.wgsl").into()),
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Burn-in Uniform Buffer"),
            contents: bytemuck::cast_slice(&[BurnInUniforms {
                decay: 0.92,      // Phosphor decay rate
                brightness: 1.0,  // Current frame brightness
                beam_y_start: 0.0,
                beam_y_end: 1.0,  // Full screen by default (no beam simulation)
                current_field: 0,
                interlace_enabled: 0,
                screen_height: 600.0,
                _padding: 0.0,
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Burn-in Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Burn-in Bind Group Layout"),
            entries: &[
                // Uniforms
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
                // Current frame texture
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
                // Previous burn-in texture
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Burn-in Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Burn-in Pipeline"),
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

        // Create ping-pong textures
        let (textures, views) = Self::create_textures(device, format, width, height);

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            sampler,
            textures,
            views,
            current_target: 0,
            bind_groups: [None, None],
        }
    }

    fn create_textures(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> ([wgpu::Texture; 2], [wgpu::TextureView; 2]) {
        let create_texture = |label: &str| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };

        let tex0 = create_texture("Burn-in Texture 0");
        let tex1 = create_texture("Burn-in Texture 1");
        let view0 = tex0.create_view(&wgpu::TextureViewDescriptor::default());
        let view1 = tex1.create_view(&wgpu::TextureViewDescriptor::default());

        ([tex0, tex1], [view0, view1])
    }

    pub fn resize(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) {
        let (textures, views) = Self::create_textures(device, format, width, height);
        self.textures = textures;
        self.views = views;
        self.bind_groups = [None, None]; // Invalidate bind groups
    }

    /// Create bind groups for a render pass
    /// current_frame_view: the texture view of the current rendered frame
    pub fn prepare_bind_groups(&mut self, device: &wgpu::Device, current_frame_view: &wgpu::TextureView) {
        // We write to current_target, read from the other one
        let read_idx = 1 - self.current_target;

        // Create bind group for this frame
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Burn-in Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(current_frame_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.views[read_idx]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        self.bind_groups[self.current_target] = Some(bind_group);
    }

    /// Get the texture view to render to (the current target)
    pub fn target_view(&self) -> &wgpu::TextureView {
        &self.views[self.current_target]
    }

    /// Get the texture view to read from (for CRT pass - the result of burn-in)
    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.views[self.current_target]
    }

    /// Render the burn-in pass
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if let Some(bind_group) = &self.bind_groups[self.current_target] {
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }
    }

    /// Swap buffers for next frame
    pub fn swap(&mut self) {
        self.current_target = 1 - self.current_target;
    }

    /// Update uniforms (decay rate, beam position, etc.)
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &self,
        queue: &wgpu::Queue,
        decay: f32,
        brightness: f32,
        beam_y_start: f32,
        beam_y_end: f32,
        current_field: u32,
        interlace_enabled: bool,
        screen_height: f32,
    ) {
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[BurnInUniforms {
                decay,
                brightness,
                beam_y_start,
                beam_y_end,
                current_field,
                interlace_enabled: if interlace_enabled { 1 } else { 0 },
                screen_height,
                _padding: 0.0,
            }]),
        );
    }
}
