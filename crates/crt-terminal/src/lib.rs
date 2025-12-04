// ABOUTME: Terminal emulation and PTY handling.
// ABOUTME: Wraps alacritty_terminal to provide terminal state and I/O.

pub mod terminal;

pub use alacritty_terminal::term::TermMode;
pub use terminal::Terminal;
