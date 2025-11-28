// ABOUTME: Glyph atlas for GPU text rendering.
// ABOUTME: Rasterizes font glyphs and packs them into a texture atlas.
// ABOUTME: Supports both TTF (via fontdue) and BDF bitmap fonts.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;

use crate::bdf::BdfFont;

/// The font source - either a rasterized TTF or a pixel-perfect BDF
enum FontSource {
    /// TTF font with fontdue rasterizer
    Ttf {
        font: Font,
        font_size: f32,
    },
    /// BDF bitmap font (no rasterization needed)
    Bdf {
        font: BdfFont,
    },
}

pub struct GlyphAtlas {
    source: FontSource,
    ascent: f32,
    cell_width: f32,
    cell_height: f32,
    fallback_font: Option<Font>,
    fallback_font_size: f32,
    symbols_font: Option<Font>,
    symbols_font_size: f32,
    emoji_font: Option<Font>,
    emoji_font_size: f32,
    bdf_fallback: Option<BdfFallback>,
    glyphs: HashMap<char, GlyphInfo>,
    atlas_data: Vec<u8>,
    atlas_width: u32,
    atlas_height: u32,
    next_x: u32,
    next_y: u32,
    row_height: u32,
}

/// BDF font used as fallback, with its native cell dimensions for scaling
struct BdfFallback {
    font: BdfFont,
    cell_width: u32,
    cell_height: u32,
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
    /// Create a new atlas from TTF font data
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
            source: FontSource::Ttf { font, font_size },
            ascent: line_metrics.ascent,
            cell_width,
            cell_height,
            fallback_font: None,
            fallback_font_size: font_size,
            symbols_font: None,
            symbols_font_size: font_size,
            emoji_font: None,
            emoji_font_size: font_size,
            bdf_fallback: None,
            glyphs: HashMap::new(),
            atlas_data,
            atlas_width,
            atlas_height,
            next_x: 0,
            next_y: 0,
            row_height: 0,
        })
    }

    /// Create a new atlas from BDF font data
    pub fn from_bdf(bdf_data: &[u8]) -> Result<Self, AtlasError> {
        let font = BdfFont::parse(bdf_data)
            .map_err(|e| AtlasError::FontLoadError(e.to_string()))?;

        let cell_width = font.cell_width() as f32;
        let cell_height = font.cell_height() as f32;
        let ascent = font.ascent as f32;

        // BDF fonts typically have limited character sets, so use a smaller default font size
        // for fallback scaling
        let fallback_font_size = cell_height;

        let atlas_width = 1024;
        let atlas_height = 1024;
        let atlas_data = vec![0u8; (atlas_width * atlas_height) as usize];

        tracing::info!(
            "Loaded BDF font: {}x{} cell, ascent={}, descent={}, {} glyphs",
            cell_width, cell_height, font.ascent, font.descent, font.glyphs.len()
        );

        Ok(Self {
            source: FontSource::Bdf { font },
            ascent,
            cell_width,
            cell_height,
            fallback_font: None,
            fallback_font_size,
            symbols_font: None,
            symbols_font_size: fallback_font_size,
            emoji_font: None,
            emoji_font_size: fallback_font_size,
            bdf_fallback: None,
            glyphs: HashMap::new(),
            atlas_data,
            atlas_width,
            atlas_height,
            next_x: 0,
            next_y: 0,
            row_height: 0,
        })
    }

    /// Get the font size (for TTF) or cell height (for BDF)
    fn primary_font_size(&self) -> f32 {
        match &self.source {
            FontSource::Ttf { font_size, .. } => *font_size,
            FontSource::Bdf { .. } => self.cell_height,
        }
    }

    /// Set a fallback font for characters missing from the primary font.
    /// The fallback font size is calculated to match the primary font's cell height.
    pub fn set_fallback(&mut self, fallback_data: &[u8]) -> Result<(), AtlasError> {
        let fallback = Font::from_bytes(fallback_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(format!("fallback: {}", e)))?;

        let base_size = self.primary_font_size();

        // Calculate font size for fallback to match primary cell height
        let fallback_line_metrics = fallback
            .horizontal_line_metrics(base_size)
            .unwrap_or(fontdue::LineMetrics {
                ascent: base_size * 0.8,
                descent: base_size * -0.2,
                line_gap: 0.0,
                new_line_size: base_size,
            });

        // Scale fallback to match primary cell height
        let fallback_natural_height = fallback_line_metrics.ascent - fallback_line_metrics.descent;
        let scale = self.cell_height / fallback_natural_height;
        let fallback_font_size = base_size * scale;

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

    /// Set a symbols fallback font for technical symbols.
    pub fn set_symbols_fallback(&mut self, symbols_data: &[u8]) -> Result<(), AtlasError> {
        let symbols = Font::from_bytes(symbols_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(format!("symbols: {}", e)))?;

        let base_size = self.primary_font_size();

        // Calculate font size for symbols to match primary cell height
        let symbols_line_metrics = symbols
            .horizontal_line_metrics(base_size)
            .unwrap_or(fontdue::LineMetrics {
                ascent: base_size * 0.8,
                descent: base_size * -0.2,
                line_gap: 0.0,
                new_line_size: base_size,
            });

        let symbols_natural_height = symbols_line_metrics.ascent - symbols_line_metrics.descent;
        let scale = self.cell_height / symbols_natural_height;
        let symbols_font_size = base_size * scale;

        self.symbols_font = Some(symbols);
        self.symbols_font_size = symbols_font_size;

        tracing::info!(
            "Symbols fallback font configured: size={:.1}",
            symbols_font_size
        );

        Ok(())
    }

    /// Set an emoji fallback font for emoji characters.
    pub fn set_emoji_fallback(&mut self, emoji_data: &[u8]) -> Result<(), AtlasError> {
        let emoji = Font::from_bytes(emoji_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(format!("emoji: {}", e)))?;

        let base_size = self.primary_font_size();

        // Calculate font size for emoji to match primary cell height
        let emoji_line_metrics = emoji
            .horizontal_line_metrics(base_size)
            .unwrap_or(fontdue::LineMetrics {
                ascent: base_size * 0.8,
                descent: base_size * -0.2,
                line_gap: 0.0,
                new_line_size: base_size,
            });

        let emoji_natural_height = emoji_line_metrics.ascent - emoji_line_metrics.descent;
        let scale = self.cell_height / emoji_natural_height;
        let emoji_font_size = base_size * scale;

        self.emoji_font = Some(emoji);
        self.emoji_font_size = emoji_font_size;

        tracing::info!(
            "Emoji fallback font configured: size={:.1}",
            emoji_font_size
        );

        Ok(())
    }

    /// Set a BDF fallback font for comprehensive Unicode coverage.
    /// The font will be scaled to match the primary font's cell dimensions.
    pub fn set_bdf_fallback(&mut self, bdf_data: &[u8]) -> Result<(), AtlasError> {
        let font = BdfFont::parse(bdf_data)
            .map_err(|e| AtlasError::FontLoadError(format!("bdf fallback: {}", e)))?;

        let cell_width = font.cell_width();
        let cell_height = font.cell_height();

        tracing::info!(
            "BDF fallback font configured: {}x{} cell, {} glyphs (scaling to {:.0}x{:.0})",
            cell_width, cell_height, font.glyphs.len(),
            self.cell_width, self.cell_height
        );

        self.bdf_fallback = Some(BdfFallback {
            font,
            cell_width,
            cell_height,
        });

        Ok(())
    }

    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// Check if primary font has a glyph (not .notdef)
    fn primary_has_glyph(&self, c: char) -> bool {
        match &self.source {
            FontSource::Ttf { font, .. } => font.lookup_glyph_index(c) != 0,
            FontSource::Bdf { font } => font.get_char(c).is_some(),
        }
    }

    /// Check if fallback font has a glyph (not .notdef)
    fn fallback_has_glyph(&self, c: char) -> bool {
        self.fallback_font
            .as_ref()
            .map(|f| f.lookup_glyph_index(c) != 0)
            .unwrap_or(false)
    }

    /// Check if symbols font has a glyph (not .notdef)
    fn symbols_has_glyph(&self, c: char) -> bool {
        self.symbols_font
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

    /// Check if BDF fallback font has a glyph
    fn bdf_fallback_has_glyph(&self, c: char) -> bool {
        self.bdf_fallback
            .as_ref()
            .map(|fb| fb.font.get_char(c).is_some())
            .unwrap_or(false)
    }

    /// Get glyph info, rasterizing if needed. Falls back to fallback font if available,
    /// or '?' if neither font has the character.
    /// is_wide indicates if this is a double-width character (CJK, etc.)
    pub fn get_glyph(&mut self, c: char, is_wide: bool) -> Result<GlyphInfo, AtlasError> {
        // Cache key includes is_wide to handle rare cases where same char might be rendered differently
        let cache_key = if is_wide {
            // Use private use area to differentiate wide glyphs in cache
            char::from_u32(c as u32 | 0x100000).unwrap_or(c)
        } else {
            c
        };

        if let Some(info) = self.glyphs.get(&cache_key) {
            return Ok(*info);
        }

        // Try fonts in order: primary -> fallback -> symbols -> bdf_fallback -> emoji -> '?'
        let primary_has = self.primary_has_glyph(c);
        let fallback_has = self.fallback_has_glyph(c);
        let symbols_has = self.symbols_has_glyph(c);
        let bdf_fallback_has = self.bdf_fallback_has_glyph(c);
        let emoji_has = self.emoji_has_glyph(c);

        // Rasterize glyph from appropriate font
        // Returns (width, height, xmin, ymin, advance, bitmap, source_name)
        let (width, height, xmin, ymin, advance, bitmap, source_name): (usize, usize, i32, i32, f32, Vec<u8>, &str) =
            if primary_has {
                match &self.source {
                    FontSource::Ttf { font, font_size } => {
                        let (m, b) = font.rasterize(c, *font_size);
                        // If primary returned empty bitmap, try fallbacks
                        if (m.width == 0 || m.height == 0) && c != ' ' {
                            if fallback_has {
                                let fallback = self.fallback_font.as_ref().unwrap();
                                let (fm, fb) = fallback.rasterize(c, self.fallback_font_size);
                                (fm.width, fm.height, fm.xmin, fm.ymin, self.cell_width, fb, "fallback (primary empty)")
                            } else if symbols_has {
                                let symbols = self.symbols_font.as_ref().unwrap();
                                let (sm, sb) = symbols.rasterize(c, self.symbols_font_size);
                                (sm.width, sm.height, sm.xmin, sm.ymin, self.cell_width, sb, "symbols (primary empty)")
                            } else if bdf_fallback_has {
                                self.render_bdf_fallback_glyph(c, is_wide, "bdf fallback (primary empty)")
                            } else if emoji_has {
                                let emoji = self.emoji_font.as_ref().unwrap();
                                let (em, eb) = emoji.rasterize(c, self.emoji_font_size);
                                (em.width, em.height, em.xmin, em.ymin, self.cell_width, eb, "emoji (primary empty)")
                            } else {
                                (m.width, m.height, m.xmin, m.ymin, m.advance_width, b, "primary")
                            }
                        } else {
                            (m.width, m.height, m.xmin, m.ymin, m.advance_width, b, "primary")
                        }
                    }
                    FontSource::Bdf { font } => {
                        let glyph = font.get_char(c).unwrap();
                        let bitmap = glyph.render();
                        // BDF offset_y is from baseline (positive = above), fontdue ymin is from baseline (positive = above)
                        (glyph.width as usize, glyph.height as usize,
                         glyph.offset_x, glyph.offset_y,
                         glyph.dwidth_x as f32, bitmap, "primary (bdf)")
                    }
                }
            } else if fallback_has {
                // Primary doesn't have it, try fallback
                let fallback = self.fallback_font.as_ref().unwrap();
                let (m, b) = fallback.rasterize(c, self.fallback_font_size);
                (m.width, m.height, m.xmin, m.ymin, self.cell_width, b, "fallback")
            } else if symbols_has {
                // Try symbols font
                let symbols = self.symbols_font.as_ref().unwrap();
                let (m, b) = symbols.rasterize(c, self.symbols_font_size);
                (m.width, m.height, m.xmin, m.ymin, self.cell_width, b, "symbols")
            } else if bdf_fallback_has {
                // Try BDF fallback (e.g., Unifont for comprehensive Unicode coverage)
                self.render_bdf_fallback_glyph(c, is_wide, "bdf fallback")
            } else if emoji_has {
                // Try emoji font
                let emoji = self.emoji_font.as_ref().unwrap();
                let (m, b) = emoji.rasterize(c, self.emoji_font_size);
                (m.width, m.height, m.xmin, m.ymin, self.cell_width, b, "emoji")
            } else {
                // No font has this glyph - use '?' from primary or fallback
                match &self.source {
                    FontSource::Ttf { font, font_size } => {
                        let (m, b) = font.rasterize('?', *font_size);
                        (m.width, m.height, m.xmin, m.ymin, m.advance_width, b, "? (no font has glyph)")
                    }
                    FontSource::Bdf { font } => {
                        // Try to get '?' from BDF, otherwise use fallback
                        if let Some(glyph) = font.get_char('?') {
                            let bitmap = glyph.render();
                            (glyph.width as usize, glyph.height as usize,
                             glyph.offset_x, glyph.offset_y,
                             glyph.dwidth_x as f32, bitmap, "? (bdf)")
                        } else if let Some(fallback) = &self.fallback_font {
                            let (m, b) = fallback.rasterize('?', self.fallback_font_size);
                            (m.width, m.height, m.xmin, m.ymin, self.cell_width, b, "? (fallback)")
                        } else {
                            // Return empty glyph
                            (0, 0, 0, 0, self.cell_width, vec![], "? (empty)")
                        }
                    }
                }
            };

        // Log non-ASCII glyph resolution (only on first rasterization, not cached)
        if !c.is_ascii() {
            tracing::debug!(
                "Glyph {:?} (U+{:04X}): source={}, size={}x{}, offset=({},{}), cell={:.1}x{:.1}",
                c, c as u32, source_name, width, height,
                xmin, ymin, self.cell_width, self.cell_height
            );
        }

        if width == 0 || height == 0 {
            // Space or empty glyph
            let info = GlyphInfo {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_width: 0.0,
                uv_height: 0.0,
                width: 0,
                height: 0,
                advance,
                offset_x: xmin as f32,
                offset_y: ymin as f32,
            };
            self.glyphs.insert(cache_key, info);
            return Ok(info);
        }

        // Check if we need to wrap to next row
        if self.next_x + width as u32 > self.atlas_width {
            self.next_x = 0;
            self.next_y += self.row_height + 1;
            self.row_height = 0;
        }

        // Check if atlas is full
        if self.next_y + height as u32 > self.atlas_height {
            return Err(AtlasError::AtlasFull);
        }

        // Copy glyph bitmap to atlas
        for y in 0..height {
            for x in 0..width {
                let src_idx = y * width + x;
                let dst_x = self.next_x + x as u32;
                let dst_y = self.next_y + y as u32;
                let dst_idx = (dst_y * self.atlas_width + dst_x) as usize;
                self.atlas_data[dst_idx] = bitmap[src_idx];
            }
        }

        let info = GlyphInfo {
            uv_x: self.next_x as f32 / self.atlas_width as f32,
            uv_y: self.next_y as f32 / self.atlas_height as f32,
            uv_width: width as f32 / self.atlas_width as f32,
            uv_height: height as f32 / self.atlas_height as f32,
            width: width as u32,
            height: height as u32,
            advance,
            offset_x: xmin as f32,
            offset_y: ymin as f32,
        };

        self.next_x += width as u32 + 1;
        self.row_height = self.row_height.max(height as u32);

        self.glyphs.insert(cache_key, info);
        Ok(info)
    }

    /// Render a glyph from the BDF fallback font, scaling to match primary cell size.
    /// For wide characters (CJK, etc.), scales to 2x cell width.
    /// Returns (width, height, xmin, ymin, advance, bitmap, source_name).
    fn render_bdf_fallback_glyph(&self, c: char, is_wide: bool, source_name: &'static str) -> (usize, usize, i32, i32, f32, Vec<u8>, &'static str) {
        let fb = self.bdf_fallback.as_ref().unwrap();
        let glyph = fb.font.get_char(c).unwrap();

        // Wide chars (CJK, etc.) render at 2x cell width
        let target_width = if is_wide {
            (self.cell_width * 2.0) as u32
        } else {
            self.cell_width as u32
        };

        let scaled = glyph.render_scaled(
            target_width,
            self.cell_height as u32,
            fb.cell_width,
            fb.cell_height,
        );

        let advance = if is_wide {
            self.cell_width * 2.0
        } else {
            self.cell_width
        };

        (
            scaled.width as usize,
            scaled.height as usize,
            scaled.offset_x,
            scaled.offset_y,
            advance,
            scaled.bitmap,
            source_name,
        )
    }

    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }

    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_width, self.atlas_height)
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }
}
