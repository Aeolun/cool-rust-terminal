// ABOUTME: Glyph atlas for GPU text rendering.
// ABOUTME: Rasterizes font glyphs and packs them into a texture atlas.
// ABOUTME: Supports both TTF (via fontdue) and BDF bitmap fonts.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;

use crate::bdf::BdfFont;

/// The font source - either a rasterized TTF or a pixel-perfect BDF
enum FontSource {
    /// TTF font with fontdue rasterizer
    Ttf { font: Font, font_size: f32 },
    /// BDF bitmap font (no rasterization needed)
    Bdf { font: BdfFont },
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
    /// Whether the atlas texture data has changed since the last GPU upload
    dirty: bool,
}

/// BDF font used as fallback for comprehensive Unicode coverage. Glyphs are
/// cropped to their ink and fit to the primary cell at render time, so the
/// source font's own cell dimensions aren't needed here.
struct BdfFallback {
    font: BdfFont,
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

/// Measure a font's cap height in pixels at the given px size, using the bounding
/// box of a reference capital glyph. Returns `None` if the font contains none of
/// the reference glyphs (e.g. a pure-symbol or emoji font with no Latin capitals).
fn measure_cap_height_px(font: &Font, px: f32) -> Option<f32> {
    for ch in ['H', 'X', 'I', 'E', 'M'] {
        if font.lookup_glyph_index(ch) != 0 {
            let m = font.metrics(ch, px);
            if m.height > 0 {
                return Some(m.height as f32);
            }
        }
    }
    None
}

impl GlyphAtlas {
    /// Create a new atlas from TTF font data
    pub fn new(font_data: &[u8], font_size: f32) -> Result<Self, AtlasError> {
        let font = Font::from_bytes(font_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(e.to_string()))?;

        // Get line metrics for proper baseline positioning
        let line_metrics =
            font.horizontal_line_metrics(font_size)
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
            dirty: true,
        })
    }

    /// Create a new atlas from BDF font data
    pub fn from_bdf(bdf_data: &[u8]) -> Result<Self, AtlasError> {
        let font =
            BdfFont::parse(bdf_data).map_err(|e| AtlasError::FontLoadError(e.to_string()))?;

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
            cell_width,
            cell_height,
            font.ascent,
            font.descent,
            font.glyphs.len()
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
            dirty: true,
        })
    }

    /// Get the font size (for TTF) or cell height (for BDF)
    fn primary_font_size(&self) -> f32 {
        match &self.source {
            FontSource::Ttf { font_size, .. } => *font_size,
            FontSource::Bdf { .. } => self.cell_height,
        }
    }

    /// The primary font's cap height in pixels (visual height of capital letters).
    /// Used as the target size for fallback fonts so their glyphs render at the
    /// same visual scale as the primary font's capitals.
    fn primary_cap_height(&self) -> f32 {
        match &self.source {
            FontSource::Ttf { font, font_size } => {
                measure_cap_height_px(font, *font_size).unwrap_or(*font_size * 0.7)
            }
            // BDF has no notion of cap height; approximate as ~70% of the cell.
            FontSource::Bdf { .. } => self.cell_height * 0.7,
        }
    }

    /// Choose the rasterization px size for a fallback TTF so its glyphs render
    /// at the same visual size as the primary font's capitals.
    ///
    /// We match cap height (measured from a reference capital) rather than the
    /// font's ascent-to-descent span. Symbol/emoji fonts reserve large vertical
    /// metrics for tall glyphs, so the old `cell_height / (ascent - descent)`
    /// heuristic shrank ordinary marks like `✗` into thin, aliased smudges.
    /// When the font has no Latin capital to measure (typical for pure symbol or
    /// emoji fonts), fall back to matching the em square (`base_size`).
    fn fallback_px_size(&self, font: &Font) -> f32 {
        let base_size = self.primary_font_size();
        match measure_cap_height_px(font, base_size) {
            Some(fallback_cap) if fallback_cap > 0.0 => {
                base_size * (self.primary_cap_height() / fallback_cap)
            }
            _ => base_size,
        }
    }

    /// Set a fallback font for characters missing from the primary font.
    /// The fallback font size is calculated to match the primary font's cell height.
    pub fn set_fallback(&mut self, fallback_data: &[u8]) -> Result<(), AtlasError> {
        let fallback = Font::from_bytes(fallback_data, FontSettings::default())
            .map_err(|e| AtlasError::FontLoadError(format!("fallback: {}", e)))?;

        // Size the fallback so its capitals match the primary font's cap height.
        let fallback_font_size = self.fallback_px_size(&fallback);

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

        // Size symbols so they match the primary font's cap height. Symbol fonts
        // have no Latin capitals, so this matches the em square instead.
        let symbols_font_size = self.fallback_px_size(&symbols);

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

        // Size emoji to match the primary font's cap height. Emoji fonts have no
        // Latin capitals, so this matches the em square instead.
        let emoji_font_size = self.fallback_px_size(&emoji);

        self.emoji_font = Some(emoji);
        self.emoji_font_size = emoji_font_size;

        tracing::info!(
            "Emoji fallback font configured: size={:.1}",
            emoji_font_size
        );

        Ok(())
    }

    /// Set a BDF fallback font for comprehensive Unicode coverage.
    /// Glyphs are cropped to their ink and fit to the primary cell at render time.
    pub fn set_bdf_fallback(&mut self, bdf_data: &[u8]) -> Result<(), AtlasError> {
        let font = BdfFont::parse(bdf_data)
            .map_err(|e| AtlasError::FontLoadError(format!("bdf fallback: {}", e)))?;

        tracing::info!(
            "BDF fallback font configured: {}x{} source cell, {} glyphs (fitting to {:.0}x{:.0})",
            font.cell_width(),
            font.cell_height(),
            font.glyphs.len(),
            self.cell_width,
            self.cell_height
        );

        self.bdf_fallback = Some(BdfFallback { font });

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
        let (width, height, xmin, ymin, advance, bitmap, source_name): (
            usize,
            usize,
            i32,
            i32,
            f32,
            Vec<u8>,
            &str,
        ) = if primary_has {
            match &self.source {
                FontSource::Ttf { font, font_size } => {
                    let (m, b) = font.rasterize(c, *font_size);
                    // If primary returned empty bitmap, try fallbacks
                    if (m.width == 0 || m.height == 0) && c != ' ' {
                        if fallback_has {
                            let fallback = self.fallback_font.as_ref().unwrap();
                            let (fm, fb) = fallback.rasterize(c, self.fallback_font_size);
                            (
                                fm.width,
                                fm.height,
                                fm.xmin,
                                fm.ymin,
                                self.cell_width,
                                fb,
                                "fallback (primary empty)",
                            )
                        } else if symbols_has {
                            let symbols = self.symbols_font.as_ref().unwrap();
                            let (sm, sb) = symbols.rasterize(c, self.symbols_font_size);
                            (
                                sm.width,
                                sm.height,
                                sm.xmin,
                                sm.ymin,
                                self.cell_width,
                                sb,
                                "symbols (primary empty)",
                            )
                        } else if bdf_fallback_has {
                            self.render_bdf_fallback_glyph(
                                c,
                                is_wide,
                                "bdf fallback (primary empty)",
                            )
                        } else if emoji_has {
                            let emoji = self.emoji_font.as_ref().unwrap();
                            let (em, eb) = emoji.rasterize(c, self.emoji_font_size);
                            (
                                em.width,
                                em.height,
                                em.xmin,
                                em.ymin,
                                self.cell_width,
                                eb,
                                "emoji (primary empty)",
                            )
                        } else {
                            (
                                m.width,
                                m.height,
                                m.xmin,
                                m.ymin,
                                m.advance_width,
                                b,
                                "primary",
                            )
                        }
                    } else {
                        (
                            m.width,
                            m.height,
                            m.xmin,
                            m.ymin,
                            m.advance_width,
                            b,
                            "primary",
                        )
                    }
                }
                FontSource::Bdf { font } => {
                    let glyph = font.get_char(c).unwrap();
                    let bitmap = glyph.render();
                    // BDF offset_y is from baseline (positive = above), fontdue ymin is from baseline (positive = above)
                    (
                        glyph.width as usize,
                        glyph.height as usize,
                        glyph.offset_x,
                        glyph.offset_y,
                        glyph.dwidth_x as f32,
                        bitmap,
                        "primary (bdf)",
                    )
                }
            }
        } else if fallback_has {
            // Primary doesn't have it, try fallback
            let fallback = self.fallback_font.as_ref().unwrap();
            let (m, b) = fallback.rasterize(c, self.fallback_font_size);
            (
                m.width,
                m.height,
                m.xmin,
                m.ymin,
                self.cell_width,
                b,
                "fallback",
            )
        } else if symbols_has {
            // Try symbols font
            let symbols = self.symbols_font.as_ref().unwrap();
            let (m, b) = symbols.rasterize(c, self.symbols_font_size);
            (
                m.width,
                m.height,
                m.xmin,
                m.ymin,
                self.cell_width,
                b,
                "symbols",
            )
        } else if bdf_fallback_has {
            // Try BDF fallback (e.g., Unifont for comprehensive Unicode coverage)
            self.render_bdf_fallback_glyph(c, is_wide, "bdf fallback")
        } else if emoji_has {
            // Try emoji font
            let emoji = self.emoji_font.as_ref().unwrap();
            let (m, b) = emoji.rasterize(c, self.emoji_font_size);
            (
                m.width,
                m.height,
                m.xmin,
                m.ymin,
                self.cell_width,
                b,
                "emoji",
            )
        } else {
            // No font has this glyph - use '?' from primary or fallback
            match &self.source {
                FontSource::Ttf { font, font_size } => {
                    let (m, b) = font.rasterize('?', *font_size);
                    (
                        m.width,
                        m.height,
                        m.xmin,
                        m.ymin,
                        m.advance_width,
                        b,
                        "? (no font has glyph)",
                    )
                }
                FontSource::Bdf { font } => {
                    // Try to get '?' from BDF, otherwise use fallback
                    if let Some(glyph) = font.get_char('?') {
                        let bitmap = glyph.render();
                        (
                            glyph.width as usize,
                            glyph.height as usize,
                            glyph.offset_x,
                            glyph.offset_y,
                            glyph.dwidth_x as f32,
                            bitmap,
                            "? (bdf)",
                        )
                    } else if let Some(fallback) = &self.fallback_font {
                        let (m, b) = fallback.rasterize('?', self.fallback_font_size);
                        (
                            m.width,
                            m.height,
                            m.xmin,
                            m.ymin,
                            self.cell_width,
                            b,
                            "? (fallback)",
                        )
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
                c,
                c as u32,
                source_name,
                width,
                height,
                xmin,
                ymin,
                self.cell_width,
                self.cell_height
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
        self.dirty = true;

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

    /// Render a glyph from the BDF fallback font, fitting it to the primary cell.
    /// For wide characters (CJK, etc.), the target is 2x cell width.
    /// Returns (width, height, xmin, ymin, advance, bitmap, source_name).
    ///
    /// The glyph is cropped to its visible ink first, then scaled to fit the cell
    /// preserving aspect ratio, then centered. Cropping is what removes the source
    /// font's padding: Unifont stores glyphs in an oversized (e.g. 16x16) cell,
    /// and scaling that whole cell left half-width marks like `✓` jammed against
    /// one side with a lopsided gap. Fitting the cropped ink uses the cell evenly.
    fn render_bdf_fallback_glyph(
        &self,
        c: char,
        is_wide: bool,
        source_name: &'static str,
    ) -> (usize, usize, i32, i32, f32, Vec<u8>, &'static str) {
        let fb = self.bdf_fallback.as_ref().unwrap();
        let glyph = fb.font.get_char(c).unwrap();

        // Target cell the glyph should occupy (wide chars span two cells).
        let target_width = if is_wide {
            self.cell_width * 2.0
        } else {
            self.cell_width
        };
        let advance = target_width;

        // Crop to the actual ink so the source font's padding doesn't eat space.
        let Some(cropped) = glyph.render_cropped() else {
            // No ink (e.g. space): nothing to draw.
            return (0, 0, 0, 0, advance, vec![], source_name);
        };

        // Fit the ink into the cell preserving aspect ratio.
        let fit = (target_width.max(1.0) / cropped.width as f32)
            .min(self.cell_height.max(1.0) / cropped.height as f32);
        let scaled_w = ((cropped.width as f32 * fit).round() as u32).max(1);
        let scaled_h = ((cropped.height as f32 * fit).round() as u32).max(1);

        let bitmap = crate::bdf::resample_area(
            &cropped.pixels,
            cropped.width,
            cropped.height,
            scaled_w,
            scaled_h,
        );

        // Center horizontally within the cell.
        let xmin = ((target_width - scaled_w as f32) / 2.0).round() as i32;
        // Center vertically within the cell. The renderer places the glyph top at
        // `baseline_y - height - ymin` and the cell top at `baseline_y - ascent`,
        // so centering gives `ymin = ascent - (cell_height + scaled_h) / 2`.
        let ymin = (self.ascent - (self.cell_height + scaled_h as f32) / 2.0).round() as i32;

        (
            scaled_w as usize,
            scaled_h as usize,
            xmin,
            ymin,
            advance,
            bitmap,
            source_name,
        )
    }

    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }

    /// Whether the atlas texture has changed since the last `clear_dirty()` call.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the atlas as uploaded — clears the dirty flag.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn atlas_dimensions(&self) -> (u32, u32) {
        (self.atlas_width, self.atlas_height)
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }
}
