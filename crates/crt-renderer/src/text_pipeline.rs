// ABOUTME: Text rendering pipeline for terminal characters.
// ABOUTME: Renders glyphs from atlas texture using instanced quads.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::atlas::GlyphAtlas;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Uniforms {
    screen_size: [f32; 2],
    _padding: [f32; 2],
}

pub struct TextPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    max_chars: usize,
    num_indices: u32,
}

impl TextPipeline {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        atlas: &GlyphAtlas,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Text Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/text.wgsl").into()),
        });

        // Create atlas texture
        let (atlas_width, atlas_height) = atlas.atlas_dimensions();
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            atlas.atlas_data(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(atlas_width),
                rows_per_image: Some(atlas_height),
            },
            wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
        );

        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Atlas Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[Uniforms {
                screen_size: [800.0, 600.0],
                _padding: [0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Text Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Text Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Text Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Text Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

        // Pre-allocate buffers for up to 10000 characters
        let max_chars = 10000;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Text Vertex Buffer"),
            size: (max_chars * 4 * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Text Index Buffer"),
            size: (max_chars * 6 * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            uniform_buffer,
            vertex_buffer,
            index_buffer,
            max_chars,
            num_indices: 0,
        }
    }

    pub fn update_screen_size(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[Uniforms {
                screen_size: [width, height],
                _padding: [0.0, 0.0],
            }]),
        );
    }

    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        atlas: &mut GlyphAtlas,
        chars: &[(char, f32, f32, [f32; 4])], // char, x, baseline_y, color
    ) {
        let mut vertices = Vec::with_capacity(chars.len() * 4);
        let mut indices = Vec::with_capacity(chars.len() * 6);

        for (i, &(c, x, baseline_y, color)) in chars.iter().enumerate() {
            let glyph = match atlas.get_glyph(c) {
                Ok(g) => g,
                Err(_) => continue,
            };

            if glyph.width == 0 || glyph.height == 0 {
                continue;
            }

            // offset_x is xmin (horizontal bearing)
            // offset_y is ymin - distance from baseline to top of glyph (negative = above baseline)
            // In screen coords (Y down), glyph top is at baseline_y - (height - ymin)
            let x0 = x + glyph.offset_x;
            let y0 = baseline_y - glyph.height as f32 - glyph.offset_y;
            let x1 = x0 + glyph.width as f32;
            let y1 = y0 + glyph.height as f32;

            let u0 = glyph.uv_x;
            let v0 = glyph.uv_y;
            let u1 = glyph.uv_x + glyph.uv_width;
            let v1 = glyph.uv_y + glyph.uv_height;

            let base = (vertices.len() / 4) as u32 * 4;

            vertices.push(Vertex {
                position: [x0, y0],
                tex_coords: [u0, v0],
                color,
            });
            vertices.push(Vertex {
                position: [x1, y0],
                tex_coords: [u1, v0],
                color,
            });
            vertices.push(Vertex {
                position: [x1, y1],
                tex_coords: [u1, v1],
                color,
            });
            vertices.push(Vertex {
                position: [x0, y1],
                tex_coords: [u0, v1],
                color,
            });

            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
            indices.push(base);
            indices.push(base + 2);
            indices.push(base + 3);

            if i >= self.max_chars - 1 {
                break;
            }
        }

        if !vertices.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
            queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&indices));
        }

        self.num_indices = indices.len() as u32;
    }

    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.num_indices == 0 {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
    }
}
