// ABOUTME: Shared types and configuration for cool-rust-term.
// ABOUTME: Defines colors, effect settings, and config file handling.

pub mod color;
pub mod config;
pub mod effects;

pub use color::Color;
pub use config::{ColorScheme, Config, Font};
pub use effects::EffectSettings;
