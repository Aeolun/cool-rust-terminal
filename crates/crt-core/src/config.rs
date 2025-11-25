// ABOUTME: Application configuration handling.
// ABOUTME: Loads and saves settings from TOML config files.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::EffectSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Visual effect settings
    pub effects: EffectSettings,

    /// Font file path
    pub font_path: Option<PathBuf>,

    /// Font size in pixels
    pub font_size: f32,

    /// Window dimensions
    pub window_width: u32,
    pub window_height: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            effects: EffectSettings::default(),
            font_path: None,
            font_size: 16.0,
            window_width: 800,
            window_height: 600,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse config: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Failed to serialize config: {0}")]
    SerializeError(#[from] toml::ser::Error),
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
