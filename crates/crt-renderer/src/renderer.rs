// ABOUTME: Main GPU renderer using wgpu.
// ABOUTME: Renders terminal panes with CRT shader effects.

use std::sync::Arc;
use std::time::Instant;
use winit::window::Window;

use crate::atlas::GlyphAtlas;
use crate::crt_pipeline::CrtPipeline;
use crate::gpu::GpuState;
use crate::text_pipeline::TextPipeline;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Surface error: {0}")]
    Surface(#[from] wgpu::SurfaceError),

    #[error("Failed to create surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),

    #[error("Atlas error: {0}")]
    Atlas(#[from] crate::atlas::AtlasError),
}

/// A single cell to render
pub struct RenderCell {
    pub c: char,
    pub fg: [f32; 4],
    pub bg: [f32; 4],
}

pub struct Renderer {
    gpu: GpuState,
    clear_color: wgpu::Color,
    text_pipeline: TextPipeline,
    atlas: GlyphAtlas,
    font_color: [f32; 4],
    crt_pipeline: CrtPipeline,
    offscreen_texture: wgpu::Texture,
    offscreen_view: wgpu::TextureView,
    crt_bind_group: wgpu::BindGroup,
    last_frame: Instant,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, RenderError> {
        let gpu = GpuState::new(window).await?;

        // Dark background color
        let clear_color = wgpu::Color {
            r: 0.02,
            g: 0.02,
            b: 0.02,
            a: 1.0,
        };

        // Load font - use IBM VGA for that authentic look
        let font_data = include_bytes!("../../../assets/fonts/1985-ibm-pc-vga/PxPlus_IBM_VGA8.ttf");
        let mut atlas = GlyphAtlas::new(font_data, 24.0)?;

        // Pre-populate common ASCII characters
        for c in ' '..='~' {
            let _ = atlas.get_glyph(c);
        }
        // Block characters for cursor
        let _ = atlas.get_glyph('█');
        let _ = atlas.get_glyph('▌');
        let _ = atlas.get_glyph('▐');
        let _ = atlas.get_glyph('▀');
        let _ = atlas.get_glyph('▄');

        let text_pipeline = TextPipeline::new(&gpu.device, &gpu.queue, gpu.config.format, &atlas);

        // Amber color
        let font_color = [1.0, 0.7, 0.0, 1.0];

        // Create CRT pipeline
        let crt_pipeline = CrtPipeline::new(&gpu.device, gpu.config.format);

        // Create off-screen render texture
        let (width, height) = gpu.size;
        let (offscreen_texture, offscreen_view) =
            Self::create_offscreen_texture(&gpu.device, width, height, gpu.config.format);

        let crt_bind_group = crt_pipeline.create_bind_group(&gpu.device, &offscreen_view);

        Ok(Self {
            gpu,
            clear_color,
            text_pipeline,
            atlas,
            font_color,
            crt_pipeline,
            offscreen_texture,
            offscreen_view,
            crt_bind_group,
            last_frame: Instant::now(),
        })
    }

    fn create_offscreen_texture(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Offscreen Texture"),
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
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.gpu.resize(width, height);
        self.text_pipeline
            .update_screen_size(&self.gpu.queue, width as f32, height as f32);

        // Recreate off-screen texture at new size
        let (offscreen_texture, offscreen_view) =
            Self::create_offscreen_texture(&self.gpu.device, width, height, self.gpu.config.format);
        self.offscreen_texture = offscreen_texture;
        self.offscreen_view = offscreen_view;
        self.crt_bind_group = self.crt_pipeline.create_bind_group(&self.gpu.device, &self.offscreen_view);
    }

    pub fn cell_size(&self) -> (f32, f32) {
        self.atlas.cell_size()
    }

    /// Calculate how many columns and rows fit in the current window
    pub fn grid_size(&self) -> (u16, u16) {
        let (cell_w, cell_h) = self.atlas.cell_size();
        let (width, height) = self.gpu.size;
        let cols = (width as f32 / cell_w).floor() as u16;
        let rows = (height as f32 / cell_h).floor() as u16;
        (cols.max(1), rows.max(1))
    }

    /// Calculate grid size for a region (in pixels)
    pub fn grid_size_for_region(&self, width_px: u32, height_px: u32) -> (u16, u16) {
        let (cell_w, cell_h) = self.atlas.cell_size();
        let cols = (width_px as f32 / cell_w).floor() as u16;
        let rows = (height_px as f32 / cell_h).floor() as u16;
        (cols.max(1), rows.max(1))
    }

    /// Get window size in pixels
    pub fn window_size(&self) -> (u32, u32) {
        self.gpu.size
    }

    /// Render a grid of cells with CRT post-processing
    pub fn render_grid(
        &mut self,
        cells: &[Vec<RenderCell>],
    ) -> Result<(), RenderError> {
        let (width, height) = self.gpu.size;
        let (cell_w, cell_h) = self.atlas.cell_size();
        let ascent = self.atlas.ascent();

        // Calculate delta time for animations
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        let mut chars = Vec::new();

        for (row_idx, row) in cells.iter().enumerate() {
            let baseline_y = (row_idx as f32 * cell_h) + ascent;

            for (col_idx, cell) in row.iter().enumerate() {
                if cell.c == ' ' || cell.c == '\0' {
                    continue;
                }

                let x = col_idx as f32 * cell_w;
                chars.push((cell.c, x, baseline_y, cell.fg));
            }
        }

        self.text_pipeline
            .update_screen_size(&self.gpu.queue, width as f32, height as f32);
        self.text_pipeline
            .prepare(&self.gpu.queue, &mut self.atlas, &chars);

        // Update CRT uniforms
        self.crt_pipeline.update(&self.gpu.queue, width as f32, height as f32, dt);

        let output = self.gpu.surface.get_current_texture()?;
        let screen_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Pass 1: Render text to off-screen texture
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_pipeline.render(&mut render_pass);
        }

        // Pass 2: Apply CRT effect to screen
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("CRT Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &screen_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.crt_pipeline.render(&mut render_pass, &self.crt_bind_group);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Render multiple panes, each with its pixel region and cells
    /// Each pane is (x_offset, y_offset, cells)
    pub fn render_panes(
        &mut self,
        panes: &[(f32, f32, &[Vec<RenderCell>])],
    ) -> Result<(), RenderError> {
        let (width, height) = self.gpu.size;
        let (cell_w, cell_h) = self.atlas.cell_size();
        let ascent = self.atlas.ascent();

        // Calculate delta time for animations
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        let mut chars = Vec::new();

        for &(x_offset, y_offset, cells) in panes {
            for (row_idx, row) in cells.iter().enumerate() {
                let baseline_y = y_offset + (row_idx as f32 * cell_h) + ascent;

                for (col_idx, cell) in row.iter().enumerate() {
                    if cell.c == ' ' || cell.c == '\0' {
                        continue;
                    }

                    let x = x_offset + col_idx as f32 * cell_w;
                    chars.push((cell.c, x, baseline_y, cell.fg));
                }
            }
        }

        self.text_pipeline
            .update_screen_size(&self.gpu.queue, width as f32, height as f32);
        self.text_pipeline
            .prepare(&self.gpu.queue, &mut self.atlas, &chars);

        // Update CRT uniforms
        self.crt_pipeline
            .update(&self.gpu.queue, width as f32, height as f32, dt);

        let output = self.gpu.surface.get_current_texture()?;
        let screen_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Pass 1: Render text to off-screen texture
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.offscreen_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_pipeline.render(&mut render_pass);
        }

        // Pass 2: Apply CRT effect to screen
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("CRT Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &screen_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.crt_pipeline
                .render(&mut render_pass, &self.crt_bind_group);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Render test text (for debugging)
    pub fn render(&mut self) -> Result<(), RenderError> {
        let test_text = "cool-rust-term v0.1.0\n\nTerminal not connected\n\n$ _";
        let (cell_w, cell_h) = self.atlas.cell_size();
        let ascent = self.atlas.ascent();
        let line_height = cell_h;
        let mut chars = Vec::new();

        let mut x = 10.0;
        let mut baseline_y = 10.0 + ascent;

        for c in test_text.chars() {
            if c == '\n' {
                x = 10.0;
                baseline_y += line_height;
                continue;
            }

            chars.push((c, x, baseline_y, self.font_color));
            x += cell_w;
        }

        let (width, height) = self.gpu.size;
        self.text_pipeline
            .update_screen_size(&self.gpu.queue, width as f32, height as f32);
        self.text_pipeline
            .prepare(&self.gpu.queue, &mut self.atlas, &chars);

        let output = self.gpu.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.text_pipeline.render(&mut render_pass);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}
