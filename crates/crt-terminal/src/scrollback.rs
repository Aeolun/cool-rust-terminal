// ABOUTME: Scrollback buffer serialization for session restoration.
// ABOUTME: Captures terminal grid content as compressed data.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use alacritty_terminal::Grid;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Serialized representation of a single cell
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedCell {
    pub c: char,
    pub fg: SerializedColor,
    pub bg: SerializedColor,
    pub flags: u16,
}

/// Simplified color representation for serialization
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SerializedColor {
    Named(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl From<Color> for SerializedColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Named(named) => SerializedColor::Named(named as u8),
            Color::Indexed(idx) => SerializedColor::Indexed(idx),
            Color::Spec(rgb) => SerializedColor::Rgb(rgb.r, rgb.g, rgb.b),
        }
    }
}

impl From<SerializedColor> for Color {
    fn from(color: SerializedColor) -> Self {
        match color {
            SerializedColor::Named(n) => {
                // Convert back to NamedColor
                let named = match n {
                    0 => NamedColor::Black,
                    1 => NamedColor::Red,
                    2 => NamedColor::Green,
                    3 => NamedColor::Yellow,
                    4 => NamedColor::Blue,
                    5 => NamedColor::Magenta,
                    6 => NamedColor::Cyan,
                    7 => NamedColor::White,
                    8 => NamedColor::BrightBlack,
                    9 => NamedColor::BrightRed,
                    10 => NamedColor::BrightGreen,
                    11 => NamedColor::BrightYellow,
                    12 => NamedColor::BrightBlue,
                    13 => NamedColor::BrightMagenta,
                    14 => NamedColor::BrightCyan,
                    15 => NamedColor::BrightWhite,
                    16 => NamedColor::Foreground,
                    17 => NamedColor::Background,
                    18 => NamedColor::Cursor,
                    19 => NamedColor::DimBlack,
                    20 => NamedColor::DimRed,
                    21 => NamedColor::DimGreen,
                    22 => NamedColor::DimYellow,
                    23 => NamedColor::DimBlue,
                    24 => NamedColor::DimMagenta,
                    25 => NamedColor::DimCyan,
                    26 => NamedColor::DimWhite,
                    27 => NamedColor::BrightForeground,
                    28 => NamedColor::DimForeground,
                    _ => NamedColor::Foreground,
                };
                Color::Named(named)
            }
            SerializedColor::Indexed(idx) => Color::Indexed(idx),
            SerializedColor::Rgb(r, g, b) => {
                Color::Spec(alacritty_terminal::vte::ansi::Rgb { r, g, b })
            }
        }
    }
}

/// A serialized line of terminal content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedLine {
    pub cells: Vec<SerializedCell>,
}

/// Complete scrollback data for a pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollbackData {
    pub version: u32,
    pub columns: usize,
    pub lines: Vec<SerializedLine>,
}

impl ScrollbackData {
    pub const CURRENT_VERSION: u32 = 1;

    /// Extract scrollback data from a terminal grid
    pub fn from_grid(grid: &Grid<Cell>) -> Self {
        let columns = grid.columns();
        let topmost = grid.topmost_line();
        let bottommost = grid.bottommost_line();

        let mut lines = Vec::new();

        // Iterate from topmost (oldest history) to bottommost (newest)
        for line_idx in topmost.0..=bottommost.0 {
            let line = Line(line_idx);
            let row = &grid[line];

            let mut cells = Vec::with_capacity(columns);
            for col_idx in 0..columns {
                let cell = &row[Column(col_idx)];
                cells.push(SerializedCell {
                    c: cell.c,
                    fg: cell.fg.into(),
                    bg: cell.bg.into(),
                    flags: cell.flags.bits(),
                });
            }

            lines.push(SerializedLine { cells });
        }

        ScrollbackData {
            version: Self::CURRENT_VERSION,
            columns,
            lines,
        }
    }

    /// Compress scrollback data using zstd
    pub fn compress(&self) -> Result<Vec<u8>, std::io::Error> {
        let json = serde_json::to_vec(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut encoder = zstd::Encoder::new(Vec::new(), 3)?;
        encoder.write_all(&json)?;
        encoder.finish()
    }

    /// Decompress scrollback data
    pub fn decompress(data: &[u8]) -> Result<Self, std::io::Error> {
        let mut decoder = zstd::Decoder::new(data)?;
        let mut json = Vec::new();
        decoder.read_to_end(&mut json)?;

        serde_json::from_slice(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Generate ANSI escape sequences to restore this content to a terminal
    pub fn to_ansi_output(&self) -> Vec<u8> {
        let mut output = Vec::new();

        for line in &self.lines {
            let mut line_str = String::new();
            for cell in &line.cells {
                line_str.push(cell.c);
            }
            // Trim trailing spaces
            let trimmed = line_str.trim_end();
            output.extend_from_slice(trimmed.as_bytes());
            output.push(b'\n');
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_color() {
        let colors = vec![
            SerializedColor::Named(0),
            SerializedColor::Indexed(128),
            SerializedColor::Rgb(255, 128, 64),
        ];

        for color in colors {
            let json = serde_json::to_string(&color).unwrap();
            let restored: SerializedColor = serde_json::from_str(&json).unwrap();
            match (color, restored) {
                (SerializedColor::Named(a), SerializedColor::Named(b)) => assert_eq!(a, b),
                (SerializedColor::Indexed(a), SerializedColor::Indexed(b)) => assert_eq!(a, b),
                (SerializedColor::Rgb(r1, g1, b1), SerializedColor::Rgb(r2, g2, b2)) => {
                    assert_eq!((r1, g1, b1), (r2, g2, b2))
                }
                _ => panic!("Color type mismatch"),
            }
        }
    }

    #[test]
    fn test_compress_decompress() {
        let data = ScrollbackData {
            version: ScrollbackData::CURRENT_VERSION,
            columns: 80,
            lines: vec![
                SerializedLine {
                    cells: vec![SerializedCell {
                        c: 'H',
                        fg: SerializedColor::Named(16),
                        bg: SerializedColor::Named(17),
                        flags: 0,
                    }],
                },
                SerializedLine {
                    cells: vec![SerializedCell {
                        c: 'i',
                        fg: SerializedColor::Named(16),
                        bg: SerializedColor::Named(17),
                        flags: 0,
                    }],
                },
            ],
        };

        let compressed = data.compress().unwrap();
        let restored = ScrollbackData::decompress(&compressed).unwrap();

        assert_eq!(data.version, restored.version);
        assert_eq!(data.columns, restored.columns);
        assert_eq!(data.lines.len(), restored.lines.len());
    }
}
