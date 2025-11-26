// ABOUTME: Glyph atlas for GPU text rendering.
// ABOUTME: Rasterizes font glyphs and packs them into a texture atlas.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;

pub struct GlyphAtlas {
    font: Font,
    font_size: f32,
    ascent: f32,
    cell_width: f32,
    cell_height: f32,
    fallback_font: Option<Font>,
    fallback_font_size: f32,
    emoji_font: Option<Font>,
    emoji_font_size: f32,
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

        // Calculate cell size from 'M' character
        let metrics = font.metrics('M', font_size);
        let cell_width = metrics.advance_width;
        let cell_height = font_size;

        let atlas_width = 1024;
        let atlas_height = 1024;
        let atlas_data = vec![0u8; (atlas_width * atlas_height) as usize];

        Ok(Self {
            font,
            font_size,
            ascent: line_metrics.ascent,
            cell_width,
            cell_height,
            fallback_font: None,
            fallback_font_size: font_size,
            emoji_font: None,
            emoji_font_size: font_size,
            glyphs: HashMap::new(),
            atlas_data,
            atlas_width,
            atlas_height,
            next_x: 0,
            next_y: 0,
            row_height: 0,
        })
    }

    /// Set a fallback font for characters missing from the primary font.
    /// The fallback font size is calculated to match the primary font's cell height.
    pub fn set_fallback(&mut self, fallback_data: &[u8]) -> Result<(), AtlasError> {
        let fallback = Font::from_bytes(fallback_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(format!("fallback: {}", e)))?;

        // Calculate font size for fallback to match primary cell height
        let fallback_line_metrics = fallback
            .horizontal_line_metrics(self.font_size)
            .unwrap_or(fontdue::LineMetrics {
                ascent: self.font_size * 0.8,
                descent: self.font_size * -0.2,
                line_gap: 0.0,
                new_line_size: self.font_size,
            });

        // Scale fallback to match primary cell height
        let fallback_natural_height = fallback_line_metrics.ascent - fallback_line_metrics.descent;
        let scale = self.cell_height / fallback_natural_height;
        let fallback_font_size = self.font_size * scale;

        self.fallback_font = Some(fallback);
        self.fallback_font_size = fallback_font_size;

        tracing::info!(
            "Fallback font configured: size={:.1} (primary cell: {:.1}x{:.1})",
            fallback_font_size,
            self.cell_width,
            self.cell_height
        );

        Ok(())
    }

    /// Set an emoji fallback font for emoji characters.
    pub fn set_emoji_fallback(&mut self, emoji_data: &[u8]) -> Result<(), AtlasError> {
        let emoji = Font::from_bytes(emoji_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(format!("emoji: {}", e)))?;

        // Calculate font size for emoji to match primary cell height
        let emoji_line_metrics = emoji
            .horizontal_line_metrics(self.font_size)
            .unwrap_or(fontdue::LineMetrics {
                ascent: self.font_size * 0.8,
                descent: self.font_size * -0.2,
                line_gap: 0.0,
                new_line_size: self.font_size,
            });

        let emoji_natural_height = emoji_line_metrics.ascent - emoji_line_metrics.descent;
        let scale = self.cell_height / emoji_natural_height;
        let emoji_font_size = self.font_size * scale;

        self.emoji_font = Some(emoji);
        self.emoji_font_size = emoji_font_size;

        tracing::info!(
            "Emoji fallback font configured: size={:.1}",
            emoji_font_size
        );

        Ok(())
    }

    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// Check if primary font has a glyph (not .notdef)
    fn primary_has_glyph(&self, c: char) -> bool {
        self.font.lookup_glyph_index(c) != 0
    }

    /// Check if fallback font has a glyph (not .notdef)
    fn fallback_has_glyph(&self, c: char) -> bool {
        self.fallback_font
            .as_ref()
            .map(|f| f.lookup_glyph_index(c) != 0)
            .unwrap_or(false)
    }

    /// Check if emoji font has a glyph (not .notdef)
    fn emoji_has_glyph(&self, c: char) -> bool {
        self.emoji_font
            .as_ref()
            .map(|f| f.lookup_glyph_index(c) != 0)
            .unwrap_or(false)
    }

    /// Get glyph info, rasterizing if needed. Falls back to fallback font if available,
    /// or '?' if neither font has the character.
    pub fn get_glyph(&mut self, c: char) -> Result<GlyphInfo, AtlasError> {
        if let Some(info) = self.glyphs.get(&c) {
            return Ok(*info);
        }

        // Try fonts in order: primary -> fallback -> emoji -> '?'
        let primary_has = self.primary_has_glyph(c);
        let fallback_has = self.fallback_has_glyph(c);
        let emoji_has = self.emoji_has_glyph(c);

        let (metrics, bitmap, advance_override, source) = if primary_has {
            let (m, b) = self.font.rasterize(c, self.font_size);
            // If primary returned empty bitmap, try fallbacks
            if (m.width == 0 || m.height == 0) && c != ' ' {
                if fallback_has {
                    let fallback = self.fallback_font.as_ref().unwrap();
                    let (fm, fb) = fallback.rasterize(c, self.fallback_font_size);
                    (fm, fb, Some(self.cell_width), "fallback (primary empty)")
                } else if emoji_has {
                    let emoji = self.emoji_font.as_ref().unwrap();
                    let (em, eb) = emoji.rasterize(c, self.emoji_font_size);
                    (em, eb, Some(self.cell_width), "emoji (primary empty)")
                } else {
                    (m, b, None, "primary")
                }
            } else {
                (m, b, None, "primary")
            }
        } else if fallback_has {
            // Primary doesn't have it, try fallback
            let fallback = self.fallback_font.as_ref().unwrap();
            let (m, b) = fallback.rasterize(c, self.fallback_font_size);
            (m, b, Some(self.cell_width), "fallback")
        } else if emoji_has {
            // Try emoji font
            let emoji = self.emoji_font.as_ref().unwrap();
            let (m, b) = emoji.rasterize(c, self.emoji_font_size);
            (m, b, Some(self.cell_width), "emoji")
        } else {
            // No font has this glyph - use '?' from primary
            let (m, b) = self.font.rasterize('?', self.font_size);
            (m, b, None, "? (no font has glyph)")
        };

        // Log non-ASCII glyph resolution (only on first rasterization, not cached)
        if !c.is_ascii() {
            tracing::debug!(
                "Glyph {:?} (U+{:04X}): source={}, size={}x{}, offset=({:.1},{:.1}), cell={:.1}x{:.1}",
                c, c as u32, source, metrics.width, metrics.height,
                metrics.xmin, metrics.ymin, self.cell_width, self.cell_height
            );
        }

        let advance = advance_override.unwrap_or(metrics.advance_width);

        if metrics.width == 0 || metrics.height == 0 {
            // Space or empty glyph
            let info = GlyphInfo {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_width: 0.0,
                uv_height: 0.0,
                width: 0,
                height: 0,
                advance,
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
            advance,
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
