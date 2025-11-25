// ABOUTME: Glyph atlas for GPU text rendering.
// ABOUTME: Rasterizes font glyphs and packs them into a texture atlas.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;

pub struct GlyphAtlas {
    font: Font,
    font_size: f32,
    ascent: f32,
    glyphs: HashMap<char, GlyphInfo>,
    atlas_data: Vec<u8>,
    atlas_width: u32,
    atlas_height: u32,
    next_x: u32,
    next_y: u32,
    row_height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphInfo {
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_width: f32,
    pub uv_height: f32,
    pub width: u32,
    pub height: u32,
    pub advance: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum AtlasError {
    #[error("Failed to load font: {0}")]
    FontLoadError(String),

    #[error("Atlas is full")]
    AtlasFull,
}

impl GlyphAtlas {
    pub fn new(font_data: &[u8], font_size: f32) -> Result<Self, AtlasError> {
        let font = Font::from_bytes(font_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(e.to_string()))?;

        // Get line metrics for proper baseline positioning
        let line_metrics = font
            .horizontal_line_metrics(font_size)
            .unwrap_or(fontdue::LineMetrics {
                ascent: font_size * 0.8,
                descent: font_size * -0.2,
                line_gap: 0.0,
                new_line_size: font_size,
            });

        let atlas_width = 1024;
        let atlas_height = 1024;
        let atlas_data = vec![0u8; (atlas_width * atlas_height) as usize];

        Ok(Self {
            font,
            font_size,
            ascent: line_metrics.ascent,
            glyphs: HashMap::new(),
            atlas_data,
            atlas_width,
            atlas_height,
            next_x: 0,
            next_y: 0,
            row_height: 0,
        })
    }

    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// Get glyph info, rasterizing if needed
    pub fn get_glyph(&mut self, c: char) -> Result<GlyphInfo, AtlasError> {
        if let Some(info) = self.glyphs.get(&c) {
            return Ok(*info);
        }

        let (metrics, bitmap) = self.font.rasterize(c, self.font_size);

        if metrics.width == 0 || metrics.height == 0 {
            // Space or empty glyph
            let info = GlyphInfo {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_width: 0.0,
                uv_height: 0.0,
                width: 0,
                height: 0,
                advance: metrics.advance_width,
                offset_x: metrics.xmin as f32,
                offset_y: metrics.ymin as f32,
            };
            self.glyphs.insert(c, info);
            return Ok(info);
        }

        // Check if we need to wrap to next row
        if self.next_x + metrics.width as u32 > self.atlas_width {
            self.next_x = 0;
            self.next_y += self.row_height + 1;
            self.row_height = 0;
        }

        // Check if atlas is full
        if self.next_y + metrics.height as u32 > self.atlas_height {
            return Err(AtlasError::AtlasFull);
        }

        // Copy glyph bitmap to atlas
        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let src_idx = y * metrics.width + x;
                let dst_x = self.next_x + x as u32;
                let dst_y = self.next_y + y as u32;
                let dst_idx = (dst_y * self.atlas_width + dst_x) as usize;
                self.atlas_data[dst_idx] = bitmap[src_idx];
            }
        }

        let info = GlyphInfo {
            uv_x: self.next_x as f32 / self.atlas_width as f32,
            uv_y: self.next_y as f32 / self.atlas_height as f32,
            uv_width: metrics.width as f32 / self.atlas_width as f32,
            uv_height: metrics.height as f32 / self.atlas_height as f32,
            width: metrics.width as u32,
            height: metrics.height as u32,
            advance: metrics.advance_width,
            offset_x: metrics.xmin as f32,
            offset_y: metrics.ymin as f32,
        };

        self.next_x += metrics.width as u32 + 1;
        self.row_height = self.row_height.max(metrics.height as u32);

        self.glyphs.insert(c, info);
        Ok(info)
    }

    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }

    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_width, self.atlas_height)
    }

    pub fn cell_size(&self) -> (f32, f32) {
        let metrics = self.font.metrics('M', self.font_size);
        (metrics.advance_width, self.font_size)
    }
}
