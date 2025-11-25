// ABOUTME: Terminal instance wrapping alacritty_terminal.
// ABOUTME: Manages PTY, processes input, and exposes cell grid for rendering.

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty;
use alacritty_terminal::Grid;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
}

impl alacritty_terminal::event::EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if let Event::Exit = event {
            self.exited.store(true, Ordering::SeqCst);
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
        let pty_config = tty::Options {
            shell: None,
            working_directory: None,
            drain_on_exit: true,
            env: std::collections::HashMap::new(),
        };

        let window_size = WindowSize {
            num_cols: columns,
            num_lines: rows,
            cell_width: 1,
            cell_height: 1,
        };

        let pty = tty::new(&pty_config, window_size, 0)?;

        let exited = Arc::new(AtomicBool::new(false));
        let event_proxy = EventProxy {
            exited: Arc::clone(&exited),
        };

        let term_size = TermSize::new(columns as usize, rows as usize);
        let term_config = alacritty_terminal::term::Config::default();
        let term = Term::new(term_config, &term_size, event_proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let event_loop = EventLoop::new(Arc::clone(&term), event_proxy, pty, false, false)?;

        let sender = event_loop.channel();

        // Spawn the event loop on a background thread
        std::thread::spawn(move || {
            event_loop.spawn();
        });

        Ok(Self { term, sender, exited })
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
    pub fn cursor_position(&self) -> (usize, usize) {
        let term = self.term.lock();
        let cursor = term.grid().cursor.point;
        (cursor.column.0, cursor.line.0 as usize)
    }
}
