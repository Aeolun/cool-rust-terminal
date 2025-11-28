// ABOUTME: Low-level wgpu initialization and state management.
// ABOUTME: Handles device, queue, surface, and swap chain configuration.

use std::sync::Arc;
use wgpu::{Device, Queue, Surface, SurfaceConfiguration};
use winit::window::Window;

pub struct GpuState {
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub size: (u32, u32),
}

impl GpuState {
    pub async fn new(window: Arc<Window>) -> Result<Self, wgpu::CreateSurfaceError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Main Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .expect("Failed to create device");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        // Log available present modes for debugging
        tracing::info!("Available present modes: {:?}", surface_caps.present_modes);

        // Prefer Mailbox for high refresh rate displays (no frame limiting, triple-buffered)
        // Fall back to Fifo (standard vsync) if Mailbox isn't available
        let present_mode = if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            tracing::info!("Using Mailbox present mode (uncapped framerate)");
            wgpu::PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            tracing::info!("Using Fifo present mode (vsync)");
            wgpu::PresentMode::Fifo
        } else {
            tracing::info!("Using AutoVsync present mode (fallback)");
            wgpu::PresentMode::AutoVsync
        };

        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            size: (size.width, size.height),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.size = (width, height);
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }
}
