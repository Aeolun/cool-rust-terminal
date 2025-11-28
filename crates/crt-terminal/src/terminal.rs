// ABOUTME: Terminal instance wrapping alacritty_terminal.
// ABOUTME: Manages PTY, processes input, and exposes cell grid for rendering.

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty;
use alacritty_terminal::Grid;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Default scrollback history size (number of lines)
const SCROLLBACK_LINES: usize = 10_000;

/// Terminal instance with PTY and terminal state
pub struct Terminal {
    term: Arc<FairMutex<Term<EventProxy>>>,
    sender: EventLoopSender,
    exited: Arc<AtomicBool>,
}

/// Proxy for terminal events
#[derive(Clone)]
struct EventProxy {
    exited: Arc<AtomicBool>,
    sender: std::sync::mpsc::Sender<String>,
}

impl alacritty_terminal::event::EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Exit => {
                self.exited.store(true, Ordering::SeqCst);
            }
            Event::PtyWrite(text) => {
                // Send response back to PTY (e.g., cursor position query response)
                let _ = self.sender.send(text);
            }
            _ => {}
        }
    }
}

/// Simple size type that implements Dimensions
struct TermSize {
    columns: usize,
    lines: usize,
}

impl TermSize {
    fn new(columns: usize, lines: usize) -> Self {
        Self { columns, lines }
    }
}

impl Dimensions for TermSize {
    fn columns(&self) -> usize {
        self.columns
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn total_lines(&self) -> usize {
        self.lines
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("Failed to create PTY: {0}")]
    PtyError(#[from] std::io::Error),
}

impl Terminal {
    /// Create a new terminal with the given dimensions
    pub fn new(columns: u16, rows: u16) -> Result<Self, TerminalError> {
        // Set TERM and COLORTERM in the process environment before spawning the shell.
        // This is required for GUI apps launched from Finder which have no parent terminal.
        tty::setup_env();

        #[cfg(not(windows))]
        let pty_config = tty::Options {
            shell: None,
            working_directory: dirs::home_dir(),
            drain_on_exit: true,
            env: std::collections::HashMap::new(),
        };

        #[cfg(windows)]
        let pty_config = tty::Options {
            shell: None,
            working_directory: dirs::home_dir(),
            drain_on_exit: true,
            env: std::collections::HashMap::new(),
            escape_args: true,
        };

        let window_size = WindowSize {
            num_cols: columns,
            num_lines: rows,
            cell_width: 1,
            cell_height: 1,
        };

        let pty = tty::new(&pty_config, window_size, 0)?;

        let exited = Arc::new(AtomicBool::new(false));

        // Channel for PtyWrite events (cursor position queries, etc.)
        let (pty_write_tx, pty_write_rx) = std::sync::mpsc::channel::<String>();

        let event_proxy = EventProxy {
            exited: Arc::clone(&exited),
            sender: pty_write_tx,
        };

        let term_size = TermSize::new(columns as usize, rows as usize);
        let term_config = alacritty_terminal::term::Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Default::default()
        };
        let term = Term::new(term_config, &term_size, event_proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let event_loop = EventLoop::new(Arc::clone(&term), event_proxy, pty, false, false)?;

        let sender = event_loop.channel();

        // Spawn thread to forward PtyWrite events back to the PTY
        let pty_sender = sender.clone();
        std::thread::spawn(move || {
            while let Ok(text) = pty_write_rx.recv() {
                let _ = pty_sender.send(Msg::Input(text.into_bytes().into()));
            }
        });

        // Spawn the event loop on a background thread
        std::thread::spawn(move || {
            event_loop.spawn();
        });

        Ok(Self {
            term,
            sender,
            exited,
        })
    }

    /// Check if the shell has exited
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Send input bytes to the terminal
    pub fn input(&self, bytes: &[u8]) {
        let _ = self.sender.send(Msg::Input(bytes.to_vec().into()));
    }

    /// Resize the terminal
    pub fn resize(&self, columns: u16, rows: u16) {
        let window_size = WindowSize {
            num_cols: columns,
            num_lines: rows,
            cell_width: 1,
            cell_height: 1,
        };

        let term_size = TermSize::new(columns as usize, rows as usize);

        let _ = self.sender.send(Msg::Resize(window_size));
        self.term.lock().resize(term_size);
    }

    /// Access the terminal grid for rendering
    pub fn with_grid<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Grid<alacritty_terminal::term::cell::Cell>) -> R,
    {
        let term = self.term.lock();
        f(term.grid())
    }

    /// Access terminal content including cursor for rendering
    pub fn with_content<F, R>(&self, f: F) -> R
    where
        F: FnOnce(alacritty_terminal::term::RenderableContent<'_>) -> R,
    {
        let term = self.term.lock();
        let content = term.renderable_content();
        f(content)
    }

    /// Get terminal dimensions
    pub fn size(&self) -> (u16, u16) {
        let term = self.term.lock();
        let grid = term.grid();
        (grid.columns() as u16, grid.screen_lines() as u16)
    }

    /// Get cursor position (column, line)
    /// Returns None if cursor is hidden or out of bounds
    pub fn cursor_position(&self) -> Option<(usize, usize)> {
        let term = self.term.lock();
        let cursor = term.grid().cursor.point;
        let screen_lines = term.grid().screen_lines();

        // cursor.line.0 is i32, can be negative or out of bounds
        let line = cursor.line.0;
        if line < 0 || line as usize >= screen_lines {
            return None;
        }

        Some((cursor.column.0, line as usize))
    }

    /// Scroll the display by a number of lines (negative = up, positive = down)
    pub fn scroll(&self, delta: i32) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Delta(delta));
    }

    /// Scroll up by one page
    pub fn scroll_page_up(&self) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::PageUp);
    }

    /// Scroll down by one page
    pub fn scroll_page_down(&self) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::PageDown);
    }

    /// Scroll to the bottom (most recent output)
    pub fn scroll_to_bottom(&self) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Bottom);
    }

    /// Get current scroll position (0 = at bottom, positive = scrolled up)
    pub fn display_offset(&self) -> usize {
        let term = self.term.lock();
        term.grid().display_offset()
    }

    /// Get total scrollback history size (number of lines above visible area)
    pub fn history_size(&self) -> usize {
        let term = self.term.lock();
        term.grid().history_size()
    }
}
