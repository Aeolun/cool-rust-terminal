// ABOUTME: CRT visual effect parameters.
// ABOUTME: Controls scanlines, curvature, bloom, burn-in, and other retro effects.

use serde::{Deserialize, Serialize};

use crate::Color;

/// Scanline rendering mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScanlineMode {
    /// One scanline cycle per text row - works with any font, less authentic
    #[default]
    RowBased,
    /// Pixel-level scanlines like real CRT - best with BDF bitmap fonts
    Pixel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectSettings {
    /// Font/text color (phosphor color)
    pub font_color: Color,

    /// Background color
    pub background_color: Color,

    /// Screen curvature amount (0.0 = flat, 1.0 = very curved)
    pub screen_curvature: f32,

    /// Scanline intensity (0.0 = none, 1.0 = strong)
    pub scanline_intensity: f32,

    /// Scanline rendering mode (row-based for TTF, pixel for BDF bitmap fonts)
    pub scanline_mode: ScanlineMode,

    /// Bloom/glow amount (0.0 = none, 1.0 = strong)
    pub bloom: f32,

    /// Phosphor burn-in persistence (0.0 = none, 1.0 = long persistence)
    pub burn_in: f32,

    /// Static noise amount
    pub static_noise: f32,

    /// Screen flicker amount
    pub flicker: f32,

    /// Horizontal sync jitter
    pub horizontal_sync: f32,

    /// RGB color shift/chromatic aberration
    pub rgb_shift: f32,

    /// Ambient light reflection
    pub ambient_light: f32,

    /// Overall brightness
    pub brightness: f32,

    /// Vignette intensity - darkening toward screen edges (0.0 = none, 1.0 = strong)
    pub vignette: f32,

    /// Focus glow corner radius (0.0 = sharp corners, 0.2 = very rounded)
    pub focus_glow_radius: f32,

    /// Focus glow width - how far the glow extends inward (0.01 = thin, 0.2 = wide)
    pub focus_glow_width: f32,

    /// Focus glow intensity (0.0 = invisible, 1.0 = bright)
    pub focus_glow_intensity: f32,

    /// Enable CRT monitor bezel frame
    pub bezel_enabled: bool,

    /// Horizontal content scale - adjusts how wide the content is drawn
    /// 1.0 = fills screen width, <1.0 = narrower (black bars on sides), >1.0 = wider (edges hidden)
    pub content_scale_x: f32,

    /// Vertical content scale - adjusts how tall the content is drawn
    /// 1.0 = fills screen height, <1.0 = shorter (black bars top/bottom), >1.0 = taller (edges hidden)
    pub content_scale_y: f32,

    /// Enable physically-accurate beam simulation (requires 240Hz+ monitor)
    /// Simulates electron beam sweep across phosphor screen
    pub beam_simulation_enabled: bool,

    /// Enable interlaced rendering (odd/even scanline fields)
    /// Only applies when beam_simulation_enabled is true
    pub interlace_enabled: bool,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self::amber()
    }
}

impl EffectSettings {
    /// Amber preset - warm CRT monitor look
    pub fn amber() -> Self {
        Self {
            font_color: Color::AMBER,
            background_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            screen_curvature: 0.1,
            scanline_intensity: 0.45,
            scanline_mode: ScanlineMode::RowBased,
            bloom: 0.4,
            burn_in: 0.4,
            static_noise: 0.02,
            flicker: 0.25,
            horizontal_sync: 0.0,
            rgb_shift: 0.0,
            ambient_light: 0.1,
            brightness: 1.0,
            vignette: 0.25,
            focus_glow_radius: 0.01,
            focus_glow_width: 0.005,
            focus_glow_intensity: 0.4,
            bezel_enabled: false,
            content_scale_x: 1.0,
            content_scale_y: 1.0,
            beam_simulation_enabled: false,
            interlace_enabled: true,  // Default on when beam sim is enabled
        }
    }
}
