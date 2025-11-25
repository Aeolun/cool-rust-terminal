// ABOUTME: Main GPU renderer using wgpu.
// ABOUTME: Renders terminal panes with CRT shader effects.

use std::sync::Arc;
use std::time::Instant;
use winit::window::Window;

use crt_core::Font;

use crate::atlas::GlyphAtlas;
use crate::burnin_pipeline::BurnInPipeline;
use crate::crt_pipeline::CrtPipeline;
use crate::fonts::get_font_data;
use crate::gpu::GpuState;
use crate::line_pipeline::LinePipeline;
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

/// Effect settings for CRT shader
pub struct EffectParams {
    pub curvature: f32,
    pub scanline_intensity: f32,
    pub bloom: f32,
    pub focus_glow_radius: f32,
    pub focus_glow_width: f32,
    pub focus_glow_intensity: f32,
    pub static_noise: f32,
    pub flicker: f32,
    pub brightness: f32,
    pub vignette: f32,
    pub bezel_enabled: bool,
    pub content_scale_x: f32,
    pub content_scale_y: f32,
    pub glow_color: [f32; 4],
}

pub struct Renderer {
    gpu: GpuState,
    clear_color: wgpu::Color,
    text_pipeline: TextPipeline,
    line_pipeline: LinePipeline,
    atlas: GlyphAtlas,
    font_color: [f32; 4],
    current_font: Font,
    current_font_size: f32,
    crt_pipeline: CrtPipeline,
    burnin_pipeline: BurnInPipeline,
    offscreen_texture: wgpu::Texture,
    offscreen_view: wgpu::TextureView,
    crt_bind_group: wgpu::BindGroup,
    last_frame: Instant,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, font: Font, font_size: f32) -> Result<Self, RenderError> {
        let gpu = GpuState::new(window).await?;

        // Dark background color
        let clear_color = wgpu::Color {
            r: 0.02,
            g: 0.02,
            b: 0.02,
            a: 1.0,
        };

        // Load font
        let font_data = get_font_data(font);
        let mut atlas = GlyphAtlas::new(font_data, font_size)?;

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
        // Box drawing for separators
        let _ = atlas.get_glyph('│');
        let _ = atlas.get_glyph('─');
        // Corner brackets for focus indicator
        let _ = atlas.get_glyph('┌');
        let _ = atlas.get_glyph('┐');
        let _ = atlas.get_glyph('└');
        let _ = atlas.get_glyph('┘');

        let text_pipeline = TextPipeline::new(&gpu.device, &gpu.queue, gpu.config.format, &atlas);
        let line_pipeline = LinePipeline::new(&gpu.device, gpu.config.format);

        // Amber color
        let font_color = [1.0, 0.7, 0.0, 1.0];

        // Create CRT pipeline
        let crt_pipeline = CrtPipeline::new(&gpu.device, &gpu.queue, gpu.config.format);

        // Create burn-in pipeline
        let (width, height) = gpu.size;
        let burnin_pipeline = BurnInPipeline::new(&gpu.device, gpu.config.format, width, height);

        // Create off-screen render texture
        let (offscreen_texture, offscreen_view) =
            Self::create_offscreen_texture(&gpu.device, width, height, gpu.config.format);

        // CRT reads from burn-in output
        let crt_bind_group = crt_pipeline.create_bind_group(&gpu.device, burnin_pipeline.output_view());

        Ok(Self {
            gpu,
            clear_color,
            text_pipeline,
            line_pipeline,
            atlas,
            font_color,
            current_font: font,
            current_font_size: font_size,
            crt_pipeline,
            burnin_pipeline,
            offscreen_texture,
            offscreen_view,
            crt_bind_group,
            last_frame: Instant::now(),
        })
    }

    /// Change the font and/or size. Recreates the atlas and text pipeline.
    pub fn set_font(&mut self, font: Font, font_size: f32) -> Result<(), RenderError> {
        if font == self.current_font && (font_size - self.current_font_size).abs() < 0.1 {
            return Ok(()); // No change needed
        }

        // Create new atlas with new font
        let font_data = get_font_data(font);
        let mut atlas = GlyphAtlas::new(font_data, font_size)?;

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
        // Box drawing for separators
        let _ = atlas.get_glyph('│');
        let _ = atlas.get_glyph('─');
        // Corner brackets for focus indicator
        let _ = atlas.get_glyph('┌');
        let _ = atlas.get_glyph('┐');
        let _ = atlas.get_glyph('└');
        let _ = atlas.get_glyph('┘');

        // Recreate text pipeline with new atlas
        let text_pipeline = TextPipeline::new(
            &self.gpu.device,
            &self.gpu.queue,
            self.gpu.config.format,
            &atlas,
        );

        self.atlas = atlas;
        self.text_pipeline = text_pipeline;
        self.current_font = font;
        self.current_font_size = font_size;

        Ok(())
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

        // Resize burn-in textures
        self.burnin_pipeline.resize(&self.gpu.device, self.gpu.config.format, width, height);

        // CRT reads from burn-in output
        self.crt_bind_group = self.crt_pipeline.create_bind_group(&self.gpu.device, self.burnin_pipeline.output_view());
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

        // Update CRT uniforms (whole-screen mode for simple grid render)
        self.crt_pipeline.update(
            &self.gpu.queue,
            width as f32,
            height as f32,
            dt,
            false, // whole-screen mode
            &[(0.0, 0.0, 1.0, 1.0)], // single full-screen pane
            -1, // no focused pane
            0.03, // default curvature
            0.3,  // default scanlines
            0.3,  // default bloom
            0.05, // default glow radius
            0.06, // default glow width
            0.6,  // default glow intensity
            0.05, // default static noise
            0.05, // default flicker
            1.0,  // default brightness
            0.2,  // default vignette
            false, // bezel disabled for simple render
            1.0,  // default content scale x
            1.0,  // default content scale y
            [1.0, 0.7, 0.0, 1.0],  // default amber glow
        );

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
    /// Separators are (x, y, length, is_vertical) in pixels
    /// focus_rect is (x, y, width, height) in pixels for the focused pane
    /// size_indicators are (center_x, center_y, text) for each pane's size display
    /// pane_rects_normalized are (x, y, width, height) in normalized coords (0-1) for CRT
    /// per_pane_crt enables per-pane CRT effects
    /// debug_grid draws 1px lines at cell boundaries for debugging alignment
    /// focused_pane_index is the index of the focused pane in pane_rects_normalized (-1 if single pane)
    /// effects contains the CRT effect parameters from config
    pub fn render_panes(
        &mut self,
        panes: &[(f32, f32, &[Vec<RenderCell>])],
        separators: &[(f32, f32, f32, bool)],
        focus_rect: Option<(f32, f32, f32, f32)>,
        size_indicators: &[(f32, f32, String)],
        pane_rects_normalized: &[(f32, f32, f32, f32)],
        per_pane_crt: bool,
        debug_grid: bool,
        focused_pane_index: i32,
        effects: EffectParams,
    ) -> Result<(), RenderError> {
        let (width, height) = self.gpu.size;
        let (cell_w, cell_h) = self.atlas.cell_size();
        let ascent = self.atlas.ascent();

        // Calculate delta time for animations
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        let mut chars = Vec::new();

        // Render pane contents
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

        // Separators will be drawn via line_pipeline (see below)

        // Render size indicators (centered in each pane)
        let size_color = [1.0, 1.0, 1.0, 0.9]; // Bright white
        for (center_x, center_y, text) in size_indicators {
            let text_width = text.len() as f32 * cell_w;
            let start_x = center_x - text_width / 2.0;
            let y = center_y + ascent / 2.0;

            for (i, c) in text.chars().enumerate() {
                chars.push((c, start_x + i as f32 * cell_w, y, size_color));
            }
        }

        self.text_pipeline
            .update_screen_size(&self.gpu.queue, width as f32, height as f32);
        self.text_pipeline
            .prepare(&self.gpu.queue, &mut self.atlas, &chars);

        // Prepare lines for rendering (separators + focus borders + optional debug grid)
        // In per-pane CRT mode, skip separator/focus lines (use shader glow instead)
        let mut all_lines: Vec<(f32, f32, f32, f32, f32, [f32; 4])> = Vec::new();

        if !per_pane_crt {
            // Draw separators as lines
            let separator_color = [1.0, 0.7, 0.0, 0.6]; // Amber, slightly transparent
            let separator_thickness = 1.0;
            for &(x, y, length, is_vertical) in separators {
                if is_vertical {
                    all_lines.push((x, y, x, y + length, separator_thickness, separator_color));
                } else {
                    all_lines.push((x, y, x + length, y, separator_thickness, separator_color));
                }
            }

            // Draw focus indicator as highlighted borders (on top of separators)
            if let Some((fx, fy, fw, fh)) = focus_rect {
                let focus_color = [1.0, 0.9, 0.3, 1.0]; // Bright yellow-amber
                let line_thickness = 2.0;
                let edge_threshold = 5.0; // Pixels from window edge to consider "at edge"

                // Left edge (if not at window edge)
                if fx > edge_threshold {
                    all_lines.push((fx, fy, fx, fy + fh, line_thickness, focus_color));
                }
                // Right edge (if not at window edge)
                if fx + fw < width as f32 - edge_threshold {
                    all_lines.push((fx + fw, fy, fx + fw, fy + fh, line_thickness, focus_color));
                }
                // Top edge (if not at window edge)
                if fy > edge_threshold {
                    all_lines.push((fx, fy, fx + fw, fy, line_thickness, focus_color));
                }
                // Bottom edge (if not at window edge)
                if fy + fh < height as f32 - edge_threshold {
                    all_lines.push((fx, fy + fh, fx + fw, fy + fh, line_thickness, focus_color));
                }
            }
        }

        // Add debug grid lines if enabled
        if debug_grid {
            let grid_color = [0.3, 0.3, 0.3, 0.5]; // Dark gray, semi-transparent
            let line_thickness = 1.0;

            // Draw grid for each pane
            for &(x_offset, y_offset, cells) in panes {
                let num_rows = cells.len();
                let num_cols = if num_rows > 0 { cells[0].len() } else { 0 };

                // Vertical lines (column boundaries)
                for col in 0..=num_cols {
                    let x = x_offset + col as f32 * cell_w;
                    let y0 = y_offset;
                    let y1 = y_offset + num_rows as f32 * cell_h;
                    all_lines.push((x, y0, x, y1, line_thickness, grid_color));
                }

                // Horizontal lines (row boundaries)
                for row in 0..=num_rows {
                    let y = y_offset + row as f32 * cell_h;
                    let x0 = x_offset;
                    let x1 = x_offset + num_cols as f32 * cell_w;
                    all_lines.push((x0, y, x1, y, line_thickness, grid_color));
                }
            }
        }

        self.line_pipeline.update_screen_size(&self.gpu.queue, width as f32, height as f32);
        self.line_pipeline.prepare(&self.gpu.queue, &all_lines);

        // Update CRT uniforms
        self.crt_pipeline.update(
            &self.gpu.queue,
            width as f32,
            height as f32,
            dt,
            per_pane_crt,
            pane_rects_normalized,
            focused_pane_index,
            effects.curvature,
            effects.scanline_intensity,
            effects.bloom,
            effects.focus_glow_radius,
            effects.focus_glow_width,
            effects.focus_glow_intensity,
            effects.static_noise,
            effects.flicker,
            effects.brightness,
            effects.vignette,
            effects.bezel_enabled,
            effects.content_scale_x,
            effects.content_scale_y,
            effects.glow_color,
        );

        // Update burn-in uniforms
        self.burnin_pipeline.update(&self.gpu.queue, 0.92, 1.0);

        // Prepare burn-in bind groups (needs current frame texture)
        self.burnin_pipeline.prepare_bind_groups(&self.gpu.device, &self.offscreen_view);

        // Update CRT bind group to read from burn-in output
        self.crt_bind_group = self.crt_pipeline.create_bind_group(&self.gpu.device, self.burnin_pipeline.output_view());

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

            // Render lines (focus borders + debug grid if enabled)
            self.line_pipeline.render(&mut render_pass);
        }

        // Pass 2: Apply burn-in effect (blend current frame with decayed previous)
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Burn-in Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.burnin_pipeline.target_view(),
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

            self.burnin_pipeline.render(&mut render_pass);
        }

        // Pass 3: Apply CRT effect to screen
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

        // Swap burn-in buffers for next frame
        self.burnin_pipeline.swap();

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
