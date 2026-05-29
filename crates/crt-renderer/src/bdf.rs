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

    /// Render this glyph scaled to a target size using an area-averaging (box)
    /// filter. Returns (scaled_width, scaled_height, scaled_offset_x,
    /// scaled_offset_y, bitmap). The offsets are scaled proportionally to
    /// maintain correct positioning.
    ///
    /// Area averaging is essential when downscaling into small cells (e.g. fitting
    /// 16px Unifont into a 7x14 BDF cell): each destination pixel integrates the
    /// coverage of the source region it maps to, so thin 1px strokes survive as
    /// partial-coverage gray pixels. Nearest-neighbor instead samples one source
    /// pixel per destination and drops the rest, which made thin marks like `✓`
    /// vanish entirely.
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

        let scaled = resample_area(
            &original,
            self.width,
            self.height,
            scaled_width,
            scaled_height,
        );

        ScaledGlyph {
            width: scaled_width,
            height: scaled_height,
            offset_x: scaled_offset_x,
            offset_y: scaled_offset_y,
            dwidth_x: scaled_dwidth_x,
            bitmap: scaled,
        }
    }

    /// Render the glyph and crop it to the tight bounding box of its lit (ink)
    /// pixels. Returns `(ink_width, ink_height, pixels)` where `pixels` is a
    /// row-major `ink_width * ink_height` grayscale buffer, or `None` if the
    /// glyph has no ink (e.g. space).
    ///
    /// Many bitmap fonts (notably Unifont) place glyphs in an oversized cell with
    /// blank padding around the ink. Cropping to the real ink before scaling lets
    /// callers fit the visible mark to the target cell instead of wasting space on
    /// the source font's padding (which otherwise shows up as a lopsided gap).
    pub fn render_cropped(&self) -> Option<CroppedGlyph> {
        if self.width == 0 || self.height == 0 {
            return None;
        }

        let pixels = self.render();
        let w = self.width as usize;
        let h = self.height as usize;

        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        let mut found = false;

        for y in 0..h {
            for x in 0..w {
                if pixels[y * w + x] > 0 {
                    found = true;
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }

        if !found {
            return None;
        }

        let ink_w = max_x - min_x + 1;
        let ink_h = max_y - min_y + 1;
        let mut cropped = vec![0u8; ink_w * ink_h];
        for y in 0..ink_h {
            for x in 0..ink_w {
                cropped[y * ink_w + x] = pixels[(min_y + y) * w + (min_x + x)];
            }
        }

        Some(CroppedGlyph {
            width: ink_w as u32,
            height: ink_h as u32,
            pixels: cropped,
        })
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

/// A glyph rendered and cropped to the tight bounding box of its ink pixels.
#[derive(Debug, Clone)]
pub struct CroppedGlyph {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Resample a grayscale bitmap from `(src_w, src_h)` to `(dst_w, dst_h)` using an
/// area-averaging (box) filter.
///
/// Each destination pixel covers a `[x0, x1) x [y0, y1)` footprint in source
/// space; we accumulate each overlapped source pixel weighted by the overlap
/// area, then normalize. This preserves thin strokes as partial-coverage gray
/// when downscaling (nearest-neighbor would drop them) and degenerates to clean
/// block replication for integer upscales.
pub fn resample_area(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w * dst_h) as usize];
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return dst;
    }

    // Source pixels per destination pixel along each axis.
    let inv_scale_x = src_w as f32 / dst_w as f32;
    let inv_scale_y = src_h as f32 / dst_h as f32;

    for dy in 0..dst_h {
        let sy0 = dy as f32 * inv_scale_y;
        let sy1 = sy0 + inv_scale_y;
        let iy0 = sy0.floor() as u32;
        let iy1 = (sy1.ceil() as u32).min(src_h);

        for dx in 0..dst_w {
            let sx0 = dx as f32 * inv_scale_x;
            let sx1 = sx0 + inv_scale_x;
            let ix0 = sx0.floor() as u32;
            let ix1 = (sx1.ceil() as u32).min(src_w);

            let mut acc = 0.0f32;
            let mut weight = 0.0f32;

            for sy in iy0..iy1 {
                let wy = ((sy + 1) as f32).min(sy1) - (sy as f32).max(sy0);
                if wy <= 0.0 {
                    continue;
                }
                for sx in ix0..ix1 {
                    let wx = ((sx + 1) as f32).min(sx1) - (sx as f32).max(sx0);
                    if wx <= 0.0 {
                        continue;
                    }
                    let w = wx * wy;
                    acc += src[(sy * src_w + sx) as usize] as f32 * w;
                    weight += w;
                }
            }

            dst[(dy * dst_w + dx) as usize] = if weight > 0.0 {
                (acc / weight).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
        }
    }

    dst
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

    /// Cropping must strip blank padding down to the tight ink box, matching the
    /// padding Unifont bakes around glyphs like `✓`.
    #[test]
    fn test_render_cropped_strips_padding() {
        // 8x16 glyph with ink only in columns 2..=5 and rows 4..=11.
        let mut bitmap = Vec::with_capacity(16);
        for row in 0..16usize {
            let mut bytes = [0u8; 1]; // 8 bits => 1 byte
            if (4..=11).contains(&row) {
                // Set columns 2..=5: bits 0x20|0x10|0x08|0x04 = 0x3C
                bytes[0] = 0x3C;
            }
            bitmap.push(bytes.to_vec());
        }

        let glyph = BdfGlyph {
            encoding: 0x2713,
            name: "padded".to_string(),
            dwidth_x: 8,
            width: 8,
            height: 16,
            offset_x: 0,
            offset_y: 0,
            bitmap,
        };

        let cropped = glyph.render_cropped().expect("glyph has ink");
        assert_eq!(cropped.width, 4, "columns 2..=5 => width 4");
        assert_eq!(cropped.height, 8, "rows 4..=11 => height 8");
        assert!(cropped.pixels.iter().all(|&v| v == 255));
        assert_eq!(cropped.pixels.len(), 4 * 8);
    }

    /// A blank glyph (no ink) has no bounding box to crop to.
    #[test]
    fn test_render_cropped_blank_is_none() {
        let glyph = BdfGlyph {
            encoding: 32,
            name: "space".to_string(),
            dwidth_x: 8,
            width: 8,
            height: 16,
            offset_x: 0,
            offset_y: 0,
            bitmap: vec![vec![0u8]; 16],
        };
        assert!(glyph.render_cropped().is_none());
    }

    /// A thin 1px diagonal stroke (like the `✓` in Unifont) must survive an
    /// aggressive downscale into a small cell. Nearest-neighbor sampling used to
    /// drop most of its columns, making the mark vanish; area averaging keeps the
    /// ink as partial-coverage gray.
    #[test]
    fn test_render_scaled_thin_diagonal_survives_downscale() {
        // 16x16 glyph with a single-pixel diagonal from top-left to bottom-right.
        let mut bitmap = Vec::with_capacity(16);
        for row in 0..16usize {
            // Set the bit at column == row. Bitmap rows are left-aligned bytes,
            // 16 bits => 2 bytes per row.
            let mut bytes = [0u8; 2];
            let col = row; // diagonal
            bytes[col / 8] = 0x80 >> (col % 8);
            bitmap.push(bytes.to_vec());
        }

        let glyph = BdfGlyph {
            encoding: 0x2713,
            name: "check".to_string(),
            dwidth_x: 16,
            width: 16,
            height: 16,
            offset_x: 0,
            offset_y: 0,
            bitmap,
        };

        // The full diagonal has 16 lit source pixels.
        let original = glyph.render();
        let source_ink: u32 = original.iter().map(|&v| v as u32).sum();
        assert_eq!(source_ink, 16 * 255);

        // Downscale into a 7x14 cell (the case that used to drop the mark).
        let scaled = glyph.render_scaled(7, 14, 16, 16);
        assert!(scaled.width >= 1 && scaled.height >= 1);

        let scaled_ink: u32 = scaled.bitmap.iter().map(|&v| v as u32).sum();
        assert!(
            scaled_ink > 0,
            "thin diagonal disappeared after downscale (ink={scaled_ink})"
        );

        // Area averaging should preserve roughly area-proportional ink, far more
        // than nearest-neighbor (which routinely dropped most of it).
        let area_ratio = (7.0 * 14.0) / (16.0 * 16.0);
        let expected = source_ink as f32 * area_ratio;
        assert!(
            scaled_ink as f32 > expected * 0.33,
            "too much ink lost: got {scaled_ink}, expected ~{expected}"
        );
    }
}
