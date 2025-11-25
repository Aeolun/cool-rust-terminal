// ABOUTME: Color representation and conversion utilities.
// ABOUTME: Supports RGB, linear RGB, and preset CRT phosphor colors.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Classic amber phosphor color (P3 phosphor)
    pub const AMBER: Self = Self::rgb(1.0, 0.7, 0.0);

    /// Classic green phosphor color (P1 phosphor)
    pub const GREEN: Self = Self::rgb(0.2, 1.0, 0.2);

    /// White phosphor
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
}

impl Default for Color {
    fn default() -> Self {
        Self::AMBER
    }
}
