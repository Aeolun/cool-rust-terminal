// ABOUTME: Main application entry point.
// ABOUTME: Sets up window, event loop, and coordinates terminal/rendering.

mod config_ui;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use arboard::Clipboard;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Icon, Window, WindowAttributes, WindowId};

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb as AnsiRgb};
use config_ui::{ConfigAction, ConfigUI};
use crt_core::{ColorScheme, Config, ScanlineMode};
use crt_layout::{LayoutTree, PaneId};
use crt_renderer::{EffectParams, RenderCell, Renderer};
use crt_terminal::{TermMode, Terminal};

/// Convert an ANSI color from alacritty_terminal to our [f32; 4] format
fn ansi_color_to_rgba(color: AnsiColor, scheme: &ColorScheme, is_dim: bool) -> [f32; 4] {
    let base = match color {
        AnsiColor::Named(named) => {
            match named {
                // Standard colors 0-7
                NamedColor::Black => scheme.colors[0],
                NamedColor::Red => scheme.colors[1],
                NamedColor::Green => scheme.colors[2],
                NamedColor::Yellow => scheme.colors[3],
                NamedColor::Blue => scheme.colors[4],
                NamedColor::Magenta => scheme.colors[5],
                NamedColor::Cyan => scheme.colors[6],
                NamedColor::White => scheme.colors[7],
                // Bright colors 8-15
                NamedColor::BrightBlack => scheme.colors[8],
                NamedColor::BrightRed => scheme.colors[9],
                NamedColor::BrightGreen => scheme.colors[10],
                NamedColor::BrightYellow => scheme.colors[11],
                NamedColor::BrightBlue => scheme.colors[12],
                NamedColor::BrightMagenta => scheme.colors[13],
                NamedColor::BrightCyan => scheme.colors[14],
                NamedColor::BrightWhite => scheme.colors[15],
                // Dim colors - use the base color at 60%
                NamedColor::DimBlack => dim_color(scheme.colors[0]),
                NamedColor::DimRed => dim_color(scheme.colors[1]),
                NamedColor::DimGreen => dim_color(scheme.colors[2]),
                NamedColor::DimYellow => dim_color(scheme.colors[3]),
                NamedColor::DimBlue => dim_color(scheme.colors[4]),
                NamedColor::DimMagenta => dim_color(scheme.colors[5]),
                NamedColor::DimCyan => dim_color(scheme.colors[6]),
                NamedColor::DimWhite => dim_color(scheme.colors[7]),
                // Special colors
                NamedColor::Foreground | NamedColor::BrightForeground => scheme.foreground,
                NamedColor::DimForeground => dim_color(scheme.foreground),
                NamedColor::Background => scheme.background,
                NamedColor::Cursor => scheme.foreground, // Use foreground for cursor
            }
        }
        AnsiColor::Spec(AnsiRgb { r, g, b }) => {
            // True color RGB
            [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
        }
        AnsiColor::Indexed(idx) => scheme.indexed_color(idx),
    };

    if is_dim {
        dim_color(base)
    } else {
        base
    }
}

/// Apply dim effect to a color (60% brightness)
fn dim_color(color: [f32; 4]) -> [f32; 4] {
    [color[0] * 0.6, color[1] * 0.6, color[2] * 0.6, color[3]]
}

/// Kitty keyboard protocol encoder
mod kitty_keyboard {
    use winit::keyboard::{Key, ModifiersState, NamedKey};

    /// Encode a key event in Kitty keyboard protocol format.
    /// Returns None if the key shouldn't be sent (e.g., modifier-only keys).
    pub fn encode(key: &Key, modifiers: ModifiersState, mode: crate::TermMode) -> Option<Vec<u8>> {
        // Calculate modifier parameter: (flags + 1) where flags = shift*1 + alt*2 + ctrl*4 + super*8
        let mod_flags = modifier_flags(modifiers);
        let report_all = mode.contains(crate::TermMode::REPORT_ALL_KEYS_AS_ESC);
        let app_cursor = mode.contains(crate::TermMode::APP_CURSOR);

        match key {
            Key::Character(s) => {
                if let Some(c) = s.chars().next() {
                    // For single characters, use CSI codepoint ; modifiers u
                    let codepoint = c as u32;

                    if mod_flags > 0 || report_all {
                        // With modifiers: CSI codepoint ; modifiers u
                        Some(format!("\x1b[{};{}u", codepoint, mod_flags + 1).into_bytes())
                    } else {
                        // No modifiers and not reporting all: just send the character
                        Some(s.as_bytes().to_vec())
                    }
                } else {
                    None
                }
            }
            Key::Named(named) => encode_named_key(named, mod_flags, report_all, app_cursor, mode),
            _ => None,
        }
    }

    fn modifier_flags(modifiers: ModifiersState) -> u8 {
        let mut flags = 0u8;
        if modifiers.shift_key() {
            flags |= 1;
        }
        if modifiers.alt_key() {
            flags |= 2;
        }
        if modifiers.control_key() {
            flags |= 4;
        }
        if modifiers.super_key() {
            flags |= 8;
        }
        flags
    }

    fn encode_named_key(
        named: &NamedKey,
        mod_flags: u8,
        report_all: bool,
        app_cursor: bool,
        mode: crate::TermMode,
    ) -> Option<Vec<u8>> {
        // Kitty protocol functional key codepoints and legacy suffixes
        // For cursor keys: suffix is the letter (A/B/C/D), ss3_key indicates if it can use SS3 format
        let (codepoint, legacy_suffix, is_cursor_key): (Option<u32>, Option<&[u8]>, bool) =
            match named {
                NamedKey::Enter => (Some(13), None, false),
                NamedKey::Tab => (Some(9), None, false),
                NamedKey::Backspace => (Some(127), None, false),
                NamedKey::Escape => (Some(27), None, false),
                NamedKey::Space => (Some(32), None, false),
                NamedKey::Delete => (Some(57423), Some(b"3~"), false),
                NamedKey::Insert => (Some(57425), Some(b"2~"), false),
                NamedKey::Home => (Some(57419), Some(b"H"), true),
                NamedKey::End => (Some(57420), Some(b"F"), true),
                NamedKey::PageUp => (Some(57421), Some(b"5~"), false),
                NamedKey::PageDown => (Some(57422), Some(b"6~"), false),
                NamedKey::ArrowUp => (Some(57352), Some(b"A"), true),
                NamedKey::ArrowDown => (Some(57353), Some(b"B"), true),
                NamedKey::ArrowRight => (Some(57354), Some(b"C"), true),
                NamedKey::ArrowLeft => (Some(57351), Some(b"D"), true),
                NamedKey::F1 => (Some(57364), Some(b"P"), true),
                NamedKey::F2 => (Some(57365), Some(b"Q"), true),
                NamedKey::F3 => (Some(57366), Some(b"R"), true),
                NamedKey::F4 => (Some(57367), Some(b"S"), true),
                NamedKey::F5 => (Some(57368), Some(b"15~"), false),
                NamedKey::F6 => (Some(57369), Some(b"17~"), false),
                NamedKey::F7 => (Some(57370), Some(b"18~"), false),
                NamedKey::F8 => (Some(57371), Some(b"19~"), false),
                NamedKey::F9 => (Some(57372), Some(b"20~"), false),
                NamedKey::F10 => (Some(57373), Some(b"21~"), false),
                NamedKey::F11 => (Some(57374), Some(b"23~"), false),
                NamedKey::F12 => (Some(57375), Some(b"24~"), false),
                _ => (None, None, false),
            };

        if let Some(cp) = codepoint {
            // Detect if the app is likely a proper Kitty protocol implementation or crossterm.
            // Crossterm doesn't support REPORT_ASSOCIATED_TEXT, so if it's requested,
            // the app is probably spec-compliant and expects proper CSI u codepoints.
            // Otherwise, use legacy format for functional keys since crossterm doesn't
            // correctly parse Kitty's functional key codepoints (57351-57354 for arrows).
            let report_associated_text = mode.contains(crate::TermMode::REPORT_ASSOCIATED_TEXT);
            let is_functional_key = legacy_suffix.is_some();
            let use_legacy_for_functional = is_functional_key && !report_associated_text;

            if report_all && !use_legacy_for_functional {
                // Full Kitty mode with spec-compliant app: use CSI u format
                Some(format!("\x1b[{};{}u", cp, mod_flags + 1).into_bytes())
            } else if mod_flags > 0 {
                // Disambiguate mode with modifiers: use legacy format with modifiers
                if let Some(suffix) = legacy_suffix {
                    if suffix.ends_with(b"~") {
                        // For keys with ~ suffix: CSI number ; modifiers ~
                        let suffix_str = String::from_utf8_lossy(suffix);
                        let number = suffix_str.trim_end_matches('~');
                        Some(format!("\x1b[{};{}~", number, mod_flags + 1).into_bytes())
                    } else {
                        // For single-letter suffix: CSI 1 ; modifiers letter
                        Some(
                            format!(
                                "\x1b[1;{}{}",
                                mod_flags + 1,
                                String::from_utf8_lossy(suffix)
                            )
                            .into_bytes(),
                        )
                    }
                } else {
                    // No legacy suffix (Enter, Tab, etc. with modifiers), use CSI u
                    Some(format!("\x1b[{};{}u", cp, mod_flags + 1).into_bytes())
                }
            } else {
                // No modifiers: use legacy format for compatibility
                match named {
                    NamedKey::Enter => Some(vec![b'\r']),
                    NamedKey::Tab => Some(vec![b'\t']),
                    NamedKey::Backspace => Some(vec![0x7f]),
                    NamedKey::Escape => Some(vec![0x1b]),
                    NamedKey::Space => Some(vec![b' ']),
                    _ => {
                        // Use legacy escape sequence
                        if let Some(suffix) = legacy_suffix {
                            // When APP_CURSOR (DECCKM) is set, cursor keys use SS3 format
                            if app_cursor && is_cursor_key && suffix.len() == 1 {
                                let mut seq = vec![0x1b, b'O'];
                                seq.extend_from_slice(suffix);
                                Some(seq)
                            } else {
                                let mut seq = vec![0x1b, b'['];
                                seq.extend_from_slice(suffix);
                                Some(seq)
                            }
                        } else {
                            None
                        }
                    }
                }
            }
        } else {
            None
        }
    }
}

const PANE_PADDING: f32 = 8.0; // Pixels of padding around each pane's content

/// Buffer-relative cell position (row can be negative for scrollback history)
#[derive(Clone, Copy, Debug, Default)]
struct CellPos {
    col: usize,
    /// Buffer-relative row: 0 = first screen line when not scrolled,
    /// negative = scrollback history, positive when scrolled up
    row: i32,
}

#[derive(Default)]
struct Selection {
    start: CellPos,
    end: CellPos,
    active: bool,
}

impl Selection {
    fn normalized(&self) -> (CellPos, CellPos) {
        let (start_row, end_row, start_col, end_col) = if self.start.row < self.end.row
            || (self.start.row == self.end.row && self.start.col <= self.end.col)
        {
            (self.start.row, self.end.row, self.start.col, self.end.col)
        } else {
            (self.end.row, self.start.row, self.end.col, self.start.col)
        };
        (
            CellPos {
                col: start_col,
                row: start_row,
            },
            CellPos {
                col: end_col,
                row: end_row,
            },
        )
    }

    /// Check if a buffer-relative position is within the selection
    fn contains(&self, col: usize, row: i32) -> bool {
        // Never highlight a single cell (click without drag)
        if self.start.row == self.end.row && self.start.col == self.end.col {
            return false;
        }
        let (start, end) = self.normalized();
        if row < start.row || row > end.row {
            return false;
        }
        if start.row == end.row {
            col >= start.col && col <= end.col
        } else if row == start.row {
            col >= start.col
        } else if row == end.row {
            col <= end.col
        } else {
            true
        }
    }
}

const RESIZE_INDICATOR_DURATION: Duration = Duration::from_millis(1000);
const SCROLLBAR_FADE_DURATION: Duration = Duration::from_millis(1500);
const SCROLLBAR_VISIBLE_DURATION: Duration = Duration::from_millis(800);
const DEFAULT_FPS: u32 = 60; // Fallback if we can't detect refresh rate
const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(500);

// Startup hint timing (after power-on animation)
const POWERON_DURATION: f32 = 1.05; // Must match shader's POWERON_TOTAL
const STARTUP_HINT_DELAY: f32 = POWERON_DURATION;
const STARTUP_HINT_DURATION: f32 = 2.0;
const STARTUP_HINT_FADE: f32 = 0.5;

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    layout: LayoutTree,
    terminals: HashMap<PaneId, Terminal>,
    modifiers: ModifiersState,
    selection: Selection,
    mouse_pos: (f64, f64),
    clipboard: Option<Clipboard>,
    last_grid: Vec<Vec<char>>,
    last_resize: Option<Instant>,
    last_scroll: HashMap<PaneId, Instant>,
    last_frame: Instant,
    frame_duration: Duration,
    fps_samples: [f32; 60],
    fps_sample_idx: usize,
    app_start: Instant,
    config: Config,
    config_ui: ConfigUI,
    debug_grid: bool,
    beam_paused: bool,
    beam_step_held: bool,    // Is step key currently held
    beam_step_delay_ms: u32, // Delay between steps when holding (in ms)
    beam_step_last: Instant, // Last time we stepped
    last_click_time: Option<Instant>,
    last_click_pos: Option<CellPos>,
    click_count: u8,
    /// Track Kitty keyboard protocol state per pane for change detection
    kitty_mode_state: HashMap<PaneId, bool>,
    /// When to show the Kitty protocol message (pane_id, start_time, enabled, crossterm_compat)
    kitty_mode_message: Option<(PaneId, Instant, bool, bool)>,
}

impl App {
    fn new() -> Self {
        let config = Config::load_or_default();
        tracing::info!("Loaded config: per_pane_crt={}", config.per_pane_crt);

        Self {
            window: None,
            renderer: None,
            layout: LayoutTree::new(),
            terminals: HashMap::new(),
            modifiers: ModifiersState::empty(),
            selection: Selection::default(),
            mouse_pos: (0.0, 0.0),
            clipboard: Clipboard::new().ok(),
            last_grid: Vec::new(),
            last_resize: None,
            last_scroll: HashMap::new(),
            last_frame: Instant::now(),
            frame_duration: Duration::from_nanos(1_000_000_000 / (DEFAULT_FPS * 2) as u64),
            fps_samples: [0.0; 60],
            fps_sample_idx: 0,
            app_start: Instant::now(),
            config_ui: ConfigUI::new(config.clone()),
            config,
            debug_grid: false,
            beam_paused: false,
            beam_step_held: false,
            beam_step_delay_ms: 100, // Start at 100ms between steps
            beam_step_last: Instant::now(),
            last_click_time: None,
            last_click_pos: None,
            kitty_mode_state: HashMap::new(),
            kitty_mode_message: None,
            click_count: 0,
        }
    }

    /// Record a frame time sample and return the average FPS
    fn record_frame_time(&mut self, dt: f32) -> f32 {
        self.fps_samples[self.fps_sample_idx] = dt;
        self.fps_sample_idx = (self.fps_sample_idx + 1) % self.fps_samples.len();

        let sum: f32 = self.fps_samples.iter().sum();
        let avg_dt = sum / self.fps_samples.len() as f32;
        if avg_dt > 0.0 {
            1.0 / avg_dt
        } else {
            0.0
        }
    }

    /// Returns the currently active config - either the preview config if
    /// the settings UI is open, or the saved config otherwise.
    fn current_config(&self) -> &Config {
        if self.config_ui.visible {
            &self.config_ui.config
        } else {
            &self.config
        }
    }

    /// Convert pixel coordinates to cell position, also returns debug info:
    /// Returns None if pointing at the void (outside CRT content area)
    /// Otherwise returns (cell_pos, content_pixel, pane_local_pixel, pane_offset)
    #[allow(clippy::type_complexity)]
    fn pixel_to_cell_debug(
        &self,
        x: f64,
        y: f64,
    ) -> Option<(CellPos, (f64, f64), (f64, f64), (f64, f64))> {
        let Some(renderer) = &self.renderer else {
            return None;
        };

        let curvature = self.current_config().effects.screen_curvature as f64;
        let per_pane_crt = self.current_config().per_pane_crt;
        let (win_width, win_height) = renderer.window_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);
        let focused = self.layout.focused_pane();

        let rect = rects.get(&focused)?;

        // Pane bounds in pixels (with padding)
        let pane_x = (rect.x * win_width as f32 + PANE_PADDING) as f64;
        let pane_y = (rect.y * win_height as f32 + PANE_PADDING) as f64;
        let pane_w = (rect.width * win_width as f32 - PANE_PADDING * 2.0) as f64;
        let pane_h = (rect.height * win_height as f32 - PANE_PADDING * 2.0) as f64;

        let (content_x, content_y) = if curvature.abs() < 0.0001 {
            // No distortion
            (x, y)
        } else if per_pane_crt {
            // Per-pane mode: apply distortion in local pane space
            // Convert to local pane UV (0-1)
            let local_uv_x = (x - pane_x) / pane_w;
            let local_uv_y = (y - pane_y) / pane_h;

            // Convert to centered coords (-1 to 1)
            let centered_x = local_uv_x * 2.0 - 1.0;
            let centered_y = local_uv_y * 2.0 - 1.0;

            // Apply barrel distortion
            let r2 = centered_x * centered_x + centered_y * centered_y;
            let scale = 1.0 + curvature * r2;
            let distorted_x = centered_x * scale;
            let distorted_y = centered_y * scale;

            // Convert back to local UV
            let content_local_x = distorted_x * 0.5 + 0.5;
            let content_local_y = distorted_y * 0.5 + 0.5;

            // Check if in void
            if !(0.0..=1.0).contains(&content_local_x) || !(0.0..=1.0).contains(&content_local_y) {
                return None;
            }

            // Convert back to global pixel coords
            (
                pane_x + content_local_x * pane_w,
                pane_y + content_local_y * pane_h,
            )
        } else {
            // Whole-screen mode: apply distortion globally
            let uv_x = x / win_width as f64;
            let uv_y = y / win_height as f64;

            let centered_x = uv_x * 2.0 - 1.0;
            let centered_y = uv_y * 2.0 - 1.0;

            let r2 = centered_x * centered_x + centered_y * centered_y;
            let scale = 1.0 + curvature * r2;
            let distorted_x = centered_x * scale;
            let distorted_y = centered_y * scale;

            let content_uv_x = distorted_x * 0.5 + 0.5;
            let content_uv_y = distorted_y * 0.5 + 0.5;

            if !(0.0..=1.0).contains(&content_uv_x) || !(0.0..=1.0).contains(&content_uv_y) {
                return None;
            }

            (
                content_uv_x * win_width as f64,
                content_uv_y * win_height as f64,
            )
        };

        let (cell_w, cell_h) = renderer.cell_size();
        let local_x = content_x - pane_x;
        let local_y = content_y - pane_y;
        let col = (local_x / cell_w as f64).floor().max(0.0) as usize;
        let screen_row = (local_y / cell_h as f64).floor().max(0.0) as i32;

        // Convert screen row to buffer-relative row
        let display_offset = self
            .terminals
            .get(&focused)
            .map(|t| t.display_offset() as i32)
            .unwrap_or(0);
        let row = screen_row - display_offset;

        Some((
            CellPos { col, row },
            (content_x, content_y),
            (local_x, local_y),
            (pane_x, pane_y),
        ))
    }

    fn pixel_to_cell(&self, x: f64, y: f64) -> Option<CellPos> {
        self.pixel_to_cell_debug(x, y).map(|(pos, _, _, _)| pos)
    }

    fn pixel_to_normalized(&self, x: f64, y: f64) -> (f32, f32) {
        let Some(renderer) = &self.renderer else {
            return (0.0, 0.0);
        };
        let (win_width, win_height) = renderer.window_size();
        (
            (x / win_width as f64) as f32,
            (y / win_height as f64) as f32,
        )
    }

    fn copy_selection(&mut self) {
        let focused = self.layout.focused_pane();
        let Some(terminal) = self.terminals.get(&focused) else {
            return;
        };

        let (start, end) = self.selection.normalized();

        // Read directly from terminal grid using buffer-relative coordinates
        let text = terminal.with_grid(|grid| {
            use alacritty_terminal::grid::Dimensions;
            use alacritty_terminal::index::{Column, Line};
            use alacritty_terminal::term::cell::Flags;
            let cols = grid.columns();
            let mut text = String::new();

            for row in start.row..=end.row {
                let line = Line(row);
                let col_start = if row == start.row { start.col } else { 0 };
                let col_end = if row == end.row {
                    end.col.min(cols.saturating_sub(1))
                } else {
                    cols.saturating_sub(1)
                };

                for col in col_start..=col_end {
                    let cell = &grid[line][Column(col)];
                    let c = cell.c;
                    if c != ' ' && c != '\0' {
                        text.push(c);
                    } else if c == ' ' {
                        text.push(' ');
                    }
                }
                // Only add newline if this row wasn't soft-wrapped
                if row != end.row {
                    let last_cell = &grid[line][Column(cols - 1)];
                    if !last_cell.flags.contains(Flags::WRAPLINE) {
                        text.push('\n');
                    }
                }
            }
            text
        });

        // Trim trailing whitespace from each line but keep structure
        let trimmed: String = text
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        if let Some(clipboard) = &mut self.clipboard {
            if let Err(e) = clipboard.set_text(&trimmed) {
                tracing::error!("Failed to copy to clipboard: {}", e);
            } else {
                tracing::info!("Copied {} chars to clipboard", trimmed.len());
            }
        }
    }

    /// Find word boundaries around the given position.
    /// Returns (start, end) positions that encompass the word.
    fn find_word_boundaries(&self, pos: CellPos) -> Option<(CellPos, CellPos)> {
        let focused = self.layout.focused_pane();
        let terminal = self.terminals.get(&focused)?;

        terminal.with_grid(|grid| {
            use alacritty_terminal::grid::Dimensions;
            use alacritty_terminal::index::{Column, Line};
            let cols = grid.columns();
            let line = Line(pos.row);

            // Check if the clicked position has a non-whitespace character
            let clicked_char = grid[line][Column(pos.col)].c;
            if clicked_char.is_whitespace() || clicked_char == '\0' {
                return None;
            }

            // Scan left to find word start
            let mut start_col = pos.col;
            while start_col > 0 {
                let c = grid[line][Column(start_col - 1)].c;
                if c.is_whitespace() || c == '\0' {
                    break;
                }
                start_col -= 1;
            }

            // Scan right to find word end
            let mut end_col = pos.col;
            while end_col < cols - 1 {
                let c = grid[line][Column(end_col + 1)].c;
                if c.is_whitespace() || c == '\0' {
                    break;
                }
                end_col += 1;
            }

            Some((
                CellPos {
                    col: start_col,
                    row: pos.row,
                },
                CellPos {
                    col: end_col,
                    row: pos.row,
                },
            ))
        })
    }

    /// Find line boundaries for the given position.
    /// Returns (start, end) positions that encompass the line content (excluding trailing whitespace).
    fn find_line_boundaries(&self, pos: CellPos) -> Option<(CellPos, CellPos)> {
        let focused = self.layout.focused_pane();
        let terminal = self.terminals.get(&focused)?;

        terminal.with_grid(|grid| {
            use alacritty_terminal::grid::Dimensions;
            use alacritty_terminal::index::{Column, Line};
            let cols = grid.columns();
            let line = Line(pos.row);

            // Find the last non-whitespace column
            let mut end_col = 0;
            for col in 0..cols {
                let c = grid[line][Column(col)].c;
                if !c.is_whitespace() && c != '\0' {
                    end_col = col;
                }
            }

            Some((
                CellPos {
                    col: 0,
                    row: pos.row,
                },
                CellPos {
                    col: end_col,
                    row: pos.row,
                },
            ))
        })
    }

    fn create_terminal_for_pane(&mut self, pane_id: PaneId) {
        let Some(renderer) = &self.renderer else {
            return;
        };

        let (win_width, win_height) = renderer.window_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);

        if let Some(rect) = rects.get(&pane_id) {
            // Subtract padding from usable area
            let pane_width = ((rect.width * win_width as f32) - PANE_PADDING * 2.0).max(1.0) as u32;
            let pane_height =
                ((rect.height * win_height as f32) - PANE_PADDING * 2.0).max(1.0) as u32;
            let (cols, rows) = renderer.grid_size_for_region(pane_width, pane_height);

            match Terminal::new(cols, rows) {
                Ok(terminal) => {
                    self.terminals.insert(pane_id, terminal);
                    tracing::info!(
                        "Created terminal for pane {:?} ({}x{} cells)",
                        pane_id,
                        cols,
                        rows
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to create terminal: {}", e);
                }
            }
        }
    }

    fn resize_terminals(&mut self) {
        let Some(renderer) = &self.renderer else {
            return;
        };

        let (win_width, win_height) = renderer.window_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);

        for (pane_id, terminal) in &self.terminals {
            if let Some(rect) = rects.get(pane_id) {
                // Subtract padding from usable area
                let pane_width =
                    ((rect.width * win_width as f32) - PANE_PADDING * 2.0).max(1.0) as u32;
                let pane_height =
                    ((rect.height * win_height as f32) - PANE_PADDING * 2.0).max(1.0) as u32;
                let (cols, rows) = renderer.grid_size_for_region(pane_width, pane_height);
                terminal.resize(cols, rows);
            }
        }
    }

    fn render_terminals(&mut self, dt: f32) {
        // Record frame time for FPS display
        let fps = self.record_frame_time(dt);

        // Get mouse debug info before mutable borrow (None if in the void or debug disabled)
        let mouse_debug = if self.debug_grid {
            self.pixel_to_cell_debug(self.mouse_pos.0, self.mouse_pos.1)
        } else {
            None
        };

        // Fetch config values before mutable borrow of renderer
        let current_cfg = self.current_config();
        let color_scheme = current_cfg.color_scheme.clone();
        let per_pane_crt = current_cfg.per_pane_crt;

        let Some(renderer) = &mut self.renderer else {
            return;
        };

        let (win_width, win_height) = renderer.window_size();
        let (cell_w, cell_h) = renderer.cell_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);
        let focused_pane = self.layout.focused_pane();

        let mut pane_renders: Vec<(f32, f32, Vec<Vec<RenderCell>>)> = Vec::new();

        for pane_id in self.layout.panes() {
            let Some(rect) = rects.get(pane_id) else {
                continue;
            };
            let Some(terminal) = self.terminals.get(pane_id) else {
                continue;
            };

            // Check for Kitty keyboard protocol state changes
            let term_mode = terminal.term_mode();
            let kitty_enabled = term_mode.contains(TermMode::DISAMBIGUATE_ESC_CODES);
            let prev_state = self.kitty_mode_state.get(pane_id).copied();
            if prev_state != Some(kitty_enabled) {
                self.kitty_mode_state.insert(*pane_id, kitty_enabled);
                // Only show message if this isn't the initial state detection
                if prev_state.is_some() {
                    // Crossterm compat mode: REPORT_ASSOCIATED_TEXT not requested
                    let crossterm_compat =
                        kitty_enabled && !term_mode.contains(TermMode::REPORT_ASSOCIATED_TEXT);
                    self.kitty_mode_message =
                        Some((*pane_id, Instant::now(), kitty_enabled, crossterm_compat));
                    tracing::info!(
                        "Kitty keyboard protocol {} for pane {:?}{}",
                        if kitty_enabled { "enabled" } else { "disabled" },
                        pane_id,
                        if crossterm_compat {
                            " (crossterm compat)"
                        } else {
                            ""
                        }
                    );
                }
            }

            // Add padding offset, rounded to integer pixels for crisp bitmap font rendering
            let x_offset = (rect.x * win_width as f32 + PANE_PADDING).floor();
            let y_offset = (rect.y * win_height as f32 + PANE_PADDING).floor();

            // Only show cursor in focused pane
            let is_focused = *pane_id == focused_pane;

            let cursor_pos = terminal.cursor_position();
            let selection = &self.selection;

            let cells = terminal.with_grid(|grid| {
                use alacritty_terminal::grid::Dimensions;
                use alacritty_terminal::index::{Column, Line};
                use alacritty_terminal::term::cell::Flags;

                let grid_cols = grid.columns();
                let grid_lines = grid.screen_lines();
                let display_offset = grid.display_offset() as i32;

                let mut rows: Vec<Vec<RenderCell>> = Vec::with_capacity(grid_lines);

                for line_idx in 0..grid_lines {
                    let mut row = Vec::with_capacity(grid_cols);
                    // When scrolled (display_offset > 0), access history with negative line indices
                    let line = Line(line_idx as i32 - display_offset);

                    for col_idx in 0..grid_cols {
                        let cell = &grid[line][Column(col_idx)];
                        let c = cell.c;
                        let flags = cell.flags;

                        // Skip wide char spacer cells - the wide char in the adjacent cell
                        // visually extends into this space
                        if flags.contains(Flags::WIDE_CHAR_SPACER)
                            || flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
                        {
                            row.push(RenderCell {
                                c: ' ',
                                fg: [0.0, 0.0, 0.0, 0.0],
                                bg: [0.0, 0.0, 0.0, 0.0],
                                is_wide: false,
                            });
                            continue;
                        }

                        let is_wide = flags.contains(Flags::WIDE_CHAR);

                        // Check if this cell is the cursor position
                        let is_cursor = if let Some((cursor_col, cursor_line)) = cursor_pos {
                            // Cursor is at grid Line(cursor_line). We're displaying Line(line_idx - display_offset).
                            // So cursor appears when line_idx - display_offset == cursor_line, i.e., line_idx == cursor_line + display_offset
                            let cursor_display_line = cursor_line as i32 + display_offset;
                            is_focused
                                && cursor_display_line >= 0
                                && line_idx == cursor_display_line as usize
                                && col_idx == cursor_col
                        } else {
                            false
                        };
                        // Selection uses buffer-relative rows (screen_row - display_offset)
                        let buffer_row = line_idx as i32 - display_offset;
                        let is_selected = is_focused && selection.contains(col_idx, buffer_row);
                        let is_dim = cell.flags.contains(Flags::DIM);
                        let is_inverse = cell.flags.contains(Flags::INVERSE);

                        // Get the cell's actual colors from terminal state
                        let mut cell_fg = ansi_color_to_rgba(cell.fg, &color_scheme, is_dim);

                        // Check if cell has an explicit background (not the default Background)
                        let has_explicit_bg =
                            !matches!(cell.bg, AnsiColor::Named(NamedColor::Background));
                        let mut cell_bg = if has_explicit_bg {
                            ansi_color_to_rgba(cell.bg, &color_scheme, false)
                        } else {
                            [0.0, 0.0, 0.0, 0.0] // Transparent for default background
                        };

                        // Handle inverse video (swap fg/bg)
                        if is_inverse {
                            // For inverse, if bg was transparent, use actual background color
                            if !has_explicit_bg {
                                cell_bg = color_scheme.background;
                            }
                            std::mem::swap(&mut cell_fg, &mut cell_bg);
                        }

                        // Apply special rendering states (cursor and selection invert colors)
                        // Resolve transparent background to scheme background for inversion
                        let resolved_bg = if cell_bg[3] < 0.01 {
                            color_scheme.background
                        } else {
                            cell_bg
                        };

                        let (fg, bg) = if is_cursor || is_selected {
                            // Invert: swap fg and bg
                            (resolved_bg, cell_fg)
                        } else {
                            (cell_fg, cell_bg)
                        };

                        row.push(RenderCell { c, fg, bg, is_wide });
                    }

                    rows.push(row);
                }

                rows
            });

            // Update last_grid for copy operations on the focused pane
            if is_focused {
                self.last_grid = cells
                    .iter()
                    .map(|row| row.iter().map(|cell| cell.c).collect())
                    .collect();
            }

            pane_renders.push((x_offset, y_offset, cells));
        }

        // Calculate separators from pane boundaries
        // Format: (x, y, length, is_vertical)
        let mut separators: Vec<(f32, f32, f32, bool)> = Vec::new();
        if self.layout.panes().len() > 1 {
            let rect_list: Vec<_> = rects.values().collect();

            // For each pair of panes, check if they share an edge
            for i in 0..rect_list.len() {
                for j in (i + 1)..rect_list.len() {
                    let r1 = rect_list[i];
                    let r2 = rect_list[j];

                    // Check for vertical separator (panes side by side)
                    // r1's right edge meets r2's left edge
                    let r1_right = r1.x + r1.width;
                    let r2_right = r2.x + r2.width;

                    if (r1_right - r2.x).abs() < 0.01 {
                        // r1 is to the left of r2
                        // Find overlapping Y range
                        let y_start = r1.y.max(r2.y);
                        let y_end = (r1.y + r1.height).min(r2.y + r2.height);
                        if y_end > y_start {
                            let x_px = r1_right * win_width as f32;
                            let y_start_px = y_start * win_height as f32;
                            let length = (y_end - y_start) * win_height as f32;
                            separators.push((x_px, y_start_px, length, true));
                        }
                    } else if (r2_right - r1.x).abs() < 0.01 {
                        // r2 is to the left of r1
                        let y_start = r1.y.max(r2.y);
                        let y_end = (r1.y + r1.height).min(r2.y + r2.height);
                        if y_end > y_start {
                            let x_px = r2_right * win_width as f32;
                            let y_start_px = y_start * win_height as f32;
                            let length = (y_end - y_start) * win_height as f32;
                            separators.push((x_px, y_start_px, length, true));
                        }
                    }

                    // Check for horizontal separator (panes stacked)
                    // r1's bottom edge meets r2's top edge
                    let r1_bottom = r1.y + r1.height;
                    let r2_bottom = r2.y + r2.height;

                    if (r1_bottom - r2.y).abs() < 0.01 {
                        // r1 is above r2
                        let x_start = r1.x.max(r2.x);
                        let x_end = (r1.x + r1.width).min(r2.x + r2.width);
                        if x_end > x_start {
                            let y_px = r1_bottom * win_height as f32;
                            let x_start_px = x_start * win_width as f32;
                            let length = (x_end - x_start) * win_width as f32;
                            separators.push((x_start_px, y_px, length, false));
                        }
                    } else if (r2_bottom - r1.y).abs() < 0.01 {
                        // r2 is above r1
                        let x_start = r1.x.max(r2.x);
                        let x_end = (r1.x + r1.width).min(r2.x + r2.width);
                        if x_end > x_start {
                            let y_px = r2_bottom * win_height as f32;
                            let x_start_px = x_start * win_width as f32;
                            let length = (x_end - x_start) * win_width as f32;
                            separators.push((x_start_px, y_px, length, false));
                        }
                    }
                }
            }
        }

        // Convert to the format render_panes expects
        let panes: Vec<(f32, f32, &[Vec<RenderCell>])> = pane_renders
            .iter()
            .map(|(x, y, cells)| (*x, *y, cells.as_slice()))
            .collect();

        // Calculate focus rectangle (only show when multiple panes)
        let focus_rect = if self.layout.panes().len() > 1 {
            rects.get(&focused_pane).map(|rect| {
                (
                    rect.x * win_width as f32,
                    rect.y * win_height as f32,
                    rect.width * win_width as f32,
                    rect.height * win_height as f32,
                )
            })
        } else {
            None
        };

        // Calculate indicators (show during resize)
        let show_resize = self
            .last_resize
            .is_some_and(|t| t.elapsed() < RESIZE_INDICATOR_DURATION);

        let mut size_indicators: Vec<(f32, f32, String)> = if show_resize {
            self.layout
                .panes()
                .iter()
                .filter_map(|pane_id| {
                    let rect = rects.get(pane_id)?;
                    let terminal = self.terminals.get(pane_id)?;
                    let center_x = (rect.x + rect.width / 2.0) * win_width as f32;
                    let center_y = (rect.y + rect.height / 2.0) * win_height as f32;

                    let (cols, rows) = terminal.size();
                    Some((center_x, center_y, format!("{}x{}", cols, rows)))
                })
                .collect()
        } else {
            Vec::new()
        };

        // Add FPS counter in bottom-left when debug grid is enabled
        if self.debug_grid {
            let fps_text = format!("{:.0} FPS", fps);
            let text_width = fps_text.len() as f32 * cell_w;
            // Position: bottom-left, with some padding
            let x = text_width / 2.0 + cell_w;
            let y = win_height as f32 - cell_h * 1.5;
            size_indicators.push((x, y, fps_text));
        }

        // Add startup hint after power-on animation
        if self.config.behavior.show_startup_hint && !self.config_ui.visible {
            let elapsed = self.app_start.elapsed().as_secs_f32();
            let hint_start = STARTUP_HINT_DELAY;
            let hint_end = hint_start + STARTUP_HINT_DURATION + STARTUP_HINT_FADE;

            if elapsed >= hint_start && elapsed < hint_end {
                // Position in center of focused pane
                if let Some(rect) = rects.get(&focused_pane) {
                    let center_x = (rect.x + rect.width / 2.0) * win_width as f32;
                    let center_y = (rect.y + rect.height / 2.0) * win_height as f32;
                    // Show version and hint lines
                    size_indicators.push((
                        center_x,
                        center_y - cell_h * 2.0,
                        format!("Cool Rust Term v{}", env!("CARGO_PKG_VERSION")),
                    ));
                    size_indicators.push((center_x, center_y, "Ctrl+, for settings".to_string()));
                    size_indicators.push((
                        center_x,
                        center_y + cell_h * 1.5,
                        "Ctrl+Shift+Enter for new pane".to_string(),
                    ));
                }
            }
        }

        // Show Kitty keyboard protocol status message (top right of pane)
        const KITTY_MSG_DURATION: f32 = 1.5;
        if self.config.behavior.show_kitty_message {
            if let Some((pane_id, start_time, enabled, crossterm_compat)) = self.kitty_mode_message
            {
                let elapsed = start_time.elapsed().as_secs_f32();
                if elapsed < KITTY_MSG_DURATION {
                    if let Some(rect) = rects.get(&pane_id) {
                        let msg = if enabled {
                            "Kitty keyboard protocol enabled"
                        } else {
                            "Kitty keyboard protocol disabled"
                        };
                        // Position at top right, accounting for message width
                        let msg_width = msg.len() as f32 * cell_w;
                        let x = (rect.x + rect.width) * win_width as f32
                            - msg_width / 2.0
                            - PANE_PADDING;
                        let y = rect.y * win_height as f32 + cell_h + PANE_PADDING;
                        size_indicators.push((x, y, msg.to_string()));

                        // Show crossterm compat indicator on second line
                        if crossterm_compat {
                            let compat_msg = "(crossterm compat)";
                            let compat_width = compat_msg.len() as f32 * cell_w;
                            let compat_x = (rect.x + rect.width) * win_width as f32
                                - compat_width / 2.0
                                - PANE_PADDING;
                            let compat_y = y + cell_h * 1.2;
                            size_indicators.push((compat_x, compat_y, compat_msg.to_string()));
                        }
                    }
                } else {
                    // Message expired, clear it
                    self.kitty_mode_message = None;
                }
            }
        }

        // Collect normalized pane rects for CRT shader and find focused pane index
        let mut focused_pane_index: i32 = -1;
        let pane_rects_normalized: Vec<(f32, f32, f32, f32)> = self
            .layout
            .panes()
            .iter()
            .enumerate()
            .filter_map(|(i, pane_id)| {
                let rect = rects.get(pane_id)?;
                if *pane_id == focused_pane {
                    focused_pane_index = i as i32;
                }
                Some((rect.x, rect.y, rect.width, rect.height))
            })
            .collect();

        // Calculate scrollbars for each pane (with per-pane opacity based on scroll time)
        // Each scrollbar is (x, y, height, thumb_start, thumb_height, opacity) in pixels
        let scrollbars: Vec<(f32, f32, f32, f32, f32, f32)> = self
            .layout
            .panes()
            .iter()
            .filter_map(|pane_id| {
                let rect = rects.get(pane_id)?;
                let terminal = self.terminals.get(pane_id)?;

                let history = terminal.history_size();
                if history == 0 {
                    return None; // No scrollback, no scrollbar
                }

                // Calculate per-pane scrollbar opacity
                let scrollbar_opacity = self
                    .last_scroll
                    .get(pane_id)
                    .map(|t| {
                        let elapsed = t.elapsed();
                        if elapsed < SCROLLBAR_VISIBLE_DURATION {
                            1.0_f32
                        } else if elapsed < SCROLLBAR_VISIBLE_DURATION + SCROLLBAR_FADE_DURATION {
                            let fade_elapsed = elapsed - SCROLLBAR_VISIBLE_DURATION;
                            1.0 - (fade_elapsed.as_secs_f32()
                                / SCROLLBAR_FADE_DURATION.as_secs_f32())
                        } else {
                            0.0
                        }
                    })
                    .unwrap_or(0.0);

                if scrollbar_opacity < 0.001 {
                    return None; // Scrollbar fully faded
                }

                let offset = terminal.display_offset();
                let (_, rows) = terminal.size();
                let total_lines = history + rows as usize;

                // Scrollbar position (right edge of pane, with some margin)
                let pane_x = rect.x * win_width as f32;
                let pane_y = rect.y * win_height as f32 + PANE_PADDING;
                let pane_h = rect.height * win_height as f32 - PANE_PADDING * 2.0;
                let pane_w = rect.width * win_width as f32;

                let scrollbar_x = pane_x + pane_w - PANE_PADDING - 2.0; // 2px from right edge
                let track_height = pane_h;

                // Thumb size proportional to visible portion
                let visible_fraction = (rows as f32) / (total_lines as f32);
                let thumb_height = (track_height * visible_fraction).max(20.0); // Minimum 20px

                // Thumb position: offset 0 = at bottom, offset = history = at top
                // When offset = 0, thumb should be at bottom (track_height - thumb_height)
                // When offset = history, thumb should be at top (0)
                let scroll_fraction = if history > 0 {
                    offset as f32 / history as f32
                } else {
                    0.0
                };
                let thumb_start = (1.0 - scroll_fraction) * (track_height - thumb_height);

                Some((
                    scrollbar_x,
                    pane_y,
                    track_height,
                    thumb_start,
                    thumb_height,
                    scrollbar_opacity,
                ))
            })
            .collect();

        // If config UI is visible, render it instead of terminals
        if self.config_ui.visible {
            // Live preview font changes - handle both BDF and TTF
            if let Some(bdf_font) = self.config_ui.config.bdf_font {
                if let Err(e) = renderer.set_bdf_font(bdf_font) {
                    tracing::error!("Failed to preview BDF font: {}", e);
                }
            } else {
                let preview_font = self.config_ui.config.font;
                let preview_font_size = self.config_ui.config.font_size * self.config_ui.config.ui_scale;
                if let Err(e) = renderer.set_font(preview_font, preview_font_size) {
                    tracing::error!("Failed to preview font: {}", e);
                }
            }

            let (cell_w, cell_h) = renderer.cell_size();
            let width_cells = (win_width as f32 / cell_w) as usize;
            let height_cells = (win_height as f32 / cell_h) as usize;

            let ui_cells = self.config_ui.render(width_cells, height_cells);
            let ui_panes = vec![(0.0_f32, 0.0_f32, ui_cells.as_slice())];

            // Use config_ui settings for live preview
            let fg = self.config_ui.config.color_scheme.foreground;
            let effects = EffectParams {
                curvature: self.config_ui.config.effects.screen_curvature,
                scanline_intensity: self.config_ui.config.effects.scanline_intensity,
                scanline_mode: match self.config_ui.config.effects.scanline_mode {
                    ScanlineMode::RowBased => 0,
                    ScanlineMode::Pixel => 1,
                },
                bloom: self.config_ui.config.effects.bloom,
                burn_in: self.config_ui.config.effects.burn_in,
                focus_glow_radius: self.config_ui.config.effects.focus_glow_radius,
                focus_glow_width: self.config_ui.config.effects.focus_glow_width,
                focus_glow_intensity: self.config_ui.config.effects.focus_glow_intensity,
                static_noise: self.config_ui.config.effects.static_noise,
                flicker: self.config_ui.config.effects.flicker,
                brightness: self.config_ui.config.effects.brightness,
                vignette: self.config_ui.config.effects.vignette,
                bezel_enabled: self.config_ui.config.effects.bezel_enabled,
                content_scale_x: self.config_ui.config.effects.content_scale_x,
                content_scale_y: self.config_ui.config.effects.content_scale_y,
                glow_color: [fg[0], fg[1], fg[2], 1.0],
                // Beam sweep / interlacing (disabled in config UI preview for now)
                interlace_enabled: false,
                beam_speed_divisor: 0,
                beam_paused: false,
                beam_step_count: 0,
            };

            // Use per_pane_crt from config UI so user can preview glow while adjusting
            let ui_per_pane_crt = self.config_ui.config.per_pane_crt;

            if let Err(e) = renderer.render_panes(
                &ui_panes,
                &[],
                None,
                &[],
                &[], // No scrollbars in config UI
                &[(0.0, 0.0, 1.0, 1.0)],
                ui_per_pane_crt,
                self.debug_grid,
                &[], // No debug lines in config UI
                0,   // pane 0 is focused (the whole screen) so glow shows
                effects,
            ) {
                tracing::error!("Config UI render error: {}", e);
            }
        } else {
            // Ensure we're using the saved config's font (in case preview changed it)
            // BDF fonts take priority over TTF fonts
            if self.config.bdf_font.is_none() {
                if let Err(e) = renderer.set_font(self.config.font, self.config.font_size) {
                    tracing::error!("Failed to restore font: {}", e);
                }
            }

            let fg = self.config.color_scheme.foreground;
            let effects = EffectParams {
                curvature: self.config.effects.screen_curvature,
                scanline_intensity: self.config.effects.scanline_intensity,
                scanline_mode: match self.config.effects.scanline_mode {
                    ScanlineMode::RowBased => 0,
                    ScanlineMode::Pixel => 1,
                },
                bloom: self.config.effects.bloom,
                burn_in: self.config.effects.burn_in,
                focus_glow_radius: self.config.effects.focus_glow_radius,
                focus_glow_width: self.config.effects.focus_glow_width,
                focus_glow_intensity: self.config.effects.focus_glow_intensity,
                static_noise: self.config.effects.static_noise,
                flicker: self.config.effects.flicker,
                brightness: self.config.effects.brightness,
                vignette: self.config.effects.vignette,
                bezel_enabled: self.config.effects.bezel_enabled,
                content_scale_x: self.config.effects.content_scale_x,
                content_scale_y: self.config.effects.content_scale_y,
                glow_color: [fg[0], fg[1], fg[2], 1.0],
                // Beam sweep / interlacing simulation
                // At 240Hz with divisor 4: 60 fields/sec (NTSC timing)
                // beam_speed_divisor 0 disables beam simulation
                interlace_enabled: self.config.effects.interlace_enabled
                    && self.config.effects.beam_simulation_enabled,
                beam_speed_divisor: if self.config.effects.beam_simulation_enabled {
                    4
                } else {
                    0
                },
                beam_paused: self.beam_paused,
                beam_step_count: {
                    // Step if key is held and enough time has passed
                    let should_step = self.beam_step_held
                        && self.beam_step_last.elapsed()
                            >= Duration::from_millis(self.beam_step_delay_ms as u64);
                    if should_step {
                        self.beam_step_last = Instant::now();
                        1
                    } else {
                        0
                    }
                },
            };

            // Build debug visualization lines - green rectangle around hovered cell
            let debug_lines: Vec<(f32, f32, f32, f32, f32, [f32; 4])> =
                if let Some((cell_pos, _content, _local, pane_offset)) = mouse_debug {
                    let green = [0.0, 1.0, 0.0, 1.0];
                    let (pane_x, pane_y) = (pane_offset.0 as f32, pane_offset.1 as f32);
                    let cell_x = pane_x + cell_pos.col as f32 * cell_w;
                    let cell_y = pane_y + cell_pos.row as f32 * cell_h;
                    vec![
                        (cell_x, cell_y, cell_x + cell_w, cell_y, 2.0, green), // top
                        (
                            cell_x,
                            cell_y + cell_h,
                            cell_x + cell_w,
                            cell_y + cell_h,
                            2.0,
                            green,
                        ), // bottom
                        (cell_x, cell_y, cell_x, cell_y + cell_h, 2.0, green), // left
                        (
                            cell_x + cell_w,
                            cell_y,
                            cell_x + cell_w,
                            cell_y + cell_h,
                            2.0,
                            green,
                        ), // right
                    ]
                } else {
                    Vec::new()
                };

            if let Err(e) = renderer.render_panes(
                &panes,
                &separators,
                focus_rect,
                &size_indicators,
                &scrollbars,
                &pane_rects_normalized,
                per_pane_crt,
                self.debug_grid,
                &debug_lines,
                focused_pane_index,
                effects,
            ) {
                tracing::error!("Render error: {}", e);
            }
        }
    }

    fn add_pane(&mut self) {
        const MAX_PANES: usize = 16;
        if self.layout.panes().len() >= MAX_PANES {
            tracing::warn!("Maximum pane limit ({}) reached", MAX_PANES);
            return;
        }
        let new_pane_id = self.layout.add_pane();
        self.resize_terminals(); // Existing terminals need to shrink
        self.create_terminal_for_pane(new_pane_id);
        tracing::info!(
            "Added pane {:?}, total panes: {}",
            new_pane_id,
            self.layout.panes().len()
        );
    }

    fn close_pane(&mut self, pane_id: PaneId) {
        self.terminals.remove(&pane_id);
        self.layout.close(pane_id);
        self.resize_terminals(); // Remaining terminals expand
        tracing::info!(
            "Closed pane {:?}, remaining panes: {}",
            pane_id,
            self.layout.panes().len()
        );
    }

    fn check_exited_terminals(&mut self) -> Vec<PaneId> {
        let mut exited = Vec::new();
        for (pane_id, terminal) in &self.terminals {
            if terminal.has_exited() {
                exited.push(*pane_id);
            }
        }
        exited
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Load application icon
        let icon = load_icon();

        let mut window_attrs = WindowAttributes::default()
            .with_title("cool-rust-term")
            .with_inner_size(LogicalSize::new(
                self.config.window_width,
                self.config.window_height,
            ))
            .with_window_icon(icon);

        // Restore window position if saved
        if let (Some(x), Some(y)) = (self.config.window_x, self.config.window_y) {
            window_attrs = window_attrs.with_position(winit::dpi::PhysicalPosition::new(x, y));
        }

        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("Failed to create window"),
        );

        // Initialize renderer with font from config
        // Apply ui_scale to font_size for TTF fonts (BDF fonts ignore scaling)
        let mut renderer = pollster::block_on(Renderer::new(
            Arc::clone(&window),
            self.config.font,
            self.config.font_size * self.config.ui_scale,
        ))
        .expect("Failed to create renderer");

        // If BDF font is configured, load and apply it
        if let Some(bdf_font) = self.config.bdf_font {
            if let Err(e) = renderer.set_bdf_font(bdf_font) {
                tracing::error!("Failed to load BDF font {:?}: {}", bdf_font, e);
            } else {
                tracing::info!("Loaded BDF font: {}", bdf_font.label());
            }
        }

        // Log scale factor for debugging
        let scale_factor = window.scale_factor();
        let physical_size = window.inner_size();
        tracing::info!(
            "Window created: {}x{} physical pixels, scale factor: {}",
            physical_size.width,
            physical_size.height,
            scale_factor
        );

        // Query monitor refresh rate and set frame duration to 2x refresh rate (max 240fps)
        let refresh_hz = window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz())
            .map(|mhz| mhz / 1000)
            .unwrap_or(DEFAULT_FPS);
        let target_fps = (refresh_hz * 2).min(240); // 2x refresh rate, capped at 240fps
        self.frame_duration = Duration::from_nanos(1_000_000_000 / target_fps as u64);
        tracing::info!(
            "Monitor refresh rate: {}Hz, targeting {}fps",
            refresh_hz,
            target_fps
        );

        self.window = Some(window);
        self.renderer = Some(renderer);

        // Create terminal for the initial pane
        let initial_pane = self.layout.focused_pane();
        self.create_terminal_for_pane(initial_pane);

        // Restore additional panes from saved config
        let panes_to_restore = self.config.pane_count.saturating_sub(1);
        for _ in 0..panes_to_restore {
            self.add_pane();
        }
        if panes_to_restore > 0 {
            tracing::info!("Restored {} additional panes from config", panes_to_restore);
        }

        let (cols, rows) = self.renderer.as_ref().unwrap().grid_size();
        tracing::info!("Window and renderer initialized ({}x{} cells)", cols, rows);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                // Save window state before exiting
                self.config.pane_count = self.layout.panes().len() as u32;
                if let Err(e) = self.config.save_to_default() {
                    tracing::error!("Failed to save window state: {}", e);
                } else {
                    tracing::info!("Window state saved");
                }
                tracing::info!("Close requested, exiting");
                event_loop.exit();
            }
            WindowEvent::Moved(position) => {
                // Save window position
                self.config.window_x = Some(position.x);
                self.config.window_y = Some(position.y);
            }
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size.width, new_size.height);
                    self.resize_terminals();
                    self.last_resize = Some(Instant::now());
                }
                // Save window size
                self.config.window_width = new_size.width;
                self.config.window_height = new_size.height;
            }
            WindowEvent::RedrawRequested => {
                // Check for exited terminals and close their panes
                let exited = self.check_exited_terminals();
                for pane_id in exited {
                    tracing::info!("Shell in pane {:?} exited", pane_id);
                    self.close_pane(pane_id);
                }

                // Exit if no panes remain
                if self.layout.panes().is_empty() {
                    tracing::info!("All panes closed, exiting");
                    event_loop.exit();
                    return;
                }

                // Frame rate limiting - skip render if too soon
                let now = Instant::now();
                let elapsed = now.duration_since(self.last_frame);
                if elapsed >= self.frame_duration {
                    let dt = elapsed.as_secs_f32();
                    self.last_frame = now;
                    self.render_terminals(dt);
                } else {
                    // Sleep for remaining time to avoid busy-waiting
                    std::thread::sleep(self.frame_duration - elapsed);
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x, position.y);
                if self.selection.active {
                    // Only update selection if pointing at valid content (not the void)
                    if let Some(pos) = self.pixel_to_cell(position.x, position.y) {
                        self.selection.end = pos;
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    match state {
                        ElementState::Pressed => {
                            // Hit test to change focus
                            if let Some(renderer) = &self.renderer {
                                let (win_width, win_height) = renderer.window_size();
                                let (norm_x, norm_y) =
                                    self.pixel_to_normalized(self.mouse_pos.0, self.mouse_pos.1);
                                if let Some(clicked_pane) = self.layout.hit_test(
                                    norm_x,
                                    norm_y,
                                    win_width as f32,
                                    win_height as f32,
                                ) {
                                    if clicked_pane != self.layout.focused_pane() {
                                        self.layout.set_focus(clicked_pane);
                                        tracing::info!("Focus changed to pane {:?}", clicked_pane);
                                    }
                                }
                            }

                            // Only start selection if pointing at valid content (not the void)
                            if let Some(pos) =
                                self.pixel_to_cell(self.mouse_pos.0, self.mouse_pos.1)
                            {
                                let now = Instant::now();

                                // Check if this is a consecutive click (same position, within threshold)
                                let is_consecutive = self
                                    .last_click_time
                                    .map(|t| now.duration_since(t) < DOUBLE_CLICK_THRESHOLD)
                                    .unwrap_or(false)
                                    && self
                                        .last_click_pos
                                        .map(|p| p.col == pos.col && p.row == pos.row)
                                        .unwrap_or(false);

                                if is_consecutive {
                                    self.click_count += 1;
                                } else {
                                    self.click_count = 1;
                                }

                                match self.click_count {
                                    2 => {
                                        // Double-click: select word
                                        if let Some((start, end)) = self.find_word_boundaries(pos) {
                                            self.selection.start = start;
                                            self.selection.end = end;
                                            self.selection.active = false;
                                        }
                                    }
                                    3 => {
                                        // Triple-click: select line
                                        if let Some((start, end)) = self.find_line_boundaries(pos) {
                                            self.selection.start = start;
                                            self.selection.end = end;
                                            self.selection.active = false;
                                        }
                                        // Reset after triple-click
                                        self.click_count = 0;
                                    }
                                    _ => {
                                        // Single click: start normal selection
                                        self.selection.start = pos;
                                        self.selection.end = pos;
                                        self.selection.active = true;
                                    }
                                }

                                self.last_click_time = Some(now);
                                self.last_click_pos = Some(pos);
                            }
                        }
                        ElementState::Released => {
                            self.selection.active = false;
                            if self.config.behavior.auto_copy_selection {
                                self.copy_selection();
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Scroll the focused terminal
                let focused = self.layout.focused_pane();
                if let Some(terminal) = self.terminals.get(&focused) {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y as i32 * 3,
                        MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
                    };
                    if lines != 0 {
                        terminal.scroll(lines);
                        self.last_scroll.insert(focused, Instant::now());

                        // Update selection end if actively selecting while scrolling
                        if self.selection.active {
                            if let Some(pos) =
                                self.pixel_to_cell(self.mouse_pos.0, self.mouse_pos.1)
                            {
                                self.selection.end = pos;
                            }
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let ctrl = self.modifiers.control_key();
                    let shift = self.modifiers.shift_key();
                    let super_key = self.modifiers.super_key();

                    // Shift+Ctrl+Enter: Add new pane
                    if ctrl && shift && event.logical_key == Key::Named(NamedKey::Enter) {
                        self.add_pane();
                        return;
                    }

                    // Ctrl+, or Ctrl+Shift+P: Open config UI
                    if (ctrl && event.logical_key == Key::Character(",".into()))
                        || (ctrl && shift && event.logical_key == Key::Character("P".into()))
                    {
                        if self.config_ui.visible {
                            self.config_ui.hide();
                        } else {
                            self.config_ui.show(&self.config);
                        }
                        return;
                    }

                    // Ctrl+Shift+G: Toggle debug grid
                    if ctrl && shift && event.logical_key == Key::Character("G".into()) {
                        self.debug_grid = !self.debug_grid;
                        tracing::info!("Debug grid: {}", self.debug_grid);
                        return;
                    }

                    // Ctrl+Shift+B: Toggle beam pause (freeze beam position for debugging)
                    if ctrl && shift && event.logical_key == Key::Character("B".into()) {
                        self.beam_paused = !self.beam_paused;
                        tracing::info!("Beam paused: {}", self.beam_paused);
                        return;
                    }

                    // Ctrl+Shift+N: Hold to step frames forward (when beam is paused)
                    if ctrl && shift && event.logical_key == Key::Character("N".into()) {
                        if self.beam_paused {
                            self.beam_step_held = true;
                            // Immediate first step
                            self.beam_step_last = Instant::now()
                                - Duration::from_millis(self.beam_step_delay_ms as u64);
                        }
                        return;
                    }

                    // Ctrl+Shift+=: Decrease step delay (faster stepping)
                    if ctrl
                        && shift
                        && (event.logical_key == Key::Character("=".into())
                            || event.logical_key == Key::Character("+".into()))
                    {
                        self.beam_step_delay_ms =
                            (self.beam_step_delay_ms.saturating_sub(10)).max(4);
                        tracing::info!(
                            "Beam step delay: {}ms ({:.1} fps)",
                            self.beam_step_delay_ms,
                            1000.0 / self.beam_step_delay_ms as f32
                        );
                        return;
                    }

                    // Ctrl+Shift+-: Increase step delay (slower stepping)
                    if ctrl && shift && event.logical_key == Key::Character("-".into()) {
                        self.beam_step_delay_ms = (self.beam_step_delay_ms + 10).min(500);
                        tracing::info!(
                            "Beam step delay: {}ms ({:.1} fps)",
                            self.beam_step_delay_ms,
                            1000.0 / self.beam_step_delay_ms as f32
                        );
                        return;
                    }

                    // Ctrl+Shift+C or Cmd+C: Copy selection
                    if (ctrl && shift && event.logical_key == Key::Character("C".into()))
                        || (super_key && event.logical_key == Key::Character("c".into()))
                    {
                        self.copy_selection();
                        return;
                    }

                    // Ctrl+Shift+V or Cmd+V: Paste from clipboard
                    if (ctrl && shift && event.logical_key == Key::Character("V".into()))
                        || (super_key && event.logical_key == Key::Character("v".into()))
                    {
                        if let Some(clipboard) = &mut self.clipboard {
                            if let Ok(text) = clipboard.get_text() {
                                let focused = self.layout.focused_pane();
                                if let Some(terminal) = self.terminals.get(&focused) {
                                    terminal.input(text.as_bytes());
                                }
                            }
                        }
                        return;
                    }

                    // Ctrl+Shift+T: Replay CRT power-on animation
                    if ctrl && shift && event.logical_key == Key::Character("T".into()) {
                        if let Some(renderer) = &mut self.renderer {
                            renderer.replay_power_on();
                        }
                        return;
                    }

                    // Shift+PageUp/PageDown: Scroll history
                    if shift && !ctrl && event.logical_key == Key::Named(NamedKey::PageUp) {
                        let focused = self.layout.focused_pane();
                        if let Some(terminal) = self.terminals.get(&focused) {
                            terminal.scroll_page_up();
                            self.last_scroll.insert(focused, Instant::now());
                        }
                        return;
                    }
                    if shift && !ctrl && event.logical_key == Key::Named(NamedKey::PageDown) {
                        let focused = self.layout.focused_pane();
                        if let Some(terminal) = self.terminals.get(&focused) {
                            terminal.scroll_page_down();
                            self.last_scroll.insert(focused, Instant::now());
                        }
                        return;
                    }

                    // Handle config UI navigation when visible
                    if self.config_ui.visible {
                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                self.config = self.config_ui.cancel();
                            }
                            Key::Named(NamedKey::ArrowUp) => {
                                self.config_ui.move_up();
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                self.config_ui.move_down();
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                self.config_ui.adjust_left();
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                self.config_ui.adjust_right();
                            }
                            Key::Named(NamedKey::Tab) => {
                                if self.modifiers.shift_key() {
                                    self.config_ui.prev_tab();
                                } else {
                                    self.config_ui.next_tab();
                                }
                            }
                            Key::Character(c) if c == "1" => {
                                self.config_ui.current_tab = crate::config_ui::ConfigTab::Effects;
                                self.config_ui.selected = 0;
                            }
                            Key::Character(c) if c == "2" => {
                                self.config_ui.current_tab =
                                    crate::config_ui::ConfigTab::Appearance;
                                self.config_ui.selected = 0;
                            }
                            Key::Character(c) if c == "3" => {
                                self.config_ui.current_tab = crate::config_ui::ConfigTab::Behavior;
                                self.config_ui.selected = 0;
                            }
                            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                                if let Some(action) = self.config_ui.toggle_or_activate() {
                                    match action {
                                        ConfigAction::Save => {
                                            let new_config = self.config_ui.save();
                                            // Update font if changed
                                            if let Some(renderer) = &mut self.renderer {
                                                let font_changed = new_config.bdf_font
                                                    != self.config.bdf_font
                                                    || new_config.font != self.config.font
                                                    || (new_config.font_size
                                                        - self.config.font_size)
                                                        .abs()
                                                        > 0.1;

                                                if font_changed {
                                                    // Apply the appropriate font type
                                                    if let Some(bdf_font) = new_config.bdf_font {
                                                        if let Err(e) =
                                                            renderer.set_bdf_font(bdf_font)
                                                        {
                                                            tracing::error!(
                                                                "Failed to change to BDF font: {}",
                                                                e
                                                            );
                                                        } else {
                                                            tracing::info!(
                                                                "Font changed to BDF: {}",
                                                                bdf_font.label()
                                                            );
                                                            self.config = new_config.clone();
                                                            self.resize_terminals();
                                                        }
                                                    } else if let Err(e) = renderer.set_font(
                                                        new_config.font,
                                                        new_config.font_size * new_config.ui_scale,
                                                    ) {
                                                        tracing::error!(
                                                            "Failed to change font: {}",
                                                            e
                                                        );
                                                    } else {
                                                        tracing::info!(
                                                            "Font changed to {} at {}px",
                                                            new_config.font.label(),
                                                            new_config.font_size
                                                        );
                                                        self.config = new_config.clone();
                                                        self.resize_terminals();
                                                    }
                                                }
                                            }
                                            self.config = new_config;
                                            if let Err(e) = self.config.save_to_default() {
                                                tracing::error!("Failed to save config: {}", e);
                                            } else {
                                                tracing::info!("Config saved");
                                            }
                                        }
                                        ConfigAction::Cancel => {
                                            self.config = self.config_ui.cancel();
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                        return;
                    }

                    // Send input to focused terminal
                    let focused = self.layout.focused_pane();
                    if let Some(terminal) = self.terminals.get(&focused) {
                        let mode = terminal.term_mode();
                        let use_kitty = mode.contains(TermMode::DISAMBIGUATE_ESC_CODES);

                        // Convert key to bytes and send to terminal
                        let bytes: Option<Vec<u8>> = if use_kitty {
                            // Use Kitty keyboard protocol
                            kitty_keyboard::encode(&event.logical_key, self.modifiers, mode)
                        } else {
                            // Legacy escape sequence encoding
                            let alt = self.modifiers.alt_key();
                            let app_cursor = mode.contains(TermMode::APP_CURSOR);
                            match &event.logical_key {
                                Key::Character(s) => {
                                    if ctrl && s.len() == 1 {
                                        // Ctrl+letter sends control code
                                        let c = s.chars().next().unwrap();
                                        if c.is_ascii_lowercase() {
                                            Some(vec![c as u8 - b'a' + 1])
                                        } else if c.is_ascii_uppercase() {
                                            Some(vec![c as u8 - b'A' + 1])
                                        } else {
                                            Some(s.as_bytes().to_vec())
                                        }
                                    } else if alt && s.len() == 1 {
                                        // Alt+key sends ESC + key
                                        let mut bytes = vec![0x1b];
                                        bytes.extend(s.as_bytes());
                                        Some(bytes)
                                    } else {
                                        Some(s.as_bytes().to_vec())
                                    }
                                }
                                Key::Named(named) => match named {
                                    NamedKey::Enter => {
                                        if alt {
                                            Some(vec![0x1b, b'\r'])
                                        } else {
                                            Some(vec![b'\r'])
                                        }
                                    }
                                    NamedKey::Backspace => Some(vec![0x7f]),
                                    NamedKey::Tab => Some(vec![b'\t']),
                                    NamedKey::Escape => Some(vec![0x1b]),
                                    // Cursor keys: use SS3 format when APP_CURSOR (DECCKM) is set
                                    NamedKey::ArrowUp => {
                                        if app_cursor {
                                            Some(b"\x1bOA".to_vec())
                                        } else {
                                            Some(b"\x1b[A".to_vec())
                                        }
                                    }
                                    NamedKey::ArrowDown => {
                                        if app_cursor {
                                            Some(b"\x1bOB".to_vec())
                                        } else {
                                            Some(b"\x1b[B".to_vec())
                                        }
                                    }
                                    NamedKey::ArrowRight => {
                                        if app_cursor {
                                            Some(b"\x1bOC".to_vec())
                                        } else {
                                            Some(b"\x1b[C".to_vec())
                                        }
                                    }
                                    NamedKey::ArrowLeft => {
                                        if app_cursor {
                                            Some(b"\x1bOD".to_vec())
                                        } else {
                                            Some(b"\x1b[D".to_vec())
                                        }
                                    }
                                    NamedKey::Home => {
                                        if app_cursor {
                                            Some(b"\x1bOH".to_vec())
                                        } else {
                                            Some(b"\x1b[H".to_vec())
                                        }
                                    }
                                    NamedKey::End => {
                                        if app_cursor {
                                            Some(b"\x1bOF".to_vec())
                                        } else {
                                            Some(b"\x1b[F".to_vec())
                                        }
                                    }
                                    NamedKey::PageUp => Some(b"\x1b[5~".to_vec()),
                                    NamedKey::PageDown => Some(b"\x1b[6~".to_vec()),
                                    NamedKey::Delete => Some(b"\x1b[3~".to_vec()),
                                    NamedKey::Space => {
                                        if alt {
                                            Some(vec![0x1b, b' '])
                                        } else {
                                            Some(vec![b' '])
                                        }
                                    }
                                    _ => None,
                                },
                                _ => None,
                            }
                        };

                        if let Some(ref bytes) = bytes {
                            // Auto-scroll to bottom when typing
                            terminal.scroll_to_bottom();
                            terminal.input(bytes);
                        }
                    }
                } else if event.state == ElementState::Released {
                    // Handle key releases
                    if event.logical_key == Key::Character("N".into())
                        || event.logical_key == Key::Character("n".into())
                    {
                        self.beam_step_held = false;
                    }
                }
            }
            _ => {}
        }
    }
}

fn load_icon() -> Option<Icon> {
    let icon_bytes = include_bytes!("../../../assets/icon.png");
    let image = image::load_from_memory(icon_bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).ok()
}

fn main() -> Result<()> {
    // Force 1:1 pixel scaling on X11 (winit guesses wrong sometimes)
    // TODO: Make this configurable for high-DPI displays
    std::env::set_var("WINIT_X11_SCALE_FACTOR", "1");

    tracing_subscriber::fmt::init();

    tracing::info!("Starting cool-rust-term");

    let event_loop = EventLoop::new()?;
    let mut app = App::new();

    event_loop.run_app(&mut app)?;

    Ok(())
}
