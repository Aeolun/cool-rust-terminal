// ABOUTME: Configuration UI overlay for adjusting CRT effect settings.
// ABOUTME: Renders a text-based settings panel with keyboard navigation.
// ABOUTME: Uses tabs to organize settings into Effects and Appearance categories.

use crt_core::{BdfFont, ColorScheme, Config, ScanlineMode};
use crt_renderer::RenderCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTab {
    Effects,
    Appearance,
    Behavior,
}

impl ConfigTab {
    fn all() -> &'static [ConfigTab] {
        &[
            ConfigTab::Effects,
            ConfigTab::Appearance,
            ConfigTab::Behavior,
        ]
    }

    fn label(&self) -> &'static str {
        match self {
            ConfigTab::Effects => "Effects",
            ConfigTab::Appearance => "Appearance",
            ConfigTab::Behavior => "Behavior",
        }
    }

    fn index(&self) -> usize {
        match self {
            ConfigTab::Effects => 0,
            ConfigTab::Appearance => 1,
            ConfigTab::Behavior => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    // Effects tab
    Curvature,
    Scanlines,
    ScanlineMode,
    Bloom,
    BurnIn,
    StaticNoise,
    Flicker,
    Vignette,
    Brightness,
    PerPaneCrt,
    FocusGlowRadius,
    FocusGlowWidth,
    FocusGlowIntensity,
    // Bezel settings
    BezelEnabled,
    ContentScaleX,
    ContentScaleY,
    // Beam simulation (requires 240Hz+)
    BeamSimulation,
    Interlace,
    // Appearance tab
    FontType,      // Toggle between TTF and BDF
    FontFamily,    // TTF font selector (hidden when BDF selected)
    FontSize,      // TTF font size (hidden when BDF selected)
    BdfFontFamily, // BDF font selector (hidden when TTF selected)
    ColorSchemeField,
    // Behavior tab
    AutoCopySelection,
    ShowStartupHint,
    // Common
    Save,
    Cancel,
}

impl ConfigField {
    fn all() -> &'static [ConfigField] {
        &[
            // Effects tab
            ConfigField::Curvature,
            ConfigField::Scanlines,
            ConfigField::ScanlineMode,
            ConfigField::Bloom,
            ConfigField::BurnIn,
            ConfigField::StaticNoise,
            ConfigField::Flicker,
            ConfigField::Vignette,
            ConfigField::Brightness,
            ConfigField::FocusGlowRadius,
            ConfigField::FocusGlowWidth,
            ConfigField::FocusGlowIntensity,
            ConfigField::PerPaneCrt,
            ConfigField::BezelEnabled,
            ConfigField::ContentScaleX,
            ConfigField::ContentScaleY,
            ConfigField::BeamSimulation,
            ConfigField::Interlace,
            // Appearance tab
            ConfigField::FontType,
            ConfigField::FontFamily,
            ConfigField::FontSize,
            ConfigField::BdfFontFamily,
            ConfigField::ColorSchemeField,
            // Behavior tab
            ConfigField::AutoCopySelection,
            ConfigField::ShowStartupHint,
            // Common
            ConfigField::Save,
            ConfigField::Cancel,
        ]
    }

    /// Returns true if a blank line should be rendered before this field
    fn has_separator_before(&self) -> bool {
        matches!(
            self,
            ConfigField::PerPaneCrt | ConfigField::BezelEnabled | ConfigField::BeamSimulation
        )
    }

    fn label(&self) -> &'static str {
        match self {
            ConfigField::Curvature => "Curvature",
            ConfigField::Scanlines => "Scanlines",
            ConfigField::ScanlineMode => "Scanline Type",
            ConfigField::Bloom => "Bloom",
            ConfigField::BurnIn => "Burn-in",
            ConfigField::StaticNoise => "Static",
            ConfigField::Flicker => "Flicker",
            ConfigField::Vignette => "Vignette",
            ConfigField::Brightness => "Brightness",
            ConfigField::PerPaneCrt => "Per-pane CRT",
            ConfigField::FocusGlowRadius => "Glow Radius",
            ConfigField::FocusGlowWidth => "Glow Width",
            ConfigField::FocusGlowIntensity => "Glow Bright",
            ConfigField::BezelEnabled => "Bezel",
            ConfigField::ContentScaleX => "H-Size",
            ConfigField::ContentScaleY => "V-Size",
            ConfigField::BeamSimulation => "Beam Sim",
            ConfigField::Interlace => "Interlace",
            ConfigField::FontType => "Font Type",
            ConfigField::FontFamily => "TTF Font",
            ConfigField::FontSize => "Font Size",
            ConfigField::BdfFontFamily => "BDF Font",
            ConfigField::ColorSchemeField => "Colors",
            ConfigField::AutoCopySelection => "Auto-copy",
            ConfigField::ShowStartupHint => "Startup hint",
            ConfigField::Save => "[ Save ]",
            ConfigField::Cancel => "[ Cancel ]",
        }
    }

    fn is_slider(&self) -> bool {
        matches!(
            self,
            ConfigField::Curvature
                | ConfigField::Scanlines
                | ConfigField::Bloom
                | ConfigField::BurnIn
                | ConfigField::StaticNoise
                | ConfigField::Flicker
                | ConfigField::Vignette
                | ConfigField::Brightness
                | ConfigField::FocusGlowRadius
                | ConfigField::FocusGlowWidth
                | ConfigField::FocusGlowIntensity
                | ConfigField::ContentScaleX
                | ConfigField::ContentScaleY
                | ConfigField::FontSize
        )
    }

    fn is_toggle(&self) -> bool {
        matches!(
            self,
            ConfigField::PerPaneCrt
                | ConfigField::BezelEnabled
                | ConfigField::AutoCopySelection
                | ConfigField::ShowStartupHint
                | ConfigField::FontType
                | ConfigField::ScanlineMode
                | ConfigField::BeamSimulation
                | ConfigField::Interlace
        )
    }

    fn is_selector(&self) -> bool {
        matches!(
            self,
            ConfigField::FontFamily | ConfigField::BdfFontFamily | ConfigField::ColorSchemeField
        )
    }

    fn is_button(&self) -> bool {
        matches!(self, ConfigField::Save | ConfigField::Cancel)
    }

    fn tab(&self) -> Option<ConfigTab> {
        match self {
            // Effects tab
            ConfigField::Curvature
            | ConfigField::Scanlines
            | ConfigField::ScanlineMode
            | ConfigField::Bloom
            | ConfigField::BurnIn
            | ConfigField::StaticNoise
            | ConfigField::Flicker
            | ConfigField::Vignette
            | ConfigField::Brightness
            | ConfigField::FocusGlowRadius
            | ConfigField::FocusGlowWidth
            | ConfigField::FocusGlowIntensity
            | ConfigField::PerPaneCrt
            | ConfigField::BezelEnabled
            | ConfigField::ContentScaleX
            | ConfigField::ContentScaleY
            | ConfigField::BeamSimulation
            | ConfigField::Interlace => Some(ConfigTab::Effects),
            // Appearance tab
            ConfigField::FontType
            | ConfigField::FontFamily
            | ConfigField::FontSize
            | ConfigField::BdfFontFamily
            | ConfigField::ColorSchemeField => Some(ConfigTab::Appearance),
            // Behavior tab
            ConfigField::AutoCopySelection | ConfigField::ShowStartupHint => {
                Some(ConfigTab::Behavior)
            }
            // Save/Cancel are on all tabs
            ConfigField::Save | ConfigField::Cancel => None,
        }
    }

    fn fields_for_tab(tab: ConfigTab, config: &Config) -> Vec<ConfigField> {
        let mut fields: Vec<ConfigField> = ConfigField::all()
            .iter()
            .filter(|f| f.tab() == Some(tab) && f.should_show(config))
            .copied()
            .collect();
        // Always add Save/Cancel at the end
        fields.push(ConfigField::Save);
        fields.push(ConfigField::Cancel);
        fields
    }

    /// Returns true if this field should be shown given the current config state
    fn should_show(&self, config: &Config) -> bool {
        match self {
            // TTF-specific fields: only show when BDF is not selected
            ConfigField::FontFamily | ConfigField::FontSize => config.bdf_font.is_none(),
            // BDF-specific fields: only show when BDF is selected
            ConfigField::BdfFontFamily => config.bdf_font.is_some(),
            // Interlace only shows when beam simulation is enabled
            ConfigField::Interlace => config.effects.beam_simulation_enabled,
            // All other fields always show
            _ => true,
        }
    }
}

pub struct ConfigUI {
    pub visible: bool,
    pub selected: usize,
    pub current_tab: ConfigTab,
    pub config: Config,
    original_config: Config,
}

impl ConfigUI {
    pub fn new(config: Config) -> Self {
        Self {
            visible: false,
            selected: 0,
            current_tab: ConfigTab::Effects,
            config: config.clone(),
            original_config: config,
        }
    }

    /// Get the foreground color from the current color scheme
    fn fg_color(&self) -> [f32; 4] {
        self.config.color_scheme.foreground
    }

    /// Get a color for borders/decorations - uses cyan (color 6) to show scheme variety
    fn border_color(&self) -> [f32; 4] {
        self.config.color_scheme.colors[6] // Cyan - shows color difference between schemes
    }

    /// Get a bright version of the foreground color
    fn bright_color(&self) -> [f32; 4] {
        // Use "bright white" from the scheme (color 15), or brighten the foreground
        self.config.color_scheme.colors[15]
    }

    /// Get a dim version of the foreground color
    fn dim_color(&self) -> [f32; 4] {
        let fg = self.config.color_scheme.foreground;
        [fg[0] * 0.6, fg[1] * 0.6, fg[2] * 0.6, fg[3]]
    }

    /// Get the background color (transparent - let CRT show through)
    fn bg_color(&self) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    /// Get a slightly lighter background for selection highlight
    fn highlight_bg(&self) -> [f32; 4] {
        let fg = self.config.color_scheme.foreground;
        // Mix a bit of foreground into background for highlight
        [fg[0] * 0.15, fg[1] * 0.15, fg[2] * 0.15, 1.0]
    }

    pub fn show(&mut self, config: &Config) {
        self.config = config.clone();
        self.original_config = config.clone();
        self.visible = true;
        self.selected = 0;
        self.current_tab = ConfigTab::Effects;
    }

    pub fn next_tab(&mut self) {
        let tabs = ConfigTab::all();
        let current_idx = self.current_tab.index();
        let next_idx = (current_idx + 1) % tabs.len();
        self.current_tab = tabs[next_idx];
        self.selected = 0; // Reset selection when switching tabs
    }

    pub fn prev_tab(&mut self) {
        let tabs = ConfigTab::all();
        let current_idx = self.current_tab.index();
        let prev_idx = if current_idx == 0 {
            tabs.len() - 1
        } else {
            current_idx - 1
        };
        self.current_tab = tabs[prev_idx];
        self.selected = 0; // Reset selection when switching tabs
    }

    fn current_fields(&self) -> Vec<ConfigField> {
        ConfigField::fields_for_tab(self.current_tab, &self.config)
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn cancel(&mut self) -> Config {
        self.visible = false;
        self.original_config.clone()
    }

    pub fn save(&mut self) -> Config {
        self.visible = false;
        self.config.clone()
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let fields = self.current_fields();
        let max = fields.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    pub fn adjust_left(&mut self) {
        let fields = self.current_fields();
        if self.selected < fields.len() {
            self.adjust_field(fields[self.selected], -0.05);
        }
    }

    pub fn adjust_right(&mut self) {
        let fields = self.current_fields();
        if self.selected < fields.len() {
            self.adjust_field(fields[self.selected], 0.05);
        }
    }

    pub fn toggle_or_activate(&mut self) -> Option<ConfigAction> {
        let fields = self.current_fields();
        if self.selected >= fields.len() {
            return None;
        }
        let field = fields[self.selected];
        match field {
            ConfigField::PerPaneCrt => {
                self.config.per_pane_crt = !self.config.per_pane_crt;
                None
            }
            ConfigField::BezelEnabled => {
                self.config.effects.bezel_enabled = !self.config.effects.bezel_enabled;
                None
            }
            ConfigField::AutoCopySelection => {
                self.config.behavior.auto_copy_selection =
                    !self.config.behavior.auto_copy_selection;
                None
            }
            ConfigField::ShowStartupHint => {
                self.config.behavior.show_startup_hint = !self.config.behavior.show_startup_hint;
                None
            }
            ConfigField::FontType => {
                // Toggle between TTF and BDF
                if self.config.bdf_font.is_some() {
                    self.config.bdf_font = None;
                } else {
                    // Default to Fixed 9x18 when enabling BDF
                    self.config.bdf_font = Some(BdfFont::Fixed9x18);
                }
                None
            }
            ConfigField::ScanlineMode => {
                // Toggle between Row-based and Pixel scanlines
                self.config.effects.scanline_mode = match self.config.effects.scanline_mode {
                    ScanlineMode::RowBased => ScanlineMode::Pixel,
                    ScanlineMode::Pixel => ScanlineMode::RowBased,
                };
                None
            }
            ConfigField::BeamSimulation => {
                self.config.effects.beam_simulation_enabled =
                    !self.config.effects.beam_simulation_enabled;
                None
            }
            ConfigField::Interlace => {
                self.config.effects.interlace_enabled = !self.config.effects.interlace_enabled;
                None
            }
            ConfigField::Save => Some(ConfigAction::Save),
            ConfigField::Cancel => Some(ConfigAction::Cancel),
            _ => None,
        }
    }

    fn adjust_field(&mut self, field: ConfigField, delta: f32) {
        let effects = &mut self.config.effects;
        match field {
            ConfigField::Curvature => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.screen_curvature = (effects.screen_curvature + change).clamp(0.0, 0.5);
            }
            ConfigField::Scanlines => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.scanline_intensity = (effects.scanline_intensity + change).clamp(0.0, 1.0);
            }
            ConfigField::ScanlineMode => {
                // Toggle between Row-based and Pixel scanlines via left/right
                effects.scanline_mode = match effects.scanline_mode {
                    ScanlineMode::RowBased => ScanlineMode::Pixel,
                    ScanlineMode::Pixel => ScanlineMode::RowBased,
                };
            }
            ConfigField::Bloom => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.bloom = (effects.bloom + change).clamp(0.0, 1.0);
            }
            ConfigField::BurnIn => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.burn_in = (effects.burn_in + change).clamp(0.0, 1.0);
            }
            ConfigField::StaticNoise => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.static_noise = (effects.static_noise + change).clamp(0.0, 0.5);
            }
            ConfigField::Flicker => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.flicker = (effects.flicker + change).clamp(0.0, 0.5);
            }
            ConfigField::Vignette => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.vignette = (effects.vignette + change).clamp(0.0, 1.0);
            }
            ConfigField::Brightness => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.brightness = (effects.brightness + change).clamp(0.1, 2.0);
            }
            ConfigField::PerPaneCrt => {
                self.config.per_pane_crt = delta > 0.0;
            }
            ConfigField::FocusGlowRadius => {
                // Finer increments (0.0025) when at/below 0.02, coarser (0.01) above
                let increment = if effects.focus_glow_radius <= 0.02 {
                    0.0025
                } else {
                    0.01
                };
                let change = if delta > 0.0 { increment } else { -increment };
                effects.focus_glow_radius = (effects.focus_glow_radius + change).clamp(0.0, 0.3);
            }
            ConfigField::FocusGlowWidth => {
                // Finer increments (0.0025) when at/below 0.02, coarser (0.01) above
                let increment = if effects.focus_glow_width <= 0.02 {
                    0.0025
                } else {
                    0.01
                };
                let change = if delta > 0.0 { increment } else { -increment };
                effects.focus_glow_width = (effects.focus_glow_width + change).clamp(0.001, 0.3);
            }
            ConfigField::FocusGlowIntensity => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.focus_glow_intensity =
                    (effects.focus_glow_intensity + change).clamp(0.0, 1.0);
            }
            ConfigField::FontType => {
                // Toggle between TTF and BDF via left/right arrows
                if self.config.bdf_font.is_some() {
                    self.config.bdf_font = None;
                } else {
                    self.config.bdf_font = Some(BdfFont::Fixed9x18);
                }
            }
            ConfigField::FontFamily => {
                if delta > 0.0 {
                    self.config.font = self.config.font.next();
                } else {
                    self.config.font = self.config.font.prev();
                }
            }
            ConfigField::FontSize => {
                let change = if delta > 0.0 { 1.0 } else { -1.0 };
                self.config.font_size = (self.config.font_size + change).clamp(8.0, 32.0);
            }
            ConfigField::BdfFontFamily => {
                if let Some(ref mut bdf) = self.config.bdf_font {
                    if delta > 0.0 {
                        *bdf = bdf.next();
                    } else {
                        *bdf = bdf.prev();
                    }
                }
            }
            ConfigField::ColorSchemeField => {
                let presets = ColorScheme::presets();
                let current_name = &self.config.color_scheme.name;
                let current_idx = presets
                    .iter()
                    .position(|s| &s.name == current_name)
                    .unwrap_or(0);
                let new_idx = if delta > 0.0 {
                    (current_idx + 1) % presets.len()
                } else if current_idx == 0 {
                    presets.len() - 1
                } else {
                    current_idx - 1
                };
                self.config.color_scheme = presets[new_idx].clone();
            }
            ConfigField::BezelEnabled => {
                self.config.effects.bezel_enabled = delta > 0.0;
            }
            ConfigField::AutoCopySelection => {
                self.config.behavior.auto_copy_selection = delta > 0.0;
            }
            ConfigField::ShowStartupHint => {
                self.config.behavior.show_startup_hint = delta > 0.0;
            }
            ConfigField::ContentScaleX => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.content_scale_x = (effects.content_scale_x + change).clamp(0.8, 1.2);
            }
            ConfigField::ContentScaleY => {
                let change = if delta > 0.0 { 0.01 } else { -0.01 };
                effects.content_scale_y = (effects.content_scale_y + change).clamp(0.8, 1.2);
            }
            ConfigField::BeamSimulation => {
                effects.beam_simulation_enabled = delta > 0.0;
            }
            ConfigField::Interlace => {
                effects.interlace_enabled = delta > 0.0;
            }
            _ => {}
        }
    }

    fn get_field_value(&self, field: ConfigField) -> f32 {
        match field {
            ConfigField::Curvature => self.config.effects.screen_curvature / 0.5,
            ConfigField::Scanlines => self.config.effects.scanline_intensity,
            ConfigField::Bloom => self.config.effects.bloom,
            ConfigField::BurnIn => self.config.effects.burn_in,
            ConfigField::StaticNoise => self.config.effects.static_noise / 0.5,
            ConfigField::Flicker => self.config.effects.flicker / 0.5,
            ConfigField::Vignette => self.config.effects.vignette,
            ConfigField::Brightness => (self.config.effects.brightness - 0.1) / 1.9,
            ConfigField::FocusGlowRadius => self.config.effects.focus_glow_radius / 0.3,
            ConfigField::FocusGlowWidth => (self.config.effects.focus_glow_width - 0.001) / 0.299,
            ConfigField::FocusGlowIntensity => self.config.effects.focus_glow_intensity,
            ConfigField::ContentScaleX => (self.config.effects.content_scale_x - 0.8) / 0.4, // 0.8 to 1.2 range
            ConfigField::ContentScaleY => (self.config.effects.content_scale_y - 0.8) / 0.4, // 0.8 to 1.2 range
            ConfigField::FontSize => (self.config.font_size - 8.0) / 24.0, // 8-32 range
            _ => 0.0,
        }
    }

    /// Calculate panel height - fixed across all tabs for consistent UI
    fn panel_height(&self) -> usize {
        // Find max height across all tabs
        // Use a "maximal" config to get the maximum possible field count
        let mut max_rows = 0;
        for tab in ConfigTab::all() {
            let fields = ConfigField::fields_for_tab(*tab, &self.config);
            let mut rows = 0;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 && field.has_separator_before() {
                    rows += 1; // separator line
                }
                rows += 1; // field line
            }
            max_rows = max_rows.max(rows);
        }
        // Add extra space since TTF vs BDF modes have different field counts
        // This keeps the panel a consistent size
        max_rows = max_rows.max(6); // Minimum height for Appearance tab
                                    // Add: top border (1) + tab bar (1) + padding (1) + content rows + bottom border (1)
        4 + max_rows
    }

    /// Render the config UI overlay
    /// Returns cells to be rendered at (row, col) with the given offsets
    pub fn render(&self, width_cells: usize, height_cells: usize) -> Vec<Vec<RenderCell>> {
        let panel_width = 44;
        let panel_height = self.panel_height();

        // Center the panel
        let start_col = (width_cells.saturating_sub(panel_width)) / 2;
        let start_row = (height_cells.saturating_sub(panel_height)) / 2;

        let mut rows: Vec<Vec<RenderCell>> = Vec::with_capacity(height_cells);

        for row in 0..height_cells {
            let mut cells: Vec<RenderCell> = Vec::with_capacity(width_cells);

            for col in 0..width_cells {
                let in_panel = col >= start_col
                    && col < start_col + panel_width
                    && row >= start_row
                    && row < start_row + panel_height;

                if !in_panel {
                    // Outside panel - transparent (no background drawn)
                    cells.push(RenderCell {
                        c: ' ',
                        fg: [0.0; 4],
                        bg: [0.0, 0.0, 0.0, 0.0],
                        is_wide: false,
                    });
                    continue;
                }

                let panel_col = col - start_col;
                let panel_row = row - start_row;

                let (c, fg, bg) =
                    self.render_panel_cell(panel_col, panel_row, panel_width, panel_height);
                cells.push(RenderCell {
                    c,
                    fg,
                    bg,
                    is_wide: false,
                });
            }

            rows.push(cells);
        }

        rows
    }

    fn render_panel_cell(
        &self,
        col: usize,
        row: usize,
        width: usize,
        height: usize,
    ) -> (char, [f32; 4], [f32; 4]) {
        let last_row = height - 1;
        let fg = self.fg_color();
        let bright = self.bright_color();
        let border = self.border_color();
        let bg = self.bg_color();

        // Top border
        if row == 0 {
            if col == 0 {
                return ('┌', border, bg);
            } else if col == width - 1 {
                return ('┐', border, bg);
            } else {
                let title = " Settings ";
                let title_start = (width - title.len()) / 2;
                if col >= title_start && col < title_start + title.len() {
                    let c = title.chars().nth(col - title_start).unwrap_or('─');
                    return (c, bright, bg);
                }
            }
            return ('─', border, bg);
        }

        // Bottom border
        if row == last_row {
            if col == 0 {
                return ('└', border, bg);
            } else if col == width - 1 {
                return ('┘', border, bg);
            }
            return ('─', border, bg);
        }

        // Side borders
        if col == 0 || col == width - 1 {
            return ('│', border, bg);
        }

        // Tab bar (row 1)
        if row == 1 {
            return self.render_tab_bar_cell(col - 1, width - 2);
        }

        // Empty row after tabs (row 2)
        if row == 2 {
            return (' ', fg, bg);
        }

        // Content area (row 3+)
        // Left inner margin (col 1) - return space
        if col == 1 {
            return (' ', fg, bg);
        }
        let content_col = col - 2;
        let content_row = row - 3;

        if content_col >= width - 4 {
            return (' ', fg, bg);
        }

        let fields = self.current_fields();

        // Calculate field index, accounting for separator lines
        let mut field_idx = 0;
        let mut display_row = 0;

        while field_idx < fields.len() && display_row < content_row {
            display_row += 1;
            if display_row <= content_row {
                // Check if next field has separator before it
                if field_idx + 1 < fields.len() && fields[field_idx + 1].has_separator_before() {
                    if display_row == content_row {
                        // This row is the separator
                        return (' ', fg, bg);
                    }
                    display_row += 1;
                }
                field_idx += 1;
            }
        }

        if field_idx < fields.len() && display_row == content_row {
            let field = fields[field_idx];
            let is_selected = field_idx == self.selected;

            let line = self.format_field_line(field, width - 6, is_selected);
            if content_col < line.len() {
                let c = line.chars().nth(content_col).unwrap_or(' ');
                let text_fg = if is_selected { bright } else { fg };
                let text_bg = if is_selected { self.highlight_bg() } else { bg };
                return (c, text_fg, text_bg);
            }
        }

        (' ', fg, bg)
    }

    fn render_tab_bar_cell(&self, col: usize, width: usize) -> (char, [f32; 4], [f32; 4]) {
        // Build tab bar string: " [1:Effects] [2:Appearance] "
        let tabs = ConfigTab::all();
        let mut bar = String::new();

        let fg = self.fg_color();
        let bright = self.bright_color();
        let dim = self.dim_color();
        let bg = self.bg_color();

        for (i, tab) in tabs.iter().enumerate() {
            if i > 0 {
                bar.push_str("  ");
            }
            bar.push_str(&format!("[{}:{}]", i + 1, tab.label()));
        }

        // Center the tab bar
        let padding = (width.saturating_sub(bar.len())) / 2;

        if col < padding || col >= padding + bar.len() {
            return (' ', fg, bg);
        }

        let bar_col = col - padding;
        let c = bar.chars().nth(bar_col).unwrap_or(' ');

        // Determine if this character is within the current tab's label
        let mut pos = 0;
        for (i, tab) in tabs.iter().enumerate() {
            if i > 0 {
                pos += 2; // spacing
            }
            let tab_label = format!("[{}:{}]", i + 1, tab.label());
            let tab_end = pos + tab_label.len();

            if bar_col >= pos && bar_col < tab_end {
                // This column is within this tab
                let is_current = *tab == self.current_tab;
                let tab_fg = if is_current { bright } else { dim };
                return (c, tab_fg, bg);
            }
            pos = tab_end;
        }

        (c, fg, bg)
    }

    fn format_field_line(&self, field: ConfigField, _width: usize, selected: bool) -> String {
        let label = field.label();

        if field.is_slider() {
            let value = self.get_field_value(field);
            let bar_width = 12;
            let filled = ((value * bar_width as f32).round() as usize).min(bar_width);
            let empty = bar_width - filled;

            let bar = format!("[{}{}]", "=".repeat(filled), "-".repeat(empty));

            let value_str = match field {
                ConfigField::Curvature => format!("{:.2}", self.config.effects.screen_curvature),
                ConfigField::Scanlines => format!("{:.2}", self.config.effects.scanline_intensity),
                ConfigField::Bloom => format!("{:.2}", self.config.effects.bloom),
                ConfigField::BurnIn => format!("{:.2}", self.config.effects.burn_in),
                ConfigField::StaticNoise => format!("{:.2}", self.config.effects.static_noise),
                ConfigField::Flicker => format!("{:.2}", self.config.effects.flicker),
                ConfigField::Vignette => format!("{:.2}", self.config.effects.vignette),
                ConfigField::Brightness => format!("{:.2}", self.config.effects.brightness),
                ConfigField::FocusGlowRadius => {
                    format!("{:.4}", self.config.effects.focus_glow_radius)
                }
                ConfigField::FocusGlowWidth => {
                    format!("{:.4}", self.config.effects.focus_glow_width)
                }
                ConfigField::FocusGlowIntensity => {
                    format!("{:.2}", self.config.effects.focus_glow_intensity)
                }
                ConfigField::FontSize => format!("{:.0}px", self.config.font_size),
                _ => String::new(),
            };

            let prefix = if selected { "> " } else { "  " };
            format!("{}{:12} {} {}", prefix, label, bar, value_str)
        } else if field.is_selector() {
            let value_name = match field {
                ConfigField::FontFamily => self.config.font.label().to_string(),
                ConfigField::BdfFontFamily => self
                    .config
                    .bdf_font
                    .map(|f| f.label())
                    .unwrap_or("?")
                    .to_string(),
                ConfigField::ColorSchemeField => self.config.color_scheme.name.clone(),
                _ => "?".to_string(),
            };
            let prefix = if selected { "> " } else { "  " };
            format!("{}{:12} < {:^13} >", prefix, label, value_name)
        } else if field.is_toggle() {
            // FontType is special - shows TTF/BDF instead of ON/OFF, same width as selectors
            if field == ConfigField::FontType {
                let type_name = if self.config.bdf_font.is_some() {
                    "BDF"
                } else {
                    "TTF"
                };
                let prefix = if selected { "> " } else { "  " };
                return format!("{}{:12} < {:^13} >", prefix, label, type_name);
            }
            // ScanlineMode shows Row/Pixel instead of ON/OFF
            if field == ConfigField::ScanlineMode {
                let mode_name = match self.config.effects.scanline_mode {
                    ScanlineMode::RowBased => "Row",
                    ScanlineMode::Pixel => "Pixel",
                };
                let prefix = if selected { "> " } else { "  " };
                return format!("{}{:12} < {:^13} >", prefix, label, mode_name);
            }
            // BeamSimulation shows warning when ON
            if field == ConfigField::BeamSimulation {
                let prefix = if selected { "> " } else { "  " };
                if self.config.effects.beam_simulation_enabled {
                    return format!("{}{:12} [ON ] 240Hz+ REQ!", prefix, label);
                } else {
                    return format!("{}{:12} [OFF]", prefix, label);
                }
            }
            let is_on = match field {
                ConfigField::PerPaneCrt => self.config.per_pane_crt,
                ConfigField::BezelEnabled => self.config.effects.bezel_enabled,
                ConfigField::AutoCopySelection => self.config.behavior.auto_copy_selection,
                ConfigField::ShowStartupHint => self.config.behavior.show_startup_hint,
                ConfigField::Interlace => self.config.effects.interlace_enabled,
                _ => false,
            };
            let state = if is_on { "[ON ]" } else { "[OFF]" };
            let prefix = if selected { "> " } else { "  " };
            format!("{}{:12} {}", prefix, label, state)
        } else if field.is_button() {
            let prefix = if selected { "> " } else { "  " };
            format!("{}{}", prefix, label)
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    Save,
    Cancel,
}
