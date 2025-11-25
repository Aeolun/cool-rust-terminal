// ABOUTME: Main application entry point.
// ABOUTME: Sets up window, event loop, and coordinates terminal/rendering.

use std::sync::Arc;

use anyhow::Result;
use arboard::Clipboard;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crt_renderer::{RenderCell, Renderer};
use crt_terminal::Terminal;

const AMBER: [f32; 4] = [1.0, 0.7, 0.0, 1.0];
const SELECTION_FG: [f32; 4] = [1.0, 0.3, 0.1, 1.0]; // Red-orange for selected text

#[derive(Clone, Copy, Debug, Default)]
struct CellPos {
    col: usize,
    row: usize,
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
            CellPos { col: start_col, row: start_row },
            CellPos { col: end_col, row: end_row },
        )
    }

    fn contains(&self, col: usize, row: usize) -> bool {
        if !self.active && self.start.row == self.end.row && self.start.col == self.end.col {
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

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    terminal: Option<Terminal>,
    modifiers: ModifiersState,
    selection: Selection,
    mouse_pos: (f64, f64),
    clipboard: Option<Clipboard>,
    last_grid: Vec<Vec<char>>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            terminal: None,
            modifiers: ModifiersState::empty(),
            selection: Selection::default(),
            mouse_pos: (0.0, 0.0),
            clipboard: Clipboard::new().ok(),
            last_grid: Vec::new(),
        }
    }

    fn pixel_to_cell(&self, x: f64, y: f64) -> CellPos {
        let Some(renderer) = &self.renderer else {
            return CellPos::default();
        };
        let (cell_w, cell_h) = renderer.cell_size();
        let col = (x / cell_w as f64).floor() as usize;
        let row = (y / cell_h as f64).floor() as usize;
        CellPos { col, row }
    }

    fn copy_selection(&mut self) {
        if self.last_grid.is_empty() {
            return;
        }
        let (start, end) = self.selection.normalized();
        let mut text = String::new();

        for row in start.row..=end.row {
            if row >= self.last_grid.len() {
                break;
            }
            let row_data = &self.last_grid[row];
            let col_start = if row == start.row { start.col } else { 0 };
            let col_end = if row == end.row {
                end.col.min(row_data.len().saturating_sub(1))
            } else {
                row_data.len().saturating_sub(1)
            };

            for col in col_start..=col_end {
                if col < row_data.len() {
                    let c = row_data[col];
                    if c != '\0' {
                        text.push(c);
                    }
                }
            }
            if row != end.row {
                text.push('\n');
            }
        }

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

    fn render_terminal(&mut self) {
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        let Some(terminal) = &self.terminal else {
            if let Err(e) = renderer.render() {
                tracing::error!("Render error: {}", e);
            }
            return;
        };

        // Get cursor position first
        let (cursor_col, cursor_line) = terminal.cursor_position();

        // Read terminal grid and convert to render cells
        let (_cols, _rows_count) = renderer.grid_size();
        let selection = &self.selection;
        let (cells, grid_chars) = terminal.with_grid(|grid| {
            use alacritty_terminal::grid::Dimensions;
            use alacritty_terminal::index::{Column, Line};
            use alacritty_terminal::term::cell::Flags;

            let grid_cols = grid.columns();
            let grid_lines = grid.screen_lines();

            let mut rows: Vec<Vec<RenderCell>> = Vec::with_capacity(grid_lines);
            let mut grid_chars: Vec<Vec<char>> = Vec::with_capacity(grid_lines);

            for line_idx in 0..grid_lines {
                let mut row = Vec::with_capacity(grid_cols);
                let mut row_chars = Vec::with_capacity(grid_cols);
                let line = Line(line_idx as i32);
                let mut in_non_ascii_run = false;

                for col_idx in 0..grid_cols {
                    let cell = &grid[line][Column(col_idx)];
                    let mut c = cell.c;
                    let flags = cell.flags;

                    // Collapse runs of non-ASCII (and wide char spacers) into single '?'
                    let is_wide_spacer = flags.contains(Flags::WIDE_CHAR_SPACER);
                    let is_non_ascii = !c.is_ascii() || (c.is_control() && c != ' ' && c != '\0');

                    if is_wide_spacer {
                        // Second half of wide char - skip entirely
                        continue;
                    } else if is_non_ascii {
                        if in_non_ascii_run {
                            // Skip consecutive non-ASCII chars
                            continue;
                        } else {
                            c = '?';
                            in_non_ascii_run = true;
                        }
                    } else {
                        in_non_ascii_run = false;
                    }

                    let is_cursor = line_idx == cursor_line && col_idx == cursor_col;
                    let is_selected = selection.contains(col_idx, line_idx);

                    let fg = if is_cursor {
                        [0.0, 0.0, 0.0, 1.0]
                    } else if is_selected {
                        SELECTION_FG
                    } else if cell.flags.contains(Flags::DIM) {
                        [0.6, 0.42, 0.0, 1.0]
                    } else {
                        AMBER
                    };

                    let bg = if is_cursor {
                        AMBER
                    } else {
                        [0.0, 0.0, 0.0, 0.0]
                    };

                    if is_cursor && (c == ' ' || c == '\0') {
                        c = '█';
                    }

                    // For cursor block on empty cell, use amber foreground
                    let final_fg = if is_cursor && c == '█' {
                        AMBER
                    } else {
                        fg
                    };

                    row_chars.push(c);
                    row.push(RenderCell { c, fg: final_fg, bg });
                }

                rows.push(row);
                grid_chars.push(row_chars);
            }

            (rows, grid_chars)
        });

        // Store grid for copy
        self.last_grid = grid_chars;

        if let Err(e) = renderer.render_grid(&cells) {
            tracing::error!("Render error: {}", e);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_attrs = WindowAttributes::default()
            .with_title("cool-rust-term")
            .with_inner_size(LogicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("Failed to create window"),
        );

        // Initialize renderer
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))
            .expect("Failed to create renderer");

        // Calculate terminal size based on cell dimensions
        let (cols, rows) = renderer.grid_size();

        // Create terminal
        let terminal = Terminal::new(cols, rows).expect("Failed to create terminal");

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.terminal = Some(terminal);

        tracing::info!(
            "Window and renderer initialized ({}x{} cells)",
            cols,
            rows
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("Close requested, exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size.width, new_size.height);

                    // Resize terminal to match
                    let (cols, rows) = renderer.grid_size();
                    if let Some(terminal) = &self.terminal {
                        terminal.resize(cols, rows);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // Check if terminal has exited
                if let Some(terminal) = &self.terminal {
                    if terminal.has_exited() {
                        tracing::info!("Shell exited, closing window");
                        event_loop.exit();
                        return;
                    }
                }

                self.render_terminal();
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
                    self.selection.end = self.pixel_to_cell(position.x, position.y);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    match state {
                        ElementState::Pressed => {
                            let pos = self.pixel_to_cell(self.mouse_pos.0, self.mouse_pos.1);
                            self.selection.start = pos;
                            self.selection.end = pos;
                            self.selection.active = true;
                        }
                        ElementState::Released => {
                            self.selection.active = false;
                            // Auto-copy on selection release (like iTerm2)
                            self.copy_selection();
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    if let Some(terminal) = &self.terminal {
                        let ctrl = self.modifiers.control_key();
                        let alt = self.modifiers.alt_key();

                        // Convert key to bytes and send to terminal
                        let bytes: Option<Vec<u8>> = match &event.logical_key {
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
                                NamedKey::Enter => Some(vec![b'\r']),
                                NamedKey::Backspace => Some(vec![0x7f]),
                                NamedKey::Tab => Some(vec![b'\t']),
                                NamedKey::Escape => Some(vec![0x1b]),
                                NamedKey::ArrowUp => Some(b"\x1b[A".to_vec()),
                                NamedKey::ArrowDown => Some(b"\x1b[B".to_vec()),
                                NamedKey::ArrowRight => Some(b"\x1b[C".to_vec()),
                                NamedKey::ArrowLeft => Some(b"\x1b[D".to_vec()),
                                NamedKey::Home => Some(b"\x1b[H".to_vec()),
                                NamedKey::End => Some(b"\x1b[F".to_vec()),
                                NamedKey::PageUp => Some(b"\x1b[5~".to_vec()),
                                NamedKey::PageDown => Some(b"\x1b[6~".to_vec()),
                                NamedKey::Delete => Some(b"\x1b[3~".to_vec()),
                                NamedKey::Space => Some(vec![b' ']),
                                _ => None,
                            },
                            _ => None,
                        };

                        if let Some(bytes) = bytes {
                            terminal.input(&bytes);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    tracing::info!("Starting cool-rust-term");

    let event_loop = EventLoop::new()?;
    let mut app = App::new();

    event_loop.run_app(&mut app)?;

    Ok(())
}
