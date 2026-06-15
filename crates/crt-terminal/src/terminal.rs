// ABOUTME: Terminal instance wrapping alacritty_terminal.
// ABOUTME: Manages PTY, processes input, and exposes cell grid for rendering.

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty;
use alacritty_terminal::Grid;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Default scrollback history size (number of lines)
const SCROLLBACK_LINES: usize = 10_000;

/// Terminal instance with PTY and terminal state
pub struct Terminal {
    term: Arc<FairMutex<Term<EventProxy>>>,
    sender: EventLoopSender,
    exited: Arc<AtomicBool>,
    /// Set whenever the terminal produced something that changes the rendered
    /// output (new PTY output, mode change, bell, child exit). The app clears it
    /// via `take_dirty()` to decide whether a frame needs a content rebuild.
    dirty: Arc<AtomicBool>,
    /// PID of the shell process (Unix only, 0 on Windows)
    child_pid: u32,
}

/// Proxy for terminal events
#[derive(Clone)]
struct EventProxy {
    exited: Arc<AtomicBool>,
    dirty: Arc<AtomicBool>,
    sender: std::sync::mpsc::Sender<String>,
}

impl alacritty_terminal::event::EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Exit => {
                self.exited.store(true, Ordering::SeqCst);
                self.dirty.store(true, Ordering::SeqCst);
            }
            Event::PtyWrite(text) => {
                // Send response back to PTY (e.g., cursor position query response)
                let _ = self.sender.send(text);
            }
            // Anything that mutates rendered cells / triggers a redraw. `Wakeup`
            // is emitted once per processed PTY byte batch, so it covers grid
            // output, cursor movement, and terminal mode changes.
            Event::Wakeup | Event::ChildExit(_) | Event::Bell | Event::MouseCursorDirty => {
                self.dirty.store(true, Ordering::SeqCst);
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
        Self::with_working_directory(columns, rows, None)
    }

    /// Create a new terminal with the given dimensions and working directory
    pub fn with_working_directory(
        columns: u16,
        rows: u16,
        working_directory: Option<PathBuf>,
    ) -> Result<Self, TerminalError> {
        // Set TERM and COLORTERM in the process environment before spawning the shell.
        // This is required for GUI apps launched from Finder which have no parent terminal.
        tty::setup_env();

        let cwd = working_directory.or_else(dirs::home_dir);

        #[cfg(not(windows))]
        let pty_config = tty::Options {
            shell: None,
            working_directory: cwd,
            drain_on_exit: true,
            env: std::collections::HashMap::new(),
        };

        #[cfg(windows)]
        let pty_config = tty::Options {
            shell: None,
            working_directory: cwd,
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

        // Capture PID before pty is moved into EventLoop
        #[cfg(not(windows))]
        let child_pid = pty.child().id();
        #[cfg(windows)]
        let child_pid = 0;

        let exited = Arc::new(AtomicBool::new(false));
        // Start dirty so the very first frame performs a full content render.
        let dirty = Arc::new(AtomicBool::new(true));

        // Channel for PtyWrite events (cursor position queries, etc.)
        let (pty_write_tx, pty_write_rx) = std::sync::mpsc::channel::<String>();

        let event_proxy = EventProxy {
            exited: Arc::clone(&exited),
            dirty: Arc::clone(&dirty),
            sender: pty_write_tx,
        };

        let term_size = TermSize::new(columns as usize, rows as usize);
        let term_config = alacritty_terminal::term::Config {
            scrolling_history: SCROLLBACK_LINES,
            kitty_keyboard: true,
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
            dirty,
            child_pid,
        })
    }

    /// Check if the shell has exited
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Return whether the terminal changed since the last call, clearing the
    /// flag. Used by the render loop to decide if a content rebuild is needed.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::SeqCst)
    }

    /// Get the PID of the shell process (Unix only, returns 0 on Windows)
    pub fn child_pid(&self) -> u32 {
        self.child_pid
    }

    /// Force-kill the shell process and its entire process group with SIGKILL.
    ///
    /// SIGKILL cannot be caught or ignored, so this nukes a hung shell that
    /// won't die from the SIGHUP sent when the PTY is closed. The shell is a
    /// session/process-group leader (alacritty's PTY calls setsid), so we
    /// signal the negated PID to take down any children it spawned too.
    #[cfg(unix)]
    pub fn kill(&self) {
        if self.child_pid == 0 {
            return;
        }
        let pid = self.child_pid as libc::pid_t;
        // SAFETY: kill(2) on a possibly-dead PID is harmless (returns ESRCH).
        unsafe {
            // Kill the whole process group; fall back to the lone PID if the
            // process group is already gone.
            if libc::kill(-pid, libc::SIGKILL) != 0 {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        tracing::info!("Sent SIGKILL to shell process group {}", self.child_pid);
    }

    /// Force-kill the shell process (no-op on Windows).
    #[cfg(windows)]
    pub fn kill(&self) {}

    /// Get the current working directory of the shell process
    pub fn working_directory(&self) -> Option<std::path::PathBuf> {
        crate::process_info::get_process_cwd(self.child_pid)
    }

    /// Capture scrollback data for session restoration
    pub fn capture_scrollback(&self) -> crate::scrollback::ScrollbackData {
        let term = self.term.lock();
        crate::scrollback::ScrollbackData::from_grid(term.grid())
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

    /// Access the terminal grid for rendering.
    pub fn with_grid<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Grid<alacritty_terminal::term::cell::Cell>) -> R,
    {
        let term = self.term.lock();
        f(term.grid())
    }

    /// Monotonically increasing count of lines that have scrolled off the top of the screen.
    /// Reads directly from alacritty's Grid, which tracks this in scroll_up().
    pub fn total_lines_scrolled(&self) -> u64 {
        let term = self.term.lock();
        term.grid().total_lines_scrolled()
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

    /// Scroll to a specific display offset (0 = bottom, history_size = top)
    pub fn scroll_to_offset(&self, offset: usize) {
        let mut term = self.term.lock();
        let current = term.grid().display_offset();
        let delta = offset as i32 - current as i32;
        term.scroll_display(Scroll::Delta(delta));
    }

    /// Collect all regex matches in the terminal scrollback and screen.
    /// Returns matches as Vec of (start_point, end_point) pairs.
    pub fn search_all(
        &self,
        regex: &mut alacritty_terminal::term::search::RegexSearch,
    ) -> Vec<std::ops::RangeInclusive<alacritty_terminal::index::Point>> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Direction, Point};
        use alacritty_terminal::term::search::RegexIter;

        let term = self.term.lock();
        let grid = term.grid();
        let start = Point::new(grid.topmost_line(), Column(0));
        let end = Point::new(grid.bottommost_line(), grid.last_column());

        RegexIter::new(start, end, Direction::Right, &term, regex).collect()
    }

    /// Check if Kitty keyboard protocol is enabled
    pub fn kitty_keyboard_enabled(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        let term = self.term.lock();
        term.mode().contains(TermMode::DISAMBIGUATE_ESC_CODES)
    }

    /// Get the full terminal mode flags for keyboard handling
    pub fn term_mode(&self) -> alacritty_terminal::term::TermMode {
        let term = self.term.lock();
        *term.mode()
    }
}

impl Drop for Terminal {
    /// Tear down the PTY reader thread and the shell when a pane is closed.
    ///
    /// Without this, dropping a `Terminal` leaks: alacritty's `EventLoop` keeps
    /// its own copy of the channel sender, so the background "PTY reader" thread
    /// never sees the channel disconnect and keeps running, holding the PTY (and
    /// therefore the shell process) alive forever.
    fn drop(&mut self) {
        // Only signal a live shell. If it already exited, alacritty's `Pty::drop`
        // has reaped the child via `waitpid`, after which the PID can be recycled
        // by an unrelated process — signalling it (especially the negated PID, a
        // whole process group) would then hit the wrong target.
        if !self.has_exited() {
            // Force-kill the shell's process group so no child outlives its pane.
            self.kill();
        }

        // Tell the reader thread to stop. Draining `Msg::Shutdown` breaks its
        // loop, which drops the `EventLoop` (and its `Pty`); `Pty::drop` then
        // reaps the child and, by dropping the last `Term`, releases the sender
        // that keeps the `PtyWrite` forwarder thread parked on `recv`.
        let _ = self.sender.send(Msg::Shutdown);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Whether a process (or zombie) with `pid` still exists.
    ///
    /// `kill(pid, 0)` does the kernel's permission/existence check without
    /// delivering a signal: it returns 0 while the PID exists and fails with
    /// `ESRCH` once it's gone and reaped.
    fn process_alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// Regression test: dropping a `Terminal` must not leak the shell process.
    ///
    /// Before `Terminal` had a `Drop`, closing a pane only removed it from the
    /// app's map; the alacritty reader thread kept the PTY (and the shell) alive
    /// indefinitely.
    #[test]
    fn drop_kills_shell_process() {
        let term = Terminal::new(80, 24).expect("spawn terminal");
        let pid = term.child_pid();
        assert_ne!(pid, 0, "expected a real child PID on unix");
        assert!(
            process_alive(pid),
            "shell should be running right after spawn"
        );

        drop(term);

        // Drop force-kills the process group and signals the reader thread to
        // stop; that thread's teardown reaps the child. Poll until it's gone.
        let mut gone = false;
        for _ in 0..200 {
            if !process_alive(pid) {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(gone, "shell process {pid} survived Terminal drop (leak)");
    }
}
