// ABOUTME: Main application entry point.
// ABOUTME: Sets up window, event loop, and coordinates terminal/rendering.

mod config_ui;
mod search_ui;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use arboard::Clipboard;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Icon, Window, WindowAttributes, WindowId};

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb as AnsiRgb};
use config_ui::{ConfigAction, ConfigUI};
use crt_core::{BdfFont, ColorScheme, Config, Font, ScanlineMode, SessionData, UpdateInfo};
use crt_layout::{Direction, LayoutTree, PaneId};
use crt_renderer::{EffectParams, RenderCell, Renderer};
use crt_terminal::{TermMode, Terminal};
use search_ui::{SearchAction, SearchUI};

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

            // Special case: Shift+Tab sends \x1b[Z (backtab) in legacy/crossterm-compat mode
            // This is the standard xterm sequence that most applications expect
            if *named == NamedKey::Tab && mod_flags == 1 && !report_associated_text {
                return Some(b"\x1b[Z".to_vec());
            }

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
const SCROLLBAR_HOVER_PROXIMITY: f64 = 20.0; // Show scrollbar when mouse within this many px of edge
const SCROLLBAR_WIDTH: f32 = 4.0;
const DEFAULT_FPS: u32 = 60; // Fallback if we can't detect refresh rate
const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(500);
const HIGH_DPI_SCALE_THRESHOLD: f64 = 1.5;

// Startup hint timing (after power-on animation)
const POWERON_DURATION: f32 = 1.05; // Must match shader's POWERON_TOTAL
const STARTUP_HINT_DELAY: f32 = POWERON_DURATION;
const STARTUP_HINT_DURATION: f32 = 2.0;
const STARTUP_HINT_FADE: f32 = 0.5;
const PASTE_DIALOG_MIN_WIDTH: usize = 28;
const BRACKETED_MSG_DURATION: f32 = 1.5;

#[derive(Debug, Clone, Copy)]
struct ActiveFontSettings {
    font: Font,
    font_size: f32,
    ui_scale: f32,
    bdf_font: Option<BdfFont>,
}

impl ActiveFontSettings {
    fn approx_eq(self, other: Self) -> bool {
        self.font == other.font
            && self.bdf_font == other.bdf_font
            && (self.font_size - other.font_size).abs() < 0.1
            && (self.ui_scale - other.ui_scale).abs() < 0.01
    }
}

/// Computed scrollbar geometry for a single pane, used for both rendering and hit testing.
#[derive(Clone, Copy)]
struct ScrollbarGeometry {
    pane_id: PaneId,
    /// Left edge of the scrollbar in pixels
    x: f32,
    /// Top of the scrollbar track in pixels
    y: f32,
    /// Total track height in pixels
    track_height: f32,
    /// Thumb offset from track top in pixels
    thumb_start: f32,
    /// Thumb height in pixels
    thumb_height: f32,
    /// Current opacity (0.0-1.0)
    opacity: f32,
    /// History size at time of computation (for scroll ratio)
    history_size: usize,
}

impl ScrollbarGeometry {
    /// Convert to the tuple format expected by the renderer
    fn to_render_tuple(self) -> (f32, f32, f32, f32, f32, f32) {
        (
            self.x,
            self.y,
            self.track_height,
            self.thumb_start,
            self.thumb_height,
            self.opacity,
        )
    }

    /// Check if a pixel position (in content-space, post barrel distortion) hits this scrollbar.
    /// Uses a wider hit area than the visual width for easier clicking.
    fn hit_test(&self, px: f64, py: f64) -> bool {
        let hit_margin = 8.0; // Extra pixels on each side for easier clicking
        let left = self.x as f64 - hit_margin;
        let right = self.x as f64 + SCROLLBAR_WIDTH as f64 + hit_margin;
        let top = self.y as f64;
        let bottom = self.y as f64 + self.track_height as f64;
        px >= left && px <= right && py >= top && py <= bottom
    }

    /// Check if a pixel position hits the thumb specifically
    fn thumb_hit_test(&self, px: f64, py: f64) -> bool {
        let hit_margin = 8.0;
        let left = self.x as f64 - hit_margin;
        let right = self.x as f64 + SCROLLBAR_WIDTH as f64 + hit_margin;
        let top = (self.y + self.thumb_start) as f64;
        let bottom = (self.y + self.thumb_start + self.thumb_height) as f64;
        px >= left && px <= right && py >= top && py <= bottom
    }

    /// Convert a Y pixel position on the track to a scroll offset
    fn y_to_offset(&self, py: f64) -> usize {
        let track_y = py - self.y as f64;
        let scroll_range = self.track_height - self.thumb_height;
        if scroll_range <= 0.0 {
            return 0;
        }
        // Center the thumb on the click position
        let centered = track_y - self.thumb_height as f64 / 2.0;
        let fraction = (centered / scroll_range as f64).clamp(0.0, 1.0);
        // fraction 0 = top = max offset, fraction 1 = bottom = offset 0
        ((1.0 - fraction) * self.history_size as f64).round() as usize
    }
}

/// State for an active scrollbar drag operation
struct ScrollbarDrag {
    pane_id: PaneId,
    /// Y pixel position where the drag started
    start_y: f64,
    /// Display offset when drag started
    start_offset: usize,
    /// Scrollbar geometry snapshot at drag start
    geo: ScrollbarGeometry,
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    layout: LayoutTree,
    terminals: HashMap<PaneId, Terminal>,
    modifiers: ModifiersState,
    selection: Selection,
    /// Scroll counter snapshot when selection was created, for adjusting selection
    /// as new output pushes content up.
    selection_scroll_anchor: Option<u64>,
    mouse_pos: (f64, f64),
    clipboard: Option<Clipboard>,
    last_resize: Option<Instant>,
    last_scroll: HashMap<PaneId, Instant>,
    last_frame: Instant,
    frame_duration: Duration,
    fps_samples: [f32; 60],
    fps_sample_idx: usize,
    /// Rolling buffer of render work times in seconds (excludes vsync/sleep)
    render_time_samples: [f32; 100],
    render_time_idx: usize,
    app_start: Instant,
    config: Config,
    config_ui: ConfigUI,
    search_ui: SearchUI,
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
    /// Track bracketed paste mode per pane for change detection
    bracketed_paste_state: HashMap<PaneId, bool>,
    /// When to show the bracketed paste message (pane_id, start_time, enabled)
    bracketed_paste_message: Option<(PaneId, Instant, bool)>,
    /// Active paste confirmation dialog
    paste_dialog: Option<PasteDialog>,
    /// Accumulator for pixel-based scroll deltas (touchpad)
    scroll_accumulator: f64,
    /// Active scrollbar drag state
    scrollbar_drag: Option<ScrollbarDrag>,
    /// Cached scrollbar geometries from last render (for hit testing)
    scrollbar_geometries: Vec<ScrollbarGeometry>,
    /// Whether the mouse is hovering near any scrollbar edge
    scrollbar_hover_pane: Option<PaneId>,
    /// Receiver for update check results from background thread
    update_receiver: Option<Receiver<UpdateInfo>>,
    /// Cached update info after check completes
    update_info: Option<UpdateInfo>,
    last_font_settings: Option<ActiveFontSettings>,
    /// Global hotkey manager for system-wide show/focus hotkey
    hotkey_manager: Option<global_hotkey::GlobalHotKeyManager>,
    /// Currently registered global hotkey ID (for unregistering)
    registered_hotkey: Option<global_hotkey::hotkey::HotKey>,
    /// Whether the window was hidden via the global hotkey (for dock click re-show)
    hidden_by_hotkey: bool,
    /// Degauss animation start time (None = not active)
    degauss_start: Option<Instant>,
    /// Horizontal sync loss toggle (broken H-HOLD knob)
    hsync_lost: bool,
    /// rm -rf glitch trigger time per pane (None = not active)
    /// bool = true means "rm -rf /" was matched (permanent glitch)
    rmrf_glitch_start: HashMap<PaneId, (Instant, bool)>,
    /// Which panes had rm -rf detected in the previous frame (for edge detection)
    rmrf_was_detected: HashSet<PaneId>,
    /// Panes currently showing "sudo" on screen (rainbow effect while visible)
    sudo_active_panes: HashSet<PaneId>,
}

struct PasteDialog {
    pane_id: PaneId,
    original_text: String,
    paste_text: String,
    stripped_text: String,
    selected: usize,
    bracketed_paste_enabled: bool,
    strip_cr_enabled: bool,
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
            selection_scroll_anchor: None,
            mouse_pos: (0.0, 0.0),
            clipboard: Clipboard::new().ok(),
            last_resize: None,
            last_scroll: HashMap::new(),
            last_frame: Instant::now(),
            frame_duration: Duration::from_nanos(1_000_000_000 / (DEFAULT_FPS * 2) as u64),
            fps_samples: [0.0; 60],
            fps_sample_idx: 0,
            render_time_samples: [0.0; 100],
            render_time_idx: 0,
            app_start: Instant::now(),
            config_ui: ConfigUI::new(config.clone()),
            search_ui: SearchUI::new(),
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
            bracketed_paste_state: HashMap::new(),
            bracketed_paste_message: None,
            paste_dialog: None,
            click_count: 0,
            scroll_accumulator: 0.0,
            scrollbar_drag: None,
            scrollbar_geometries: Vec::new(),
            scrollbar_hover_pane: None,
            update_receiver: None,
            update_info: None,
            last_font_settings: None,
            hotkey_manager: None,
            registered_hotkey: None,
            hidden_by_hotkey: false,
            degauss_start: None,
            hsync_lost: false,
            rmrf_glitch_start: HashMap::new(),
            rmrf_was_detected: HashSet::new(),
            sudo_active_panes: HashSet::new(),
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

    fn paste_needs_confirmation(text: &str) -> bool {
        text.chars()
            .any(|c| c == '\n' || c == '\r' || c.is_control())
    }

    fn strip_control_chars(text: &str) -> String {
        text.chars().filter(|c| !c.is_control()).collect()
    }

    fn strip_carriage_returns(text: &str) -> String {
        text.replace('\r', "")
    }

    fn open_paste_dialog(
        &mut self,
        pane_id: PaneId,
        text: String,
        bracketed_paste: bool,
        strip_cr: bool,
    ) {
        let paste_text = if strip_cr {
            Self::strip_carriage_returns(&text)
        } else {
            text.clone()
        };
        let stripped_text = Self::strip_control_chars(&paste_text);
        self.paste_dialog = Some(PasteDialog {
            pane_id,
            original_text: text,
            paste_text,
            stripped_text,
            selected: 0,
            bracketed_paste_enabled: bracketed_paste,
            strip_cr_enabled: strip_cr,
        });
    }

    fn handle_paste_dialog_input(&mut self, key: &Key) -> bool {
        let Some(dialog) = &mut self.paste_dialog else {
            return false;
        };

        match key {
            Key::Named(NamedKey::Escape) => {
                self.paste_dialog = None;
                return true;
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if dialog.selected == 0 {
                    dialog.selected = 2;
                } else {
                    dialog.selected -= 1;
                }
                return true;
            }
            Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::Tab) => {
                dialog.selected = (dialog.selected + 1) % 3;
                return true;
            }
            Key::Character(c) if c == "1" => {
                dialog.selected = 0;
                return true;
            }
            Key::Character(c) if c == "2" => {
                dialog.selected = 1;
                return true;
            }
            Key::Character(c) if c == "3" => {
                dialog.selected = 2;
                return true;
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                let dialog = self.paste_dialog.take().unwrap();
                match dialog.selected {
                    0 => {
                        if let Some(terminal) = self.terminals.get(&dialog.pane_id) {
                            terminal.input(dialog.paste_text.as_bytes());
                        }
                    }
                    1 => {
                        if let Some(terminal) = self.terminals.get(&dialog.pane_id) {
                            terminal.input(dialog.stripped_text.as_bytes());
                        }
                    }
                    _ => {}
                }
                return true;
            }
            _ => {}
        }

        true
    }

    fn overlay_paste_dialog(
        cells: &mut [Vec<RenderCell>],
        dialog: &PasteDialog,
        color_scheme: &ColorScheme,
    ) {
        let height = cells.len();
        if height == 0 {
            return;
        }
        let width = cells[0].len();
        if width == 0 {
            return;
        }

        let mut panel_width = width.min(44);
        if panel_width < PASTE_DIALOG_MIN_WIDTH {
            panel_width = width.min(PASTE_DIALOG_MIN_WIDTH);
        }
        let panel_height = height.min(7);
        if panel_width < 4 || panel_height < 4 {
            return;
        }

        let start_col = (width.saturating_sub(panel_width)) / 2;
        let start_row = (height.saturating_sub(panel_height)) / 2;

        let fg = color_scheme.foreground;
        let bright = color_scheme.colors[15];
        let border = color_scheme.colors[6];
        let bg = [0.0, 0.0, 0.0, 0.0];
        let highlight_bg = [fg[0] * 0.15, fg[1] * 0.15, fg[2] * 0.15, 1.0];

        let last_row = panel_height - 1;
        let last_col = panel_width - 1;
        let title = " Paste ";
        let title_start = (panel_width.saturating_sub(title.len())) / 2;

        for row in 0..panel_height {
            let grid_row = start_row + row;
            if grid_row >= height {
                continue;
            }
            for col in 0..panel_width {
                let grid_col = start_col + col;
                if grid_col >= width {
                    continue;
                }
                let (c, cell_fg, cell_bg) = if row == 0 {
                    if col == 0 {
                        ('┌', border, bg)
                    } else if col == last_col {
                        ('┐', border, bg)
                    } else if col >= title_start && col < title_start + title.len() {
                        let c = title.chars().nth(col - title_start).unwrap_or('─');
                        (c, bright, bg)
                    } else {
                        ('─', border, bg)
                    }
                } else if row == last_row {
                    if col == 0 {
                        ('└', border, bg)
                    } else if col == last_col {
                        ('┘', border, bg)
                    } else {
                        ('─', border, bg)
                    }
                } else if col == 0 || col == last_col {
                    ('│', border, bg)
                } else {
                    (' ', fg, bg)
                };

                cells[grid_row][grid_col] = RenderCell {
                    c,
                    fg: cell_fg,
                    bg: cell_bg,
                    is_wide: false,
                };
            }
        }

        let inner_left = start_col + 1;
        let inner_width = panel_width - 2;
        let inner_top = start_row + 1;
        let inner_bottom = start_row + panel_height - 2;
        let mut line_row = inner_top;
        if panel_height >= 7 {
            line_row += 1;
        }

        let mut lf_count = 0usize;
        let mut cr_count = 0usize;
        let mut control_counts: std::collections::BTreeMap<u32, usize> =
            std::collections::BTreeMap::new();
        for c in dialog.original_text.chars() {
            if c == '\n' {
                lf_count += 1;
            } else if c == '\r' {
                cr_count += 1;
            } else if c.is_control() {
                *control_counts.entry(c as u32).or_insert(0) += 1;
            }
        }
        let control_count: usize = control_counts.values().sum();
        let ctrl_details = if control_count == 0 {
            None
        } else {
            let mut parts: Vec<String> = Vec::new();
            for (code, count) in control_counts.iter().take(3) {
                parts.push(format!("0x{:02X} x{}", code, count));
            }
            let suffix = if control_counts.len() > 3 { ",..." } else { "" };
            Some(format!("Ctrl: {}{}", parts.join(", "), suffix))
        };
        let mut detail_parts = Vec::new();
        if lf_count > 0 {
            detail_parts.push(format!("LF {}", lf_count));
        }
        if cr_count > 0 {
            detail_parts.push(format!("CR {}", cr_count));
        }
        if control_count > 0 {
            let ctrl = ctrl_details.unwrap_or_else(|| format!("Ctrl {}", control_count));
            detail_parts.push(ctrl);
        }
        let detail_line = if detail_parts.is_empty() {
            "No control chars detected".to_string()
        } else {
            detail_parts.join(", ")
        };

        let bracketed_line = format!(
            "Bracketed paste: {}",
            if dialog.bracketed_paste_enabled {
                "ON"
            } else {
                "OFF"
            }
        );
        let cr_line = format!(
            "Strip CR: {}",
            if dialog.strip_cr_enabled { "ON" } else { "OFF" }
        );
        let message_lines = [
            "Unsafe paste detected".to_string(),
            detail_line,
            bracketed_line,
            cr_line,
        ];
        for line in message_lines {
            if line_row >= inner_bottom {
                break;
            }
            let line_len = line.chars().count();
            let start = inner_left + (inner_width.saturating_sub(line_len)) / 2;
            for (i, ch) in line.chars().enumerate() {
                let col = start + i;
                if col < inner_left + inner_width && line_row < height {
                    cells[line_row][col] = RenderCell {
                        c: ch,
                        fg: bright,
                        bg,
                        is_wide: false,
                    };
                }
            }
            line_row += 1;
        }

        let button_row = inner_bottom;
        let buttons = ["Paste", "Strip Ctrl", "Cancel"];
        let rendered_buttons: Vec<String> = buttons
            .iter()
            .map(|label| format!("[ {} ]", label))
            .collect();
        let buttons_width: usize = rendered_buttons.iter().map(|b| b.len()).sum::<usize>()
            + (rendered_buttons.len() - 1) * 2;
        let buttons_start = inner_left + (inner_width.saturating_sub(buttons_width)) / 2;

        let mut cursor = buttons_start;
        for (idx, button) in rendered_buttons.iter().enumerate() {
            let is_selected = dialog.selected == idx;
            let button_fg = if is_selected { bright } else { fg };
            let button_bg = if is_selected { highlight_bg } else { bg };
            for ch in button.chars() {
                if button_row < height && cursor < inner_left + inner_width {
                    cells[button_row][cursor] = RenderCell {
                        c: ch,
                        fg: button_fg,
                        bg: button_bg,
                        is_wide: false,
                    };
                }
                cursor += 1;
            }
            cursor += 2;
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

    fn is_high_dpi(scale_factor: f64) -> bool {
        scale_factor >= HIGH_DPI_SCALE_THRESHOLD
    }

    fn active_font_settings(config: &Config, scale_factor: f64) -> ActiveFontSettings {
        if Self::is_high_dpi(scale_factor) {
            ActiveFontSettings {
                font: config.high_dpi_font,
                font_size: config.high_dpi_font_size,
                ui_scale: config.high_dpi_ui_scale,
                bdf_font: config.high_dpi_bdf_font,
            }
        } else {
            ActiveFontSettings {
                font: config.font,
                font_size: config.font_size,
                ui_scale: config.ui_scale,
                bdf_font: config.bdf_font,
            }
        }
    }

    fn current_scale_factor(&self) -> f64 {
        self.window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0)
    }

    fn apply_font_settings(&mut self, config: &Config, scale_factor: f64) -> bool {
        let Some(renderer) = &mut self.renderer else {
            return false;
        };

        let settings = Self::active_font_settings(config, scale_factor);
        let needs_update = self
            .last_font_settings
            .map(|prev| !prev.approx_eq(settings))
            .unwrap_or(true);

        if !needs_update {
            return false;
        }

        if let Some(bdf_font) = settings.bdf_font {
            if let Err(e) = renderer.set_bdf_font(bdf_font) {
                tracing::error!("Failed to apply BDF font: {}", e);
            }
        } else if let Err(e) =
            renderer.set_font(settings.font, settings.font_size * settings.ui_scale)
        {
            tracing::error!("Failed to apply font: {}", e);
        }

        self.last_font_settings = Some(settings);
        true
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

        let per_pane_crt = self.current_config().per_pane_crt;
        let (win_width, win_height) = renderer.window_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);
        let focused = self.layout.focused_pane();

        let rect = rects.get(&focused)?;

        // Pane bounds in pixels (with padding)
        let pane_x = (rect.x * win_width as f32 + PANE_PADDING) as f64;
        let pane_y = (rect.y * win_height as f32 + PANE_PADDING) as f64;

        let pane_rect_ref = if per_pane_crt { Some(rect) } else { None };
        let (content_x, content_y) = self.screen_to_content(x, y, pane_rect_ref)?;

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

    /// Map screen-space pixel coordinates to content-space (pre-barrel-distortion).
    /// In global CRT mode, the distortion is window-relative.
    /// In per-pane CRT mode, pass the pane's normalized rect for pane-local distortion.
    /// Returns None if the point is in the void (outside CRT content area).
    fn screen_to_content(
        &self,
        x: f64,
        y: f64,
        pane_rect: Option<&crt_layout::Rect>,
    ) -> Option<(f64, f64)> {
        let renderer = self.renderer.as_ref()?;
        let curvature = self.current_config().effects.screen_curvature as f64;
        let (win_width, win_height) = renderer.window_size();

        if curvature.abs() < 0.0001 {
            return Some((x, y));
        }

        match pane_rect {
            Some(rect) => {
                // Per-pane mode: distortion in local pane space
                let pane_x = (rect.x * win_width as f32 + PANE_PADDING) as f64;
                let pane_y = (rect.y * win_height as f32 + PANE_PADDING) as f64;
                let pane_w = (rect.width * win_width as f32 - PANE_PADDING * 2.0) as f64;
                let pane_h = (rect.height * win_height as f32 - PANE_PADDING * 2.0) as f64;

                let local_uv_x = (x - pane_x) / pane_w;
                let local_uv_y = (y - pane_y) / pane_h;
                let centered_x = local_uv_x * 2.0 - 1.0;
                let centered_y = local_uv_y * 2.0 - 1.0;
                let r2 = centered_x * centered_x + centered_y * centered_y;
                let scale = 1.0 + curvature * r2;
                let content_local_x = (centered_x * scale) * 0.5 + 0.5;
                let content_local_y = (centered_y * scale) * 0.5 + 0.5;

                if !(0.0..=1.0).contains(&content_local_x)
                    || !(0.0..=1.0).contains(&content_local_y)
                {
                    return None;
                }
                Some((
                    pane_x + content_local_x * pane_w,
                    pane_y + content_local_y * pane_h,
                ))
            }
            None => {
                // Global mode: distortion in window space
                let uv_x = x / win_width as f64;
                let uv_y = y / win_height as f64;
                let centered_x = uv_x * 2.0 - 1.0;
                let centered_y = uv_y * 2.0 - 1.0;
                let r2 = centered_x * centered_x + centered_y * centered_y;
                let scale = 1.0 + curvature * r2;
                let content_uv_x = (centered_x * scale) * 0.5 + 0.5;
                let content_uv_y = (centered_y * scale) * 0.5 + 0.5;

                if !(0.0..=1.0).contains(&content_uv_x) || !(0.0..=1.0).contains(&content_uv_y) {
                    return None;
                }
                Some((
                    content_uv_x * win_width as f64,
                    content_uv_y * win_height as f64,
                ))
            }
        }
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

    /// Dump every cell in the current selection (as a rectangle) to the clipboard
    /// for debugging. Only useful while the debug grid is on — intended for
    /// "what is actually on screen at this cell?" investigations.
    fn copy_selection_debug(&mut self) {
        let focused = self.layout.focused_pane();
        let Some(terminal) = self.terminals.get(&focused) else {
            return;
        };

        let (start, end) = self.selection.normalized();

        let dump = terminal.with_grid(|grid| {
            use alacritty_terminal::grid::Dimensions;
            use alacritty_terminal::index::{Column, Line};
            use alacritty_terminal::term::cell::Flags;

            let cols = grid.columns();
            let col_lo = start.col.min(cols.saturating_sub(1));
            let col_hi = end.col.min(cols.saturating_sub(1));
            let row_count = (end.row - start.row + 1).max(0) as usize;
            let col_count = col_hi.saturating_sub(col_lo) + 1;

            let mut out = String::new();
            out.push_str(&format!(
                "# crt debug cell dump\n# rows {}..={}  cols {}..={}  ({} rows x {} cols = {} cells)\n",
                start.row,
                end.row,
                col_lo,
                col_hi,
                row_count,
                col_count,
                row_count * col_count,
            ));
            out.push_str("row\tcol\tchar\tcodepoint\tfg\tbg\tflags\twide\tzerowidth\n");

            for row in start.row..=end.row {
                let line = Line(row);
                for col in col_lo..=col_hi {
                    let cell = &grid[line][Column(col)];
                    let c = cell.c;
                    let char_repr = match c {
                        '\0' => "NUL".to_string(),
                        ' ' => "SP".to_string(),
                        '\t' => "TAB".to_string(),
                        _ if c.is_control() => format!("CTRL(\\x{:02x})", c as u32),
                        _ => format!("{:?}", c),
                    };
                    let zw = cell
                        .zerowidth()
                        .map(|zs| {
                            zs.iter()
                                .map(|ch| format!("U+{:04X}", *ch as u32))
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    let wide = cell.flags.contains(Flags::WIDE_CHAR);
                    out.push_str(&format!(
                        "{}\t{}\t{}\tU+{:04X}\t{:?}\t{:?}\t{:?}\t{}\t{}\n",
                        row, col, char_repr, c as u32, cell.fg, cell.bg, cell.flags, wide, zw
                    ));
                }
            }
            out
        });

        if let Some(clipboard) = &mut self.clipboard {
            match clipboard.set_text(&dump) {
                Ok(_) => {
                    tracing::info!("Copied debug cell dump ({} bytes) to clipboard", dump.len())
                }
                Err(e) => tracing::error!("Failed to copy debug dump: {}", e),
            }
        }
    }

    /// Check if the mouse is within proximity of any pane's scrollbar edge.
    /// Returns the PaneId if hovering near a scrollbar area.
    /// Coordinates are undistorted to account for barrel distortion.
    fn check_scrollbar_hover(&self, mouse_x: f64, mouse_y: f64) -> Option<PaneId> {
        let renderer = self.renderer.as_ref()?;
        let (win_width, win_height) = renderer.window_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);
        let per_pane_crt = self.current_config().per_pane_crt;

        for pane_id in self.layout.panes() {
            let Some(rect) = rects.get(pane_id) else {
                continue;
            };
            let Some(terminal) = self.terminals.get(pane_id) else {
                continue;
            };
            if terminal.history_size() == 0 {
                continue;
            }

            // Undistort mouse position to content-space
            let pane_rect = if per_pane_crt { Some(rect) } else { None };
            let Some((cx, cy)) = self.screen_to_content(mouse_x, mouse_y, pane_rect) else {
                continue;
            };

            let pane_right = (rect.x + rect.width) as f64 * win_width as f64 - PANE_PADDING as f64;
            let pane_top = rect.y as f64 * win_height as f64 + PANE_PADDING as f64;
            let pane_bottom =
                (rect.y + rect.height) as f64 * win_height as f64 - PANE_PADDING as f64;

            if cx >= pane_right - SCROLLBAR_HOVER_PROXIMITY
                && cx <= pane_right
                && cy >= pane_top
                && cy <= pane_bottom
            {
                return Some(*pane_id);
            }
        }
        None
    }

    /// Find the scrollbar geometry for a given screen-space pixel position, if any.
    /// Undistorts coordinates before hit testing.
    fn scrollbar_at(&self, screen_x: f64, screen_y: f64) -> Option<ScrollbarGeometry> {
        let renderer = self.renderer.as_ref()?;
        let (win_width, win_height) = renderer.window_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);
        let per_pane_crt = self.current_config().per_pane_crt;

        for geo in &self.scrollbar_geometries {
            let pane_rect = if per_pane_crt {
                rects.get(&geo.pane_id)
            } else {
                None
            };
            let Some((cx, cy)) = self.screen_to_content(screen_x, screen_y, pane_rect) else {
                continue;
            };
            if geo.hit_test(cx, cy) {
                return Some(*geo);
            }
        }
        None
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
        self.create_terminal_for_pane_with_session(pane_id, None, None);
    }

    fn create_terminal_for_pane_with_session(
        &mut self,
        pane_id: PaneId,
        working_directory: Option<std::path::PathBuf>,
        scrollback: Option<&[u8]>,
    ) {
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

            let result = if working_directory.is_some() {
                Terminal::with_working_directory(cols, rows, working_directory)
            } else {
                Terminal::new(cols, rows)
            };

            match result {
                Ok(terminal) => {
                    // Note: Scrollback data is captured but not restored to display.
                    // Proper scrollback restore would require direct grid manipulation,
                    // which alacritty_terminal doesn't easily expose. For now we just
                    // restore the working directory.
                    if scrollback.is_some() {
                        tracing::debug!(
                            "Scrollback data available for pane {:?} but display restore not implemented",
                            pane_id
                        );
                    }

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
        let _span = tracing::trace_span!("render_terminals").entered();

        // Record frame time for FPS display
        let fps = self.record_frame_time(dt);

        let scale_factor = self
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0);

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

        if !self.config_ui.visible {
            let config = self.config.clone();
            self.apply_font_settings(&config, scale_factor);
        }

        let Some(renderer) = &mut self.renderer else {
            return;
        };

        let (win_width, win_height) = renderer.window_size();
        let (cell_w, cell_h) = renderer.cell_size();
        let rects = self.layout.pane_rects(win_width as f32, win_height as f32);
        let focused_pane = self.layout.focused_pane();

        // Adjust selection for any lines that scrolled since it was created
        if let Some(anchor) = self.selection_scroll_anchor {
            if let Some(terminal) = self.terminals.get(&focused_pane) {
                let current = terminal.total_lines_scrolled();
                let delta = current.saturating_sub(anchor) as i32;
                if delta > 0 {
                    self.selection.start.row -= delta;
                    self.selection.end.row -= delta;
                    self.selection_scroll_anchor = Some(current);
                }
            }
        }

        #[allow(clippy::type_complexity)]
        let mut pane_renders: Vec<(PaneId, f32, f32, Vec<Vec<RenderCell>>, Option<usize>)> =
            Vec::new();

        let _grid_span = tracing::trace_span!("read_terminal_grids").entered();
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

            // Check for bracketed paste mode changes
            let bracketed_enabled = term_mode.contains(TermMode::BRACKETED_PASTE);
            let prev_bracketed = self.bracketed_paste_state.get(pane_id).copied();
            if prev_bracketed != Some(bracketed_enabled) {
                self.bracketed_paste_state
                    .insert(*pane_id, bracketed_enabled);
                if prev_bracketed.is_some() {
                    self.bracketed_paste_message =
                        Some((*pane_id, Instant::now(), bracketed_enabled));
                }
            }

            // Add padding offset, rounded to integer pixels for crisp bitmap font rendering
            let x_offset = (rect.x * win_width as f32 + PANE_PADDING).floor();
            let y_offset = (rect.y * win_height as f32 + PANE_PADDING).floor();

            // Only show cursor in focused pane
            let is_focused = *pane_id == focused_pane;

            let cursor_pos = terminal.cursor_position();
            let selection = &self.selection;
            let search_active = self.search_ui.visible && is_focused;
            let search_ui = &self.search_ui;

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
                        let is_search_match =
                            search_active && search_ui.is_match(line, Column(col_idx));
                        let is_current_match =
                            search_active && search_ui.is_current_match(line, Column(col_idx));
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

                        let (fg, bg) = if is_cursor {
                            // Cursor uses fixed scheme colors rather than inverting
                            // the cell. Inverting breaks when the running TUI already
                            // drew its own cursor as an INVERSE cell (double-invert =
                            // invisible cursor).
                            (color_scheme.background, color_scheme.foreground)
                        } else if is_selected {
                            // Invert: swap fg and bg
                            (resolved_bg, cell_fg)
                        } else if is_current_match {
                            // Current match: bright inverted highlight
                            (resolved_bg, cell_fg)
                        } else if is_search_match {
                            // Other matches: subtle highlight background
                            let match_bg = [
                                color_scheme.foreground[0] * 0.25,
                                color_scheme.foreground[1] * 0.25,
                                color_scheme.foreground[2] * 0.25,
                                1.0,
                            ];
                            (cell_fg, match_bg)
                        } else {
                            (cell_fg, cell_bg)
                        };

                        row.push(RenderCell { c, fg, bg, is_wide });
                    }

                    rows.push(row);
                }

                rows
            });

            // Compute cursor's rendered row index for easter egg detection
            let cursor_row = cursor_pos.map(|(_col, line)| {
                let display_offset = terminal.display_offset();
                line + display_offset
            });

            pane_renders.push((*pane_id, x_offset, y_offset, cells, cursor_row));
        }
        drop(_grid_span);

        // Easter egg: scan visible terminal content for "rm -rf" and trigger per-pane glitch
        // Only matches when all chars in the pattern share the same fg color (ignores
        // autocomplete ghost text which is typically rendered in a different/dimmer color)
        // "rm -rf /" triggers permanent corruption; plain "rm -rf" is a brief burst
        {
            let mut currently_detected = HashSet::new();
            for (pane_id, _x, _y, cells, _cursor) in &pane_renders {
                let mut found = false;
                let mut found_slash = false;
                for row in cells {
                    let line: String = row.iter().map(|c| c.c).collect();
                    // Check each possible match position
                    for pattern in &["rm -rf", "rm  -rf"] {
                        let mut search_from = 0;
                        while let Some(pos) = line[search_from..].find(pattern) {
                            let start = search_from + pos;
                            let end = start + pattern.len();
                            // Verify all cells in the match have the same fg color
                            if end <= row.len() {
                                let ref_fg = row[start].fg;
                                let uniform_style = (start..end).all(|i| {
                                    let fg = row[i].fg;
                                    (fg[0] - ref_fg[0]).abs() < 0.01
                                        && (fg[1] - ref_fg[1]).abs() < 0.01
                                        && (fg[2] - ref_fg[2]).abs() < 0.01
                                });
                                if uniform_style {
                                    found = true;
                                    // Check if followed by " /" (with same fg color)
                                    let slash_patterns = [" /", "  /"];
                                    for sp in &slash_patterns {
                                        let slash_end = end + sp.len();
                                        if slash_end <= row.len() && line[end..].starts_with(sp) {
                                            let slash_uniform = (end..slash_end).all(|i| {
                                                let fg = row[i].fg;
                                                (fg[0] - ref_fg[0]).abs() < 0.01
                                                    && (fg[1] - ref_fg[1]).abs() < 0.01
                                                    && (fg[2] - ref_fg[2]).abs() < 0.01
                                            });
                                            if slash_uniform {
                                                found_slash = true;
                                            }
                                        }
                                    }
                                    break;
                                }
                            }
                            search_from = start + 1;
                        }
                        if found {
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
                if found {
                    currently_detected.insert(*pane_id);
                    // Trigger on rising edge (first frame this pane shows rm -rf)
                    if !self.rmrf_was_detected.contains(pane_id) {
                        self.rmrf_glitch_start
                            .insert(*pane_id, (Instant::now(), found_slash));
                    } else if found_slash {
                        // Upgrade existing glitch to permanent if "/" just appeared
                        if let Some(entry) = self.rmrf_glitch_start.get_mut(pane_id) {
                            if !entry.1 {
                                entry.1 = true;
                                entry.0 = Instant::now(); // Reset to full intensity
                            }
                        }
                    }
                }
            }
            self.rmrf_was_detected = currently_detected;
        }

        // Easter egg: detect "sudo" on screen — rainbow power surge while visible
        // Uses same uniform-fg-color check to ignore autocomplete suggestions
        {
            let mut sudo_panes = HashSet::new();
            for (pane_id, _x, _y, cells, cursor_row) in &pane_renders {
                // Only check the cursor line (active prompt), not scrollback history
                let Some(crow) = cursor_row else {
                    continue;
                };
                if *crow >= cells.len() {
                    continue;
                }
                let row = &cells[*crow];
                let line: String = row.iter().map(|c| c.c).collect();
                let mut found = false;
                let mut search_from = 0;
                while let Some(pos) = line[search_from..].find("sudo") {
                    let start = search_from + pos;
                    let end = start + 4; // "sudo".len()
                    if end <= row.len() {
                        let ref_fg = row[start].fg;
                        let uniform_style = (start..end).all(|i| {
                            let fg = row[i].fg;
                            (fg[0] - ref_fg[0]).abs() < 0.01
                                && (fg[1] - ref_fg[1]).abs() < 0.01
                                && (fg[2] - ref_fg[2]).abs() < 0.01
                        });
                        if uniform_style {
                            found = true;
                            break;
                        }
                    }
                    search_from = start + 1;
                }
                if found {
                    sudo_panes.insert(*pane_id);
                }
            }
            self.sudo_active_panes = sudo_panes;
        }

        // Apply per-pane rm -rf glitch: corrupt cell colors with static noise
        const RMRF_GLITCH_DURATION: f32 = 0.5;
        {
            let mut expired = Vec::new();
            for (pane_id, (start, permanent)) in &self.rmrf_glitch_start {
                let elapsed = start.elapsed().as_secs_f32();
                if !permanent && elapsed >= RMRF_GLITCH_DURATION {
                    expired.push(*pane_id);
                    continue;
                }
                // Permanent: full intensity forever. Brief: sharp attack, exponential decay
                let intensity = if *permanent {
                    1.0
                } else {
                    (1.0 - elapsed / RMRF_GLITCH_DURATION).powi(2)
                };

                // Find and corrupt this pane's cells
                for (render_pane_id, _x, _y, cells, _cursor) in &mut pane_renders {
                    if render_pane_id != pane_id {
                        continue;
                    }
                    // Use a simple hash to pseudo-randomly corrupt cells
                    let frame_seed = (elapsed * 1000.0) as u32;
                    for (row_idx, row) in cells.iter_mut().enumerate() {
                        for (col_idx, cell) in row.iter_mut().enumerate() {
                            // Hash the position + frame to decide if this cell gets glitched
                            let hash = (row_idx as u32)
                                .wrapping_mul(374761393)
                                .wrapping_add(col_idx as u32)
                                .wrapping_mul(668265263)
                                .wrapping_add(frame_seed.wrapping_mul(1013904223));
                            let rand = (hash >> 16) as f32 / 65535.0;

                            // Probability of corruption decreases with intensity
                            if rand < intensity * 0.3 {
                                // Randomly brighten fg or add static-white to bg
                                let noise = ((hash >> 8) & 0xFF) as f32 / 255.0;
                                cell.fg = [noise, noise, noise, 1.0];
                                if noise > 0.5 {
                                    cell.bg = [noise * 0.3, noise * 0.3, noise * 0.3, 1.0];
                                }
                            }
                        }
                    }
                }
            }
            for pane_id in expired {
                self.rmrf_glitch_start.remove(&pane_id);
            }
        }

        // Apply per-pane sudo power surge: rainbow-cycle all text colors
        if !self.sudo_active_panes.is_empty() {
            let time = self.app_start.elapsed().as_secs_f32();
            for (pane_id, _x, _y, cells, _cursor) in &mut pane_renders {
                if !self.sudo_active_panes.contains(pane_id) {
                    continue;
                }
                for (row_idx, row) in cells.iter_mut().enumerate() {
                    for (col_idx, cell) in row.iter_mut().enumerate() {
                        if cell.c == ' ' || cell.c == '\0' {
                            continue;
                        }
                        // Per-cell hue offset based on position + time for a rolling rainbow
                        let hue = time * 3.0 + row_idx as f32 * 0.15 + col_idx as f32 * 0.08;
                        // HSV to RGB (saturation=0.7, value=brightness from original)
                        let brightness = cell.fg[0].max(cell.fg[1]).max(cell.fg[2]);
                        let brightness = brightness.max(0.6); // Ensure minimum brightness
                        let h = (hue % 1.0 + 1.0) % 1.0 * 6.0;
                        let f = h - h.floor();
                        let s = 0.7_f32;
                        let p = brightness * (1.0 - s);
                        let q = brightness * (1.0 - s * f);
                        let t = brightness * (1.0 - s * (1.0 - f));
                        let (r, g, b) = match h.floor() as i32 % 6 {
                            0 => (brightness, t, p),
                            1 => (q, brightness, p),
                            2 => (p, brightness, t),
                            3 => (p, q, brightness),
                            4 => (t, p, brightness),
                            _ => (brightness, p, q),
                        };
                        cell.fg = [r, g, b, cell.fg[3]];
                    }
                }
            }
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
        if let Some(dialog) = &self.paste_dialog {
            if let Some((_, _, _, cells, _)) = pane_renders
                .iter_mut()
                .find(|(pane_id, _, _, _, _)| *pane_id == dialog.pane_id)
            {
                Self::overlay_paste_dialog(cells, dialog, &color_scheme);
            }
        }

        // Overlay search bar on focused pane
        if self.search_ui.visible {
            if let Some((_, _, _, cells, _)) = pane_renders
                .iter_mut()
                .find(|(pane_id, _, _, _, _)| *pane_id == focused_pane)
            {
                SearchUI::overlay_search_bar(cells, &self.search_ui, &color_scheme);
            }
        }

        let panes: Vec<(f32, f32, &[Vec<RenderCell>])> = pane_renders
            .iter()
            .map(|(_, x, y, cells, _)| (*x, *y, cells.as_slice()))
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

        // Add FPS counter and render timing in bottom-left when debug grid is enabled
        if self.debug_grid {
            let fps_text = format!("{:.0} FPS", fps);
            let text_width = fps_text.len() as f32 * cell_w;
            // Position: bottom-left, with some padding
            let x = text_width / 2.0 + cell_w;
            let y = win_height as f32 - cell_h * 1.5;
            size_indicators.push((x, y, fps_text));

            // Render work time stats (avg / max of last 100 frames)
            let samples = &self.render_time_samples;
            let valid: Vec<f32> = samples.iter().copied().filter(|&t| t > 0.0).collect();
            if !valid.is_empty() {
                let avg_ms = valid.iter().sum::<f32>() / valid.len() as f32 * 1000.0;
                let max_ms = valid.iter().cloned().fold(0.0f32, f32::max) * 1000.0;
                let timing_text = format!("render: {:.1}ms avg / {:.1}ms max", avg_ms, max_ms);
                let timing_width = timing_text.len() as f32 * cell_w;
                let tx = timing_width / 2.0 + cell_w;
                let ty = win_height as f32 - cell_h * 3.0;
                size_indicators.push((tx, ty, timing_text));
            }

            // GPU stats from last frame
            {
                let stats = renderer.last_stats();
                let stats_text = format!(
                    "glyphs: {} inst  lines: {} quads  verts: {}  idx: {}{}",
                    stats.glyph_instances,
                    stats.line_quads,
                    stats.total_vertices,
                    stats.total_indices,
                    if stats.atlas_uploaded {
                        "  [atlas upload]"
                    } else {
                        ""
                    },
                );
                let sw = stats_text.len() as f32 * cell_w;
                let sx = sw / 2.0 + cell_w;
                let sy = win_height as f32 - cell_h * 4.5;
                size_indicators.push((sx, sy, stats_text));
            }
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

                    // Check if update is available
                    let has_update = self
                        .update_info
                        .as_ref()
                        .map(|i| i.update_available)
                        .unwrap_or(false);
                    let line_offset = if has_update { 0.5 } else { 0.0 };

                    // Show version and hint lines
                    size_indicators.push((
                        center_x,
                        center_y - cell_h * (4.0 + line_offset),
                        format!("Cool Rust Term v{}", env!("CARGO_PKG_VERSION")),
                    ));

                    // Show update available message if applicable
                    if let Some(ref info) = self.update_info {
                        if info.update_available {
                            size_indicators.push((
                                center_x,
                                center_y - cell_h * 2.5,
                                format!("Update available: v{}", info.latest_version),
                            ));
                        }
                    }

                    let hints = [
                        "Ctrl+, for settings",
                        "Ctrl+Shift+Enter for new pane",
                        "Ctrl+Shift+Arrow to navigate panes",
                        "Ctrl+Shift+F to search scrollback",
                    ];
                    for (i, hint) in hints.iter().enumerate() {
                        size_indicators.push((
                            center_x,
                            center_y + cell_h * (i as f32 * 1.5 + line_offset),
                            hint.to_string(),
                        ));
                    }
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

        // Show bracketed paste mode status message (top right of pane)
        if let Some((pane_id, start_time, enabled)) = self.bracketed_paste_message {
            let elapsed = start_time.elapsed().as_secs_f32();
            if elapsed < BRACKETED_MSG_DURATION {
                if let Some(rect) = rects.get(&pane_id) {
                    let msg = if enabled {
                        "Bracketed paste enabled"
                    } else {
                        "Bracketed paste disabled"
                    };
                    let msg_width = msg.len() as f32 * cell_w;
                    let x =
                        (rect.x + rect.width) * win_width as f32 - msg_width / 2.0 - PANE_PADDING;
                    let mut y = rect.y * win_height as f32 + cell_h + PANE_PADDING;

                    if let Some((kitty_pane, kitty_start, _kitty_enabled, kitty_compat)) =
                        self.kitty_mode_message
                    {
                        let kitty_elapsed = kitty_start.elapsed().as_secs_f32();
                        if kitty_pane == pane_id && kitty_elapsed < KITTY_MSG_DURATION {
                            let kitty_lines = if kitty_compat { 2 } else { 1 };
                            y += cell_h * 1.2 * kitty_lines as f32;
                        }
                    }

                    size_indicators.push((x, y, msg.to_string()));
                }
            } else {
                self.bracketed_paste_message = None;
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

        // Calculate scrollbar geometries for each pane
        self.scrollbar_geometries = self
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

                // Calculate per-pane scrollbar opacity from multiple sources
                let scroll_opacity = self
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

                // Scrollbar is also visible when hovering near it, dragging, or searching
                let hover_visible = self.scrollbar_hover_pane == Some(*pane_id);
                let drag_visible = self
                    .scrollbar_drag
                    .as_ref()
                    .map(|d| d.pane_id == *pane_id)
                    .unwrap_or(false);
                let search_visible = self.search_ui.visible
                    && *pane_id == focused_pane
                    && !self.search_ui.matches.is_empty();

                let scrollbar_opacity = if drag_visible || hover_visible || search_visible {
                    1.0
                } else {
                    scroll_opacity
                };

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
                let scroll_fraction = if history > 0 {
                    offset as f32 / history as f32
                } else {
                    0.0
                };
                let thumb_start = (1.0 - scroll_fraction) * (track_height - thumb_height);

                Some(ScrollbarGeometry {
                    pane_id: *pane_id,
                    x: scrollbar_x,
                    y: pane_y,
                    track_height,
                    thumb_start,
                    thumb_height,
                    opacity: scrollbar_opacity,
                    history_size: history,
                })
            })
            .collect();

        let scrollbars: Vec<(f32, f32, f32, f32, f32, f32)> = self
            .scrollbar_geometries
            .iter()
            .map(|g| g.to_render_tuple())
            .collect();

        // If config UI is visible, render it instead of terminals
        if self.config_ui.visible {
            let preview_font = Self::active_font_settings(&self.config_ui.config, scale_factor);
            if let Some(bdf_font) = preview_font.bdf_font {
                if let Err(e) = renderer.set_bdf_font(bdf_font) {
                    tracing::error!("Failed to preview BDF font: {}", e);
                }
            } else if let Err(e) = renderer.set_font(
                preview_font.font,
                preview_font.font_size * preview_font.ui_scale,
            ) {
                tracing::error!("Failed to preview font: {}", e);
            }

            let (cell_w, cell_h) = renderer.cell_size();
            let width_cells = (win_width as f32 / cell_w) as usize;
            let height_cells = (win_height as f32 / cell_h) as usize;

            let ui_cells =
                self.config_ui
                    .render(width_cells, height_cells, Self::is_high_dpi(scale_factor));
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
                degauss_progress: 0.0,
                hsync_intensity: 0.0,
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
            let fg = self.config.color_scheme.foreground;

            // Calculate degauss animation progress (0.8 second animation)
            const DEGAUSS_DURATION: f32 = 0.8;
            let degauss_progress = if let Some(start) = self.degauss_start {
                let elapsed = start.elapsed().as_secs_f32();
                if elapsed >= DEGAUSS_DURATION {
                    self.degauss_start = None;
                    0.0
                } else {
                    elapsed / DEGAUSS_DURATION
                }
            } else {
                0.0
            };

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
                degauss_progress,
                hsync_intensity: if self.hsync_lost { 1.0 } else { 0.0 },
            };

            // Build debug visualization lines - green rectangle around hovered cell
            let mut debug_lines: Vec<(f32, f32, f32, f32, f32, [f32; 4])> =
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

            // Add search match indicators in the scrollbar track
            if self.search_ui.visible && !self.search_ui.matches.is_empty() {
                if let Some(geo) = self
                    .scrollbar_geometries
                    .iter()
                    .find(|g| g.pane_id == focused_pane)
                {
                    let total_lines = geo.history_size as f32
                        + self
                            .terminals
                            .get(&focused_pane)
                            .map(|t| t.size().1 as f32)
                            .unwrap_or(0.0);

                    let fg = color_scheme.foreground;
                    let match_color = [fg[0] * 0.6, fg[1] * 0.6, fg[2] * 0.6, geo.opacity];
                    let current_color = [fg[0], fg[1], fg[2], geo.opacity];
                    let indicator_width = SCROLLBAR_WIDTH + 4.0;

                    for (i, m) in self.search_ui.matches.iter().enumerate() {
                        // Line(-history_size) = top of track, Line(screen_lines-1) = bottom
                        let line_from_top = (m.start.line.0 + geo.history_size as i32) as f32;
                        let fraction = line_from_top / total_lines;
                        let y = geo.y + fraction * geo.track_height;

                        let is_current = i == self.search_ui.current_match;
                        let (color, width) = if is_current {
                            (current_color, indicator_width + 2.0)
                        } else {
                            (match_color, indicator_width)
                        };

                        let x_center = geo.x + SCROLLBAR_WIDTH / 2.0;
                        debug_lines.push((
                            x_center - width / 2.0,
                            y,
                            x_center + width / 2.0,
                            y,
                            2.0,
                            color,
                        ));
                    }
                }
            }

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

    /// Initialize the global hotkey manager and register the configured hotkey (if any)
    fn init_global_hotkey(&mut self) {
        match global_hotkey::GlobalHotKeyManager::new() {
            Ok(manager) => {
                self.hotkey_manager = Some(manager);
                if let Some(hotkey_str) = self.config.behavior.global_hotkey.clone() {
                    self.register_global_hotkey(&hotkey_str);
                }
            }
            Err(e) => {
                tracing::error!("Failed to create global hotkey manager: {}", e);
            }
        }
    }

    /// Register a global hotkey from a string like "Ctrl+Shift+T" or "F12"
    fn register_global_hotkey(&mut self, hotkey_str: &str) {
        // Unregister any existing hotkey first
        if let (Some(manager), Some(old)) = (&self.hotkey_manager, self.registered_hotkey.take()) {
            let _ = manager.unregister(old);
        }

        let Some(manager) = &self.hotkey_manager else {
            return;
        };

        match hotkey_str.parse::<global_hotkey::hotkey::HotKey>() {
            Ok(hotkey) => {
                if let Err(e) = manager.register(hotkey) {
                    tracing::error!("Failed to register global hotkey '{}': {}", hotkey_str, e);
                } else {
                    tracing::info!("Registered global hotkey: {}", hotkey_str);
                    self.registered_hotkey = Some(hotkey);
                }
            }
            Err(e) => {
                tracing::error!("Failed to parse global hotkey '{}': {}", hotkey_str, e);
            }
        }
    }

    /// Unregister the current global hotkey
    fn unregister_global_hotkey(&mut self) {
        if let (Some(manager), Some(hotkey)) = (&self.hotkey_manager, self.registered_hotkey.take())
        {
            if let Err(e) = manager.unregister(hotkey) {
                tracing::warn!("Failed to unregister global hotkey: {}", e);
            }
        }
    }

    /// Check for global hotkey events and focus the window if triggered
    fn poll_global_hotkey(&mut self) {
        if self.registered_hotkey.is_none() {
            return;
        }
        if let Ok(event) = global_hotkey::GlobalHotKeyEvent::receiver().try_recv() {
            if event.state != global_hotkey::HotKeyState::Pressed {
                return;
            }
            if let Some(window) = &self.window {
                if self.hidden_by_hotkey {
                    window.set_visible(true);
                    window.focus_window();
                    self.hidden_by_hotkey = false;
                    tracing::info!("Global hotkey triggered: showing window");
                } else {
                    window.set_visible(false);
                    self.hidden_by_hotkey = true;
                    tracing::info!("Global hotkey triggered: hiding window");
                }
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Spawn background update check if enabled
        if self.config.behavior.check_for_updates && self.update_receiver.is_none() {
            let (tx, rx) = mpsc::channel();
            self.update_receiver = Some(rx);
            std::thread::spawn(move || {
                if let Some(info) = crt_core::update_check::check_for_updates() {
                    let _ = tx.send(info);
                }
            });
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

        // Initialize renderer with font from config (scale-aware)
        let scale_factor = window.scale_factor();
        let active_font = Self::active_font_settings(&self.config, scale_factor);
        let mut renderer = pollster::block_on(Renderer::new(
            Arc::clone(&window),
            active_font.font,
            active_font.font_size * active_font.ui_scale,
        ))
        .expect("Failed to create renderer");

        // If BDF font is configured, load and apply it
        if let Some(bdf_font) = active_font.bdf_font {
            if let Err(e) = renderer.set_bdf_font(bdf_font) {
                tracing::error!("Failed to load BDF font {:?}: {}", bdf_font, e);
            } else {
                tracing::info!("Loaded BDF font: {}", bdf_font.label());
            }
        }
        self.last_font_settings = Some(active_font);

        // Log scale factor for debugging
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

        // Try to load session data for restoration (Unix only, if enabled)
        #[cfg(not(windows))]
        let session = if self.config.behavior.restore_session {
            SessionData::load_from_default()
        } else {
            None
        };
        #[cfg(windows)]
        let session: Option<SessionData> = None;

        // Create terminal for the initial pane
        let initial_pane = self.layout.focused_pane();
        if let Some(ref sess) = session {
            if let Some(pane_session) = sess.panes.first() {
                self.create_terminal_for_pane_with_session(
                    initial_pane,
                    pane_session.cwd.clone(),
                    Some(&pane_session.scrollback),
                );
            } else {
                self.create_terminal_for_pane(initial_pane);
            }
        } else {
            self.create_terminal_for_pane(initial_pane);
        }

        // Restore additional panes from saved config (use session data if available)
        let panes_to_restore = self.config.pane_count.saturating_sub(1);
        for i in 0..panes_to_restore {
            let new_pane_id = self.layout.add_pane();
            self.resize_terminals();

            // Get session data for this pane index (i+1 because first pane is index 0)
            let pane_idx = (i + 1) as usize;
            if let Some(ref sess) = session {
                if let Some(pane_session) = sess.panes.get(pane_idx) {
                    self.create_terminal_for_pane_with_session(
                        new_pane_id,
                        pane_session.cwd.clone(),
                        Some(&pane_session.scrollback),
                    );
                } else {
                    self.create_terminal_for_pane(new_pane_id);
                }
            } else {
                self.create_terminal_for_pane(new_pane_id);
            }
        }
        if panes_to_restore > 0 {
            tracing::info!("Restored {} additional panes from config", panes_to_restore);
        }
        if session.is_some() {
            tracing::info!("Session data restored");
        }

        // Initialize global hotkey manager
        self.init_global_hotkey();

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
                // Save session data (scrollback + cwd for each pane) if enabled
                #[cfg(not(windows))]
                if self.config.behavior.restore_session {
                    let mut session = SessionData::new();
                    for (idx, pane_id) in self.layout.panes().iter().enumerate() {
                        if let Some(terminal) = self.terminals.get(pane_id) {
                            let scrollback = terminal.capture_scrollback();
                            let compressed = scrollback.compress().unwrap_or_default();
                            let cwd = terminal.working_directory();
                            session.add_pane(compressed, cwd, idx);
                        }
                    }
                    if let Err(e) = session.save_to_default() {
                        tracing::error!("Failed to save session: {}", e);
                    } else {
                        tracing::info!("Session saved ({} panes)", session.panes.len());
                    }
                }

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
                let scale_factor = self
                    .window
                    .as_ref()
                    .map(|window| window.scale_factor())
                    .unwrap_or(1.0);
                let config = if self.config_ui.visible {
                    self.config_ui.config.clone()
                } else {
                    self.config.clone()
                };
                if self.apply_font_settings(&config, scale_factor) {
                    self.resize_terminals();
                }
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
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                let new_size = self
                    .window
                    .as_ref()
                    .map(|window| window.inner_size())
                    .unwrap_or_else(|| {
                        PhysicalSize::new(self.config.window_width, self.config.window_height)
                    });
                let _ = inner_size_writer.request_inner_size(new_size);
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(new_size.width, new_size.height);
                    self.resize_terminals();
                    self.last_resize = Some(Instant::now());
                }
                self.config.window_width = new_size.width;
                self.config.window_height = new_size.height;
                let config = if self.config_ui.visible {
                    self.config_ui.config.clone()
                } else {
                    self.config.clone()
                };
                if self.apply_font_settings(&config, scale_factor) {
                    self.resize_terminals();
                }
            }
            WindowEvent::RedrawRequested => {
                #[cfg(feature = "tracy")]
                if let Some(c) = tracy_client::Client::running() {
                    c.frame_mark();
                }

                let _span = tracing::trace_span!("redraw_requested").entered();

                // Poll for global hotkey events
                self.poll_global_hotkey();

                // Check for update check results
                if let Some(ref rx) = self.update_receiver {
                    if let Ok(info) = rx.try_recv() {
                        if info.update_available {
                            tracing::info!(
                                "Update available: v{} -> v{}",
                                info.current_version,
                                info.latest_version
                            );
                        }
                        self.update_info = Some(info);
                    }
                }

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
                    let render_start = Instant::now();
                    self.render_terminals(dt);
                    let render_time = render_start.elapsed().as_secs_f32();
                    self.render_time_samples[self.render_time_idx] = render_time;
                    self.render_time_idx =
                        (self.render_time_idx + 1) % self.render_time_samples.len();
                } else {
                    // Sleep for remaining time to avoid busy-waiting
                    let _sleep_span = tracing::trace_span!("frame_sleep").entered();
                    std::thread::sleep(self.frame_duration - elapsed);
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Focused(true) => {
                // Re-show window when activated (e.g. dock click) after hiding via hotkey
                if self.hidden_by_hotkey {
                    if let Some(window) = &self.window {
                        window.set_visible(true);
                    }
                    self.hidden_by_hotkey = false;
                    tracing::info!("Window re-shown via focus (was hidden by hotkey)");
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x, position.y);

                // Handle scrollbar drag
                if let Some(ref drag) = self.scrollbar_drag {
                    // Undistort current mouse position to content-space
                    let per_pane_crt = self.current_config().per_pane_crt;
                    let pane_rect = if per_pane_crt {
                        self.renderer.as_ref().and_then(|r| {
                            let (ww, wh) = r.window_size();
                            let rects = self.layout.pane_rects(ww as f32, wh as f32);
                            rects.get(&drag.pane_id).copied()
                        })
                    } else {
                        None
                    };
                    let (_, cy) = self
                        .screen_to_content(position.x, position.y, pane_rect.as_ref())
                        .unwrap_or((position.x, position.y));

                    let delta_y = cy - drag.start_y;
                    let scroll_range = drag.geo.track_height - drag.geo.thumb_height;
                    if scroll_range > 0.0 {
                        let offset_delta =
                            (delta_y as f32 / scroll_range) * drag.geo.history_size as f32;
                        let new_offset = (drag.start_offset as f32 - offset_delta)
                            .clamp(0.0, drag.geo.history_size as f32)
                            as usize;
                        let pane_id = drag.pane_id;
                        if let Some(terminal) = self.terminals.get(&pane_id) {
                            terminal.scroll_to_offset(new_offset);
                            self.last_scroll.insert(pane_id, Instant::now());
                        }
                    }
                    return;
                }

                // Check scrollbar hover proximity
                self.scrollbar_hover_pane = self.check_scrollbar_hover(position.x, position.y);

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
                            // Check if clicking on a scrollbar first
                            // scrollbar_at already undistorts internally for hit testing
                            let (mx, my) = self.mouse_pos;
                            if let Some(geo) = self.scrollbar_at(mx, my) {
                                // Undistort mouse position for thumb hit test and position math
                                let per_pane_crt = self.current_config().per_pane_crt;
                                let pane_rect = if per_pane_crt {
                                    self.renderer.as_ref().and_then(|r| {
                                        let (ww, wh) = r.window_size();
                                        let rects = self.layout.pane_rects(ww as f32, wh as f32);
                                        rects.get(&geo.pane_id).copied()
                                    })
                                } else {
                                    None
                                };
                                let (cx, cy) = self
                                    .screen_to_content(mx, my, pane_rect.as_ref())
                                    .unwrap_or((mx, my));

                                // Focus the pane that owns this scrollbar
                                if geo.pane_id != self.layout.focused_pane() {
                                    self.layout.set_focus(geo.pane_id);
                                }

                                let current_offset = self
                                    .terminals
                                    .get(&geo.pane_id)
                                    .map(|t| t.display_offset())
                                    .unwrap_or(0);

                                if geo.thumb_hit_test(cx, cy) {
                                    // Start dragging the thumb
                                    self.scrollbar_drag = Some(ScrollbarDrag {
                                        pane_id: geo.pane_id,
                                        start_y: cy,
                                        start_offset: current_offset,
                                        geo,
                                    });
                                } else {
                                    // Clicked on track: jump scroll to this position
                                    let target_offset = geo.y_to_offset(cy);
                                    if let Some(terminal) = self.terminals.get(&geo.pane_id) {
                                        terminal.scroll_to_offset(target_offset);
                                        self.last_scroll.insert(geo.pane_id, Instant::now());
                                    }
                                    // Start dragging from the new position
                                    self.scrollbar_drag = Some(ScrollbarDrag {
                                        pane_id: geo.pane_id,
                                        start_y: cy,
                                        start_offset: target_offset,
                                        geo,
                                    });
                                }
                                return;
                            }

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

                                // Record scroll counter so we can adjust
                                // selection if new output pushes content up
                                let focused = self.layout.focused_pane();
                                self.selection_scroll_anchor = self
                                    .terminals
                                    .get(&focused)
                                    .map(|t| t.total_lines_scrolled());

                                self.last_click_time = Some(now);
                                self.last_click_pos = Some(pos);
                            }
                        }
                        ElementState::Released => {
                            if self.scrollbar_drag.is_some() {
                                self.scrollbar_drag = None;
                                return;
                            }
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
                        MouseScrollDelta::LineDelta(_, y) => {
                            // Accumulate fractional line deltas (touchpads often send these)
                            self.scroll_accumulator += y as f64 * 3.0;
                            let lines = self.scroll_accumulator as i32;
                            self.scroll_accumulator -= lines as f64;
                            lines
                        }
                        MouseScrollDelta::PixelDelta(pos) => {
                            // Touchpad pixel mode: accumulate and convert
                            self.scroll_accumulator += pos.y / 20.0;
                            let lines = self.scroll_accumulator as i32;
                            self.scroll_accumulator -= lines as f64;
                            lines
                        }
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

                    if self.paste_dialog.is_some()
                        && self.handle_paste_dialog_input(&event.logical_key)
                    {
                        return;
                    }

                    // Shift+Ctrl+Enter: Add new pane
                    if ctrl && shift && event.logical_key == Key::Named(NamedKey::Enter) {
                        self.add_pane();
                        return;
                    }

                    // Ctrl+Shift+W: Close focused pane
                    if ctrl && shift && event.logical_key == Key::Character("W".into()) {
                        let focused = self.layout.focused_pane();
                        self.close_pane(focused);
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

                    // Ctrl+Shift+D: Copy debug cell dump for current selection
                    // (only while debug grid is on — this is a diagnostic tool).
                    if ctrl && shift && event.logical_key == Key::Character("D".into()) {
                        if self.debug_grid {
                            self.copy_selection_debug();
                        } else {
                            tracing::info!(
                                "Ctrl+Shift+D ignored: enable debug grid (Ctrl+Shift+G) first"
                            );
                        }
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
                                if self.config.behavior.confirm_unsafe_paste
                                    && Self::paste_needs_confirmation(&text)
                                {
                                    let bracketed =
                                        self.terminals.get(&focused).is_some_and(|terminal| {
                                            terminal.term_mode().contains(TermMode::BRACKETED_PASTE)
                                        });
                                    let strip_cr = self.config.behavior.strip_paste_cr;
                                    self.open_paste_dialog(focused, text, bracketed, strip_cr);
                                } else if let Some(terminal) = self.terminals.get(&focused) {
                                    let paste_text = if self.config.behavior.strip_paste_cr {
                                        Self::strip_carriage_returns(&text)
                                    } else {
                                        text
                                    };
                                    terminal.input(paste_text.as_bytes());
                                }
                            }
                        }
                        return;
                    }

                    // Ctrl+Shift+F: Toggle search
                    if ctrl && shift && event.logical_key == Key::Character("F".into()) {
                        if self.search_ui.visible {
                            self.search_ui.hide();
                        } else {
                            self.search_ui.show();
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

                    // Ctrl+Shift+D: Degauss CRT screen
                    if ctrl && shift && event.logical_key == Key::Character("D".into()) {
                        self.degauss_start = Some(Instant::now());
                        return;
                    }

                    // Ctrl+Shift+H: Toggle horizontal sync loss (broken H-HOLD knob)
                    if ctrl && shift && event.logical_key == Key::Character("H".into()) {
                        self.hsync_lost = !self.hsync_lost;
                        return;
                    }

                    // Ctrl+Shift+Arrow: Navigate between panes
                    if ctrl && shift {
                        let direction = match event.logical_key {
                            Key::Named(NamedKey::ArrowLeft) => Some(Direction::Left),
                            Key::Named(NamedKey::ArrowRight) => Some(Direction::Right),
                            Key::Named(NamedKey::ArrowUp) => Some(Direction::Up),
                            Key::Named(NamedKey::ArrowDown) => Some(Direction::Down),
                            _ => None,
                        };
                        if let Some(dir) = direction {
                            if let Some(renderer) = &self.renderer {
                                let (w, h) = renderer.window_size();
                                if let Some(pane) =
                                    self.layout.focus_direction(dir, w as f32, h as f32)
                                {
                                    tracing::info!("Focus changed to pane {:?}", pane);
                                }
                            }
                            return;
                        }
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
                        // Hotkey recording mode: capture the next keypress
                        if self.config_ui.recording_hotkey {
                            match &event.logical_key {
                                Key::Named(NamedKey::Escape) => {
                                    // Cancel recording
                                    self.config_ui.recording_hotkey = false;
                                }
                                Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                                    // Clear the hotkey
                                    self.config_ui.record_hotkey(None);
                                }
                                // Ignore bare modifier keys — wait for actual key
                                Key::Named(
                                    NamedKey::Shift
                                    | NamedKey::Control
                                    | NamedKey::Alt
                                    | NamedKey::Super,
                                ) => {}
                                key => {
                                    if let Some(hotkey_str) =
                                        winit_key_to_hotkey_string(key, self.modifiers)
                                    {
                                        // Validate the hotkey string parses correctly
                                        if hotkey_str
                                            .parse::<global_hotkey::hotkey::HotKey>()
                                            .is_ok()
                                        {
                                            self.config_ui.record_hotkey(Some(hotkey_str));
                                        } else {
                                            tracing::warn!(
                                                "Key combination not supported as global hotkey"
                                            );
                                            self.config_ui.recording_hotkey = false;
                                        }
                                    } else {
                                        self.config_ui.recording_hotkey = false;
                                    }
                                }
                            }
                            return;
                        }

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
                                            let scale_factor = self.current_scale_factor();
                                            if self.apply_font_settings(&new_config, scale_factor) {
                                                self.config = new_config.clone();
                                                self.resize_terminals();
                                            }
                                            // Update global hotkey registration
                                            let old_hotkey =
                                                self.config.behavior.global_hotkey.clone();
                                            let new_hotkey =
                                                new_config.behavior.global_hotkey.clone();
                                            self.config = new_config;
                                            if old_hotkey != new_hotkey {
                                                match &new_hotkey {
                                                    Some(hk) => self.register_global_hotkey(hk),
                                                    None => self.unregister_global_hotkey(),
                                                }
                                            }
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

                    // Handle search UI input when visible
                    if self.search_ui.visible {
                        let focused = self.layout.focused_pane();
                        let action = match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                self.search_ui.hide();
                                // Scroll back to bottom when closing search
                                if let Some(terminal) = self.terminals.get(&focused) {
                                    terminal.scroll_to_bottom();
                                    self.last_scroll.insert(focused, Instant::now());
                                }
                                SearchAction::None
                            }
                            Key::Named(NamedKey::Enter) => {
                                if shift {
                                    self.search_ui.prev_match()
                                } else {
                                    self.search_ui.next_match()
                                }
                            }
                            Key::Named(NamedKey::Backspace) => {
                                if let Some(terminal) = self.terminals.get(&focused) {
                                    self.search_ui.backspace(terminal)
                                } else {
                                    SearchAction::None
                                }
                            }
                            Key::Named(NamedKey::Delete) => {
                                if let Some(terminal) = self.terminals.get(&focused) {
                                    self.search_ui.delete(terminal)
                                } else {
                                    SearchAction::None
                                }
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                self.search_ui.cursor_left();
                                SearchAction::None
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                self.search_ui.cursor_right();
                                SearchAction::None
                            }
                            Key::Named(NamedKey::ArrowUp) => self.search_ui.prev_match(),
                            Key::Named(NamedKey::ArrowDown) => self.search_ui.next_match(),
                            Key::Named(NamedKey::Space) => {
                                if let Some(terminal) = self.terminals.get(&focused) {
                                    self.search_ui.insert_char(' ', terminal)
                                } else {
                                    SearchAction::None
                                }
                            }
                            Key::Character(s) => {
                                // Don't insert if ctrl is held (those are hotkeys)
                                if ctrl || super_key {
                                    SearchAction::None
                                } else if let Some(terminal) = self.terminals.get(&focused) {
                                    let mut action = SearchAction::None;
                                    for c in s.chars() {
                                        action = self.search_ui.insert_char(c, terminal);
                                    }
                                    action
                                } else {
                                    SearchAction::None
                                }
                            }
                            _ => SearchAction::None,
                        };

                        // Handle the search action
                        match action {
                            SearchAction::ScrollToMatch(point) => {
                                if let Some(terminal) = self.terminals.get(&focused) {
                                    let (_, rows) = terminal.size();
                                    let history = terminal.history_size();
                                    // point.line: Line(0) = first screen row at display_offset=0,
                                    // negative = scrollback history.
                                    // With display_offset=D, screen shows Line(-D) at top.
                                    // To center: -D + rows/2 = point.line.0
                                    //   => D = rows/2 - point.line.0
                                    let target_offset =
                                        (rows as i32 / 2 - point.line.0).max(0) as usize;
                                    let clamped = target_offset.min(history);
                                    terminal.scroll_to_offset(clamped);
                                    self.last_scroll.insert(focused, Instant::now());
                                }
                            }
                            SearchAction::UpdateHighlights | SearchAction::None => {}
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
                                Key::Named(named) => {
                                    // xterm modifier encoding: shift=1, alt=2, ctrl=4
                                    // Parameter value = bits + 1
                                    let xterm_mod = 1
                                        + if shift { 1 } else { 0 }
                                        + if alt { 2 } else { 0 }
                                        + if ctrl { 4 } else { 0 };
                                    let has_mods = xterm_mod > 1;

                                    match named {
                                        NamedKey::Enter => {
                                            if alt {
                                                Some(vec![0x1b, b'\r'])
                                            } else {
                                                Some(vec![b'\r'])
                                            }
                                        }
                                        NamedKey::Backspace => Some(vec![0x7f]),
                                        NamedKey::Tab => {
                                            if shift {
                                                Some(b"\x1b[Z".to_vec()) // Backtab
                                            } else {
                                                Some(vec![b'\t'])
                                            }
                                        }
                                        NamedKey::Escape => Some(vec![0x1b]),
                                        NamedKey::Space => {
                                            if alt {
                                                Some(vec![0x1b, b' '])
                                            } else {
                                                Some(vec![b' '])
                                            }
                                        }
                                        // Cursor keys: SS3 format when APP_CURSOR, CSI with modifiers
                                        NamedKey::ArrowUp => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}A", xterm_mod).into_bytes())
                                            } else if app_cursor {
                                                Some(b"\x1bOA".to_vec())
                                            } else {
                                                Some(b"\x1b[A".to_vec())
                                            }
                                        }
                                        NamedKey::ArrowDown => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}B", xterm_mod).into_bytes())
                                            } else if app_cursor {
                                                Some(b"\x1bOB".to_vec())
                                            } else {
                                                Some(b"\x1b[B".to_vec())
                                            }
                                        }
                                        NamedKey::ArrowRight => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}C", xterm_mod).into_bytes())
                                            } else if app_cursor {
                                                Some(b"\x1bOC".to_vec())
                                            } else {
                                                Some(b"\x1b[C".to_vec())
                                            }
                                        }
                                        NamedKey::ArrowLeft => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}D", xterm_mod).into_bytes())
                                            } else if app_cursor {
                                                Some(b"\x1bOD".to_vec())
                                            } else {
                                                Some(b"\x1b[D".to_vec())
                                            }
                                        }
                                        NamedKey::Home => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}H", xterm_mod).into_bytes())
                                            } else if app_cursor {
                                                Some(b"\x1bOH".to_vec())
                                            } else {
                                                Some(b"\x1b[H".to_vec())
                                            }
                                        }
                                        NamedKey::End => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}F", xterm_mod).into_bytes())
                                            } else if app_cursor {
                                                Some(b"\x1bOF".to_vec())
                                            } else {
                                                Some(b"\x1b[F".to_vec())
                                            }
                                        }
                                        // Tilde-style keys: CSI num ~ or CSI num ; mod ~
                                        NamedKey::Insert => {
                                            if has_mods {
                                                Some(format!("\x1b[2;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[2~".to_vec())
                                            }
                                        }
                                        NamedKey::Delete => {
                                            if has_mods {
                                                Some(format!("\x1b[3;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[3~".to_vec())
                                            }
                                        }
                                        NamedKey::PageUp => {
                                            if has_mods {
                                                Some(format!("\x1b[5;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[5~".to_vec())
                                            }
                                        }
                                        NamedKey::PageDown => {
                                            if has_mods {
                                                Some(format!("\x1b[6;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[6~".to_vec())
                                            }
                                        }
                                        // Function keys F1-F4: SS3 format or CSI 1 ; mod X
                                        NamedKey::F1 => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}P", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1bOP".to_vec())
                                            }
                                        }
                                        NamedKey::F2 => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}Q", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1bOQ".to_vec())
                                            }
                                        }
                                        NamedKey::F3 => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}R", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1bOR".to_vec())
                                            }
                                        }
                                        NamedKey::F4 => {
                                            if has_mods {
                                                Some(format!("\x1b[1;{}S", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1bOS".to_vec())
                                            }
                                        }
                                        // Function keys F5-F12: CSI num ~ format
                                        NamedKey::F5 => {
                                            if has_mods {
                                                Some(format!("\x1b[15;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[15~".to_vec())
                                            }
                                        }
                                        NamedKey::F6 => {
                                            if has_mods {
                                                Some(format!("\x1b[17;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[17~".to_vec())
                                            }
                                        }
                                        NamedKey::F7 => {
                                            if has_mods {
                                                Some(format!("\x1b[18;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[18~".to_vec())
                                            }
                                        }
                                        NamedKey::F8 => {
                                            if has_mods {
                                                Some(format!("\x1b[19;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[19~".to_vec())
                                            }
                                        }
                                        NamedKey::F9 => {
                                            if has_mods {
                                                Some(format!("\x1b[20;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[20~".to_vec())
                                            }
                                        }
                                        NamedKey::F10 => {
                                            if has_mods {
                                                Some(format!("\x1b[21;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[21~".to_vec())
                                            }
                                        }
                                        NamedKey::F11 => {
                                            if has_mods {
                                                Some(format!("\x1b[23;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[23~".to_vec())
                                            }
                                        }
                                        NamedKey::F12 => {
                                            if has_mods {
                                                Some(format!("\x1b[24;{}~", xterm_mod).into_bytes())
                                            } else {
                                                Some(b"\x1b[24~".to_vec())
                                            }
                                        }
                                        _ => None,
                                    }
                                }
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

/// Convert a winit Key + ModifiersState into a global-hotkey compatible string.
/// Returns None for keys that can't be represented.
fn winit_key_to_hotkey_string(key: &Key, modifiers: ModifiersState) -> Option<String> {
    let key_part = match key {
        Key::Character(s) => {
            let c = s.to_uppercase();
            match c.as_str() {
                "A" => "KeyA",
                "B" => "KeyB",
                "C" => "KeyC",
                "D" => "KeyD",
                "E" => "KeyE",
                "F" => "KeyF",
                "G" => "KeyG",
                "H" => "KeyH",
                "I" => "KeyI",
                "J" => "KeyJ",
                "K" => "KeyK",
                "L" => "KeyL",
                "M" => "KeyM",
                "N" => "KeyN",
                "O" => "KeyO",
                "P" => "KeyP",
                "Q" => "KeyQ",
                "R" => "KeyR",
                "S" => "KeyS",
                "T" => "KeyT",
                "U" => "KeyU",
                "V" => "KeyV",
                "W" => "KeyW",
                "X" => "KeyX",
                "Y" => "KeyY",
                "Z" => "KeyZ",
                "0" => "Digit0",
                "1" => "Digit1",
                "2" => "Digit2",
                "3" => "Digit3",
                "4" => "Digit4",
                "5" => "Digit5",
                "6" => "Digit6",
                "7" => "Digit7",
                "8" => "Digit8",
                "9" => "Digit9",
                "`" => "Backquote",
                "-" => "Minus",
                "=" => "Equal",
                "[" => "BracketLeft",
                "]" => "BracketRight",
                "\\" => "Backslash",
                ";" => "Semicolon",
                "'" => "Quote",
                "," => "Comma",
                "." => "Period",
                "/" => "Slash",
                _ => return None,
            }
        }
        Key::Named(named) => match named {
            NamedKey::F1 => "F1",
            NamedKey::F2 => "F2",
            NamedKey::F3 => "F3",
            NamedKey::F4 => "F4",
            NamedKey::F5 => "F5",
            NamedKey::F6 => "F6",
            NamedKey::F7 => "F7",
            NamedKey::F8 => "F8",
            NamedKey::F9 => "F9",
            NamedKey::F10 => "F10",
            NamedKey::F11 => "F11",
            NamedKey::F12 => "F12",
            NamedKey::F13 => "F13",
            NamedKey::F14 => "F14",
            NamedKey::F15 => "F15",
            NamedKey::F16 => "F16",
            NamedKey::F17 => "F17",
            NamedKey::F18 => "F18",
            NamedKey::F19 => "F19",
            NamedKey::F20 => "F20",
            NamedKey::Space => "Space",
            NamedKey::Enter => "Enter",
            NamedKey::Tab => "Tab",
            NamedKey::Backspace => "Backspace",
            NamedKey::Delete => "Delete",
            NamedKey::Insert => "Insert",
            NamedKey::Home => "Home",
            NamedKey::End => "End",
            NamedKey::PageUp => "PageUp",
            NamedKey::PageDown => "PageDown",
            NamedKey::ArrowUp => "ArrowUp",
            NamedKey::ArrowDown => "ArrowDown",
            NamedKey::ArrowLeft => "ArrowLeft",
            NamedKey::ArrowRight => "ArrowRight",
            NamedKey::CapsLock => "CapsLock",
            NamedKey::ScrollLock => "ScrollLock",
            NamedKey::NumLock => "NumLock",
            NamedKey::PrintScreen => "PrintScreen",
            NamedKey::Pause => "Pause",
            _ => return None,
        },
        _ => return None,
    };

    let mut parts = Vec::new();
    if modifiers.control_key() {
        parts.push("Ctrl");
    }
    if modifiers.alt_key() {
        parts.push("Alt");
    }
    if modifiers.shift_key() {
        parts.push("Shift");
    }
    if modifiers.super_key() {
        parts.push("Super");
    }
    parts.push(key_part);

    Some(parts.join("+"))
}

fn main() -> Result<()> {
    // Force 1:1 pixel scaling on X11 (winit guesses wrong sometimes)
    // TODO: Make this configurable for high-DPI displays
    std::env::set_var("WINIT_X11_SCALE_FACTOR", "1");

    #[cfg(feature = "tracy")]
    {
        use tracing_subscriber::layer::SubscriberExt;
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(tracing_tracy::TracyLayer::default()),
        )
        .expect("Failed to set Tracy subscriber");
    }
    #[cfg(not(feature = "tracy"))]
    tracing_subscriber::fmt::init();

    tracing::info!("Starting cool-rust-term");

    let event_loop = EventLoop::new()?;
    let mut app = App::new();

    event_loop.run_app(&mut app)?;

    Ok(())
}
