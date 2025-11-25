// ABOUTME: GPU rendering and CRT shader effects.
// ABOUTME: Uses wgpu to render terminal text and apply retro visual effects.

pub mod atlas;
mod crt_pipeline;
mod gpu;
pub mod renderer;
mod text_pipeline;

pub use renderer::{RenderCell, Renderer};
