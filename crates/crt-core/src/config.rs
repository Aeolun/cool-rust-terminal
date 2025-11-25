// ABOUTME: Application configuration handling.
// ABOUTME: Loads and saves settings from TOML config files.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::EffectSettings;

/// A 16-color terminal palette plus foreground/background
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorScheme {
    pub name: String,
    pub foreground: [f32; 4],
    pub background: [f32; 4],
    /// ANSI colors 0-15 (8 normal + 8 bright)
    pub colors: [[f32; 4]; 16],
}

impl ColorScheme {
    /// Classic amber monochrome CRT (matches cool-retro-term's Default Amber #ff8100)
    pub fn amber() -> Self {
        // #ff8100 = rgb(255, 129, 0) - classic amber/orange phosphor
        let bg = [0.05, 0.02, 0.0, 1.0];
        let dark = [0.4, 0.2, 0.0, 1.0];
        let medium = [0.7, 0.35, 0.0, 1.0];
        let bright = [1.0, 0.506, 0.0, 1.0];  // #ff8100
        let full = [1.0, 0.7, 0.2, 1.0];

        Self {
            name: "Amber".to_string(),
            foreground: bright,
            background: bg,
            colors: [
                // Normal colors (0-7): varying intensities
                bg,      // 0: black
                dark,    // 1: red
                medium,  // 2: green
                medium,  // 3: yellow
                dark,    // 4: blue
                dark,    // 5: magenta
                medium,  // 6: cyan
                bright,  // 7: white
                // Bright colors (8-15)
                dark,    // 8: bright black (gray)
                medium,  // 9: bright red
                bright,  // 10: bright green
                bright,  // 11: bright yellow
                medium,  // 12: bright blue
                medium,  // 13: bright magenta
                bright,  // 14: bright cyan
                full,    // 15: bright white
            ],
        }
    }

    /// Fallout terminal green (#22a75f)
    pub fn green() -> Self {
        // #22a75f = rgb(34, 167, 95) - Fallout Pip-Boy green
        let bg = [0.0, 0.02, 0.01, 1.0];
        let dark = [0.05, 0.26, 0.15, 1.0];
        let medium = [0.09, 0.46, 0.26, 1.0];
        let bright = [0.133, 0.655, 0.373, 1.0];  // #22a75f
        let full = [0.2, 0.85, 0.5, 1.0];

        Self {
            name: "Green".to_string(),
            foreground: bright,
            background: bg,
            colors: [
                bg, dark, medium, medium, dark, dark, medium, bright,
                dark, medium, bright, bright, medium, medium, bright, full,
            ],
        }
    }

    /// White/gray monochrome (matches cool-retro-term's white #ffffff)
    pub fn white() -> Self {
        let bg = [0.0, 0.0, 0.0, 1.0];  // Pure black background like cool-retro-term
        let dark = [0.3, 0.3, 0.3, 1.0];
        let medium = [0.6, 0.6, 0.6, 1.0];
        let bright = [1.0, 1.0, 1.0, 1.0];  // #ffffff
        let full = [1.0, 1.0, 1.0, 1.0];

        Self {
            name: "White".to_string(),
            foreground: bright,
            background: bg,
            colors: [
                bg, dark, medium, medium, dark, dark, medium, bright,
                dark, medium, bright, bright, medium, medium, bright, full,
            ],
        }
    }

    /// Full color scheme with actual ANSI colors
    pub fn ansi() -> Self {
        Self {
            name: "ANSI".to_string(),
            foreground: [0.85, 0.85, 0.85, 1.0],
            background: [0.1, 0.1, 0.1, 1.0],
            colors: [
                [0.0, 0.0, 0.0, 1.0],       // 0: black
                [0.8, 0.2, 0.2, 1.0],       // 1: red
                [0.2, 0.8, 0.2, 1.0],       // 2: green
                [0.8, 0.8, 0.2, 1.0],       // 3: yellow
                [0.2, 0.2, 0.8, 1.0],       // 4: blue
                [0.8, 0.2, 0.8, 1.0],       // 5: magenta
                [0.2, 0.8, 0.8, 1.0],       // 6: cyan
                [0.75, 0.75, 0.75, 1.0],    // 7: white
                [0.4, 0.4, 0.4, 1.0],       // 8: bright black
                [1.0, 0.4, 0.4, 1.0],       // 9: bright red
                [0.4, 1.0, 0.4, 1.0],       // 10: bright green
                [1.0, 1.0, 0.4, 1.0],       // 11: bright yellow
                [0.4, 0.4, 1.0, 1.0],       // 12: bright blue
                [1.0, 0.4, 1.0, 1.0],       // 13: bright magenta
                [0.4, 1.0, 1.0, 1.0],       // 14: bright cyan
                [1.0, 1.0, 1.0, 1.0],       // 15: bright white
            ],
        }
    }

    pub fn presets() -> Vec<ColorScheme> {
        vec![
            Self::amber(),
            Self::green(),
            Self::white(),
            Self::ansi(),
        ]
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::amber()
    }
}

impl ColorScheme {
    /// Convert a 256-color palette index to RGBA
    /// - 0-15: use the scheme's ANSI colors
    /// - 16-231: 6x6x6 color cube
    /// - 232-255: grayscale ramp
    pub fn indexed_color(&self, index: u8) -> [f32; 4] {
        match index {
            0..=15 => self.colors[index as usize],
            16..=231 => {
                // 6x6x6 color cube
                let idx = index - 16;
                let r = (idx / 36) % 6;
                let g = (idx / 6) % 6;
                let b = idx % 6;
                // Convert 0-5 to 0-255: 0->0, 1->95, 2->135, 3->175, 4->215, 5->255
                let to_255 = |v: u8| -> f32 {
                    if v == 0 {
                        0.0
                    } else {
                        (55.0 + v as f32 * 40.0) / 255.0
                    }
                };
                [to_255(r), to_255(g), to_255(b), 1.0]
            }
            232..=255 => {
                // Grayscale ramp: 232 -> rgb(8,8,8), 255 -> rgb(238,238,238)
                let gray = (8.0 + (index - 232) as f32 * 10.0) / 255.0;
                [gray, gray, gray, 1.0]
            }
        }
    }
}

/// Bundled font options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Font {
    /// IBM VGA 8x16 (1985) - default
    #[default]
    IbmVga,
    /// IBM BIOS 8x8 (1981)
    IbmBios,
    /// IBM 3278 Terminal (1971)
    Ibm3278,
    /// Apple II (1977)
    Apple2,
    /// Commodore PET (1977)
    CommodorePet,
    /// Commodore 64 (1982)
    Commodore64,
    /// Atari 400/800 (1979)
    Atari,
    /// Terminus (modern)
    Terminus,
    /// Fixedsys Excelsior (modern)
    Fixedsys,
    /// ProggyTiny (modern)
    ProggyTiny,
    /// ProFont (modern)
    ProFont,
    /// Hermit (modern)
    Hermit,
    /// Inconsolata (modern)
    Inconsolata,
}

impl Font {
    pub fn all() -> &'static [Font] {
        &[
            Font::IbmVga,
            Font::IbmBios,
            Font::Ibm3278,
            Font::Apple2,
            Font::CommodorePet,
            Font::Commodore64,
            Font::Atari,
            Font::Terminus,
            Font::Fixedsys,
            Font::ProggyTiny,
            Font::ProFont,
            Font::Hermit,
            Font::Inconsolata,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Font::IbmVga => "IBM VGA",
            Font::IbmBios => "IBM BIOS",
            Font::Ibm3278 => "IBM 3278",
            Font::Apple2 => "Apple II",
            Font::CommodorePet => "Commodore PET",
            Font::Commodore64 => "Commodore 64",
            Font::Atari => "Atari 400",
            Font::Terminus => "Terminus",
            Font::Fixedsys => "Fixedsys",
            Font::ProggyTiny => "ProggyTiny",
            Font::ProFont => "ProFont",
            Font::Hermit => "Hermit",
            Font::Inconsolata => "Inconsolata",
        }
    }

    pub fn next(&self) -> Font {
        let all = Font::all();
        let idx = all.iter().position(|f| f == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    pub fn prev(&self) -> Font {
        let all = Font::all();
        let idx = all.iter().position(|f| f == self).unwrap_or(0);
        if idx == 0 {
            all[all.len() - 1]
        } else {
            all[idx - 1]
        }
    }

    /// Get the font file path relative to assets/fonts/
    pub fn asset_path(&self) -> &'static str {
        match self {
            Font::IbmVga => "1985-ibm-pc-vga/PxPlus_IBM_VGA8.ttf",
            Font::IbmBios => "1981-ibm-pc/PxPlus_IBM_BIOS.ttf",
            Font::Ibm3278 => "1971-ibm-3278/3270-Regular.ttf",
            Font::Apple2 => "1977-apple2/PrintChar21.ttf",
            Font::CommodorePet => "1977-commodore-pet/PetMe.ttf",
            Font::Commodore64 => "1982-commodore64/C64_Pro_Mono-STYLE.ttf",
            Font::Atari => "1979-atari-400-800/AtariClassic-Regular.ttf",
            Font::Terminus => "modern-terminus/TerminusTTF-4.46.0.ttf",
            Font::Fixedsys => "modern-fixedsys-excelsior/FSEX301-L2.ttf",
            Font::ProggyTiny => "modern-proggy-tiny/ProggyTiny.ttf",
            Font::ProFont => "modern-pro-font-win-tweaked/ProFontWindows.ttf",
            Font::Hermit => "modern-hermit/Hermit-medium.otf",
            Font::Inconsolata => "modern-inconsolata/Inconsolata.otf",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Visual effect settings
    pub effects: EffectSettings,

    /// Selected font
    pub font: Font,

    /// Font size in pixels
    pub font_size: f32,

    /// Color scheme (16 ANSI colors + fg/bg)
    pub color_scheme: ColorScheme,

    /// Window dimensions
    pub window_width: u32,
    pub window_height: u32,

    /// Per-pane CRT effects (each pane is its own "monitor")
    pub per_pane_crt: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            effects: EffectSettings::default(),
            font: Font::default(),
            font_size: 18.0,
            color_scheme: ColorScheme::default(),
            window_width: 1200,
            window_height: 800,
            per_pane_crt: false,
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
    /// Get the default config file path (~/.config/cool-rust-term/config.toml)
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("cool-rust-term").join("config.toml"))
    }

    /// Load config from a path
    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load config from default path, or return default config if not found
    pub fn load_or_default() -> Self {
        Self::default_path()
            .and_then(|path| Self::load(&path).ok())
            .unwrap_or_default()
    }

    /// Save config to a path
    pub fn save(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Save config to default path
    pub fn save_to_default(&self) -> Result<PathBuf, ConfigError> {
        let path = Self::default_path().ok_or_else(|| {
            ConfigError::ReadError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory",
            ))
        })?;
        self.save(&path)?;
        Ok(path)
    }
}
