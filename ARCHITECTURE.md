# cool-rust-term Architecture

## Overview

A GPU-accelerated terminal emulator with CRT visual effects and tiling/split support.

## Core Components

```
┌─────────────────────────────────────────────────────────────────┐
│                         Application                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   Config    │  │   Input     │  │      Window/Event       │  │
│  │   System    │  │   Router    │  │        (winit)          │  │
│  └─────────────┘  └──────┬──────┘  └─────────────────────────┘  │
└──────────────────────────┼──────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────────┐
│                       Layout Manager                             │
│                                                                  │
│   Automatic grid layout. Arranges N panes in near-square grid.  │
│   Adapts to window aspect ratio (landscape vs portrait).        │
│                                                                  │
│   Example: 5 panes in landscape (1/2/2 columns)                 │
│   ┌───────┬───────┬───────┐                                     │
│   │       │   2   │   4   │                                     │
│   │   1   ├───────┼───────┤                                     │
│   │       │   3   │   5   │                                     │
│   └───────┴───────┴───────┘                                     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
                           │
                           │ Each pane owns:
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Terminal Pane                               │
│  ┌──────────────────┐  ┌──────────────────────────────────────┐ │
│  │  PTY + Process   │  │     alacritty_terminal::Term         │ │
│  │                  │◄─┤     (escape parsing, grid state)     │ │
│  └──────────────────┘  └──────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                           │
                           │ Grid of cells (char + style)
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Renderer                                  │
│                                                                  │
│  ┌─────────────────┐      ┌─────────────────────────────────┐   │
│  │   Glyph Atlas   │      │      Per-Pane Render Target     │   │
│  │  (font raster)  │─────►│  1. Render text to texture      │   │
│  └─────────────────┘      │  2. Apply CRT shader chain      │   │
│                           │  3. Output to pane region       │   │
│                           └─────────────────────────────────┘   │
│                                                                  │
│  CRT Shader Chain (per pane):                                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │  Text    │─►│  Static  │─►│ Dynamic  │─►│ Burn-in  │─► Out  │
│  │ Texture  │  │ Effects  │  │ Effects  │  │ Feedback │        │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘        │
│                                                                  │
│  Static: curvature, bloom, RGB shift, colorization             │
│  Dynamic: scanlines, noise, flicker, h-sync jitter             │
│  Burn-in: recursive framebuffer for phosphor decay             │
└─────────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Final Compositor                             │
│                                                                  │
│  Composites all pane outputs into final framebuffer.            │
│  Draws pane borders/separators if configured.                   │
└─────────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. Automatic Grid Layout

```rust
struct LayoutTree {
    panes: Vec<PaneId>,
    focused: PaneId,
    next_id: u64,
}
```

The layout automatically arranges panes in a near-square grid:
- Number of columns = `ceil(sqrt(n))`
- Extra panes fill the last columns
- Landscape windows: columns side-by-side
- Portrait windows: rows stacked

Operations:
- `add_pane()` - adds a new pane, gets focus
- `close(pane_id)` - removes pane, grid reflows
- `pane_rects(width, height)` - calculates layout rectangles

### 2. Per-Pane vs Shared Rendering

**Implemented: Toggleable per-pane CRT effects (Ctrl+Shift+P)**

Two modes available:
- **Whole-screen mode** (default): All panes render to a single offscreen texture,
  CRT effects applied to the whole thing. Amber separator lines between panes.
- **Per-pane mode**: Each pane gets independent CRT effects (barrel distortion,
  vignette, scanlines, flicker). The CRT shader transforms UV coordinates to
  local pane space, making each pane look like its own "monitor".

### 3. Burn-in Implementation

The burn-in effect requires feeding the previous frame back into the shader.

```
Frame N:
  1. Read burn-in texture from frame N-1
  2. Blend current text with decayed burn-in
  3. Write result to burn-in texture for frame N+1
  4. Output to screen
```

Needs ping-pong buffers (two textures, alternate read/write each frame).

### 4. Text Rendering Pipeline

Using a glyph atlas approach (standard for GPU text):

1. Rasterize glyphs on-demand using a font library (fontdue, ab_glyph)
2. Pack into atlas texture
3. Render quads with atlas UV coordinates
4. Cell background colors rendered as solid quads behind text

### 5. Input Handling

```
Keyboard → focused pane's PTY
Mouse → hit-test layout tree → target pane
  - Click: focus pane
  - Drag: text selection (auto-copy on release)
```

**Keyboard Shortcuts:**
- `Ctrl+Shift+Enter` - Add new pane
- `Ctrl+,` or `Ctrl+Shift+P` - Toggle config UI overlay
- `Ctrl+Shift+G` - Toggle debug grid (shows cell boundaries)
- `Ctrl+Shift+C` - Copy selection
- `Ctrl+Shift+V` - Paste
- `Shift+PageUp` / `Shift+PageDown` - Scroll history
- Mouse wheel - Scroll history
- Click on pane - Focus that pane

## Crate Structure

```
cool-rust-term/
├── Cargo.toml              # Workspace
├── crates/
│   ├── crt-core/           # Shared types, config
│   ├── crt-terminal/       # PTY + alacritty_terminal wrapper
│   ├── crt-layout/         # Layout tree, split management
│   ├── crt-renderer/       # wgpu rendering, shaders, glyph atlas
│   └── crt-app/            # Main binary, window, input, glue
└── shaders/
    ├── text.wgsl           # Text/glyph rendering
    ├── crt.wgsl            # CRT effects (curvature, scanlines, bloom, vignette, noise, focus glow)
    ├── burnin.wgsl         # Phosphor burn-in with ping-pong buffers
    └── line.wgsl           # Solid-color line rendering (separators, focus borders, debug grid)
```

## Decisions

1. **Config format**: TOML

2. **Font handling**: Use original cool-retro-term bitmap-style TTF fonts.
   Available: IBM 3278, Terminus, Apple II, IBM PC BIOS, Commodore PET,
   Fixedsys, Hermit, C64, Inconsolata, ProFont, Atari 400/800, IBM VGA, ProggyTiny

3. **Pane borders**: Amber separator lines using box drawing characters

4. **Effect presets**: Start with Amber only

5. **Scrollback**: Scrollbar + mouse wheel

## Implementation Progress

### Completed (Phase 1 + Phase 2 partial)

- [x] Cargo workspace with 5 crates (crt-core, crt-terminal, crt-layout, crt-renderer, crt-app)
- [x] winit window + event loop
- [x] wgpu device/surface initialization
- [x] Glyph atlas using fontdue (IBM VGA font, 18px)
- [x] Text rendering shader (text.wgsl)
- [x] Terminal emulation via alacritty_terminal 0.25
- [x] PTY creation and I/O
- [x] Grid rendering from terminal state
- [x] Cursor display (amber block)
- [x] Keyboard input with modifier keys (Ctrl, Alt)
- [x] App closes when shell exits
- [x] Non-ASCII character handling (collapsed to single '?')
- [x] Text selection with mouse (auto-copy to clipboard on release)
- [x] CRT post-processing shader with two-pass rendering:
  - Off-screen texture for terminal content
  - Barrel distortion (curvature: 0.03)
  - Animated scanlines with subtle drift
  - Bloom effect
  - Flicker animation
  - Static noise
  - Vignette darkening at edges
- [x] Automatic grid layout system (adapts to window aspect ratio)
- [x] Multiple panes with independent terminals
- [x] Shift+Ctrl+Enter to add new pane
- [x] Shell exit closes pane, last pane closes app
- [x] Amber separator lines between panes
- [x] Pane content padding (8px)
- [x] Click to change pane focus
- [x] Visual focus indicator (corner brackets on focused pane)
- [x] Pane size indicator during window resize (shows cols×rows)
- [x] Burn-in effect (phosphor persistence with ping-pong framebuffers)
- [x] Per-pane CRT effects (Ctrl+Shift+P to toggle)
- [x] Line pipeline for separators and focus indicators (line.wgsl)
- [x] Focus indicator: highlighted borders on focused pane (line-based, not box chars)
- [x] Per-pane focus glow in shader (configurable radius, width, intensity)
- [x] Anti-aliased CRT borders using fwidth() for smooth edges
- [x] Improved scanlines: triangle wave with fract() (like cool-retro-term)
- [x] Synchronized scanline drift (moves whole scanlines to avoid moiré)
- [x] Config UI overlay (text-based, Ctrl+, to toggle)
- [x] Live preview of settings in config UI
- [x] All CRT effects wired to config: curvature, scanlines, bloom, burn-in, static, flicker, brightness
- [x] Focus glow settings in config: radius, width, intensity
- [x] Debug grid toggle (Ctrl+Shift+G) - draws 1px lines at cell boundaries
- [x] Text selection with visual highlight (inverted colors from color scheme)
- [x] Full ANSI color support:
  - 16 named colors mapped through ColorScheme
  - 256-color palette (16 scheme + 216 color cube + 24 grayscale)
  - True color (24-bit RGB)
  - Dim, inverse video, and all SGR attributes
- [x] Font selection with 13 bundled fonts (retro + modern)
- [x] Color scheme presets (Amber, Green, White, ANSI)
- [x] Scrollback buffer (10,000 lines):
  - Mouse wheel scrolling
  - Shift+PageUp/PageDown for page scrolling
  - Scroll position indicator popup [offset/history]
  - Auto-scroll to bottom on keyboard input

### Known Issues

- Only ASCII rendering supported (non-ASCII → '?')

### Next Steps

**Phase 4 - Polish:**
- BDF bitmap font support (skip rasterizer, write directly to atlas)

### 6. Configuration System

**Implemented:** TOML-based config with load/save to `~/.config/cool-rust-term/config.toml`

```rust
// crt-core/src/config.rs
pub struct Config {
    pub effects: EffectSettings,  // CRT effect parameters
    pub font_path: Option<PathBuf>,
    pub font_size: f32,
    pub window_width: u32,
    pub window_height: u32,
    pub per_pane_crt: bool,
}

// crt-core/src/effects.rs
pub struct EffectSettings {
    pub font_color: Color,
    pub background_color: Color,
    pub screen_curvature: f32,    // 0.0-0.5
    pub scanline_intensity: f32,  // 0.0-1.0
    pub bloom: f32,               // 0.0-1.0
    pub burn_in: f32,             // 0.0-1.0
    pub static_noise: f32,        // 0.0-0.5
    pub flicker: f32,             // 0.0-0.5
    pub brightness: f32,          // 0.1-2.0
    // ... more fields
}
```

Config loading: `Config::load_or_default()` - loads from default path or returns defaults
Config saving: `config.save_to_default()` - creates directories and saves TOML

**Status:** All config values are now wired to the CRT shader via `EffectParams`:
- curvature, scanline_intensity, bloom, burn_in, static_noise, flicker, brightness
- focus_glow_radius, focus_glow_width, focus_glow_intensity
- per_pane_crt mode

## Recent Changes (Session Notes)

### Line Pipeline & Focus Indicators
- Created `line_pipeline.rs` and `line.wgsl` for solid-color line rendering
- Separators between panes now use line pipeline (not box-drawing chars)
- Focus indicator draws highlighted borders on focused pane edges
- In per-pane CRT mode, focus is shown via shader glow instead of lines

### Focus Glow (Per-Pane CRT Mode)
- Implemented in `crt.wgsl` using signed distance field (SDF) for rounded rectangle
- Uses `fwidth()` for anti-aliased edges
- Configurable: radius (corner roundness), width (fade distance), intensity
- Applied before other CRT effects so it gets scanlines/noise treatment

### Scanline Improvements
- Changed from sine wave to triangle wave using `fract()` (matches cool-retro-term)
- Synchronized drift: adds to screen_y before fract() to move whole scanlines
- This eliminates moiré patterns from time-based interference

### Anti-Aliasing
- CRT borders use `edge_mask_aa()` with `fwidth()` for smooth edges
- Replaced hard `is_outside()` check with smooth alpha falloff

### Config UI
- Text-based overlay in `config_ui.rs` (not iced - simpler approach)
- Live preview: changes apply immediately to shader
- Ctrl+, or Ctrl+Shift+P to toggle
- **Tabbed interface**: Effects tab and Appearance tab
- Tab/Shift+Tab or press 1/2 to switch tabs
- Fixed panel height across tabs for consistent UI

### Fonts
- 13 bundled fonts embedded at compile time (`crates/crt-renderer/src/fonts.rs`)
- Retro: IBM VGA, IBM BIOS, IBM 3278, Apple II, Commodore PET/64, Atari
- Modern: Terminus, Fixedsys, ProggyTiny, ProFont, Hermit, Inconsolata
- Font selector in Appearance tab with live preview
- Font size adjustable 8-32px
- `renderer.set_font()` recreates atlas when font changes

### Color Schemes & ANSI Color Support
- `ColorScheme` struct: 16 ANSI colors + foreground/background
- Presets: Amber, Green, White (monochrome), ANSI (full color)
- Monochrome schemes map all 16 colors to intensity variants of one hue
- Color selector in Appearance tab with live preview
- Full ANSI color support via `ansi_color_to_rgba()` in main.rs:
  - Reads `cell.fg` and `cell.bg` from alacritty_terminal cells
  - Maps NamedColor to scheme colors, handles dim variants
  - 256-color via `indexed_color()`: 0-15 scheme, 16-231 color cube, 232-255 grayscale
  - True color (24-bit RGB) passed through directly
  - Inverse video swaps fg/bg
- Text selection uses inverted colors from the active color scheme

### Key Files Modified
- `crates/crt-renderer/src/line_pipeline.rs` - NEW: line rendering
- `crates/crt-renderer/src/crt_pipeline.rs` - added effect uniforms
- `crates/crt-renderer/src/renderer.rs` - EffectParams, font switching, line integration
- `crates/crt-renderer/src/fonts.rs` - NEW: embedded font data
- `crates/crt-app/src/config_ui.rs` - NEW: tabbed config UI with fonts/colors
- `crates/crt-app/src/main.rs` - config UI integration, color scheme wiring
- `crates/crt-core/src/config.rs` - Font enum, ColorScheme struct
- `crates/crt-core/src/effects.rs` - focus glow settings
- `shaders/crt.wgsl` - focus glow, AA edges, improved scanlines
- `shaders/line.wgsl` - NEW: solid color lines

### Remaining Work
- Investigate remaining moiré in curved scanline areas (may need fwidth() AA)
- BDF bitmap font support (skip rasterizer, write directly to atlas)

### Future: Authentic Scanlines with BDF Fonts

The current scanline implementation uses one "virtual scanline" per text row - this is a
compromise that works with TTF fonts but isn't physically accurate to real CRTs.

**The problem with TTF + scanlines:**
- TTF fonts use anti-aliasing and sub-pixel positioning
- Horizontal strokes land at arbitrary pixel positions
- Traditional scanlines (every N pixels) cut through characters unpredictably
- This destroys readability, especially for thin strokes

**How real CRT terminals worked:**
- Bitmap fonts with fixed pixel structure (each pixel on or off)
- Character cells were N scanlines tall (e.g., 16 scanlines for a 16px cell)
- Font designers placed horizontal strokes to align WITH bright scanlines
- Dark bands fell in the natural gaps between text rows and within character whitespace

**Plan for BDF font support:**
1. Load BDF fonts directly to atlas (no rasterization, preserve exact pixel structure)
2. Add `scanlines_per_cell` config (e.g., cell_height or cell_height/2)
3. Calculate total scanlines as: `num_rows * scanlines_per_cell`
4. Align phase so bright bands hit middle of cell (where character body is)
5. Optionally: analyze font to find common stroke rows and optimize alignment

**Result:** When resizing the window, you get more/fewer text rows, but each character
maintains the same scanline relationship. The phosphor pitch is effectively fixed,
just like real CRT hardware.

For TTF fonts, keep the current row-aligned approximation (or offer reduced-intensity
pixel-based scanlines as an option).

## Technical Notes

### Terminal Grid Reading

The terminal grid is read via `alacritty_terminal::Grid`. Key points:
- `Line(0)` is top of visible area
- Wide chars have `WIDE_CHAR_SPACER` flag on second cell
- Cursor position from `grid.cursor.point`

### Character Filtering

Non-ASCII characters and wide char spacers are collapsed - each non-ASCII run
becomes a single '?', and wide char spacers are skipped entirely:
```rust
if is_wide_spacer {
    continue;  // Skip spacer cells entirely
} else if is_non_ascii {
    if in_run { continue; }  // Skip consecutive non-ASCII
    c = '?';
    in_run = true;
} else {
    in_run = false;
}
```

### CRT Effect Pipeline

```
Terminal Grid → Text Texture → Burn-in → CRT Shader → Screen
                                   ↓           ↓
                            Ping-pong    - Barrel distortion
                            buffers      - Scanlines (triangle wave)
                                         - Bloom/glow
                                         - Static noise
                                         - Flicker
                                         - Vignette
                                         - Focus glow (per-pane mode)
```
