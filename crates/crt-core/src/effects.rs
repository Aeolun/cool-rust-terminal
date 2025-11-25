// ABOUTME: CRT visual effect parameters.
// ABOUTME: Controls scanlines, curvature, bloom, burn-in, and other retro effects.

use serde::{Deserialize, Serialize};

use crate::Color;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectSettings {
    /// Font/text color (phosphor color)
    pub font_color: Color,

    /// Background color
    pub background_color: Color,

    /// Screen curvature amount (0.0 = flat, 1.0 = very curved)
    pub screen_curvature: f32,

    /// Scanline intensity (0.0 = none, 1.0 = strong)
    pub scanline_intensity: f32,

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
            scanline_intensity: 0.3,
            bloom: 0.4,
            burn_in: 0.4,
            static_noise: 0.05,
            flicker: 0.05,
            horizontal_sync: 0.0,
            rgb_shift: 0.0,
            ambient_light: 0.1,
            brightness: 1.0,
        }
    }
}
