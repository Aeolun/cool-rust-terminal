// ABOUTME: Search bar overlay for scrollback history search.
// ABOUTME: Handles search state, input, match navigation, and rendering.

use std::ops::RangeInclusive;

use crt_core::ColorScheme;
use crt_renderer::RenderCell;
use crt_terminal::{Column, Line, Point, RegexSearch, Terminal};

/// A single search match, stored as inclusive start/end points.
#[derive(Clone, Debug)]
pub struct SearchMatch {
    pub start: Point,
    pub end: Point,
}

impl SearchMatch {
    fn from_range(range: &RangeInclusive<Point>) -> Self {
        Self {
            start: *range.start(),
            end: *range.end(),
        }
    }

    /// Check if a grid position (alacritty Line + Column) falls within this match.
    pub fn contains(&self, line: Line, col: Column) -> bool {
        let point = Point::new(line, col);
        point >= self.start && point <= self.end
    }
}

/// What should happen after a search input event.
pub enum SearchAction {
    /// No action needed
    None,
    /// Matches changed, re-render needed
    UpdateHighlights,
    /// Scroll the terminal to show the current match
    ScrollToMatch(Point),
}

/// Search bar state
pub struct SearchUI {
    pub visible: bool,
    /// The search query text
    query: String,
    /// Text cursor position within the query
    cursor_pos: usize,
    /// All matches in the scrollback
    pub matches: Vec<SearchMatch>,
    /// Index of the "current" match (highlighted brighter)
    pub current_match: usize,
    /// Compiled regex (None if query is empty or invalid)
    regex: Option<RegexSearch>,
    /// Whether the regex failed to compile
    pub error: bool,
}

impl SearchUI {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            cursor_pos: 0,
            matches: Vec::new(),
            current_match: 0,
            regex: None,
            error: false,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.query.clear();
        self.cursor_pos = 0;
        self.matches.clear();
        self.current_match = 0;
        self.regex = None;
        self.error = false;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.query.clear();
        self.matches.clear();
        self.regex = None;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Handle a character input. Returns the appropriate action.
    pub fn insert_char(&mut self, c: char, terminal: &Terminal) -> SearchAction {
        self.query.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.perform_search(terminal)
    }

    /// Handle backspace. Returns the appropriate action.
    pub fn backspace(&mut self, terminal: &Terminal) -> SearchAction {
        if self.cursor_pos > 0 {
            // Find the previous char boundary
            let prev = self.query[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
            self.perform_search(terminal)
        } else {
            SearchAction::None
        }
    }

    /// Handle delete key
    pub fn delete(&mut self, terminal: &Terminal) -> SearchAction {
        if self.cursor_pos < self.query.len() {
            let next = self.query[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.query.len());
            self.query.drain(self.cursor_pos..next);
            self.perform_search(terminal)
        } else {
            SearchAction::None
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.query[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.query.len() {
            self.cursor_pos = self.query[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.query.len());
        }
    }

    /// Navigate to the next match
    pub fn next_match(&mut self) -> SearchAction {
        if self.matches.is_empty() {
            return SearchAction::None;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();
        let point = self.matches[self.current_match].start;
        SearchAction::ScrollToMatch(point)
    }

    /// Navigate to the previous match
    pub fn prev_match(&mut self) -> SearchAction {
        if self.matches.is_empty() {
            return SearchAction::None;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len() - 1;
        } else {
            self.current_match -= 1;
        }
        let point = self.matches[self.current_match].start;
        SearchAction::ScrollToMatch(point)
    }

    /// Escape special regex characters so we do literal search by default.
    fn escape_regex(s: &str) -> String {
        let mut escaped = String::with_capacity(s.len() * 2);
        for c in s.chars() {
            match c {
                '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^'
                | '$' => {
                    escaped.push('\\');
                    escaped.push(c);
                }
                _ => escaped.push(c),
            }
        }
        escaped
    }

    /// Perform the search across the entire terminal scrollback.
    fn perform_search(&mut self, terminal: &Terminal) -> SearchAction {
        self.matches.clear();
        self.current_match = 0;
        self.error = false;

        if self.query.is_empty() {
            self.regex = None;
            return SearchAction::UpdateHighlights;
        }

        let escaped = Self::escape_regex(&self.query);
        let regex = match RegexSearch::new(&escaped) {
            Ok(r) => r,
            Err(_) => {
                self.error = true;
                self.regex = None;
                return SearchAction::UpdateHighlights;
            }
        };
        self.regex = Some(regex);

        // Collect all matches
        let regex = self.regex.as_mut().unwrap();
        let raw_matches = terminal.search_all(regex);
        self.matches = raw_matches.iter().map(SearchMatch::from_range).collect();

        // Find the match closest to the current viewport (bottom of screen)
        if !self.matches.is_empty() {
            let display_offset = terminal.display_offset() as i32;
            // The visible screen shows lines from -display_offset to screen_lines - display_offset - 1
            // Find the first match that's on screen or below the screen
            let screen_top = Line(-display_offset);
            self.current_match = self
                .matches
                .iter()
                .position(|m| m.start.line >= screen_top)
                .unwrap_or(self.matches.len() - 1);
        }

        if self.matches.is_empty() {
            SearchAction::UpdateHighlights
        } else {
            let point = self.matches[self.current_match].start;
            SearchAction::ScrollToMatch(point)
        }
    }

    /// Check if a grid cell is part of any search match.
    /// `line` is the alacritty Line coordinate (negative for scrollback).
    pub fn is_match(&self, line: Line, col: Column) -> bool {
        // Binary search: matches are sorted by start position
        self.matches
            .binary_search_by(|m| {
                if line > m.end.line || (line == m.end.line && col > m.end.column) {
                    std::cmp::Ordering::Less
                } else if line < m.start.line || (line == m.start.line && col < m.start.column) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    /// Check if a grid cell is part of the current (focused) match.
    pub fn is_current_match(&self, line: Line, col: Column) -> bool {
        if self.matches.is_empty() {
            return false;
        }
        self.matches[self.current_match].contains(line, col)
    }

    /// Overlay the search bar onto the bottom row(s) of the cell grid.
    pub fn overlay_search_bar(
        cells: &mut [Vec<RenderCell>],
        search: &SearchUI,
        scheme: &ColorScheme,
    ) {
        let height = cells.len();
        if height < 2 {
            return;
        }
        let width = if cells[0].is_empty() {
            return;
        } else {
            cells[0].len()
        };

        let bar_row = height - 1;

        let fg = scheme.foreground;
        let border = scheme.colors[6]; // cyan
        let bg = [0.0, 0.0, 0.0, 0.85];
        let error_fg = scheme.colors[1]; // red
        let match_fg = scheme.colors[2]; // green
        let query_fg = scheme.colors[15]; // bright white

        // Build the search bar content
        let prefix = "SEARCH: ";
        let query = search.query();

        // Match count suffix
        let count_str = if search.error {
            " [err]".to_string()
        } else if query.is_empty() {
            String::new()
        } else {
            format!(
                " {}/{}",
                if search.matches.is_empty() {
                    0
                } else {
                    search.current_match + 1
                },
                search.match_count()
            )
        };

        // Fill the entire row with background
        for cell in cells[bar_row].iter_mut().take(width) {
            *cell = RenderCell {
                c: ' ',
                fg,
                bg,
                is_wide: false,
            };
        }

        let mut x = 0;

        // Left border
        if x < width {
            cells[bar_row][x] = RenderCell {
                c: '╶',
                fg: border,
                bg,
                is_wide: false,
            };
            x += 1;
        }
        if x < width {
            cells[bar_row][x] = RenderCell {
                c: '─',
                fg: border,
                bg,
                is_wide: false,
            };
            x += 1;
        }
        if x < width {
            cells[bar_row][x] = RenderCell {
                c: ' ',
                fg: border,
                bg,
                is_wide: false,
            };
            x += 1;
        }

        // "SEARCH: "
        for c in prefix.chars() {
            if x >= width {
                break;
            }
            cells[bar_row][x] = RenderCell {
                c,
                fg: border,
                bg,
                is_wide: false,
            };
            x += 1;
        }

        // Query text with cursor
        let cursor_byte = search.cursor_pos;
        let mut byte_pos = 0;
        for c in query.chars() {
            if x >= width {
                break;
            }
            let is_cursor = byte_pos == cursor_byte;
            let (char_fg, char_bg) = if is_cursor {
                (bg, query_fg) // Inverted at cursor
            } else {
                (query_fg, bg)
            };
            cells[bar_row][x] = RenderCell {
                c,
                fg: char_fg,
                bg: char_bg,
                is_wide: false,
            };
            x += 1;
            byte_pos += c.len_utf8();
        }

        // Cursor at end of query (block cursor)
        if byte_pos == cursor_byte && x < width {
            cells[bar_row][x] = RenderCell {
                c: ' ',
                fg: bg,
                bg: query_fg,
                is_wide: false,
            };
            x += 1;
        }

        // Fill with border until count
        let count_start = width.saturating_sub(count_str.len() + 3); // 3 for " ─╴"
        while x < count_start {
            if x < width {
                cells[bar_row][x] = RenderCell {
                    c: ' ',
                    fg: border,
                    bg,
                    is_wide: false,
                };
            }
            x += 1;
        }

        // Match count
        let count_fg = if search.error || (search.matches.is_empty() && !query.is_empty()) {
            error_fg
        } else {
            match_fg
        };
        for c in count_str.chars() {
            if x >= width {
                break;
            }
            cells[bar_row][x] = RenderCell {
                c,
                fg: count_fg,
                bg,
                is_wide: false,
            };
            x += 1;
        }

        // Right border
        if x < width {
            cells[bar_row][x] = RenderCell {
                c: ' ',
                fg: border,
                bg,
                is_wide: false,
            };
            x += 1;
        }
        if x < width {
            cells[bar_row][x] = RenderCell {
                c: '─',
                fg: border,
                bg,
                is_wide: false,
            };
            x += 1;
        }
        if x < width {
            cells[bar_row][x] = RenderCell {
                c: '╴',
                fg: border,
                bg,
                is_wide: false,
            };
        }
    }
}
