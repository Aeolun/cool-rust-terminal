// ABOUTME: Terminal emulation and PTY handling.
// ABOUTME: Wraps alacritty_terminal to provide terminal state and I/O.

pub mod process_info;
pub mod scrollback;
pub mod terminal;

pub use alacritty_terminal::term::TermMode;
pub use process_info::get_process_cwd;
pub use scrollback::ScrollbackData;
pub use terminal::Terminal;
