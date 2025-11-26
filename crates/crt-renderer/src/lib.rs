// ABOUTME: GPU rendering and CRT shader effects.
// ABOUTME: Uses wgpu to render terminal text and apply retro visual effects.

pub mod atlas;
pub mod bdf;
mod burnin_pipeline;
mod crt_pipeline;
pub mod fonts;
mod gpu;
mod line_pipeline;
pub mod renderer;
mod text_pipeline;

pub use atlas::GlyphAtlas;
pub use bdf::BdfFont;
pub use fonts::{get_bdf_font_data, get_font_data};
pub use renderer::{EffectParams, RenderCell, Renderer};
