// ABOUTME: GPU rendering and CRT shader effects.
// ABOUTME: Uses wgpu to render terminal text and apply retro visual effects.

pub mod atlas;
mod burnin_pipeline;
mod crt_pipeline;
pub mod fonts;
mod gpu;
mod line_pipeline;
pub mod renderer;
mod text_pipeline;

pub use fonts::get_font_data;
pub use renderer::{EffectParams, RenderCell, Renderer};
