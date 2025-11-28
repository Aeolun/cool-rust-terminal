// ABOUTME: BDF (Bitmap Distribution Format) font parser.
// ABOUTME: Loads bitmap fonts directly without rasterization for pixel-perfect rendering.

use std::collections::HashMap;

/// A parsed BDF font
#[derive(Debug, Clone)]
pub struct BdfFont {
    /// Font name from FONT property
    pub name: String,
    /// Pixel size from SIZE or PIXEL_SIZE
    pub pixel_size: u32,
    /// Global bounding box width
    pub bbox_width: u32,
    /// Global bounding box height
    pub bbox_height: u32,
    /// Global X offset
    pub bbox_offset_x: i32,
    /// Global Y offset (typically negative, distance from baseline to bottom)
    pub bbox_offset_y: i32,
    /// Font ascent (from FONT_ASCENT property)
    pub ascent: i32,
    /// Font descent (from FONT_DESCENT property)
    pub descent: i32,
    /// All glyphs indexed by Unicode codepoint
    pub glyphs: HashMap<u32, BdfGlyph>,
}

/// A single glyph in a BDF font
#[derive(Debug, Clone)]
pub struct BdfGlyph {
    /// Unicode codepoint
    pub encoding: u32,
    /// Character name (e.g., "A", "space", "exclam")
    pub name: String,
    /// Device width - how much to advance cursor horizontally
    pub dwidth_x: i32,
    /// Bounding box width in pixels
    pub width: u32,
    /// Bounding box height in pixels
    pub height: u32,
    /// X offset from origin
    pub offset_x: i32,
    /// Y offset from baseline (positive = above baseline)
    pub offset_y: i32,
    /// Bitmap data - each row is a Vec<u8>, bits are left-aligned
    /// Length should be height rows, each row has (width + 7) / 8 bytes
    pub bitmap: Vec<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub enum BdfError {
    #[error("Invalid BDF format: {0}")]
    InvalidFormat(String),
    #[error("Missing required property: {0}")]
    MissingProperty(String),
    #[error("Failed to parse number: {0}")]
    ParseNumber(String),
}

impl BdfFont {
    /// Parse a BDF font from raw bytes
    pub fn parse(data: &[u8]) -> Result<Self, BdfError> {
        let content = std::str::from_utf8(data)
            .map_err(|e| BdfError::InvalidFormat(format!("Invalid UTF-8: {}", e)))?;
        Self::parse_str(content)
    }

    /// Parse a BDF font from a string
    pub fn parse_str(content: &str) -> Result<Self, BdfError> {
        let mut lines = content.lines().peekable();

        // Verify STARTFONT
        let first_line = lines
            .next()
            .ok_or(BdfError::InvalidFormat("Empty file".into()))?;
        if !first_line.starts_with("STARTFONT") {
            return Err(BdfError::InvalidFormat("Missing STARTFONT".into()));
        }

        let mut name = String::new();
        let mut pixel_size = 0u32;
        let mut bbox_width = 0u32;
        let mut bbox_height = 0u32;
        let mut bbox_offset_x = 0i32;
        let mut bbox_offset_y = 0i32;
        let mut ascent = 0i32;
        let mut descent = 0i32;
        let mut glyphs = HashMap::new();

        // Parse header
        for line in lines.by_ref() {
            let line = line.trim();
            if line.starts_with("CHARS ") {
                // Done with header, parse glyphs
                break;
            }

            if let Some(rest) = line.strip_prefix("FONT ") {
                name = rest.to_string();
            } else if let Some(rest) = line.strip_prefix("SIZE ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if !parts.is_empty() {
                    pixel_size = parts[0].parse().unwrap_or(0);
                }
            } else if let Some(rest) = line.strip_prefix("PIXEL_SIZE ") {
                pixel_size = rest.trim().parse().unwrap_or(pixel_size);
            } else if let Some(rest) = line.strip_prefix("FONTBOUNDINGBOX ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() >= 4 {
                    bbox_width = parts[0].parse().unwrap_or(0);
                    bbox_height = parts[1].parse().unwrap_or(0);
                    bbox_offset_x = parts[2].parse().unwrap_or(0);
                    bbox_offset_y = parts[3].parse().unwrap_or(0);
                }
            } else if let Some(rest) = line.strip_prefix("FONT_ASCENT ") {
                ascent = rest.trim().parse().unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("FONT_DESCENT ") {
                descent = rest.trim().parse().unwrap_or(0);
            }
        }

        // If ascent/descent not set, derive from bounding box
        if ascent == 0 && descent == 0 {
            // bbox_offset_y is typically the descent (negative distance from baseline to bottom)
            descent = -bbox_offset_y;
            ascent = bbox_height as i32 - descent;
        }

        // Parse glyphs
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line == "ENDFONT" {
                break;
            }
            if let Some(glyph_name) = line.strip_prefix("STARTCHAR ") {
                if let Some(glyph) = Self::parse_glyph(glyph_name, &mut lines)? {
                    glyphs.insert(glyph.encoding, glyph);
                }
            }
        }

        Ok(BdfFont {
            name,
            pixel_size,
            bbox_width,
            bbox_height,
            bbox_offset_x,
            bbox_offset_y,
            ascent,
            descent,
            glyphs,
        })
    }

    fn parse_glyph<'a, I>(
        name: &str,
        lines: &mut std::iter::Peekable<I>,
    ) -> Result<Option<BdfGlyph>, BdfError>
    where
        I: Iterator<Item = &'a str>,
    {
        let mut encoding: Option<u32> = None;
        let mut dwidth_x = 0i32;
        let mut width = 0u32;
        let mut height = 0u32;
        let mut offset_x = 0i32;
        let mut offset_y = 0i32;
        let mut bitmap = Vec::new();
        let mut in_bitmap = false;

        while let Some(line) = lines.next() {
            let line = line.trim();

            if line == "ENDCHAR" {
                break;
            }

            if in_bitmap {
                // Parse hex bitmap row
                let bytes = Self::parse_hex_row(line)?;
                bitmap.push(bytes);
            } else if let Some(rest) = line.strip_prefix("ENCODING ") {
                let enc: i32 = rest
                    .trim()
                    .parse()
                    .map_err(|_| BdfError::ParseNumber(format!("encoding: {}", rest)))?;
                // Skip negative encodings (they're Adobe-specific)
                if enc < 0 {
                    // Skip to ENDCHAR
                    for skip_line in lines.by_ref() {
                        if skip_line.trim() == "ENDCHAR" {
                            break;
                        }
                    }
                    return Ok(None);
                }
                encoding = Some(enc as u32);
            } else if let Some(rest) = line.strip_prefix("DWIDTH ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if !parts.is_empty() {
                    dwidth_x = parts[0].parse().unwrap_or(0);
                }
            } else if let Some(rest) = line.strip_prefix("BBX ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() >= 4 {
                    width = parts[0].parse().unwrap_or(0);
                    height = parts[1].parse().unwrap_or(0);
                    offset_x = parts[2].parse().unwrap_or(0);
                    offset_y = parts[3].parse().unwrap_or(0);
                }
            } else if line == "BITMAP" {
                in_bitmap = true;
            }
        }

        let encoding = match encoding {
            Some(e) => e,
            None => return Ok(None),
        };

        Ok(Some(BdfGlyph {
            encoding,
            name: name.to_string(),
            dwidth_x,
            width,
            height,
            offset_x,
            offset_y,
            bitmap,
        }))
    }

    fn parse_hex_row(hex: &str) -> Result<Vec<u8>, BdfError> {
        let hex = hex.trim();
        let mut bytes = Vec::new();
        let mut chars = hex.chars().peekable();

        while chars.peek().is_some() {
            let hi = chars
                .next()
                .ok_or_else(|| BdfError::InvalidFormat("Unexpected end of hex".into()))?;
            let lo = chars.next().unwrap_or('0');
            let byte = u8::from_str_radix(&format!("{}{}", hi, lo), 16)
                .map_err(|_| BdfError::InvalidFormat(format!("Invalid hex: {}{}", hi, lo)))?;
            bytes.push(byte);
        }

        Ok(bytes)
    }

    /// Get a glyph by Unicode codepoint
    pub fn get_glyph(&self, codepoint: u32) -> Option<&BdfGlyph> {
        self.glyphs.get(&codepoint)
    }

    /// Get a glyph by char
    pub fn get_char(&self, c: char) -> Option<&BdfGlyph> {
        self.glyphs.get(&(c as u32))
    }

    /// Cell width (typically same as bbox_width for monospace fonts)
    pub fn cell_width(&self) -> u32 {
        self.bbox_width
    }

    /// Cell height (ascent + descent)
    pub fn cell_height(&self) -> u32 {
        (self.ascent + self.descent) as u32
    }
}

impl BdfGlyph {
    /// Render this glyph to a grayscale bitmap.
    /// Returns a Vec<u8> with width * height elements, each 0 or 255.
    pub fn render(&self) -> Vec<u8> {
        let mut pixels = vec![0u8; (self.width * self.height) as usize];

        for (row_idx, row_bytes) in self.bitmap.iter().enumerate() {
            if row_idx >= self.height as usize {
                break;
            }
            for col in 0..self.width as usize {
                let byte_idx = col / 8;
                let bit_idx = 7 - (col % 8);
                if byte_idx < row_bytes.len() {
                    let bit = (row_bytes[byte_idx] >> bit_idx) & 1;
                    if bit == 1 {
                        pixels[row_idx * self.width as usize + col] = 255;
                    }
                }
            }
        }

        pixels
    }

    /// Render this glyph scaled to a target size using nearest-neighbor interpolation.
    /// Returns (scaled_width, scaled_height, scaled_offset_x, scaled_offset_y, bitmap).
    /// The offsets are scaled proportionally to maintain correct positioning.
    pub fn render_scaled(
        &self,
        target_cell_width: u32,
        target_cell_height: u32,
        source_cell_width: u32,
        source_cell_height: u32,
    ) -> ScaledGlyph {
        // Calculate scale factors
        let scale_x = target_cell_width as f32 / source_cell_width as f32;
        let scale_y = target_cell_height as f32 / source_cell_height as f32;

        // Scale glyph dimensions
        let scaled_width = ((self.width as f32 * scale_x).round() as u32).max(1);
        let scaled_height = ((self.height as f32 * scale_y).round() as u32).max(1);

        // Scale offsets
        let scaled_offset_x = (self.offset_x as f32 * scale_x).round() as i32;
        let scaled_offset_y = (self.offset_y as f32 * scale_y).round() as i32;

        // Scale advance width
        let scaled_dwidth_x = (self.dwidth_x as f32 * scale_x).round() as i32;

        // Render original bitmap first
        let original = self.render();

        // If no scaling needed, return original
        if self.width == scaled_width && self.height == scaled_height {
            return ScaledGlyph {
                width: scaled_width,
                height: scaled_height,
                offset_x: scaled_offset_x,
                offset_y: scaled_offset_y,
                dwidth_x: scaled_dwidth_x,
                bitmap: original,
            };
        }

        // Handle zero-size glyphs (like space)
        if self.width == 0 || self.height == 0 {
            return ScaledGlyph {
                width: 0,
                height: 0,
                offset_x: scaled_offset_x,
                offset_y: scaled_offset_y,
                dwidth_x: scaled_dwidth_x,
                bitmap: vec![],
            };
        }

        // Scale using nearest-neighbor
        let mut scaled = vec![0u8; (scaled_width * scaled_height) as usize];

        for dst_y in 0..scaled_height {
            for dst_x in 0..scaled_width {
                // Map destination pixel to source pixel
                let src_x = ((dst_x as f32 / scale_x).floor() as u32).min(self.width - 1);
                let src_y = ((dst_y as f32 / scale_y).floor() as u32).min(self.height - 1);

                let src_idx = (src_y * self.width + src_x) as usize;
                let dst_idx = (dst_y * scaled_width + dst_x) as usize;

                scaled[dst_idx] = original[src_idx];
            }
        }

        ScaledGlyph {
            width: scaled_width,
            height: scaled_height,
            offset_x: scaled_offset_x,
            offset_y: scaled_offset_y,
            dwidth_x: scaled_dwidth_x,
            bitmap: scaled,
        }
    }
}

/// A glyph that has been scaled to a target size
#[derive(Debug, Clone)]
pub struct ScaledGlyph {
    pub width: u32,
    pub height: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub dwidth_x: i32,
    pub bitmap: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BDF: &str = r#"STARTFONT 2.1
FONT -Test-Fixed-Medium-R-Normal--13-120-75-75-C-60-ISO10646-1
SIZE 13 75 75
FONTBOUNDINGBOX 6 13 0 -2
STARTPROPERTIES 2
FONT_ASCENT 11
FONT_DESCENT 2
ENDPROPERTIES
CHARS 2
STARTCHAR space
ENCODING 32
SWIDTH 480 0
DWIDTH 6 0
BBX 6 13 0 -2
BITMAP
00
00
00
00
00
00
00
00
00
00
00
00
00
ENDCHAR
STARTCHAR A
ENCODING 65
SWIDTH 480 0
DWIDTH 6 0
BBX 6 13 0 -2
BITMAP
00
00
20
50
88
88
88
F8
88
88
88
00
00
ENDCHAR
ENDFONT
"#;

    #[test]
    fn test_parse_bdf() {
        let font = BdfFont::parse_str(TEST_BDF).unwrap();
        assert_eq!(font.bbox_width, 6);
        assert_eq!(font.bbox_height, 13);
        assert_eq!(font.ascent, 11);
        assert_eq!(font.descent, 2);
        assert_eq!(font.glyphs.len(), 2);

        let a = font.get_char('A').unwrap();
        assert_eq!(a.encoding, 65);
        assert_eq!(a.width, 6);
        assert_eq!(a.height, 13);
        assert_eq!(a.bitmap.len(), 13);
    }

    #[test]
    fn test_render_glyph() {
        let font = BdfFont::parse_str(TEST_BDF).unwrap();
        let a = font.get_char('A').unwrap();
        let pixels = a.render();

        // Check that 'A' has pixels in expected places
        // Row 2 (0-indexed): 0x20 = 00100000, so pixel at col 2
        assert_eq!(pixels[2 * 6 + 2], 255); // Row 2, col 2
        assert_eq!(pixels[2 * 6], 0); // Row 2, col 0

        // Row 7: 0xF8 = 11111000, pixels at cols 0-4
        assert_eq!(pixels[7 * 6], 255);
        assert_eq!(pixels[7 * 6 + 4], 255);
        assert_eq!(pixels[7 * 6 + 5], 0); // Col 5 is off
    }

    #[test]
    fn test_render_scaled_2x() {
        let font = BdfFont::parse_str(TEST_BDF).unwrap();
        let a = font.get_char('A').unwrap();

        // Scale from 6x13 to 12x26 (2x)
        let scaled = a.render_scaled(12, 26, 6, 13);

        assert_eq!(scaled.width, 12);
        assert_eq!(scaled.height, 26);
        assert_eq!(scaled.bitmap.len(), (12 * 26) as usize);

        // At 2x scale, each original pixel becomes a 2x2 block
        // Original row 2, col 2 had a pixel, so scaled row 4-5, col 4-5 should have pixels
        assert_eq!(scaled.bitmap[4 * 12 + 4], 255);
        assert_eq!(scaled.bitmap[4 * 12 + 5], 255);
        assert_eq!(scaled.bitmap[5 * 12 + 4], 255);
        assert_eq!(scaled.bitmap[5 * 12 + 5], 255);

        // Original row 2, col 0 was empty, so scaled row 4, col 0-1 should be empty
        assert_eq!(scaled.bitmap[4 * 12], 0);
        assert_eq!(scaled.bitmap[4 * 12 + 1], 0);
    }

    #[test]
    fn test_render_scaled_same_size() {
        let font = BdfFont::parse_str(TEST_BDF).unwrap();
        let a = font.get_char('A').unwrap();

        // Scale to same size should return identical bitmap
        let scaled = a.render_scaled(6, 13, 6, 13);
        let original = a.render();

        assert_eq!(scaled.width, 6);
        assert_eq!(scaled.height, 13);
        assert_eq!(scaled.bitmap, original);
    }
}
